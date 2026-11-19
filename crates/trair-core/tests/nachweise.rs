// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Proofs for the server: login by device key, account deletion.

use trair_core::proofs::{
    CHALLENGE_LEN, sign_account_deletion, sign_login, verify_account_deletion, verify_login,
};
use trair_core::{Account, DeviceIdentity};

fn challenge() -> Vec<u8> {
    let mut c = vec![0u8; CHALLENGE_LEN];
    getrandom::fill(&mut c).expect("random");
    c
}

#[test]
fn n01_login_proof_is_bound_to_device_user_and_challenge() {
    let device = DeviceIdentity::generate().unwrap();
    let other = DeviceIdentity::generate().unwrap();
    let c = challenge();
    let sig = sign_login(&device, "local-abc", &c).unwrap();
    assert!(verify_login(&device.entry(), "local-abc", &c, &sig).is_ok());
    assert!(
        verify_login(&other.entry(), "local-abc", &c, &sig).is_err(),
        "other device"
    );
    assert!(
        verify_login(&device.entry(), "local-xyz", &c, &sig).is_err(),
        "other user"
    );
    assert!(
        verify_login(&device.entry(), "local-abc", &challenge(), &sig).is_err(),
        "other challenge"
    );
    let mut renamed = device.entry();
    renamed.id = "d-00000000".into();
    assert!(
        verify_login(&renamed, "local-abc", &c, &sig).is_err(),
        "other device id"
    );
    assert!(
        sign_login(&device, "local-abc", &[1, 2, 3]).is_err(),
        "short challenge"
    );
}

#[test]
fn n02_deletion_proof_needs_the_account_key_and_the_time() {
    let account = Account::generate("local-abc").unwrap();
    let stranger = Account::generate("local-abc").unwrap();
    let now = 1_800_000_000;
    let sig = sign_account_deletion(&account, now).unwrap();
    assert!(verify_account_deletion(account.public_key(), "local-abc", now, &sig).is_ok());
    assert!(verify_account_deletion(account.public_key(), "local-abc", now + 1, &sig).is_err());
    assert!(verify_account_deletion(account.public_key(), "local-xyz", now, &sig).is_err());
    assert!(verify_account_deletion(stranger.public_key(), "local-abc", now, &sig).is_err());
}
