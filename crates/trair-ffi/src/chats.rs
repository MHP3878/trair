// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Contacts and chats behind the door (M4). The chat database opens with
//! the vault and closes when Trair locks. The app gets texts, names and
//! ciphertext to send; keys and the database key never leave the core.
//!
//! Ops (see [`crate::api`] for the request names): `contact_code`,
//! `add_contact`, `contacts`, `confirm_contact`, `chats`, `messages`,
//! `start_chat`, `add_devices`, `remove_devices`, `update_keys`,
//! `withdrawn_members`, `pending_commits`, `commit_answer`,
//! `send_message`, `receive`, `accept_request`, `decline_request`,
//! `key_packages`, `apply_contact_list`.
//!
//! A change of a chat comes back as `commit`: `{chat, epoch, commit,
//! to, welcome, welcome_to, removed, hash}` (bytes in base64, devices as
//! `user:device`). The app posts it to the server and reports the answer
//! with `commit_answer`.
//!
//! Extra error codes: `chats_unavailable`, `blocked`, `forged`,
//! `withdrawn_member`, `unknown_chat`, `bad_package`, `rejected`,
//! `storage`.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::Deserialize;
use serde_json::{Value, json};
use trair_core::chats::{CHATS_FILE, ChatState};
use trair_core::{
    Chat, Contact, CoreError, Fingerprint, Incoming, KeyPackageOffer, Message, Messenger,
    PendingCommit, SignedDeviceList, Vault,
};

/// Key packages per request at most (the server keeps 100 per device).
pub const MAX_KEY_PACKAGES: usize = 100;

/// One key package offered by the server.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Offer {
    /// `user:device`, as the server claims it (checked by the core).
    device: String,
    /// Base64.
    package: String,
}

pub(crate) fn core_error(e: CoreError) -> Value {
    fail(match e {
        CoreError::Blocked(_) => "blocked",
        CoreError::Forged(_) => "forged",
        CoreError::Withdrawn(_) => "withdrawn_member",
        CoreError::UnknownGroup(_) => "unknown_chat",
        CoreError::GroupExists(_) => "exists",
        CoreError::InvalidKeyPackage(_) => "bad_package",
        CoreError::Rejected(_) => "rejected",
        CoreError::InvalidName(_) => "bad_request",
        CoreError::Storage(_) => "storage",
        CoreError::Internal(_) => "internal",
    })
}

fn fail(code: &str) -> Value {
    json!({"ok": false, "error": code})
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Open this device's chat database next to the vault.
pub(crate) fn open(dir: &Path, vault: &Vault) -> Result<Messenger, CoreError> {
    let mut m = Messenger::open(
        &dir.join(CHATS_FILE),
        vault.chats_key(),
        vault.account(),
        vault.device(),
        &vault.device_list().list,
        now(),
    )?;
    m.set_strict(strict_of(vault.profile()))?;
    Ok(m)
}

/// Strict mode from the profile; on unless switched off.
pub(crate) fn strict_of(profile: &Value) -> bool {
    profile
        .get("strict")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Delete the chat database (the account is deleted).
pub(crate) fn remove_files(dir: &Path) {
    for name in [CHATS_FILE.to_string(), format!("{CHATS_FILE}-journal")] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

fn b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

fn unb64(text: &str) -> Option<Vec<u8>> {
    B64.decode(text).ok().filter(|b| !b.is_empty())
}

fn contact_json(c: &Contact) -> Value {
    json!({
        "user": c.user,
        "fingerprint": c.fingerprint.grouped(),
        "name": c.name,
        "confirmed": c.confirmed,
        "added": c.added,
    })
}

fn chat_json(c: &Chat, name: &str) -> Value {
    json!({
        "id": c.id,
        "peer": c.peer,
        "name": name,
        "state": match c.state {
            ChatState::Active => "active",
            ChatState::Request => "request",
            ChatState::Left => "left",
        },
        "created": c.created,
        "last": c.last,
    })
}

fn message_json(m: &Message) -> Value {
    json!({
        "id": m.id,
        "chat": m.chat,
        "from": m.from,
        "user": m.user,
        "text": m.text,
        "at": m.at,
        "outgoing": m.outgoing,
        "missing": m.missing,
    })
}

fn commit_json(p: &PendingCommit) -> Value {
    json!({
        "chat": p.chat,
        "epoch": p.epoch,
        "commit": b64(&p.commit),
        "to": p.to,
        "welcome": p.welcome.as_deref().map(b64),
        "welcome_to": p.welcome_to,
        "removed": p.removed,
        "hash": p.hash(),
    })
}

fn offers(list: Vec<Offer>) -> Option<Vec<KeyPackageOffer>> {
    list.into_iter()
        .map(|o| {
            Some(KeyPackageOffer {
                claimed_device: o.device,
                bytes: unb64(&o.package)?,
            })
        })
        .collect()
}

fn name_of(m: &Messenger, user: &str) -> String {
    m.contact(user)
        .ok()
        .flatten()
        .map(|c| c.name)
        .unwrap_or_else(|| user.to_string())
}

pub(crate) fn contact_code(m: &Messenger) -> Value {
    json!({"ok": true, "code": m.contact_code()})
}

pub(crate) fn add_contact(m: &mut Messenger, code: &str, name: &str) -> Value {
    match m.add_contact(code, name, now()) {
        Ok(c) => json!({"ok": true, "contact": contact_json(&c)}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn contacts(m: &Messenger) -> Value {
    match m.contacts() {
        Ok(list) => {
            json!({"ok": true, "contacts": list.iter().map(contact_json).collect::<Vec<_>>()})
        }
        Err(e) => core_error(e),
    }
}

pub(crate) fn confirm_contact(m: &mut Messenger, user: &str) -> Value {
    match m.confirm_contact(user) {
        Ok(()) => json!({"ok": true}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn chats(m: &Messenger) -> Value {
    match m.chats() {
        Ok(list) => json!({
            "ok": true,
            "chats": list.iter().map(|c| chat_json(c, &name_of(m, &c.peer))).collect::<Vec<_>>(),
        }),
        Err(e) => core_error(e),
    }
}

pub(crate) fn messages(
    m: &Messenger,
    chat: &str,
    before: Option<i64>,
    limit: Option<u32>,
) -> Value {
    let limit = limit.unwrap_or(50).clamp(1, 500) as usize;
    match m.messages(chat, before, limit) {
        Ok(list) => {
            json!({"ok": true, "messages": list.iter().map(message_json).collect::<Vec<_>>()})
        }
        Err(e) => core_error(e),
    }
}

pub(crate) fn start_chat(m: &mut Messenger, user: &str, list: Vec<Offer>) -> Value {
    let Some(offers) = offers(list) else {
        return fail("bad_request");
    };
    match m.start_chat(user, &offers, now()) {
        Ok((chat, p)) => json!({
            "ok": true,
            "chat": chat_json(&chat, &name_of(m, &chat.peer)),
            "commit": commit_json(&p),
        }),
        Err(e) => core_error(e),
    }
}

pub(crate) fn add_devices(m: &mut Messenger, chat: &str, list: Vec<Offer>) -> Value {
    let Some(offers) = offers(list) else {
        return fail("bad_request");
    };
    match m.add_devices(chat, &offers) {
        Ok(p) => json!({"ok": true, "commit": commit_json(&p)}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn remove_devices(m: &mut Messenger, chat: &str, ids: &[String]) -> Value {
    match m.remove_devices(chat, ids) {
        Ok(p) => json!({"ok": true, "commit": commit_json(&p)}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn update_keys(m: &mut Messenger, chat: &str) -> Value {
    match m.update_keys(chat) {
        Ok(p) => json!({"ok": true, "commit": commit_json(&p)}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn withdrawn_members(m: &Messenger, chat: &str) -> Value {
    match m.withdrawn_members(chat) {
        Ok(ids) => json!({"ok": true, "ids": ids}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn pending_commits(m: &Messenger) -> Value {
    match m.pending_commits() {
        Ok(list) => {
            json!({"ok": true, "commits": list.iter().map(commit_json).collect::<Vec<_>>()})
        }
        Err(e) => core_error(e),
    }
}

pub(crate) fn commit_answer(
    m: &mut Messenger,
    chat: &str,
    accepted: bool,
    taken: Option<&str>,
) -> Value {
    match m.commit_answer(chat, accepted, taken, now()) {
        Ok(applied) => json!({"ok": true, "applied": applied}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn send_message(m: &mut Messenger, chat: &str, text: &str) -> Value {
    let to = match m.recipients(chat) {
        Ok(to) => to,
        Err(e) => return core_error(e),
    };
    match m.send(chat, text, now()) {
        Ok((bytes, msg)) => json!({
            "ok": true,
            "body": b64(&bytes),
            "to": to,
            "message": message_json(&msg),
        }),
        Err(e) => core_error(e),
    }
}

pub(crate) fn receive(m: &mut Messenger, body: &str) -> Value {
    let Some(bytes) = unb64(body) else {
        return fail("bad_request");
    };
    let incoming = match m.receive(&bytes, now()) {
        Ok(i) => i,
        Err(e) => return core_error(e),
    };
    let what = match incoming {
        Incoming::Duplicate => json!({"kind": "duplicate"}),
        Incoming::Chat { chat, update_keys } => json!({
            "kind": "chat",
            "chat": chat_json(&chat, &name_of(m, &chat.peer)),
            "update_keys": update_keys,
        }),
        Incoming::Message(msg) => json!({"kind": "message", "message": message_json(&msg)}),
        Incoming::Changed {
            chat,
            by,
            added,
            removed,
            left,
        } => json!({
            "kind": "changed",
            "chat": chat,
            "by": by,
            "added": added.into_iter().map(|a| a.id).collect::<Vec<_>>(),
            "removed": removed,
            "left": left,
        }),
        Incoming::Profile { user } => json!({"kind": "profile", "user": user}),
        Incoming::Other => json!({"kind": "other"}),
    };
    json!({"ok": true, "incoming": what})
}

/// This account's profile for every chat that does not have this version
/// yet. The app hands each to the server, then calls `profile_sent`.
pub(crate) fn share_profile(m: &mut Messenger, profile: &Value, only_contacts: bool) -> Value {
    match m.share_profile(profile, only_contacts) {
        Ok(out) => json!({
            "ok": true,
            "out": out.iter().map(|o| json!({
                "chat": o.chat,
                "rev": o.rev,
                "body": b64(&o.bytes),
                "to": o.to,
            })).collect::<Vec<_>>(),
        }),
        Err(e) => core_error(e),
    }
}

pub(crate) fn profile_sent(m: &mut Messenger, chat: &str, rev: u64) -> Value {
    match m.profile_sent(chat, rev) {
        Ok(()) => json!({"ok": true}),
        Err(e) => core_error(e),
    }
}

/// Profiles of other accounts. Badges are checked now (they expire) with
/// the account key the profile came with; the app gets only valid ones.
pub(crate) fn profiles(m: &Messenger) -> Value {
    let list = match m.profiles() {
        Ok(l) => l,
        Err(e) => return core_error(e),
    };
    let now = now();
    let out: Vec<Value> = list
        .iter()
        .map(|p| {
            let parsed: Vec<trair_core::badges::Badge> = p
                .profile
                .badges
                .iter()
                .filter_map(|b| serde_json::from_value(b.clone()).ok())
                .collect();
            let (shown, _) = trair_core::badges::verify_all(
                &parsed,
                &p.account_key,
                now,
                trair_core::badges::TEAM_KEYS,
            );
            json!({
                "user": p.user,
                "rev": p.profile.rev,
                "name": p.profile.name,
                "glow": p.profile.glow,
                "about": p.profile.about,
                "picture": p.profile.picture.as_deref().map(b64),
                "badges": shown,
                "name_warning": trair_core::badges::name_needs_warning(&p.profile.name, &shown),
            })
        })
        .collect();
    json!({"ok": true, "profiles": out})
}

pub(crate) fn accept_request(m: &mut Messenger, chat: &str, name: &str) -> Value {
    match m.accept_request(chat, name, now()) {
        Ok(c) => json!({"ok": true, "contact": contact_json(&c)}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn decline_request(m: &mut Messenger, chat: &str) -> Value {
    match m.decline_request(chat) {
        Ok(()) => json!({"ok": true}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn key_packages(m: &mut Messenger, n: usize, last_resort: bool) -> Value {
    if n > MAX_KEY_PACKAGES {
        return fail("bad_request");
    }
    let packages = match m.key_packages(n) {
        Ok(p) => p,
        Err(e) => return core_error(e),
    };
    let last = if last_resort {
        match m.last_resort_key_package() {
            Ok(p) => Some(b64(&p)),
            Err(e) => return core_error(e),
        }
    } else {
        None
    };
    json!({
        "ok": true,
        "packages": packages.iter().map(|p| b64(p)).collect::<Vec<_>>(),
        "last_resort": last,
    })
}

/// A contact's device list from the server. Taken only if the account
/// key fits the pinned fingerprint and signed the list; its withdrawn
/// devices are refused from then on.
pub(crate) fn apply_contact_list(m: &mut Messenger, account_key: &str, list: Value) -> Value {
    let Ok(key) = hex::decode(account_key) else {
        return fail("bad_request");
    };
    let Ok(list) = serde_json::from_value::<SignedDeviceList>(list) else {
        return fail("bad_request");
    };
    let Ok(Some(contact)) = m.contact(&list.list.user) else {
        return fail("rejected");
    };
    if Fingerprint::of(&key) != contact.fingerprint {
        return fail("blocked");
    }
    let Ok(verified) = list.verify(&key, None) else {
        return fail("refused");
    };
    let withdrawn: Vec<String> = verified
        .withdrawn
        .iter()
        .map(|d| format!("{}:{}", verified.user, d.id))
        .collect();
    let verified = verified.clone();
    match m.apply_device_list(&verified) {
        Ok(()) => json!({"ok": true, "withdrawn": withdrawn}),
        Err(e) => core_error(e),
    }
}
