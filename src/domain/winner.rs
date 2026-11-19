use std::time::{Duration, Instant};

use super::chat::ChatMessage;
use super::giveaway::Entrant;

/// A chat message the winner sent after being drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Time since the draw.
    pub after: Duration,
    pub text: String,
}

/// Whether the winner has said anything in chat since the draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitState {
    Waiting,
    /// The first reply came this long after the draw.
    Answered {
        after: Duration,
    },
}

/// Follows a drawn winner: how long since the draw, and what they wrote since.
#[derive(Debug, Clone)]
pub struct WinnerWatch {
    winner: Entrant,
    drawn_at: Instant,
    replies: Vec<Reply>,
    state: WaitState,
}

impl WinnerWatch {
    pub fn new(winner: Entrant, drawn_at: Instant) -> Self {
        Self {
            winner,
            drawn_at,
            replies: Vec::new(),
            state: WaitState::Waiting,
        }
    }

    pub fn winner(&self) -> &Entrant {
        &self.winner
    }

    pub fn replies(&self) -> &[Reply] {
        &self.replies
    }

    pub fn state(&self) -> WaitState {
        self.state
    }

    /// Records the message when the winner wrote it; returns whether it was theirs.
    pub fn on_message(&mut self, msg: &ChatMessage, now: Instant) -> bool {
        if *msg.user() != self.winner.user {
            return false;
        }
        let after = now.saturating_duration_since(self.drawn_at);
        if self.state == WaitState::Waiting {
            self.state = WaitState::Answered { after };
        }
        self.replies.push(Reply {
            after,
            text: msg.text().to_owned(),
        });
        true
    }

    /// Time since the draw; stops counting at the first reply.
    pub fn elapsed(&self, now: Instant) -> Duration {
        match self.state {
            WaitState::Waiting => now.saturating_duration_since(self.drawn_at),
            WaitState::Answered { after } => after,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::platform::{Platform, UserKey};
    use crate::domain::role::Role;

    fn winner() -> Entrant {
        Entrant {
            user: UserKey::new(Platform::Twitch, "7"),
            display_name: "Lucky".into(),
            roles: vec![Role::Pleb],
            sub_badge: None,
        }
    }

    fn msg(user_id: &str, text: &str) -> ChatMessage {
        ChatMessage::twitch(user_id, "x", text, &[])
    }

    #[test]
    fn stopwatch_runs_while_waiting() {
        let start = Instant::now();
        let watch = WinnerWatch::new(winner(), start);
        assert_eq!(watch.state(), WaitState::Waiting);
        assert_eq!(
            watch.elapsed(start + Duration::from_secs(42)),
            Duration::from_secs(42)
        );
    }

    #[test]
    fn first_reply_answers_and_freezes_the_stopwatch() {
        let start = Instant::now();
        let mut watch = WinnerWatch::new(winner(), start);
        assert!(watch.on_message(&msg("7", "I won!"), start + Duration::from_secs(12)));
        assert!(watch.on_message(&msg("7", "discord: lucky"), start + Duration::from_secs(20)));

        assert_eq!(
            watch.state(),
            WaitState::Answered {
                after: Duration::from_secs(12)
            }
        );
        assert_eq!(
            watch.elapsed(start + Duration::from_secs(90)),
            Duration::from_secs(12)
        );
        let texts: Vec<_> = watch.replies().iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["I won!", "discord: lucky"]);
        assert_eq!(watch.replies()[1].after, Duration::from_secs(20));
    }

    #[test]
    fn messages_from_other_users_are_ignored() {
        let start = Instant::now();
        let mut watch = WinnerWatch::new(winner(), start);
        assert!(!watch.on_message(&msg("8", "not me"), start + Duration::from_secs(3)));
        assert_eq!(watch.state(), WaitState::Waiting);
        assert!(watch.replies().is_empty());
    }
}
