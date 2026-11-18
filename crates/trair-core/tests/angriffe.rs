// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Permanent regression tests: the 23 checks from the MLS prototype
//! (S1a to S12) plus the tests for key package supply (problem 1) and
//! for account certificates. The server is the attacker in most of them.

mod common;

use common::*;
use openmls::prelude::tls_codec::Serialize as _;
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use trair_core::account::SCHEME;
use trair_core::keypackages::{REFILL_BELOW, refill_count};
use trair_core::mls::CIPHERSUITE;
use trair_core::{Account, CoreError, Device, Event, KeyPackageOffer, Trust};

struct World {
    net: Net,
    a: Device,
    b: Device,
    c: Device,
    d: Device,
    acc_b: Account,
    acc_c: Account,
}

/// Alice, Bob and Carol verified each other in person. Dave verified nobody.
/// Like in the prototype, strict mode is switched off here; the strict
/// tests switch it on. (In the app it is on by default, see `strict_mode_is_default`.)
fn world() -> World {
    let mut net = Net::default();
    let (acc_a, mut a) = person("alice", "pc");
    let (acc_b, mut b) = person("bob", "pc");
    let (acc_c, mut c) = person("carol", "pc");
    let (_acc_d, mut d) = person("dave", "pc");
    for dev in [&mut a, &mut b, &mut c, &mut d] {
        net.register(dev, 5);
        dev.set_strict(false);
    }
    a.verify("bob", acc_b.fingerprint());
    a.verify("carol", acc_c.fingerprint());
    b.verify("alice", acc_a.fingerprint());
    b.verify("carol", acc_c.fingerprint());
    c.verify("alice", acc_a.fingerprint());
    c.verify("bob", acc_b.fingerprint());
    World {
        net,
        a,
        b,
        c,
        d,
        acc_b,
        acc_c,
    }
}

/// Group "team" with Alice, Bob and Carol.
fn team(w: &mut World) {
    w.a.create_group("team").unwrap();
    add(&mut w.net, &mut w.a, "team", &["bob", "carol"]).unwrap();
    poll_ok(&mut w.net, &mut w.b);
    poll_ok(&mut w.net, &mut w.c);
}

fn texts(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Message { from, text, .. } => Some((from.clone(), text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn s01a_direct_chat_join_without_warning() {
    let mut w = world();
    w.a.create_group("dm-ab").unwrap();
    let out = add(&mut w.net, &mut w.a, "dm-ab", &["bob"]).unwrap();
    assert!(
        out.added.iter().all(|m| m.trust == Trust::Verified),
        "{:?}",
        out.added
    );
    let ev = poll_ok(&mut w.net, &mut w.b);
    match &ev[0] {
        Event::Joined { group, members, .. } => {
            assert_eq!(group, "dm-ab");
            assert!(members.iter().all(|m| m.trust.is_trusted()), "{members:?}");
        }
        other => panic!("expected join, got {other:?}"),
    }
}

#[test]
fn s01b_direct_chat_message_with_proven_sender() {
    let mut w = world();
    w.a.create_group("dm-ab").unwrap();
    add(&mut w.net, &mut w.a, "dm-ab", &["bob"]).unwrap();
    poll_ok(&mut w.net, &mut w.b);
    send(&mut w.net, &mut w.a, "dm-ab", "Geheimtreff Bahnhof privat").unwrap();
    let ev = poll_ok(&mut w.net, &mut w.b);
    assert_eq!(
        texts(&ev),
        vec![("alice:pc".into(), "Geheimtreff Bahnhof privat".into())]
    );
}

#[test]
fn s01c_group_members_read_everyone_in_order() {
    let mut w = world();
    team(&mut w);
    send(&mut w.net, &mut w.a, "team", "Gruppenplan Alpha").unwrap();
    poll_ok(&mut w.net, &mut w.b);
    send(&mut w.net, &mut w.b, "team", "Antwort von Bob").unwrap();
    let ev = poll_ok(&mut w.net, &mut w.c);
    assert_eq!(
        texts(&ev),
        vec![
            ("alice:pc".into(), "Gruppenplan Alpha".into()),
            ("bob:pc".into(), "Antwort von Bob".into())
        ]
    );
}

#[test]
fn s11a_server_never_sees_plaintext() {
    let mut w = world();
    team(&mut w);
    send(
        &mut w.net,
        &mut w.a,
        "team",
        "Geheimtreff Bahnhof Gruppenplan Alpha",
    )
    .unwrap();
    send(&mut w.net, &mut w.b, "team", "Antwort von Bob").unwrap();
    let all: Vec<u8> = w.net.log.concat();
    for word in ["Geheimtreff", "Bahnhof", "Gruppenplan", "Alpha", "Antwort"] {
        assert!(
            !all.windows(word.len()).any(|win| win == word.as_bytes()),
            "server saw '{word}'"
        );
    }
}

#[test]
fn s02_tampered_message_is_rejected() {
    let mut w = world();
    team(&mut w);
    let mut bytes = send(&mut w.net, &mut w.b, "team", "Antwort von Bob").unwrap();
    let n = bytes.len();
    bytes[n - 6] ^= 1;
    w.net.inbox("carol:pc");
    w.net.inject("carol:pc", &bytes);
    let r = poll(&mut w.net, &mut w.c);
    assert!(matches!(r[0], Err(CoreError::Rejected(_))), "{r:?}");
}

#[test]
fn s03_replayed_message_is_rejected() {
    let mut w = world();
    team(&mut w);
    let bytes = send(&mut w.net, &mut w.b, "team", "einmal").unwrap();
    assert_eq!(texts(&poll_ok(&mut w.net, &mut w.c)).len(), 1);
    w.net.inject("carol:pc", &bytes);
    let r = poll(&mut w.net, &mut w.c);
    assert!(matches!(r[0], Err(CoreError::Rejected(_))), "{r:?}");
}

#[test]
fn s04_forged_add_commit_is_rejected() {
    let mut w = world();
    team(&mut w);
    // Mallory builds her own group with the same id and adds her tablet.
    let (_acc_m, mut m) = person("mallory", "pc");
    let acc_m2 = Account::generate("mallory").unwrap();
    let mut m2 = Device::new(&acc_m2, "tab").unwrap();
    w.net.register(&mut m, 5);
    w.net.register(&mut m2, 5);
    m.set_strict(false);
    m.create_group("team").unwrap();
    w.net.set_honest(false);
    let fake = add(&mut w.net, &mut m, "team", &["mallory"]).unwrap();
    // The operator delivers it to Bob and Carol "from Alice".
    w.net.inject("bob:pc", &fake.commit);
    w.net.inject("carol:pc", &fake.commit);
    let rb = poll(&mut w.net, &mut w.b);
    let rc = poll(&mut w.net, &mut w.c);
    assert!(matches!(rb[0], Err(CoreError::Rejected(_))), "{rb:?}");
    assert!(matches!(rc[0], Err(CoreError::Rejected(_))), "{rc:?}");
    assert_eq!(
        member_ids(&w.b, "team"),
        vec!["alice:pc", "bob:pc", "carol:pc"]
    );
}

#[test]
fn s07_forged_invitation_is_detected() {
    let mut w = world();
    team(&mut w);
    let (_acc_m, mut m) = person("mallory", "pc");
    w.net.register(&mut m, 5);
    m.set_strict(false);
    // The operator helps: it accepts a second group under the same id.
    w.net.set_honest(false);
    // Invitation to a group with the same id as Bob's real team.
    m.create_group("team").unwrap();
    let inv = add(&mut w.net, &mut m, "team", &["bob"]).unwrap();
    let r = poll(&mut w.net, &mut w.b);
    assert!(matches!(r[0], Err(CoreError::GroupExists(_))), "{r:?}");
    assert_eq!(
        member_ids(&w.b, "team"),
        vec!["alice:pc", "bob:pc", "carol:pc"]
    );
    drop(inv);
    // Invitation to a new group that pretends to be Alice's: Bob sees who
    // is really in it, and in strict mode he cannot write there.
    m.create_group("team-alice").unwrap();
    add(&mut w.net, &mut m, "team-alice", &["bob"]).unwrap();
    let ev = poll_ok(&mut w.net, &mut w.b);
    match &ev[0] {
        Event::Joined { members, .. } => {
            assert!(
                members
                    .iter()
                    .any(|x| x.id == "mallory:pc" && x.trust == Trust::Unverified)
            );
            assert!(!members.iter().any(|x| x.id == "alice:pc"));
        }
        other => panic!("{other:?}"),
    }
    w.b.set_strict(true);
    assert!(matches!(
        w.b.encrypt("team-alice", "hi"),
        Err(CoreError::Blocked(_))
    ));
}

/// The operator secretly registers a "bob" device signed by its own key.
fn ghost(w: &mut World) -> Device {
    let fake_bob = Account::generate("bob").unwrap();
    let mut g = Device::new(&fake_bob, "ghost").unwrap();
    w.net.register(&mut g, 5);
    g
}

#[test]
fn s05a_ghost_device_gets_nothing_from_existing_groups() {
    let mut w = world();
    team(&mut w);
    let mut g = ghost(&mut w);
    let bytes = send(&mut w.net, &mut w.a, "team", "Nach dem Geist").unwrap();
    assert!(
        poll(&mut w.net, &mut g).is_empty(),
        "nobody may send to the ghost"
    );
    // Even if the operator forwards the message, the ghost cannot read it.
    w.net.inject("bob:ghost", &bytes);
    let r = poll(&mut w.net, &mut g);
    assert!(r[0].is_err(), "{r:?}");
}

#[test]
fn s05b_ghost_device_reported_when_adding() {
    let mut w = world();
    let _g = ghost(&mut w);
    w.a.create_group("team2").unwrap();
    let out = add(&mut w.net, &mut w.a, "team2", &["bob"]).unwrap();
    let gm = out
        .added
        .iter()
        .find(|m| m.id == "bob:ghost")
        .expect("ghost offered");
    assert_eq!(
        gm.trust,
        Trust::Mismatch,
        "Alice verified Bob's account; the ghost is not signed by it"
    );
}

#[test]
fn s05c_bob_sees_the_foreign_device() {
    let mut w = world();
    let _g = ghost(&mut w);
    w.a.create_group("team2").unwrap();
    add(&mut w.net, &mut w.a, "team2", &["bob"]).unwrap();
    let ev = poll_ok(&mut w.net, &mut w.b);
    match &ev[0] {
        Event::Joined { members, .. } => {
            let gm = members
                .iter()
                .find(|m| m.id == "bob:ghost")
                .expect("ghost listed");
            assert_eq!(
                gm.trust,
                Trust::Mismatch,
                "a 'bob' device not signed by Bob's account"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn s05d_strict_mode_blocks_adding_the_ghost() {
    let mut w = world();
    let _g = ghost(&mut w);
    w.a.set_strict(true);
    w.a.create_group("team3").unwrap();
    let r = add(&mut w.net, &mut w.a, "team3", &["bob"]);
    assert!(
        matches!(r, Err(StepError::Core(CoreError::Blocked(_)))),
        "{r:?}"
    );
}

#[test]
fn s05e_strict_mode_blocks_sending_with_the_ghost_present() {
    let mut w = world();
    let _g = ghost(&mut w);
    w.a.create_group("team2").unwrap();
    add(&mut w.net, &mut w.a, "team2", &["bob"]).unwrap();
    w.a.set_strict(true);
    let r = send(&mut w.net, &mut w.a, "team2", "x");
    assert!(
        matches!(r, Err(StepError::Core(CoreError::Blocked(_)))),
        "{r:?}"
    );
}

/// The operator makes its own "carol" account and hands out its key
/// package whenever someone asks for carol:pc.
fn swap_carol(w: &mut World) -> Device {
    let fake = Account::generate("carol").unwrap();
    let mut x = Device::new(&fake, "pc").unwrap();
    w.net.register_as("evil:x", &mut x, 5);
    w.net.swap("carol:pc", "evil:x");
    x
}

#[test]
fn s06a_swapped_key_of_verified_contact_is_detected() {
    let mut w = world();
    let _x = swap_carol(&mut w);
    w.a.create_group("g6").unwrap();
    let out = add(&mut w.net, &mut w.a, "g6", &["carol"]).unwrap();
    assert_eq!(out.added[0].id, "carol:pc");
    assert_eq!(out.added[0].trust, Trust::Mismatch);
    // With strict mode on, the swap never gets into a group.
    w.a.set_strict(true);
    w.a.create_group("g6s").unwrap();
    let r = add(&mut w.net, &mut w.a, "g6s", &["carol"]);
    assert!(
        matches!(r, Err(StepError::Core(CoreError::Blocked(_)))),
        "{r:?}"
    );
}

#[test]
fn s06b_without_verification_attack_works_but_is_flagged() {
    let mut w = world();
    let mut x = swap_carol(&mut w);
    w.d.create_group("g6d").unwrap();
    let out = add(&mut w.net, &mut w.d, "g6d", &["carol"]).unwrap();
    assert_eq!(
        out.added[0].trust,
        Trust::Unverified,
        "Dave never verified Carol"
    );
    let ev = poll_ok(&mut w.net, &mut x);
    assert!(matches!(&ev[0], Event::Joined { group, .. } if group == "g6d"));
    // Rest risk, as in the prototype: whoever ignores the warning is fooled.
    let _ = &w.acc_c;
}

#[test]
fn s08a_removal_is_shown_with_its_author() {
    let mut w = world();
    team(&mut w);
    remove(&mut w.net, &mut w.a, "team", "carol").unwrap();
    let ev = poll_ok(&mut w.net, &mut w.b);
    match &ev[0] {
        Event::Commit { by, removed, .. } => {
            assert_eq!(by, "alice:pc");
            assert_eq!(removed, &vec!["carol:pc".to_string()]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn s08b_removed_member_notices() {
    let mut w = world();
    team(&mut w);
    remove(&mut w.net, &mut w.a, "team", "carol").unwrap();
    let ev = poll_ok(&mut w.net, &mut w.c);
    assert!(
        matches!(
            &ev[0],
            Event::Commit {
                i_was_removed: true,
                ..
            }
        ),
        "{ev:?}"
    );
}

#[test]
fn s08c_removed_member_cannot_read_new_messages() {
    let mut w = world();
    team(&mut w);
    remove(&mut w.net, &mut w.a, "team", "carol").unwrap();
    poll_ok(&mut w.net, &mut w.c);
    poll_ok(&mut w.net, &mut w.b);
    let bytes = send(&mut w.net, &mut w.a, "team", "Ohne Carol").unwrap();
    w.net.inject("carol:pc", &bytes);
    let r = poll(&mut w.net, &mut w.c);
    assert!(r[0].is_err(), "{r:?}");
}

#[test]
fn s12_new_member_cannot_read_old_messages() {
    let mut w = world();
    team(&mut w);
    let old = send(&mut w.net, &mut w.a, "team", "vor Dave").unwrap();
    poll_ok(&mut w.net, &mut w.b);
    poll_ok(&mut w.net, &mut w.c);
    add(&mut w.net, &mut w.a, "team", &["dave"]).unwrap();
    poll_ok(&mut w.net, &mut w.d);
    w.net.inject("dave:pc", &old);
    let r = poll(&mut w.net, &mut w.d);
    assert!(r[0].is_err(), "{r:?}");
}

#[test]
fn s09_withheld_messages_are_detected() {
    let mut w = world();
    team(&mut w);
    w.net.drop_between("alice:pc", "bob:pc");
    send(&mut w.net, &mut w.a, "team", "weg1").unwrap();
    send(&mut w.net, &mut w.a, "team", "weg2").unwrap();
    w.net.clear_drops();
    send(&mut w.net, &mut w.a, "team", "sichtbar").unwrap();
    let ev = poll_ok(&mut w.net, &mut w.b);
    match ev.last() {
        Some(Event::Message { text, missing, .. }) => {
            assert_eq!(text, "sichtbar");
            assert_eq!(*missing, 2);
        }
        other => panic!("{other:?}"),
    }
}

/// Group "fork" with Alice, Bob, Carol and Dave; Erin and Frank registered.
fn fork_group(w: &mut World, group: &str) -> (Device, Device) {
    let (_ae, mut e) = person("erin", "pc");
    let (_af, mut f) = person("frank", "pc");
    w.net.register(&mut e, 5);
    w.net.register(&mut f, 5);
    w.a.create_group(group).unwrap();
    add(&mut w.net, &mut w.a, group, &["bob", "carol", "dave"]).unwrap();
    for dev in [&mut w.b, &mut w.c, &mut w.d] {
        poll_ok(&mut w.net, dev);
    }
    (e, f)
}

#[test]
fn s10a_honest_server_refuses_two_commits_for_one_epoch() {
    let mut w = world();
    let _ef = fork_group(&mut w, "fork");
    w.net.drop_between("alice:pc", "bob:pc");
    add(&mut w.net, &mut w.a, "fork", &["erin"]).unwrap();
    let r = add(&mut w.net, &mut w.b, "fork", &["frank"]);
    assert!(
        matches!(&r, Err(StepError::Server(e)) if e.contains("409")),
        "{r:?}"
    );
}

#[test]
fn s10b_s10c_split_group_is_detected() {
    let mut w = world();
    let _ef = fork_group(&mut w, "fork2");
    w.net.set_honest(false);
    for (f, t) in [
        ("alice:pc", "bob:pc"),
        ("alice:pc", "dave:pc"),
        ("bob:pc", "alice:pc"),
        ("bob:pc", "carol:pc"),
    ] {
        w.net.drop_between(f, t);
    }
    add(&mut w.net, &mut w.a, "fork2", &["erin"]).unwrap(); // Carol follows Alice
    add(&mut w.net, &mut w.b, "fork2", &["frank"]).unwrap(); // Dave follows Bob
    poll_ok(&mut w.net, &mut w.c);
    poll_ok(&mut w.net, &mut w.d);
    w.net.clear_drops();
    // S10c: same epoch number, different group fingerprints.
    assert_eq!(w.c.epoch("fork2").unwrap(), w.d.epoch("fork2").unwrap());
    assert_ne!(
        w.c.epoch_fingerprint("fork2").unwrap(),
        w.d.epoch_fingerprint("fork2").unwrap()
    );
    // S10b: a member in the other branch cannot read.
    send(&mut w.net, &mut w.a, "fork2", "Fork-Test").unwrap();
    let ec = poll_ok(&mut w.net, &mut w.c);
    assert_eq!(texts(&ec), vec![("alice:pc".into(), "Fork-Test".into())]);
    let rd = poll(&mut w.net, &mut w.d);
    assert!(rd[0].is_err(), "{rd:?}");
}

// ---------------------------------------------------------------------
// Problem 1: key package supply
// ---------------------------------------------------------------------

#[test]
fn kp1_fifty_groups_with_refill() {
    let mut net = Net::default();
    let (_aa, mut a) = person("alice", "pc");
    let (_ab, mut b) = person("bob", "pc");
    net.register(&mut a, 5);
    net.register(&mut b, 5);
    a.set_strict(false);
    for i in 0..50 {
        let gid = format!("g{i}");
        a.create_group(&gid).unwrap();
        add(&mut net, &mut a, &gid, &["bob"]).unwrap();
        let ev = poll_ok(&mut net, &mut b);
        assert!(matches!(&ev[0], Event::Joined { group, .. } if *group == gid));
        let need = refill_count(net.available("bob:pc"));
        if need > 0 {
            net.publish("bob:pc", &b, need);
        }
        assert!(net.available("bob:pc") >= REFILL_BELOW);
    }
}

#[test]
fn kp2_last_resort_keeps_working_and_triggers_key_update() {
    let mut net = Net::default();
    let (_aa, mut a) = person("alice", "pc");
    let (_ab, mut b) = person("bob", "pc");
    net.register(&mut a, 5);
    net.register(&mut b, 1); // only one one-time package, then the reserve
    a.set_strict(false);
    b.set_strict(false);
    let mut via_reserve = 0;
    for i in 0..50 {
        let gid = format!("r{i}");
        a.create_group(&gid).unwrap();
        add(&mut net, &mut a, &gid, &["bob"]).unwrap();
        let ev = poll_ok(&mut net, &mut b);
        let Event::Joined {
            via_last_resort, ..
        } = &ev[0]
        else {
            panic!("{ev:?}")
        };
        if *via_last_resort {
            via_reserve += 1;
            // Replace the keys of the reused package right away.
            update(&mut net, &mut b, &gid).unwrap();
            poll_ok(&mut net, &mut a);
        }
    }
    assert_eq!(via_reserve, 49, "the first join used the one-time package");
    // The group still works after the key update.
    send(&mut net, &mut a, "r49", "noch da").unwrap();
    assert_eq!(texts(&poll_ok(&mut net, &mut b)).len(), 1);
}

// ---------------------------------------------------------------------
// Accounts and certificates (decision 2: no recovery, devices signed by
// the account key) and strict mode (decision 6: on by default)
// ---------------------------------------------------------------------

#[test]
fn strict_mode_is_default() {
    let (_a, dev) = person("alice", "pc");
    assert!(dev.policy().strict());
}

#[test]
fn own_second_device_is_trusted_without_verification() {
    let mut net = Net::default();
    let acc = Account::generate("alice").unwrap();
    let mut pc = Device::new(&acc, "pc").unwrap();
    let mut tab = Device::new(&acc, "tab").unwrap();
    net.register(&mut pc, 5);
    net.register(&mut tab, 5);
    // Strict mode stays on.
    pc.create_group("notes").unwrap();
    let out = add(&mut net, &mut pc, "notes", &["alice"]).unwrap();
    assert_eq!(out.added[0].id, "alice:tab");
    assert_eq!(out.added[0].trust, Trust::OwnAccount);
    poll_ok(&mut net, &mut tab);
    send(&mut net, &mut tab, "notes", "vom Tablet").unwrap();
    assert_eq!(texts(&poll_ok(&mut net, &mut pc)).len(), 1);
}

/// A key package whose certificate was copied from another device.
fn forged_key_package() -> Vec<u8> {
    key_package_with(CIPHERSUITE, true)
}

/// A correctly certified Bob key package in any cipher suite, or one
/// whose certificate belongs to another device key (`steal`).
fn key_package_with(suite: Ciphersuite, steal: bool) -> Vec<u8> {
    let provider = OpenMlsRustCrypto::default();
    let acc = Account::generate("bob").unwrap();
    let real = SignatureKeyPair::new(SCHEME).unwrap();
    let thief = SignatureKeyPair::new(SCHEME).unwrap();
    let signer = if steal { &thief } else { &real };
    let cert = acc.certify_device("pc", real.public()).unwrap();
    let cred = CredentialWithKey {
        credential: BasicCredential::new(cert.to_bytes()).into(),
        signature_key: signer.to_public_vec().into(),
    };
    let bundle = KeyPackage::builder()
        .build(suite, &provider, signer, cred)
        .unwrap();
    bundle.key_package().tls_serialize_detached().unwrap()
}

#[test]
fn other_cipher_suites_are_refused() {
    let mut w = world();
    w.a.create_group("x").unwrap();
    let offer = KeyPackageOffer {
        claimed_device: "bob:pc".into(),
        bytes: key_package_with(
            Ciphersuite::MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519,
            false,
        ),
    };
    let r = w.a.add_members("x", &[offer]);
    assert!(
        matches!(r, Err(CoreError::InvalidKeyPackage(ref e)) if e.contains("cipher suite")),
        "{r:?}"
    );
}

#[test]
fn forged_certificate_is_always_refused() {
    let mut w = world(); // strict mode off
    w.a.create_group("x").unwrap();
    let offer = KeyPackageOffer {
        claimed_device: "bob:pc".into(),
        bytes: forged_key_package(),
    };
    let r = w.a.add_members("x", &[offer]);
    assert!(matches!(r, Err(CoreError::Forged(_))), "{r:?}");
    let _ = &w.acc_b;
}

#[test]
fn server_claim_must_match_the_certificate() {
    let mut w = world();
    w.a.create_group("x").unwrap();
    let mut offers = w.net.fetch("bob");
    offers[0].claimed_device = "carol:pc".into();
    let r = w.a.add_members("x", &offers);
    assert!(matches!(r, Err(CoreError::InvalidKeyPackage(_))), "{r:?}");
}
