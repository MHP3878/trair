// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The team key file and the own badges in the vault.

use serde_json::json;
use trair_core::Account;
use trair_core::badges::{Badge, BadgeKind, Community, MAX_LIFETIME, verify};
use trair_core::teamkey::{TeamKey, check_passphrase};
use trair_core::vault::{KdfParams, Limits, Vault, VaultError};

const CHEAP: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};
const TEST_LIMITS: Limits = Limits {
    min: CHEAP,
    max: Limits::PRODUCTION.max,
};
const NOW: u64 = 1_800_000_000;
const DAY: u64 = 24 * 3600;

/// Random passphrases: nothing hard-coded flows into the crypto.
fn secret(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).expect("random");
    bytes.iter().map(|b| char::from(b'a' + b % 26)).collect()
}

#[test]
fn t01_team_key_survives_sealing() {
    let pass = secret(24);
    let key = TeamKey::generate().unwrap();
    let file = key.seal_with(&pass, CHEAP).unwrap();
    let again = TeamKey::open_with_limits(&file, &pass, TEST_LIMITS).unwrap();
    assert_eq!(again.public_key_hex(), key.public_key_hex());
    assert_eq!(again.fingerprint(), key.fingerprint());
    assert_eq!(key.public_key_hex().len(), 64);
    // Nothing secret in the debug output or in the clear part of the file.
    assert_eq!(
        format!("{key:?}"),
        format!("TeamKey({})", key.public_key_hex())
    );
    assert!(!String::from_utf8_lossy(&file).contains("trair-team"));
}

#[test]
fn t02_wrong_passphrase_changed_file_and_cheap_settings_are_refused() {
    let pass = secret(24);
    let key = TeamKey::generate().unwrap();
    let file = key.seal_with(&pass, CHEAP).unwrap();
    assert_eq!(
        TeamKey::open_with_limits(&file, &secret(24), TEST_LIMITS).unwrap_err(),
        VaultError::WrongPassword
    );
    for i in [9, 30, file.len() - 1] {
        let mut bad = file.clone();
        bad[i] ^= 1;
        assert!(
            TeamKey::open_with_limits(&bad, &pass, TEST_LIMITS).is_err(),
            "byte {i}"
        );
    }
    // The real limits refuse a file with cheap settings: nobody can make
    // guessing the passphrase easier by swapping the header.
    assert!(matches!(
        TeamKey::open(&file, &pass),
        Err(VaultError::Refused(_))
    ));
}

#[test]
fn t03_short_or_lazy_passphrases_are_refused() {
    for bad in ["kurz", "fünfzehn zeich", "aaaaaaaaaaaaaaaaaaaaaa", ""] {
        assert_eq!(
            check_passphrase(bad),
            Err(VaultError::WeakPassword),
            "{bad:?}"
        );
    }
    assert!(check_passphrase(&secret(16)).is_ok());
}

#[test]
fn t04_team_and_vault_files_are_not_mixed_up() {
    let pass = secret(24);
    let team = TeamKey::generate()
        .unwrap()
        .seal_with(&pass, CHEAP)
        .unwrap();
    let (_, vault) = Vault::create_with(&pass, json!({}), CHEAP).unwrap();
    assert!(matches!(
        Vault::open_with_limits(&team, &pass, TEST_LIMITS),
        Err(VaultError::Damaged(_))
    ));
    assert!(matches!(
        TeamKey::open_with_limits(&vault, &pass, TEST_LIMITS),
        Err(VaultError::Damaged(_))
    ));
}

#[test]
fn t05_team_badge_from_the_team_key() {
    let key = TeamKey::generate().unwrap();
    let pinned_key = key.public_key_hex();
    let pinned = [pinned_key.as_str()];
    let julian = Account::generate("julian").unwrap();
    let badge = key.sign_badge(julian.public_key(), NOW, 365).unwrap();
    let v = verify(&badge, julian.public_key(), NOW + 300 * DAY, &pinned).unwrap();
    assert_eq!(v.kind, BadgeKind::Team);
    assert!(
        key.sign_badge(julian.public_key(), NOW, 367).is_err(),
        "more than a year"
    );
    assert!(key.sign_badge(julian.public_key(), NOW, 0).is_err());
    assert!(
        key.sign_badge(&[1, 2, 3], NOW, 30).is_err(),
        "not an account key"
    );
}

#[test]
fn t06_own_badges_are_checked_and_kept_in_the_vault() {
    let pass = secret(24);
    let (mut vault, _) = Vault::create_with(&pass, json!({}), CHEAP).unwrap();
    let founder = Account::generate("gruenderin").unwrap();
    let community = Community::create("Trair Beta", &founder).unwrap();
    let holder = vault.account().public_key().to_vec();
    let admin = |now: u64| {
        Badge::issue(
            &founder,
            BadgeKind::Admin,
            &holder,
            Some(community.clone()),
            now,
            7 * DAY,
            None,
        )
        .unwrap()
    };
    let first = admin(NOW);
    vault.add_badge(first, NOW, &[]).unwrap();
    // A renewed badge replaces the old one instead of piling up.
    let renewed = admin(NOW + DAY);
    vault.add_badge(renewed.clone(), NOW + DAY, &[]).unwrap();
    assert_eq!(vault.badges(), std::slice::from_ref(&renewed));

    // Someone else's badge, and a team badge without a pinned team key.
    let other = Account::generate("fremd").unwrap();
    let foreign = Badge::issue(
        &founder,
        BadgeKind::Admin,
        other.public_key(),
        Some(community.clone()),
        NOW,
        DAY,
        None,
    )
    .unwrap();
    assert!(matches!(
        vault.add_badge(foreign, NOW, &[]),
        Err(VaultError::Refused(_))
    ));
    let team = TeamKey::generate()
        .unwrap()
        .sign_badge(vault.account().public_key(), NOW, 365)
        .unwrap();
    assert!(vault.add_badge(team, NOW, &[]).is_err());

    // Stored with the vault, and only valid ones count.
    let file = vault.seal().unwrap();
    let again = Vault::open_with_limits(&file, &pass, TEST_LIMITS).unwrap();
    assert_eq!(again.badges().len(), 1);
    assert_eq!(again.verified_badges(NOW + DAY, &[]).len(), 1);
    assert!(
        again
            .verified_badges(NOW + DAY + MAX_LIFETIME, &[])
            .is_empty(),
        "expired"
    );
}
