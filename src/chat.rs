//! Read-only Twitch chat connection. Runs `twitch-irc` on its own thread and hands chat messages
//! and connection status to the UI over channels.

use std::thread;
use std::time::Duration;

use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer};
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::message::{PrivmsgMessage, ServerMessage};
use twitch_irc::{ClientConfig, SecureTCPTransport, TwitchIRCClient};

use crate::domain::chat::{Badge, ChannelLogin, ChatMessage};

/// How often the bridge asks `twitch-irc` whether the channel is still joined. A guess, not
/// measured: short enough that a drop shows within seconds, long enough to cost nothing.
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// What the UI shows about the chat connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Idle,
    /// Not joined yet; `cause` is the last problem reported while trying.
    Connecting {
        channel: ChannelLogin,
        cause: Option<String>,
    },
    Connected(ChannelLogin),
    /// The connection dropped; `twitch-irc` keeps reconnecting on its own.
    Reconnecting {
        channel: ChannelLogin,
        cause: Option<String>,
    },
}

/// Sent from the chat thread to the UI.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    Status(ConnectionStatus),
    Message(ChatMessage),
    /// A command could not be carried out; the text says why.
    Rejected(String),
}

/// Sent from the UI to the chat thread. Joining a channel leaves the previous one.
#[derive(Debug, Clone)]
pub enum ChatCommand {
    Join(ChannelLogin),
    Leave,
}

pub fn to_chat_message(msg: &PrivmsgMessage) -> ChatMessage {
    ChatMessage {
        user_id: msg.sender.id.clone(),
        display_name: msg.sender.name.clone(),
        badges: msg
            .badges
            .iter()
            .map(|b| Badge::new(b.name.clone(), b.version.clone()))
            .collect(),
        text: msg.message_text.clone(),
    }
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
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        thread::Builder::new()
            .name("twitch-chat".into())
            .spawn(move || runtime.block_on(run(command_rx, event_tx, problems)))?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
        })
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

/// Connection status of the one channel the bridge watches.
#[derive(Debug, Default)]
struct StatusTracker {
    channel: Option<ChannelLogin>,
    joined: bool,
    was_joined: bool,
    last_cause: Option<String>,
}

impl StatusTracker {
    fn status(&self) -> ConnectionStatus {
        match (&self.channel, self.joined, self.was_joined) {
            (None, _, _) => ConnectionStatus::Idle,
            (Some(c), true, _) => ConnectionStatus::Connected(c.clone()),
            (Some(c), false, false) => ConnectionStatus::Connecting {
                channel: c.clone(),
                cause: self.last_cause.clone(),
            },
            (Some(c), false, true) => ConnectionStatus::Reconnecting {
                channel: c.clone(),
                cause: self.last_cause.clone(),
            },
        }
    }

    fn watch(&mut self, channel: Option<ChannelLogin>) {
        *self = Self {
            channel,
            ..Self::default()
        };
    }

    fn set_joined(&mut self, joined: bool) {
        if joined {
            self.was_joined = true;
            self.last_cause = None;
        }
        self.joined = joined;
    }

    /// Result of asking `twitch-irc` whether the channel is joined. Before the first JOIN echo a
    /// poll may only confirm the join, never report a drop.
    fn on_poll(&mut self, joined: bool) {
        if self.was_joined || joined {
            self.set_joined(joined);
        }
    }

    fn connection_problem(&mut self, cause: String) {
        self.last_cause = Some(cause);
    }
}

async fn run(
    commands: async_channel::Receiver<ChatCommand>,
    events: async_channel::Sender<ChatEvent>,
    problems: async_channel::Receiver<String>,
) {
    let (mut incoming, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(ClientConfig::default());
    let mut tracker = StatusTracker::default();
    let mut poll = tokio::time::interval(STATUS_POLL_INTERVAL);
    let mut last_sent = tracker.status();

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Ok(command) = command else { break };
                if let Some(old) = &tracker.channel {
                    client.part(old.as_str().to_owned());
                }
                let channel = match command {
                    ChatCommand::Join(channel) => Some(channel),
                    ChatCommand::Leave => None,
                };
                if let Some(channel) = &channel
                    && let Err(err) = client.join(channel.as_str().to_owned())
                {
                    tracker.watch(None);
                    tracing::warn!(channel = channel.as_str(), %err, "join rejected");
                    let reason = format!("cannot join {}: {err}", channel.as_str());
                    if events.send(ChatEvent::Rejected(reason)).await.is_err() {
                        break;
                    }
                } else {
                    tracker.watch(channel);
                }
            }
            Some(message) = incoming.recv() => match message {
                ServerMessage::Join(join) if is_watched(&tracker, &join.channel_login) => {
                    tracker.set_joined(true);
                }
                ServerMessage::Privmsg(msg) if is_watched(&tracker, &msg.channel_login) => {
                    if events.send(ChatEvent::Message(to_chat_message(&msg))).await.is_err() {
                        break;
                    }
                }
                ServerMessage::Notice(notice) => {
                    tracker.connection_problem(notice.message_text);
                }
                _ => {}
            },
            Ok(problem) = problems.recv() => tracker.connection_problem(problem),
            _ = poll.tick() => {
                if let Some(channel) = &tracker.channel {
                    let (_, joined) = client.get_channel_status(channel.as_str().to_owned()).await;
                    tracker.on_poll(joined);
                }
            }
        }
        let status = tracker.status();
        if status != last_sent {
            if events
                .send(ChatEvent::Status(status.clone()))
                .await
                .is_err()
            {
                break;
            }
            last_sent = status;
        }
    }
}

fn is_watched(tracker: &StatusTracker, channel_login: &str) -> bool {
    tracker
        .channel
        .as_ref()
        .is_some_and(|c| c.as_str() == channel_login)
}

#[cfg(test)]
mod tests {
    use twitch_irc::message::IRCMessage;

    use super::*;

    fn channel(name: &str) -> ChannelLogin {
        ChannelLogin::parse(name).unwrap()
    }

    #[test]
    fn maps_tagged_privmsg_to_chat_message() {
        let raw = "@badge-info=subscriber/14;badges=moderator/1,subscriber/3012;color=#FF0000;\
            display-name=Viewer_1;emotes=;first-msg=0;flags=;id=abc;mod=1;room-id=11;\
            subscriber=1;tmi-sent-ts=1700000000000;turbo=0;user-id=4242;user-type=mod \
            :viewer_1!viewer_1@viewer_1.tmi.twitch.tv PRIVMSG #streamer :YEP lets go";
        let irc = IRCMessage::parse(raw).unwrap();
        let msg = PrivmsgMessage::try_from(irc).unwrap();

        assert_eq!(
            to_chat_message(&msg),
            ChatMessage {
                user_id: "4242".into(),
                display_name: "Viewer_1".into(),
                badges: vec![
                    Badge::new("moderator", "1"),
                    Badge::new("subscriber", "3012")
                ],
                text: "YEP lets go".into(),
            }
        );
    }

    #[test]
    fn status_goes_connecting_connected_then_reconnecting_with_cause() {
        let mut tracker = StatusTracker::default();
        assert_eq!(tracker.status(), ConnectionStatus::Idle);

        tracker.watch(Some(channel("streamer")));
        assert_eq!(tracker.status(), connecting("streamer", None));

        tracker.set_joined(true);
        assert_eq!(
            tracker.status(),
            ConnectionStatus::Connected(channel("streamer"))
        );

        tracker.connection_problem("Closing connection, cause: timed out".into());
        tracker.set_joined(false);
        assert_eq!(
            tracker.status(),
            ConnectionStatus::Reconnecting {
                channel: channel("streamer"),
                cause: Some("Closing connection, cause: timed out".into()),
            }
        );

        tracker.set_joined(true);
        assert_eq!(
            tracker.status(),
            ConnectionStatus::Connected(channel("streamer"))
        );
    }

    #[test]
    fn switching_channel_starts_over_without_old_cause() {
        let mut tracker = StatusTracker::default();
        tracker.watch(Some(channel("first")));
        tracker.set_joined(true);
        tracker.connection_problem("old problem".into());
        tracker.watch(Some(channel("second")));
        assert_eq!(tracker.status(), connecting("second", None));
    }

    fn connecting(name: &str, cause: Option<&str>) -> ConnectionStatus {
        ConnectionStatus::Connecting {
            channel: channel(name),
            cause: cause.map(str::to_owned),
        }
    }

    #[test]
    fn first_connect_that_fails_shows_the_cause() {
        let mut tracker = StatusTracker::default();
        tracker.watch(Some(channel("streamer")));
        tracker.connection_problem("Underlying transport failed to connect".into());
        tracker.on_poll(false);
        assert_eq!(
            tracker.status(),
            connecting("streamer", Some("Underlying transport failed to connect"))
        );
    }

    #[test]
    fn poll_confirms_first_join_and_later_reports_drop() {
        let mut tracker = StatusTracker::default();
        tracker.watch(Some(channel("streamer")));
        tracker.on_poll(true);
        assert_eq!(
            tracker.status(),
            ConnectionStatus::Connected(channel("streamer"))
        );
        tracker.on_poll(false);
        assert!(matches!(
            tracker.status(),
            ConnectionStatus::Reconnecting { .. }
        ));
    }

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
}
