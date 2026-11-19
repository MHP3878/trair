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

fn verify(key: &[u8], message: &[u8], signature: &[u8]) -> Result<()> {
    OpenMlsRustCrypto::default()
        .crypto()
        .verify_signature(SCHEME, message, key, signature)
        .map_err(|_| CoreError::Rejected("signature is wrong".into()))
}
