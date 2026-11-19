//! Kick chat. Filled in by the next commit; until then joining reports that it is not there yet.

use super::{ChatEvent, ChatTarget, ConnectionStatus, FailReason};
use crate::domain::platform::{KickChannel, Platform};

pub(super) async fn run(channel: KickChannel, events: async_channel::Sender<ChatEvent>) {
    let status = ConnectionStatus::Failed {
        target: ChatTarget::Kick(channel),
        reason: FailReason::Protocol("Kick chat is not available in this build".into()),
    };
    let _ = events.send(ChatEvent::Status(Platform::Kick, status)).await;
}
