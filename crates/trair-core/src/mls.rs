// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! One Trair device: its MLS groups, its key packages and the strict policy.
//!
//! The device never trusts what the server says about who sent what or
//! who is in a group. Senders come from MLS signatures, members come
//! from the MLS tree, and every member's device certificate is checked
//! against the account that signed it.

use std::collections::HashMap;

use openmls::prelude::tls_codec::{Deserialize as _, Serialize as _};
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;
use serde::{Deserialize, Serialize};

use crate::account::{Account, DeviceCertificate, Fingerprint, SCHEME};
use crate::devices::DeviceIdentity;
use crate::error::{CoreError, Result, internal};
use crate::groups::{self, Accounts, Acct, GroupState, TRAIR_EXTENSION};
use crate::policy::{Policy, PolicyState, Trust};

/// The only cipher suite Trair uses: X25519, AES-128-GCM, SHA-256, Ed25519.
pub const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

/// What every Trair device announces in its leaves: it understands the
/// Trair group state (and, for the reserve package, last resort).
fn leaf_capabilities(last_resort: bool) -> Capabilities {
    let mut extensions = vec![ExtensionType::Unknown(TRAIR_EXTENSION)];
    if last_resort {
        extensions.push(ExtensionType::LastResort);
    }
    Capabilities::builder().extensions(extensions).build()
}

/// The group context extensions that carry a Trair group state. Every
/// member must understand it.
fn state_extensions(state: &GroupState) -> Result<Extensions<GroupContext>> {
    Extensions::from_vec(vec![
        Extension::RequiredCapabilities(RequiredCapabilitiesExtension::new(
            &[ExtensionType::Unknown(TRAIR_EXTENSION)],
            &[],
            &[],
        )),
        Extension::Unknown(TRAIR_EXTENSION, UnknownExtension(state.to_bytes())),
    ])
    .map_err(internal)
}

fn read_state(extensions: &Extensions<GroupContext>) -> Result<Option<GroupState>> {
    extensions
        .unknown(TRAIR_EXTENSION)
        .map(|u| GroupState::parse(&u.0))
        .transpose()
}

/// The account of a leaf, from its checked certificate.
fn leaf_acct(identity: &[u8], signature_key: &[u8]) -> Option<Acct> {
    DeviceCertificate::check(identity, signature_key)
        .ok()
        .map(|c| Acct {
            user: c.user,
            fp: c.account.as_str().to_string(),
        })
}

/// Collect accounts; one user under two keys is refused.
fn collect_accounts<'a>(accts: impl IntoIterator<Item = &'a Acct>) -> Result<Accounts> {
    let mut out = Accounts::new();
    for a in accts {
        if let Some(fp) = out.insert(a.user.clone(), a.fp.clone())
            && fp != a.fp
        {
            return Err(CoreError::Blocked(format!(
                "{} is in the group under two keys",
                a.user
            )));
        }
    }
    Ok(out)
}

/// A key package as handed out by the server, together with the device
/// the server claims it belongs to. The claim is checked, never trusted.
#[derive(Debug, Clone)]
pub struct KeyPackageOffer {
    /// What the server says: `user:device`.
    pub claimed_device: String,
    /// The serialized MLS key package.
    pub bytes: Vec<u8>,
}

/// A group member as this device sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// `user:device`, taken from the checked certificate.
    pub id: String,
    /// The user part.
    pub user: String,
    /// How far this device trusts the member.
    pub trust: Trust,
}

/// A commit this device created. Send it to the server; when the server
/// accepts it call [`Device::confirm`], otherwise [`Device::discard`].
#[derive(Debug, Clone)]
pub struct Outgoing {
    /// Group id.
    pub group: String,
    /// Epoch the commit was built on (servers reject a second commit for
    /// the same epoch).
    pub epoch: u64,
    /// The commit for all current members.
    pub commit: Vec<u8>,
    /// The welcome for new members, if any were added.
    pub welcome: Option<Vec<u8>>,
    /// Members that will be added.
    pub added: Vec<Member>,
    /// Device ids that will be removed.
    pub removed: Vec<String>,
}

/// What happened when an incoming message was processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// This device joined a group.
    Joined {
        /// Group id.
        group: String,
        /// Everyone in the group, with trust levels.
        members: Vec<Member>,
        /// The device that added this one (its signature is checked).
        by: Member,
        /// The welcome used the last-resort key package. Call
        /// [`Device::self_update`] for this group right away.
        via_last_resort: bool,
        /// The group's state (a Trair group or join chat), already checked
        /// against the members and the adder's role.
        state: Option<GroupState>,
    },
    /// A chat message.
    Message {
        /// Group id.
        group: String,
        /// Sender, proven by the MLS signature.
        from: String,
        /// The text.
        text: String,
        /// How many earlier messages from this sender never arrived.
        missing: u64,
    },
    /// A profile from another member (name, picture, badges), not yet
    /// checked. `account_key` is the sender's, from its certificate.
    Profile {
        /// Group id.
        group: String,
        /// Sender, proven by the MLS signature.
        from: String,
        /// The sender's account key (all its devices in the group agree).
        account_key: Vec<u8>,
        /// The profile as sent.
        profile: serde_json::Value,
    },
    /// A chat setting from a member (not yet checked).
    Control {
        /// Group id.
        group: String,
        /// Sender, proven by the MLS signature.
        from: String,
        /// The setting as sent.
        control: serde_json::Value,
    },
    /// The group changed.
    Commit {
        /// Group id.
        group: String,
        /// Who made the change (proven by signature).
        by: String,
        /// New members.
        added: Vec<Member>,
        /// Removed device ids.
        removed: Vec<String>,
        /// This device is no longer a member.
        i_was_removed: bool,
        /// The group state changed (name, admins, guests, end).
        state_changed: bool,
    },
    /// Something else that needs no action (for example a proposal).
    Other {
        /// Group id.
        group: String,
    },
}

/// What is inside an MLS application message. A profile has `seq` 0: it
/// is not counted, so one that never left this device leaves no gap.
#[derive(Serialize, Deserialize)]
struct Payload {
    seq: u64,
    #[serde(default)]
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<serde_json::Value>,
    /// Chat settings between the devices (a friendship key's token, when
    /// the chat vanishes). Not counted either.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    control: Option<serde_json::Value>,
}

/// What a device keeps besides the OpenMLS key-value store: which groups
/// it has, its message counters, its last-resort package and its policy.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct DeviceState {
    groups: Vec<String>,
    seq_out: Vec<(String, u64)>,
    seq_in: Vec<(String, String, u64)>,
    last_resort: Option<Vec<u8>>,
    policy: PolicyState,
}

/// One device of an account.
pub struct Device {
    id: String,
    user: String,
    account: Fingerprint,
    provider: OpenMlsRustCrypto,
    keys: SignatureKeyPair,
    credential: CredentialWithKey,
    groups: HashMap<String, MlsGroup>,
    policy: Policy,
    seq_out: HashMap<String, u64>,
    seq_in: HashMap<(String, String), u64>,
    last_resort: Option<KeyPackageRef>,
}

impl Device {
    /// Create a device with a fresh device key, certified by the account.
    pub fn new(account: &Account, device: &str) -> Result<Self> {
        let keys = SignatureKeyPair::new(SCHEME).map_err(internal)?;
        Self::with_keys(account, device, keys)
    }

    /// This device as stored in the vault: same id, same key.
    pub fn from_identity(account: &Account, identity: &DeviceIdentity) -> Result<Self> {
        Self::with_keys(account, &identity.id, identity.keys.clone())
    }

    fn with_keys(account: &Account, device: &str, keys: SignatureKeyPair) -> Result<Self> {
        let provider = OpenMlsRustCrypto::default();
        keys.store(provider.storage()).map_err(internal)?;
        let cert = account.certify_device(device, keys.public())?;
        let credential = CredentialWithKey {
            credential: BasicCredential::new(cert.to_bytes()).into(),
            signature_key: keys.to_public_vec().into(),
        };
        Ok(Device {
            // Public device id; the same value the certificate carries.
            id: format!("{}:{}", account.user(), device),
            user: account.user().to_string(),
            account: account.fingerprint(),
            provider,
            keys,
            credential,
            groups: HashMap::new(),
            policy: Policy::default(),
            seq_out: HashMap::new(),
            seq_in: HashMap::new(),
            last_resort: None,
        })
    }

    /// A device as it was saved: the OpenMLS key-value store plus
    /// [`DeviceState`]. Every group listed there must load.
    pub(crate) fn restore(
        account: &Account,
        identity: &DeviceIdentity,
        values: HashMap<Vec<u8>, Vec<u8>>,
        state: DeviceState,
    ) -> Result<Self> {
        let mut dev = Self::from_identity(account, identity)?;
        {
            let mut map = dev
                .provider
                .storage()
                .values
                .write()
                .map_err(|_| CoreError::Internal("storage lock".into()))?;
            for (k, v) in values {
                map.insert(k, v);
            }
        }
        for group in state.groups {
            let g = MlsGroup::load(
                dev.provider.storage(),
                &GroupId::from_slice(group.as_bytes()),
            )
            .map_err(internal)?
            .ok_or_else(|| CoreError::Storage(format!("group {group} is missing")))?;
            dev.groups.insert(group, g);
        }
        dev.seq_out = state.seq_out.into_iter().collect();
        dev.seq_in = state
            .seq_in
            .into_iter()
            .map(|(g, s, n)| ((g, s), n))
            .collect();
        dev.last_resort = state
            .last_resort
            .map(|b| KeyPackageRef::tls_deserialize_exact(b.as_slice()))
            .transpose()
            .map_err(|_| CoreError::Storage("last-resort reference is not readable".into()))?;
        dev.policy = Policy::import(state.policy)?;
        Ok(dev)
    }

    /// The state to save next to the key-value store.
    pub(crate) fn export_state(&self) -> DeviceState {
        let mut groups: Vec<String> = self.groups.keys().cloned().collect();
        groups.sort();
        let mut seq_out: Vec<(String, u64)> =
            self.seq_out.iter().map(|(g, n)| (g.clone(), *n)).collect();
        seq_out.sort();
        let mut seq_in: Vec<(String, String, u64)> = self
            .seq_in
            .iter()
            .map(|((g, s), n)| (g.clone(), s.clone(), *n))
            .collect();
        seq_in.sort();
        DeviceState {
            groups,
            seq_out,
            seq_in,
            last_resort: self
                .last_resort
                .as_ref()
                .and_then(|r| r.tls_serialize_detached().ok()),
            policy: self.policy.export(),
        }
    }

    /// Visit the OpenMLS key-value store (for saving it).
    pub(crate) fn with_values<T>(
        &self,
        f: impl FnOnce(&HashMap<Vec<u8>, Vec<u8>>) -> T,
    ) -> Result<T> {
        let map = self
            .provider
            .storage()
            .values
            .read()
            .map_err(|_| CoreError::Internal("storage lock".into()))?;
        Ok(f(&map))
    }

    /// Drop a group from this device: its keys are deleted, later
    /// messages for it are refused.
    pub fn forget_group(&mut self, group: &str) -> Result<()> {
        let mut g = self
            .groups
            .remove(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        g.delete(self.provider.storage()).map_err(internal)?;
        self.seq_out.remove(group);
        self.seq_in.retain(|(g, _), _| g != group);
        Ok(())
    }

    /// The account fingerprint of a user in a group, from that user's
    /// checked device certificates. None if the user is not in the group
    /// or the certificates disagree.
    pub fn account_of(&self, group: &str, user: &str) -> Result<Option<Fingerprint>> {
        let g = self.group(group)?;
        let mut found: Option<Fingerprint> = None;
        for m in g.members() {
            let Ok(c) =
                DeviceCertificate::check(m.credential.serialized_content(), &m.signature_key)
            else {
                continue;
            };
            if c.user != user {
                continue;
            }
            match &found {
                Some(f) if *f != c.account => return Ok(None),
                _ => found = Some(c.account),
            }
        }
        Ok(found)
    }

    /// The account key of `user` in a group, if all its devices there
    /// name the same one.
    pub fn account_key_of(&self, group: &str, user: &str) -> Result<Option<Vec<u8>>> {
        let g = self.group(group)?;
        let mut found: Option<Vec<u8>> = None;
        for m in g.members() {
            let Ok(c) =
                DeviceCertificate::check(m.credential.serialized_content(), &m.signature_key)
            else {
                continue;
            };
            if c.user != user {
                continue;
            }
            match &found {
                Some(k) if *k != c.account_key => return Ok(None),
                _ => found = Some(c.account_key),
            }
        }
        Ok(found)
    }

    /// `user:device`
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Fingerprint of this device's account.
    pub fn account_fingerprint(&self) -> &Fingerprint {
        &self.account
    }

    /// The policy (strict mode and verified contacts).
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Take note of a verified device list (own or a contact's): its
    /// withdrawn devices are refused from now on.
    pub fn apply_device_list(&mut self, list: &crate::devices::DeviceList) {
        self.policy.apply_device_list(list);
    }

    /// Switch strict mode.
    pub fn set_strict(&mut self, on: bool) {
        self.policy.set_strict(on);
    }

    /// Mark a user as verified with the account fingerprint compared in
    /// person or by QR code.
    pub fn verify(&mut self, user: &str, fingerprint: Fingerprint) {
        self.policy.verify(user, fingerprint);
    }

    /// One-time key packages for the server.
    pub fn key_packages(&self, n: usize) -> Result<Vec<Vec<u8>>> {
        (0..n)
            .map(|_| {
                let bundle = KeyPackage::builder()
                    .leaf_node_capabilities(leaf_capabilities(false))
                    .build(
                        CIPHERSUITE,
                        &self.provider,
                        &self.keys,
                        self.credential.clone(),
                    )
                    .map_err(internal)?;
                bundle
                    .key_package()
                    .tls_serialize_detached()
                    .map_err(internal)
            })
            .collect()
    }

    /// A fresh last-resort key package. The server hands it out only when
    /// the one-time supply is empty and never deletes it.
    pub fn last_resort_key_package(&mut self) -> Result<Vec<u8>> {
        // The leaf must announce that it understands the last-resort
        // extension, or other devices refuse the package (RFC 9420, 10).
        let bundle = KeyPackage::builder()
            .leaf_node_capabilities(leaf_capabilities(true))
            .mark_as_last_resort()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.keys,
                self.credential.clone(),
            )
            .map_err(internal)?;
        let r = bundle
            .key_package()
            .hash_ref(self.provider.crypto())
            .map_err(internal)?;
        self.last_resort = Some(r);
        bundle
            .key_package()
            .tls_serialize_detached()
            .map_err(internal)
    }

    /// The group a message is for (`None` for a welcome or bytes that are
    /// not an MLS message).
    pub fn group_of(&self, bytes: &[u8]) -> Option<String> {
        let msg = MlsMessageIn::tls_deserialize_exact(bytes).ok()?;
        let pm = msg.try_into_protocol_message().ok()?;
        Some(String::from_utf8_lossy(pm.group_id().as_slice()).into_owned())
    }

    /// The accounts behind key packages (checked like any offer).
    pub fn offer_accounts(&self, offers: &[KeyPackageOffer]) -> Result<Vec<Acct>> {
        offers
            .iter()
            .map(|o| {
                let (kp, member) = self.check_offer(o)?;
                let leaf = kp.leaf_node();
                leaf_acct(
                    leaf.credential().serialized_content(),
                    leaf.signature_key().as_slice(),
                )
                .ok_or(CoreError::Forged(member.id))
            })
            .collect()
    }

    /// Members asked for new keys; an admin's next commit carries them.
    pub fn has_proposals(&self, group: &str) -> Result<bool> {
        Ok(self.group(group)?.pending_proposals().next().is_some())
    }

    /// Does this device hold the group?
    pub fn has_group(&self, group: &str) -> bool {
        self.groups.contains_key(group)
    }

    /// This device's account.
    pub fn acct(&self) -> Acct {
        Acct {
            user: self.user.clone(),
            fp: self.account.as_str().to_string(),
        }
    }

    /// The Trair state of a group; `None` for a plain one-to-one chat.
    pub fn group_state(&self, group: &str) -> Result<Option<GroupState>> {
        read_state(self.group(group)?.extensions())
    }

    /// The accounts in a group (user → account fingerprint).
    pub fn accounts(&self, group: &str) -> Result<Accounts> {
        let g = self.group(group)?;
        let accts: Vec<Acct> = g
            .members()
            .filter_map(|m| leaf_acct(m.credential.serialized_content(), &m.signature_key))
            .collect();
        collect_accounts(&accts)
    }

    /// Create a new group with only this device in it.
    pub fn create_group(&mut self, group: &str) -> Result<()> {
        self.create_group_with(group, None)
    }

    /// Create a Trair group (or join chat) with its state, with only this
    /// device in it.
    pub fn create_trair_group(&mut self, group: &str, state: &GroupState) -> Result<()> {
        if !state.is_admin(&self.acct()) && state.kind == groups::GroupKind::Group {
            return Err(CoreError::NotAllowed(
                "the creator must own the group".into(),
            ));
        }
        self.create_group_with(group, Some(state))
    }

    fn create_group_with(&mut self, group: &str, state: Option<&GroupState>) -> Result<()> {
        if self.groups.contains_key(group) {
            return Err(CoreError::GroupExists(group.to_string()));
        }
        let mut cfg = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .capabilities(leaf_capabilities(false));
        if let Some(s) = state {
            cfg = cfg.with_group_context_extensions(state_extensions(s)?);
        }
        let cfg = cfg.build();
        let g = MlsGroup::new_with_group_id(
            &self.provider,
            &self.keys,
            &cfg,
            GroupId::from_slice(group.as_bytes()),
            self.credential.clone(),
        )
        .map_err(|e| match e {
            NewGroupError::GroupAlreadyExists => CoreError::GroupExists(group.to_string()),
            other => internal(other),
        })?;
        self.groups.insert(group.to_string(), g);
        Ok(())
    }

    fn group(&self, group: &str) -> Result<&MlsGroup> {
        self.groups
            .get(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))
    }

    /// Trust level for a leaf: identity bytes plus device signature key.
    fn judge(&self, identity: &[u8], signature_key: &[u8]) -> Member {
        if signature_key == self.keys.public() {
            return Member {
                id: self.id.clone(),
                user: self.user.clone(),
                trust: Trust::Own,
            };
        }
        match DeviceCertificate::check(identity, signature_key) {
            Ok(c) => {
                let trust = if self.policy.is_withdrawn(&c.id) {
                    Trust::Withdrawn
                } else {
                    self.policy
                        .classify(&self.user, &self.account, &c.user, &c.account)
                };
                Member {
                    id: c.id,
                    user: c.user,
                    trust,
                }
            }
            Err(_) => Member {
                id: format!(
                    "forged:{}",
                    hex::encode(&signature_key[..signature_key.len().min(6)])
                ),
                user: String::new(),
                trust: Trust::Forged,
            },
        }
    }

    /// Members with their leaf index, as this device sees them.
    fn members_indexed(&self, group: &str) -> Result<Vec<(LeafNodeIndex, Member)>> {
        let g = self.group(group)?;
        Ok(g.members()
            .map(|m| {
                (
                    m.index,
                    self.judge(m.credential.serialized_content(), &m.signature_key),
                )
            })
            .collect())
    }

    /// Everyone in the group, with trust levels.
    pub fn members(&self, group: &str) -> Result<Vec<Member>> {
        Ok(self
            .members_indexed(group)?
            .into_iter()
            .map(|(_, m)| m)
            .collect())
    }

    /// Device ids of all other members (the recipients of anything sent).
    pub fn recipients(&self, group: &str) -> Result<Vec<String>> {
        Ok(self
            .members(group)?
            .into_iter()
            .filter(|m| m.trust != Trust::Own)
            .map(|m| m.id)
            .collect())
    }

    /// Current epoch of the group.
    pub fn epoch(&self, group: &str) -> Result<u64> {
        Ok(self.group(group)?.epoch().as_u64())
    }

    /// Short fingerprint of the current epoch. Two members with the same
    /// epoch number but different fingerprints are in a split group.
    pub fn epoch_fingerprint(&self, group: &str) -> Result<String> {
        Ok(hex::encode(
            &self.group(group)?.epoch_authenticator().as_slice()[..8],
        ))
    }

    fn check_offer(&self, offer: &KeyPackageOffer) -> Result<(KeyPackage, Member)> {
        let kp_in = KeyPackageIn::tls_deserialize_exact(offer.bytes.as_slice())
            .map_err(|e| CoreError::InvalidKeyPackage(format!("{e:?}")))?;
        let kp = kp_in
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(|e| CoreError::InvalidKeyPackage(format!("{e:?}")))?;
        // Only Trair's cipher suite. This also keeps attacker input away
        // from the NIST-curve code paths of the crypto library.
        if kp.ciphersuite() != CIPHERSUITE {
            return Err(CoreError::InvalidKeyPackage(format!(
                "cipher suite {:?} is not allowed",
                kp.ciphersuite()
            )));
        }
        let leaf = kp.leaf_node();
        let checked = DeviceCertificate::check(
            leaf.credential().serialized_content(),
            leaf.signature_key().as_slice(),
        )?;
        if checked.id != offer.claimed_device {
            return Err(CoreError::InvalidKeyPackage(format!(
                "server says {} but the package is for {}",
                offer.claimed_device, checked.id
            )));
        }
        let member = self.judge(
            leaf.credential().serialized_content(),
            leaf.signature_key().as_slice(),
        );
        Ok((kp, member))
    }

    /// Add devices to a group. Forged certificates are always refused; in
    /// strict mode every new member must be trusted.
    pub fn add_members(&mut self, group: &str, offers: &[KeyPackageOffer]) -> Result<Outgoing> {
        let present: Vec<String> = self.members(group)?.into_iter().map(|m| m.id).collect();
        let mut kps = Vec::new();
        let mut added = Vec::new();
        for offer in offers {
            if offer.claimed_device == self.id || present.contains(&offer.claimed_device) {
                continue;
            }
            let (kp, member) = self.check_offer(offer)?;
            kps.push(kp);
            added.push(member);
        }
        if kps.is_empty() {
            return Err(CoreError::InvalidKeyPackage("no new devices to add".into()));
        }
        let blocked = self
            .policy
            .blocking(added.iter().map(|m| (m.id.as_str(), m.trust)));
        if !blocked.is_empty() {
            return Err(CoreError::Blocked(format!(
                "not verified: {}",
                blocked.join(", ")
            )));
        }
        let epoch = self.epoch(group)?;
        let (provider, keys) = (&self.provider, &self.keys);
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        let (commit, welcome, _) = g.add_members(provider, keys, &kps).map_err(internal)?;
        Ok(Outgoing {
            group: group.to_string(),
            epoch,
            commit: commit.to_bytes().map_err(internal)?,
            welcome: Some(welcome.to_bytes().map_err(internal)?),
            added,
            removed: Vec::new(),
        })
    }

    /// Remove all devices of a user from a group.
    pub fn remove_user(&mut self, group: &str, user: &str) -> Result<Outgoing> {
        let targets: Vec<(LeafNodeIndex, String)> = self
            .members_indexed(group)?
            .into_iter()
            .filter(|(_, m)| m.user == user && m.trust != Trust::Own)
            .map(|(i, m)| (i, m.id))
            .collect();
        if targets.is_empty() {
            return Err(CoreError::Rejected(format!("{user} is not in {group}")));
        }
        let epoch = self.epoch(group)?;
        let idx: Vec<LeafNodeIndex> = targets.iter().map(|t| t.0).collect();
        let (provider, keys) = (&self.provider, &self.keys);
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        let (commit, welcome, _) = g.remove_members(provider, keys, &idx).map_err(internal)?;
        Ok(Outgoing {
            group: group.to_string(),
            epoch,
            commit: commit.to_bytes().map_err(internal)?,
            welcome: welcome
                .map(|w| w.to_bytes())
                .transpose()
                .map_err(internal)?,
            added: Vec::new(),
            removed: targets.into_iter().map(|t| t.1).collect(),
        })
    }

    /// Remove single devices (for example a withdrawn one) from a group.
    pub fn remove_devices(&mut self, group: &str, ids: &[String]) -> Result<Outgoing> {
        let targets: Vec<(LeafNodeIndex, String)> = self
            .members_indexed(group)?
            .into_iter()
            .filter(|(_, m)| ids.contains(&m.id) && m.trust != Trust::Own)
            .map(|(i, m)| (i, m.id))
            .collect();
        if targets.is_empty() {
            return Err(CoreError::Rejected(format!(
                "none of these devices is in {group}"
            )));
        }
        let epoch = self.epoch(group)?;
        let idx: Vec<LeafNodeIndex> = targets.iter().map(|t| t.0).collect();
        let (provider, keys) = (&self.provider, &self.keys);
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        let (commit, welcome, _) = g.remove_members(provider, keys, &idx).map_err(internal)?;
        Ok(Outgoing {
            group: group.to_string(),
            epoch,
            commit: commit.to_bytes().map_err(internal)?,
            welcome: welcome
                .map(|w| w.to_bytes())
                .transpose()
                .map_err(internal)?,
            added: Vec::new(),
            removed: targets.into_iter().map(|t| t.1).collect(),
        })
    }

    /// Change a Trair group in one commit: add devices (`offers`), remove
    /// devices (`remove`, as `user:device`) and set a new state. The change
    /// is checked against the group's rules first, exactly as every other
    /// member will check it. `allow` are accounts verified another way (a
    /// group key's token); strict mode lets them in although they are not
    /// contacts.
    pub fn change(
        &mut self,
        group: &str,
        offers: &[KeyPackageOffer],
        remove: &[String],
        new_state: Option<&GroupState>,
        allow: &[Acct],
    ) -> Result<Outgoing> {
        let old = self
            .group_state(group)?
            .ok_or_else(|| CoreError::Rejected("not a group".into()))?;
        let g = self.group(group)?;
        let leaves: Vec<(LeafNodeIndex, String, Option<Acct>)> = g
            .members()
            .map(|m| {
                let member = self.judge(m.credential.serialized_content(), &m.signature_key);
                (
                    m.index,
                    member.id,
                    leaf_acct(m.credential.serialized_content(), &m.signature_key),
                )
            })
            .collect();
        let mut kps = Vec::new();
        let mut added = Vec::new();
        let mut added_accts = Vec::new();
        for offer in offers {
            if offer.claimed_device == self.id || leaves.iter().any(|l| l.1 == offer.claimed_device)
            {
                continue;
            }
            let (kp, member) = self.check_offer(offer)?;
            let leaf = kp.leaf_node();
            if !leaf
                .capabilities()
                .extensions()
                .contains(&ExtensionType::Unknown(TRAIR_EXTENSION))
            {
                return Err(CoreError::Outdated(member.id));
            }
            let acct = leaf_acct(
                leaf.credential().serialized_content(),
                leaf.signature_key().as_slice(),
            )
            .ok_or_else(|| CoreError::Forged(member.id.clone()))?;
            kps.push(kp);
            added.push(member);
            added_accts.push(acct);
        }
        let blocked = self.policy.blocking(
            added
                .iter()
                .zip(&added_accts)
                .filter(|(_, a)| !allow.contains(a))
                .map(|(m, _)| (m.id.as_str(), m.trust)),
        );
        if !blocked.is_empty() {
            return Err(CoreError::Blocked(format!(
                "not verified: {}",
                blocked.join(", ")
            )));
        }
        let targets: Vec<&(LeafNodeIndex, String, Option<Acct>)> = leaves
            .iter()
            .filter(|l| remove.contains(&l.1) && l.1 != self.id)
            .collect();
        if kps.is_empty()
            && targets.is_empty()
            && new_state.is_none_or(|n| *n == old)
            && !self.has_proposals(group)?
        {
            return Err(CoreError::Rejected("nothing to change".into()));
        }
        // The same check every member makes before following it.
        let after = collect_accounts(
            leaves
                .iter()
                .filter(|l| !targets.iter().any(|t| t.0 == l.0))
                .filter_map(|l| l.2.as_ref())
                .chain(&added_accts),
        )?;
        let removed_users: Vec<String> = targets
            .iter()
            .filter_map(|t| t.2.as_ref().map(|a| a.user.clone()))
            .collect();
        let new = new_state.unwrap_or(&old);
        groups::check_change(&old, new, &self.acct(), &removed_users, &after)?;
        let extensions = match new_state {
            Some(n) if *n != old => Some(state_extensions(n)?),
            _ => None,
        };
        let epoch = self.epoch(group)?;
        let removed: Vec<String> = targets.iter().map(|t| t.1.clone()).collect();
        let indices: Vec<LeafNodeIndex> = targets.iter().map(|t| t.0).collect();
        let (provider, keys) = (&self.provider, &self.keys);
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        let mut builder = g
            .commit_builder()
            .propose_adds(kps)
            .propose_removals(indices);
        if let Some(e) = extensions {
            builder = builder
                .propose_group_context_extensions(e)
                .map_err(internal)?;
        }
        let bundle = builder
            .load_psks(provider.storage())
            .map_err(internal)?
            // Besides this commit's own proposals only the stored key
            // updates of members (see `receive`).
            .build(provider.rand(), provider.crypto(), keys, |p| {
                matches!(
                    p.proposal().proposal_type(),
                    ProposalType::Add
                        | ProposalType::Remove
                        | ProposalType::Update
                        | ProposalType::GroupContextExtensions
                )
            })
            .map_err(internal)?
            .stage_commit(provider)
            .map_err(internal)?;
        let (commit, welcome, _) = bundle.into_messages();
        Ok(Outgoing {
            group: group.to_string(),
            epoch,
            commit: commit.to_bytes().map_err(internal)?,
            welcome: welcome
                .map(|w| w.to_bytes())
                .transpose()
                .map_err(internal)?,
            added,
            removed,
        })
    }

    /// Ask for new keys of this device in a Trair group where it may not
    /// commit itself (after joining through the last-resort package). The
    /// next change by an admin carries it. Send the bytes to the members.
    pub fn propose_update(&mut self, group: &str) -> Result<Vec<u8>> {
        if self.group_state(group)?.is_none() {
            return Err(CoreError::Rejected("not a group".into()));
        }
        let (provider, keys) = (&self.provider, &self.keys);
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        let (m, _) = g
            .propose_self_update(provider, keys, LeafNodeParameters::default())
            .map_err(internal)?;
        m.to_bytes().map_err(internal)
    }

    /// Replace this device's keys in a group (needed after joining through
    /// the last-resort key package, and useful from time to time).
    pub fn self_update(&mut self, group: &str) -> Result<Outgoing> {
        let epoch = self.epoch(group)?;
        let (provider, keys) = (&self.provider, &self.keys);
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        let bundle = g
            .self_update(provider, keys, LeafNodeParameters::default())
            .map_err(internal)?;
        let (commit, welcome, _) = bundle.into_messages();
        Ok(Outgoing {
            group: group.to_string(),
            epoch,
            commit: commit.to_bytes().map_err(internal)?,
            welcome: welcome
                .map(|w| w.to_bytes())
                .transpose()
                .map_err(internal)?,
            added: Vec::new(),
            removed: Vec::new(),
        })
    }

    /// The server accepted the last commit for this group: apply it.
    pub fn confirm(&mut self, group: &str) -> Result<()> {
        let provider = &self.provider;
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        g.merge_pending_commit(provider).map_err(internal)
    }

    /// The server refused the last commit: throw it away.
    pub fn discard(&mut self, group: &str) -> Result<()> {
        let provider = &self.provider;
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        g.clear_pending_commit(provider.storage()).map_err(internal)
    }

    /// Encrypt a chat message. Refused if the group contains a forged
    /// device, and in strict mode if anyone is not trusted.
    pub fn encrypt(&mut self, group: &str, text: &str) -> Result<Vec<u8>> {
        self.check_send(group)?;
        let seq = {
            let s = self.seq_out.entry(group.to_string()).or_insert(0);
            *s += 1;
            *s
        };
        self.seal(
            group,
            &Payload {
                seq,
                text: text.to_string(),
                profile: None,
                control: None,
            },
        )
    }

    /// Encrypt this account's profile for a group. Same checks as a text.
    pub fn encrypt_profile(&mut self, group: &str, profile: &serde_json::Value) -> Result<Vec<u8>> {
        self.check_send(group)?;
        self.seal(
            group,
            &Payload {
                seq: 0,
                text: String::new(),
                profile: Some(profile.clone()),
                control: None,
            },
        )
    }

    /// Encrypt a chat setting for a group. Same checks as a text.
    pub fn encrypt_control(&mut self, group: &str, control: &serde_json::Value) -> Result<Vec<u8>> {
        self.check_send(group)?;
        self.seal(
            group,
            &Payload {
                seq: 0,
                text: String::new(),
                profile: None,
                control: Some(control.clone()),
            },
        )
    }

    /// Encrypt a group setting that must go out even while a withdrawn or
    /// unverified device is still in the group: leaving, or asking to
    /// remove one's own withdrawn device. It carries no secret. A forged
    /// device still stops it.
    pub fn encrypt_control_anyway(
        &mut self,
        group: &str,
        control: &serde_json::Value,
    ) -> Result<Vec<u8>> {
        if let Some(f) = self
            .members(group)?
            .into_iter()
            .find(|m| m.trust == Trust::Forged)
        {
            return Err(CoreError::Forged(f.id));
        }
        self.seal(
            group,
            &Payload {
                seq: 0,
                text: String::new(),
                profile: None,
                control: Some(control.clone()),
            },
        )
    }

    /// Nothing goes to a group with a forged, withdrawn or (in strict mode)
    /// unverified device.
    fn check_send(&self, group: &str) -> Result<()> {
        let members = self.members(group)?;
        let forged: Vec<String> = members
            .iter()
            .filter(|m| m.trust == Trust::Forged)
            .map(|m| m.id.clone())
            .collect();
        if !forged.is_empty() {
            return Err(CoreError::Forged(forged.join(", ")));
        }
        let withdrawn: Vec<String> = members
            .iter()
            .filter(|m| m.trust == Trust::Withdrawn)
            .map(|m| m.id.clone())
            .collect();
        if !withdrawn.is_empty() {
            return Err(CoreError::Withdrawn(withdrawn.join(", ")));
        }
        // In a Trair group every member was added by an admin (each device
        // checked that): the admin vouches for those who are not contacts.
        // A key that differs from a verified one still blocks.
        let vouched = self.group_state(group)?.is_some();
        let blocked = self.policy.blocking(
            members
                .iter()
                .filter(|m| !(vouched && m.trust == Trust::Unverified))
                .map(|m| (m.id.as_str(), m.trust)),
        );
        if !blocked.is_empty() {
            return Err(CoreError::Blocked(format!(
                "not verified: {}",
                blocked.join(", ")
            )));
        }
        Ok(())
    }

    fn seal(&mut self, group: &str, payload: &Payload) -> Result<Vec<u8>> {
        let payload = serde_json::to_vec(payload).map_err(internal)?;
        let (provider, keys) = (&self.provider, &self.keys);
        let g = self
            .groups
            .get_mut(group)
            .ok_or_else(|| CoreError::UnknownGroup(group.to_string()))?;
        let m = g
            .create_message(provider, keys, &payload)
            .map_err(internal)?;
        m.to_bytes().map_err(internal)
    }

    /// Process anything that arrives from the server: a welcome, a commit
    /// or a chat message. The server's claims about the sender are ignored.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Event> {
        let msg = MlsMessageIn::tls_deserialize_exact(bytes)
            .map_err(|e| CoreError::Rejected(format!("not an MLS message: {e:?}")))?;
        if msg.wire_format() == WireFormat::Welcome {
            match msg.extract() {
                MlsMessageBodyIn::Welcome(w) => self.join(w),
                _ => Err(CoreError::Rejected("broken welcome".into())),
            }
        } else {
            let pm = msg
                .try_into_protocol_message()
                .map_err(|e| CoreError::Rejected(format!("{e:?}")))?;
            self.process(pm)
        }
    }

    fn join(&mut self, welcome: Welcome) -> Result<Event> {
        let via_last_resort = self
            .last_resort
            .as_ref()
            .is_some_and(|r| welcome.secrets().iter().any(|s| s.new_member() == *r));
        let cfg = MlsGroupJoinConfig::builder()
            .use_ratchet_tree_extension(true)
            .build();
        let staged =
            StagedWelcome::new_from_welcome(&self.provider, &cfg, welcome, None).map_err(|e| {
                match e {
                    WelcomeError::GroupAlreadyExists => {
                        CoreError::GroupExists("welcome for a group this device already has".into())
                    }
                    other => CoreError::Rejected(format!("{other:?}")),
                }
            })?;
        let by = staged.welcome_sender().map_err(internal)?;
        let by = self.judge(
            by.credential().serialized_content(),
            by.signature_key().as_slice(),
        );
        // Check every member before the group is stored.
        let members: Vec<Member> = staged
            .members()
            .map(|m| self.judge(m.credential.serialized_content(), &m.signature_key))
            .collect();
        if let Some(f) = members.iter().find(|m| m.trust == Trust::Forged) {
            return Err(CoreError::Forged(format!("group contains {}", f.id)));
        }
        if let Some(w) = members.iter().find(|m| m.trust == Trust::Withdrawn) {
            return Err(CoreError::Withdrawn(format!("group contains {}", w.id)));
        }
        let state = read_state(staged.group_context().extensions())?;
        if let Some(s) = &state {
            let accts: Vec<Acct> = staged
                .members()
                .filter_map(|m| leaf_acct(m.credential.serialized_content(), &m.signature_key))
                .collect();
            let adder = staged.welcome_sender().map_err(internal)?;
            let adder = leaf_acct(
                adder.credential().serialized_content(),
                adder.signature_key().as_slice(),
            )
            .ok_or_else(|| CoreError::Forged("the adder".into()))?;
            groups::check_welcome(s, &adder, &collect_accounts(&accts)?)?;
        }
        let g = staged
            .into_group(&self.provider)
            .map_err(|e| CoreError::Rejected(format!("{e:?}")))?;
        let group = String::from_utf8_lossy(g.group_id().as_slice()).into_owned();
        self.groups.insert(group.clone(), g);
        Ok(Event::Joined {
            group,
            members,
            by,
            via_last_resort,
            state,
        })
    }

    fn process(&mut self, pm: ProtocolMessage) -> Result<Event> {
        let group = String::from_utf8_lossy(pm.group_id().as_slice()).into_owned();
        let before: HashMap<u32, Member> = self
            .members_indexed(&group)
            .map_err(|_| CoreError::Rejected(format!("message for unknown group {group}")))?
            .into_iter()
            .map(|(i, m)| (i.u32(), m))
            .collect();
        let old_state = self.group_state(&group)?;
        let accts_before: HashMap<u32, Acct> = self
            .group(&group)?
            .members()
            .filter_map(|m| {
                leaf_acct(m.credential.serialized_content(), &m.signature_key)
                    .map(|a| (m.index.u32(), a))
            })
            .collect();
        let provider = &self.provider;
        let g = self
            .groups
            .get_mut(&group)
            .ok_or_else(|| CoreError::UnknownGroup(group.clone()))?;
        let processed = g
            .process_message(provider, pm)
            .map_err(|e| CoreError::Rejected(format!("{e:?}")))?;
        let (sender, sender_index) = match processed.sender() {
            Sender::Member(i) => (
                before
                    .get(&i.u32())
                    .map(|m| m.id.clone())
                    .ok_or_else(|| CoreError::Rejected("sender not in group".into()))?,
                i.u32(),
            ),
            _ => return Err(CoreError::Rejected("sender is not a member".into())),
        };
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(a) => {
                let p: Payload = serde_json::from_slice(&a.into_bytes())
                    .map_err(|_| CoreError::Rejected("unreadable payload".into()))?;
                if let Some(control) = p.control {
                    return Ok(Event::Control {
                        group,
                        from: sender,
                        control,
                    });
                }
                if let Some(profile) = p.profile {
                    let user = sender.split(':').next().unwrap_or_default().to_string();
                    let account_key = self
                        .account_key_of(&group, &user)?
                        .ok_or_else(|| CoreError::Rejected("sender's account unclear".into()))?;
                    return Ok(Event::Profile {
                        group,
                        from: sender,
                        account_key,
                        profile,
                    });
                }
                let last = self
                    .seq_in
                    .insert((group.clone(), sender.clone()), p.seq)
                    .unwrap_or(0);
                let missing = p.seq.saturating_sub(last + 1);
                Ok(Event::Message {
                    group,
                    from: sender,
                    text: p.text,
                    missing,
                })
            }
            ProcessedMessageContent::StagedCommitMessage(sc) => {
                let mut added = Vec::new();
                for a in sc.add_proposals() {
                    let leaf = a.add_proposal().key_package().leaf_node();
                    let m = self.judge(
                        leaf.credential().serialized_content(),
                        leaf.signature_key().as_slice(),
                    );
                    if m.trust == Trust::Forged {
                        // Do not follow a group that now contains a forged device.
                        return Err(CoreError::Forged(format!(
                            "{sender} tried to add a forged device"
                        )));
                    }
                    if m.trust == Trust::Withdrawn {
                        return Err(CoreError::Withdrawn(format!(
                            "{sender} tried to add {}",
                            m.id
                        )));
                    }
                    added.push(m);
                }
                let removed: Vec<String> = sc
                    .remove_proposals()
                    .filter_map(|r| before.get(&r.remove_proposal().removed().u32()))
                    .map(|m| m.id.clone())
                    .collect();
                let new_state = read_state(sc.group_context().extensions())?;
                let state_changed = new_state != old_state;
                match (&old_state, &new_state) {
                    (None, None) => {}
                    (Some(old), Some(new)) => {
                        // Follow only changes the group's rules allow.
                        for p in sc.queued_proposals() {
                            if !matches!(
                                p.proposal().proposal_type(),
                                ProposalType::Add
                                    | ProposalType::Remove
                                    | ProposalType::Update
                                    | ProposalType::GroupContextExtensions
                            ) {
                                return Err(CoreError::NotAllowed(format!(
                                    "{sender} sent a kind of change Trair does not use"
                                )));
                            }
                        }
                        let committer = accts_before
                            .get(&sender_index)
                            .ok_or_else(|| CoreError::Rejected("committer unknown".into()))?;
                        let removed_idx: Vec<u32> = sc
                            .remove_proposals()
                            .map(|r| r.remove_proposal().removed().u32())
                            .collect();
                        let removed_users: Vec<String> = removed_idx
                            .iter()
                            .filter_map(|i| accts_before.get(i).map(|a| a.user.clone()))
                            .collect();
                        let mut after_accts: Vec<Acct> = accts_before
                            .iter()
                            .filter(|(i, _)| !removed_idx.contains(i))
                            .map(|(_, a)| a.clone())
                            .collect();
                        for a in sc.add_proposals() {
                            let leaf = a.add_proposal().key_package().leaf_node();
                            if let Some(acct) = leaf_acct(
                                leaf.credential().serialized_content(),
                                leaf.signature_key().as_slice(),
                            ) {
                                after_accts.push(acct);
                            }
                        }
                        groups::check_change(
                            old,
                            new,
                            committer,
                            &removed_users,
                            &collect_accounts(&after_accts)?,
                        )?;
                    }
                    _ => {
                        return Err(CoreError::NotAllowed(
                            "a chat cannot turn into a group or back".into(),
                        ));
                    }
                }
                let self_removed = sc.self_removed();
                let provider = &self.provider;
                let g = self
                    .groups
                    .get_mut(&group)
                    .ok_or_else(|| CoreError::UnknownGroup(group.clone()))?;
                g.merge_staged_commit(provider, *sc).map_err(internal)?;
                let i_was_removed = self_removed || !g.is_active();
                Ok(Event::Commit {
                    group,
                    by: sender,
                    added,
                    removed,
                    i_was_removed,
                    state_changed,
                })
            }
            ProcessedMessageContent::ProposalMessage(p) => {
                // In a Trair group members ask for new keys this way; the
                // next admin's commit carries it. Nothing else is kept.
                if old_state.is_some() && p.proposal().proposal_type() == ProposalType::Update {
                    let provider = &self.provider;
                    let g = self
                        .groups
                        .get_mut(&group)
                        .ok_or_else(|| CoreError::UnknownGroup(group.clone()))?;
                    g.store_pending_proposal(provider.storage(), *p)
                        .map_err(internal)?;
                }
                Ok(Event::Other { group })
            }
            _ => Ok(Event::Other { group }),
        }
    }
}
