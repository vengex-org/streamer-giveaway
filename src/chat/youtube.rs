//! YouTube live chat, read without an API key the way youtube.com's own chat window does: the
//! channel's `/live` page names the live video, the chat page gives the client key and the
//! "Live chat" view (not the filtered "Top chat"), and `get_live_chat` is polled for new messages.
//! None of this is a documented API, so every answer is parsed defensively.

use std::time::Duration;

use serde_json::Value;

use super::{BACKOFF, ChatEvent, ChatTarget, ConnectionStatus, FailReason, USER_AGENT};
use crate::domain::badges::BadgeUrl;
use crate::domain::chat::{ChatMessage, YouTubeBadge};
use crate::domain::platform::{Platform, YouTubeSource};

/// Skips YouTube's cookie consent page, which otherwise replaces every page in the EU.
const CONSENT_COOKIE: &str = "SOCS=CAI";

/// YouTube suggests a wait before each poll (about 5 s in the 2026-11-19 probes). The bounds keep
/// a broken value from polling in a tight loop or stalling the giveaway.
const MIN_POLL: Duration = Duration::from_secs(1);
const MAX_POLL: Duration = Duration::from_secs(10);

/// The page that shows a channel's current live stream, or `None` for a video id.
fn live_page_url(source: &YouTubeSource) -> Option<String> {
    match source {
        YouTubeSource::Video(_) => None,
        YouTubeSource::Handle(handle) => Some(format!("https://www.youtube.com/@{handle}/live")),
        YouTubeSource::Channel(id) => Some(format!("https://www.youtube.com/channel/{id}/live")),
    }
}

/// The live video on a channel's `/live` page. A channel that is not live gets its channel page
/// there instead, with no watch link.
fn find_live_video(html: &str) -> Result<String, FailReason> {
    const CANONICAL: &str = "<link rel=\"canonical\" href=\"https://www.youtube.com/watch?v=";
    let Some(start) = html.find(CANONICAL) else {
        return Err(FailReason::Offline);
    };
    let id: String = html[start + CANONICAL.len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if id.len() != 11 {
        return Err(FailReason::Protocol(
            "live page: unreadable video link".into(),
        ));
    }
    if !html.contains("\"isLiveNow\":true") {
        return Err(FailReason::Offline);
    }
    Ok(id)
}

/// What polling one stream's chat needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatSetup {
    api_key: String,
    client_version: String,
    /// Starts the "Live chat" view, which shows every message.
    continuation: String,
}

fn chat_page_url(video: &str) -> String {
    format!("https://www.youtube.com/live_chat?is_popout=1&hl=en&v={video}")
}

/// Reads the client key and the "Live chat" continuation from the chat page. A video without a
/// live chat (not live, ended, chat off) has no chat renderer.
fn chat_setup(html: &str) -> Result<ChatSetup, FailReason> {
    let protocol = |what: &str| FailReason::Protocol(format!("chat page: {what}"));
    let api_key = quoted_value(html, "INNERTUBE_API_KEY").ok_or_else(|| protocol("no key"))?;
    let client_version = quoted_value(html, "INNERTUBE_CLIENT_VERSION")
        .ok_or_else(|| protocol("no client version"))?;
    let data = initial_data(html).ok_or_else(|| protocol("no initial data"))?;
    let Some(chat) = data.pointer("/contents/liveChatRenderer") else {
        return Err(FailReason::Offline);
    };
    let views = chat
        .pointer(
            "/header/liveChatHeaderRenderer/viewSelector/sortFilterSubMenuRenderer/subMenuItems",
        )
        .and_then(Value::as_array)
        .ok_or_else(|| protocol("no chat views"))?;
    // "Live chat" by its title (the page is requested in English), else the view not selected
    // by default: "Top chat" is the default and hides part of the messages.
    let live_view = views
        .iter()
        .find(|view| view["title"] == "Live chat")
        .or_else(|| views.iter().find(|view| view["selected"] == false))
        .ok_or_else(|| protocol("no live chat view"))?;
    let continuation = live_view
        .pointer("/continuation/reloadContinuationData/continuation")
        .and_then(Value::as_str)
        .ok_or_else(|| protocol("no live chat continuation"))?;
    Ok(ChatSetup {
        api_key,
        client_version,
        continuation: continuation.to_owned(),
    })
}

/// `"NAME":"value"` from the page's inline config.
fn quoted_value(html: &str, name: &str) -> Option<String> {
    let marker = format!("\"{name}\":\"");
    let start = html.find(&marker)? + marker.len();
    let value: String = html[start..].chars().take_while(|c| *c != '"').collect();
    (!value.is_empty()).then_some(value)
}

/// The `ytInitialData` object embedded in the page.
fn initial_data(html: &str) -> Option<Value> {
    let start = ["window[\"ytInitialData\"] = ", "var ytInitialData = "]
        .iter()
        .find_map(|marker| html.find(marker).map(|at| at + marker.len()))?;
    serde_json::Deserializer::from_str(&html[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
}

/// One `get_live_chat` answer.
#[derive(Debug, Default)]
struct ChatPage {
    /// Messages with their YouTube timestamp in microseconds.
    messages: Vec<(u64, ChatMessage)>,
    /// The next continuation and how long to wait for it; `None` once the stream has ended.
    next: Option<(String, Duration)>,
    /// Actions that were not chat messages or could not be read, by kind, for the log.
    skipped: Vec<String>,
}

fn parse_chat(json: &str) -> Result<ChatPage, FailReason> {
    let answer: Value = serde_json::from_str(json)
        .map_err(|err| FailReason::Protocol(format!("chat answer: {err}")))?;
    let Some(chat) = answer.pointer("/continuationContents/liveChatContinuation") else {
        // YouTube drops the chat object when the stream is over.
        return Ok(ChatPage::default());
    };
    let next = chat["continuations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flat_map(|continuation| continuation.values())
        .find_map(|data| {
            let token = data["continuation"].as_str()?;
            let wait = data["timeoutMs"].as_u64().unwrap_or(5000);
            Some((token.to_owned(), Duration::from_millis(wait)))
        });
    let mut page = ChatPage {
        next,
        ..ChatPage::default()
    };
    for action in chat["actions"].as_array().into_iter().flatten() {
        let item = action.pointer("/addChatItemAction/item");
        let renderer = item.and_then(|item| {
            item.get("liveChatTextMessageRenderer")
                .or_else(|| item.get("liveChatPaidMessageRenderer"))
        });
        match renderer.and_then(chat_message) {
            Some(message) => page.messages.push(message),
            None => page.skipped.push(action_kind(action)),
        }
    }
    Ok(page)
}

/// The action's own key and, for added items, the item's renderer, e.g.
/// `addChatItemAction/liveChatMembershipItemRenderer`.
fn action_kind(action: &Value) -> String {
    let Some(object) = action.as_object() else {
        return "not an object".into();
    };
    let kind = object
        .keys()
        .find(|key| *key != "clickTrackingParams")
        .map_or("empty", String::as_str);
    let renderer = action
        .pointer("/addChatItemAction/item")
        .and_then(Value::as_object)
        .and_then(|item| item.keys().next());
    match renderer {
        Some(renderer) => format!("{kind}/{renderer}"),
        None => kind.to_owned(),
    }
}

fn chat_message(renderer: &Value) -> Option<(u64, ChatMessage)> {
    let channel_id = renderer["authorExternalChannelId"].as_str()?;
    let name = renderer.pointer("/authorName/simpleText")?.as_str()?;
    let timestamp = renderer["timestampUsec"].as_str()?.parse().ok()?;
    let text = renderer
        .pointer("/message/runs")
        .and_then(Value::as_array)
        .map(|runs| runs.iter().map(run_text).collect::<String>())
        .unwrap_or_default();
    let badges: Vec<YouTubeBadge> = renderer["authorBadges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|badge| author_badge(&badge["liveChatAuthorBadgeRenderer"]))
        .collect();
    Some((
        timestamp,
        ChatMessage::youtube(channel_id, name, text, &badges),
    ))
}

/// Text as chat sees it: plain runs as they are, emoji as the character, channel emoji by their
/// `:name:` shortcut.
fn run_text(run: &Value) -> String {
    if let Some(text) = run["text"].as_str() {
        return text.to_owned();
    }
    let emoji = &run["emoji"];
    let custom = emoji["isCustomEmoji"].as_bool().unwrap_or(false);
    let shortcut = emoji["shortcuts"][0].as_str();
    let id = emoji["emojiId"].as_str();
    match (custom, shortcut, id) {
        (true, Some(shortcut), _) | (false, Some(shortcut), None) => shortcut.to_owned(),
        (_, _, Some(id)) => id.to_owned(),
        _ => String::new(),
    }
}

fn author_badge(badge: &Value) -> Option<YouTubeBadge> {
    if let Some(thumbnails) = badge
        .pointer("/customThumbnail/thumbnails")
        .and_then(Value::as_array)
    {
        let picture = thumbnails
            .last()
            .and_then(|thumbnail| thumbnail["url"].as_str())
            .and_then(BadgeUrl::parse);
        return Some(YouTubeBadge::Member(picture));
    }
    match badge.pointer("/icon/iconType")?.as_str()? {
        "OWNER" => Some(YouTubeBadge::Owner),
        "MODERATOR" => Some(YouTubeBadge::Moderator),
        _ => None,
    }
}

/// Why a connection attempt ended.
enum Stop {
    UiGone,
    Fail(FailReason),
    Retry(String),
}

/// Reads one stream's chat until the task is aborted, the UI is gone, or the stream is over.
/// Network and server errors are retried with [`BACKOFF`].
pub(super) async fn run(source: YouTubeSource, events: async_channel::Sender<ChatEvent>) {
    let target = ChatTarget::YouTube(source.clone());
    let send = |status: ConnectionStatus| {
        let events = events.clone();
        async move {
            events
                .send(ChatEvent::Status(Platform::YouTube, status))
                .await
                .is_ok()
        }
    };
    let client = match gpui_pre_reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            let reason = FailReason::Protocol(format!("http client: {err}"));
            send(ConnectionStatus::Failed { target, reason }).await;
            return;
        }
    };
    let mut attempt = 0;
    let mut cause: Option<String> = None;
    let mut was_connected = false;
    // Newest message timestamp handed on. The first chat page repeats recent history; only what
    // is newer counts, so a reconnect fills the gap without replaying old messages.
    let mut seen_until: Option<u64> = None;
    loop {
        let status = if was_connected {
            ConnectionStatus::Reconnecting {
                target: target.clone(),
                cause: cause.clone(),
            }
        } else {
            ConnectionStatus::Connecting {
                target: target.clone(),
                cause: cause.clone(),
            }
        };
        if !send(status).await {
            return;
        }
        let stop = session(
            &client,
            &source,
            &target,
            &events,
            &mut was_connected,
            &mut seen_until,
            &mut attempt,
        )
        .await;
        match stop {
            Stop::UiGone => return,
            Stop::Fail(reason) => {
                tracing::warn!(?reason, "youtube chat stopped");
                send(ConnectionStatus::Failed { target, reason }).await;
                return;
            }
            Stop::Retry(why) => {
                tracing::info!(why, "youtube chat reconnecting");
                tokio::time::sleep(BACKOFF[attempt.min(BACKOFF.len() - 1)]).await;
                attempt += 1;
                cause = Some(why);
            }
        }
    }
}

fn http_stop(what: &str, status: gpui_pre_reqwest::StatusCode) -> Stop {
    match status.as_u16() {
        404 => Stop::Fail(FailReason::NotFound),
        401 | 403 => Stop::Fail(FailReason::Blocked),
        429 | 500..=599 => Stop::Retry(format!("{what}: HTTP {status}")),
        _ => Stop::Fail(FailReason::Protocol(format!("{what}: HTTP {status}"))),
    }
}

async fn text_of(request: gpui_pre_reqwest::RequestBuilder, what: &str) -> Result<String, Stop> {
    let response = request
        .header("Cookie", CONSENT_COOKIE)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|err| Stop::Retry(format!("{what}: {err}")))?;
    if !response.status().is_success() {
        return Err(http_stop(what, response.status()));
    }
    response
        .text()
        .await
        .map_err(|err| Stop::Retry(format!("{what}: {err}")))
}

async fn session(
    client: &gpui_pre_reqwest::Client,
    source: &YouTubeSource,
    target: &ChatTarget,
    events: &async_channel::Sender<ChatEvent>,
    was_connected: &mut bool,
    seen_until: &mut Option<u64>,
    attempt: &mut usize,
) -> Stop {
    let video = match (source, live_page_url(source)) {
        (YouTubeSource::Video(id), _) => id.clone(),
        (_, Some(url)) => match text_of(client.get(url), "live page").await {
            Ok(html) => match find_live_video(&html) {
                Ok(id) => id,
                Err(reason) => return Stop::Fail(reason),
            },
            Err(stop) => return stop,
        },
        (_, None) => unreachable!("only a video id has no live page"),
    };
    let setup = match text_of(client.get(chat_page_url(&video)), "chat page").await {
        Ok(html) => match chat_setup(&html) {
            Ok(setup) => setup,
            Err(reason) => return Stop::Fail(reason),
        },
        Err(stop) => return stop,
    };
    let url = format!(
        "https://www.youtube.com/youtubei/v1/live_chat/get_live_chat?key={}&prettyPrint=false",
        setup.api_key
    );
    let mut continuation = setup.continuation;
    loop {
        let body = serde_json::json!({
            "context": { "client": {
                "clientName": "WEB",
                "clientVersion": setup.client_version,
                "hl": "en",
            } },
            "continuation": continuation,
        });
        let request = client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body.to_string());
        let page = match text_of(request, "chat").await {
            Ok(json) => match parse_chat(&json) {
                Ok(page) => page,
                Err(reason) => return Stop::Fail(reason),
            },
            Err(stop) => return stop,
        };
        if !*was_connected || *attempt > 0 {
            *was_connected = true;
            *attempt = 0;
            let connected = ConnectionStatus::Connected(target.clone());
            if events
                .send(ChatEvent::Status(Platform::YouTube, connected))
                .await
                .is_err()
            {
                return Stop::UiGone;
            }
        }
        if !page.skipped.is_empty() {
            tracing::debug!(skipped = page.skipped.len(), first = %page.skipped[0], "youtube actions skipped");
        }
        let newest = page.messages.iter().map(|(at, _)| *at).max();
        if let Some(since) = *seen_until {
            for (at, message) in page.messages {
                if at > since && events.send(ChatEvent::Message(message)).await.is_err() {
                    return Stop::UiGone;
                }
            }
        }
        *seen_until = (*seen_until).max(newest).or(Some(0));
        let Some((next, wait)) = page.next else {
            return Stop::Fail(FailReason::Offline);
        };
        continuation = next;
        tokio::time::sleep(wait.clamp(MIN_POLL, MAX_POLL)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chat::SubBadge;
    use crate::domain::role::Role;

    const CHAT_PAGE: &str = include_str!("../../tests/fixtures/youtube/live_chat.html");
    const CHAT_ANSWER: &str = include_str!("../../tests/fixtures/youtube/get_live_chat.json");

    #[test]
    fn a_live_channel_page_names_its_video() {
        let html = "<link rel=\"canonical\" href=\"https://www.youtube.com/watch?v=qSyo5d-dVzA\">\
            ... \"isLiveNow\":true ...";
        assert_eq!(find_live_video(html), Ok("qSyo5d-dVzA".into()));
    }

    #[test]
    fn a_channel_that_is_not_live_is_offline() {
        let channel_page = "<link rel=\"canonical\" href=\"https://www.youtube.com/channel/UCXuqSBlHAE6Xw-yeJA0Tunw\">";
        assert_eq!(find_live_video(channel_page), Err(FailReason::Offline));
        let upcoming = "<link rel=\"canonical\" href=\"https://www.youtube.com/watch?v=qSyo5d-dVzA\">\
            \"isLiveNow\":false";
        assert_eq!(find_live_video(upcoming), Err(FailReason::Offline));
    }

    #[test]
    fn the_chat_page_gives_the_live_chat_view_not_top_chat() {
        let setup = chat_setup(CHAT_PAGE).unwrap();
        assert_eq!(setup.api_key, "AIzaTESTKEY");
        assert_eq!(setup.client_version, "2.20261118.01.00");
        assert_eq!(setup.continuation, "LIVE_CHAT_CONTINUATION");
    }

    #[test]
    fn a_video_without_live_chat_is_offline() {
        let html = "\"INNERTUBE_API_KEY\":\"k\",\"INNERTUBE_CLIENT_VERSION\":\"v\" \
            <script>var ytInitialData = {\"contents\":{\"messageRenderer\":{}}};</script>";
        assert_eq!(chat_setup(html), Err(FailReason::Offline));
    }

    #[test]
    fn a_chat_page_without_its_config_is_a_protocol_error() {
        assert!(matches!(
            chat_setup("<html></html>"),
            Err(FailReason::Protocol(_))
        ));
    }

    #[test]
    fn recorded_answer_gives_messages_roles_badges_and_the_next_poll() {
        let page = parse_chat(CHAT_ANSWER).unwrap();
        let messages: Vec<&ChatMessage> = page.messages.iter().map(|(_, m)| m).collect();
        assert_eq!(messages.len(), 5);
        assert_eq!(page.skipped, ["addLiveChatTickerItemAction"]);
        assert_eq!(
            page.next,
            Some(("NEXT_CONTINUATION".into(), Duration::from_millis(4778)))
        );

        let member = messages[0];
        assert_eq!(member.platform(), Platform::YouTube);
        assert_eq!(member.user().id, "UCmember0000000000000001");
        assert_eq!(member.display_name(), "@member_viewer");
        assert_eq!(member.text(), "GIVEME");
        assert_eq!(member.roles(), [Role::SubT1]);
        let Some(SubBadge::YouTube(picture)) = member.sub_badge() else {
            panic!("member badge picture missing");
        };
        assert_eq!(
            picture.as_str(),
            "https://yt3.ggpht.com/member-badge=s32-c-k"
        );

        assert_eq!(messages[1].roles(), [Role::Pleb]);
        assert_eq!(messages[2].text(), "\u{1F44B} GIVEME");
        assert_eq!(messages[3].roles(), [Role::Mod]);
        assert!(messages[4].is_owner());
    }

    #[test]
    fn an_answer_without_chat_means_the_stream_ended() {
        let page = parse_chat("{\"responseContext\":{}}").unwrap();
        assert!(page.next.is_none() && page.messages.is_empty());
    }

    #[test]
    fn channel_emoji_read_as_their_shortcut() {
        let run = serde_json::json!({ "emoji": {
            "emojiId": "UCx/abc", "shortcuts": [":hype:"], "isCustomEmoji": true
        } });
        assert_eq!(run_text(&run), ":hype:");
    }

    #[test]
    fn a_member_picture_off_youtube_is_dropped_but_the_role_stays() {
        let badge = serde_json::json!({ "customThumbnail": { "thumbnails": [
            { "url": "https://evil.example/badge.png" }
        ] } });
        assert_eq!(author_badge(&badge), Some(YouTubeBadge::Member(None)));
    }

    #[test]
    fn live_pages_are_found_for_handles_and_channels() {
        let handle = YouTubeSource::parse("@IShowSpeed").unwrap();
        assert_eq!(
            live_page_url(&handle).as_deref(),
            Some("https://www.youtube.com/@IShowSpeed/live")
        );
        let video = YouTubeSource::parse("youtu.be/qSyo5d-dVzA").unwrap();
        assert_eq!(live_page_url(&video), None);
    }
}
