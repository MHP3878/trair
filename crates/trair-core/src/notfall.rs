// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Emergency PINs ("Notfall-PINs").
//!
//! Besides the password, up to [`SLOTS`] PINs can be set. Typed on the
//! start screen instead of the password, a PIN deletes the account on this
//! device. Before that it can send one prepared message to one chat (for
//! example a call for help). Afterwards either the account is gone from
//! the list, or it stays and only ever says "wrong password".
//!
//! How it is stored:
//! - `notfall.trair` always holds [`SLOTS`] slots of equal size, also when
//!   no PIN is set (random filler). A slot is sealed with a key from the
//!   PIN (Argon2id, one run per attempt for all slots). Without a PIN the
//!   file does not show whether or how many PINs exist.
//! - A slot holds what its PIN does. A PIN that may send also holds the
//!   key of `notfall-daten.trair`, which carries what sending needs (the
//!   account and device keys, the chat key, the server address). Such a
//!   PIN can open the account like the password; a PIN that only deletes
//!   cannot open anything.
//! - The vault keeps the list of what each PIN does (not the PINs), so
//!   the settings can show and remove them.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::vault::{
    KdfParams, Limits, NONCE_LEN, SALT_LEN, VaultError, VaultResult, derive, internal, random,
};

/// File of the slots, next to the vault.
pub const FILE: &str = "notfall.trair";
/// File of what sending needs, next to the vault.
pub const DATA_FILE: &str = "notfall-daten.trair";
/// How many PINs at most (the file always has this many slots).
pub const SLOTS: usize = 3;
/// Shortest PIN that only deletes. It opens nothing: guessing it from a
/// copy of the files gives away no key.
pub const MIN_PIN_CHARS: usize = 6;
/// Shortest PIN that sends. It opens the account's keys (to send through
/// the chat), so it must be as strong as the password.
pub const MIN_SENDING_PIN_CHARS: usize = crate::vault::MIN_PASSWORD_CHARS;
/// Longest message a PIN sends.
pub const MAX_TEXT_CHARS: usize = 500;

/// Every slot's content is padded to this size before sealing.
const SLOT_PLAIN: usize = 4096;
const TAG_LEN: usize = 16;
const SLOT_CONTEXT: &[u8] = b"trair/notfall-slot/v1\0";
const DATA_CONTEXT: &[u8] = b"trair/notfall-daten/v1\0";

/// What the start screen shows after a PIN deleted the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum After {
    /// The account is gone from the list, as if it never existed.
    Vanish,
    /// The account stays in the list and only ever says "wrong password".
    WrongPassword,
}

/// One message a PIN sends before deleting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// The chat id.
    pub chat: String,
    /// The text.
    pub text: String,
}

/// What a PIN does, in order: send the message (if any), then delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    /// Random id, to remove the PIN later.
    pub id: String,
    /// What the start screen shows afterwards.
    pub after: After,
    /// Sent first, if set.
    pub message: Option<Message>,
}

/// What the vault remembers about a PIN (never the PIN itself).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Which slot of the file.
    pub slot: usize,
    pub action: Action,
}

/// A PIN that was typed: what it does, and the data key if it may send.
pub struct Opened {
    pub slot: usize,
    pub action: Action,
    pub data_key: Option<Zeroizing<[u8; 32]>>,
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opened")
            .field("slot", &self.slot)
            .field("action", &self.action)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
struct SlotPlain {
    v: u32,
    action: Action,
    #[serde(default)]
    data_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Slot {
    nonce: String,
    sealed: String,
}

/// The slot file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotFile {
    v: u32,
    m_kib: u32,
    t: u32,
    p: u32,
    salt: String,
    slots: Vec<Slot>,
}

fn filler() -> VaultResult<Slot> {
    let nonce = random::<NONCE_LEN>()?;
    let mut sealed = vec![0u8; SLOT_PLAIN + TAG_LEN];
    getrandom::fill(&mut sealed).map_err(internal)?;
    Ok(Slot {
        nonce: hex::encode(nonce),
        sealed: hex::encode(sealed),
    })
}

/// Check a PIN before it is set. `sends`: the PIN opens the keys.
pub fn check_pin(pin: &str, sends: bool) -> VaultResult<()> {
    let min = if sends {
        MIN_SENDING_PIN_CHARS
    } else {
        MIN_PIN_CHARS
    };
    if pin.chars().count() < min {
        return Err(VaultError::WeakPassword);
    }
    Ok(())
}

impl SlotFile {
    /// A file with only filler slots, as every vault gets one.
    pub fn new(params: KdfParams) -> VaultResult<Self> {
        Ok(SlotFile {
            v: 1,
            m_kib: params.m_kib,
            t: params.t,
            p: params.p,
            salt: hex::encode(random::<SALT_LEN>()?),
            slots: (0..SLOTS).map(|_| filler()).collect::<VaultResult<_>>()?,
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> VaultResult<Self> {
        let f: SlotFile = serde_json::from_slice(bytes)
            .map_err(|_| VaultError::Damaged("emergency file is not readable".into()))?;
        let sizes_ok = f.slots.len() == SLOTS
            && f.slots.iter().all(|s| {
                s.nonce.len() == NONCE_LEN * 2 && s.sealed.len() == (SLOT_PLAIN + TAG_LEN) * 2
            });
        if f.v != 1 || !sizes_ok || f.salt.len() != SALT_LEN * 2 {
            return Err(VaultError::Damaged("emergency file is not readable".into()));
        }
        Ok(f)
    }

    pub fn to_bytes(&self) -> VaultResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(internal)
    }

    fn params(&self) -> KdfParams {
        KdfParams {
            m_kib: self.m_kib,
            t: self.t,
            p: self.p,
        }
    }

    fn salt(&self) -> VaultResult<[u8; SALT_LEN]> {
        hex::decode(&self.salt)
            .ok()
            .and_then(|v| <[u8; SALT_LEN]>::try_from(v).ok())
            .ok_or_else(|| VaultError::Damaged("emergency file is not readable".into()))
    }

    fn aad(&self, index: usize) -> Vec<u8> {
        let mut aad = SLOT_CONTEXT.to_vec();
        aad.extend_from_slice(self.salt.as_bytes());
        aad.push(index as u8);
        aad
    }

    /// The key from a PIN: one Argon2id run for all slots.
    fn key(&self, pin: &str, limits: Limits) -> VaultResult<Zeroizing<[u8; 32]>> {
        limits.check(self.params())?;
        derive(pin, &self.salt()?, self.params())
    }

    /// Does `pin` belong to a slot? Wrong PINs and filler give `None`.
    pub fn open(&self, pin: &str, limits: Limits) -> VaultResult<Option<Opened>> {
        let key = self.key(pin, limits)?;
        let cipher = XChaCha20Poly1305::new(key.as_slice().into());
        for (i, slot) in self.slots.iter().enumerate() {
            let (Ok(nonce), Ok(sealed)) = (hex::decode(&slot.nonce), hex::decode(&slot.sealed))
            else {
                continue;
            };
            let aad = self.aad(i);
            let Ok(plain) = cipher.decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &sealed,
                    aad: &aad,
                },
            ) else {
                continue;
            };
            let plain = Zeroizing::new(plain);
            let content: SlotPlain = serde_json::from_slice(&plain)
                .map_err(|_| VaultError::Damaged("emergency slot is not readable".into()))?;
            let data_key = match content.data_key {
                Some(h) => {
                    let h = Zeroizing::new(h);
                    let mut k = Zeroizing::new([0u8; 32]);
                    hex::decode_to_slice(h.as_bytes(), k.as_mut_slice()).map_err(|_| {
                        VaultError::Damaged("emergency slot is not readable".into())
                    })?;
                    Some(k)
                }
                None => None,
            };
            return Ok(Some(Opened {
                slot: i,
                action: content.action,
                data_key,
            }));
        }
        Ok(None)
    }

    /// Seal `action` for `pin` into slot `index`. `data_key` only for a
    /// PIN that sends.
    pub fn set(
        &mut self,
        index: usize,
        pin: &str,
        action: Action,
        data_key: Option<&[u8; 32]>,
        limits: Limits,
    ) -> VaultResult<()> {
        if index >= SLOTS {
            return Err(VaultError::Refused("no such slot".into()));
        }
        check_pin(pin, data_key.is_some())?;
        if let Some(m) = &action.message
            && (m.text.trim().is_empty() || m.text.chars().count() > MAX_TEXT_CHARS)
        {
            return Err(VaultError::TooLarge);
        }
        let plain = SlotPlain {
            v: 1,
            action,
            data_key: data_key.map(hex::encode),
        };
        let mut bytes = Zeroizing::new(serde_json::to_vec(&plain).map_err(internal)?);
        if bytes.len() > SLOT_PLAIN {
            return Err(VaultError::TooLarge);
        }
        // Same size as filler: trailing spaces are still valid JSON.
        bytes.resize(SLOT_PLAIN, b' ');
        let key = self.key(pin, limits)?;
        let nonce = random::<NONCE_LEN>()?;
        let aad = self.aad(index);
        let sealed = XChaCha20Poly1305::new(key.as_slice().into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &bytes,
                    aad: &aad,
                },
            )
            .map_err(internal)?;
        self.slots[index] = Slot {
            nonce: hex::encode(nonce),
            sealed: hex::encode(sealed),
        };
        Ok(())
    }

    /// Turn a slot back into filler.
    pub fn clear(&mut self, index: usize) -> VaultResult<()> {
        if index >= SLOTS {
            return Err(VaultError::Refused("no such slot".into()));
        }
        self.slots[index] = filler()?;
        Ok(())
    }
}

/// Seal the emergency data with its key.
pub fn seal_data(key: &[u8; 32], plain: &[u8]) -> VaultResult<Vec<u8>> {
    let nonce = random::<NONCE_LEN>()?;
    let sealed = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: DATA_CONTEXT,
            },
        )
        .map_err(internal)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Open the emergency data.
pub fn open_data(key: &[u8; 32], file: &[u8]) -> VaultResult<Zeroizing<Vec<u8>>> {
    if file.len() < NONCE_LEN + TAG_LEN {
        return Err(VaultError::Damaged("emergency data is too short".into()));
    }
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(&file[..NONCE_LEN]),
            Payload {
                msg: &file[NONCE_LEN..],
                aad: DATA_CONTEXT,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| VaultError::Damaged("emergency data does not open".into()))
}

/// Random bytes in the place of the data file, while no PIN sends: the
/// file is always there, so its presence says nothing.
pub fn filler_data() -> VaultResult<Vec<u8>> {
    let len_byte = random::<1>()?[0] as usize;
    let mut out = vec![0u8; 2500 + len_byte * 8];
    getrandom::fill(&mut out).map_err(internal)?;
    Ok(out)
}

/// A random id for a new PIN.
pub fn new_id() -> VaultResult<String> {
    Ok(hex::encode(random::<8>()?))
}
