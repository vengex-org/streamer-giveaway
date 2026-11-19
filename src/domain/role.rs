use serde::{Deserialize, Serialize};

use super::chat::{Badge, YouTubeBadge};

/// A chatter's standing in the channel; each role has its own weight. Order = rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Pleb,
    SubT1,
    SubT2,
    SubT3,
    Vip,
    Mod,
}

impl Role {
    pub const ALL: [Role; 6] = [
        Role::Pleb,
        Role::SubT1,
        Role::SubT2,
        Role::SubT3,
        Role::Vip,
        Role::Mod,
    ];
}

/// Roles of a chatter from their badges, in `Role` order. A chatter with no other role is a Pleb.
pub fn roles_from_badges(badges: &[Badge]) -> Vec<Role> {
    ranked(badges.iter().filter_map(|badge| match badge.name.as_str() {
        "subscriber" | "founder" => Some(sub_tier(&badge.version)),
        "vip" => Some(Role::Vip),
        "moderator" | "lead_moderator" => Some(Role::Mod),
        _ => None,
    }))
}

/// Roles of a Kick chatter from their badge types. Kick subs have no tiers, so subscriber,
/// founder and OG all count as Sub tier 1.
pub fn kick_roles<'a>(kinds: impl Iterator<Item = &'a str>) -> Vec<Role> {
    ranked(kinds.filter_map(|kind| match kind {
        "subscriber" | "founder" | "og" => Some(Role::SubT1),
        "vip" => Some(Role::Vip),
        "moderator" => Some(Role::Mod),
        _ => None,
    }))
}

/// Roles of a YouTube chatter. Membership levels are names each channel picks, with no order,
/// so every member counts as Sub tier 1.
pub fn youtube_roles(badges: &[YouTubeBadge]) -> Vec<Role> {
    ranked(badges.iter().filter_map(|badge| match badge {
        YouTubeBadge::Member(_) => Some(Role::SubT1),
        YouTubeBadge::Moderator => Some(Role::Mod),
        YouTubeBadge::Owner => None,
    }))
}

/// Sorted, without duplicates; a chatter with no role is a Pleb.
fn ranked(roles: impl Iterator<Item = Role>) -> Vec<Role> {
    let mut roles: Vec<Role> = roles.collect();
    roles.sort();
    roles.dedup();
    if roles.is_empty() {
        roles.push(Role::Pleb);
    }
    roles
}

/// Twitch encodes the tier in the subscriber badge version: `3xxx` tier 3, `2xxx` tier 2,
/// months of a tier 1 sub otherwise.
fn sub_tier(version: &str) -> Role {
    match version.parse::<u32>() {
        Ok(3000..) => Role::SubT3,
        Ok(2000..) => Role::SubT2,
        _ => Role::SubT1,
    }
}

/// Number of draw tickets per role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weights {
    pub pleb: u32,
    pub sub_t1: u32,
    pub sub_t2: u32,
    pub sub_t3: u32,
    pub vip: u32,
    #[serde(rename = "mod")]
    pub moderator: u32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            pleb: 1,
            sub_t1: 2,
            sub_t2: 3,
            sub_t3: 4,
            vip: 2,
            moderator: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("weight must be a whole number from 0 to {max}, got '{0}'", max = u32::MAX)]
pub struct InvalidWeight(pub String);

impl Weights {
    /// Parses a weight typed by the user: a whole number, surrounding whitespace allowed.
    pub fn parse_weight(raw: &str) -> Result<u32, InvalidWeight> {
        raw.trim()
            .parse()
            .map_err(|_| InvalidWeight(raw.to_owned()))
    }

    pub fn get(&self, role: Role) -> u32 {
        match role {
            Role::Pleb => self.pleb,
            Role::SubT1 => self.sub_t1,
            Role::SubT2 => self.sub_t2,
            Role::SubT3 => self.sub_t3,
            Role::Vip => self.vip,
            Role::Mod => self.moderator,
        }
    }

    pub fn set(&mut self, role: Role, weight: u32) {
        let slot = match role {
            Role::Pleb => &mut self.pleb,
            Role::SubT1 => &mut self.sub_t1,
            Role::SubT2 => &mut self.sub_t2,
            Role::SubT3 => &mut self.sub_t3,
            Role::Vip => &mut self.vip,
            Role::Mod => &mut self.moderator,
        };
        *slot = weight;
    }

    /// The role that gives the most tickets, and its weight. A tie goes to the higher role.
    pub fn effective(&self, roles: &[Role]) -> (Role, u32) {
        roles
            .iter()
            .map(|&role| (role, self.get(role)))
            .max_by_key(|&(role, weight)| (weight, role))
            .unwrap_or((Role::Pleb, self.pleb))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn badges(list: &[(&str, &str)]) -> Vec<Badge> {
        list.iter().map(|(n, v)| Badge::new(*n, *v)).collect()
    }

    #[test]
    fn no_badges_is_pleb() {
        assert_eq!(roles_from_badges(&[]), vec![Role::Pleb]);
        assert_eq!(
            roles_from_badges(&badges(&[("premium", "1")])),
            vec![Role::Pleb]
        );
    }

    #[test]
    fn subscriber_badge_version_sets_tier() {
        let tier = |v| roles_from_badges(&badges(&[("subscriber", v)]));
        assert_eq!(tier("12"), vec![Role::SubT1]);
        assert_eq!(tier("2001"), vec![Role::SubT2]);
        assert_eq!(tier("3000"), vec![Role::SubT3]);
        assert_eq!(tier("3024"), vec![Role::SubT3]);
    }

    #[test]
    fn non_numeric_subscriber_version_is_tier1() {
        assert_eq!(
            roles_from_badges(&badges(&[("subscriber", "x")])),
            vec![Role::SubT1]
        );
    }

    #[test]
    fn founder_is_tier1_subscriber() {
        assert_eq!(
            roles_from_badges(&badges(&[("founder", "0")])),
            vec![Role::SubT1]
        );
    }

    #[test]
    fn several_roles_are_all_kept() {
        let roles = roles_from_badges(&badges(&[
            ("moderator", "1"),
            ("vip", "1"),
            ("subscriber", "2006"),
        ]));
        assert_eq!(roles, vec![Role::SubT2, Role::Vip, Role::Mod]);
    }

    #[test]
    fn lead_moderator_is_mod() {
        assert_eq!(
            roles_from_badges(&badges(&[("lead_moderator", "1")])),
            vec![Role::Mod]
        );
    }

    #[test]
    fn weight_text_accepts_whole_numbers_with_spaces() {
        assert_eq!(Weights::parse_weight(" 3 "), Ok(3));
        assert_eq!(Weights::parse_weight("0"), Ok(0));
        assert_eq!(Weights::parse_weight("4294967295"), Ok(u32::MAX));
    }

    #[test]
    fn weight_text_rejects_empty_negative_fraction_and_overflow() {
        for raw in ["", " ", "-1", "1.5", "4294967296", "abc"] {
            assert!(
                Weights::parse_weight(raw).is_err(),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn effective_weight_is_max_of_roles() {
        let w = Weights {
            sub_t2: 5,
            vip: 7,
            moderator: 1,
            ..Weights::default()
        };
        assert_eq!(
            w.effective(&[Role::SubT2, Role::Vip, Role::Mod]),
            (Role::Vip, 7)
        );
    }

    #[test]
    fn effective_tie_prefers_higher_role() {
        let w = Weights {
            vip: 3,
            moderator: 3,
            ..Weights::default()
        };
        assert_eq!(w.effective(&[Role::Vip, Role::Mod]), (Role::Mod, 3));
    }

    #[test]
    fn kick_subs_founders_and_og_are_sub_tier_1() {
        assert_eq!(kick_roles(["og"].into_iter()), [Role::SubT1]);
        assert_eq!(
            kick_roles(["subscriber", "founder", "vip", "moderator", "sub_gifter"].into_iter()),
            [Role::SubT1, Role::Vip, Role::Mod]
        );
        assert_eq!(
            kick_roles(["verified", "sub_gifter"].into_iter()),
            [Role::Pleb]
        );
    }

    #[test]
    fn youtube_members_are_sub_tier_1_and_the_owner_has_no_role() {
        assert_eq!(
            youtube_roles(&[YouTubeBadge::Member(None), YouTubeBadge::Moderator]),
            [Role::SubT1, Role::Mod]
        );
        assert_eq!(youtube_roles(&[YouTubeBadge::Owner]), [Role::Pleb]);
        assert_eq!(youtube_roles(&[]), [Role::Pleb]);
    }
}
