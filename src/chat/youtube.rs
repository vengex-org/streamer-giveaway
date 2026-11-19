//! YouTube live chat. Filled in by a later commit; until then joining reports that it is not there
//! yet.

use super::{ChatEvent, ChatTarget, ConnectionStatus, FailReason};
use crate::domain::platform::{Platform, YouTubeSource};

pub(super) async fn run(source: YouTubeSource, events: async_channel::Sender<ChatEvent>) {
    let status = ConnectionStatus::Failed {
        target: ChatTarget::YouTube(source),
        reason: FailReason::Protocol("YouTube chat is not available in this build".into()),
    };
    let _ = events
        .send(ChatEvent::Status(Platform::YouTube, status))
        .await;
}
