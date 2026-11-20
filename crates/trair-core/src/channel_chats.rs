// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Closed channels of communities (M8, step 3b): end-to-end encrypted MLS
//! groups whose admins are the community's keepers.
//!
//! - Every text carries a signature of the sender's device (context
//!   `trair/closed-message/v1`) inside the encryption. Receivers check it
//!   against the sender's MLS leaf before storing. With it, moderators can
//!   check a reported text, and a newcomer the history a keeper hands on.
//! - Keepers add whoever the community's roles let read the channel
//!   (decided on the server; the app asks it), remove those who lost it,
//!   and hand newcomers the last [`HISTORY`] signed messages. A keeper
//!   cannot forge history: every message in it must verify against its
//!   author's device in the channel, or it is dropped.
//! - Keepers (or the author) delete a message for everyone with a control
//!   message; every device deletes it.

use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Value, json};

use crate::chats::{
    Chat, ChatKind, ChatState, Incoming, Message, Messenger, PendingCommit, insert_chat,
    insert_message, put_outbox, touch, user_of,
};
use crate::error::{CoreError, Result};
use crate::group_chats::note;
use crate::groups::{Accounts, ChannelRef, GroupKind, GroupState};
use crate::mls::KeyPackageOffer;
use crate::proofs::{OpenMessage, sign_closed_message, verify_closed_message};
use crate::store::sql;

/// What a control of a closed channel does to the chat database.
pub(crate) type Apply = Box<dyn FnOnce(&Transaction) -> Result<()>>;

/// How many messages a newcomer gets from before joining.
pub const HISTORY: usize = 50;

/// Largest history handed on at once, in bytes of JSON.
pub const MAX_HISTORY_BYTES: usize = 256 * 1024;

/// A signed text of a closed channel, as stored and handed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedText {
    /// Author account.
    pub user: String,
    /// Author device (`d-…`).
    pub device: String,
    /// The text.
    pub text: String,
    /// When the device wrote it.
    pub sent: u64,
    /// Device signature.
    pub sig: Vec<u8>,
}

impl SignedText {
    fn json(&self) -> Value {
        json!({"user": self.user, "device": self.device, "text": self.text, "sent": self.sent,
            "sig": hex::encode(&self.sig)})
    }

    fn parse(v: &Value) -> Option<SignedText> {
        Some(SignedText {
            user: v.get("user")?.as_str()?.to_string(),
            device: v.get("device")?.as_str()?.to_string(),
            text: v.get("text")?.as_str()?.to_string(),
            sent: v.get("sent")?.as_u64()?,
            sig: hex::decode(v.get("sig")?.as_str()?).ok()?,
        })
    }
}

fn store_sig(tx: &Transaction, chat: &str, message: i64, sent: u64, sig: &[u8]) -> Result<()> {
    tx.execute(
        "INSERT INTO channel_sigs (message, chat, sent, sig) VALUES (?1, ?2, ?3, ?4)",
        params![
            message,
            chat,
            i64::try_from(sent).unwrap_or(i64::MAX),
            hex::encode(sig)
        ],
    )
    .map_err(sql)?;
    Ok(())
}

fn have_sig(tx: &Transaction, chat: &str, sig: &[u8]) -> Result<bool> {
    let found: Option<i64> = tx
        .query_row(
            "SELECT message FROM channel_sigs WHERE chat = ?1 AND sig = ?2",
            params![chat, hex::encode(sig)],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    Ok(found.is_some())
}

impl Messenger {
    /// The community channel a closed-channel chat belongs to.
    pub fn channel_of(&self, chat: &str) -> Result<Option<ChannelRef>> {
        if !self.device.has_group(chat) {
            return Ok(None);
        }
        Ok(self.device.group_state(chat)?.and_then(|s| s.channel))
    }

    fn channel_state(&self, chat: &str) -> Result<(Chat, GroupState, ChannelRef)> {
        let c = self.chat(chat)?;
        if c.kind != ChatKind::Channel || c.state != ChatState::Active {
            return Err(CoreError::Rejected("not an active channel".into()));
        }
        let s = self
            .device
            .group_state(chat)?
            .ok_or_else(|| CoreError::Rejected("not a channel".into()))?;
        let r = s
            .channel
            .clone()
            .ok_or_else(|| CoreError::Rejected("not a channel".into()))?;
        Ok((c, s, r))
    }

    /// A keeper makes the MLS group of a closed channel. `group` is the id
    /// the server chose for it; `offers` are devices of people who may
    /// read it (may be empty).
    pub fn create_channel(
        &mut self,
        group: &str,
        name: &str,
        channel: ChannelRef,
        offers: &[KeyPackageOffer],
        now: u64,
    ) -> Result<(Chat, Option<PendingCommit>)> {
        if group.len() != 32 || !group.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CoreError::Rejected("bad channel group id".into()));
        }
        if self.chat(group).is_ok() {
            return Err(CoreError::GroupExists(group.to_string()));
        }
        let state = GroupState::new_channel(name, self.device.acct(), channel)?;
        let allow = self.channel_allow(offers)?;
        self.device.create_trair_group(group, &state)?;
        let pending = if offers.iter().any(|o| o.claimed_device != self.device_id()) {
            match self.device.change(group, offers, &[], None, &allow) {
                Ok(out) => {
                    let mut p = self.pending(out, Vec::new());
                    p.committers = Some(state.committers(&Accounts::new()));
                    Some(p)
                }
                Err(e) => {
                    self.device.forget_group(group)?;
                    self.save(|_| Ok(()))?;
                    return Err(e);
                }
            }
        } else {
            None
        };
        let chat = Chat {
            id: group.to_string(),
            peer: String::new(),
            kind: ChatKind::Channel,
            state: ChatState::Active,
            created: now,
            last: now,
        };
        let (c, p) = (chat.clone(), pending.clone());
        let me = self.user.clone();
        self.save(move |tx| {
            insert_chat(tx, &c)?;
            note(tx, &c.id, now, &json!({"event": "created", "by": me}))?;
            if let Some(p) = &p {
                put_outbox(tx, p)?;
            }
            Ok(())
        })?;
        Ok((chat, pending))
    }

    /// A keeper adds devices of people the community lets read the channel.
    /// They need not be contacts: the keeper vouches, and every addition
    /// is a note in the channel.
    pub fn add_to_channel(
        &mut self,
        chat: &str,
        offers: &[KeyPackageOffer],
    ) -> Result<PendingCommit> {
        self.channel_state(chat)?;
        self.group_commit_any(chat, offers, &[], None)
    }

    /// A keeper removes someone who may no longer read the channel (also
    /// another keeper, or the one who made it).
    pub fn channel_remove(&mut self, chat: &str, user: &str) -> Result<PendingCommit> {
        let (_, mut state, _) = self.channel_state(chat)?;
        if user == self.user {
            return Err(CoreError::Rejected("leave instead".into()));
        }
        let devices: Vec<String> = self
            .device
            .members(chat)?
            .into_iter()
            .filter(|m| m.user == user)
            .map(|m| m.id)
            .collect();
        if devices.is_empty() {
            return Err(CoreError::Rejected(format!("{user} is not in the channel")));
        }
        state.admins.retain(|a| a.user != user);
        if state.owner.user == user {
            let me = self.device.acct();
            state.admins.retain(|a| *a != me);
            state.owner = me;
        }
        self.group_commit_any(chat, &[], &devices, Some(state))
    }

    /// Make the keepers of the channel exactly `keepers` (accounts in the
    /// channel; this account stays one if it is in the list).
    pub fn set_keepers(&mut self, chat: &str, keepers: &[String]) -> Result<Option<PendingCommit>> {
        let (_, state, _) = self.channel_state(chat)?;
        let accts = self.device.accounts(chat)?;
        let me = self.device.acct();
        let mut new = state.clone();
        if !keepers.contains(&new.owner.user) || !accts.contains_key(&new.owner.user) {
            new.owner = me.clone();
        }
        new.admins = keepers
            .iter()
            .filter(|u| **u != new.owner.user)
            .filter_map(|u| {
                accts.get(u).map(|fp| crate::groups::Acct {
                    user: u.clone(),
                    fp: fp.clone(),
                })
            })
            .collect();
        new.admins.sort();
        let mut old_admins = state.admins.clone();
        old_admins.sort();
        if new.owner == state.owner && new.admins == old_admins {
            return Ok(None);
        }
        self.group_commit_any(chat, &[], &[], Some(new)).map(Some)
    }

    /// Encrypt a text for a closed channel, signed by this device.
    pub(crate) fn send_channel(
        &mut self,
        chat: &str,
        text: &str,
        now: u64,
    ) -> Result<(Vec<u8>, Message)> {
        let (_, _, r) = self.channel_state(chat)?;
        let identity = self.device.identity();
        let m = OpenMessage {
            community: &r.community,
            channel: &r.channel,
            user: &self.user,
            device: &identity.id,
            sent: now,
            text,
        };
        let sig = sign_closed_message(&identity, &m)?;
        let bytes = self.device.encrypt_signed(chat, text, now, &sig)?;
        let from = self.device_id().to_string();
        let user = self.user.clone();
        let (id_chat, body) = (chat.to_string(), text.to_string());
        let id = self.save(move |tx| {
            let id = insert_message(tx, &id_chat, &from, &user, &body, now, true, 0)?;
            store_sig(tx, &id_chat, id, now, &sig)?;
            touch(tx, &id_chat, now)?;
            Ok(id)
        })?;
        Ok((
            bytes,
            Message {
                id,
                chat: chat.to_string(),
                from: self.device_id().to_string(),
                user: self.user.clone(),
                text: text.to_string(),
                at: now,
                outgoing: true,
                missing: 0,
            },
        ))
    }

    /// A text arrived in a closed channel: only with a valid signature of
    /// the sending device is it stored.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn channel_text(
        &mut self,
        chat: &str,
        from: &str,
        text: String,
        missing: u64,
        signed: Option<(u64, Vec<u8>)>,
        key: &[u8],
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        let user = user_of(from).to_string();
        let valid = match (&signed, self.channel_of(chat)?) {
            (Some((sent, sig)), Some(r)) => {
                let m = OpenMessage {
                    community: &r.community,
                    channel: &r.channel,
                    user: &user,
                    device: from.split_once(':').map_or("", |(_, d)| d),
                    sent: *sent,
                    text: &text,
                };
                verify_closed_message(key, &m, sig).is_ok()
            }
            _ => false,
        };
        let Some((sent, sig)) = signed.filter(|_| valid) else {
            self.save(mark)?;
            return Ok(Incoming::Other);
        };
        let outgoing = user == self.user;
        let (c, f, u, t) = (
            chat.to_string(),
            from.to_string(),
            user.clone(),
            text.clone(),
        );
        let id = self.save(move |tx| {
            mark(tx)?;
            if have_sig(tx, &c, &sig)? {
                return Ok(None);
            }
            let id = insert_message(tx, &c, &f, &u, &t, now, outgoing, missing)?;
            store_sig(tx, &c, id, sent, &sig)?;
            touch(tx, &c, now)?;
            Ok(Some(id))
        })?;
        let Some(id) = id else {
            return Ok(Incoming::Duplicate);
        };
        Ok(Incoming::Message(Message {
            id,
            chat: chat.to_string(),
            from: from.to_string(),
            user,
            text,
            at: now,
            outgoing,
            missing,
        }))
    }

    /// Signatures of stored texts of a closed channel, by message id.
    pub fn channel_signature(&self, message: i64) -> Result<Option<SignedText>> {
        self.store
            .conn()
            .query_row(
                "SELECT m.user, m.sender, m.body, s.sent, s.sig FROM channel_sigs s
                 JOIN messages m ON m.id = s.message WHERE s.message = ?1",
                [message],
                |r| {
                    let sender: String = r.get(1)?;
                    let sig: String = r.get(4)?;
                    Ok(SignedText {
                        user: r.get(0)?,
                        device: sender.split_once(':').map_or("", |(_, d)| d).to_string(),
                        text: r.get(2)?,
                        sent: u64::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
                        sig: hex::decode(sig).unwrap_or_default(),
                    })
                },
            )
            .optional()
            .map_err(sql)
    }

    /// A keeper hands a newcomer the last [`HISTORY`] signed texts: one
    /// encrypted message, only to the newcomer's devices. Nothing to send
    /// gives `None`.
    pub fn channel_history(
        &mut self,
        chat: &str,
        user: &str,
    ) -> Result<Option<(Vec<u8>, Vec<String>)>> {
        let (_, state, _) = self.channel_state(chat)?;
        if !state.is_admin(&self.device.acct()) {
            return Err(CoreError::NotAllowed("only keepers hand on history".into()));
        }
        let to: Vec<String> = self
            .device
            .members(chat)?
            .into_iter()
            .filter(|m| m.user == user)
            .map(|m| m.id)
            .collect();
        if to.is_empty() || user == self.user {
            return Ok(None);
        }
        let ids: Vec<i64> = {
            let mut st = self
                .store
                .conn()
                .prepare(
                    "SELECT s.message FROM channel_sigs s JOIN messages m ON m.id = s.message
                     WHERE s.chat = ?1 ORDER BY s.message DESC LIMIT ?2",
                )
                .map_err(sql)?;
            let rows = st
                .query_map(params![chat, HISTORY as i64], |r| r.get(0))
                .map_err(sql)?;
            rows.collect::<rusqlite::Result<_>>().map_err(sql)?
        };
        let mut items = Vec::new();
        let mut size = 0;
        for id in ids {
            if let Some(t) = self.channel_signature(id)? {
                let j = t.json();
                size += j.to_string().len();
                if size > MAX_HISTORY_BYTES {
                    break;
                }
                items.push(j);
            }
        }
        if items.is_empty() {
            return Ok(None);
        }
        items.reverse();
        let bytes = self
            .device
            .encrypt_control(chat, &json!({"history": items, "for": user}))?;
        Ok(Some((bytes, to)))
    }

    /// Delete a text of a closed channel for everyone (keepers: any text;
    /// others: their own). Deletes it here at once.
    pub fn delete_channel_message(
        &mut self,
        chat: &str,
        message: i64,
    ) -> Result<(Vec<u8>, Vec<String>)> {
        let (_, state, _) = self.channel_state(chat)?;
        let t = self
            .channel_signature(message)?
            .ok_or_else(|| CoreError::Rejected("no such message".into()))?;
        if t.user != self.user && !state.is_admin(&self.device.acct()) {
            return Err(CoreError::NotAllowed(
                "only keepers delete others' texts".into(),
            ));
        }
        let bytes = self
            .device
            .encrypt_control(chat, &json!({"delete": hex::encode(&t.sig)}))?;
        let to = self.device.recipients(chat)?;
        let id = chat.to_string();
        self.save(move |tx| delete_by_sig(tx, &id, &t.sig))?;
        Ok((bytes, to))
    }

    /// A control message of a closed channel: history for this account, or
    /// a deletion. `None`: not one of these.
    pub(crate) fn channel_control(
        &mut self,
        chat: &Chat,
        from: &str,
        control: &Value,
        now: u64,
    ) -> Result<Option<Apply>> {
        if chat.kind != ChatKind::Channel {
            return Ok(None);
        }
        let user = user_of(from).to_string();
        let Some(state) = self.device.group_state(&chat.id)? else {
            return Ok(None);
        };
        let accts = self.device.accounts(&chat.id)?;
        let from_keeper = accts.get(&user).is_some_and(|fp| {
            state.is_admin(&crate::groups::Acct {
                user: user.clone(),
                fp: fp.clone(),
            })
        });
        if let Some(sig) = control.get("delete").and_then(Value::as_str) {
            let Ok(sig) = hex::decode(sig) else {
                return Ok(Some(Box::new(|_| Ok(()))));
            };
            let id = chat.id.clone();
            let author: Option<String> = self
                .store
                .conn()
                .query_row(
                    "SELECT m.user FROM channel_sigs s JOIN messages m ON m.id = s.message
                     WHERE s.chat = ?1 AND s.sig = ?2",
                    params![id, hex::encode(&sig)],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?;
            if from_keeper || author.as_deref() == Some(user.as_str()) {
                return Ok(Some(Box::new(move |tx| delete_by_sig(tx, &id, &sig))));
            }
            return Ok(Some(Box::new(|_| Ok(()))));
        }
        if control.get("for").and_then(Value::as_str) == Some(self.user.as_str())
            && let Some(items) = control.get("history").and_then(Value::as_array)
        {
            if !from_keeper {
                return Ok(Some(Box::new(|_| Ok(()))));
            }
            let Some(r) = state.channel.clone() else {
                return Ok(Some(Box::new(|_| Ok(()))));
            };
            // Only texts whose authors' devices are in the channel and
            // whose signatures hold: a keeper cannot put words in anyone's
            // mouth.
            let mut good = Vec::new();
            for t in items.iter().take(HISTORY).filter_map(SignedText::parse) {
                let device = format!("{}:{}", t.user, t.device);
                let Some(key) = self.device.member_key(&chat.id, &device)? else {
                    continue;
                };
                let m = OpenMessage {
                    community: &r.community,
                    channel: &r.channel,
                    user: &t.user,
                    device: &t.device,
                    sent: t.sent,
                    text: &t.text,
                };
                if verify_closed_message(&key, &m, &t.sig).is_ok() {
                    good.push((device, t));
                }
            }
            let id = chat.id.clone();
            let me = self.user.clone();
            let by = user;
            return Ok(Some(Box::new(move |tx| {
                let mut n = 0;
                for (device, t) in &good {
                    if have_sig(tx, &id, &t.sig)? {
                        continue;
                    }
                    let mid =
                        insert_message(tx, &id, device, &t.user, &t.text, t.sent, t.user == me, 0)?;
                    store_sig(tx, &id, mid, t.sent, &t.sig)?;
                    n += 1;
                }
                if n > 0 {
                    note(
                        tx,
                        &id,
                        now,
                        &json!({"event": "history", "by": by, "count": n}),
                    )?;
                    touch(tx, &id, now)?;
                }
                Ok(())
            })));
        }
        Ok(None)
    }

    /// A welcome into a closed channel: the keeper who sent it vouches for
    /// the members (checked by [`crate::groups::check_welcome`]).
    pub(crate) fn joined_channel(
        &mut self,
        group: String,
        by: &str,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        let chat = Chat {
            id: group,
            peer: String::new(),
            kind: ChatKind::Channel,
            state: ChatState::Active,
            created: now,
            last: now,
        };
        let c = chat.clone();
        let adder = by.to_string();
        self.save(move |tx| {
            mark(tx)?;
            insert_chat(tx, &c)?;
            note(tx, &c.id, now, &json!({"event": "joined", "by": adder}))
        })?;
        Ok(Incoming::Chat {
            chat: self.chat(&chat.id)?,
            update_keys: false,
        })
    }

    /// The keeper vouches for the accounts it adds: strict mode lets them
    /// in although they are no contacts. A contact (or this account) under
    /// another key than the pinned one is still refused.
    fn channel_allow(&self, offers: &[KeyPackageOffer]) -> Result<Vec<crate::groups::Acct>> {
        let accts = self.device.offer_accounts(offers)?;
        for a in &accts {
            let pinned = if a.user == self.user {
                Some(self.device.acct().fp)
            } else {
                self.contact(&a.user)?
                    .map(|c| c.fingerprint.as_str().to_string())
            };
            if pinned.is_some_and(|fp| fp != a.fp) {
                return Err(CoreError::Blocked(format!(
                    "{} came with a different key",
                    a.user
                )));
            }
        }
        Ok(accts)
    }

    /// A change of a group or channel without the contact check; kept for
    /// the server.
    fn group_commit_any(
        &mut self,
        chat: &str,
        offers: &[KeyPackageOffer],
        remove: &[String],
        new_state: Option<GroupState>,
    ) -> Result<PendingCommit> {
        let old = self
            .device
            .group_state(chat)?
            .ok_or_else(|| CoreError::Rejected("not a group".into()))?;
        if old.kind != GroupKind::Channel {
            return Err(CoreError::Rejected("not a channel".into()));
        }
        self.no_pending(chat)?;
        let allow = self.channel_allow(offers)?;
        let to = self.commit_recipients(chat)?;
        let out = self
            .device
            .change(chat, offers, remove, new_state.as_ref(), &allow)?;
        let mut p = self.pending(out, to);
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
}

fn delete_by_sig(tx: &Transaction, chat: &str, sig: &[u8]) -> Result<()> {
    let message: Option<i64> = tx
        .query_row(
            "SELECT message FROM channel_sigs WHERE chat = ?1 AND sig = ?2",
            params![chat, hex::encode(sig)],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    if let Some(m) = message {
        tx.execute("DELETE FROM channel_sigs WHERE message = ?1", [m])
            .map_err(sql)?;
        tx.execute("DELETE FROM messages WHERE id = ?1", [m])
            .map_err(sql)?;
    }
    Ok(())
}
