//! Kick chat, read without login the way kick.com's own page does: the channel API gives the
//! chatroom id, a guest token opens Kick's Centrifugo socket, and the chatroom topic carries the
//! messages. None of this is a documented API, so every answer is parsed defensively.

use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::{self, Message};

use super::{BACKOFF, ChatEvent, ChatTarget, ConnectionStatus, FailReason, USER_AGENT};
use crate::domain::chat::{ChatMessage, KickBadge};
use crate::domain::platform::{KickChannel, Platform};

const ORIGIN: &str = "https://kick.com";
const TOKEN_URL: &str = "https://web.kick.com/api/v1/realtime/auth/connection";
const SOCKET_URL: &str = "wss://realtime.us-west-2.platform.kick.com/connection/websocket";

/// Guest tokens lived 2699-2700 s in the 2026-11-19 probes; reconnecting a little earlier with a
/// fresh one avoids being cut off mid-giveaway.
const TOKEN_REFRESH_AFTER: Duration = Duration::from_secs(2600);

pub fn channel_url(channel: &KickChannel) -> String {
    format!("https://kick.com/api/v2/channels/{}", channel.as_str())
}

#[derive(Debug, Deserialize)]
struct ChannelInfo {
    chatroom: Chatroom,
}

#[derive(Debug, Deserialize)]
struct Chatroom {
    id: u64,
}

/// The chatroom id from the channel API's answer.
fn parse_chatroom_id(json: &str) -> Result<u64, FailReason> {
    serde_json::from_str::<ChannelInfo>(json)
        .map(|info| info.chatroom.id)
        .map_err(|err| FailReason::Protocol(format!("channel: {err}")))
}

#[derive(Debug, Deserialize)]
struct TokenAnswer {
    data: TokenData,
}

#[derive(Debug, Deserialize)]
struct TokenData {
    token: String,
}

fn parse_token(json: &str) -> Result<String, FailReason> {
    serde_json::from_str::<TokenAnswer>(json)
        .map(|answer| answer.data.token)
        .map_err(|err| FailReason::Protocol(format!("token: {err}")))
}

/// What one socket message held. Kick may put several JSON frames in one message, one per line.
#[derive(Debug, Default, PartialEq, Eq)]
struct Frames {
    messages: Vec<ChatMessage>,
    /// The server sent `{}` and expects `{}` back.
    ping: bool,
    /// The connect or subscribe command was answered.
    replies: usize,
    /// The server refused a command, with its message.
    error: Option<String>,
    /// Frames that were not chat messages or could not be read, by event name, for the log.
    skipped: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Frame {
    id: Option<u64>,
    error: Option<FrameError>,
    push: Option<Push>,
}

#[derive(Debug, Deserialize)]
struct FrameError {
    code: Option<u32>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Push {
    #[serde(rename = "pub")]
    publication: Option<Publication>,
}

#[derive(Debug, Deserialize)]
struct Publication {
    data: PushData,
}

#[derive(Debug, Deserialize)]
struct PushData {
    event: String,
    /// The event's own JSON, as a string.
    data: String,
}

#[derive(Debug, Deserialize)]
struct KickChat {
    content: String,
    sender: KickSender,
}

#[derive(Debug, Deserialize)]
struct KickSender {
    id: u64,
    username: String,
    identity: KickIdentity,
}

#[derive(Debug, Deserialize)]
struct KickIdentity {
    #[serde(default)]
    badges: Vec<KickBadgeJson>,
}

#[derive(Debug, Deserialize)]
struct KickBadgeJson {
    #[serde(rename = "type")]
    kind: String,
    count: Option<u32>,
}

const CHAT_EVENT: &str = "App\\Events\\ChatMessageEvent";

fn read_frames(text: &str) -> Frames {
    let mut frames = Frames::default();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Ok(frame) = serde_json::from_str::<Frame>(line) else {
            frames.skipped.push("unreadable frame".into());
            continue;
        };
        match frame {
            Frame {
                error: Some(error), ..
            } => {
                let code = error.code.map_or(String::new(), |c| format!("{c} "));
                frames.error = Some(format!("{code}{}", error.message.unwrap_or_default()));
            }
            Frame { id: Some(_), .. } => frames.replies += 1,
            Frame {
                push:
                    Some(Push {
                        publication: Some(publication),
                    }),
                ..
            } => match chat_message(&publication.data) {
                Some(message) => frames.messages.push(message),
                None => frames.skipped.push(publication.data.event),
            },
            Frame { push: Some(_), .. } => frames.skipped.push("push without data".into()),
            Frame { .. } => frames.ping = true,
        }
    }
    frames
}

fn chat_message(data: &PushData) -> Option<ChatMessage> {
    if data.event != CHAT_EVENT {
        return None;
    }
    let chat: KickChat = serde_json::from_str(&data.data).ok()?;
    let badges: Vec<KickBadge> = chat
        .sender
        .identity
        .badges
        .into_iter()
        .map(|badge| KickBadge {
            kind: badge.kind,
            months: badge.count,
        })
        .collect();
    Some(ChatMessage::kick(
        chat.sender.id.to_string(),
        chat.sender.username,
        emote_names(&chat.content),
        &badges,
    ))
}

/// Kick writes emotes as `[emote:<id>:<name>]`; the name is what chat sees and types.
fn emote_names(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("[emote:") {
        out.push_str(&rest[..start]);
        let tag = &rest[start..];
        let Some(end) = tag.find(']') else {
            out.push_str(tag);
            return out;
        };
        let name = tag["[emote:".len()..end]
            .split_once(':')
            .map_or("", |(_, name)| name);
        out.push_str(name);
        rest = &tag[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Why a connection attempt ended.
enum Stop {
    /// The UI is gone; nothing left to do.
    UiGone,
    /// Cannot work until the user changes something.
    Fail(FailReason),
    /// Worth another try after a pause.
    Retry(String),
    /// The token is about to expire; reconnect now with a fresh one.
    Refresh,
}

/// Reads one channel until the task is aborted, the UI is gone, or Kick says the channel cannot
/// be read. Drops and server errors are retried with [`BACKOFF`].
pub(super) async fn run(channel: KickChannel, events: async_channel::Sender<ChatEvent>) {
    let target = ChatTarget::Kick(channel.clone());
    let send = |status: ConnectionStatus| {
        let events = events.clone();
        async move {
            events
                .send(ChatEvent::Status(Platform::Kick, status))
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
        match session(&client, &channel, &target, &events, &mut was_connected).await {
            Stop::UiGone => return,
            Stop::Fail(reason) => {
                tracing::warn!(channel = channel.as_str(), ?reason, "kick chat stopped");
                send(ConnectionStatus::Failed { target, reason }).await;
                return;
            }
            Stop::Refresh => {
                attempt = 0;
                cause = None;
            }
            Stop::Retry(why) => {
                tracing::info!(channel = channel.as_str(), why, "kick chat reconnecting");
                tokio::time::sleep(BACKOFF[attempt.min(BACKOFF.len() - 1)]).await;
                attempt += 1;
                cause = Some(why);
            }
        }
    }
}

/// HTTP failures: missing channel and refusals are final, everything else is worth a retry.
fn http_stop(what: &str, status: gpui_pre_reqwest::StatusCode) -> Stop {
    match status.as_u16() {
        404 => Stop::Fail(FailReason::NotFound),
        401 | 403 => Stop::Fail(FailReason::Blocked),
        429 | 500..=599 => Stop::Retry(format!("{what}: HTTP {status}")),
        _ => Stop::Fail(FailReason::Protocol(format!("{what}: HTTP {status}"))),
    }
}

async fn get_text(request: gpui_pre_reqwest::RequestBuilder, what: &str) -> Result<String, Stop> {
    let response = request
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
    channel: &KickChannel,
    target: &ChatTarget,
    events: &async_channel::Sender<ChatEvent>,
    was_connected: &mut bool,
) -> Stop {
    let channel_json = match get_text(
        client
            .get(channel_url(channel))
            .header("Accept", "application/json"),
        "channel",
    )
    .await
    {
        Ok(text) => text,
        Err(stop) => return stop,
    };
    let chatroom = match parse_chatroom_id(&channel_json) {
        Ok(id) => id,
        Err(reason) => return Stop::Fail(reason),
    };
    let client_id = uuid::Uuid::new_v4().to_string();
    let token_json = match get_text(
        client
            .post(TOKEN_URL)
            .header("Origin", ORIGIN)
            .header("Content-Type", "application/json")
            .body(serde_json::json!({ "client_id": client_id }).to_string()),
        "token",
    )
    .await
    {
        Ok(text) => text,
        Err(stop) => return stop,
    };
    let token = match parse_token(&token_json) {
        Ok(token) => token,
        Err(reason) => return Stop::Fail(reason),
    };

    let mut request = match tungstenite::client::IntoClientRequest::into_client_request(SOCKET_URL)
    {
        Ok(request) => request,
        Err(err) => return Stop::Fail(FailReason::Protocol(format!("socket: {err}"))),
    };
    let headers = request.headers_mut();
    headers.insert("Origin", ORIGIN.parse().expect("static header value"));
    headers.insert(
        "User-Agent",
        USER_AGENT.parse().expect("static header value"),
    );
    let mut socket = match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => socket,
        Err(tungstenite::Error::Http(response)) if response.status().as_u16() == 403 => {
            return Stop::Fail(FailReason::Blocked);
        }
        Err(err) => return Stop::Retry(format!("socket: {err}")),
    };
    let commands = [
        serde_json::json!({ "id": 1, "connect": { "token": token, "name": "js" } }),
        serde_json::json!({ "id": 2, "subscribe": { "channel": format!("chatrooms.{chatroom}.v2") } }),
    ];
    for command in commands {
        if let Err(err) = socket.send(Message::text(command.to_string())).await {
            return Stop::Retry(format!("socket: {err}"));
        }
    }

    let refresh_at = tokio::time::Instant::now() + TOKEN_REFRESH_AFTER;
    let mut replies = 0;
    let mut subscribed = false;
    loop {
        let message = tokio::select! {
            message = socket.next() => message,
            _ = tokio::time::sleep_until(refresh_at) => return Stop::Refresh,
        };
        let text = match message {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Close(frame))) => {
                return Stop::Retry(format!(
                    "closed: {}",
                    frame.map_or_else(String::new, |f| f.reason.to_string())
                ));
            }
            Some(Ok(_)) => continue,
            Some(Err(err)) => return Stop::Retry(format!("socket: {err}")),
            None => return Stop::Retry("socket closed".into()),
        };
        let frames = read_frames(&text);
        if let Some(error) = frames.error {
            return Stop::Fail(FailReason::Protocol(error));
        }
        if !frames.skipped.is_empty() {
            tracing::debug!(skipped = frames.skipped.len(), first = %frames.skipped[0], "kick frames skipped");
        }
        if frames.ping && socket.send(Message::text("{}")).await.is_err() {
            return Stop::Retry("socket: ping answer failed".into());
        }
        // Connect and subscribe are both answered: the chatroom is being read.
        replies += frames.replies;
        if replies >= 2 && !subscribed {
            subscribed = true;
            *was_connected = true;
            let connected = ConnectionStatus::Connected(target.clone());
            if events
                .send(ChatEvent::Status(Platform::Kick, connected))
                .await
                .is_err()
            {
                return Stop::UiGone;
            }
        }
        for message in frames.messages {
            if events.send(ChatEvent::Message(message)).await.is_err() {
                return Stop::UiGone;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chat::SubBadge;
    use crate::domain::role::Role;

    const CHANNEL: &str = include_str!("../../tests/fixtures/kick/channel.json");
    const FRAMES: &str = include_str!("../../tests/fixtures/kick/frames.txt");
    const CONNECT_REPLY: &str = include_str!("../../tests/fixtures/kick/connect_reply.json");

    #[test]
    fn chatroom_id_comes_from_the_channel_answer() {
        assert_eq!(parse_chatroom_id(CHANNEL), Ok(18490228));
        assert!(matches!(
            parse_chatroom_id(r#"{"message":"Not found"}"#),
            Err(FailReason::Protocol(_))
        ));
    }

    #[test]
    fn the_token_is_read_from_its_answer() {
        assert_eq!(
            parse_token(r#"{"data":{"token":"abc"},"message":"success"}"#),
            Ok("abc".into())
        );
        assert!(parse_token(r#"{"data":{}}"#).is_err());
    }

    #[test]
    fn recorded_frames_give_three_messages_a_ping_and_one_skipped_event() {
        let frames = read_frames(FRAMES);
        assert_eq!(frames.messages.len(), 3);
        assert!(frames.ping);
        assert_eq!(frames.skipped, ["App\\Events\\PinnedMessageCreatedEvent"]);
        assert_eq!(frames.error, None);

        let sub = &frames.messages[0];
        assert_eq!(sub.platform(), Platform::Kick);
        assert_eq!(sub.user().id, "1001");
        assert_eq!(sub.display_name(), "sub_viewer");
        assert_eq!(sub.text(), "collectiblesNOWAY");
        assert_eq!(sub.roles(), [Role::SubT1]);
        assert_eq!(sub.sub_badge(), Some(&SubBadge::Kick(2)));

        let plain = &frames.messages[1];
        assert_eq!(plain.text(), "hello chat");
        assert_eq!(plain.roles(), [Role::Pleb]);
    }

    #[test]
    fn the_connect_reply_counts_as_a_reply() {
        let frames = read_frames(CONNECT_REPLY);
        assert_eq!(frames.replies, 1);
        assert!(frames.messages.is_empty() && !frames.ping);
    }

    #[test]
    fn a_refused_command_is_an_error_and_garbage_is_skipped() {
        let frames = read_frames(
            "{\"id\":2,\"error\":{\"code\":103,\"message\":\"permission denied\"}}\nnot json",
        );
        assert_eq!(frames.error.as_deref(), Some("103 permission denied"));
        assert_eq!(frames.skipped, ["unreadable frame"]);
    }

    #[test]
    fn emote_tags_become_their_names() {
        assert_eq!(emote_names("GIVEME [emote:1:KEKW] now"), "GIVEME KEKW now");
        assert_eq!(emote_names("[emote:5:a][emote:6:b]"), "ab");
        assert_eq!(emote_names("broken [emote:7:x"), "broken [emote:7:x");
        assert_eq!(emote_names("plain"), "plain");
    }

    #[test]
    fn http_errors_split_into_final_and_retried() {
        use gpui_pre_reqwest::StatusCode;
        assert!(matches!(
            http_stop("channel", StatusCode::NOT_FOUND),
            Stop::Fail(FailReason::NotFound)
        ));
        assert!(matches!(
            http_stop("channel", StatusCode::FORBIDDEN),
            Stop::Fail(FailReason::Blocked)
        ));
        assert!(matches!(
            http_stop("channel", StatusCode::BAD_GATEWAY),
            Stop::Retry(_)
        ));
        assert!(matches!(
            http_stop("channel", StatusCode::TOO_MANY_REQUESTS),
            Stop::Retry(_)
        ));
        assert!(matches!(
            http_stop("channel", StatusCode::BAD_REQUEST),
            Stop::Fail(FailReason::Protocol(_))
        ));
    }
}
