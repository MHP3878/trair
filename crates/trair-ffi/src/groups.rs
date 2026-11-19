// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Group chats behind the door (M5). Like the chat ops, every change of a
//! group comes back as `commit` for the server (now with `committers`:
//! the accounts that may change the group from then on); messages come
//! back as `body` (base64) and `to` (`user:device`).
//!
//! Ops: `create_group`, `group_info`, `add_to_group`, `kick`,
//! `set_admin`, `hand_over`, `rename_group`, `set_group_end`,
//! `close_group`, `leave_prepare`, `leave_group`, `accept_group`,
//! `delete_chat`, `accept_new_key`, `propose_update`, `tidy_group`,
//! `create_group_invite`, `revoke_group_invites`, `group_invites`,
//! `join_group`, `join_message`, `join_requests`, `admit`, `close_join`,
//! `deny_join`.
//!
//! Extra error codes: `not_allowed` (the group's rules say no),
//! `outdated` (a device runs a Trair without groups).

use serde_json::{Value, json};
use trair_core::{GroupView, Messenger, PendingCommit, Tidy};

use crate::chats::{Offer, b64, chat_json, chat_name, commit_json, core_error, fail, now, offers};

/// Longest time for a group key or an end, in minutes (30 days).
const MAX_MINUTES: u64 = 30 * 24 * 60;

fn ends_in(minutes: u64) -> Option<u64> {
    (minutes > 0 && minutes <= MAX_MINUTES).then(|| now().saturating_add(minutes * 60))
}

fn commit(r: trair_core::Result<PendingCommit>) -> Value {
    match r {
        Ok(p) => json!({"ok": true, "commit": commit_json(&p)}),
        Err(e) => core_error(e),
    }
}

fn message(r: trair_core::Result<(Vec<u8>, Vec<String>)>) -> Value {
    match r {
        Ok((bytes, to)) => json!({"ok": true, "body": b64(&bytes), "to": to}),
        Err(e) => core_error(e),
    }
}

fn done(r: trair_core::Result<()>) -> Value {
    match r {
        Ok(()) => json!({"ok": true}),
        Err(e) => core_error(e),
    }
}

fn view_json(v: &GroupView) -> Value {
    json!({
        "chat": v.chat,
        "name": v.name,
        "me": v.me.as_str(),
        "ends": v.ends,
        "my_end": v.my_end,
        "members": v.members.iter().map(|m| json!({
            "user": m.user,
            "fingerprint": m.fp,
            "role": m.role.as_str(),
            "guest_until": m.guest_until,
            "devices": m.devices,
            "leaving": m.leaving,
            "key_changed": m.key_changed,
            "trust": format!("{:?}", m.trust).to_lowercase(),
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn create_group(
    m: &mut Messenger,
    name: &str,
    minutes: Option<u64>,
    list: Vec<Offer>,
) -> Value {
    let Some(offers) = offers(list) else {
        return fail("bad_request");
    };
    let ends = match minutes {
        Some(min) => match ends_in(min) {
            Some(e) => Some(e),
            None => return fail("bad_request"),
        },
        None => None,
    };
    match m.create_group(name, ends, &offers, now()) {
        Ok((chat, p)) => json!({
            "ok": true,
            "chat": chat_json(&chat, &chat_name(m, &chat)),
            "commit": p.as_ref().map(commit_json),
        }),
        Err(e) => core_error(e),
    }
}

pub(crate) fn group_info(m: &Messenger, chat: &str) -> Value {
    match m.group_view(chat) {
        Ok(v) => json!({"ok": true, "group": view_json(&v)}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn add_to_group(m: &mut Messenger, chat: &str, list: Vec<Offer>) -> Value {
    let Some(offers) = offers(list) else {
        return fail("bad_request");
    };
    commit(m.add_to_group(chat, &offers))
}

pub(crate) fn kick(m: &mut Messenger, chat: &str, user: &str) -> Value {
    commit(m.kick(chat, user))
}

pub(crate) fn set_admin(m: &mut Messenger, chat: &str, user: &str, admin: bool) -> Value {
    commit(m.set_admin(chat, user, admin))
}

pub(crate) fn hand_over(m: &mut Messenger, chat: &str, user: &str) -> Value {
    commit(m.hand_over(chat, user))
}

pub(crate) fn rename_group(m: &mut Messenger, chat: &str, name: &str) -> Value {
    commit(m.rename_group(chat, name))
}

/// `minutes`: from now; null: the group stays.
pub(crate) fn set_group_end(m: &mut Messenger, chat: &str, minutes: Option<u64>) -> Value {
    let ends = match minutes {
        Some(min) => match ends_in(min) {
            Some(e) => Some(e),
            None => return fail("bad_request"),
        },
        None => None,
    };
    commit(m.set_group_end(chat, ends, now()))
}

pub(crate) fn close_group(m: &mut Messenger, chat: &str) -> Value {
    commit(m.close_group(chat))
}

/// Before leaving: an owner hands on, an admin steps down. Without
/// `commit` nothing has to go to the server first.
pub(crate) fn leave_prepare(m: &mut Messenger, chat: &str) -> Value {
    match m.leave_prepare(chat) {
        Ok(p) => json!({"ok": true, "commit": p.as_ref().map(commit_json)}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn leave_group(m: &mut Messenger, chat: &str) -> Value {
    message(m.leave_group(chat, now()))
}

pub(crate) fn accept_group(m: &mut Messenger, chat: &str) -> Value {
    done(m.accept_group(chat))
}

pub(crate) fn delete_chat(m: &mut Messenger, chat: &str) -> Value {
    done(m.delete_chat(chat))
}

pub(crate) fn accept_new_key(m: &mut Messenger, chat: &str, user: &str) -> Value {
    done(m.accept_new_key(chat, user))
}

pub(crate) fn propose_update(m: &mut Messenger, chat: &str) -> Value {
    message(m.propose_update(chat))
}

/// What a group still needs: `commit` (an admin's change), `body` and
/// `to` (a member's request), or neither.
pub(crate) fn tidy_group(m: &mut Messenger, chat: &str) -> Value {
    match m.tidy_group(chat, now()) {
        Ok(Tidy::Nothing) => json!({"ok": true}),
        Ok(Tidy::Commit(p)) => json!({"ok": true, "commit": commit_json(&p)}),
        Ok(Tidy::Message { bytes, to }) => json!({"ok": true, "body": b64(&bytes), "to": to}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn create_group_invite(
    m: &mut Messenger,
    chat: &str,
    minutes: u64,
    guest: bool,
) -> Value {
    let Some(ends) = ends_in(minutes) else {
        return fail("bad_request");
    };
    match m.create_group_invite(chat, guest, ends, now()) {
        Ok(code) => json!({"ok": true, "code": code, "ends": ends, "guest": guest}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn revoke_group_invites(m: &mut Messenger, chat: &str) -> Value {
    match m.revoke_group_invites(chat) {
        Ok(n) => json!({"ok": true, "revoked": n}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn group_invites(m: &Messenger, chat: &str) -> Value {
    match m.group_invites(chat, now()) {
        Ok((count, last)) => json!({"ok": true, "count": count, "last": last}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn join_group(m: &mut Messenger, code: &str, list: Vec<Offer>) -> Value {
    let Some(offers) = offers(list) else {
        return fail("bad_request");
    };
    match m.join_group(code, &offers, now()) {
        Ok((chat, p)) => json!({
            "ok": true,
            "chat": chat_json(&chat, &chat_name(m, &chat)),
            "commit": commit_json(&p),
        }),
        Err(e) => core_error(e),
    }
}

/// The token for the admin, once the join chat exists. Without `body`:
/// it went out already.
pub(crate) fn join_message(m: &mut Messenger, chat: &str) -> Value {
    match m.join_message(chat) {
        Ok(Some((bytes, to))) => json!({"ok": true, "body": b64(&bytes), "to": to}),
        Ok(None) => json!({"ok": true}),
        Err(e) => core_error(e),
    }
}

pub(crate) fn join_requests(m: &Messenger) -> Value {
    match m.join_requests() {
        Ok(list) => json!({
            "ok": true,
            "requests": list.iter().map(|r| json!({
                "chat": r.chat,
                "group": r.group,
                "user": r.user,
                "guest_until": r.guest_until,
                "at": r.at,
            })).collect::<Vec<_>>(),
        }),
        Err(e) => core_error(e),
    }
}

pub(crate) fn admit(m: &mut Messenger, chat: &str, list: Vec<Offer>) -> Value {
    let Some(offers) = offers(list) else {
        return fail("bad_request");
    };
    commit(m.admit(chat, &offers))
}

pub(crate) fn close_join(m: &mut Messenger, chat: &str) -> Value {
    done(m.close_join(chat))
}

pub(crate) fn deny_join(m: &mut Messenger, chat: &str) -> Value {
    message(m.deny_join(chat))
}
