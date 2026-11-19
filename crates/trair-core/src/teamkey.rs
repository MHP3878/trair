// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The Trair team key: signs team badges (decisions of 19.11.2026).
//!
//! It is made and kept on the project owner's own computer, in a file
//! encrypted with a long passphrase (Argon2id with 1 GiB, XChaCha20-
//! Poly1305, the same construction as the vault). A copy goes on a USB
//! stick, the passphrase on paper. Only the public key is ever put into
//! the app's code ([`crate::badges::TEAM_KEYS`]); the key never goes to a
//! server or into Git.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use openmls_basic_credential::SignatureKeyPair;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::account::{Account, Fingerprint};
use crate::badges::{Badge, BadgeKind};
use crate::vault::{
    HEADER_LEN, Header, KdfParams, Limits, NONCE_LEN, SALT_LEN, VaultError, VaultResult, derive,
    internal, random,
};

/// First bytes of a team key file.
pub const TEAM_MAGIC: &[u8; 8] = b"TRAIRT1\0";
/// Shortest passphrase, in characters. Five random words are better.
pub const MIN_PASSPHRASE_CHARS: usize = 16;
/// Argon2id for the team key: 1 GiB, 3 passes (a few seconds; the key is
/// used about once a year).
pub const TEAM_KDF: KdfParams = KdfParams {
    m_kib: 1024 * 1024,
    t: 3,
    p: 1,
};
/// Settings a team key file must have to be opened. Weaker files are
/// refused, so nobody can swap in a cheaper header.
pub const TEAM_LIMITS: Limits = Limits {
    min: KdfParams {
        m_kib: 512 * 1024,
        t: 3,
        p: 1,
    },
    max: Limits::PRODUCTION.max,
};
// The settings used for sealing must pass the limits used for opening,
// and the limits must stay well above a cheap header (checked at build).
const _: () = assert!(
    TEAM_LIMITS.min.m_kib <= TEAM_KDF.m_kib
        && TEAM_KDF.m_kib <= TEAM_LIMITS.max.m_kib
        && TEAM_LIMITS.min.t <= TEAM_KDF.t
        && TEAM_KDF.t <= TEAM_LIMITS.max.t
        && TEAM_LIMITS.min.m_kib >= 512 * 1024
);
const USER: &str = "trair-team";
const CONTENT_VERSION: u32 = 1;

#[derive(Serialize)]
struct ContentRef<'a> {
    v: u32,
    account: &'a SignatureKeyPair,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Content {
    v: u32,
    account: SignatureKeyPair,
}

/// An unlocked team key. Dropping it wipes the private key.
pub struct TeamKey {
    account: Account,
}

impl std::fmt::Debug for TeamKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TeamKey({})", self.public_key_hex())
    }
}

impl TeamKey {
    /// A new random team key.
    pub fn generate() -> VaultResult<TeamKey> {
        Ok(TeamKey {
            account: Account::generate(USER).map_err(internal)?,
        })
    }

    /// The public key, lowercase hex: this goes into `TEAM_KEYS`.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.account.public_key())
    }

    /// Fingerprint of the public key, to compare by eye.
    pub fn fingerprint(&self) -> Fingerprint {
        self.account.fingerprint()
    }

    /// Encrypt with `passphrase` and the settings [`TEAM_KDF`].
    pub fn seal(&self, passphrase: &str) -> VaultResult<Vec<u8>> {
        self.seal_with(passphrase, TEAM_KDF)
    }

    /// Like [`TeamKey::seal`] with other settings. For tests.
    pub fn seal_with(&self, passphrase: &str, params: KdfParams) -> VaultResult<Vec<u8>> {
        check_passphrase(passphrase)?;
        let salt = random::<SALT_LEN>()?;
        let nonce = random::<NONCE_LEN>()?;
        let key = derive(passphrase, &salt, params)?;
        let header = Header {
            params,
            salt,
            nonce,
        }
        .to_bytes(TEAM_MAGIC);
        let plain = Zeroizing::new(
            serde_json::to_vec(&ContentRef {
                v: CONTENT_VERSION,
                account: self.account.keys(),
            })
            .map_err(internal)?,
        );
        let sealed = XChaCha20Poly1305::new(key.as_slice().into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: &header,
                },
            )
            .map_err(internal)?;
        let mut file = header;
        file.extend_from_slice(&sealed);
        Ok(file)
    }

    /// Open a team key file.
    pub fn open(file: &[u8], passphrase: &str) -> VaultResult<TeamKey> {
        Self::open_with_limits(file, passphrase, TEAM_LIMITS)
    }

    /// Like [`TeamKey::open`] with other limits. For tests.
    pub fn open_with_limits(file: &[u8], passphrase: &str, limits: Limits) -> VaultResult<TeamKey> {
        let header = Header::parse(file, TEAM_MAGIC)?;
        limits.check(header.params)?;
        let key = derive(passphrase, &header.salt, header.params)?;
        let plain = Zeroizing::new(
            XChaCha20Poly1305::new(key.as_slice().into())
                .decrypt(
                    XNonce::from_slice(&header.nonce),
                    Payload {
                        msg: &file[HEADER_LEN..],
                        aad: &file[..HEADER_LEN],
                    },
                )
                .map_err(|_| VaultError::WrongPassword)?,
        );
        let content: Content = serde_json::from_slice(&plain)
            .map_err(|_| VaultError::Damaged("team key content".into()))?;
        if content.v != CONTENT_VERSION {
            return Err(VaultError::Damaged(format!(
                "unknown team key version {}",
                content.v
            )));
        }
        let account = Account::from_keys(USER, content.account)
            .map_err(|_| VaultError::Damaged("team key does not fit together".into()))?;
        Ok(TeamKey { account })
    }

    /// Sign a team badge for the account key `holder`, valid from `now`
    /// (unix seconds) for `days` days (at most 366).
    pub fn sign_badge(&self, holder: &[u8], now: u64, days: u64) -> VaultResult<Badge> {
        if holder.len() != 32 {
            return Err(VaultError::Refused("account key must be 32 bytes".into()));
        }
        Badge::issue(
            &self.account,
            BadgeKind::Team,
            holder,
            None,
            now,
            days.saturating_mul(24 * 3600),
            None,
        )
        .map_err(|e| VaultError::Refused(e.to_string()))
    }
}

/// At least [`MIN_PASSPHRASE_CHARS`] characters and not one character
/// repeated.
pub fn check_passphrase(passphrase: &str) -> VaultResult<()> {
    let mut chars = passphrase.chars();
    let first = chars.next();
    let varied = chars.any(|c| Some(c) != first);
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS || !varied {
        return Err(VaultError::WeakPassword);
    }
    Ok(())
}
