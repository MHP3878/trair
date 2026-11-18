// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The vault ("Tresor"): the account key and the profile, encrypted with
//! the launcher password and stored on this computer.
//!
//! Format, version 1 (all integers little endian):
//!
//! | bytes | content                                            |
//! |-------|----------------------------------------------------|
//! | 8     | magic `TRAIRV1\0`                                  |
//! | 1     | key derivation, 1 = Argon2id version 0x13          |
//! | 12    | Argon2id memory (KiB), passes, lanes as 3 × u32    |
//! | 16    | random salt                                        |
//! | 24    | random nonce for XChaCha20-Poly1305                |
//! | rest  | encrypted payload plus 16 byte tag                 |
//!
//! The 61 header bytes are the associated data of the encryption, so a
//! change to any of them makes opening fail. The Argon2id settings are
//! checked against [`Limits`] before any work is done: a file cannot make
//! the app use absurd amounts of memory, and it cannot lower the cost of
//! guessing the password.
//!
//! What this protects against: someone who copies the file (stolen disk,
//! backup, other user on the PC) has to guess the password, and every
//! guess costs about one second of a full CPU core and 256 MiB of memory.
//! What it does not protect against: malware on the running PC, a key
//! logger, or someone who uses the PC while Trair is unlocked.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use openmls_basic_credential::SignatureKeyPair;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::account::Account;

/// First bytes of every vault file.
pub const MAGIC: &[u8; 8] = b"TRAIRV1\0";
/// Shortest allowed launcher password, in characters.
pub const MIN_PASSWORD_CHARS: usize = 12;
/// Largest profile the vault stores, in bytes of JSON.
pub const MAX_PROFILE_BYTES: usize = 64 * 1024;

const KDF_ARGON2ID: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = 8 + 1 + 12 + SALT_LEN + NONCE_LEN;
const TAG_LEN: usize = 16;
const PAYLOAD_VERSION: u32 = 1;

/// Errors of the vault. The app shows one text per kind; none of them
/// contains secret data.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VaultError {
    /// The password is shorter than [`MIN_PASSWORD_CHARS`].
    #[error("password too short")]
    WeakPassword,
    /// Wrong password, or the encrypted part was changed. Both look the
    /// same on purpose: the encryption cannot tell them apart.
    #[error("wrong password or damaged vault")]
    WrongPassword,
    /// The file is not a vault, is cut off or its header is broken.
    #[error("damaged vault: {0}")]
    Damaged(String),
    /// The key derivation settings are outside [`Limits`].
    #[error("vault settings refused: {0}")]
    Refused(String),
    /// The profile is larger than [`MAX_PROFILE_BYTES`].
    #[error("profile too large")]
    TooLarge,
    /// Error from a crypto library or the random number generator.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Short form for vault results.
pub type VaultResult<T> = std::result::Result<T, VaultError>;

/// Argon2id settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Number of passes.
    pub t: u32,
    /// Lanes (parallelism).
    pub p: u32,
}

impl KdfParams {
    /// About one second on a current PC (decision of 18.11.2026):
    /// 256 MiB, 4 passes, 1 lane. Measured 0.9 s on a CI-class CPU.
    pub const DEFAULT: KdfParams = KdfParams {
        m_kib: 256 * 1024,
        t: 4,
        p: 1,
    };
}

/// Allowed range for the Argon2id settings of a file that is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Weakest settings accepted.
    pub min: KdfParams,
    /// Most expensive settings accepted.
    pub max: KdfParams,
}

impl Limits {
    /// The limits the app uses.
    pub const PRODUCTION: Limits = Limits {
        min: KdfParams {
            m_kib: 64 * 1024,
            t: 2,
            p: 1,
        },
        max: KdfParams {
            m_kib: 1024 * 1024,
            t: 10,
            p: 4,
        },
    };

    fn check(&self, k: KdfParams) -> VaultResult<()> {
        let ok = (self.min.m_kib..=self.max.m_kib).contains(&k.m_kib)
            && (self.min.t..=self.max.t).contains(&k.t)
            && (self.min.p..=self.max.p).contains(&k.p);
        if ok {
            Ok(())
        } else {
            Err(VaultError::Refused(format!(
                "argon2id m={} KiB t={} p={}",
                k.m_kib, k.t, k.p
            )))
        }
    }
}

/// The decrypted content. Only lives in memory while Trair is unlocked;
/// the key pair wipes its private half when dropped.
#[derive(Serialize)]
struct ContentRef<'a> {
    v: u32,
    user: &'a str,
    account: &'a SignatureKeyPair,
    profile: &'a Value,
}

#[derive(Deserialize)]
struct Content {
    v: u32,
    user: String,
    account: SignatureKeyPair,
    profile: Value,
}

/// An unlocked vault. Dropping it wipes the key from memory.
pub struct Vault {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key: Zeroizing<[u8; 32]>,
    account: Account,
    profile: Value,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.debug_struct("Vault")
            .field("user", &self.account.user())
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Create a new account and seal it with `password`, using
    /// [`KdfParams::DEFAULT`]. Returns the unlocked vault and the file.
    pub fn create(password: &str, profile: Value) -> VaultResult<(Vault, Vec<u8>)> {
        Self::create_with(password, profile, KdfParams::DEFAULT)
    }

    /// Like [`Vault::create`] with other Argon2id settings. For tests.
    pub fn create_with(
        password: &str,
        profile: Value,
        params: KdfParams,
    ) -> VaultResult<(Vault, Vec<u8>)> {
        check_password(password)?;
        check_profile(&profile)?;
        let user = format!("local-{}", hex::encode(random::<6>()?));
        let account = Account::generate(&user).map_err(internal)?;
        let salt = random::<SALT_LEN>()?;
        let key = derive(password, &salt, params)?;
        let vault = Vault {
            params,
            salt,
            key,
            account,
            profile,
        };
        let file = vault.seal()?;
        Ok((vault, file))
    }

    /// Open a vault file with [`Limits::PRODUCTION`].
    pub fn open(file: &[u8], password: &str) -> VaultResult<Vault> {
        Self::open_with_limits(file, password, Limits::PRODUCTION)
    }

    /// Like [`Vault::open`] with other limits. For tests.
    pub fn open_with_limits(file: &[u8], password: &str, limits: Limits) -> VaultResult<Vault> {
        let header = Header::parse(file)?;
        limits.check(header.params)?;
        let key = derive(password, &header.salt, header.params)?;
        let cipher = XChaCha20Poly1305::new(key.as_slice().into());
        let plain = Zeroizing::new(
            cipher
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
            .map_err(|_| VaultError::Damaged("content is not readable".into()))?;
        if content.v != PAYLOAD_VERSION {
            return Err(VaultError::Damaged(format!(
                "unknown content version {}",
                content.v
            )));
        }
        let account = Account::from_keys(&content.user, content.account)
            .map_err(|_| VaultError::Damaged("account keys do not belong together".into()))?;
        Ok(Vault {
            params: header.params,
            salt: header.salt,
            key,
            account,
            profile: content.profile,
        })
    }

    /// Encrypt the current state again, with a fresh nonce.
    pub fn seal(&self) -> VaultResult<Vec<u8>> {
        let content = ContentRef {
            v: PAYLOAD_VERSION,
            user: self.account.user(),
            account: self.account.keys(),
            profile: &self.profile,
        };
        let plain = Zeroizing::new(serde_json::to_vec(&content).map_err(internal)?);
        let nonce = random::<NONCE_LEN>()?;
        let header = Header {
            params: self.params,
            salt: self.salt,
            nonce,
        }
        .to_bytes();
        let cipher = XChaCha20Poly1305::new(self.key.as_slice().into());
        let sealed = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: &header,
                },
            )
            .map_err(internal)?;
        let mut file = Vec::with_capacity(HEADER_LEN + sealed.len());
        file.extend_from_slice(&header);
        file.extend_from_slice(&sealed);
        Ok(file)
    }

    /// The account stored in this vault.
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// The stored profile (display name, colours, settings).
    pub fn profile(&self) -> &Value {
        &self.profile
    }

    /// Replace the profile. Call [`Vault::seal`] afterwards to store it.
    pub fn set_profile(&mut self, profile: Value) -> VaultResult<()> {
        check_profile(&profile)?;
        self.profile = profile;
        Ok(())
    }

    /// Use a new password from now on: new salt, new key, current default
    /// settings. Call [`Vault::seal`] afterwards to store it.
    pub fn change_password(&mut self, new_password: &str) -> VaultResult<()> {
        self.change_password_with(new_password, KdfParams::DEFAULT)
    }

    /// Like [`Vault::change_password`] with other settings. For tests.
    pub fn change_password_with(
        &mut self,
        new_password: &str,
        params: KdfParams,
    ) -> VaultResult<()> {
        check_password(new_password)?;
        let salt = random::<SALT_LEN>()?;
        self.key = derive(new_password, &salt, params)?;
        self.salt = salt;
        self.params = params;
        Ok(())
    }
}

struct Header {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
}

impl Header {
    fn parse(file: &[u8]) -> VaultResult<Header> {
        if file.len() < HEADER_LEN + TAG_LEN {
            return Err(VaultError::Damaged("file is too short".into()));
        }
        if &file[..8] != MAGIC {
            return Err(VaultError::Damaged("not a Trair vault".into()));
        }
        if file[8] != KDF_ARGON2ID {
            return Err(VaultError::Damaged(format!(
                "unknown key derivation {}",
                file[8]
            )));
        }
        let u32_at =
            |i: usize| u32::from_le_bytes([file[i], file[i + 1], file[i + 2], file[i + 3]]);
        let params = KdfParams {
            m_kib: u32_at(9),
            t: u32_at(13),
            p: u32_at(17),
        };
        let broken = |_| VaultError::Damaged("header".into());
        let salt: [u8; SALT_LEN] = file[21..21 + SALT_LEN].try_into().map_err(broken)?;
        let nonce: [u8; NONCE_LEN] = file[21 + SALT_LEN..HEADER_LEN].try_into().map_err(broken)?;
        Ok(Header {
            params,
            salt,
            nonce,
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut h = Vec::with_capacity(HEADER_LEN);
        h.extend_from_slice(MAGIC);
        h.push(KDF_ARGON2ID);
        h.extend_from_slice(&self.params.m_kib.to_le_bytes());
        h.extend_from_slice(&self.params.t.to_le_bytes());
        h.extend_from_slice(&self.params.p.to_le_bytes());
        h.extend_from_slice(&self.salt);
        h.extend_from_slice(&self.nonce);
        h
    }
}

fn check_password(password: &str) -> VaultResult<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(VaultError::WeakPassword);
    }
    Ok(())
}

fn check_profile(profile: &Value) -> VaultResult<()> {
    let size = serde_json::to_vec(profile).map_err(internal)?.len();
    if size > MAX_PROFILE_BYTES {
        return Err(VaultError::TooLarge);
    }
    Ok(())
}

fn derive(password: &str, salt: &[u8], k: KdfParams) -> VaultResult<Zeroizing<[u8; 32]>> {
    let params = Params::new(k.m_kib, k.t, k.p, Some(32))
        .map_err(|e| VaultError::Refused(format!("argon2id: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut_slice())
        .map_err(internal)?;
    Ok(key)
}

fn random<const N: usize>() -> VaultResult<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(internal)?;
    Ok(b)
}

fn internal<E: std::fmt::Debug>(e: E) -> VaultError {
    VaultError::Internal(format!("{e:?}"))
}
