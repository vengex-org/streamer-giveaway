//! Read-only chat connections. One thread runs a tokio runtime with a task per connected
//! platform; chat messages and each platform's connection status reach the UI over channels.

mod kick;
mod twitch;
mod youtube;

use std::collections::HashMap;
use std::thread;

use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer};

use crate::domain::chat::{ChannelLogin, ChatMessage};
use crate::domain::platform::{KickChannel, Platform, UserKey, YouTubeSource};

pub use kick::channel_url as kick_channel_url;
pub use twitch::to_chat_message;

/// Browser-like client identity for Kick and YouTube, whose bot filters turn away requests that
/// look scripted.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36     (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Waits between reconnect attempts on Kick and YouTube, then the last one again. A guess: quick
/// after a short drop, slow enough not to hammer a platform during an outage.
const BACKOFF: [std::time::Duration; 5] = [
    std::time::Duration::from_secs(1),
    std::time::Duration::from_secs(2),
    std::time::Duration::from_secs(5),
    std::time::Duration::from_secs(10),
    std::time::Duration::from_secs(30),
];

/// A chat to read: a Twitch channel, a Kick channel or a YouTube stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatTarget {
    Twitch(ChannelLogin),
    Kick(KickChannel),
    YouTube(YouTubeSource),
}

impl ChatTarget {
    pub fn platform(&self) -> Platform {
        match self {
            Self::Twitch(_) => Platform::Twitch,
            Self::Kick(_) => Platform::Kick,
            Self::YouTube(_) => Platform::YouTube,
        }
    }

    /// Short name for status lines, e.g. `#xqc`, `kick.com/xqc`, `@IShowSpeed`.
    pub fn label(&self) -> String {
        match self {
            Self::Twitch(channel) => format!("#{}", channel.as_str()),
            Self::Kick(channel) => format!("kick.com/{}", channel.as_str()),
            Self::YouTube(YouTubeSource::Video(id)) => format!("youtu.be/{id}"),
            Self::YouTube(source) => source.to_string(),
        }
    }
}

/// Why a chat cannot be read. These do not get better by retrying, so the platform stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailReason {
    /// The channel or stream does not exist.
    NotFound,
    /// The channel exists but is not live, so it has no live chat.
    Offline,
    /// The platform refused the app's requests (HTTP 403, e.g. a bot filter).
    Blocked,
    /// The platform answered in a shape this version cannot read; the text says what was wrong.
    Protocol(String),
}

/// What the UI shows about one platform's chat connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Idle,
    /// Not reading yet; `cause` is the last problem reported while trying.
    Connecting {
        target: ChatTarget,
        cause: Option<String>,
    },
    Connected(ChatTarget),
    /// The connection dropped; the platform task keeps reconnecting on its own.
    Reconnecting {
        target: ChatTarget,
        cause: Option<String>,
    },
    /// Stopped for good until the user connects again.
    Failed {
        target: ChatTarget,
        reason: FailReason,
    },
}

/// Sent from the chat thread to the UI.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    Status(Platform, ConnectionStatus),
    Message(ChatMessage),
    /// A command could not be carried out; the text says why.
    Rejected(String),
}

/// Sent from the UI to the chat thread. Joining replaces that platform's previous chat; the other
/// platforms keep running.
#[derive(Debug, Clone)]
pub enum ChatCommand {
    Join(ChatTarget),
    Leave(Platform),
    /// This account was just drawn. The demo chat makes them answer; real chats ignore it.
    WinnerDrawn(UserKey),
}

/// Handle to the chat thread. Dropping it closes the command channel and stops the thread.
pub struct ChatBridge {
    pub commands: async_channel::Sender<ChatCommand>,
    pub events: async_channel::Receiver<ChatEvent>,
}

impl ChatBridge {
    /// Starts the chat thread. `problems` receives connection problems reported by `twitch-irc`
    /// (see [`connection_problem_layer`]).
    pub fn spawn(problems: async_channel::Receiver<String>) -> std::io::Result<Self> {
        // The GPUI and reqwest crates enable both of rustls' crypto backends, and then rustls
        // cannot pick one on its own. Kick's socket needs it chosen; an earlier install wins.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        thread::Builder::new().name("chat".into()).spawn(move || {
            // Platform tasks run on this one thread, so they need not be `Send`.
            let tasks = tokio::task::LocalSet::new();
            runtime.block_on(tasks.run_until(run(command_rx, event_tx, problems)))
        })?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
        })
    }
}

/// Why one connection attempt to Kick or YouTube ended.
enum Stop {
    /// The UI is gone; nothing left to do.
    UiGone,
    /// Cannot work until the user changes something.
    Fail(FailReason),
    /// Worth another try after a pause.
    Retry(String),
    /// Kick's token is about to expire; reconnect now with a fresh one.
    Refresh,
}

/// HTTP failures: a missing channel and refusals are final, overload and server errors are
/// worth a retry, anything else means the platform answers in a way this version cannot read.
fn http_stop(what: &str, status: gpui_pre_reqwest::StatusCode) -> Stop {
    match status.as_u16() {
        404 => Stop::Fail(FailReason::NotFound),
        401 | 403 => Stop::Fail(FailReason::Blocked),
        429 | 500..=599 => Stop::Retry(format!("{what}: HTTP {status}")),
        _ => Stop::Fail(FailReason::Protocol(format!("{what}: HTTP {status}"))),
    }
}

/// The body of a successful answer; network errors are retried, HTTP errors per [`http_stop`].
async fn fetch_text(request: gpui_pre_reqwest::RequestBuilder, what: &str) -> Result<String, Stop> {
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

/// What a connection attempt reports to the UI.
struct Link {
    target: ChatTarget,
    events: async_channel::Sender<ChatEvent>,
    /// This attempt reached the chat.
    reached: std::sync::atomic::AtomicBool,
}

impl Link {
    /// The chat is being read. Returns `false` when the UI is gone.
    async fn connected(&self) -> bool {
        self.reached
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let status = ConnectionStatus::Connected(self.target.clone());
        self.events
            .send(ChatEvent::Status(self.target.platform(), status))
            .await
            .is_ok()
    }

    fn reached(&self) -> bool {
        self.reached.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Hands on a chat message. Returns `false` when the UI is gone.
    async fn message(&self, message: ChatMessage) -> bool {
        self.events.send(ChatEvent::Message(message)).await.is_ok()
    }
}

/// Runs `attempt` until the UI is gone or the chat cannot be read, reconnecting after drops with
/// [`BACKOFF`]. The wait starts over once an attempt reaches the chat, so a stream that drops now
/// and then always reconnects quickly.
async fn run_with_retries(
    target: ChatTarget,
    events: async_channel::Sender<ChatEvent>,
    mut attempt: impl AsyncFnMut(&gpui_pre_reqwest::Client, &Link) -> Stop,
) {
    let platform = target.platform();
    let send = async |status: ConnectionStatus| {
        events
            .send(ChatEvent::Status(platform, status))
            .await
            .is_ok()
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
    let mut failures = 0;
    let mut cause: Option<String> = None;
    let mut ever_reached = false;
    loop {
        let status = if ever_reached {
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
        let link = Link {
            target: target.clone(),
            events: events.clone(),
            reached: std::sync::atomic::AtomicBool::new(false),
        };
        let stop = attempt(&client, &link).await;
        if link.reached() {
            ever_reached = true;
            failures = 0;
        }
        match stop {
            Stop::UiGone => return,
            Stop::Fail(reason) => {
                tracing::warn!(%platform, ?reason, "chat stopped");
                send(ConnectionStatus::Failed { target, reason }).await;
                return;
            }
            Stop::Refresh => cause = None,
            Stop::Retry(why) => {
                tracing::info!(%platform, why, "chat reconnecting");
                tokio::time::sleep(BACKOFF[failures.min(BACKOFF.len() - 1)]).await;
                failures += 1;
                cause = Some(why);
            }
        }
    }
}

/// Starts and stops one task per platform as the UI joins and leaves chats.
async fn run(
    commands: async_channel::Receiver<ChatCommand>,
    events: async_channel::Sender<ChatEvent>,
    problems: async_channel::Receiver<String>,
) {
    let mut tasks: HashMap<Platform, tokio::task::JoinHandle<()>> = HashMap::new();
    while let Ok(command) = commands.recv().await {
        match command {
            ChatCommand::Join(target) => {
                if let Some(task) = tasks.remove(&target.platform()) {
                    task.abort();
                }
                let platform = target.platform();
                let events = events.clone();
                let task = match target {
                    ChatTarget::Twitch(channel) => {
                        tokio::task::spawn_local(twitch::run(channel, events, problems.clone()))
                    }
                    ChatTarget::Kick(channel) => {
                        tokio::task::spawn_local(kick::run(channel, events))
                    }
                    ChatTarget::YouTube(source) => {
                        tokio::task::spawn_local(youtube::run(source, events))
                    }
                };
                tasks.insert(platform, task);
            }
            ChatCommand::Leave(platform) => {
                if let Some(task) = tasks.remove(&platform) {
                    task.abort();
                }
                let idle = ChatEvent::Status(platform, ConnectionStatus::Idle);
                if events.send(idle).await.is_err() {
                    break;
                }
            }
            ChatCommand::WinnerDrawn(_) => {}
        }
    }
    for task in tasks.values() {
        task.abort();
    }
}

/// A `tracing` layer that forwards the connection problems `twitch-irc` only logs (dropped
/// connection, transport errors), so the UI can show why the chat disconnected.
pub fn connection_problem_layer() -> (ConnectionProblemLayer, async_channel::Receiver<String>) {
    let (tx, rx) = async_channel::unbounded();
    (ConnectionProblemLayer { problems: tx }, rx)
}

pub struct ConnectionProblemLayer {
    problems: async_channel::Sender<String>,
}

impl<S: tracing::Subscriber> Layer<S> for ConnectionProblemLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let meta = event.metadata();
        if !meta.target().starts_with("twitch_irc") {
            return;
        }
        let mut message = MessageField(String::new());
        event.record(&mut message);
        let is_problem =
            *meta.level() == tracing::Level::ERROR || message.0.starts_with("Closing connection");
        if is_problem {
            // The receiver lives as long as the chat thread; after it ends there is nobody to tell.
            let _ = self.problems.try_send(message.0);
        }
    }
}

struct MessageField(String);

impl Visit for MessageField {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured_problems(emit: impl FnOnce()) -> Vec<String> {
        use tracing_subscriber::prelude::*;
        let (layer, problems) = connection_problem_layer();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, emit);
        std::iter::from_fn(|| problems.try_recv().ok()).collect()
    }

    #[test]
    fn layer_forwards_twitch_irc_errors_and_closing_connection() {
        let problems = captured_problems(|| {
            tracing::error!(target: "twitch_irc::connection", "transport failed");
            tracing::info!(target: "twitch_irc::connection", "Closing connection, cause: timeout");
        });
        assert_eq!(
            problems,
            ["transport failed", "Closing connection, cause: timeout"]
        );
    }

    #[test]
    fn layer_skips_other_targets_and_routine_info() {
        let problems = captured_problems(|| {
            tracing::error!(target: "gpui::window", "unrelated error");
            tracing::info!(target: "twitch_irc::client", "Making a new pool connection");
        });
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn labels_name_the_chat() {
        let twitch = ChatTarget::Twitch(ChannelLogin::parse("xqc").unwrap());
        let kick = ChatTarget::Kick(KickChannel::parse("xqc").unwrap());
        let video = ChatTarget::YouTube(YouTubeSource::parse("youtu.be/qSyo5d-dVzA").unwrap());
        let handle = ChatTarget::YouTube(YouTubeSource::parse("@IShowSpeed").unwrap());
        assert_eq!(twitch.label(), "#xqc");
        assert_eq!(kick.label(), "kick.com/xqc");
        assert_eq!(video.label(), "youtu.be/qSyo5d-dVzA");
        assert_eq!(handle.label(), "@IShowSpeed");
    }

    #[tokio::test]
    async fn leaving_one_platform_reports_it_idle_and_keeps_listening() {
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let (_problem_tx, problems) = async_channel::unbounded();
        let bridge = tokio::spawn(run(command_rx, event_tx, problems));
        command_tx
            .send(ChatCommand::Leave(Platform::Kick))
            .await
            .unwrap();
        command_tx
            .send(ChatCommand::Leave(Platform::YouTube))
            .await
            .unwrap();
        for platform in [Platform::Kick, Platform::YouTube] {
            let event = event_rx.recv().await.unwrap();
            assert!(
                matches!(event, ChatEvent::Status(p, ConnectionStatus::Idle) if p == platform),
                "{event:?}"
            );
        }
        drop(command_tx);
        bridge.await.unwrap();
    }

    #[test]
    fn http_errors_split_into_final_and_retried() {
        use gpui_pre_reqwest::StatusCode;
        let stop = |status| http_stop("channel", status);
        assert!(matches!(
            stop(StatusCode::NOT_FOUND),
            Stop::Fail(FailReason::NotFound)
        ));
        assert!(matches!(
            stop(StatusCode::FORBIDDEN),
            Stop::Fail(FailReason::Blocked)
        ));
        assert!(matches!(stop(StatusCode::BAD_GATEWAY), Stop::Retry(_)));
        assert!(matches!(
            stop(StatusCode::TOO_MANY_REQUESTS),
            Stop::Retry(_)
        ));
        assert!(matches!(
            stop(StatusCode::BAD_REQUEST),
            Stop::Fail(FailReason::Protocol(_))
        ));
    }

    fn statuses(events: &async_channel::Receiver<ChatEvent>) -> Vec<String> {
        std::iter::from_fn(|| events.try_recv().ok())
            .map(|event| match event {
                ChatEvent::Status(_, ConnectionStatus::Connecting { .. }) => "connecting".into(),
                ChatEvent::Status(_, ConnectionStatus::Connected(_)) => "connected".into(),
                ChatEvent::Status(_, ConnectionStatus::Reconnecting { cause, .. }) => {
                    format!("reconnecting: {}", cause.unwrap_or_default())
                }
                ChatEvent::Status(_, ConnectionStatus::Failed { reason, .. }) => {
                    format!("failed: {reason:?}")
                }
                other => format!("{other:?}"),
            })
            .collect()
    }

    fn kick_target() -> ChatTarget {
        ChatTarget::Kick(KickChannel::parse("xqc").unwrap())
    }

    #[tokio::test(start_paused = true)]
    async fn a_drop_after_connecting_reconnects_and_reports_each_step() {
        let (events, received) = async_channel::unbounded();
        let mut attempts = 0;
        run_with_retries(kick_target(), events, async |_, link: &Link| {
            attempts += 1;
            link.connected().await;
            match attempts {
                1 => Stop::Retry("socket closed".into()),
                _ => Stop::Fail(FailReason::NotFound),
            }
        })
        .await;
        assert_eq!(
            statuses(&received),
            [
                "connecting",
                "connected",
                "reconnecting: socket closed",
                "connected",
                "failed: NotFound"
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_wait_starts_over_after_each_successful_connect() {
        let (events, _received) = async_channel::unbounded();
        let mut attempts = 0;
        let start = tokio::time::Instant::now();
        run_with_retries(kick_target(), events, async |_, link: &Link| {
            attempts += 1;
            if attempts > 5 {
                return Stop::UiGone;
            }
            link.connected().await;
            Stop::Retry("drop".into())
        })
        .await;
        // Five drops, each after a successful connect: five waits of the first, shortest step.
        assert_eq!(start.elapsed(), BACKOFF[0] * 5);
    }

    #[tokio::test(start_paused = true)]
    async fn failures_without_connecting_wait_longer_each_time() {
        let (events, _received) = async_channel::unbounded();
        let mut attempts = 0;
        let start = tokio::time::Instant::now();
        run_with_retries(kick_target(), events, async |_, _: &Link| {
            attempts += 1;
            if attempts > 3 {
                return Stop::UiGone;
            }
            Stop::Retry("down".into())
        })
        .await;
        assert_eq!(start.elapsed(), BACKOFF[0] + BACKOFF[1] + BACKOFF[2]);
    }
}
