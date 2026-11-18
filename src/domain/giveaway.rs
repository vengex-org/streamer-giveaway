use std::collections::HashSet;

use indexmap::IndexMap;
use rand::{Rng, RngExt};

use super::chat::{ChatMessage, Keyword};
use super::matcher::{MatchMode, matches};
use super::role::{Role, Weights, roles_from_badges};

/// A chatter who entered the giveaway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entrant {
    pub user_id: String,
    pub display_name: String,
    /// Last known roles: refreshed by every later message of this user.
    pub roles: Vec<Role>,
}

impl Entrant {
    pub fn effective(&self, weights: &Weights) -> (Role, u32) {
        weights.effective(&self.roles)
    }
}

/// Whether matching messages add entrants. A new giveaway starts `Closed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiveawayState {
    Open,
    Closed,
}

/// What a chat message did to the giveaway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageOutcome {
    Entered,
    /// The author was already an entrant; their roles and display name were refreshed.
    AlreadyEntered,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawOutcome {
    Winner(Entrant),
    EmptyPool,
    /// Entrants exist, but every one of them has weight 0.
    NoDrawable,
}

#[derive(Debug, Clone)]
pub struct Giveaway {
    keyword: Keyword,
    mode: MatchMode,
    state: GiveawayState,
    entrants: IndexMap<String, Entrant>,
    /// User ids drawn since the last reset; they cannot enter again.
    winners: HashSet<String>,
}

impl Giveaway {
    pub fn new(keyword: Keyword, mode: MatchMode) -> Self {
        Self {
            keyword,
            mode,
            state: GiveawayState::Closed,
            entrants: IndexMap::new(),
            winners: HashSet::new(),
        }
    }

    pub fn set_rule(&mut self, keyword: Keyword, mode: MatchMode) {
        self.keyword = keyword;
        self.mode = mode;
    }

    pub fn state(&self) -> GiveawayState {
        self.state
    }

    pub fn open(&mut self) {
        self.state = GiveawayState::Open;
    }

    pub fn close(&mut self) {
        self.state = GiveawayState::Closed;
    }

    /// Clears entrants and past winners; the open/closed state stays.
    pub fn reset(&mut self) {
        self.entrants.clear();
        self.winners.clear();
    }

    /// Entrants in the order they entered.
    pub fn entrants(&self) -> impl ExactSizeIterator<Item = &Entrant> {
        self.entrants.values()
    }

    pub fn on_message(&mut self, msg: &ChatMessage, weights: &Weights) -> MessageOutcome {
        if msg.is_broadcaster() {
            return MessageOutcome::Ignored;
        }
        let roles = roles_from_badges(&msg.badges);
        if let Some(entrant) = self.entrants.get_mut(&msg.user_id) {
            entrant.roles = roles;
            entrant.display_name.clone_from(&msg.display_name);
            return MessageOutcome::AlreadyEntered;
        }
        let entering = self.state == GiveawayState::Open
            && !self.winners.contains(&msg.user_id)
            && weights.effective(&roles).1 > 0
            && matches(self.mode, &self.keyword, &msg.text);
        if !entering {
            return MessageOutcome::Ignored;
        }
        self.entrants.insert(
            msg.user_id.clone(),
            Entrant {
                user_id: msg.user_id.clone(),
                display_name: msg.display_name.clone(),
                roles,
            },
        );
        MessageOutcome::Entered
    }

    /// Picks a winner with probability proportional to their current weight and removes them for
    /// good: a winner cannot enter again until `reset`.
    pub fn draw<R: Rng + ?Sized>(&mut self, weights: &Weights, rng: &mut R) -> DrawOutcome {
        if self.entrants.is_empty() {
            return DrawOutcome::EmptyPool;
        }
        let ticket_weight = |e: &Entrant| u64::from(e.effective(weights).1);
        let total: u64 = self.entrants.values().map(ticket_weight).sum();
        if total == 0 {
            return DrawOutcome::NoDrawable;
        }
        let mut ticket = rng.random_range(0..total);
        let index = self
            .entrants
            .values()
            .position(|e| {
                let w = ticket_weight(e);
                if ticket < w {
                    return true;
                }
                ticket -= w;
                false
            })
            .expect("ticket is below the total weight");
        let (_, winner) = self
            .entrants
            .shift_remove_index(index)
            .expect("index comes from the same map");
        self.winners.insert(winner.user_id.clone());
        DrawOutcome::Winner(winner)
    }

    /// Drawable entrants (weight above 0) per effective role, in `Role::ALL` order.
    pub fn counts_by_role(&self, weights: &Weights) -> Vec<(Role, usize)> {
        Role::ALL
            .iter()
            .map(|&role| {
                let count = self
                    .entrants
                    .values()
                    .filter(|e| {
                        let (effective_role, weight) = e.effective(weights);
                        effective_role == role && weight > 0
                    })
                    .count();
                (role, count)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;
    use crate::domain::chat::Badge;

    fn msg(user_id: &str, text: &str, badges: &[(&str, &str)]) -> ChatMessage {
        ChatMessage {
            user_id: user_id.into(),
            display_name: format!("User{user_id}"),
            badges: badges.iter().map(|(n, v)| Badge::new(*n, *v)).collect(),
            text: text.into(),
        }
    }

    fn open_giveaway() -> Giveaway {
        let mut g = Giveaway::new(Keyword::parse("YEP").unwrap(), MatchMode::Word);
        g.open();
        g
    }

    fn ids(g: &Giveaway) -> Vec<&str> {
        g.entrants().map(|e| e.user_id.as_str()).collect()
    }

    #[test]
    fn new_giveaway_is_closed_and_open_close_switch_state() {
        let mut g = Giveaway::new(Keyword::parse("YEP").unwrap(), MatchMode::Word);
        assert_eq!(g.state(), GiveawayState::Closed);
        g.open();
        assert_eq!(g.state(), GiveawayState::Open);
        g.close();
        assert_eq!(g.state(), GiveawayState::Closed);
    }

    #[test]
    fn matching_message_enters_user() {
        let mut g = open_giveaway();
        let outcome = g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        assert_eq!(outcome, MessageOutcome::Entered);
        assert_eq!(ids(&g), ["1"]);
    }

    #[test]
    fn non_matching_message_is_ignored() {
        let mut g = open_giveaway();
        let outcome = g.on_message(&msg("1", "nope", &[]), &Weights::default());
        assert_eq!(outcome, MessageOutcome::Ignored);
        assert_eq!(g.entrants().len(), 0);
    }

    #[test]
    fn second_matching_message_from_same_user_is_not_a_second_entry() {
        let mut g = open_giveaway();
        g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        let outcome = g.on_message(&msg("1", "YEP YEP", &[]), &Weights::default());
        assert_eq!(outcome, MessageOutcome::AlreadyEntered);
        assert_eq!(ids(&g), ["1"]);
    }

    #[test]
    fn closed_giveaway_ignores_matching_messages() {
        let mut g = open_giveaway();
        g.close();
        let outcome = g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        assert_eq!(outcome, MessageOutcome::Ignored);
        assert_eq!(g.entrants().len(), 0);
    }

    #[test]
    fn reset_clears_entrants() {
        let mut g = open_giveaway();
        g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        g.reset();
        assert_eq!(g.entrants().len(), 0);
    }

    #[test]
    fn broadcaster_is_never_an_entrant() {
        let mut g = open_giveaway();
        let outcome = g.on_message(
            &msg("1", "YEP", &[("broadcaster", "1"), ("subscriber", "0")]),
            &Weights::default(),
        );
        assert_eq!(outcome, MessageOutcome::Ignored);
        assert_eq!(g.entrants().len(), 0);
    }

    #[test]
    fn user_whose_weight_is_zero_is_not_added() {
        let mut g = open_giveaway();
        let weights = Weights {
            pleb: 0,
            ..Weights::default()
        };
        let outcome = g.on_message(&msg("1", "YEP", &[]), &weights);
        assert_eq!(outcome, MessageOutcome::Ignored);
        assert_eq!(g.entrants().len(), 0);
    }

    #[test]
    fn later_message_updates_roles_even_without_keyword() {
        let mut g = open_giveaway();
        g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        g.on_message(
            &msg("1", "just subbed", &[("subscriber", "3000")]),
            &Weights::default(),
        );
        let entrant = g.entrants().next().unwrap();
        assert_eq!(entrant.roles, vec![Role::SubT3]);
    }

    #[test]
    fn entrant_dropped_to_weight_zero_stays_listed_but_is_not_drawn() {
        let mut g = open_giveaway();
        g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        g.on_message(&msg("2", "YEP", &[("vip", "1")]), &Weights::default());
        let weights = Weights {
            pleb: 0,
            ..Weights::default()
        };
        let mut rng = StdRng::seed_from_u64(7);
        assert_eq!(ids(&g), ["1", "2"]);
        match g.draw(&weights, &mut rng) {
            DrawOutcome::Winner(w) => assert_eq!(w.user_id, "2"),
            other => panic!("expected winner, got {other:?}"),
        }
        assert_eq!(g.draw(&weights, &mut rng), DrawOutcome::NoDrawable);
        assert_eq!(ids(&g), ["1"]);
    }

    #[test]
    fn draw_on_empty_pool_reports_empty_pool() {
        let mut g = open_giveaway();
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(
            g.draw(&Weights::default(), &mut rng),
            DrawOutcome::EmptyPool
        );
    }

    #[test]
    fn draw_removes_winner_so_redraw_picks_someone_else() {
        let mut g = open_giveaway();
        g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        g.on_message(&msg("2", "YEP", &[]), &Weights::default());
        let mut rng = StdRng::seed_from_u64(3);
        let DrawOutcome::Winner(first) = g.draw(&Weights::default(), &mut rng) else {
            panic!("expected a winner");
        };
        let DrawOutcome::Winner(second) = g.draw(&Weights::default(), &mut rng) else {
            panic!("expected a second winner");
        };
        assert_ne!(first.user_id, second.user_id);
        assert_eq!(
            g.draw(&Weights::default(), &mut rng),
            DrawOutcome::EmptyPool
        );
    }

    #[test]
    fn draw_follows_weights() {
        let weights = Weights {
            pleb: 1,
            sub_t1: 3,
            ..Weights::default()
        };
        let mut pool = open_giveaway();
        pool.on_message(&msg("pleb", "YEP", &[]), &weights);
        pool.on_message(&msg("sub", "YEP", &[("subscriber", "1")]), &weights);
        let mut rng = StdRng::seed_from_u64(42);
        let rounds = 10_000;
        let sub_wins = (0..rounds)
            .filter(|_| {
                let mut g = pool.clone();
                matches!(g.draw(&weights, &mut rng), DrawOutcome::Winner(w) if w.user_id == "sub")
            })
            .count();
        let share = sub_wins as f64 / rounds as f64;
        assert!(
            (0.72..0.78).contains(&share),
            "sub share {share}, expected ~0.75"
        );
    }

    #[test]
    fn winner_cannot_enter_again_until_reset() {
        let mut g = open_giveaway();
        g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        let mut rng = StdRng::seed_from_u64(5);
        assert!(matches!(
            g.draw(&Weights::default(), &mut rng),
            DrawOutcome::Winner(_)
        ));
        let outcome = g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        assert_eq!(outcome, MessageOutcome::Ignored);
        g.reset();
        let outcome = g.on_message(&msg("1", "YEP", &[]), &Weights::default());
        assert_eq!(outcome, MessageOutcome::Entered);
    }

    #[test]
    fn new_rule_replaces_old_one() {
        let mut g = open_giveaway();
        g.set_rule(Keyword::parse("!join").unwrap(), MatchMode::Exact);
        let weights = Weights::default();
        assert_eq!(
            g.on_message(&msg("1", "YEP", &[]), &weights),
            MessageOutcome::Ignored
        );
        assert_eq!(
            g.on_message(&msg("2", "!JOIN", &[]), &weights),
            MessageOutcome::Entered
        );
    }

    #[test]
    fn huge_weights_do_not_overflow() {
        let weights = Weights {
            pleb: u32::MAX,
            ..Weights::default()
        };
        let mut g = open_giveaway();
        g.on_message(&msg("1", "YEP", &[]), &weights);
        g.on_message(&msg("2", "YEP", &[]), &weights);
        g.on_message(&msg("3", "YEP", &[]), &weights);
        let mut rng = StdRng::seed_from_u64(9);
        assert!(matches!(g.draw(&weights, &mut rng), DrawOutcome::Winner(_)));
    }

    #[test]
    fn counts_by_role_counts_drawable_entrants_by_effective_role() {
        let mut g = open_giveaway();
        let weights = Weights::default();
        g.on_message(&msg("1", "YEP", &[]), &weights);
        g.on_message(&msg("2", "YEP", &[]), &weights);
        g.on_message(
            &msg("3", "YEP", &[("subscriber", "3001"), ("moderator", "1")]),
            &weights,
        );
        g.on_message(&msg("4", "YEP", &[("vip", "1")]), &weights);
        let weights = Weights { vip: 0, ..weights };
        let counts = g.counts_by_role(&weights);
        assert_eq!(
            counts,
            vec![
                (Role::Pleb, 2),
                (Role::SubT1, 0),
                (Role::SubT2, 0),
                (Role::SubT3, 1),
                (Role::Vip, 0),
                (Role::Mod, 0),
            ]
        );
    }
}
