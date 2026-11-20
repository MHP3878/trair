// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Checking messages of open community channels (M8, step 3a).
//!
//! Open channels are readable on the server, but every message carries a
//! signature of the sender's device (see [`crate::proofs::OpenMessage`]).
//! This module checks it against the sender's device list, signed by the
//! sender's account key, and says how far that key is known:
//!
//! - a contact's pinned key (compared in person or not),
//! - otherwise the key first seen for that name in any community (kept
//!   here, so the server cannot swap it later without a warning),
//! - or a name seen for the first time.
//!
//! The server could still invent an account for a name nobody here has
//! seen before; that is what "new" says, and why the app marks only
//! contacts as trusted.

use rusqlite::{OptionalExtension, params};

use crate::account::Fingerprint;
use crate::chats::Messenger;
use crate::devices::SignedDeviceList;
use crate::error::Result;
use crate::proofs::{OpenMessage, verify_open_message};
use crate::store::sql;

/// What a signature check says about one open message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    /// Written by one of this account's devices.
    Own,
    /// A contact whose key was compared in person.
    Verified,
    /// A contact (key from their invitation, not compared).
    Contact,
    /// Not a contact; the same key as the first time.
    Known,
    /// Not a contact and never seen before: the key is pinned now.
    New,
    /// Correctly signed, but by another account key than the one pinned
    /// for this name. Either a new account or the server lying.
    Changed,
    /// No valid signature (forged, changed text, withdrawn device).
    Bad,
}

impl Author {
    /// The word the door answers.
    pub fn as_str(self) -> &'static str {
        match self {
            Author::Own => "own",
            Author::Verified => "verified",
            Author::Contact => "contact",
            Author::Known => "known",
            Author::New => "new",
            Author::Changed => "changed",
            Author::Bad => "bad",
        }
    }
}

/// The sender's keys, as the server handed them out.
#[derive(Debug, Clone)]
pub struct AuthorKeys {
    /// Public account key.
    pub account_key: Vec<u8>,
    /// Device list signed by that key.
    pub device_list: SignedDeviceList,
}

impl Messenger {
    /// Check one open message. Nothing is pinned for a bad signature.
    pub fn check_open_message(
        &mut self,
        m: &OpenMessage,
        signature: &[u8],
        keys: Option<&AuthorKeys>,
    ) -> Result<Author> {
        let Some(keys) = keys else {
            return Ok(Author::Bad);
        };
        let Ok(list) = keys.device_list.verify(&keys.account_key, None) else {
            return Ok(Author::Bad);
        };
        if list.user != m.user {
            return Ok(Author::Bad);
        }
        let Some(entry) = list.devices.iter().find(|d| d.id == m.device) else {
            return Ok(Author::Bad);
        };
        if verify_open_message(entry, m, signature).is_err() {
            return Ok(Author::Bad);
        }
        let fp = Fingerprint::of(&keys.account_key);
        if m.user == self.user {
            return Ok(if &fp == self.device.account_fingerprint() {
                Author::Own
            } else {
                Author::Changed
            });
        }
        if let Some(c) = self.contact(m.user)? {
            return Ok(match (c.fingerprint == fp, c.confirmed) {
                (true, true) => Author::Verified,
                (true, false) => Author::Contact,
                (false, _) => Author::Changed,
            });
        }
        let pinned: Option<String> = self
            .store
            .conn()
            .query_row("SELECT fp FROM open_pins WHERE user = ?1", [m.user], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)?;
        Ok(match pinned {
            Some(p) if p == fp.as_str() => Author::Known,
            Some(_) => Author::Changed,
            None => {
                self.store
                    .conn()
                    .execute(
                        "INSERT INTO open_pins (user, fp) VALUES (?1, ?2)",
                        params![m.user, fp.as_str()],
                    )
                    .map_err(sql)?;
                Author::New
            }
        })
    }

    /// Accept another account key for a name that is not a contact (after
    /// a [`Author::Changed`] the person explained).
    pub fn accept_open_key(&mut self, user: &str, account_key: &[u8]) -> Result<()> {
        let fp = Fingerprint::of(account_key);
        self.store
            .conn()
            .execute(
                "INSERT INTO open_pins (user, fp) VALUES (?1, ?2)
                 ON CONFLICT (user) DO UPDATE SET fp = excluded.fp",
                params![user, fp.as_str()],
            )
            .map_err(sql)?;
        Ok(())
    }
}
