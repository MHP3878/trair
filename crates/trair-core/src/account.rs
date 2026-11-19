// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Accounts and device certificates.
//!
//! An account is a random signing key pair. There is deliberately no
//! recovery phrase and no way to derive the key again: if every device
//! of an account is lost or locked, the account is gone (decision 2,
//! changed on 2026-11-18).
//!
//! Every device has its own MLS signing key. The account key signs a
//! certificate that binds `user`, `device` and that device key. The
//! certificate travels inside the MLS credential, so every member of a
//! group can check it without asking the server. Contacts verify a
//! person once (the account fingerprint); all devices signed by that
//! account are then trusted.

use openmls::prelude::SignatureScheme;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;
use openmls_traits::crypto::OpenMlsCrypto;
use openmls_traits::signatures::Signer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{CoreError, Result, internal};

/// Signature scheme for account and device keys.
pub const SCHEME: SignatureScheme = SignatureScheme::ED25519;

/// Domain separation for device certificates.
const CERT_CONTEXT: &[u8] = b"trair/device-cert/v1\0";

/// User and device names: 1 to 32 characters, lowercase letters, digits,
/// `_` and `-`. No separators that could make two names collide.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(CoreError::InvalidName(name.to_string()))
    }
}

/// Short, comparable fingerprint of an account key (first 16 bytes of
/// SHA-256, lowercase hex). People compare it in person or by QR code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint(String);

impl Fingerprint {
    /// Fingerprint of a raw public key.
    pub fn of(public_key: &[u8]) -> Self {
        Fingerprint(hex::encode(&Sha256::digest(public_key)[..16]))
    }

    /// Read a fingerprint written as 32 lowercase hex characters (as in a
    /// contact code). Anything else is refused.
    pub fn parse(text: &str) -> Result<Self> {
        let ok = text.len() == 32
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if ok {
            Ok(Fingerprint(text.to_string()))
        } else {
            Err(CoreError::Rejected("not a fingerprint".into()))
        }
    }

    /// The fingerprint as 32 hex characters.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The fingerprint in groups of four characters, for display.
    pub fn grouped(&self) -> String {
        self.0
            .as_bytes()
            .chunks(4)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// An account: the user name plus the account signing key.
pub struct Account {
    user: String,
    keys: SignatureKeyPair,
}

impl Account {
    /// Create a new account with a fresh random key.
    pub fn generate(user: &str) -> Result<Self> {
        validate_name(user)?;
        let keys = SignatureKeyPair::new(SCHEME).map_err(internal)?;
        Ok(Account {
            user: user.to_string(),
            keys,
        })
    }

    /// The user name.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The public account key.
    pub fn public_key(&self) -> &[u8] {
        self.keys.public()
    }

    /// Fingerprint other people verify.
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(self.keys.public())
    }

    /// Rebuild an account from stored keys (see [`crate::vault`]). Signs
    /// and verifies a probe, so a private key that does not belong to the
    /// public key is refused.
    pub(crate) fn from_keys(user: &str, keys: SignatureKeyPair) -> Result<Self> {
        validate_name(user)?;
        if keys.signature_scheme() != SCHEME {
            return Err(CoreError::Internal("unexpected signature scheme".into()));
        }
        let probe = b"trair/account-probe/v1";
        let signature = keys.sign(probe).map_err(internal)?;
        OpenMlsRustCrypto::default()
            .crypto()
            .verify_signature(SCHEME, probe, keys.public(), &signature)
            .map_err(|_| CoreError::Internal("account keys do not match".into()))?;
        Ok(Account {
            user: user.to_string(),
            keys,
        })
    }

    /// The key pair, only for sealing it into the vault.
    pub(crate) fn keys(&self) -> &SignatureKeyPair {
        &self.keys
    }

    /// Sign a certificate for one of this account's devices.
    pub fn certify_device(
        &self,
        device: &str,
        device_signature_key: &[u8],
    ) -> Result<DeviceCertificate> {
        validate_name(device)?;
        let payload = cert_payload(&self.user, device, device_signature_key);
        let signature = self.keys.sign(&payload).map_err(internal)?;
        Ok(DeviceCertificate {
            v: 1,
            user: self.user.clone(),
            device: device.to_string(),
            account_key: hex::encode(self.keys.public()),
            signature: hex::encode(signature),
        })
    }
}

fn cert_payload(user: &str, device: &str, device_signature_key: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(CERT_CONTEXT.len() + user.len() + device.len() + 40);
    p.extend_from_slice(CERT_CONTEXT);
    p.extend_from_slice(user.as_bytes());
    p.push(0);
    p.extend_from_slice(device.as_bytes());
    p.push(0);
    p.extend_from_slice(device_signature_key);
    p
}

/// Certificate that binds a device key to an account. Encoded as the
/// identity of the MLS basic credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCertificate {
    v: u8,
    user: String,
    device: String,
    account_key: String,
    signature: String,
}

/// Result of checking a certificate against the device key it came with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedCertificate {
    /// `user:device`
    pub id: String,
    /// The user name.
    pub user: String,
    /// Fingerprint of the account key that signed the device.
    pub account: Fingerprint,
    /// The account key itself (public).
    pub account_key: Vec<u8>,
}

impl DeviceCertificate {
    /// `user:device`
    pub fn id(&self) -> String {
        format!("{}:{}", self.user, self.device)
    }

    /// Encode for the MLS credential.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Serializing a plain struct of strings cannot fail.
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Decode and check against the device signature key from the MLS
    /// leaf node. Any failure means the device is not what it claims.
    pub fn check(identity: &[u8], device_signature_key: &[u8]) -> Result<CheckedCertificate> {
        let cert: DeviceCertificate = serde_json::from_slice(identity)
            .map_err(|_| CoreError::Forged("credential is not a Trair certificate".into()))?;
        if cert.v != 1 {
            return Err(CoreError::Forged(format!(
                "unknown certificate version {}",
                cert.v
            )));
        }
        validate_name(&cert.user).map_err(|_| CoreError::Forged("bad user name".into()))?;
        validate_name(&cert.device).map_err(|_| CoreError::Forged("bad device name".into()))?;
        let account_key = hex::decode(&cert.account_key)
            .map_err(|_| CoreError::Forged("account key is not hex".into()))?;
        let signature = hex::decode(&cert.signature)
            .map_err(|_| CoreError::Forged("signature is not hex".into()))?;
        let payload = cert_payload(&cert.user, &cert.device, device_signature_key);
        let provider = OpenMlsRustCrypto::default();
        provider
            .crypto()
            .verify_signature(SCHEME, &payload, &account_key, &signature)
            .map_err(|_| {
                CoreError::Forged(format!(
                    "{} is not signed by the account key it names",
                    cert.id()
                ))
            })?;
        Ok(CheckedCertificate {
            id: cert.id(),
            user: cert.user,
            account: Fingerprint::of(&account_key),
            account_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_restricted() {
        assert!(validate_name("bob").is_ok());
        assert!(validate_name("bob-2_pc").is_ok());
        for bad in ["", "Bob", "bob:pc", "bob pc", "bob\0", &"a".repeat(33)] {
            assert!(validate_name(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn certificate_round_trip() {
        let acc = Account::generate("bob").unwrap();
        let device = SignatureKeyPair::new(SCHEME).unwrap();
        let cert = acc.certify_device("pc", device.public()).unwrap();
        let checked = DeviceCertificate::check(&cert.to_bytes(), device.public()).unwrap();
        assert_eq!(checked.id, "bob:pc");
        assert_eq!(checked.account, acc.fingerprint());
    }

    #[test]
    fn certificate_bound_to_device_key() {
        let acc = Account::generate("bob").unwrap();
        let device = SignatureKeyPair::new(SCHEME).unwrap();
        let other = SignatureKeyPair::new(SCHEME).unwrap();
        let cert = acc.certify_device("pc", device.public()).unwrap();
        let err = DeviceCertificate::check(&cert.to_bytes(), other.public()).unwrap_err();
        assert!(matches!(err, CoreError::Forged(_)));
    }

    #[test]
    fn edited_certificate_is_rejected() {
        let acc = Account::generate("bob").unwrap();
        let device = SignatureKeyPair::new(SCHEME).unwrap();
        let mut cert = acc.certify_device("pc", device.public()).unwrap();
        cert.device = "tablet".into();
        let err = DeviceCertificate::check(&cert.to_bytes(), device.public()).unwrap_err();
        assert!(matches!(err, CoreError::Forged(_)));
    }

    #[test]
    fn fingerprint_grouping() {
        let fp = Fingerprint::of(b"key");
        assert_eq!(fp.as_str().len(), 32);
        assert_eq!(fp.grouped().split(' ').count(), 8);
    }
}
