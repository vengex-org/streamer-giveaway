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

pub use twitch::to_chat_message;

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
        thread::Builder::new()
            .name("chat".into())
            .spawn(move || runtime.block_on(run(command_rx, event_tx, problems)))?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
        })
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
                        tokio::spawn(twitch::run(channel, events, problems.clone()))
                    }
                    ChatTarget::Kick(channel) => tokio::spawn(kick::run(channel, events)),
                    ChatTarget::YouTube(source) => tokio::spawn(youtube::run(source, events)),
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
}
