//! Twitch chat, read anonymously over IRC with `twitch-irc`.

use std::time::Duration;

use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::message::{PrivmsgMessage, ServerMessage};
use twitch_irc::{ClientConfig, SecureTCPTransport, TwitchIRCClient};

use super::{ChatEvent, ChatTarget, ConnectionStatus};
use crate::domain::chat::{Badge, ChannelLogin, ChatMessage};
use crate::domain::platform::Platform;

/// How often the task asks `twitch-irc` whether the channel is still joined. A guess, not
/// measured: short enough that a drop shows within seconds, long enough to cost nothing.
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(3);

pub fn to_chat_message(msg: &PrivmsgMessage) -> ChatMessage {
    let badges: Vec<Badge> = msg
        .badges
        .iter()
        .map(|b| Badge::new(b.name.clone(), b.version.clone()))
        .collect();
    ChatMessage::twitch(
        msg.sender.id.clone(),
        msg.sender.name.clone(),
        msg.message_text.clone(),
        &badges,
    )
}

/// Connection status of the one channel the task watches.
#[derive(Debug, Default)]
struct StatusTracker {
    channel: Option<ChannelLogin>,
    joined: bool,
    was_joined: bool,
    last_cause: Option<String>,
}

impl StatusTracker {
    fn status(&self) -> ConnectionStatus {
        let target = |c: &ChannelLogin| ChatTarget::Twitch(c.clone());
        match (&self.channel, self.joined, self.was_joined) {
            (None, _, _) => ConnectionStatus::Idle,
            (Some(c), true, _) => ConnectionStatus::Connected(target(c)),
            (Some(c), false, false) => ConnectionStatus::Connecting {
                target: target(c),
                cause: self.last_cause.clone(),
            },
            (Some(c), false, true) => ConnectionStatus::Reconnecting {
                target: target(c),
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

/// Reads one channel until the task is aborted or the UI is gone. `twitch-irc` reconnects on its
/// own; this only reports the status.
pub(super) async fn run(
    channel: ChannelLogin,
    events: async_channel::Sender<ChatEvent>,
    problems: async_channel::Receiver<String>,
) {
    let (mut incoming, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(ClientConfig::default());
    let mut tracker = StatusTracker::default();
    if let Err(err) = client.join(channel.as_str().to_owned()) {
        tracing::warn!(channel = channel.as_str(), %err, "join rejected");
        let reason = format!("cannot join {}: {err}", channel.as_str());
        let _ = events.send(ChatEvent::Rejected(reason)).await;
        let _ = events
            .send(ChatEvent::Status(Platform::Twitch, ConnectionStatus::Idle))
            .await;
        return;
    }
    tracker.watch(Some(channel.clone()));
    let mut poll = tokio::time::interval(STATUS_POLL_INTERVAL);
    let mut last_sent = None;

    loop {
        let status = tracker.status();
        if last_sent.as_ref() != Some(&status) {
            let event = ChatEvent::Status(Platform::Twitch, status.clone());
            if events.send(event).await.is_err() {
                return;
            }
            last_sent = Some(status);
        }
        tokio::select! {
            Some(message) = incoming.recv() => match message {
                ServerMessage::Join(join) if join.channel_login == channel.as_str() => {
                    tracker.set_joined(true);
                }
                ServerMessage::Privmsg(msg) if msg.channel_login == channel.as_str() => {
                    if events.send(ChatEvent::Message(to_chat_message(&msg))).await.is_err() {
                        return;
                    }
                }
                ServerMessage::Notice(notice) => {
                    tracker.connection_problem(notice.message_text);
                }
                _ => {}
            },
            Ok(problem) = problems.recv() => tracker.connection_problem(problem),
            _ = poll.tick() => {
                let (_, joined) = client.get_channel_status(channel.as_str().to_owned()).await;
                tracker.on_poll(joined);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use twitch_irc::message::IRCMessage;

    use super::*;

    fn channel(name: &str) -> ChannelLogin {
        ChannelLogin::parse(name).unwrap()
    }

    fn target(name: &str) -> ChatTarget {
        ChatTarget::Twitch(channel(name))
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
            ChatMessage::twitch(
                "4242",
                "Viewer_1",
                "YEP lets go",
                &[
                    Badge::new("moderator", "1"),
                    Badge::new("subscriber", "3012")
                ]
            )
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
            ConnectionStatus::Connected(target("streamer"))
        );

        tracker.connection_problem("Closing connection, cause: timed out".into());
        tracker.set_joined(false);
        assert_eq!(
            tracker.status(),
            ConnectionStatus::Reconnecting {
                target: target("streamer"),
                cause: Some("Closing connection, cause: timed out".into()),
            }
        );

        tracker.set_joined(true);
        assert_eq!(
            tracker.status(),
            ConnectionStatus::Connected(target("streamer"))
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
            target: target(name),
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
            ConnectionStatus::Connected(target("streamer"))
        );
        tracker.on_poll(false);
        assert!(matches!(
            tracker.status(),
            ConnectionStatus::Reconnecting { .. }
        ));
    }
}
