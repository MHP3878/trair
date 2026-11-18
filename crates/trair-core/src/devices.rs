// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The devices of one account (milestone M2).
//!
//! Every device has its own signing key. The account key certifies it
//! ([`crate::DeviceCertificate`]) and lists it in a [`SignedDeviceList`].
//! The list says which devices belong to the account and which were
//! withdrawn ("zurückgezogen"). It carries a sequence number, so an old
//! list cannot be passed off as the current one.
//!
//! Device names that people choose ("Julians Laptop") stay on the device
//! and are never part of the list: the server and contacts only ever see
//! random device ids.
//!
//! Limit, said openly: every paired device holds the full account key
//! (decision of 18.11.2026, so a second device can rescue the account).
//! Withdrawing a lost device stops it from being accepted, but if someone
//! also knows that device's password they have the account key and can
//! certify new devices. Then only a new account helps, and contacts see
//! the change of account fingerprint.

use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;
use openmls_traits::crypto::OpenMlsCrypto;
use openmls_traits::signatures::Signer;
use serde::{Deserialize, Serialize};

use crate::account::{Account, Fingerprint, SCHEME, validate_name};
use crate::error::{CoreError, Result, internal};

const LIST_CONTEXT: &[u8] = b"trair/device-list/v1\0";

/// Longest device name, in characters.
pub const MAX_LABEL_CHARS: usize = 40;

/// Check a device name chosen by a person.
pub fn validate_label(label: &str) -> Result<()> {
    let n = label.chars().count();
    if n == 0 || n > MAX_LABEL_CHARS || label.chars().any(char::is_control) || label.trim() != label
    {
        return Err(CoreError::InvalidName(format!(
            "device name must be 1 to {MAX_LABEL_CHARS} characters, no control characters, no outer spaces"
        )));
    }
    Ok(())
}

/// A new random device id: `d-` and 8 hex digits.
pub fn new_device_id() -> Result<String> {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).map_err(internal)?;
    Ok(format!("d-{}", hex::encode(b)))
}

/// One device in the list: its id and its public signing key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    /// Random id, `d-xxxxxxxx`.
    pub id: String,
    /// Public signing key of the device, hex.
    pub key: String,
}

impl DeviceEntry {
    /// Fingerprint of the device key, as shown in the app.
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(&hex::decode(&self.key).unwrap_or_default())
    }
}

/// The content of a device list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceList {
    /// Format version, 1.
    pub v: u32,
    /// The account's user name.
    pub user: String,
    /// Increases with every change.
    pub seq: u64,
    /// Devices that belong to the account.
    pub devices: Vec<DeviceEntry>,
    /// Devices that were withdrawn. They stay listed for good, so they can
    /// never come back with an older list.
    pub withdrawn: Vec<DeviceEntry>,
}

/// A device list plus the account's signature over it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedDeviceList {
    /// The list.
    pub list: DeviceList,
    /// Signature of the account key, hex.
    pub signature: String,
}

impl DeviceList {
    fn signed_bytes(&self) -> Vec<u8> {
        let mut b = LIST_CONTEXT.to_vec();
        // A plain struct of strings and numbers always serializes.
        b.extend_from_slice(&serde_json::to_vec(self).unwrap_or_default());
        b
    }

    /// Sign with the account key.
    pub fn sign(self, account: &Account) -> Result<SignedDeviceList> {
        if self.user != account.user() {
            return Err(CoreError::Internal(
                "device list is for another user".into(),
            ));
        }
        let signature = account
            .keys()
            .sign(&self.signed_bytes())
            .map_err(internal)?;
        Ok(SignedDeviceList {
            list: self,
            signature: hex::encode(signature),
        })
    }

    /// Is this device id currently part of the account?
    pub fn contains(&self, id: &str) -> bool {
        self.devices.iter().any(|d| d.id == id)
    }

    /// Was this device id withdrawn?
    pub fn is_withdrawn(&self, id: &str) -> bool {
        self.withdrawn.iter().any(|d| d.id == id)
    }
}

impl SignedDeviceList {
    /// The first list of a new account: just this device.
    pub fn first(account: &Account, device: DeviceEntry) -> Result<Self> {
        DeviceList {
            v: 1,
            user: account.user().to_string(),
            seq: 1,
            devices: vec![device],
            withdrawn: vec![],
        }
        .sign(account)
    }

    /// Check the signature against the account key and return the list.
    /// `newer_than`: the sequence number of the list already known; an
    /// older or equal one is refused (no rollback).
    pub fn verify(
        &self,
        account_public_key: &[u8],
        newer_than: Option<u64>,
    ) -> Result<&DeviceList> {
        if self.list.v != 1 {
            return Err(CoreError::Rejected(format!(
                "unknown device list version {}",
                self.list.v
            )));
        }
        let signature = hex::decode(&self.signature)
            .map_err(|_| CoreError::Rejected("device list signature is not hex".into()))?;
        OpenMlsRustCrypto::default()
            .crypto()
            .verify_signature(
                SCHEME,
                &self.list.signed_bytes(),
                account_public_key,
                &signature,
            )
            .map_err(|_| CoreError::Rejected("device list is not signed by the account".into()))?;
        if let Some(known) = newer_than
            && self.list.seq <= known
        {
            return Err(CoreError::Rejected(format!(
                "device list {} is not newer than {known}",
                self.list.seq
            )));
        }
        Ok(&self.list)
    }

    /// A new list with one more device.
    pub fn with_device(&self, account: &Account, device: DeviceEntry) -> Result<Self> {
        validate_name(&device.id)?;
        if self.list.contains(&device.id) || self.list.is_withdrawn(&device.id) {
            return Err(CoreError::Rejected(format!(
                "device {} is already known",
                device.id
            )));
        }
        // An Ed25519 public key, and one never seen before: a withdrawn key
        // must not come back under a new id.
        if hex::decode(&device.key).map(|k| k.len()) != Ok(32) {
            return Err(CoreError::Rejected("device key is not a public key".into()));
        }
        let mut known = self.list.devices.iter().chain(&self.list.withdrawn);
        if known.any(|d| d.key.eq_ignore_ascii_case(&device.key)) {
            return Err(CoreError::Rejected("device key is already known".into()));
        }
        let mut list = self.list.clone();
        list.seq += 1;
        list.devices.push(device);
        list.sign(account)
    }

    /// A new list without `id`, which is listed as withdrawn from now on.
    pub fn without_device(&self, account: &Account, id: &str) -> Result<Self> {
        let mut list = self.list.clone();
        let Some(pos) = list.devices.iter().position(|d| d.id == id) else {
            return Err(CoreError::Rejected(format!(
                "device {id} is not in the list"
            )));
        };
        let entry = list.devices.remove(pos);
        list.withdrawn.push(entry);
        list.seq += 1;
        list.sign(account)
    }
}

/// This device's own identity: its id and its signing key. Kept in the
/// vault; the certificate is made fresh from the account key when needed.
pub struct DeviceIdentity {
    /// Random id, `d-xxxxxxxx`.
    pub id: String,
    /// The device's signing key pair.
    pub keys: SignatureKeyPair,
}

impl std::fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceIdentity")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl DeviceIdentity {
    /// A new device with a random id and a fresh key.
    pub fn generate() -> Result<Self> {
        Ok(DeviceIdentity {
            id: new_device_id()?,
            keys: SignatureKeyPair::new(SCHEME).map_err(internal)?,
        })
    }

    /// The entry for the device list.
    pub fn entry(&self) -> DeviceEntry {
        DeviceEntry {
            id: self.id.clone(),
            key: hex::encode(self.keys.public()),
        }
    }
}
