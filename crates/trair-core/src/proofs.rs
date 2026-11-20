// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Signed proofs a client sends to the Trair server (M3). The app and the
//! server use this same code, so they cannot disagree about the bytes.
//!
//! - **Login:** the server sends a random challenge; the device signs it
//!   together with the user and device id. No password ever reaches the
//!   server.
//! - **Account deletion:** the account key signs user and time, so a
//!   stolen session token alone cannot delete an account.

use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;
use openmls_traits::crypto::OpenMlsCrypto;
use openmls_traits::signatures::Signer;

use crate::account::{Account, SCHEME};
use crate::devices::{DeviceEntry, DeviceIdentity};
use crate::error::{CoreError, Result, internal};

const LOGIN: &[u8] = b"trair/login/v1\0";
const DELETE: &[u8] = b"trair/delete-account/v1\0";

/// Length of a login challenge, in bytes.
pub const CHALLENGE_LEN: usize = 32;

fn login_bytes(user: &str, device_id: &str, challenge: &[u8]) -> Vec<u8> {
    let mut b = LOGIN.to_vec();
    for part in [user.as_bytes(), device_id.as_bytes(), challenge] {
        b.extend_from_slice(&(part.len() as u64).to_le_bytes());
        b.extend_from_slice(part);
    }
    b
}

fn delete_bytes(user: &str, now: u64) -> Vec<u8> {
    let mut b = DELETE.to_vec();
    b.extend_from_slice(&(user.len() as u64).to_le_bytes());
    b.extend_from_slice(user.as_bytes());
    b.extend_from_slice(&now.to_le_bytes());
    b
}

/// The device signs the server's challenge.
pub fn sign_login(device: &DeviceIdentity, user: &str, challenge: &[u8]) -> Result<Vec<u8>> {
    if challenge.len() != CHALLENGE_LEN {
        return Err(CoreError::Rejected("challenge length".into()));
    }
    device
        .keys
        .sign(&login_bytes(user, &device.id, challenge))
        .map_err(internal)
}

/// The server checks a login against the device's entry in the account's
/// current device list.
pub fn verify_login(
    device: &DeviceEntry,
    user: &str,
    challenge: &[u8],
    signature: &[u8],
) -> Result<()> {
    if challenge.len() != CHALLENGE_LEN {
        return Err(CoreError::Rejected("challenge length".into()));
    }
    let key = hex::decode(&device.key).map_err(|_| CoreError::Rejected("device key".into()))?;
    verify(&key, &login_bytes(user, &device.id, challenge), signature)
}

/// The account key agrees to delete the account at unix time `now`.
pub fn sign_account_deletion(account: &Account, now: u64) -> Result<Vec<u8>> {
    account
        .keys()
        .sign(&delete_bytes(account.user(), now))
        .map_err(internal)
}

/// The server checks a deletion request against the stored account key.
pub fn verify_account_deletion(
    account_key: &[u8],
    user: &str,
    now: u64,
    signature: &[u8],
) -> Result<()> {
    verify(account_key, &delete_bytes(user, now), signature)
}

/// What a device signs for a message in an open community channel. The
/// server can read such a message and delete it, but not write one in
/// someone's name or change it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMessage<'a> {
    /// Community id.
    pub community: &'a str,
    /// Channel id.
    pub channel: &'a str,
    /// Sending account.
    pub user: &'a str,
    /// Sending device id (`d-…`).
    pub device: &'a str,
    /// When the device sent it (seconds since 1970).
    pub sent: u64,
    /// The text.
    pub text: &'a str,
}

const OPEN_MESSAGE: &[u8] = b"trair/open-message/v1\0";
/// Messages of closed channels sign with another context: one can never
/// pass for the other.
const CLOSED_MESSAGE: &[u8] = b"trair/closed-message/v1\0";

fn open_bytes(m: &OpenMessage) -> Vec<u8> {
    channel_bytes(OPEN_MESSAGE, m)
}

fn channel_bytes(context: &[u8], m: &OpenMessage) -> Vec<u8> {
    let mut b = context.to_vec();
    for part in [m.community, m.channel, m.user, m.device, m.text] {
        b.extend_from_slice(&(part.len() as u64).to_le_bytes());
        b.extend_from_slice(part.as_bytes());
    }
    b.extend_from_slice(&m.sent.to_le_bytes());
    b
}

/// Sign a message for an open channel with this device's key.
pub fn sign_open_message(device: &DeviceIdentity, m: &OpenMessage) -> Result<Vec<u8>> {
    if m.device != device.id {
        return Err(CoreError::Rejected("not this device".into()));
    }
    device.keys.sign(&open_bytes(m)).map_err(internal)
}

/// Check a message of an open channel against the sending device's entry
/// in its account's (verified) device list.
pub fn verify_open_message(device: &DeviceEntry, m: &OpenMessage, signature: &[u8]) -> Result<()> {
    if device.id != m.device {
        return Err(CoreError::Rejected("another device".into()));
    }
    let key = hex::decode(&device.key).map_err(|_| CoreError::Rejected("device key".into()))?;
    verify(&key, &open_bytes(m), signature)
}

/// Sign a message of a closed (end-to-end encrypted) channel. It travels
/// inside the MLS message, so moderators can check a report and newcomers
/// the history a keeper hands them.
pub fn sign_closed_message(device: &DeviceIdentity, m: &OpenMessage) -> Result<Vec<u8>> {
    if m.device != device.id {
        return Err(CoreError::Rejected("not this device".into()));
    }
    device
        .keys
        .sign(&channel_bytes(CLOSED_MESSAGE, m))
        .map_err(internal)
}

/// Check a message of a closed channel against the sending device's public
/// key (from its MLS leaf or its account's verified device list).
pub fn verify_closed_message(device_key: &[u8], m: &OpenMessage, signature: &[u8]) -> Result<()> {
    verify(device_key, &channel_bytes(CLOSED_MESSAGE, m), signature)
}

pub(crate) fn verify(key: &[u8], message: &[u8], signature: &[u8]) -> Result<()> {
    OpenMlsRustCrypto::default()
        .crypto()
        .verify_signature(SCHEME, message, key, signature)
        .map_err(|_| CoreError::Rejected("signature is wrong".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_and_open_signatures_never_stand_in_for_each_other() {
        let device = DeviceIdentity::generate().unwrap();
        let entry = device.entry();
        let key = hex::decode(&entry.key).unwrap();
        let m = OpenMessage {
            community: "c1",
            channel: "team",
            user: "olga",
            device: &device.id,
            sent: 1_800_000_000,
            text: "geheim",
        };
        let closed = sign_closed_message(&device, &m).unwrap();
        verify_closed_message(&key, &m, &closed).unwrap();
        assert!(verify_open_message(&entry, &m, &closed).is_err());
        let open = sign_open_message(&device, &m).unwrap();
        assert!(verify_closed_message(&key, &m, &open).is_err());
        let other = OpenMessage {
            text: "anders",
            ..m.clone()
        };
        assert!(verify_closed_message(&key, &other, &closed).is_err());
        let stranger = DeviceIdentity::generate().unwrap();
        let wrong = OpenMessage {
            device: &stranger.id,
            ..m.clone()
        };
        assert!(sign_closed_message(&device, &wrong).is_err());
        assert!(
            verify_closed_message(&hex::decode(stranger.entry().key).unwrap(), &m, &closed)
                .is_err()
        );
    }

    #[test]
    fn open_messages_are_bound_to_device_and_every_field() {
        let device = DeviceIdentity::generate().unwrap();
        let entry = device.entry();
        let m = OpenMessage {
            community: "c1",
            channel: "allgemein",
            user: "olga",
            device: &device.id,
            sent: 1_800_000_000,
            text: "Hallo zusammen",
        };
        let sig = sign_open_message(&device, &m).unwrap();
        verify_open_message(&entry, &m, &sig).unwrap();
        let changed = [
            OpenMessage {
                community: "c2",
                ..m.clone()
            },
            OpenMessage {
                channel: "regeln",
                ..m.clone()
            },
            OpenMessage {
                user: "ada",
                ..m.clone()
            },
            OpenMessage {
                sent: m.sent + 1,
                ..m.clone()
            },
            OpenMessage {
                text: "Hallo zusammen!",
                ..m.clone()
            },
        ];
        for c in &changed {
            assert!(verify_open_message(&entry, c, &sig).is_err(), "{c:?}");
        }
        // Another device's key, or a message naming another device.
        let other = DeviceIdentity::generate().unwrap();
        assert!(verify_open_message(&other.entry(), &m, &sig).is_err());
        assert!(sign_open_message(&other, &m).is_err());
        // Shifting text between fields does not give the same bytes.
        let a = OpenMessage {
            channel: "ab",
            text: "c",
            ..m.clone()
        };
        let b = OpenMessage {
            channel: "a",
            text: "bc",
            ..m.clone()
        };
        assert_ne!(open_bytes(&a), open_bytes(&b));
    }
}
