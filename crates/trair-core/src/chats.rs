// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Real chats (M4): contacts, one-to-one chats, requests and the chat
//! history, all kept in the encrypted chat database of this device.
//!
//! Every chat is an MLS group with a random id. It holds all devices of
//! both accounts. A change to a group (devices added or removed) is a
//! commit; the server takes only one commit per group and epoch. If
//! another device was faster, this device throws its commit away
//! ([`Messenger::commit_refused`]), reads the winner and tries again.
//!
//! Order of work, so nothing secret is ever sent twice or lost: every
//! change is first stored (one transaction), only then handed out. An
//! incoming message is stored before the server may delete it.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use crate::account::{Account, Fingerprint};
use crate::contacts::{ContactCode, InviteCode, InviteKind};
use crate::devices::{DeviceIdentity, DeviceList};
use crate::error::{CoreError, Result, internal};
use crate::mls::{Device, DeviceState, Event, KeyPackageOffer, Member, Outgoing};
use crate::policy::Trust;
use crate::profiles;
use crate::store::{Store, put_meta, sql};

/// File name of the chat database in the data folder.
pub const CHATS_FILE: &str = "chats.db";

/// Longest chat message, in bytes.
pub const MAX_TEXT: usize = 32 * 1024;

/// How long the hashes of processed messages are kept (the server keeps
/// waiting messages at most 30 days).
pub const SEEN_DAYS: u64 = 31;

/// A contact: an account whose key fingerprint is pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    /// User name on the server.
    pub user: String,
    /// Pinned fingerprint of the account key.
    pub fingerprint: Fingerprint,
    /// Name shown in the app.
    pub name: String,
    /// The fingerprint was compared in person.
    pub confirmed: bool,
    /// When the contact was added (seconds since 1970).
    pub added: u64,
}

/// State of a chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatState {
    /// A chat with a contact.
    Active,
    /// Someone who is not a contact wrote first. Nothing can be sent
    /// until the request is accepted.
    Request,
    /// This device was removed from the chat. The history stays.
    Left,
}

impl ChatState {
    fn as_str(self) -> &'static str {
        match self {
            ChatState::Active => "active",
            ChatState::Request => "request",
            ChatState::Left => "left",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s {
            "active" => Ok(ChatState::Active),
            "request" => Ok(ChatState::Request),
            "left" => Ok(ChatState::Left),
            other => Err(CoreError::Storage(format!("unknown chat state {other}"))),
        }
    }
}

/// A one-to-one chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chat {
    /// Random id, also the MLS group id.
    pub id: String,
    /// The other account.
    pub peer: String,
    /// Active, request or left.
    pub state: ChatState,
    /// When the chat started (seconds since 1970).
    pub created: u64,
    /// Time of the last message or change.
    pub last: u64,
}

/// A stored chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Running number on this device.
    pub id: i64,
    /// The chat.
    pub chat: String,
    /// Sending device (`user:device`), proven by the MLS signature.
    pub from: String,
    /// Sending account.
    pub user: String,
    /// The text.
    pub text: String,
    /// When it was sent or received here (seconds since 1970).
    pub at: u64,
    /// Sent by this account (this device or another own device).
    pub outgoing: bool,
    /// Earlier messages of this sender that never arrived.
    pub missing: u64,
}

/// A change of a chat (devices added or removed, new keys) waiting for
/// the server. It stays on disk until [`Messenger::commit_accepted`] or
/// [`Messenger::commit_refused`], so after a restart it can be sent again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCommit {
    /// The chat.
    pub chat: String,
    /// Epoch the change was built on. The server takes one change per
    /// chat and epoch.
    pub epoch: u64,
    /// The commit, for the devices in `to`.
    pub commit: Vec<u8>,
    /// Current devices of the chat besides this one (withdrawn ones are
    /// left out: the server would refuse them).
    pub to: Vec<String>,
    /// The welcome, for the devices in `welcome_to`.
    pub welcome: Option<Vec<u8>>,
    /// New devices.
    pub welcome_to: Vec<String>,
    /// Devices that will be removed.
    pub removed: Vec<String>,
}

impl PendingCommit {
    /// SHA-256 of the commit, hex. The server names the commit it took
    /// for an epoch this way; equal means it was this one.
    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(&self.commit))
    }
}

/// What an incoming message did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// Seen before: nothing happened again.
    Duplicate,
    /// This device joined a chat (a request if the other account is not a
    /// contact). With `update_keys`, call [`Messenger::update_keys`].
    Chat {
        /// The chat.
        chat: Chat,
        /// The welcome used the last-resort key package.
        update_keys: bool,
    },
    /// A chat message, already stored.
    Message(Message),
    /// Devices were added to or removed from a chat.
    Changed {
        /// The chat.
        chat: String,
        /// Who made the change.
        by: String,
        /// New devices.
        added: Vec<Member>,
        /// Removed devices.
        removed: Vec<String>,
        /// This device is no longer in the chat.
        left: bool,
    },
    /// When the chat vanishes changed (`None`: it stays).
    Ends {
        /// The chat.
        chat: String,
        /// Seconds since 1970.
        ends: Option<u64>,
    },
    /// The chat partner sent a newer profile (stored; see
    /// [`Messenger::profile`]).
    Profile {
        /// Whose profile.
        user: String,
    },
    /// Nothing to show (for example a proposal).
    Other,
}

/// Another account's profile, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerProfile {
    /// Whose profile.
    pub user: String,
    /// What they sent, checked.
    pub profile: profiles::Checked,
    /// Their account key (to check badges against).
    pub account_key: Vec<u8>,
    /// When it arrived.
    pub at: u64,
}

/// This account's profile, ready for one chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOut {
    /// The chat.
    pub chat: String,
    /// Which version.
    pub rev: u64,
    /// The encrypted message.
    pub bytes: Vec<u8>,
    /// Devices that get it.
    pub to: Vec<String>,
}

/// The chats of this device.
pub struct Messenger {
    device: Device,
    store: Store,
    /// SHA-256 of every stored OpenMLS value, to write only changes.
    saved: HashMap<Vec<u8>, [u8; 32]>,
    user: String,
}

impl std::fmt::Debug for Messenger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Messenger")
            .field("device", &self.device.id())
            .finish_non_exhaustive()
    }
}

fn digest(v: &[u8]) -> [u8; 32] {
    Sha256::digest(v).into()
}

fn random_chat_id() -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(internal)?;
    Ok(hex::encode(b))
}

fn user_of(device_id: &str) -> &str {
    device_id.split(':').next().unwrap_or("")
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

impl Messenger {
    /// Open the chat database at `path` (created on first use) for this
    /// device. `own_list` is the account's current device list.
    pub fn open(
        path: &Path,
        key: &[u8; 32],
        account: &Account,
        identity: &DeviceIdentity,
        own_list: &DeviceList,
        now: u64,
    ) -> Result<Self> {
        let store = Store::open(path, key)?;
        // The database belongs to exactly one device of one account.
        let owner = format!(
            "{}:{}:{}",
            account.user(),
            identity.id,
            account.fingerprint().as_str()
        );
        match store.meta("owner")? {
            Some(o) if o != owner.as_bytes() => {
                return Err(CoreError::Storage(
                    "chat database belongs to another device".into(),
                ));
            }
            _ => {}
        }
        let values: HashMap<Vec<u8>, Vec<u8>> = {
            let mut stmt = store.conn().prepare("SELECT k, v FROM mls").map_err(sql)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
                })
                .map_err(sql)?;
            rows.collect::<std::result::Result<_, _>>().map_err(sql)?
        };
        let saved = values.iter().map(|(k, v)| (k.clone(), digest(v))).collect();
        let mut device = match store.meta("device")? {
            Some(bytes) => {
                let state: DeviceState = serde_json::from_slice(&bytes)
                    .map_err(|_| CoreError::Storage("device state is not readable".into()))?;
                Device::restore(account, identity, values, state)?
            }
            None => Device::from_identity(account, identity)?,
        };
        device.apply_device_list(own_list);
        let mut m = Messenger {
            device,
            store,
            saved,
            user: account.user().to_string(),
        };
        let cutoff = to_i64(now.saturating_sub(SEEN_DAYS * 86_400));
        m.save(|tx| {
            put_meta(tx, "owner", owner.as_bytes())?;
            tx.execute("DELETE FROM seen WHERE at < ?1", [cutoff])
                .map_err(sql)?;
            Ok(())
        })?;
        // Whatever ran out while this device was off goes now.
        m.sweep(now)?;
        Ok(m)
    }

    /// Store the device state and whatever `f` writes, in one transaction.
    fn save<T>(&mut self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let state = serde_json::to_vec(&self.device.export_state()).map_err(internal)?;
        let saved = &self.saved;
        let (upserts, deletes, next) = self.device.with_values(|map| {
            let mut upserts: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            let mut next = HashMap::with_capacity(map.len());
            for (k, v) in map {
                let h = digest(v);
                if saved.get(k) != Some(&h) {
                    upserts.push((k.clone(), v.clone()));
                }
                next.insert(k.clone(), h);
            }
            let deletes: Vec<Vec<u8>> = saved
                .keys()
                .filter(|k| !map.contains_key(*k))
                .cloned()
                .collect();
            (upserts, deletes, next)
        })?;
        let out = self.store.write(|tx| {
            for (k, v) in &upserts {
                tx.execute(
                    "INSERT INTO mls (k, v) VALUES (?1, ?2)
                     ON CONFLICT (k) DO UPDATE SET v = excluded.v",
                    params![k, v],
                )
                .map_err(sql)?;
            }
            for k in &deletes {
                tx.execute("DELETE FROM mls WHERE k = ?1", [k])
                    .map_err(sql)?;
            }
            put_meta(tx, "device", &state)?;
            f(tx)
        })?;
        self.saved = next;
        Ok(out)
    }

    /// `user:device` of this device.
    pub fn device_id(&self) -> &str {
        self.device.id()
    }

    /// The contact code of this account, to give to other people.
    pub fn contact_code(&self) -> String {
        ContactCode {
            user: self.user.clone(),
            fingerprint: self.device.account_fingerprint().clone(),
        }
        .text()
    }

    /// Switch strict mode (from the app setting).
    pub fn set_strict(&mut self, on: bool) -> Result<()> {
        self.device.set_strict(on);
        self.save(|_| Ok(()))
    }

    /// Take note of a verified device list (own or a contact's): its
    /// withdrawn devices are refused from now on.
    pub fn apply_device_list(&mut self, list: &DeviceList) -> Result<()> {
        self.device.apply_device_list(list);
        self.save(|_| Ok(()))
    }

    /// One-time key packages for the server. Their private keys are
    /// stored before the packages are handed out.
    pub fn key_packages(&mut self, n: usize) -> Result<Vec<Vec<u8>>> {
        let kps = self.device.key_packages(n)?;
        self.save(|_| Ok(()))?;
        Ok(kps)
    }

    /// A fresh last-resort key package.
    pub fn last_resort_key_package(&mut self) -> Result<Vec<u8>> {
        let kp = self.device.last_resort_key_package()?;
        self.save(|_| Ok(()))?;
        Ok(kp)
    }

    // ----- contacts -----

    /// Add a contact from a code. Pins the account key in the code.
    /// Requests from this account become chats.
    pub fn add_contact(&mut self, code: &str, name: &str, now: u64) -> Result<Contact> {
        let Ok(invite) = InviteCode::parse(code) else {
            return self.add_contact_code(ContactCode::parse(code)?, name, now);
        };
        if invite.ends <= now {
            return Err(CoreError::Rejected(
                "this friendship key has expired".into(),
            ));
        }
        let existed = self.contact(&invite.contact.user)?.is_some();
        let contact = self.add_contact_code(invite.contact.clone(), name, now)?;
        // The token goes into the first chat with this account, see
        // [`Messenger::invite_message`].
        let (user, token, kind, ends) = (
            contact.user.clone(),
            invite.token.to_vec(),
            invite.kind.letter().to_string(),
            to_i64(invite.ends),
        );
        let vanishing = invite.kind == InviteKind::Vanishing && !existed;
        self.save(move |tx| {
            tx.execute(
                "INSERT INTO redeem (user, token, kind, ends) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (user) DO UPDATE SET token = excluded.token, kind = excluded.kind,
                   ends = excluded.ends",
                params![user, token, kind, ends],
            )
            .map_err(sql)?;
            if vanishing {
                tx.execute(
                    "INSERT INTO contact_ends (user, ends) VALUES (?1, ?2) ON CONFLICT (user) DO NOTHING",
                    params![user, ends],
                )
                .map_err(sql)?;
            }
            Ok(())
        })?;
        Ok(contact)
    }

    fn add_contact_code(&mut self, code: ContactCode, name: &str, now: u64) -> Result<Contact> {
        if code.user == self.user {
            return Err(CoreError::Rejected("that is your own code".into()));
        }
        let name = clean_name(name, &code.user);
        if let Some(existing) = self.contact(&code.user)? {
            if existing.fingerprint != code.fingerprint {
                return Err(CoreError::Blocked(format!(
                    "{} is already a contact with a different key",
                    code.user
                )));
            }
            return Ok(existing);
        }
        // Open requests from this account turn into chats, but only if
        // their devices really belong to the key in the code.
        let mut accepted = Vec::new();
        for chat in self.chats()? {
            if chat.peer == code.user && chat.state == ChatState::Request {
                if self.device.account_of(&chat.id, &code.user)? == Some(code.fingerprint.clone()) {
                    accepted.push(chat.id);
                } else {
                    return Err(CoreError::Blocked(format!(
                        "an open request from {} used a different key",
                        code.user
                    )));
                }
            }
        }
        self.device.verify(&code.user, code.fingerprint.clone());
        let contact = Contact {
            user: code.user.clone(),
            fingerprint: code.fingerprint.clone(),
            name,
            confirmed: false,
            added: now,
        };
        let c = contact.clone();
        self.save(move |tx| {
            insert_contact(tx, &c)?;
            for id in &accepted {
                set_state(tx, id, ChatState::Active)?;
            }
            Ok(())
        })?;
        Ok(contact)
    }

    /// Mark a contact as compared in person.
    pub fn confirm_contact(&mut self, user: &str) -> Result<()> {
        if self.contact(user)?.is_none() {
            return Err(CoreError::Rejected(format!("{user} is not a contact")));
        }
        let user = user.to_string();
        self.save(move |tx| {
            tx.execute("UPDATE contacts SET confirmed = 1 WHERE user = ?1", [&user])
                .map_err(sql)?;
            Ok(())
        })
    }

    /// One contact.
    pub fn contact(&self, user: &str) -> Result<Option<Contact>> {
        self.store
            .conn()
            .query_row(
                "SELECT user, fingerprint, name, confirmed, added FROM contacts WHERE user = ?1",
                [user],
                row_contact,
            )
            .optional()
            .map_err(sql)?
            .transpose()
    }

    /// All contacts, by name.
    pub fn contacts(&self) -> Result<Vec<Contact>> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT user, fingerprint, name, confirmed, added FROM contacts ORDER BY name, user")
            .map_err(sql)?;
        let rows = stmt.query_map([], row_contact).map_err(sql)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(sql)??);
        }
        Ok(out)
    }

    // ----- chats -----

    /// Start a chat with a contact. `offers` are key packages for the
    /// contact's devices and for this account's other devices. Send the
    /// commit (it reaches nobody yet) and the welcome; then call
    /// [`Messenger::commit_accepted`].
    pub fn start_chat(
        &mut self,
        user: &str,
        offers: &[KeyPackageOffer],
        now: u64,
    ) -> Result<(Chat, PendingCommit)> {
        let Some(contact) = self.contact(user)? else {
            return Err(CoreError::Rejected(format!("{user} is not a contact")));
        };
        self.check_offers(&contact.user, offers)?;
        if !offers
            .iter()
            .any(|o| user_of(&o.claimed_device) == contact.user)
        {
            return Err(CoreError::InvalidKeyPackage(format!(
                "no device of {} to add",
                contact.user
            )));
        }
        let id = random_chat_id()?;
        self.device.create_group(&id)?;
        let out = match self.device.add_members(&id, offers) {
            Ok(out) => out,
            Err(e) => {
                // Nothing of this chat stays behind.
                self.device.forget_group(&id)?;
                self.save(|_| Ok(()))?;
                return Err(e);
            }
        };
        let pending = self.pending(out, Vec::new());
        let chat = Chat {
            id,
            peer: contact.user,
            state: ChatState::Active,
            created: now,
            last: now,
        };
        let (c, p) = (chat.clone(), pending.clone());
        self.save(move |tx| {
            insert_chat(tx, &c)?;
            put_outbox(tx, &p)
        })?;
        Ok((chat, pending))
    }

    fn pending(&self, out: Outgoing, to: Vec<String>) -> PendingCommit {
        PendingCommit {
            chat: out.group,
            epoch: out.epoch,
            commit: out.commit,
            to,
            welcome: out.welcome,
            welcome_to: out.added.into_iter().map(|m| m.id).collect(),
            removed: out.removed,
        }
    }

    /// The devices that get a commit for this chat: everyone but this
    /// device and withdrawn devices.
    fn commit_recipients(&self, chat: &str) -> Result<Vec<String>> {
        Ok(self
            .device
            .members(chat)?
            .into_iter()
            .filter(|m| m.trust != Trust::Own && m.trust != Trust::Withdrawn)
            .map(|m| m.id)
            .collect())
    }

    /// Only one change per chat at a time.
    fn no_pending(&self, chat: &str) -> Result<()> {
        if self.pending_commit(chat)?.is_some() {
            return Err(CoreError::Rejected(
                "a change of this chat is still waiting for the server".into(),
            ));
        }
        Ok(())
    }

    /// The change of a chat that waits for the server, if any.
    pub fn pending_commit(&self, chat: &str) -> Result<Option<PendingCommit>> {
        self.store
            .conn()
            .query_row(
                "SELECT chat, epoch, commit_bytes, recipients, welcome, welcome_to, removed
                 FROM outbox WHERE chat = ?1",
                [chat],
                row_pending,
            )
            .optional()
            .map_err(sql)?
            .transpose()
    }

    /// All changes that wait for the server (send them again after a
    /// restart).
    pub fn pending_commits(&self) -> Result<Vec<PendingCommit>> {
        let mut stmt = self
            .store
            .conn()
            .prepare(
                "SELECT chat, epoch, commit_bytes, recipients, welcome, welcome_to, removed
                 FROM outbox ORDER BY chat",
            )
            .map_err(sql)?;
        let rows = stmt.query_map([], row_pending).map_err(sql)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(sql)??);
        }
        Ok(out)
    }

    /// Key packages may only add devices of the chat partner or of this
    /// account; the certificates are checked again by MLS.
    fn check_offers(&self, peer: &str, offers: &[KeyPackageOffer]) -> Result<()> {
        for o in offers {
            let u = user_of(&o.claimed_device);
            if u != peer && u != self.user {
                return Err(CoreError::InvalidKeyPackage(format!(
                    "{} does not belong in this chat",
                    o.claimed_device
                )));
            }
        }
        Ok(())
    }

    /// Add devices to a chat (a new own device, a new device of the
    /// contact, or a retry after a refused commit).
    pub fn add_devices(&mut self, chat: &str, offers: &[KeyPackageOffer]) -> Result<PendingCommit> {
        let c = self.chat(chat)?;
        if c.state == ChatState::Left {
            return Err(CoreError::Rejected("this device left the chat".into()));
        }
        self.no_pending(chat)?;
        self.check_offers(&c.peer, offers)?;
        let to = self.commit_recipients(chat)?;
        let out = self.device.add_members(chat, offers)?;
        self.store_pending(out, to)
    }

    fn store_pending(&mut self, out: Outgoing, to: Vec<String>) -> Result<PendingCommit> {
        let p = self.pending(out, to);
        let q = p.clone();
        self.save(move |tx| put_outbox(tx, &q))?;
        Ok(p)
    }

    /// Remove devices from a chat (for example a withdrawn one).
    pub fn remove_devices(&mut self, chat: &str, ids: &[String]) -> Result<PendingCommit> {
        self.chat(chat)?;
        self.no_pending(chat)?;
        // Removed devices get the commit too, so they learn it (unless
        // they were withdrawn: the server knows them no more).
        let to = self.commit_recipients(chat)?;
        let out = self.device.remove_devices(chat, ids)?;
        self.store_pending(out, to)
    }

    /// Withdrawn devices still in a chat. Nothing can be sent until they
    /// are removed with [`Messenger::remove_devices`].
    pub fn withdrawn_members(&self, chat: &str) -> Result<Vec<String>> {
        Ok(self
            .device
            .members(chat)?
            .into_iter()
            .filter(|m| m.trust == Trust::Withdrawn)
            .map(|m| m.id)
            .collect())
    }

    /// New keys for this device in a chat (after joining through the
    /// last-resort key package).
    pub fn update_keys(&mut self, chat: &str) -> Result<PendingCommit> {
        self.chat(chat)?;
        self.no_pending(chat)?;
        let to = self.commit_recipients(chat)?;
        let out = self.device.self_update(chat)?;
        self.store_pending(out, to)
    }

    /// The server took the commit: apply it.
    pub fn commit_accepted(&mut self, chat: &str, now: u64) -> Result<()> {
        if self.pending_commit(chat)?.is_none() {
            return Err(CoreError::Rejected("no change is waiting".into()));
        }
        self.device.confirm(chat)?;
        let id = chat.to_string();
        self.save(move |tx| {
            del_outbox(tx, &id)?;
            touch(tx, &id, now)
        })
    }

    /// The server took another commit first: throw this one away. Read
    /// the waiting messages, then try the change again.
    pub fn commit_refused(&mut self, chat: &str) -> Result<()> {
        if self.pending_commit(chat)?.is_none() {
            return Err(CoreError::Rejected("no change is waiting".into()));
        }
        // An incoming commit may already have replaced it.
        if self.device.has_group(chat) {
            self.device.discard(chat)?;
        }
        let id = chat.to_string();
        self.save(move |tx| del_outbox(tx, &id))
    }

    /// The server's answer to a change: `taken` is the hash of the commit
    /// it holds for this epoch (if it refused). Returns true if the change
    /// is in effect, false if it has to be made again.
    pub fn commit_answer(
        &mut self,
        chat: &str,
        accepted: bool,
        taken: Option<&str>,
        now: u64,
    ) -> Result<bool> {
        let Some(p) = self.pending_commit(chat)? else {
            return Err(CoreError::Rejected("no change is waiting".into()));
        };
        if accepted || taken == Some(p.hash().as_str()) {
            self.commit_accepted(chat, now)?;
            Ok(true)
        } else {
            self.commit_refused(chat)?;
            Ok(false)
        }
    }

    /// Everyone in a chat, with trust levels.
    pub fn members(&self, chat: &str) -> Result<Vec<Member>> {
        self.device.members(chat)
    }

    /// The devices a message for this chat goes to.
    pub fn recipients(&self, chat: &str) -> Result<Vec<String>> {
        self.device.recipients(chat)
    }

    /// Current epoch of a chat (the server orders commits by it).
    pub fn epoch(&self, chat: &str) -> Result<u64> {
        self.device.epoch(chat)
    }

    /// Encrypt and store a message. Send the bytes to
    /// [`Messenger::recipients`] only after this returned.
    pub fn send(&mut self, chat: &str, text: &str, now: u64) -> Result<(Vec<u8>, Message)> {
        let c = self.chat(chat)?;
        match c.state {
            ChatState::Active => {}
            ChatState::Request => {
                return Err(CoreError::Blocked(
                    "accept the request before answering".into(),
                ));
            }
            ChatState::Left => {
                return Err(CoreError::Rejected("this device left the chat".into()));
            }
        }
        if text.trim().is_empty() || text.len() > MAX_TEXT {
            return Err(CoreError::Rejected("empty or too long".into()));
        }
        let bytes = self.device.encrypt(chat, text)?;
        let from = self.device.id().to_string();
        let user = self.user.clone();
        let (id_chat, body) = (chat.to_string(), text.to_string());
        let id = self.save(move |tx| {
            let id = insert_message(tx, &id_chat, &from, &user, &body, now, true, 0)?;
            touch(tx, &id_chat, now)?;
            Ok(id)
        })?;
        Ok((
            bytes,
            Message {
                id,
                chat: chat.to_string(),
                from: self.device.id().to_string(),
                user: self.user.clone(),
                text: text.to_string(),
                at: now,
                outgoing: true,
                missing: 0,
            },
        ))
    }

    /// Process anything from the server. The result is stored before this
    /// returns; only then may the server forget the message.
    pub fn receive(&mut self, bytes: &[u8], now: u64) -> Result<Incoming> {
        let hash = digest(bytes).to_vec();
        let seen: Option<i64> = self
            .store
            .conn()
            .query_row("SELECT at FROM seen WHERE hash = ?1", [&hash], |r| r.get(0))
            .optional()
            .map_err(sql)?;
        if seen.is_some() {
            return Ok(Incoming::Duplicate);
        }
        let at = to_i64(now);
        let mark = move |tx: &Transaction| -> Result<()> {
            tx.execute(
                "INSERT INTO seen (hash, at) VALUES (?1, ?2) ON CONFLICT (hash) DO NOTHING",
                params![hash, at],
            )
            .map_err(sql)?;
            Ok(())
        };
        let event = match self.device.receive(bytes) {
            Ok(e) => e,
            Err(e) => {
                // The device state may still have moved (for example a
                // ratchet step); keep disk and memory the same.
                self.save(mark)?;
                return Err(e);
            }
        };
        match event {
            Event::Joined {
                group,
                members,
                by,
                via_last_resort,
            } => self.joined(group, members, by, via_last_resort, now, mark),
            Event::Message {
                group,
                from,
                text,
                missing,
            } => {
                let user = user_of(&from).to_string();
                let outgoing = user == self.user;
                let (g, f, u, t) = (group.clone(), from.clone(), user.clone(), text.clone());
                let id = self.save(move |tx| {
                    mark(tx)?;
                    let id = insert_message(tx, &g, &f, &u, &t, now, outgoing, missing)?;
                    touch(tx, &g, now)?;
                    Ok(id)
                })?;
                Ok(Incoming::Message(Message {
                    id,
                    chat: group,
                    from,
                    user,
                    text,
                    at: now,
                    outgoing,
                    missing,
                }))
            }
            Event::Profile {
                group,
                from,
                account_key,
                profile,
            } => self.got_profile(&group, &from, account_key, &profile, now, mark),
            Event::Control {
                group,
                from,
                control,
            } => self.got_control(&group, &from, &control, now, mark),
            Event::Commit {
                group,
                by,
                added,
                removed,
                i_was_removed,
            } => {
                if i_was_removed {
                    // Keys of a chat this device is no longer in go away.
                    self.device.forget_group(&group)?;
                }
                let g = group.clone();
                self.save(move |tx| {
                    mark(tx)?;
                    if i_was_removed {
                        set_state(tx, &g, ChatState::Left)?;
                        del_outbox(tx, &g)?;
                    }
                    touch(tx, &g, now)
                })?;
                Ok(Incoming::Changed {
                    chat: group,
                    by,
                    added,
                    removed,
                    left: i_was_removed,
                })
            }
            Event::Other { .. } => {
                self.save(mark)?;
                Ok(Incoming::Other)
            }
        }
    }

    fn joined(
        &mut self,
        group: String,
        members: Vec<Member>,
        by: Member,
        via_last_resort: bool,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        let mut peers: Vec<&str> = members
            .iter()
            .filter(|m| m.user != self.user)
            .map(|m| m.user.as_str())
            .collect();
        peers.sort_unstable();
        peers.dedup();
        let refuse = if members.iter().any(|m| m.trust == Trust::Mismatch) {
            // A device signed by a different key than the pinned one: an
            // attack or a lost account, never a normal chat.
            Some(CoreError::Blocked(
                "a device in this chat is signed by an unknown key".into(),
            ))
        } else if peers.len() != 1 {
            Some(CoreError::Rejected("only one-to-one chats for now".into()))
        } else {
            None
        };
        if let Some(e) = refuse {
            self.device.forget_group(&group)?;
            self.save(mark)?;
            return Err(e);
        }
        let peer = peers[0].to_string();
        // A chat is a request unless the other account is a contact, or
        // one of this account's own devices set it up (then that device
        // already decided, and the contact is taken over from the chat).
        let mut new_contact = None;
        let state = if self.contact(&peer)?.is_some() {
            ChatState::Active
        } else if by.trust == Trust::OwnAccount {
            let Some(fingerprint) = self.device.account_of(&group, &peer)? else {
                self.device.forget_group(&group)?;
                self.save(mark)?;
                return Err(CoreError::Blocked(format!(
                    "the devices of {peer} do not agree on one key"
                )));
            };
            self.device.verify(&peer, fingerprint.clone());
            new_contact = Some(Contact {
                user: peer.clone(),
                fingerprint,
                name: peer.clone(),
                confirmed: false,
                added: now,
            });
            ChatState::Active
        } else {
            ChatState::Request
        };
        let chat = Chat {
            id: group,
            peer,
            state,
            created: now,
            last: now,
        };
        let c = chat.clone();
        self.save(move |tx| {
            mark(tx)?;
            if let Some(k) = &new_contact {
                insert_contact(tx, k)?;
            }
            insert_chat(tx, &c)
        })?;
        Ok(Incoming::Chat {
            chat,
            update_keys: via_last_resort,
        })
    }

    // ------------------------------------------ friendship keys, vanishing

    /// A friendship key of this account: works once, until `ends`. With
    /// [`InviteKind::Vanishing`] the chat it opens vanishes at `ends`.
    pub fn create_invite(&mut self, kind: InviteKind, ends: u64, now: u64) -> Result<String> {
        if ends <= now || ends > now + MAX_END_SECS {
            return Err(CoreError::Rejected("end time out of range".into()));
        }
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|e| CoreError::Internal(e.to_string()))?;
        let hash = digest(&token).to_vec();
        let (k, e) = (kind.letter().to_string(), to_i64(ends));
        self.save(move |tx| {
            tx.execute(
                "INSERT INTO invites (hash, kind, ends, used) VALUES (?1, ?2, ?3, 0)",
                params![hash, k, e],
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
            kind,
            ends,
        }
        .text())
    }

    /// The token of a friendship key this account used, for the first chat
    /// with its owner (send it after the chat started). `None` if there is
    /// none or it expired. A vanishing key makes this chat vanish too.
    pub fn invite_message(
        &mut self,
        chat: &str,
        now: u64,
    ) -> Result<Option<(Vec<u8>, Vec<String>)>> {
        let c = self.chat(chat)?;
        let row: Option<(Vec<u8>, String, i64)> = self
            .store
            .conn()
            .query_row(
                "SELECT token, kind, ends FROM redeem WHERE user = ?1",
                [&c.peer],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((token, kind, ends)) = row else {
            return Ok(None);
        };
        let peer = c.peer.clone();
        if to_u64(ends) <= now {
            self.save(move |tx| {
                tx.execute("DELETE FROM redeem WHERE user = ?1", [&peer])
                    .map_err(sql)?;
                Ok(())
            })?;
            return Ok(None);
        }
        let control =
            serde_json::json!({"invite": hex::encode(&token), "kind": kind, "ends": ends});
        let bytes = self.device.encrypt_control(chat, &control)?;
        let to = self.recipients(chat)?;
        let id = chat.to_string();
        self.save(move |tx| {
            tx.execute("DELETE FROM redeem WHERE user = ?1", [&peer])
                .map_err(sql)?;
            if kind == "v" {
                set_end(tx, &id, Some(ends))?;
            }
            Ok(())
        })?;
        Ok(Some((bytes, to)))
    }

    /// Let a chat vanish at `ends` (`None`: it stays). Both sides delete it
    /// then; the message for the others is returned.
    pub fn set_chat_end(
        &mut self,
        chat: &str,
        ends: Option<u64>,
        now: u64,
    ) -> Result<(Vec<u8>, Vec<String>)> {
        let c = self.chat(chat)?;
        if c.state != ChatState::Active {
            return Err(CoreError::Blocked("only in an active chat".into()));
        }
        if let Some(e) = ends
            && (e <= now || e > now + MAX_END_SECS)
        {
            return Err(CoreError::Rejected("end time out of range".into()));
        }
        let bytes = self
            .device
            .encrypt_control(chat, &serde_json::json!({"ends": ends}))?;
        let to = self.recipients(chat)?;
        let id = chat.to_string();
        self.save(move |tx| set_end(tx, &id, ends.map(to_i64)))?;
        Ok((bytes, to))
    }

    /// When a chat vanishes, if it does.
    pub fn chat_end(&self, chat: &str) -> Result<Option<u64>> {
        let e: Option<i64> = self
            .store
            .conn()
            .query_row("SELECT ends FROM chat_ends WHERE chat = ?1", [chat], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)?;
        Ok(e.map(to_u64))
    }

    /// Delete what has run out: vanished chats (keys, messages), the
    /// contacts that came with them, old friendship keys. Returns the
    /// deleted chats.
    pub fn sweep(&mut self, now: u64) -> Result<Vec<String>> {
        let n = to_i64(now);
        let ids = |sql_text: &str, conn: &rusqlite::Connection| -> Result<Vec<String>> {
            let mut st = conn.prepare(sql_text).map_err(sql)?;
            let rows = st.query_map([n], |r| r.get::<_, String>(0)).map_err(sql)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)
        };
        let chats = ids(
            "SELECT chat FROM chat_ends WHERE ends <= ?1",
            self.store.conn(),
        )?;
        let users = ids(
            "SELECT user FROM contact_ends WHERE ends <= ?1",
            self.store.conn(),
        )?;
        for id in &chats {
            // A chat this device never fully joined has no keys to forget.
            let _ = self.device.forget_group(id);
        }
        let (c2, u2) = (chats.clone(), users);
        self.save(move |tx| {
            for id in &c2 {
                for table in ["messages", "outbox", "profile_sent", "chat_ends"] {
                    tx.execute(&format!("DELETE FROM {table} WHERE chat = ?1"), [id])
                        .map_err(sql)?;
                }
                tx.execute("DELETE FROM chats WHERE id = ?1", [id])
                    .map_err(sql)?;
            }
            for user in &u2 {
                for table in ["contacts", "contact_ends", "profiles", "redeem"] {
                    tx.execute(&format!("DELETE FROM {table} WHERE user = ?1"), [user])
                        .map_err(sql)?;
                }
            }
            tx.execute("DELETE FROM invites WHERE ends <= ?1", [n])
                .map_err(sql)?;
            tx.execute("DELETE FROM redeem WHERE ends <= ?1", [n])
                .map_err(sql)?;
            Ok(())
        })?;
        Ok(chats)
    }

    /// This account's profile for every active chat that does not have
    /// this version yet. `only_contacts`: only chats with contacts (not
    /// with someone whose request was accepted). Nothing is stored: once
    /// the server took a message, call [`Messenger::profile_sent`]. A chat
    /// that cannot get it now (for example strict mode) is left out.
    pub fn share_profile(
        &mut self,
        profile: &serde_json::Value,
        only_contacts: bool,
    ) -> Result<Vec<ProfileOut>> {
        let checked = profiles::check(profile)?;
        let wire = checked.to_wire();
        let mut out = Vec::new();
        for chat in self.chats()? {
            if chat.state != ChatState::Active {
                continue;
            }
            if only_contacts && self.contact(&chat.peer)?.is_none() {
                continue;
            }
            let sent: Option<i64> = self
                .store
                .conn()
                .query_row(
                    "SELECT rev FROM profile_sent WHERE chat = ?1",
                    [&chat.id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?;
            if sent.is_some_and(|s| to_u64(s) >= checked.rev) {
                continue;
            }
            let Ok(bytes) = self.device.encrypt_profile(&chat.id, &wire) else {
                continue;
            };
            out.push(ProfileOut {
                to: self.recipients(&chat.id)?,
                chat: chat.id,
                rev: checked.rev,
                bytes,
            });
        }
        // The ratchet moved: keep disk and memory the same.
        self.save(|_| Ok(()))?;
        Ok(out)
    }

    /// The server took the profile for `chat`.
    pub fn profile_sent(&mut self, chat: &str, rev: u64) -> Result<()> {
        let (c, r) = (chat.to_string(), to_i64(rev));
        self.save(move |tx| {
            tx.execute(
                "INSERT INTO profile_sent (chat, rev) VALUES (?1, ?2)
                 ON CONFLICT (chat) DO UPDATE SET rev = max(rev, excluded.rev)",
                params![c, r],
            )
            .map_err(sql)?;
            Ok(())
        })
    }

    /// The stored profile of `user`.
    pub fn profile(&self, user: &str) -> Result<Option<PeerProfile>> {
        let mut st = self
            .store
            .conn()
            .prepare(
                "SELECT user, rev, name, glow, about, picture, badges, account_key, at
                 FROM profiles WHERE user = ?1",
            )
            .map_err(sql)?;
        let mut rows = st.query_map([user], row_profile).map_err(sql)?;
        match rows.next() {
            Some(r) => Ok(Some(r.map_err(sql)??)),
            None => Ok(None),
        }
    }

    /// Every stored profile.
    pub fn profiles(&self) -> Result<Vec<PeerProfile>> {
        let mut st = self
            .store
            .conn()
            .prepare(
                "SELECT user, rev, name, glow, about, picture, badges, account_key, at
                 FROM profiles ORDER BY user",
            )
            .map_err(sql)?;
        let rows = st.query_map([], row_profile).map_err(sql)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(sql)??);
        }
        Ok(out)
    }

    /// Accept a request: the other account becomes a contact with the key
    /// its devices carry in this chat.
    pub fn accept_request(&mut self, chat: &str, name: &str, now: u64) -> Result<Contact> {
        let c = self.chat(chat)?;
        if c.state != ChatState::Request {
            return Err(CoreError::Rejected("not a request".into()));
        }
        let Some(fingerprint) = self.device.account_of(chat, &c.peer)? else {
            return Err(CoreError::Blocked(format!(
                "the devices of {} do not agree on one key",
                c.peer
            )));
        };
        if let Some(existing) = self.contact(&c.peer)?
            && existing.fingerprint != fingerprint
        {
            return Err(CoreError::Blocked(format!(
                "{} is already a contact with a different key",
                c.peer
            )));
        }
        self.device.verify(&c.peer, fingerprint.clone());
        let contact = Contact {
            user: c.peer.clone(),
            fingerprint,
            name: clean_name(name, &c.peer),
            confirmed: false,
            added: now,
        };
        let k = contact.clone();
        let id = chat.to_string();
        self.save(move |tx| {
            tx.execute(
                "INSERT INTO contacts (user, fingerprint, name, confirmed, added)
                 VALUES (?1, ?2, ?3, 0, ?4) ON CONFLICT (user) DO NOTHING",
                params![k.user, k.fingerprint.as_str(), k.name, to_i64(k.added)],
            )
            .map_err(sql)?;
            set_state(tx, &id, ChatState::Active)
        })?;
        Ok(contact)
    }

    /// Decline a request: the chat, its keys and its messages are deleted
    /// on this device. Later messages for it are refused.
    pub fn decline_request(&mut self, chat: &str) -> Result<()> {
        let c = self.chat(chat)?;
        if c.state != ChatState::Request {
            return Err(CoreError::Rejected("not a request".into()));
        }
        self.device.forget_group(chat)?;
        let id = chat.to_string();
        self.save(move |tx| {
            tx.execute("DELETE FROM messages WHERE chat = ?1", [&id])
                .map_err(sql)?;
            tx.execute("DELETE FROM chats WHERE id = ?1", [&id])
                .map_err(sql)?;
            Ok(())
        })
    }

    /// One chat.
    pub fn chat(&self, id: &str) -> Result<Chat> {
        self.store
            .conn()
            .query_row(
                "SELECT id, peer, state, created, last FROM chats WHERE id = ?1",
                [id],
                row_chat,
            )
            .optional()
            .map_err(sql)?
            .transpose()?
            .ok_or_else(|| CoreError::UnknownGroup(id.to_string()))
    }

    /// All chats, newest first.
    pub fn chats(&self) -> Result<Vec<Chat>> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT id, peer, state, created, last FROM chats ORDER BY last DESC, id")
            .map_err(sql)?;
        let rows = stmt.query_map([], row_chat).map_err(sql)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(sql)??);
        }
        Ok(out)
    }

    /// Messages of a chat, oldest first: the newest `limit` ones, or the
    /// `limit` ones before message `before`.
    pub fn messages(&self, chat: &str, before: Option<i64>, limit: usize) -> Result<Vec<Message>> {
        let mut stmt = self
            .store
            .conn()
            .prepare(
                "SELECT id, chat, sender, user, body, at, outgoing, missing FROM messages
                 WHERE chat = ?1 AND id < ?2 ORDER BY id DESC LIMIT ?3",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map(
                params![chat, before.unwrap_or(i64::MAX), to_i64(limit as u64)],
                |r| {
                    Ok(Message {
                        id: r.get(0)?,
                        chat: r.get(1)?,
                        from: r.get(2)?,
                        user: r.get(3)?,
                        text: r.get(4)?,
                        at: to_u64(r.get(5)?),
                        outgoing: r.get::<_, i64>(6)? != 0,
                        missing: to_u64(r.get(7)?),
                    })
                },
            )
            .map_err(sql)?;
        let mut out: Vec<Message> = rows.collect::<std::result::Result<_, _>>().map_err(sql)?;
        out.reverse();
        Ok(out)
    }
}

/// Longest time a friendship key or a vanishing chat may run: 30 days.
pub const MAX_END_SECS: u64 = 30 * 86_400;

fn set_end(tx: &Transaction, chat: &str, ends: Option<i64>) -> Result<()> {
    match ends {
        Some(e) => tx.execute(
            "INSERT INTO chat_ends (chat, ends) VALUES (?1, ?2)
             ON CONFLICT (chat) DO UPDATE SET ends = excluded.ends",
            params![chat, e],
        ),
        None => tx.execute("DELETE FROM chat_ends WHERE chat = ?1", [chat]),
    }
    .map_err(sql)?;
    Ok(())
}

impl Messenger {
    /// A chat setting arrived. A friendship key's token from the chat
    /// partner makes them a contact (once, before the end); an end time
    /// from the partner or an own device applies to the chat. Anything
    /// else is only marked as seen.
    fn got_control(
        &mut self,
        group: &str,
        from: &str,
        control: &serde_json::Value,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        let user = user_of(from).to_string();
        let Ok(chat) = self.chat(group) else {
            self.save(mark)?;
            return Ok(Incoming::Other);
        };
        if let Some(token) = control.get("invite").and_then(|t| t.as_str()) {
            if user != chat.peer {
                self.save(mark)?;
                return Ok(Incoming::Other);
            }
            return Ok(self
                .redeem_token(&chat, token, now, mark)?
                .unwrap_or(Incoming::Other));
        }
        let from_ok = user == chat.peer || user == self.user;
        let ends = match control.get("ends") {
            Some(serde_json::Value::Null) => Some(None),
            Some(v) => v
                .as_u64()
                .filter(|e| *e > now && *e <= now + MAX_END_SECS)
                .map(Some),
            None => None,
        };
        match ends {
            Some(ends) if from_ok && chat.state == ChatState::Active => {
                let id = group.to_string();
                self.save(move |tx| {
                    mark(tx)?;
                    set_end(tx, &id, ends.map(to_i64))
                })?;
                Ok(Incoming::Ends {
                    chat: group.to_string(),
                    ends,
                })
            }
            _ => {
                self.save(mark)?;
                Ok(Incoming::Other)
            }
        }
    }

    /// Check a token against this account's friendship keys. Valid, unused
    /// and not expired: the chat partner becomes a contact with the key
    /// their devices show in this chat, and the chat opens. `Ok(None)`
    /// after marking the message seen when it does not fit.
    fn redeem_token(
        &mut self,
        chat: &Chat,
        token: &str,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Option<Incoming>> {
        let mut t = [0u8; 16];
        let found: Option<(String, i64, i64)> = if hex::decode_to_slice(token, &mut t).is_ok() {
            self.store
                .conn()
                .query_row(
                    "SELECT kind, ends, used FROM invites WHERE hash = ?1",
                    [digest(&t).to_vec()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(sql)?
        } else {
            None
        };
        let fingerprint = self.device.account_of(&chat.id, &chat.peer)?;
        let existing = self.contact(&chat.peer)?;
        let fits = matches!(&found, Some((_, ends, used)) if *used == 0 && to_u64(*ends) > now)
            && fingerprint.is_some()
            && existing
                .as_ref()
                .is_none_or(|e| Some(&e.fingerprint) == fingerprint.as_ref());
        let (Some((kind, ends, _)), Some(fingerprint), true) = (found, fingerprint, fits) else {
            self.save(mark)?;
            return Ok(None);
        };
        self.device.verify(&chat.peer, fingerprint.clone());
        let hash = digest(&t).to_vec();
        let (peer, id, fp) = (
            chat.peer.clone(),
            chat.id.clone(),
            fingerprint.as_str().to_string(),
        );
        let vanishing = kind == "v";
        let new_contact = existing.is_none();
        self.save(move |tx| {
            mark(tx)?;
            tx.execute("UPDATE invites SET used = 1 WHERE hash = ?1", [&hash])
                .map_err(sql)?;
            tx.execute(
                "INSERT INTO contacts (user, fingerprint, name, confirmed, added)
                 VALUES (?1, ?2, ?1, 0, ?3) ON CONFLICT (user) DO NOTHING",
                params![peer, fp, to_i64(now)],
            )
            .map_err(sql)?;
            set_state(tx, &id, ChatState::Active)?;
            if vanishing {
                set_end(tx, &id, Some(ends))?;
                if new_contact {
                    tx.execute(
                        "INSERT INTO contact_ends (user, ends) VALUES (?1, ?2) ON CONFLICT (user) DO NOTHING",
                        params![peer, ends],
                    )
                    .map_err(sql)?;
                }
            }
            Ok(())
        })?;
        Ok(Some(Incoming::Chat {
            chat: self.chat(&chat.id)?,
            update_keys: false,
        }))
    }

    /// A profile arrived. Kept only from the chat partner, only if it
    /// passes the checks and only if it is newer; otherwise just marked
    /// as seen.
    fn got_profile(
        &mut self,
        group: &str,
        from: &str,
        account_key: Vec<u8>,
        profile: &serde_json::Value,
        now: u64,
        mark: impl FnOnce(&Transaction) -> Result<()>,
    ) -> Result<Incoming> {
        let user = user_of(from).to_string();
        let checked = match self.chat(group) {
            Ok(chat) if chat.peer == user && user != self.user => profiles::check(profile).ok(),
            _ => None,
        };
        let newer = match &checked {
            Some(c) => self
                .profile(&user)?
                .is_none_or(|old| old.profile.rev < c.rev),
            None => false,
        };
        let Some(c) = checked.filter(|_| newer) else {
            self.save(mark)?;
            return Ok(Incoming::Other);
        };
        let badges =
            serde_json::to_string(&c.badges).map_err(|e| CoreError::Storage(e.to_string()))?;
        let u = user.clone();
        self.save(move |tx| {
            mark(tx)?;
            tx.execute(
                "INSERT INTO profiles (user, rev, name, glow, about, picture, badges, account_key, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (user) DO UPDATE SET rev = excluded.rev, name = excluded.name,
                   glow = excluded.glow, about = excluded.about, picture = excluded.picture,
                   badges = excluded.badges, account_key = excluded.account_key, at = excluded.at",
                params![
                    u,
                    to_i64(c.rev),
                    c.name,
                    c.glow.map(i64::from),
                    c.about,
                    c.picture,
                    badges,
                    account_key,
                    to_i64(now)
                ],
            )
            .map_err(sql)?;
            Ok(())
        })?;
        Ok(Incoming::Profile { user })
    }
}

#[allow(clippy::type_complexity)]
fn row_profile(r: &rusqlite::Row) -> rusqlite::Result<Result<PeerProfile>> {
    let glow: Option<i64> = r.get(3)?;
    let badges: String = r.get(6)?;
    let badges = match serde_json::from_str(&badges) {
        Ok(b) => b,
        Err(e) => return Ok(Err(CoreError::Storage(e.to_string()))),
    };
    Ok(Ok(PeerProfile {
        user: r.get(0)?,
        profile: profiles::Checked {
            rev: to_u64(r.get(1)?),
            name: r.get(2)?,
            glow: glow.and_then(|g| u32::try_from(g).ok()),
            about: r.get(4)?,
            picture: r.get(5)?,
            badges,
        },
        account_key: r.get(7)?,
        at: to_u64(r.get(8)?),
    }))
}

fn clean_name(name: &str, user: &str) -> String {
    let n: String = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    if n.is_empty() { user.to_string() } else { n }
}

fn row_contact(r: &rusqlite::Row) -> rusqlite::Result<Result<Contact>> {
    let fp: String = r.get(1)?;
    Ok(Fingerprint::parse(&fp).map(|fingerprint| Contact {
        user: r.get(0).unwrap_or_default(),
        fingerprint,
        name: r.get(2).unwrap_or_default(),
        confirmed: r.get::<_, i64>(3).unwrap_or(0) != 0,
        added: to_u64(r.get(4).unwrap_or(0)),
    }))
}

fn row_chat(r: &rusqlite::Row) -> rusqlite::Result<Result<Chat>> {
    let state: String = r.get(2)?;
    let (id, peer, created, last): (String, String, i64, i64) =
        (r.get(0)?, r.get(1)?, r.get(3)?, r.get(4)?);
    Ok(ChatState::parse(&state).map(|state| Chat {
        id,
        peer,
        state,
        created: to_u64(created),
        last: to_u64(last),
    }))
}

fn ids_json(ids: &[String]) -> Result<String> {
    serde_json::to_string(ids).map_err(internal)
}

fn ids_from(text: &str) -> Result<Vec<String>> {
    serde_json::from_str(text).map_err(|_| CoreError::Storage("unreadable device list".into()))
}

fn put_outbox(tx: &Transaction, p: &PendingCommit) -> Result<()> {
    tx.execute(
        "INSERT INTO outbox (chat, epoch, commit_bytes, recipients, welcome, welcome_to, removed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            p.chat,
            to_i64(p.epoch),
            p.commit,
            ids_json(&p.to)?,
            p.welcome,
            ids_json(&p.welcome_to)?,
            ids_json(&p.removed)?
        ],
    )
    .map_err(sql)?;
    Ok(())
}

fn del_outbox(tx: &Transaction, chat: &str) -> Result<()> {
    tx.execute("DELETE FROM outbox WHERE chat = ?1", [chat])
        .map_err(sql)?;
    Ok(())
}

fn row_pending(r: &rusqlite::Row) -> rusqlite::Result<Result<PendingCommit>> {
    let (chat, epoch, commit, to, welcome, welcome_to, removed): (
        String,
        i64,
        Vec<u8>,
        String,
        Option<Vec<u8>>,
        String,
        String,
    ) = (
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
    );
    Ok((|| {
        Ok(PendingCommit {
            chat,
            epoch: to_u64(epoch),
            commit,
            to: ids_from(&to)?,
            welcome,
            welcome_to: ids_from(&welcome_to)?,
            removed: ids_from(&removed)?,
        })
    })())
}

fn insert_contact(tx: &Transaction, c: &Contact) -> Result<()> {
    tx.execute(
        "INSERT INTO contacts (user, fingerprint, name, confirmed, added) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            c.user,
            c.fingerprint.as_str(),
            c.name,
            i64::from(c.confirmed),
            to_i64(c.added)
        ],
    )
    .map_err(sql)?;
    Ok(())
}

fn insert_chat(tx: &Transaction, c: &Chat) -> Result<()> {
    tx.execute(
        "INSERT INTO chats (id, peer, state, created, last) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            c.id,
            c.peer,
            c.state.as_str(),
            to_i64(c.created),
            to_i64(c.last)
        ],
    )
    .map_err(sql)?;
    Ok(())
}

fn set_state(tx: &Transaction, chat: &str, state: ChatState) -> Result<()> {
    tx.execute(
        "UPDATE chats SET state = ?2 WHERE id = ?1",
        params![chat, state.as_str()],
    )
    .map_err(sql)?;
    Ok(())
}

fn touch(tx: &Transaction, chat: &str, now: u64) -> Result<()> {
    tx.execute(
        "UPDATE chats SET last = max(last, ?2) WHERE id = ?1",
        params![chat, to_i64(now)],
    )
    .map_err(sql)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_message(
    tx: &Transaction,
    chat: &str,
    from: &str,
    user: &str,
    text: &str,
    now: u64,
    outgoing: bool,
    missing: u64,
) -> Result<i64> {
    tx.execute(
        "INSERT INTO messages (chat, sender, user, body, at, outgoing, missing)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            chat,
            from,
            user,
            text,
            to_i64(now),
            i64::from(outgoing),
            to_i64(missing)
        ],
    )
    .map_err(sql)?;
    Ok(tx.last_insert_rowid())
}
