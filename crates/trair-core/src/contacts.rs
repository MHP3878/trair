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
//!
//! A friendship key ([`InviteCode`]) is a contact code with a random
//! token and an end time:
//! `trair2:<user>:<fingerprint>:<token, 32 hex>:<f|v><end>:<check>`. Only
//! the account that made it knows the token (as a hash). Whoever uses it
//! before the end becomes a contact at once, without a request; it works
//! for one person only. `v`: the chat it opens vanishes at the end, on
//! both sides.
//!
//! A group key has the same shape with `trair3` and `m` (member) or `g`
//! (guest: leaves again at the end). It names the admin who made it; its
//! token goes to that admin, whose device adds whoever brings it before
//! the end, as often as it is used.

use sha2::{Digest, Sha256};

use crate::account::{Fingerprint, validate_name};
use crate::error::{CoreError, Result};

const PREFIX: &str = "trair1";
const INVITE_PREFIX: &str = "trair2";
const GROUP_PREFIX: &str = "trair3";
const CHECK_CONTEXT: &[u8] = b"trair/contact-code/v1\0";
const INVITE_CHECK_CONTEXT: &[u8] = b"trair/invite-code/v1\0";

/// What a friendship key does when it is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteKind {
    /// A lasting contact.
    Friend,
    /// A chat that vanishes at the end, with a contact that goes with it.
    Vanishing,
    /// A group key: whoever uses it joins the group.
    Member,
    /// A group key for guests: they leave the group again at the end.
    Guest,
}

impl InviteKind {
    /// One letter in the code and in the database.
    pub fn letter(self) -> char {
        match self {
            InviteKind::Friend => 'f',
            InviteKind::Vanishing => 'v',
            InviteKind::Member => 'm',
            InviteKind::Guest => 'g',
        }
    }

    /// A group key (not a friendship key).
    pub fn is_group(self) -> bool {
        matches!(self, InviteKind::Member | InviteKind::Guest)
    }

    fn prefix(self) -> &'static str {
        if self.is_group() {
            GROUP_PREFIX
        } else {
            INVITE_PREFIX
        }
    }

    /// From [`InviteKind::letter`].
    pub fn from_letter(c: char) -> Option<Self> {
        match c {
            'f' => Some(InviteKind::Friend),
            'v' => Some(InviteKind::Vanishing),
            'm' => Some(InviteKind::Member),
            'g' => Some(InviteKind::Guest),
            _ => None,
        }
    }
}

/// A parsed friendship key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteCode {
    /// Whose key it is.
    pub contact: ContactCode,
    /// The random token (proves the key, used once).
    pub token: [u8; 16],
    /// Friend or vanishing chat.
    pub kind: InviteKind,
    /// Seconds since 1970: the key stops working (and a vanishing chat
    /// ends) then.
    pub ends: u64,
}

fn invite_check(user: &str, fingerprint: &Fingerprint, token: &str, tail: &str) -> String {
    let mut h = Sha256::new();
    h.update(INVITE_CHECK_CONTEXT);
    for part in [user, fingerprint.as_str(), token, tail] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    hex::encode(&h.finalize()[..2])
}

impl InviteCode {
    /// The key as text.
    pub fn text(&self) -> String {
        let token = hex::encode(self.token);
        let tail = format!("{}{}", self.kind.letter(), self.ends);
        format!(
            "{}:{}:{}:{token}:{tail}:{}",
            self.kind.prefix(),
            self.contact.user,
            self.contact.fingerprint.as_str(),
            invite_check(&self.contact.user, &self.contact.fingerprint, &token, &tail)
        )
    }

    /// Read a key; like [`ContactCode::parse`], anything that does not
    /// match exactly is refused.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim().to_ascii_lowercase();
        let parts: Vec<&str> = text.split(':').collect();
        let [prefix, user, fp, token, tail, sum] = parts.as_slice() else {
            return Err(CoreError::Rejected("not a friendship key".into()));
        };
        if *prefix != INVITE_PREFIX && *prefix != GROUP_PREFIX {
            return Err(CoreError::Rejected("not a friendship key".into()));
        }
        validate_name(user)?;
        let fingerprint = Fingerprint::parse(fp)?;
        let mut t = [0u8; 16];
        hex::decode_to_slice(token, &mut t)
            .map_err(|_| CoreError::Rejected("friendship key has a typo".into()))?;
        let mut chars = tail.chars();
        let kind = chars
            .next()
            .and_then(InviteKind::from_letter)
            .ok_or_else(|| CoreError::Rejected("friendship key has a typo".into()))?;
        if kind.prefix() != *prefix {
            return Err(CoreError::Rejected("friendship key has a typo".into()));
        }
        let ends: u64 = chars
            .as_str()
            .parse()
            .map_err(|_| CoreError::Rejected("friendship key has a typo".into()))?;
        if invite_check(user, &fingerprint, token, tail) != *sum {
            return Err(CoreError::Rejected("friendship key has a typo".into()));
        }
        Ok(InviteCode {
            contact: ContactCode {
                user: user.to_string(),
                fingerprint,
            },
            token: t,
            kind,
            ends,
        })
    }
}

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

    #[test]
    fn friendship_keys_round_trip_and_catch_typos() {
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).unwrap();
        let code = InviteCode {
            contact: ContactCode {
                user: "local-0a1b2c3d4e5f".into(),
                fingerprint: Fingerprint::of(b"key"),
            },
            token,
            kind: InviteKind::Vanishing,
            ends: 1_800_003_600,
        };
        let text = code.text();
        assert!(text.starts_with("trair2:"));
        assert_eq!(InviteCode::parse(&text).unwrap(), code);
        assert!(ContactCode::parse(&text).is_err(), "not a plain code");
        for i in INVITE_PREFIX.len() + 1..text.len() {
            let mut b = text.clone().into_bytes();
            if b[i] == b':' {
                continue;
            }
            b[i] = if b[i] == b'0' { b'1' } else { b'0' };
            let changed = String::from_utf8(b).unwrap();
            assert!(InviteCode::parse(&changed).is_err(), "{changed}");
        }
    }

    #[test]
    fn group_keys_have_their_own_prefix() {
        for kind in [InviteKind::Member, InviteKind::Guest] {
            let code = InviteCode {
                contact: ContactCode {
                    user: "olga".into(),
                    fingerprint: Fingerprint::of(b"key"),
                },
                token: [7; 16],
                kind,
                ends: 1_800_003_600,
            };
            let text = code.text();
            assert!(text.starts_with("trair3:"), "{text}");
            assert_eq!(InviteCode::parse(&text).unwrap(), code);
            // The prefix and the letter must agree.
            let swapped = text.replacen("trair3", "trair2", 1);
            assert!(InviteCode::parse(&swapped).is_err());
        }
    }
}
