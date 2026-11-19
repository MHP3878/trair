// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Group chats on this device (M5). The rules every device checks are in
//! [`crate::groups`]; this part keeps the history, the join requests and
//! what an admin's device still has to do.
//!
//! Joining with a group key: the joiner opens a short join chat with the
//! admin who made the key ([`Messenger::join_group`]) and sends the
//! token in it ([`Messenger::join_message`]). The admin's device checks
//! the token ([`Incoming::JoinRequest`]), adds the joiner's devices to the
//! group ([`Messenger::admit`]) and deletes the join chat. The joiner's
//! device takes the welcome of that admin as the answer.
//!
//! Leaving: a member sends "leave" and deletes the group's keys at once.
//! An admin's device removes the member's devices next time it tidies up
//! ([`Messenger::tidy_group`]). Owners and admins hand on or step down
//! first ([`Messenger::leave_prepare`]).

use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Value, json};

use crate::chats::{
    Chat, ChatKind, ChatState, Incoming, MAX_END_SECS, Messenger, PendingCommit, del_outbox,
    digest, insert_chat, insert_message, purge, put_outbox, random_chat_id, set_end, set_state,
    to_i64, to_u64, touch, user_of,
};
use crate::contacts::{ContactCode, InviteCode, InviteKind};
use crate::error::{CoreError, Result};
use crate::groups::{Accounts, Acct, GroupKind, GroupState, Guest, Role, clean_group_name};
use crate::mls::{KeyPackageOffer, Member};
use crate::policy::Trust;
use crate::store::sql;

/// A join chat that got no answer is deleted after this long.
pub const JOIN_WAIT_SECS: u64 = 2 * 86_400;

/// Sender of notes in a group's history (they have no user).
pub const NOTE_SENDER: &str = "trair";

/// A group's state and accounts before a change.
pub(crate) type Snapshot = Option<(GroupState, Accounts)>;

/// One member of a group, for the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    /// User name.
    pub user: String,
    /// Fingerprint of the account key.
    pub fp: String,
    /// Owner, admin, member or guest.
    pub role: Role,
    /// A guest leaves then.
    pub guest_until: Option<u64>,
    /// Device ids in the group.
    pub devices: Vec<String>,
    /// Left or asked to remove devices; an admin's device removes them.
    pub leaving: bool,
    /// The account key differs from the first one seen in this group.
    pub key_changed: bool,
    /// The weakest trust of its devices.
    pub trust: Trust,
}

/// A group, for the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupView {
    /// The chat.
    pub chat: String,
    /// The name.
    pub name: String,
    /// This account's role.
    pub me: Role,
    /// When the group vanishes for everyone.
    pub ends: Option<u64>,
    /// When it vanishes for this account (a guest leaves earlier).
    pub my_end: Option<u64>,
    /// Everyone, owner first, then admins, members, guests.
    pub members: Vec<GroupMember>,
}

/// What tidying up a group needs sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tidy {
    /// Nothing to do.
    Nothing,
    /// An admin's change for the server.
    Commit(PendingCommit),
    /// A member asks the admins to remove its devices.
    Message {
        /// The encrypted request.
        bytes: Vec<u8>,
        /// Its recipients.
        to: Vec<String>,
    },
}

/// A join request that waits for this admin's device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRequest {
    /// The join chat.
    pub chat: String,
    /// The group.
    pub group: String,
    /// Who wants to join.
    pub user: String,
    /// A guest leaves then.
    pub guest_until: Option<u64>,
    /// When the token arrived.
    pub at: u64,
}

/// A note in a group's history (who joined, left, was removed, ...).
pub(crate) fn note(tx: &Transaction, chat: &str, now: u64, what: &Value) -> Result<()> {
    insert_message(tx, chat, NOTE_SENDER, "", &what.to_string(), now, false, 0)?;
    touch(tx, chat, now)
}

fn pin(tx: &Transaction, chat: &str, user: &str, fp: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO group_pins (chat, user, fp) VALUES (?1, ?2, ?3)
         ON CONFLICT (chat, user) DO NOTHING",
        params![chat, user, fp],
    )
    .map_err(sql)?;
    Ok(())
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

/// Notes for the history: what changed between two states.
fn change_notes(
    old: &GroupState,
    old_accts: &Accounts,
    new: &GroupState,
    accts: &Accounts,
    by: &str,
) -> Vec<Value> {
    let mut out = Vec::new();
    let added: Vec<&String> = accts
        .keys()
        .filter(|u| !old_accts.contains_key(*u))
        .collect();
    if !added.is_empty() {
        out.push(json!({"event": "added", "users": added, "by": by}));
    }
    let removed: Vec<&String> = old_accts
        .keys()
        .filter(|u| !accts.contains_key(*u))
        .collect();
    if !removed.is_empty() {
        out.push(json!({"event": "removed", "users": removed, "by": by}));
    }
    if new.name != old.name {
        out.push(json!({"event": "renamed", "name": new.name, "by": by}));
    }
    if new.owner != old.owner {
        out.push(json!({"event": "owner", "users": [new.owner.user], "by": by}));
    }
    let promoted: Vec<&String> = new
        .admins
        .iter()
        .filter(|a| !old.admins.contains(a) && **a != old.owner)
        .map(|a| &a.user)
        .collect();
    if !promoted.is_empty() {
        out.push(json!({"event": "admin", "users": promoted, "by": by}));
    }
    let demoted: Vec<&String> = old
        .admins
        .iter()
        .filter(|a| !new.admins.contains(a) && **a != new.owner && accts.contains_key(&a.user))
        .map(|a| &a.user)
        .collect();
    if !demoted.is_empty() {
        out.push(json!({"event": "not_admin", "users": demoted, "by": by}));
    }
    if new.ends != old.ends {
        out.push(json!({"event": "ends", "ends": new.ends, "by": by}));
    }
    out
}

impl Messenger {
    fn me(&self) -> Acct {
        self.device.acct()
    }

    /// State and accounts of a Trair group this device holds.
    pub(crate) fn group_snapshot(&self, chat: &str) -> Result<Snapshot> {
        if !self.device.has_group(chat) {
            return Ok(None);
        }
        match self.device.group_state(chat)? {
            Some(s) if s.kind == GroupKind::Group => Ok(Some((s, self.device.accounts(chat)?))),
            _ => Ok(None),
        }
    }

    /// When the group ends for this account: its end, or earlier if this
    /// account is a guest.
    fn my_end(&self, state: &GroupState) -> Option<u64> {
        match (state.ends, state.guest_until(&self.user)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// After this device followed a change of a group: notes in the
    /// history, keys of new members, the end time. A closed group goes.
    pub(crate) fn group_changed(
        &mut self,
        chat: &str,
        before: Snapshot,
        by: &str,
        now: u64,
    ) -> Result<()> {
        let Some((old, old_accts)) = before else {
            return Ok(());
        };
        let Some((new, accts)) = self.group_snapshot(chat)? else {
            return Ok(());
        };
        let id = chat.to_string();
        if new.closed {
            self.device.forget_group(chat)?;
            return self.save(move |tx| purge(tx, &id));
        }
        let notes = change_notes(&old, &old_accts, &new, &accts, by);
        let ends = self.my_end(&new);
        let present: Vec<String> = self
            .device
            .members(chat)?
            .into_iter()
            .map(|m| m.id)
            .collect();
        self.save(move |tx| {
            for n in &notes {
                note(tx, &id, now, n)?;
            }
            for (user, fp) in &accts {
                pin(tx, &id, user, fp)?;
            }
            set_end(tx, &id, ends.map(to_i64))?;
            // Devices that are gone need no removal any more.
            let leaving: Vec<String> = {
                let mut st = tx
                    .prepare("SELECT device FROM group_leaving WHERE chat = ?1")
                    .map_err(sql)?;
                let rows = st.query_map([&id], |r| r.get(0)).map_err(sql)?;
                rows.collect::<rusqlite::Result<_>>().map_err(sql)?
            };
            for d in leaving.iter().filter(|d| !present.contains(d)) {
                tx.execute(
                    "DELETE FROM group_leaving WHERE chat = ?1 AND device = ?2",
                    params![id, d],
                )
                .map_err(sql)?;
            }
            Ok(())
        })
    }

    /// A message from `user` counts: not from a guest whose time is up.
    pub(crate) fn may_write(&self, chat: &str, user: &str, now: u64) -> Result<bool> {
        if !self.device.has_group(chat) {
            return Ok(true);
        }
        Ok(match self.device.group_state(chat)? {
            Some(s) => s.guest_until(user).is_none_or(|u| u > now),
            None => true,
        })
    }

    /// Strict mode: no account key in the group differs from the one first
    /// seen there.
    pub(crate) fn check_group_keys(&self, chat: &str) -> Result<()> {
        if !self.device.policy().strict() {
            return Ok(());
        }
        for (user, fp) in self.device.accounts(chat)? {
            if self.pinned(chat, &user)?.is_some_and(|p| p != fp) {
                return Err(CoreError::Blocked(format!("the key of {user} changed")));
            }
        }
        Ok(())
    }

    fn pinned(&self, chat: &str, user: &str) -> Result<Option<String>> {
        self.store
            .conn()
            .query_row(
                "SELECT fp FROM group_pins WHERE chat = ?1 AND user = ?2",
                params![chat, user],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// Is this account owner or admin of the group?
    pub fn is_group_admin(&self, chat: &str) -> Result<bool> {
        Ok(self
            .device
            .group_state(chat)?
            .is_some_and(|s| s.is_admin(&self.me())))
    }

    /// This device joined a Trair group or a join chat.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn joined_group(
        &mut self,
        group: String,
        members: Vec<Member>,
        by: Member,
        via_last_resort: bool,
        state: GroupState,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        let own = matches!(by.trust, Trust::Own | Trust::OwnAccount);
        let accts = self.device.accounts(&group)?;
        match state.kind {
            GroupKind::Join => {
                let others: Vec<&String> = accts.keys().filter(|u| **u != self.user).collect();
                // Own join chat (made on another own device) or someone
                // else's request to this admin.
                let (kind, fits) = if own {
                    (ChatKind::Join, state.owner == self.me())
                } else {
                    (
                        ChatKind::Admit,
                        others.len() == 1 && state.owner.user == *others[0],
                    )
                };
                if others.len() != 1 || !fits {
                    self.device.forget_group(&group)?;
                    self.save(mark)?;
                    return Err(CoreError::Rejected("broken join chat".into()));
                }
                let chat = Chat {
                    id: group,
                    peer: others[0].clone(),
                    kind,
                    state: ChatState::Active,
                    created: now,
                    last: now,
                };
                let c = chat.clone();
                self.save(move |tx| {
                    mark(tx)?;
                    insert_chat(tx, &c)?;
                    set_end(tx, &c.id, Some(to_i64(now + JOIN_WAIT_SECS)))
                })?;
                if kind == ChatKind::Admit {
                    return Ok(Incoming::Other);
                }
                Ok(Incoming::Chat {
                    chat,
                    update_keys: false,
                })
            }
            GroupKind::Group => {
                // Was this account waiting to join through this admin?
                let waiting: Vec<String> = self
                    .all_chats()?
                    .into_iter()
                    .filter(|c| c.kind == ChatKind::Join && c.peer == by.user)
                    .map(|c| c.id)
                    .collect();
                let active = own || !waiting.is_empty() || self.contact(&by.user)?.is_some();
                for id in &waiting {
                    let _ = self.device.forget_group(id);
                }
                let chat = Chat {
                    id: group,
                    peer: String::new(),
                    kind: ChatKind::Group,
                    state: if active {
                        ChatState::Active
                    } else {
                        ChatState::Request
                    },
                    created: now,
                    last: now,
                };
                let c = chat.clone();
                let ends = self.my_end(&state);
                let adder = by.user.clone();
                let _ = members;
                self.save(move |tx| {
                    mark(tx)?;
                    for id in &waiting {
                        purge(tx, id)?;
                    }
                    insert_chat(tx, &c)?;
                    for (user, fp) in &accts {
                        pin(tx, &c.id, user, fp)?;
                    }
                    set_end(tx, &c.id, ends.map(to_i64))?;
                    note(tx, &c.id, now, &json!({"event": "joined", "by": adder}))
                })?;
                Ok(Incoming::Chat {
                    chat: self.chat(&chat.id)?,
                    update_keys: via_last_resort,
                })
            }
        }
    }
}

impl Messenger {
    /// Key packages may only add devices of this account or of contacts,
    /// and each under the key pinned for it (a swapped key is refused even
    /// without strict mode).
    fn check_contact_offers(&self, offers: &[KeyPackageOffer]) -> Result<()> {
        for a in self.device.offer_accounts(offers)? {
            let expected = if a.user == self.user {
                Some(self.me().fp)
            } else {
                self.contact(&a.user)?
                    .map(|c| c.fingerprint.as_str().to_string())
            };
            match expected {
                Some(fp) if fp == a.fp => {}
                Some(_) => {
                    return Err(CoreError::Blocked(format!(
                        "{} came with a different key",
                        a.user
                    )));
                }
                None => {
                    return Err(CoreError::Rejected(format!("{} is not a contact", a.user)));
                }
            }
        }
        Ok(())
    }

    fn group_chat(&self, chat: &str) -> Result<(Chat, GroupState)> {
        let c = self.chat(chat)?;
        if c.kind != ChatKind::Group {
            return Err(CoreError::Rejected("not a group".into()));
        }
        if c.state != ChatState::Active {
            return Err(CoreError::Rejected("not an active group".into()));
        }
        let s = self
            .device
            .group_state(chat)?
            .ok_or_else(|| CoreError::Rejected("not a group".into()))?;
        Ok((c, s))
    }

    /// Make a change of a group, check it, store it for the server.
    fn group_commit(
        &mut self,
        chat: &str,
        offers: &[KeyPackageOffer],
        remove: &[String],
        new_state: Option<GroupState>,
        allow: &[Acct],
    ) -> Result<PendingCommit> {
        let (_, old) = self.group_chat(chat)?;
        self.no_pending(chat)?;
        let to = self.commit_recipients(chat)?;
        let out = self
            .device
            .change(chat, offers, remove, new_state.as_ref(), allow)?;
        let mut p = self.pending(out, to);
        // The server learns who may change the group with every change.
        p.committers = Some(
            new_state
                .as_ref()
                .unwrap_or(&old)
                .committers(&Accounts::new()),
        );
        let q = p.clone();
        self.save(move |tx| put_outbox(tx, &q))?;
        Ok(p)
    }

    /// Start a group with this account as owner. `offers`: devices of
    /// contacts to add right away and this account's other devices (may
    /// be empty: invite later with a key). `ends`: the group vanishes then
    /// for everyone.
    pub fn create_group(
        &mut self,
        name: &str,
        ends: Option<u64>,
        offers: &[KeyPackageOffer],
        now: u64,
    ) -> Result<(Chat, Option<PendingCommit>)> {
        if let Some(e) = ends
            && (e <= now || e > now + MAX_END_SECS)
        {
            return Err(CoreError::Rejected("end time out of range".into()));
        }
        self.check_contact_offers(offers)?;
        let state = GroupState::new_group(name, self.me(), ends)?;
        let id = random_chat_id()?;
        self.device.create_trair_group(&id, &state)?;
        let to: Vec<String> = Vec::new();
        let pending = if offers.iter().any(|o| o.claimed_device != self.device_id()) {
            match self.device.change(&id, offers, &[], None, &[]) {
                Ok(out) => {
                    let mut p = self.pending(out, to);
                    p.committers = Some(state.committers(&Accounts::new()));
                    Some(p)
                }
                Err(e) => {
                    self.device.forget_group(&id)?;
                    self.save(|_| Ok(()))?;
                    return Err(e);
                }
            }
        } else {
            None
        };
        let chat = Chat {
            id,
            peer: String::new(),
            kind: ChatKind::Group,
            state: ChatState::Active,
            created: now,
            last: now,
        };
        let (c, p) = (chat.clone(), pending.clone());
        let me = self.me();
        self.save(move |tx| {
            insert_chat(tx, &c)?;
            pin(tx, &c.id, &me.user, &me.fp)?;
            set_end(tx, &c.id, ends.map(to_i64))?;
            note(tx, &c.id, now, &json!({"event": "created", "by": me.user}))?;
            if let Some(p) = &p {
                put_outbox(tx, p)?;
            }
            Ok(())
        })?;
        Ok((chat, pending))
    }

    /// A group with its members and roles.
    pub fn group_view(&self, chat: &str) -> Result<GroupView> {
        let c = self.chat(chat)?;
        if c.kind != ChatKind::Group {
            return Err(CoreError::Rejected("not a group".into()));
        }
        let state = self
            .device
            .group_state(chat)?
            .ok_or_else(|| CoreError::Rejected("not a group".into()))?;
        let accts = self.device.accounts(chat)?;
        let devices = self.device.members(chat)?;
        let leaving: Vec<String> = {
            let mut st = self
                .store
                .conn()
                .prepare("SELECT device FROM group_leaving WHERE chat = ?1")
                .map_err(sql)?;
            let rows = st.query_map([chat], |r| r.get(0)).map_err(sql)?;
            rows.collect::<rusqlite::Result<_>>().map_err(sql)?
        };
        let mut members = Vec::new();
        for (user, fp) in &accts {
            let acct = Acct {
                user: user.clone(),
                fp: fp.clone(),
            };
            let mine: Vec<&Member> = devices.iter().filter(|m| m.user == *user).collect();
            let trust = mine
                .iter()
                .map(|m| m.trust)
                .max_by_key(|t| trust_rank(*t))
                .unwrap_or(Trust::Unverified);
            members.push(GroupMember {
                user: user.clone(),
                fp: fp.clone(),
                role: state.role(&acct),
                guest_until: state.guest_until(user),
                devices: mine.iter().map(|m| m.id.clone()).collect(),
                leaving: mine.iter().any(|m| leaving.contains(&m.id)),
                key_changed: self.pinned(chat, user)?.is_some_and(|p| p != *fp),
                trust,
            });
        }
        members.sort_by_key(|m| {
            (
                match m.role {
                    Role::Owner => 0,
                    Role::Admin => 1,
                    Role::Member => 2,
                    Role::Guest => 3,
                },
                m.user.clone(),
            )
        });
        Ok(GroupView {
            chat: chat.to_string(),
            name: state.name.clone(),
            me: state.role(&self.me()),
            ends: state.ends,
            my_end: self.my_end(&state),
            members,
        })
    }

    /// Add contacts' devices (or this account's other devices) to a group.
    pub fn add_to_group(
        &mut self,
        chat: &str,
        offers: &[KeyPackageOffer],
    ) -> Result<PendingCommit> {
        self.check_contact_offers(offers)?;
        self.group_commit(chat, offers, &[], None, &[])
    }

    fn devices_of(&self, chat: &str, user: &str) -> Result<Vec<String>> {
        Ok(self
            .device
            .members(chat)?
            .into_iter()
            .filter(|m| m.user == user && m.trust != Trust::Own)
            .map(|m| m.id)
            .collect())
    }

    /// Remove someone from a group (admins: members and guests; the
    /// owner: anyone).
    pub fn kick(&mut self, chat: &str, user: &str) -> Result<PendingCommit> {
        if user == self.user {
            return Err(CoreError::Rejected("leave instead".into()));
        }
        let (_, mut state) = self.group_chat(chat)?;
        let devices = self.devices_of(chat, user)?;
        if devices.is_empty() {
            return Err(CoreError::Rejected(format!("{user} is not in the group")));
        }
        state.admins.retain(|a| a.user != user);
        state.guests.retain(|g| g.user != user);
        self.group_commit(chat, &[], &devices, Some(state), &[])
    }

    fn acct_in(&self, chat: &str, user: &str) -> Result<Acct> {
        let fp = self
            .device
            .accounts(chat)?
            .remove(user)
            .ok_or_else(|| CoreError::Rejected(format!("{user} is not in the group")))?;
        Ok(Acct {
            user: user.to_string(),
            fp,
        })
    }

    /// Make someone an admin or take it back (owner only; an admin may
    /// take back its own).
    pub fn set_admin(&mut self, chat: &str, user: &str, admin: bool) -> Result<PendingCommit> {
        let (_, mut state) = self.group_chat(chat)?;
        let acct = self.acct_in(chat, user)?;
        if acct == state.owner {
            return Err(CoreError::Rejected("the owner is always admin".into()));
        }
        state.admins.retain(|a| a.user != user);
        if admin {
            state.guests.retain(|g| g.user != user);
            state.admins.push(acct);
        }
        self.group_commit(chat, &[], &[], Some(state), &[])
    }

    /// Hand the group on to another member (owner only). The old owner
    /// stays as admin.
    pub fn hand_over(&mut self, chat: &str, user: &str) -> Result<PendingCommit> {
        let (_, mut state) = self.group_chat(chat)?;
        let acct = self.acct_in(chat, user)?;
        if acct == state.owner {
            return Err(CoreError::Rejected("already the owner".into()));
        }
        let old = state.owner.clone();
        state.admins.retain(|a| a.user != user);
        state.guests.retain(|g| g.user != user);
        state.admins.push(old);
        state.owner = acct;
        self.group_commit(chat, &[], &[], Some(state), &[])
    }

    /// Rename a group (admins).
    pub fn rename_group(&mut self, chat: &str, name: &str) -> Result<PendingCommit> {
        let (_, mut state) = self.group_chat(chat)?;
        let name = clean_group_name(name);
        if name.is_empty() {
            return Err(CoreError::Rejected("a group needs a name".into()));
        }
        state.name = name;
        self.group_commit(chat, &[], &[], Some(state), &[])
    }

    /// Let a group vanish for everyone at `ends` (`None`: it stays).
    pub fn set_group_end(
        &mut self,
        chat: &str,
        ends: Option<u64>,
        now: u64,
    ) -> Result<PendingCommit> {
        if let Some(e) = ends
            && (e <= now || e > now + MAX_END_SECS)
        {
            return Err(CoreError::Rejected("end time out of range".into()));
        }
        let (_, mut state) = self.group_chat(chat)?;
        state.ends = ends;
        self.group_commit(chat, &[], &[], Some(state), &[])
    }

    /// Close a group for everyone (owner only): every device deletes it.
    pub fn close_group(&mut self, chat: &str) -> Result<PendingCommit> {
        let (_, mut state) = self.group_chat(chat)?;
        state.closed = true;
        self.group_commit(chat, &[], &[], Some(state), &[])
    }

    /// Before leaving: an owner hands the group on (to an admin, else a
    /// member, else a guest), an admin steps down. `None`: nothing to do.
    pub fn leave_prepare(&mut self, chat: &str) -> Result<Option<PendingCommit>> {
        let (_, mut state) = self.group_chat(chat)?;
        let me = self.me();
        let accts = self.device.accounts(chat)?;
        if state.owner == me {
            let next = state.admins.first().cloned().or_else(|| {
                accts
                    .iter()
                    .filter(|(u, _)| **u != me.user && state.guest_until(u).is_none())
                    .chain(accts.iter().filter(|(u, _)| **u != me.user))
                    .map(|(u, fp)| Acct {
                        user: u.clone(),
                        fp: fp.clone(),
                    })
                    .next()
            });
            let Some(next) = next else {
                return Ok(None);
            };
            state.admins.retain(|a| *a != next);
            state.guests.retain(|g| g.user != next.user);
            state.owner = next;
        } else if state.admins.contains(&me) {
            state.admins.retain(|a| *a != me);
        } else {
            return Ok(None);
        }
        self.group_commit(chat, &[], &[], Some(state), &[])
            .map(Some)
    }

    /// Leave a group: the others are told, this device deletes the
    /// group's keys at once and keeps the history. Send the bytes to `to`.
    pub fn leave_group(&mut self, chat: &str, now: u64) -> Result<(Vec<u8>, Vec<String>)> {
        let c = self.chat(chat)?;
        if c.kind != ChatKind::Group || c.state == ChatState::Left {
            return Err(CoreError::Rejected("not a group to leave".into()));
        }
        let state = self.device.group_state(chat)?;
        let others = self.device.accounts(chat)?.len() > 1;
        if state.is_some_and(|s| s.is_admin(&self.me())) && others {
            return Err(CoreError::NotAllowed(
                "hand the group on or step down first".into(),
            ));
        }
        self.no_pending(chat)?;
        let bytes = self
            .device
            .encrypt_control_anyway(chat, &json!({"leave": true}))?;
        let to = self.commit_recipients(chat)?;
        self.device.forget_group(chat)?;
        let (id, me) = (chat.to_string(), self.user.clone());
        let declined = c.state == ChatState::Request;
        self.save(move |tx| {
            if declined {
                return purge(tx, &id);
            }
            set_state(tx, &id, ChatState::Left)?;
            set_end(tx, &id, None)?;
            note(tx, &id, now, &json!({"event": "left", "users": [me]}))
        })?;
        Ok((bytes, to))
    }

    /// Take a group someone added this account to.
    pub fn accept_group(&mut self, chat: &str) -> Result<()> {
        let c = self.chat(chat)?;
        if c.kind != ChatKind::Group || c.state != ChatState::Request {
            return Err(CoreError::Rejected("not a group request".into()));
        }
        let id = chat.to_string();
        self.save(move |tx| set_state(tx, &id, ChatState::Active))
    }

    /// Delete a group this device left (the history goes too).
    pub fn delete_chat(&mut self, chat: &str) -> Result<()> {
        let c = self.chat(chat)?;
        if c.state != ChatState::Left {
            return Err(CoreError::Rejected("leave it first".into()));
        }
        let id = chat.to_string();
        self.save(move |tx| purge(tx, &id))
    }

    /// Strict mode blocks a group where someone's key changed; after
    /// checking it, take the new key.
    pub fn accept_new_key(&mut self, chat: &str, user: &str) -> Result<()> {
        let acct = self.acct_in(chat, user)?;
        let id = chat.to_string();
        self.save(move |tx| {
            tx.execute(
                "INSERT INTO group_pins (chat, user, fp) VALUES (?1, ?2, ?3)
                 ON CONFLICT (chat, user) DO UPDATE SET fp = excluded.fp",
                params![id, acct.user, acct.fp],
            )
            .map_err(sql)?;
            Ok(())
        })
    }

    /// New keys for this device in a group where it is not an admin (after
    /// joining through the last-resort package): a proposal that the next
    /// admin's change carries. Send the bytes to `to`.
    pub fn propose_update(&mut self, chat: &str) -> Result<(Vec<u8>, Vec<String>)> {
        self.group_chat(chat)?;
        let bytes = self.device.propose_update(chat)?;
        let to = self.recipients(chat)?;
        self.save(|_| Ok(()))?;
        Ok((bytes, to))
    }

    /// What a group still needs. An admin's device removes members who
    /// left, guests whose time is up and withdrawn devices, and carries
    /// members' key updates. A member's device asks the admins to remove
    /// its own withdrawn devices.
    pub fn tidy_group(&mut self, chat: &str, now: u64) -> Result<Tidy> {
        let Ok((_, state)) = self.group_chat(chat) else {
            return Ok(Tidy::Nothing);
        };
        if self.pending_commit(chat)?.is_some() {
            return Ok(Tidy::Nothing);
        }
        let me = self.me();
        let members = self.device.members(chat)?;
        let leaving: Vec<String> = {
            let mut st = self
                .store
                .conn()
                .prepare("SELECT device FROM group_leaving WHERE chat = ?1")
                .map_err(sql)?;
            let rows = st.query_map([chat], |r| r.get(0)).map_err(sql)?;
            rows.collect::<rusqlite::Result<_>>().map_err(sql)?
        };
        if !state.is_admin(&me) {
            let own: Vec<String> = members
                .iter()
                .filter(|m| m.trust == Trust::Withdrawn && m.user == self.user)
                .filter(|m| !leaving.contains(&m.id))
                .map(|m| m.id.clone())
                .collect();
            if own.is_empty() {
                return Ok(Tidy::Nothing);
            }
            let bytes = self
                .device
                .encrypt_control_anyway(chat, &json!({"remove": own}))?;
            let to = self.commit_recipients(chat)?;
            let id = chat.to_string();
            self.save(move |tx| {
                for d in &own {
                    tx.execute(
                        "INSERT INTO group_leaving (chat, device) VALUES (?1, ?2)
                         ON CONFLICT DO NOTHING",
                        params![id, d],
                    )
                    .map_err(sql)?;
                }
                Ok(())
            })?;
            return Ok(Tidy::Message { bytes, to });
        }
        let is_owner = state.owner == me;
        let protected =
            |user: &str| user == state.owner.user || state.admins.iter().any(|a| a.user == user);
        let remove: Vec<String> = members
            .iter()
            .filter(|m| m.trust != Trust::Own)
            .filter(|m| {
                leaving.contains(&m.id)
                    || m.trust == Trust::Withdrawn
                    || state.guest_until(&m.user).is_some_and(|u| u <= now)
            })
            .filter(|m| m.user == self.user || is_owner || !protected(&m.user))
            .map(|m| m.id.clone())
            .collect();
        // Accounts with no device left lose their roles.
        let staying: Vec<&str> = members
            .iter()
            .filter(|m| !remove.contains(&m.id))
            .map(|m| m.user.as_str())
            .collect();
        let mut new = state.clone();
        new.admins.retain(|a| staying.contains(&a.user.as_str()));
        new.guests.retain(|g| staying.contains(&g.user.as_str()));
        if remove.is_empty() && new == state && !self.device.has_proposals(chat)? {
            return Ok(Tidy::Nothing);
        }
        let new_state = (new != state).then_some(new);
        Ok(Tidy::Commit(self.group_commit(
            chat,
            &[],
            &remove,
            new_state,
            &[],
        )?))
    }

    /// A group key: whoever brings it before `ends` joins (as a guest who
    /// leaves at `ends`, with `guest`). Admins only; usable many times.
    pub fn create_group_invite(
        &mut self,
        chat: &str,
        guest: bool,
        ends: u64,
        now: u64,
    ) -> Result<String> {
        let (_, state) = self.group_chat(chat)?;
        if !state.is_admin(&self.me()) {
            return Err(CoreError::NotAllowed("only admins make group keys".into()));
        }
        if ends <= now || ends > now + MAX_END_SECS {
            return Err(CoreError::Rejected("end time out of range".into()));
        }
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|e| CoreError::Internal(e.to_string()))?;
        let hash = digest(&token).to_vec();
        let (id, e) = (chat.to_string(), to_i64(ends));
        self.save(move |tx| {
            tx.execute(
                "INSERT INTO group_invites (hash, chat, guest, ends) VALUES (?1, ?2, ?3, ?4)",
                params![hash, id, i64::from(guest), e],
            )
            .map_err(sql)?;
            Ok(())
        })?;
        Ok(InviteCode {
            contact: ContactCode {
                user: self.user.clone(),
                fingerprint: self.device.account_fingerprint().clone(),
            },
            token,
            kind: if guest {
                InviteKind::Guest
            } else {
                InviteKind::Member
            },
            ends,
        }
        .text())
    }

    /// Take back every key of a group made on this device: whoever brings
    /// one is refused. Returns how many there were.
    pub fn revoke_group_invites(&mut self, chat: &str) -> Result<usize> {
        let id = chat.to_string();
        self.save(move |tx| {
            tx.execute(
                "UPDATE group_invites SET revoked = 1 WHERE chat = ?1 AND revoked = 0",
                [&id],
            )
            .map_err(sql)
        })
    }

    /// How many keys of a group made on this device still work, and
    /// until when the last one does.
    pub fn group_invites(&self, chat: &str, now: u64) -> Result<(usize, Option<u64>)> {
        let (n, last): (i64, Option<i64>) = self
            .store
            .conn()
            .query_row(
                "SELECT count(*), max(ends) FROM group_invites
                 WHERE chat = ?1 AND revoked = 0 AND ends > ?2",
                params![chat, to_i64(now)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)?;
        Ok((usize::try_from(n).unwrap_or(0), last.map(to_u64)))
    }

    /// Use a group key: open a join chat with the admin who made it.
    /// `offers`: key packages of the admin's devices (and this account's
    /// other devices). Send the commit; once the server took it, send
    /// [`Messenger::join_message`].
    pub fn join_group(
        &mut self,
        code: &str,
        offers: &[KeyPackageOffer],
        now: u64,
    ) -> Result<(Chat, PendingCommit)> {
        let invite = InviteCode::parse(code)?;
        if !invite.kind.is_group() {
            return Err(CoreError::Rejected("not a group key".into()));
        }
        if invite.ends <= now {
            return Err(CoreError::Rejected("this group key has expired".into()));
        }
        let admin = invite.contact.clone();
        if admin.user == self.user {
            return Err(CoreError::Rejected("that is your own key".into()));
        }
        if let Some(pin) = self.device.policy().verified(&admin.user)
            && *pin != admin.fingerprint
        {
            return Err(CoreError::Blocked(format!(
                "{} is known with a different key",
                admin.user
            )));
        }
        let accts = self.device.offer_accounts(offers)?;
        if !accts.iter().any(|a| a.user == admin.user) {
            return Err(CoreError::InvalidKeyPackage(format!(
                "no device of {} to ask",
                admin.user
            )));
        }
        for a in &accts {
            let fits = if a.user == admin.user {
                a.fp == admin.fingerprint.as_str()
            } else {
                a.user == self.user && a.fp == self.me().fp
            };
            if !fits {
                return Err(CoreError::Blocked(format!(
                    "{} does not belong in this join chat",
                    a.user
                )));
            }
        }
        // The key names the admin's account key: like a friendship key it
        // is the proof for strict mode.
        self.device.verify(&admin.user, admin.fingerprint.clone());
        let id = random_chat_id()?;
        self.device
            .create_trair_group(&id, &GroupState::new_join(self.me()))?;
        let out = match self.device.change(&id, offers, &[], None, &[]) {
            Ok(out) => out,
            Err(e) => {
                self.device.forget_group(&id)?;
                self.save(|_| Ok(()))?;
                return Err(e);
            }
        };
        let mut pending = self.pending(out, Vec::new());
        pending.committers = Some(sorted(vec![self.user.clone(), admin.user.clone()]));
        let chat = Chat {
            id,
            peer: admin.user,
            kind: ChatKind::Join,
            state: ChatState::Active,
            created: now,
            last: now,
        };
        let (c, p, token) = (chat.clone(), pending.clone(), invite.token.to_vec());
        let ends = to_i64(invite.ends);
        self.save(move |tx| {
            insert_chat(tx, &c)?;
            put_outbox(tx, &p)?;
            tx.execute(
                "INSERT INTO join_tokens (chat, token, ends) VALUES (?1, ?2, ?3)",
                params![c.id, token, ends],
            )
            .map_err(sql)?;
            set_end(tx, &c.id, Some(ends.saturating_add(to_i64(JOIN_WAIT_SECS))))
        })?;
        Ok((chat, pending))
    }

    /// The token for the admin, once the join chat exists on the server.
    /// `None` if it went out already.
    pub fn join_message(&mut self, chat: &str) -> Result<Option<(Vec<u8>, Vec<String>)>> {
        let c = self.chat(chat)?;
        if c.kind != ChatKind::Join {
            return Ok(None);
        }
        let token: Option<Vec<u8>> = self
            .store
            .conn()
            .query_row(
                "SELECT token FROM join_tokens WHERE chat = ?1",
                [chat],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        let Some(token) = token else {
            return Ok(None);
        };
        let bytes = self
            .device
            .encrypt_control(chat, &json!({"join": hex::encode(token)}))?;
        let to = self.recipients(chat)?;
        let id = chat.to_string();
        self.save(move |tx| {
            tx.execute("DELETE FROM join_tokens WHERE chat = ?1", [&id])
                .map_err(sql)?;
            Ok(())
        })?;
        Ok(Some((bytes, to)))
    }

    /// Join requests waiting for this admin's device.
    pub fn join_requests(&self) -> Result<Vec<JoinRequest>> {
        let mut st = self
            .store
            .conn()
            .prepare("SELECT chat, grp, user, guest_until, at FROM join_requests ORDER BY at, chat")
            .map_err(sql)?;
        let rows = st
            .query_map([], |r| {
                Ok(JoinRequest {
                    chat: r.get(0)?,
                    group: r.get(1)?,
                    user: r.get(2)?,
                    guest_until: r.get::<_, Option<i64>>(3)?.map(to_u64),
                    at: to_u64(r.get(4)?),
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// Add the one who brought a valid key: `offers` are key packages of
    /// that account's devices, each under the key it showed in the join
    /// chat. Then, once the server took it, [`Messenger::close_join`].
    pub fn admit(&mut self, join_chat: &str, offers: &[KeyPackageOffer]) -> Result<PendingCommit> {
        let req: Option<(String, String, String, Option<i64>)> = self
            .store
            .conn()
            .query_row(
                "SELECT grp, user, fp, guest_until FROM join_requests WHERE chat = ?1",
                [join_chat],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((group, user, fp, guest_until)) = req else {
            return Err(CoreError::Rejected("no such join request".into()));
        };
        let joiner = Acct { user, fp };
        let accts = self.device.offer_accounts(offers)?;
        if accts.is_empty() || accts.iter().any(|a| *a != joiner) {
            return Err(CoreError::Blocked(format!(
                "the devices offered are not {}'s",
                joiner.user
            )));
        }
        let (_, state) = self.group_chat(&group)?;
        if self.device.accounts(&group)?.contains_key(&joiner.user) {
            return Err(CoreError::Rejected(format!(
                "{} is in the group already",
                joiner.user
            )));
        }
        let new_state = guest_until.map(|until| {
            let mut s = state.clone();
            s.guests.push(Guest {
                user: joiner.user.clone(),
                until: to_u64(until),
            });
            s
        });
        self.group_commit(
            &group,
            offers,
            &[],
            new_state,
            std::slice::from_ref(&joiner),
        )
    }

    /// Delete a join chat on this device (after [`Messenger::admit`]).
    pub fn close_join(&mut self, join_chat: &str) -> Result<()> {
        let c = self.chat(join_chat)?;
        if !matches!(c.kind, ChatKind::Admit | ChatKind::Join) {
            return Err(CoreError::Rejected("not a join chat".into()));
        }
        let _ = self.device.forget_group(join_chat);
        let id = join_chat.to_string();
        self.save(move |tx| purge(tx, &id))
    }

    /// Tell the one who brought a key that does not work, and delete the
    /// join chat. Send the bytes to `to`.
    pub fn deny_join(&mut self, join_chat: &str) -> Result<(Vec<u8>, Vec<String>)> {
        let c = self.chat(join_chat)?;
        if c.kind != ChatKind::Admit {
            return Err(CoreError::Rejected("not a join request".into()));
        }
        let bytes = self
            .device
            .encrypt_control(join_chat, &json!({"join_denied": true}))?;
        let to = self.recipients(join_chat)?;
        self.close_join(join_chat)?;
        Ok((bytes, to))
    }

    /// A control message in a group or join chat.
    pub(crate) fn group_control(
        &mut self,
        chat: &Chat,
        user: &str,
        control: &Value,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        match chat.kind {
            ChatKind::Group if chat.state != ChatState::Left => {
                if control.get("leave").and_then(Value::as_bool) == Some(true) {
                    return self.member_left(chat, user, now, mark);
                }
                if let Some(ids) = control.get("remove").and_then(Value::as_array) {
                    let ids: Vec<String> = ids
                        .iter()
                        .filter_map(|v| v.as_str())
                        .filter(|d| user_of(d) == user)
                        .map(str::to_string)
                        .collect();
                    let id = chat.id.clone();
                    self.save(move |tx| {
                        mark(tx)?;
                        for d in &ids {
                            tx.execute(
                                "INSERT INTO group_leaving (chat, device) VALUES (?1, ?2)
                                 ON CONFLICT DO NOTHING",
                                params![id, d],
                            )
                            .map_err(sql)?;
                        }
                        Ok(())
                    })?;
                    return Ok(Incoming::Group {
                        chat: chat.id.clone(),
                    });
                }
                self.save(mark)?;
                Ok(Incoming::Other)
            }
            ChatKind::Admit if user == chat.peer => {
                match control.get("join").and_then(Value::as_str) {
                    Some(token) => self.check_join_token(chat, token, now, mark),
                    None => {
                        self.save(mark)?;
                        Ok(Incoming::Other)
                    }
                }
            }
            ChatKind::Join
                if user == chat.peer
                    && control.get("join_denied").and_then(Value::as_bool) == Some(true) =>
            {
                let _ = self.device.forget_group(&chat.id);
                let id = chat.id.clone();
                self.save(move |tx| {
                    mark(tx)?;
                    purge(tx, &id)
                })?;
                Ok(Incoming::JoinFailed {
                    chat: chat.id.clone(),
                })
            }
            _ => {
                self.save(mark)?;
                Ok(Incoming::Other)
            }
        }
    }

    /// Someone (or this account on another device) left a group.
    fn member_left(
        &mut self,
        chat: &Chat,
        user: &str,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        let id = chat.id.clone();
        if user == self.user {
            // This account left on another device: this one goes too.
            self.device.forget_group(&id)?;
            let me = self.user.clone();
            self.save(move |tx| {
                mark(tx)?;
                del_outbox(tx, &id)?;
                set_state(tx, &id, ChatState::Left)?;
                set_end(tx, &id, None)?;
                note(tx, &id, now, &json!({"event": "left", "users": [me]}))
            })?;
        } else {
            let devices = self.devices_of(&id, user)?;
            let who = user.to_string();
            self.save(move |tx| {
                mark(tx)?;
                for d in &devices {
                    tx.execute(
                        "INSERT INTO group_leaving (chat, device) VALUES (?1, ?2)
                         ON CONFLICT DO NOTHING",
                        params![id, d],
                    )
                    .map_err(sql)?;
                }
                note(tx, &id, now, &json!({"event": "left", "users": [who]}))
            })?;
        }
        Ok(Incoming::Group {
            chat: chat.id.clone(),
        })
    }

    /// A token in a join chat to this admin. A key this device never
    /// made gets no answer (another own device may have made it); one that
    /// expired or whose group this account no longer runs is refused.
    fn check_join_token(
        &mut self,
        chat: &Chat,
        token: &str,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        let mut t = [0u8; 16];
        let row: Option<(String, i64, i64, i64)> = if hex::decode_to_slice(token, &mut t).is_ok() {
            self.store
                .conn()
                .query_row(
                    "SELECT chat, guest, ends, revoked FROM group_invites WHERE hash = ?1",
                    [digest(&t).to_vec()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(sql)?
        } else {
            None
        };
        let Some((group, guest, ends, revoked)) = row else {
            self.save(mark)?;
            return Ok(Incoming::Other);
        };
        let runs_it = self
            .group_chat(&group)
            .is_ok_and(|(_, s)| s.is_admin(&self.me()));
        let joiner = self.device.accounts(&chat.id)?.remove(&chat.peer);
        let (Some(fp), true, true) = (joiner, to_u64(ends) > now && revoked == 0, runs_it) else {
            self.save(mark)?;
            return Ok(Incoming::JoinRefused {
                chat: chat.id.clone(),
            });
        };
        let (id, g, user) = (chat.id.clone(), group.clone(), chat.peer.clone());
        let until = (guest != 0).then_some(ends);
        self.save(move |tx| {
            mark(tx)?;
            tx.execute(
                "INSERT INTO join_requests (chat, grp, user, fp, guest_until, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (chat) DO NOTHING",
                params![id, g, user, fp, until, to_i64(now)],
            )
            .map_err(sql)?;
            Ok(())
        })?;
        Ok(Incoming::JoinRequest {
            chat: chat.id.clone(),
            group,
            user: chat.peer.clone(),
        })
    }
}

/// Weaker trust ranks higher (to show the weakest device of an account).
fn trust_rank(t: Trust) -> u8 {
    match t {
        Trust::Own => 0,
        Trust::OwnAccount => 1,
        Trust::Verified => 2,
        Trust::Unverified => 3,
        Trust::Mismatch => 4,
        Trust::Withdrawn => 5,
        Trust::Forged => 6,
    }
}
