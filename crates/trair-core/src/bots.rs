// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Bots (M9, step 4a). A bot is an account of its own, run by the
//! program `trair-bot` on its owner's computer. Its owner vouches for it:
//! the owner's account key signs the bot's account name, the bot's account
//! key and the owner's name ([`BotCert`]). The server only lets a bot
//! write once such a certificate is there, and everyone who sees the bot
//! can check who it belongs to without trusting the server.
//!
//! The way there: `trair-bot` makes a request code
//! (`trairbot1:<bot>:<bot account key>:<secret>`, [`request_code`]) and
//! hands the server only the hash of the secret. The owner pastes the code
//! into Trair, which signs it ([`vouch`]) and hands the certificate plus
//! the secret to the server. Name and key of a bot are public; the secret
//! is what makes sure only the person the bot's operator gave the code to
//! can become its owner. Each secret works once.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::account::{Account, validate_name};
use crate::error::{CoreError, Result, internal};
use crate::proofs;
use openmls_traits::signatures::Signer;

const CONTEXT: &[u8] = b"trair/bot-owner/v1\0";
const PREFIX: &str = "trairbot1";

/// Length of an account key (Ed25519), in bytes.
const KEY_LEN: usize = 32;
/// Length of the one-time secret of a request code, in bytes.
pub const SECRET_LEN: usize = 16;

/// The owner's word for a bot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotCert {
    /// The bot's account name.
    pub bot: String,
    /// The bot's account key, hex.
    pub bot_key: String,
    /// The owner's account name.
    pub owner: String,
    /// When the owner signed (unix seconds).
    pub made: u64,
    /// The owner's account key over all of the above, hex.
    pub sig: String,
}

fn bytes(bot: &str, bot_key: &[u8], owner: &str, made: u64) -> Vec<u8> {
    let mut b = CONTEXT.to_vec();
    for part in [bot.as_bytes(), bot_key, owner.as_bytes()] {
        b.extend_from_slice(&(part.len() as u64).to_le_bytes());
        b.extend_from_slice(part);
    }
    b.extend_from_slice(&made.to_le_bytes());
    b
}

/// A request code read back: who the bot is and its one-time secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The bot's account name.
    pub bot: String,
    /// The bot's account key.
    pub key: Vec<u8>,
    /// The one-time secret.
    pub secret: Vec<u8>,
}

/// What the server keeps of a secret: SHA-256, hex.
pub fn secret_hash(secret: &[u8]) -> String {
    hex::encode(Sha256::digest(secret))
}

/// A fresh request code for a bot, and the hash of its secret for the
/// server.
pub fn request_code(bot: &Account) -> Result<(String, String)> {
    let mut secret = [0u8; SECRET_LEN];
    getrandom::fill(&mut secret).map_err(internal)?;
    Ok((
        format!(
            "{PREFIX}:{}:{}:{}",
            bot.user(),
            hex::encode(bot.public_key()),
            hex::encode(secret)
        ),
        secret_hash(&secret),
    ))
}

/// Read a request code.
pub fn parse_request(code: &str) -> Result<Request> {
    let bad = || CoreError::Rejected("bot code".into());
    let mut parts = code.trim().split(':');
    if parts.next() != Some(PREFIX) {
        return Err(bad());
    }
    let (Some(user), Some(key), Some(secret), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad());
    };
    validate_name(user).map_err(|_| bad())?;
    let key = hex::decode(key)
        .ok()
        .filter(|k| k.len() == KEY_LEN)
        .ok_or_else(bad)?;
    let secret = hex::decode(secret)
        .ok()
        .filter(|s| s.len() == SECRET_LEN)
        .ok_or_else(bad)?;
    Ok(Request {
        bot: user.to_string(),
        key,
        secret,
    })
}

/// The owner vouches for the bot behind a request code. Returns the
/// certificate and the secret (for the server, not part of the
/// certificate).
pub fn vouch(owner: &Account, code: &str, made: u64) -> Result<(BotCert, Vec<u8>)> {
    let Request { bot, key, secret } = parse_request(code)?;
    if bot == owner.user() {
        return Err(CoreError::Rejected(
            "an account cannot be its own bot".into(),
        ));
    }
    let sig = owner
        .keys()
        .sign(&bytes(&bot, &key, owner.user(), made))
        .map_err(internal)?;
    Ok((
        BotCert {
            bot,
            bot_key: hex::encode(key),
            owner: owner.user().to_string(),
            made,
            sig: hex::encode(sig),
        },
        secret,
    ))
}

/// Check a certificate against the owner's account key. Also checks that
/// it is about `bot` with account key `bot_key`, if those are known.
pub fn verify(
    cert: &BotCert,
    owner_key: &[u8],
    bot: Option<&str>,
    bot_key: Option<&[u8]>,
) -> Result<()> {
    let forged = |why: &str| CoreError::Forged(format!("bot certificate: {why}"));
    validate_name(&cert.bot).map_err(|_| forged("bot name"))?;
    validate_name(&cert.owner).map_err(|_| forged("owner name"))?;
    if cert.bot == cert.owner {
        return Err(forged("own bot"));
    }
    let key = hex::decode(&cert.bot_key)
        .ok()
        .filter(|k| k.len() == KEY_LEN)
        .ok_or_else(|| forged("bot key"))?;
    if bot.is_some_and(|b| b != cert.bot) {
        return Err(forged("another bot"));
    }
    if bot_key.is_some_and(|k| k != key.as_slice()) {
        return Err(forged("another key"));
    }
    let sig = hex::decode(&cert.sig).map_err(|_| forged("signature"))?;
    proofs::verify(
        owner_key,
        &bytes(&cert.bot, &key, &cert.owner, cert.made),
        &sig,
    )
    .map_err(|_| forged("signature"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owner_vouches_and_nothing_else_passes() {
        let owner = Account::generate("julian").unwrap();
        let bot = Account::generate("wetterbot").unwrap();
        let other = Account::generate("mallory").unwrap();
        let (code, hash) = request_code(&bot).unwrap();
        let (cert, secret) = vouch(&owner, &code, 1000).unwrap();
        assert_eq!(secret_hash(&secret), hash);
        assert!(
            !code.contains(&hash),
            "the code carries the secret, not its hash"
        );
        let (code2, hash2) = request_code(&bot).unwrap();
        assert_ne!((&code, &hash), (&code2, &hash2), "every code is new");
        assert_eq!(cert.bot, "wetterbot");
        assert_eq!(cert.owner, "julian");
        verify(
            &cert,
            owner.public_key(),
            Some("wetterbot"),
            Some(bot.public_key()),
        )
        .unwrap();
        verify(&cert, owner.public_key(), None, None).unwrap();

        // Someone else's key, another bot, another key: refused.
        assert!(verify(&cert, other.public_key(), None, None).is_err());
        assert!(verify(&cert, owner.public_key(), Some("mallory"), None).is_err());
        assert!(verify(&cert, owner.public_key(), None, Some(other.public_key())).is_err());
        // Any field changed: refused.
        for change in [
            |c: &mut BotCert| c.bot = "mallory".into(),
            |c: &mut BotCert| c.owner = "mallory".into(),
            |c: &mut BotCert| c.made += 1,
            |c: &mut BotCert| c.bot_key = "00".repeat(32),
        ] {
            let mut c = cert.clone();
            change(&mut c);
            assert!(verify(&c, owner.public_key(), None, None).is_err(), "{c:?}");
        }
        // Not its own bot; codes must be whole.
        assert!(vouch(&owner, &request_code(&owner).unwrap().0, 1).is_err());
        let key = hex::encode(bot.public_key());
        let s = "ab".repeat(SECRET_LEN);
        for bad in [
            "",
            "trairbot1:wetterbot",
            &format!("trairbot1:wetterbot:{key}"),
            &format!("trairbot2:wetterbot:{key}:{s}"),
            &format!("trairbot1:wetter bot:{key}:{s}"),
            &format!("trairbot1:wetterbot:{}:{s}", &key[..62]),
            &format!("trairbot1:wetterbot:{key}:{}", &s[..30]),
            &format!("{code}:mehr"),
        ] {
            assert!(parse_request(bad).is_err(), "{bad:?}");
        }
        // Open and closed messages, logins: never a bot certificate.
        let sig = hex::decode(&cert.sig).unwrap();
        assert!(proofs::verify_account_deletion(owner.public_key(), "julian", 1000, &sig).is_err());
    }
}
