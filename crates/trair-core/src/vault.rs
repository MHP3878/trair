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
use std::collections::BTreeMap;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::account::Account;
use crate::badges::{self, Badge, VerifiedBadge};
use crate::devices::{DeviceIdentity, SignedDeviceList, validate_label};

/// First bytes of every vault file.
pub const MAGIC: &[u8; 8] = b"TRAIRV1\0";
/// Shortest allowed launcher password, in characters.
pub const MIN_PASSWORD_CHARS: usize = 12;
/// Largest profile the vault stores, in bytes of JSON. Room for a small
/// profile picture (128 × 128 pixels, base64).
pub const MAX_PROFILE_BYTES: usize = 256 * 1024;

const KDF_ARGON2ID: u8 = 1;
pub(crate) const SALT_LEN: usize = 16;
pub(crate) const NONCE_LEN: usize = 24;
pub(crate) const HEADER_LEN: usize = 8 + 1 + 12 + SALT_LEN + NONCE_LEN;
const TAG_LEN: usize = 16;
/// Content version. 1 = Paket 1 (account only), 2 = with this device's
/// key and the signed device list (M2). Version 1 is upgraded on opening.
const PAYLOAD_VERSION: u32 = 2;
/// Name this device gets until the person renames it.
pub const DEFAULT_LABEL: &str = "Dieser PC";

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

    pub(crate) fn check(&self, k: KdfParams) -> VaultResult<()> {
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
/// the key pairs wipe their private halves when dropped.
#[derive(Serialize)]
struct ContentRef<'a> {
    v: u32,
    user: &'a str,
    account: &'a SignatureKeyPair,
    device_id: &'a str,
    device: &'a SignatureKeyPair,
    list: &'a SignedDeviceList,
    labels: &'a BTreeMap<String, String>,
    profile: &'a Value,
    badges: &'a [Badge],
    chats_key: &'a str,
}

#[derive(Deserialize)]
struct Content {
    v: u32,
    user: String,
    account: SignatureKeyPair,
    // Missing in version 1.
    device_id: Option<String>,
    device: Option<SignatureKeyPair>,
    list: Option<SignedDeviceList>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    profile: Value,
    // Missing before own badges were stored (19.11.2026).
    #[serde(default)]
    badges: Vec<Badge>,
    // Missing before chats were stored on disk (M4). Hex, 32 bytes.
    #[serde(default)]
    chats_key: Option<String>,
}

/// What a new device receives when it is paired: everything to build its
/// own vault (see [`crate::pairing`]).
pub struct JoinedAccount {
    /// The account, with its private key.
    pub account: Account,
    /// The new device's own identity.
    pub device: DeviceIdentity,
    /// The device list that includes the new device.
    pub list: SignedDeviceList,
    /// The profile of the account (name, colours, settings).
    pub profile: Value,
}

/// An unlocked vault. Dropping it wipes the key from memory.
pub struct Vault {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key: Zeroizing<[u8; 32]>,
    account: Account,
    device: DeviceIdentity,
    list: SignedDeviceList,
    labels: BTreeMap<String, String>,
    profile: Value,
    badges: Vec<Badge>,
    chats_key: Zeroizing<[u8; 32]>,
    migrated: bool,
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
        let device = DeviceIdentity::generate().map_err(internal)?;
        let list = SignedDeviceList::first(&account, device.entry()).map_err(internal)?;
        let joined = JoinedAccount {
            account,
            device,
            list,
            profile,
        };
        Self::create_joined_with(password, joined, params)
    }

    /// Build the vault of a newly paired device, using
    /// [`KdfParams::DEFAULT`].
    pub fn create_joined(password: &str, joined: JoinedAccount) -> VaultResult<(Vault, Vec<u8>)> {
        Self::create_joined_with(password, joined, KdfParams::DEFAULT)
    }

    /// Like [`Vault::create_joined`] with other Argon2id settings. For tests.
    pub fn create_joined_with(
        password: &str,
        joined: JoinedAccount,
        params: KdfParams,
    ) -> VaultResult<(Vault, Vec<u8>)> {
        check_password(password)?;
        check_profile(&joined.profile)?;
        check_own_device(&joined.account, &joined.device, &joined.list)?;
        let salt = random::<SALT_LEN>()?;
        let key = derive(password, &salt, params)?;
        let mut labels = BTreeMap::new();
        labels.insert(joined.device.id.clone(), DEFAULT_LABEL.to_string());
        let vault = Vault {
            params,
            salt,
            key,
            account: joined.account,
            device: joined.device,
            list: joined.list,
            labels,
            profile: joined.profile,
            badges: Vec::new(),
            chats_key: Zeroizing::new(random::<32>()?),
            migrated: false,
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
        let header = Header::parse(file, MAGIC)?;
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
        let account = Account::from_keys(&content.user, content.account)
            .map_err(|_| VaultError::Damaged("account keys do not belong together".into()))?;
        let mut labels = content.labels;
        let (device, list, migrated) =
            match (content.v, content.device_id, content.device, content.list) {
                // Paket 1: the account had no device key yet. Give it one now;
                // the caller stores the upgraded vault (see `migrated`).
                (1, None, None, None) => {
                    let device = DeviceIdentity::generate().map_err(internal)?;
                    let list =
                        SignedDeviceList::first(&account, device.entry()).map_err(internal)?;
                    labels.insert(device.id.clone(), DEFAULT_LABEL.to_string());
                    (device, list, true)
                }
                (PAYLOAD_VERSION, Some(id), Some(keys), Some(list)) => {
                    (DeviceIdentity { id, keys }, list, false)
                }
                (v, ..) => {
                    return Err(VaultError::Damaged(format!("unknown content version {v}")));
                }
            };
        check_own_device(&account, &device, &list)?;
        // Each device has its own random key for its chat database. Older
        // vaults get one now and are stored again (see `migrated`).
        let (chats_key, new_chats_key) = match content.chats_key {
            Some(hex_key) => {
                let hex_key = Zeroizing::new(hex_key);
                let mut k = Zeroizing::new([0u8; 32]);
                hex::decode_to_slice(hex_key.as_bytes(), k.as_mut_slice())
                    .map_err(|_| VaultError::Damaged("chat key is not readable".into()))?;
                (k, false)
            }
            None => (Zeroizing::new(random::<32>()?), true),
        };
        Ok(Vault {
            params: header.params,
            salt: header.salt,
            key,
            account,
            device,
            list,
            labels,
            profile: content.profile,
            badges: content.badges,
            chats_key,
            migrated: migrated || new_chats_key,
        })
    }

    /// Encrypt the current state again, with a fresh nonce.
    pub fn seal(&self) -> VaultResult<Vec<u8>> {
        let chats_key = Zeroizing::new(hex::encode(self.chats_key.as_slice()));
        let content = ContentRef {
            v: PAYLOAD_VERSION,
            user: self.account.user(),
            account: self.account.keys(),
            device_id: &self.device.id,
            device: &self.device.keys,
            list: &self.list,
            labels: &self.labels,
            profile: &self.profile,
            badges: &self.badges,
            chats_key: &chats_key,
        };
        let plain = Zeroizing::new(serde_json::to_vec(&content).map_err(internal)?);
        let nonce = random::<NONCE_LEN>()?;
        let header = Header {
            params: self.params,
            salt: self.salt,
            nonce,
        }
        .to_bytes(MAGIC);
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

    /// Key of this device's chat database (SQLCipher). Random, never
    /// leaves the vault, not tied to the password.
    pub fn chats_key(&self) -> &[u8; 32] {
        &self.chats_key
    }

    /// The account stored in this vault.
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// The stored profile (display name, colours, settings).
    pub fn profile(&self) -> &Value {
        &self.profile
    }

    /// This device: its id and signing key.
    pub fn device(&self) -> &DeviceIdentity {
        &self.device
    }

    /// The account's current, signed device list.
    pub fn device_list(&self) -> &SignedDeviceList {
        &self.list
    }

    /// Names people gave to devices, by device id. Only stored here.
    pub fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }

    /// True if this vault came from Paket 1 and was upgraded while
    /// opening. Store it with [`Vault::seal`] right away.
    pub fn migrated(&self) -> bool {
        self.migrated
    }

    /// Give a device a name. Call [`Vault::seal`] afterwards.
    pub fn set_label(&mut self, device_id: &str, label: &str) -> VaultResult<()> {
        validate_label(label).map_err(|_| VaultError::Refused("device name".into()))?;
        if !self.list.list.contains(device_id) {
            return Err(VaultError::Refused("unknown device".into()));
        }
        self.labels.insert(device_id.to_string(), label.to_string());
        Ok(())
    }

    /// Replace the device list with a newer one signed by this account
    /// (for example after pairing). This device must still be on it.
    pub fn set_device_list(&mut self, list: SignedDeviceList) -> VaultResult<()> {
        list.verify(self.account.public_key(), Some(self.list.list.seq))
            .map_err(|e| VaultError::Refused(format!("{e}")))?;
        check_own_device(&self.account, &self.device, &list)?;
        self.list = list;
        Ok(())
    }

    /// Withdraw another device of this account. It is refused everywhere
    /// from now on. This device cannot withdraw itself. Call
    /// [`Vault::seal`] afterwards.
    pub fn withdraw_device(&mut self, device_id: &str) -> VaultResult<()> {
        if device_id == self.device.id {
            return Err(VaultError::Refused(
                "this device cannot withdraw itself".into(),
            ));
        }
        let list = self
            .list
            .without_device(&self.account, device_id)
            .map_err(|e| VaultError::Refused(format!("{e}")))?;
        self.labels.remove(device_id);
        self.list = list;
        Ok(())
    }

    /// The own badges, signed, as stored. See [`Vault::verified_badges`].
    pub fn badges(&self) -> &[Badge] {
        &self.badges
    }

    /// The own badges that are valid at unix time `now`.
    pub fn verified_badges(&self, now: u64, team_keys: &[&str]) -> Vec<VerifiedBadge> {
        badges::verify_all(&self.badges, self.account.public_key(), now, team_keys).0
    }

    /// Keep a badge for this account (for example a team badge from the
    /// team tool). It must be valid now; it replaces an older badge of
    /// the same kind and community. Call [`Vault::seal`] afterwards.
    pub fn add_badge(
        &mut self,
        badge: Badge,
        now: u64,
        team_keys: &[&str],
    ) -> VaultResult<VerifiedBadge> {
        let verified = badges::verify(&badge, self.account.public_key(), now, team_keys)
            .map_err(|e| VaultError::Refused(e.to_string()))?;
        let same = |b: &Badge| {
            b.claim.kind == badge.claim.kind
                && b.claim.community.as_ref().map(|c| &c.id)
                    == badge.claim.community.as_ref().map(|c| &c.id)
        };
        // Expired badges are dropped on the way.
        self.badges.retain(|b| {
            !same(b) && badges::verify(b, self.account.public_key(), now, team_keys).is_ok()
        });
        if self.badges.len() >= badges::MAX_BADGES {
            return Err(VaultError::Refused("too many badges".into()));
        }
        self.badges.push(badge);
        self.badges.sort_by_key(|b| {
            (
                b.claim.kind,
                b.claim.community.as_ref().map(|c| c.id.clone()),
            )
        });
        Ok(verified)
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

/// The clear header in front of every sealed file: magic, Argon2id
/// settings, salt and nonce. It is authenticated as associated data.
pub(crate) struct Header {
    pub(crate) params: KdfParams,
    pub(crate) salt: [u8; SALT_LEN],
    pub(crate) nonce: [u8; NONCE_LEN],
}

impl Header {
    pub(crate) fn parse(file: &[u8], magic: &[u8; 8]) -> VaultResult<Header> {
        if file.len() < HEADER_LEN + TAG_LEN {
            return Err(VaultError::Damaged("file is too short".into()));
        }
        if &file[..8] != magic {
            return Err(VaultError::Damaged("not a Trair file of this kind".into()));
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

    pub(crate) fn to_bytes(&self, magic: &[u8; 8]) -> Vec<u8> {
        let mut h = Vec::with_capacity(HEADER_LEN);
        h.extend_from_slice(magic);
        h.push(KDF_ARGON2ID);
        h.extend_from_slice(&self.params.m_kib.to_le_bytes());
        h.extend_from_slice(&self.params.t.to_le_bytes());
        h.extend_from_slice(&self.params.p.to_le_bytes());
        h.extend_from_slice(&self.salt);
        h.extend_from_slice(&self.nonce);
        h
    }
}

/// The list must be signed by the account and contain this device with
/// exactly its key.
fn check_own_device(
    account: &Account,
    device: &DeviceIdentity,
    list: &SignedDeviceList,
) -> VaultResult<()> {
    let verified = list
        .verify(account.public_key(), None)
        .map_err(|_| VaultError::Damaged("device list".into()))?;
    if verified.user != account.user() || !verified.devices.contains(&device.entry()) {
        return Err(VaultError::Damaged(
            "this device is not on the device list".into(),
        ));
    }
    Ok(())
}

/// A password must have at least the minimum length (for the app: check
/// before a step that cannot be repeated, such as finishing a pairing).
pub fn check_password(password: &str) -> VaultResult<()> {
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

pub(crate) fn derive(
    password: &str,
    salt: &[u8],
    k: KdfParams,
) -> VaultResult<Zeroizing<[u8; 32]>> {
    let params = Params::new(k.m_kib, k.t, k.p, Some(32))
        .map_err(|e| VaultError::Refused(format!("argon2id: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut_slice())
        .map_err(internal)?;
    Ok(key)
}

pub(crate) fn random<const N: usize>() -> VaultResult<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(internal)?;
    Ok(b)
}

pub(crate) fn internal<E: std::fmt::Debug>(e: E) -> VaultError {
    VaultError::Internal(format!("{e:?}"))
}
