// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Emergency PINs behind the door (see `trair_core::notfall`).
//!
//! `unlock` with a PIN instead of the password: a PIN that only deletes
//! deletes at once; a PIN that sends first prepares the message and keeps
//! a short session, so the app can log in (`notfall_sign_login`), send,
//! delete the account on the server (`notfall_sign_deletion`) and then
//! call `notfall_finish`, which deletes everything here. Both answer like
//! a wrong password, plus `notfall: {after, send, ...}` for the app.

use std::path::Path;

use serde_json::{Value, json};
use trair_core::chats::{CHATS_FILE, ChatState};
use trair_core::notfall::{self, Action, After, DATA_FILE, Entry, FILE, Message, SLOTS, SlotFile};
use trair_core::vault::{KdfParams, Limits, Vault};
use trair_core::{Messenger, proofs};

/// A PIN that sends is running: what it needs until `notfall_finish`.
pub(crate) struct Session {
    vault: Vault,
    after: After,
}

fn after_str(a: After) -> &'static str {
    match a {
        After::Vanish => "vanish",
        After::WrongPassword => "wrong_password", // pragma: allowlist secret (Fehlercode)
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Make sure the slot file and the data file exist, and that the data
/// file carries the current secrets if a PIN may send. Called whenever
/// the vault was opened or stored.
pub(crate) fn refresh(
    dir: &Path,
    vault: &Vault,
    read: impl Fn(&Path) -> Result<Option<Vec<u8>>, &'static str>,
    write: impl Fn(&Path, &[u8]) -> Result<(), ()>,
) -> Result<(), ()> {
    let slots = dir.join(FILE);
    if !matches!(read(&slots), Ok(Some(_))) {
        let f = SlotFile::new(KdfParams::DEFAULT).map_err(|_| ())?;
        write(&slots, &f.to_bytes().map_err(|_| ())?)?;
    }
    let data = dir.join(DATA_FILE);
    match vault.notfall_key() {
        Some(key) => {
            let plain = vault.emergency_secrets().map_err(|_| ())?;
            write(&data, &notfall::seal_data(key, &plain).map_err(|_| ())?)?;
        }
        None => {
            if !matches!(read(&data), Ok(Some(_))) {
                write(&data, &notfall::filler_data().map_err(|_| ())?)?;
            }
        }
    }
    Ok(())
}

/// The PINs as the settings show them (never the PINs themselves).
pub(crate) fn list(vault: &Vault, chats: Option<&Messenger>) -> Value {
    let entries: Vec<Value> = vault
        .notfall_entries()
        .iter()
        .map(|e| {
            let chat_name = e.action.message.as_ref().and_then(|m| {
                let c = chats?.chat(&m.chat).ok()?;
                Some(
                    chats?
                        .contact(&c.peer)
                        .ok()
                        .flatten()
                        .map(|k| k.name)
                        .unwrap_or(c.peer),
                )
            });
            json!({
                "id": e.action.id,
                "after": after_str(e.action.after),
                "chat": e.action.message.as_ref().map(|m| m.chat.clone()),
                "chat_name": chat_name,
                "text": e.action.message.as_ref().map(|m| m.text.clone()),
            })
        })
        .collect();
    json!({"ok": true, "pins": entries, "max": SLOTS})
}

/// Set up a PIN. The password was checked by the caller.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add(
    dir: &Path,
    vault: &mut Vault,
    chats: Option<&Messenger>,
    password: &str,
    pin: &str,
    after: &str,
    chat: Option<String>,
    text: Option<String>,
    read: impl Fn(&Path) -> Result<Option<Vec<u8>>, &'static str>,
) -> Result<SlotFile, &'static str> {
    if notfall::check_pin(pin, chat.is_some()).is_err() {
        return Err(if chat.is_some() {
            "weak_sending_pin"
        } else {
            "weak_pin"
        });
    }
    if pin == password {
        return Err("pin_is_password");
    }
    let after = match after {
        "vanish" => After::Vanish,
        "wrong_password" => After::WrongPassword,
        _ => return Err("bad_request"),
    };
    let message = match (chat, text) {
        (Some(chat), Some(text)) => {
            let m = chats.ok_or("chats_unavailable")?;
            match m.chat(&chat) {
                Ok(c) if c.state == ChatState::Active => {}
                _ => return Err("unknown_chat"),
            }
            if text.trim().is_empty() || text.chars().count() > notfall::MAX_TEXT_CHARS {
                return Err("bad_request");
            }
            Some(Message { chat, text })
        }
        (None, None) => None,
        _ => return Err("bad_request"),
    };
    let mut slots = match read(&dir.join(FILE)) {
        Ok(Some(bytes)) => SlotFile::from_bytes(&bytes).map_err(|_| "damaged")?,
        _ => SlotFile::new(KdfParams::DEFAULT).map_err(|_| "internal")?,
    };
    if slots
        .open(pin, Limits::PRODUCTION)
        .map_err(|_| "internal")?
        .is_some()
    {
        return Err("pin_taken");
    }
    let used: Vec<usize> = vault.notfall_entries().iter().map(|e| e.slot).collect();
    let Some(index) = (0..SLOTS).find(|i| !used.contains(i)) else {
        return Err("full");
    };
    let key = if message.is_some() {
        Some(vault.ensure_notfall_key().map_err(|_| "internal")?)
    } else {
        None
    };
    let action = Action {
        id: notfall::new_id().map_err(|_| "internal")?,
        after,
        message,
    };
    slots
        .set(index, pin, action.clone(), key.as_ref(), Limits::PRODUCTION)
        .map_err(|_| "internal")?;
    let mut entries = vault.notfall_entries().to_vec();
    entries.push(Entry {
        slot: index,
        action,
    });
    vault.set_notfall_entries(entries);
    Ok(slots)
}

/// Remove a PIN: its slot becomes filler again.
pub(crate) fn remove(
    dir: &Path,
    vault: &mut Vault,
    id: &str,
    read: impl Fn(&Path) -> Result<Option<Vec<u8>>, &'static str>,
) -> Result<SlotFile, &'static str> {
    let mut entries = vault.notfall_entries().to_vec();
    let Some(pos) = entries.iter().position(|e| e.action.id == id) else {
        return Err("unknown_pin");
    };
    let entry = entries.remove(pos);
    let mut slots = match read(&dir.join(FILE)) {
        Ok(Some(bytes)) => SlotFile::from_bytes(&bytes).map_err(|_| "damaged")?,
        _ => SlotFile::new(KdfParams::DEFAULT).map_err(|_| "internal")?,
    };
    slots.clear(entry.slot).map_err(|_| "internal")?;
    vault.set_notfall_entries(entries);
    Ok(slots)
}

/// Did a PIN open a slot? Only tried when the password was wrong.
pub(crate) fn try_pin(
    dir: &Path,
    pin: &str,
    read: impl Fn(&Path) -> Result<Option<Vec<u8>>, &'static str>,
) -> Option<notfall::Opened> {
    let bytes = read(&dir.join(FILE)).ok()??;
    let slots = SlotFile::from_bytes(&bytes).ok()?;
    slots.open(pin, Limits::PRODUCTION).ok()?
}

/// Prepare what a sending PIN does: open the account from the data file,
/// encrypt the message, keep the session. `None` if that is not possible
/// (then the caller only deletes).
pub(crate) fn prepare(
    dir: &Path,
    opened: &notfall::Opened,
    read: impl Fn(&Path) -> Result<Option<Vec<u8>>, &'static str>,
) -> Option<(Session, Value)> {
    let message = opened.action.message.as_ref()?;
    let key = opened.data_key.as_ref()?;
    let file = read(&dir.join(DATA_FILE)).ok()??;
    let plain = notfall::open_data(key, &file).ok()?;
    let vault = Vault::from_emergency_secrets(&plain).ok()?;
    let mut m = Messenger::open(
        &dir.join(CHATS_FILE),
        vault.chats_key(),
        vault.account(),
        vault.device(),
        &vault.device_list().list,
        now(),
    )
    .ok()?;
    let to = m.recipients(&message.chat).ok()?;
    let (bytes, _) = m.send(&message.chat, &message.text, now()).ok()?;
    use base64::Engine;
    let info = json!({
        "after": after_str(opened.action.after),
        "send": true,
        "server": vault.profile().get("server").cloned().unwrap_or(Value::Null),
        "user": vault.account().user(),
        "device": vault.device().id,
        "body": base64::engine::general_purpose::STANDARD.encode(bytes),
        "to": to,
    });
    Some((
        Session {
            vault,
            after: opened.action.after,
        },
        info,
    ))
}

impl Session {
    pub(crate) fn after(&self) -> After {
        self.after
    }

    pub(crate) fn sign_login(&self, challenge: &str) -> Value {
        let Ok(challenge) = hex::decode(challenge) else {
            return json!({"ok": false, "error": "bad_request"});
        };
        match proofs::sign_login(self.vault.device(), self.vault.account().user(), &challenge) {
            Ok(sig) => json!({"ok": true, "signature": hex::encode(sig)}),
            Err(_) => json!({"ok": false, "error": "bad_request"}),
        }
    }

    pub(crate) fn sign_deletion(&self, now: u64) -> Value {
        match proofs::sign_account_deletion(self.vault.account(), now) {
            Ok(sig) => json!({"ok": true, "signature": hex::encode(sig)}),
            Err(_) => json!({"ok": false, "error": "internal"}),
        }
    }
}

/// What a PIN answers, like a wrong password.
pub(crate) fn answer(wait: u64, info: Value) -> Value {
    json!({"ok": false, "error": "wrong_password", "wait": wait, "notfall": info})
}

pub(crate) fn info_without_send(after: After) -> Value {
    json!({"after": after_str(after), "send": false})
}
