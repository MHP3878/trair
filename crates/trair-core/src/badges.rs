// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Badges ("Abzeichen") on profiles: Trair team, founder, admin and
//! moderator. Decisions of 18.11.2026: they look like Discord badges and
//! are shown everywhere, not only in their community.
//!
//! A badge is never a word in a profile. It is a signed statement:
//!
//! - **Team:** signed by a Trair team key. Only the public team keys are
//!   built into the app ([`TEAM_KEYS`]); the private key stays offline with
//!   the project owner and never enters the repository.
//! - **Founder:** a community's id is derived from the founder's account
//!   key and a random value, so only the founder can sign for it.
//! - **Admin:** signed by the community's founder.
//! - **Moderator:** signed by the founder, or by an admin who attaches
//!   their own admin badge (one step, never longer chains).
//!
//! Every badge names the holder's account key, so it cannot be copied to
//! another profile, and it expires: community roles after at most
//! [`MAX_LIFETIME`] (the server renews them in the background, M3), team
//! badges after at most [`MAX_TEAM_LIFETIME`] (decision of 19.11.2026:
//! the team key stays offline and signs once a year).
//!
//! Limits, said openly: a removed admin keeps the badge until it expires
//! (at most 14 days); a team badge cannot be taken back before it expires
//! except by removing the team key from the app. Community names are chosen by their founders;
//! names that look official get a warning ([`claims_authority`]).

use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;
use openmls_traits::crypto::OpenMlsCrypto;
use openmls_traits::signatures::Signer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::account::{Account, SCHEME};
use crate::error::{CoreError, Result, internal};

/// Public keys (lowercase hex) of the Trair team. Empty until the project
/// owner creates the team key on their own computer; only the public half
/// is ever added here.
pub const TEAM_KEYS: &[&str] = &[];

/// Longest validity of a founder, admin or moderator badge, in seconds
/// (14 days).
pub const MAX_LIFETIME: u64 = 14 * 24 * 3600;
/// Longest validity of a team badge, in seconds (366 days).
pub const MAX_TEAM_LIFETIME: u64 = 366 * 24 * 3600;
/// Clocks may differ by this much (5 minutes).
pub const CLOCK_SKEW: u64 = 5 * 60;
/// Most badges one profile may carry; more are refused.
pub const MAX_BADGES: usize = 16;
/// Longest community name, in characters.
pub const MAX_COMMUNITY_NAME: usize = 40;

const BADGE_CONTEXT: &[u8] = b"trair/badge/v1\0";
const COMMUNITY_CONTEXT: &[u8] = b"trair/community/v1\0";

/// The kinds of badge, in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BadgeKind {
    /// Member of the Trair team.
    Team,
    /// Created a community.
    Founder,
    /// Admin of a community.
    Admin,
    /// Moderator of a community.
    Mod,
}

/// A community as badges name it. The id is derived from the founder's
/// key and a random value, so nobody else can claim to be its founder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Community {
    /// `c-` and 32 hex digits.
    pub id: String,
    /// Display name chosen by the founder.
    pub name: String,
    /// The founder's account key, lowercase hex.
    pub founder: String,
    /// Random value, lowercase hex (16 bytes).
    pub nonce: String,
}

impl Community {
    /// A new community founded by `founder`.
    pub fn create(name: &str, founder: &Account) -> Result<Self> {
        validate_community_name(name)?;
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(internal)?;
        let founder = hex::encode(founder.public_key());
        let nonce = hex::encode(nonce);
        Ok(Community {
            id: community_id(&founder, &nonce),
            name: name.to_string(),
            founder,
            nonce,
        })
    }

    /// The id must match founder and nonce; the name must be sane.
    pub fn check(&self) -> Result<()> {
        validate_community_name(&self.name)?;
        if !is_hex(&self.founder, 32) || !is_hex(&self.nonce, 16) {
            return Err(rejected("community keys are not lowercase hex"));
        }
        if self.id != community_id(&self.founder, &self.nonce) {
            return Err(rejected("community id does not belong to its founder"));
        }
        Ok(())
    }
}

fn community_id(founder_hex: &str, nonce_hex: &str) -> String {
    let mut h = Sha256::new();
    h.update(COMMUNITY_CONTEXT);
    h.update(founder_hex.as_bytes());
    h.update(b"\0");
    h.update(nonce_hex.as_bytes());
    format!("c-{}", hex::encode(&h.finalize()[..16]))
}

/// Longest validity for a kind of badge, in seconds.
pub fn max_lifetime(kind: BadgeKind) -> u64 {
    match kind {
        BadgeKind::Team => MAX_TEAM_LIFETIME,
        BadgeKind::Founder | BadgeKind::Admin | BadgeKind::Mod => MAX_LIFETIME,
    }
}

/// Community names: 1 to 40 characters, no control characters, no
/// spaces at either end.
pub fn validate_community_name(name: &str) -> Result<()> {
    let n = name.chars().count();
    if n == 0 || n > MAX_COMMUNITY_NAME || name.chars().any(char::is_control) || name.trim() != name
    {
        return Err(CoreError::InvalidName("community name".into()));
    }
    Ok(())
}

/// What a badge says. Signed as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BadgeClaim {
    /// Format version, 1.
    pub v: u32,
    /// Which badge.
    pub kind: BadgeKind,
    /// The holder's account key, lowercase hex.
    pub holder: String,
    /// The community for founder, admin and moderator badges.
    pub community: Option<Community>,
    /// Unix time (seconds) when it was signed.
    pub issued: u64,
    /// Unix time (seconds) when it stops being valid.
    pub expires: u64,
    /// The key that signed it, lowercase hex.
    pub issuer: String,
}

impl BadgeClaim {
    fn signed_bytes(&self) -> Vec<u8> {
        let mut b = BADGE_CONTEXT.to_vec();
        // A plain struct of strings and numbers always serializes.
        b.extend_from_slice(&serde_json::to_vec(self).unwrap_or_default());
        b
    }
}

/// A signed badge, as it travels with a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Badge {
    /// What is claimed.
    pub claim: BadgeClaim,
    /// Signature of the issuer, hex.
    pub signature: String,
    /// For a moderator badge signed by an admin: that admin's badge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Box<Badge>>,
}

impl Badge {
    /// Sign a badge for the account key `holder`, valid from `now` for
    /// `lifetime` seconds (at most [`max_lifetime`] of its kind).
    pub fn issue(
        issuer: &Account,
        kind: BadgeKind,
        holder: &[u8],
        community: Option<Community>,
        now: u64,
        lifetime: u64,
        via: Option<Badge>,
    ) -> Result<Badge> {
        if lifetime == 0 || lifetime > max_lifetime(kind) {
            return Err(rejected("badge lifetime"));
        }
        let claim = BadgeClaim {
            v: 1,
            kind,
            holder: hex::encode(holder),
            community,
            issued: now,
            expires: now.saturating_add(lifetime),
            issuer: hex::encode(issuer.public_key()),
        };
        let signature = issuer
            .keys()
            .sign(&claim.signed_bytes())
            .map_err(internal)?;
        Ok(Badge {
            claim,
            signature: hex::encode(signature),
            via: via.map(Box::new),
        })
    }
}

/// A badge that passed every check. This is all the app gets to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifiedBadge {
    /// Which badge.
    pub kind: BadgeKind,
    /// Community name, for founder, admin and moderator.
    pub community: Option<String>,
    /// Community id, for founder, admin and moderator.
    pub community_id: Option<String>,
    /// True if the community name looks official (see [`claims_authority`]).
    pub community_warning: bool,
    /// Unix time when it stops being valid.
    pub expires: u64,
}

/// Check one badge for the profile whose account key is `holder`, at unix
/// time `now`, against the pinned `team_keys`.
pub fn verify(badge: &Badge, holder: &[u8], now: u64, team_keys: &[&str]) -> Result<VerifiedBadge> {
    verify_inner(badge, &hex::encode(holder), now, team_keys, true)
}

fn verify_inner(
    badge: &Badge,
    holder: &str,
    now: u64,
    team_keys: &[&str],
    allow_via: bool,
) -> Result<VerifiedBadge> {
    let c = &badge.claim;
    if c.v != 1 {
        return Err(rejected("unknown badge version"));
    }
    if !is_hex(&c.holder, 32) || !is_hex(&c.issuer, 32) {
        return Err(rejected("badge keys are not lowercase hex"));
    }
    if c.holder != holder {
        return Err(rejected("badge belongs to another account"));
    }
    if c.expires <= c.issued || c.expires - c.issued > max_lifetime(c.kind) {
        return Err(rejected("badge lifetime"));
    }
    if c.issued > now.saturating_add(CLOCK_SKEW) || now >= c.expires {
        return Err(rejected("badge is not valid at this time"));
    }
    let signature =
        hex::decode(&badge.signature).map_err(|_| rejected("badge signature is not hex"))?;
    let issuer = hex::decode(&c.issuer).map_err(|_| rejected("badge issuer is not hex"))?;
    OpenMlsRustCrypto::default()
        .crypto()
        .verify_signature(SCHEME, &c.signed_bytes(), &issuer, &signature)
        .map_err(|_| rejected("badge signature is wrong"))?;

    match c.kind {
        BadgeKind::Team => {
            if c.community.is_some() || badge.via.is_some() {
                return Err(rejected("team badge with extra parts"));
            }
            if !team_keys.contains(&c.issuer.as_str()) {
                return Err(rejected("team badge not signed by a Trair team key"));
            }
        }
        BadgeKind::Founder | BadgeKind::Admin | BadgeKind::Mod => {
            let community = c
                .community
                .as_ref()
                .ok_or_else(|| rejected("badge without community"))?;
            community.check()?;
            let by_founder = c.issuer == community.founder;
            match (c.kind, &badge.via) {
                (BadgeKind::Founder, None) if by_founder && c.holder == community.founder => {}
                (BadgeKind::Admin, None) | (BadgeKind::Mod, None) if by_founder => {}
                (BadgeKind::Mod, Some(admin)) if allow_via && !by_founder => {
                    let same = admin.claim.kind == BadgeKind::Admin
                        && admin.claim.community.as_ref().map(|x| &x.id) == Some(&community.id);
                    if !same {
                        return Err(rejected("moderator badge with a wrong admin badge"));
                    }
                    // The admin badge must be valid itself and belong to
                    // the key that signed this one.
                    verify_inner(admin, &c.issuer, now, team_keys, false)?;
                }
                _ => return Err(rejected("badge not signed by someone allowed to")),
            }
        }
    }
    let community = c.community.as_ref();
    Ok(VerifiedBadge {
        kind: c.kind,
        community: community.map(|x| x.name.clone()),
        community_id: community.map(|x| x.id.clone()),
        community_warning: community.is_some_and(|x| claims_authority(&x.name)),
        expires: c.expires,
    })
}

/// Check all badges of a profile. Returns the valid ones, in display
/// order and without repeats, and how many were refused. A founder badge
/// hides admin and moderator badges of the same community, an admin
/// badge hides the moderator badge.
pub fn verify_all(
    badges: &[Badge],
    holder: &[u8],
    now: u64,
    team_keys: &[&str],
) -> (Vec<VerifiedBadge>, usize) {
    let mut ok: Vec<VerifiedBadge> = Vec::new();
    let mut refused = badges.len().saturating_sub(MAX_BADGES);
    for badge in badges.iter().take(MAX_BADGES) {
        match verify(badge, holder, now, team_keys) {
            Ok(v) => ok.push(v),
            Err(_) => refused += 1,
        }
    }
    ok.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then(a.community_id.cmp(&b.community_id))
    });
    let mut shown: Vec<VerifiedBadge> = Vec::new();
    for v in ok {
        // Same badge twice, or a lower role in a community where a higher
        // one is already shown.
        let covered = shown.iter().any(|s| {
            s.community_id == v.community_id
                && (s.kind == v.kind || (s.community_id.is_some() && s.kind < v.kind))
        });
        if !covered {
            shown.push(v);
        }
    }
    (shown, refused)
}

/// Lowercase, with look-alike letters and digits mapped to plain latin
/// letters, so "TR41R", "trаir" (Cyrillic a) and "T.r.a.i.r" all read
/// as "trair".
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            '0' => 'o',
            '1' | '!' | '|' | 'í' | 'ì' | 'і' | 'ı' => 'i',
            '3' | 'е' | 'é' | 'è' | 'ê' => 'e',
            '4' | '@' | 'а' | 'á' | 'à' | 'â' | 'ä' => 'a',
            '5' | '$' | 'ѕ' => 's',
            '7' | 'т' => 't',
            'о' | 'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'р' => 'p',
            'с' => 'c',
            'х' => 'x',
            'ӏ' => 'l',
            'ü' | 'ú' | 'ù' | 'û' => 'u',
            other => other,
        })
        .collect()
}

const OFFICIAL_WORDS: [&str; 6] = [
    "trair",
    "support",
    "staff",
    "offiziell",
    "official",
    "sicherheitsteam",
];
const ROLE_WORDS: [&str; 6] = [
    "admin",
    "moderator",
    "gruender",
    "grunder",
    "founder",
    "owner",
];

/// True if the text pretends to be official: it contains "Trair",
/// "Support", "Staff" or similar, also when spelled with look-alikes.
pub fn claims_authority(text: &str) -> bool {
    let joined: String = fold(text)
        .chars()
        .filter(char::is_ascii_lowercase)
        .collect();
    OFFICIAL_WORDS.iter().any(|w| joined.contains(w))
}

/// True if a display name should get a warning: it looks official without
/// a team badge, or it names a role (admin, mod, ...) without any role
/// badge.
pub fn name_needs_warning(name: &str, badges: &[VerifiedBadge]) -> bool {
    let team = badges.iter().any(|b| b.kind == BadgeKind::Team);
    let any_role = !badges.is_empty();
    if claims_authority(name) && !team {
        return true;
    }
    let folded = fold(name);
    let joined: String = folded.chars().filter(char::is_ascii_lowercase).collect();
    let words = folded
        .split(|c: char| !c.is_ascii_lowercase())
        .filter(|w| !w.is_empty());
    let names_role = ROLE_WORDS.iter().any(|w| joined.contains(w))
        || words.into_iter().any(|w| w == "mod" || w == "mods");
    names_role && !any_role
}

fn is_hex(text: &str, bytes: usize) -> bool {
    text.len() == bytes * 2
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn rejected(why: &str) -> CoreError {
    CoreError::Rejected(why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A claim signed directly, without the checks of [`Badge::issue`].
    fn signed(issuer: &Account, claim: BadgeClaim) -> Badge {
        let signature = issuer.keys().sign(&claim.signed_bytes()).unwrap();
        Badge {
            claim,
            signature: hex::encode(signature),
            via: None,
        }
    }

    #[test]
    fn too_long_or_reversed_lifetimes_are_refused_even_if_signed() {
        let founder = Account::generate("gruenderin").unwrap();
        let community = Community::create("Trainingsgruppe", &founder).unwrap();
        let holder = hex::encode(founder.public_key());
        let now = 1_800_000_000;
        assert!(
            Badge::issue(
                &founder,
                BadgeKind::Founder,
                founder.public_key(),
                Some(community.clone()),
                now,
                MAX_LIFETIME + 1,
                None
            )
            .is_err()
        );
        for (issued, expires) in [(now, now + MAX_LIFETIME + 1), (now, now), (now + 10, now)] {
            let badge = signed(
                &founder,
                BadgeClaim {
                    v: 1,
                    kind: BadgeKind::Founder,
                    holder: holder.clone(),
                    community: Some(community.clone()),
                    issued,
                    expires,
                    issuer: holder.clone(),
                },
            );
            assert!(
                verify(&badge, founder.public_key(), now + 1, TEAM_KEYS).is_err(),
                "{issued} {expires}"
            );
        }
    }

    #[test]
    fn uppercase_hex_is_refused() {
        let founder = Account::generate("gruenderin").unwrap();
        let community = Community::create("Trainingsgruppe", &founder).unwrap();
        let now = 1_800_000_000;
        let holder = hex::encode(founder.public_key());
        let badge = signed(
            &founder,
            BadgeClaim {
                v: 1,
                kind: BadgeKind::Founder,
                holder: holder.to_uppercase(),
                community: Some(community),
                issued: now,
                expires: now + 60,
                issuer: holder,
            },
        );
        assert!(verify(&badge, founder.public_key(), now, TEAM_KEYS).is_err());
    }
}
