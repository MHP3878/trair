// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Communities behind the door (M8, step 3a). The app talks to the
//! server's community API itself; in here only what needs keys:
//!
//! | op                    | fields                                          | answer on success |
//! |-----------------------|-------------------------------------------------|-------------------|
//! | `sign_open_message`   | `community`, `channel`, `text`, `sent`          | `signature` (hex), `device`, `user` |
//! | `check_open_messages` | `community`, `channel`, `messages`, `authors`   | `results`         |
//! | `accept_open_key`     | `user`, `account_key`                           |                   |
//!
//! `messages` are `{user, device, text, sent, sig}` as the server gives
//! them; `authors` maps each user to `{account_key, device_list}` from
//! `GET /v1/accounts/<user>`. `results` has one word per message, in the
//! same order: `own`, `verified`, `contact`, `known`, `new`, `changed`
//! or `bad` (see [`trair_core::Author`]).

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Value, json};
use trair_core::proofs::{self, OpenMessage};
use trair_core::{AuthorKeys, Messenger, SignedDeviceList, Vault};

use crate::chats::{core_error, fail};

/// Most messages checked at once.
pub const MAX_CHECK: usize = 500;

#[derive(Deserialize)]
pub(crate) struct OpenItem {
    user: String,
    device: String,
    text: String,
    sent: u64,
    sig: String,
}

#[derive(Deserialize)]
pub(crate) struct AuthorItem {
    account_key: String,
    device_list: SignedDeviceList,
}

/// Sign a message for an open channel with this device.
pub(crate) fn sign_open_message(
    vault: &Vault,
    community: &str,
    channel: &str,
    text: &str,
    sent: u64,
) -> Value {
    let m = OpenMessage {
        community,
        channel,
        user: vault.account().user(),
        device: &vault.device().id,
        sent,
        text,
    };
    match proofs::sign_open_message(vault.device(), &m) {
        Ok(sig) => json!({"ok": true, "signature": hex::encode(sig),
            "device": vault.device().id, "user": vault.account().user()}),
        Err(_) => fail("bad_request"),
    }
}

/// Check the signatures of open messages and how well their keys are known.
pub(crate) fn check_open_messages(
    m: &mut Messenger,
    community: &str,
    channel: &str,
    messages: Vec<OpenItem>,
    authors: HashMap<String, AuthorItem>,
) -> Value {
    if messages.len() > MAX_CHECK {
        return fail("too_large");
    }
    let mut keys: HashMap<String, AuthorKeys> = HashMap::new();
    for (user, a) in authors {
        let Ok(account_key) = hex::decode(&a.account_key) else {
            continue;
        };
        keys.insert(
            user,
            AuthorKeys {
                account_key,
                device_list: a.device_list,
            },
        );
    }
    let mut results = Vec::with_capacity(messages.len());
    for item in &messages {
        let msg = OpenMessage {
            community,
            channel,
            user: &item.user,
            device: &item.device,
            sent: item.sent,
            text: &item.text,
        };
        let Ok(sig) = hex::decode(&item.sig) else {
            results.push("bad");
            continue;
        };
        match m.check_open_message(&msg, &sig, keys.get(&item.user)) {
            Ok(a) => results.push(a.as_str()),
            Err(e) => return core_error(e),
        }
    }
    json!({"ok": true, "results": results})
}

/// Take another key for a name that is not a contact.
pub(crate) fn accept_open_key(m: &mut Messenger, user: &str, account_key: &str) -> Value {
    let Ok(key) = hex::decode(account_key) else {
        return fail("bad_request");
    };
    if key.len() != 32 {
        return fail("bad_request");
    }
    match m.accept_open_key(user, &key) {
        Ok(()) => json!({"ok": true}),
        Err(e) => core_error(e),
    }
}
