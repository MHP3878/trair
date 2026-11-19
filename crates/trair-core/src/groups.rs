// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Groups (M5): who may change a group, kept inside the MLS group itself.
//!
//! The state of a group (name, owner, admins, guests, when it ends) is an
//! extension of the MLS group context. Every member's keys depend on it:
//! members with a different state cannot read each other. It changes only
//! through a commit, and every device checks each commit against the
//! rules here before it follows it ([`check_change`]). The server takes
//! changes only from the accounts in [`GroupState::committers`], so a
//! member who is not an admin cannot block a group with a forbidden
//! change either.
//!
//! The rules:
//! - Only the owner and admins change a group (add, remove, rename, end).
//! - Admins remove members and guests; only the owner removes admins.
//!   Everyone may remove their own other devices.
//! - Only the owner names admins, hands the group on or closes it. An
//!   admin may step down itself.
//! - Owner, admins and guests must be members, each account under one key.
//!
//! A join chat ([`GroupKind::Join`]) is a short two-account chat that only
//! carries the token of a group invitation key to an admin.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// MLS extension type of the Trair group state (private-use range).
pub const TRAIR_EXTENSION: u16 = 0xF7A1;

/// Longest group name, in characters.
pub const MAX_GROUP_NAME_CHARS: usize = 64;

/// Most admins of one group.
pub const MAX_ADMINS: usize = 100;

/// Most guests of one group at a time.
pub const MAX_GUESTS: usize = 1000;

/// Largest encoded state.
pub const MAX_STATE_BYTES: usize = 64 * 1024;

/// An account: user name plus the fingerprint of its account key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acct {
    /// User name on the server.
    pub user: String,
    /// Fingerprint of the account key.
    pub fp: String,
}

/// Someone who leaves the group again at `until`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guest {
    /// User name.
    pub user: String,
    /// Seconds since 1970.
    pub until: u64,
}

/// What kind of MLS group this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
    /// A group chat.
    Group,
    /// A join chat: brings an invitation key's token to an admin.
    Join,
}

/// The state of a group, as all members hold it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupState {
    /// Format version, 1.
    pub v: u32,
    /// Group or join chat.
    pub kind: GroupKind,
    /// Name of the group.
    #[serde(default)]
    pub name: String,
    /// The owner (for a join chat: the account that wants to join).
    pub owner: Acct,
    /// Admins besides the owner.
    #[serde(default)]
    pub admins: Vec<Acct>,
    /// When the group vanishes for everyone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends: Option<u64>,
    /// Guests and when they leave.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guests: Vec<Guest>,
    /// The owner closed the group: everyone deletes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
}

/// Someone's role in a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Created the group or got it handed on.
    Owner,
    /// May add and remove members and change the group.
    Admin,
    /// Writes and reads.
    Member,
    /// Writes and reads until a time, then leaves.
    Guest,
}

impl Role {
    /// Name for the app.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Guest => "guest",
        }
    }
}

/// The accounts in a group: user name → account fingerprint.
pub type Accounts = BTreeMap<String, String>;

fn refuse(text: impl Into<String>) -> CoreError {
    CoreError::NotAllowed(text.into())
}

/// A group name as stored: trimmed, without control characters, at most
/// [`MAX_GROUP_NAME_CHARS`].
pub fn clean_group_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_GROUP_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

impl GroupState {
    /// A new group with `owner` as its only admin.
    pub fn new_group(name: &str, owner: Acct, ends: Option<u64>) -> Result<Self> {
        let name = clean_group_name(name);
        if name.is_empty() {
            return Err(CoreError::Rejected("a group needs a name".into()));
        }
        Ok(GroupState {
            v: 1,
            kind: GroupKind::Group,
            name,
            owner,
            admins: Vec::new(),
            ends,
            guests: Vec::new(),
            closed: false,
        })
    }

    /// The state of a join chat started by `joiner`.
    pub fn new_join(joiner: Acct) -> Self {
        GroupState {
            v: 1,
            kind: GroupKind::Join,
            name: String::new(),
            owner: joiner,
            admins: Vec::new(),
            ends: None,
            guests: Vec::new(),
            closed: false,
        }
    }

    /// Encoded for the MLS extension.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Read a state from the MLS extension.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_STATE_BYTES {
            return Err(CoreError::Rejected("group state too large".into()));
        }
        let s: GroupState = serde_json::from_slice(bytes)
            .map_err(|_| CoreError::Rejected("unreadable group state".into()))?;
        if s.v != 1 {
            return Err(CoreError::Rejected(format!(
                "group state version {} is unknown",
                s.v
            )));
        }
        Ok(s)
    }

    /// Owner or admin.
    pub fn is_admin(&self, a: &Acct) -> bool {
        self.owner == *a || self.admins.contains(a)
    }

    /// Someone's role.
    pub fn role(&self, a: &Acct) -> Role {
        if self.owner == *a {
            Role::Owner
        } else if self.admins.contains(a) {
            Role::Admin
        } else if self.guests.iter().any(|g| g.user == a.user) {
            Role::Guest
        } else {
            Role::Member
        }
    }

    /// When a guest leaves, if `user` is one.
    pub fn guest_until(&self, user: &str) -> Option<u64> {
        self.guests.iter().find(|g| g.user == user).map(|g| g.until)
    }

    /// The accounts the server lets change the group: owner and admins
    /// (for a join chat: both sides).
    pub fn committers(&self, members: &Accounts) -> Vec<String> {
        let mut out: Vec<String> = match self.kind {
            GroupKind::Group => std::iter::once(&self.owner)
                .chain(&self.admins)
                .map(|a| a.user.clone())
                .collect(),
            GroupKind::Join => members.keys().cloned().collect(),
        };
        out.sort();
        out.dedup();
        out
    }

    /// Is `a` a member under this key?
    fn holds(members: &Accounts, a: &Acct) -> bool {
        members.get(&a.user) == Some(&a.fp)
    }

    /// The state is well-formed for these members.
    pub fn check_shape(&self, members: &Accounts) -> Result<()> {
        if self.v != 1 {
            return Err(CoreError::Rejected("unknown group state version".into()));
        }
        if self.name != clean_group_name(&self.name) {
            return Err(CoreError::Rejected("bad group name".into()));
        }
        if self.admins.len() > MAX_ADMINS || self.guests.len() > MAX_GUESTS {
            return Err(CoreError::Rejected("too many admins or guests".into()));
        }
        if self.kind == GroupKind::Join {
            if !self.name.is_empty()
                || !self.admins.is_empty()
                || !self.guests.is_empty()
                || self.ends.is_some()
                || self.closed
            {
                return Err(CoreError::Rejected("bad join chat".into()));
            }
            return Ok(());
        }
        if self.name.is_empty() {
            return Err(CoreError::Rejected("a group needs a name".into()));
        }
        if self.closed {
            return Ok(());
        }
        if !Self::holds(members, &self.owner) {
            return Err(CoreError::Rejected("the owner is not in the group".into()));
        }
        for (i, a) in self.admins.iter().enumerate() {
            if !Self::holds(members, a)
                || a.user == self.owner.user
                || self.admins[..i].iter().any(|b| b.user == a.user)
            {
                return Err(CoreError::Rejected(format!("bad admin {}", a.user)));
            }
        }
        for (i, g) in self.guests.iter().enumerate() {
            if !members.contains_key(&g.user)
                || g.user == self.owner.user
                || self.admins.iter().any(|a| a.user == g.user)
                || self.guests[..i].iter().any(|h| h.user == g.user)
                || g.until == 0
            {
                return Err(CoreError::Rejected(format!("bad guest {}", g.user)));
            }
        }
        Ok(())
    }
}

/// Check the state a welcome brings: well-formed, and `by` (who added
/// this device) may add people.
pub fn check_welcome(state: &GroupState, by: &Acct, members: &Accounts) -> Result<()> {
    state.check_shape(members)?;
    if state.closed {
        return Err(CoreError::Rejected("the group is closed".into()));
    }
    let ok = match state.kind {
        GroupKind::Group => state.is_admin(by),
        GroupKind::Join => GroupState::holds(members, by),
    };
    if ok {
        Ok(())
    } else {
        Err(refuse(format!("{} may not add anyone here", by.user)))
    }
}

/// Check a change of a group before following it.
///
/// `committer` made the change; `removed` are the users of the removed
/// devices (one entry per device); `members` are the accounts after it.
pub fn check_change(
    old: &GroupState,
    new: &GroupState,
    committer: &Acct,
    removed: &[String],
    members: &Accounts,
) -> Result<()> {
    if new.kind != old.kind {
        return Err(refuse("the kind of a group cannot change"));
    }
    if old.kind == GroupKind::Join {
        if new != old {
            return Err(refuse("a join chat does not change"));
        }
        return new.check_shape(members);
    }
    if old.closed {
        return Err(refuse("the group is closed"));
    }
    let is_owner = old.owner == *committer;
    if !is_owner && !old.admins.contains(committer) {
        return Err(refuse(format!(
            "{} is not an admin of this group",
            committer.user
        )));
    }
    for user in removed {
        let target_is_admin = *user == old.owner.user || old.admins.iter().any(|a| a.user == *user);
        if *user != committer.user && !is_owner && target_is_admin {
            return Err(refuse(format!("only the owner removes {user}")));
        }
    }
    if new.owner != old.owner && !is_owner {
        return Err(refuse("only the owner hands the group on"));
    }
    if new.admins != old.admins && !is_owner {
        // An admin may only step down itself.
        let stepped_down: Vec<Acct> = old
            .admins
            .iter()
            .filter(|a| *a != committer)
            .cloned()
            .collect();
        if new.admins != stepped_down {
            return Err(refuse("only the owner names admins"));
        }
    }
    if new.closed && !is_owner {
        return Err(refuse("only the owner closes the group"));
    }
    new.check_shape(members)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct(user: &str) -> Acct {
        Acct {
            user: user.into(),
            fp: format!("fp-{user}"),
        }
    }

    fn members(users: &[&str]) -> Accounts {
        users
            .iter()
            .map(|u| (u.to_string(), format!("fp-{u}")))
            .collect()
    }

    fn group() -> GroupState {
        let mut s = GroupState::new_group("  Abi\n2026  ", acct("olga"), None).unwrap();
        s.admins.push(acct("ada"));
        s.guests.push(Guest {
            user: "gus".into(),
            until: 99,
        });
        s
    }

    const ALL: &[&str] = &["olga", "ada", "max", "gus"];

    #[test]
    fn names_are_cleaned_and_the_state_reads_back() {
        let s = group();
        assert_eq!(s.name, "Abi2026");
        assert_eq!(GroupState::parse(&s.to_bytes()).unwrap(), s);
        assert!(GroupState::new_group(" \n ", acct("olga"), None).is_err());
        let long = "x".repeat(200);
        assert_eq!(
            GroupState::new_group(&long, acct("olga"), None)
                .unwrap()
                .name
                .chars()
                .count(),
            MAX_GROUP_NAME_CHARS
        );
        assert!(
            GroupState::parse(br#"{"v":2,"kind":"group","owner":{"user":"a","fp":"b"}}"#).is_err()
        );
        assert!(
            GroupState::parse(br#"{"v":1,"kind":"group","owner":{"user":"a","fp":"b"},"x":1}"#)
                .is_err()
        );
        assert!(GroupState::parse(&vec![b' '; MAX_STATE_BYTES + 1]).is_err());
    }

    #[test]
    fn roles_and_committers() {
        let s = group();
        assert_eq!(s.role(&acct("olga")), Role::Owner);
        assert_eq!(s.role(&acct("ada")), Role::Admin);
        assert_eq!(s.role(&acct("gus")), Role::Guest);
        assert_eq!(s.role(&acct("max")), Role::Member);
        // The same name with another key is not the admin.
        let fake = Acct {
            user: "ada".into(),
            fp: "other".into(),
        };
        assert!(!s.is_admin(&fake));
        assert_eq!(s.committers(&members(ALL)), ["ada", "olga"]);
        assert_eq!(s.guest_until("gus"), Some(99));
    }

    #[test]
    fn only_admins_change_a_group() {
        let old = group();
        let mut new = old.clone();
        new.name = "Neu".into();
        let m = members(ALL);
        check_change(&old, &new, &acct("olga"), &[], &m).unwrap();
        check_change(&old, &new, &acct("ada"), &[], &m).unwrap();
        for who in ["max", "gus"] {
            assert!(matches!(
                check_change(&old, &new, &acct(who), &[], &m),
                Err(CoreError::NotAllowed(_))
            ));
        }
        // Ada's name with another key is nobody.
        let fake = Acct {
            user: "ada".into(),
            fp: "evil".into(),
        };
        assert!(check_change(&old, &new, &fake, &[], &m).is_err());
    }

    #[test]
    fn admins_remove_members_only_the_owner_removes_admins() {
        let old = group();
        // Ada removes Max: fine.
        check_change(
            &old,
            &old,
            &acct("ada"),
            &["max".into()],
            &members(&["olga", "ada", "gus"]),
        )
        .unwrap();
        // Ada removes Gus and drops the guest entry: fine.
        let mut no_gus = old.clone();
        no_gus.guests.clear();
        check_change(
            &old,
            &no_gus,
            &acct("ada"),
            &["gus".into()],
            &members(&["olga", "ada", "max"]),
        )
        .unwrap();
        // Removing Gus but keeping him as guest: the state is wrong.
        assert!(
            check_change(
                &old,
                &old,
                &acct("ada"),
                &["gus".into()],
                &members(&["olga", "ada", "max"])
            )
            .is_err()
        );
        // Ada may not remove Olga.
        assert!(check_change(&old, &old, &acct("ada"), &["olga".into()], &members(ALL)).is_err());
        // Olga removes Ada and takes her off the admins.
        let mut no_ada = old.clone();
        no_ada.admins.clear();
        check_change(
            &old,
            &no_ada,
            &acct("olga"),
            &["ada".into()],
            &members(&["olga", "max", "gus"]),
        )
        .unwrap();
        // Everyone may remove their own other devices.
        check_change(&old, &old, &acct("ada"), &["ada".into()], &members(ALL)).unwrap();
    }

    #[test]
    fn only_the_owner_names_admins_and_hands_on() {
        let old = group();
        let m = members(ALL);
        let mut more = old.clone();
        more.admins.push(acct("max"));
        check_change(&old, &more, &acct("olga"), &[], &m).unwrap();
        assert!(check_change(&old, &more, &acct("ada"), &[], &m).is_err());
        // Ada steps down: fine. Ada demotes nobody else.
        let mut down = old.clone();
        down.admins.clear();
        check_change(&old, &down, &acct("ada"), &[], &m).unwrap();
        let mut two = old.clone();
        two.admins.push(acct("max"));
        let mut only_ada = two.clone();
        only_ada.admins.retain(|a| a.user == "ada");
        assert!(check_change(&two, &only_ada, &acct("ada"), &[], &m).is_err());
        // Handing on and closing are the owner's.
        let mut handed = old.clone();
        handed.owner = acct("ada");
        handed.admins = vec![acct("olga")];
        check_change(&old, &handed, &acct("olga"), &[], &m).unwrap();
        assert!(check_change(&old, &handed, &acct("ada"), &[], &m).is_err());
        let mut closed = old.clone();
        closed.closed = true;
        check_change(&old, &closed, &acct("olga"), &[], &m).unwrap();
        assert!(check_change(&old, &closed, &acct("ada"), &[], &m).is_err());
        // After closing nothing changes any more.
        assert!(check_change(&closed, &closed, &acct("olga"), &[], &m).is_err());
    }

    #[test]
    fn the_shape_must_fit_the_members() {
        let s = group();
        s.check_shape(&members(ALL)).unwrap();
        // Owner or admin not in the group, or under another key.
        assert!(s.check_shape(&members(&["ada", "max", "gus"])).is_err());
        let mut m = members(ALL);
        m.insert("ada".into(), "evil".into());
        assert!(s.check_shape(&m).is_err());
        // A guest who is also an admin, or twice.
        let mut t = s.clone();
        t.guests.push(Guest {
            user: "ada".into(),
            until: 5,
        });
        assert!(t.check_shape(&members(ALL)).is_err());
        let mut u = s.clone();
        u.guests.push(u.guests[0].clone());
        assert!(u.check_shape(&members(ALL)).is_err());
        // The kind never changes; a join chat never changes at all.
        let join = GroupState::new_join(acct("max"));
        let jm = members(&["max", "olga"]);
        join.check_shape(&jm).unwrap();
        assert!(check_change(&s, &join, &acct("olga"), &[], &jm).is_err());
        let mut join2 = join.clone();
        join2.name = "x".into();
        assert!(check_change(&join, &join2, &acct("max"), &[], &jm).is_err());
        check_change(&join, &join, &acct("olga"), &[], &jm).unwrap();
    }

    #[test]
    fn welcomes_come_from_admins() {
        let s = group();
        let m = members(ALL);
        check_welcome(&s, &acct("ada"), &m).unwrap();
        assert!(check_welcome(&s, &acct("max"), &m).is_err());
        let mut closed = s.clone();
        closed.closed = true;
        assert!(check_welcome(&closed, &acct("olga"), &m).is_err());
        let join = GroupState::new_join(acct("max"));
        check_welcome(&join, &acct("max"), &members(&["max", "olga"])).unwrap();
        assert!(check_welcome(&join, &acct("eve"), &members(&["max", "olga"])).is_err());
    }
}
