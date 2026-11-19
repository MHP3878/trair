// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Contact codes.
//!
//! There is no phone book and no search: two people exchange contact
//! codes. A code names the account and carries the fingerprint of its
//! account key, so whoever adds a contact by code pins exactly that key.
//! A code is not secret, but it only protects as well as the way it was
//! passed on: a code handed over in person or by QR code cannot be swapped
//! by anyone in between.
//!
//! Format: `trair1:<user>:<fingerprint, 32 hex>:<check, 4 hex>`. The check
//! only catches typing and copy mistakes; the fingerprint is the security.

use sha2::{Digest, Sha256};

use crate::account::{Fingerprint, validate_name};
use crate::error::{CoreError, Result};

const PREFIX: &str = "trair1";
const CHECK_CONTEXT: &[u8] = b"trair/contact-code/v1\0";

/// A parsed contact code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactCode {
    /// The account's user name on the server.
    pub user: String,
    /// Fingerprint of the account key.
    pub fingerprint: Fingerprint,
}

fn check(user: &str, fingerprint: &Fingerprint) -> String {
    let mut h = Sha256::new();
    h.update(CHECK_CONTEXT);
    h.update(user.as_bytes());
    h.update([0]);
    h.update(fingerprint.as_str().as_bytes());
    hex::encode(&h.finalize()[..2])
}

impl ContactCode {
    /// The code as text.
    pub fn text(&self) -> String {
        format!(
            "{PREFIX}:{}:{}:{}",
            self.user,
            self.fingerprint.as_str(),
            check(&self.user, &self.fingerprint)
        )
    }

    /// Read a code. Spaces around it and upper case are fine; anything
    /// else that does not match exactly is refused.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim().to_ascii_lowercase();
        let parts: Vec<&str> = text.split(':').collect();
        let [prefix, user, fp, sum] = parts.as_slice() else {
            return Err(CoreError::Rejected("not a contact code".into()));
        };
        if *prefix != PREFIX {
            return Err(CoreError::Rejected("not a contact code".into()));
        }
        validate_name(user)?;
        let fingerprint = Fingerprint::parse(fp)?;
        if check(user, &fingerprint) != *sum {
            return Err(CoreError::Rejected("contact code has a typo".into()));
        }
        Ok(ContactCode {
            user: user.to_string(),
            fingerprint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_typos() {
        let code = ContactCode {
            user: "local-0a1b2c3d4e5f".into(),
            fingerprint: Fingerprint::of(b"key"),
        };
        let text = code.text();
        assert_eq!(ContactCode::parse(&text).unwrap(), code);
        assert_eq!(
            ContactCode::parse(&format!("  {}  ", text.to_uppercase())).unwrap(),
            code
        );
        // One changed character anywhere is noticed.
        for i in PREFIX.len() + 1..text.len() {
            let mut b = text.clone().into_bytes();
            if b[i] == b':' {
                continue;
            }
            b[i] = if b[i] == b'0' { b'1' } else { b'0' };
            let changed = String::from_utf8(b).unwrap();
            assert!(ContactCode::parse(&changed).is_err(), "{changed}");
        }
        assert!(ContactCode::parse("trair1:a:b").is_err());
        assert!(ContactCode::parse("").is_err());
    }
}
