// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Trust levels and the strict mode (decision 6: on by default).

use std::collections::{HashMap, HashSet};

use crate::account::Fingerprint;
use crate::devices::DeviceList;

/// How far a group member can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// This device.
    Own,
    /// Another device signed by my own account.
    OwnAccount,
    /// Signed by an account whose fingerprint I verified for this user.
    Verified,
    /// Valid certificate, but I never verified this user.
    Unverified,
    /// I verified this user, but this device is signed by a different
    /// account key. Either a new account or an attack: never normal.
    Mismatch,
    /// The device certificate is invalid. Always refused.
    Forged,
    /// The account withdrew this device (lost or given away). Always
    /// refused, strict mode or not.
    Withdrawn,
}

impl Trust {
    /// True for members that strict mode accepts.
    pub fn is_trusted(self) -> bool {
        matches!(self, Trust::Own | Trust::OwnAccount | Trust::Verified)
    }

    /// True for members that are refused even without strict mode.
    pub fn always_refused(self) -> bool {
        matches!(self, Trust::Forged | Trust::Withdrawn)
    }
}

/// Verified account fingerprints per user plus the strict switch.
#[derive(Debug, Clone)]
pub struct Policy {
    strict: bool,
    pins: HashMap<String, Fingerprint>,
    /// Withdrawn devices, as `user:device`.
    withdrawn: HashSet<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            strict: true,
            pins: HashMap::new(),
            withdrawn: HashSet::new(),
        }
    }
}

impl Policy {
    /// Is strict mode on?
    pub fn strict(&self) -> bool {
        self.strict
    }

    /// Switch strict mode.
    pub fn set_strict(&mut self, on: bool) {
        self.strict = on;
    }

    /// Remember the verified account fingerprint of a user (after
    /// comparing it in person or by QR code). Replaces an older one.
    pub fn verify(&mut self, user: &str, fingerprint: Fingerprint) {
        self.pins.insert(user.to_string(), fingerprint);
    }

    /// The verified fingerprint of a user, if any.
    pub fn verified(&self, user: &str) -> Option<&Fingerprint> {
        self.pins.get(user)
    }

    /// Take note of the withdrawn devices in a verified device list.
    /// Withdrawals are never undone.
    pub fn apply_device_list(&mut self, list: &DeviceList) {
        for d in &list.withdrawn {
            self.withdrawn.insert(format!("{}:{}", list.user, d.id));
        }
    }

    /// Was this device (`user:device`) withdrawn by its account?
    pub fn is_withdrawn(&self, device_id: &str) -> bool {
        self.withdrawn.contains(device_id)
    }

    /// Trust level of a member that is not this device.
    pub fn classify(
        &self,
        my_user: &str,
        my_account: &Fingerprint,
        member_user: &str,
        member_account: &Fingerprint,
    ) -> Trust {
        if member_account == my_account && member_user == my_user {
            return Trust::OwnAccount;
        }
        if member_user == my_user {
            // A device with my name but not signed by my account.
            return Trust::Mismatch;
        }
        match self.pins.get(member_user) {
            Some(pin) if pin == member_account => Trust::Verified,
            Some(_) => Trust::Mismatch,
            None => Trust::Unverified,
        }
    }

    /// Members that stop an action. Forged and withdrawn devices always
    /// do; in strict mode every member that is not trusted does.
    pub fn blocking<'a>(&self, members: impl IntoIterator<Item = (&'a str, Trust)>) -> Vec<String> {
        members
            .into_iter()
            .filter(|(_, t)| t.always_refused() || (self.strict && !t.is_trusted()))
            .map(|(id, _)| id.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_is_default() {
        assert!(Policy::default().strict());
    }

    #[test]
    fn classification() {
        let mine = Fingerprint::of(b"me");
        let bob = Fingerprint::of(b"bob");
        let evil = Fingerprint::of(b"evil");
        let mut p = Policy::default();
        assert_eq!(
            p.classify("alice", &mine, "alice", &mine),
            Trust::OwnAccount
        );
        assert_eq!(p.classify("alice", &mine, "alice", &evil), Trust::Mismatch);
        assert_eq!(p.classify("alice", &mine, "bob", &bob), Trust::Unverified);
        p.verify("bob", bob.clone());
        assert_eq!(p.classify("alice", &mine, "bob", &bob), Trust::Verified);
        assert_eq!(p.classify("alice", &mine, "bob", &evil), Trust::Mismatch);
    }

    #[test]
    fn strict_blocks_only_untrusted() {
        let mut p = Policy::default();
        let members = [
            ("a:pc", Trust::Own),
            ("b:pc", Trust::Verified),
            ("c:pc", Trust::Unverified),
            ("x:pc", Trust::Forged),
        ];
        assert_eq!(
            p.blocking(members),
            vec!["c:pc".to_string(), "x:pc".to_string()]
        );
        p.set_strict(false);
        assert_eq!(
            p.blocking(members),
            vec!["x:pc".to_string()],
            "forged is always blocked"
        );
    }
}
