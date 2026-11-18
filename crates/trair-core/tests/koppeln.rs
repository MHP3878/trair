// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! M2: pairing a second device with a short code and SPAKE2. The relay in
//! these tests stands for the server and is sometimes hostile.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use serde_json::json;
use trair_core::pairing::{Code, LIFETIME, PairingGuest, PairingHost, WORDS};
use trair_core::vault::{KdfParams, Limits, Vault};
use trair_core::{Device, Trust};

static PW: LazyLock<String> = LazyLock::new(|| secret(24));
const CHEAP: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};
const TEST_LIMITS: Limits = Limits {
    min: CHEAP,
    max: Limits::PRODUCTION.max,
};

fn secret(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).expect("random");
    bytes.iter().map(|b| char::from(b'a' + b % 26)).collect()
}

fn host_vault() -> Vault {
    Vault::create_with(&PW, json!({"name": "Julian", "glow": 1}), CHEAP)
        .expect("create")
        .0
}

/// A full honest run. Returns the host vault and the new device's vault.
fn pair(host: &mut Vault) -> Vault {
    let now = Instant::now();
    let (mut h, code, host_hello) = PairingHost::start(7, now).unwrap();
    let typed = Code::parse(&code.text()).unwrap();
    let (mut g, guest_hello) = PairingGuest::start(&typed).unwrap();
    let offer = g.offer(&host_hello).unwrap();
    let bundle = h.finish(&guest_hello, &offer, host, now).unwrap();
    let joined = g.finish(&bundle).unwrap();
    Vault::create_joined_with(&secret(20), joined, CHEAP)
        .unwrap()
        .0
}

#[test]
fn k01_honest_pairing_gives_the_new_device_the_account() {
    let mut host = host_vault();
    let guest = pair(&mut host);
    assert_eq!(guest.account().fingerprint(), host.account().fingerprint());
    assert_eq!(guest.account().user(), host.account().user());
    assert_eq!(guest.profile()["name"], "Julian");
    assert_ne!(guest.device().id, host.device().id);
    // Both have the same list, now with two devices, signed by the account.
    let list = host
        .device_list()
        .verify(host.account().public_key(), None)
        .unwrap();
    assert_eq!(list.seq, 2);
    assert!(list.devices.contains(&guest.device().entry()));
    assert_eq!(guest.device_list(), host.device_list());
    // In MLS both see each other as their own account's devices.
    let mut a = Device::from_identity(host.account(), host.device()).unwrap();
    let mut b = Device::from_identity(guest.account(), guest.device()).unwrap();
    a.create_group("eigene").unwrap();
    let kp = b.key_packages(1).unwrap().remove(0);
    let offer = trair_core::KeyPackageOffer {
        claimed_device: b.id().to_string(),
        bytes: kp,
    };
    let out = a.add_members("eigene", &[offer]).unwrap();
    a.confirm("eigene").unwrap();
    b.receive(out.welcome.as_ref().unwrap()).unwrap();
    let trust = a
        .members("eigene")
        .unwrap()
        .into_iter()
        .find(|m| m.id == b.id())
        .unwrap()
        .trust;
    assert_eq!(trust, Trust::OwnAccount);
    let msg = b.encrypt("eigene", "Hallo von Gerät 2").unwrap();
    assert!(matches!(
        a.receive(&msg).unwrap(),
        trair_core::Event::Message { .. }
    ));
}

#[test]
fn k02_wrong_code_gives_nothing_and_uses_up_the_code() {
    let mut host = host_vault();
    let now = Instant::now();
    let (mut h, code, host_hello) = PairingHost::start(7, now).unwrap();
    // One word different.
    let mut words: Vec<String> = code.text().split('-').map(String::from).collect();
    words[3] = WORDS.iter().find(|w| **w != words[3]).unwrap().to_string();
    let wrong = Code::parse(&words.join("-")).unwrap();
    let (mut g, guest_hello) = PairingGuest::start(&wrong).unwrap();
    let offer = g.offer(&host_hello).unwrap();
    assert!(h.finish(&guest_hello, &offer, &mut host, now).is_err());
    assert_eq!(host.device_list().list.seq, 1, "no device was added");
    // The right code does not help any more: one guess per code.
    let (mut g2, guest_hello2) = PairingGuest::start(&code).unwrap();
    let offer2 = g2.offer(&host_hello).unwrap();
    assert!(h.finish(&guest_hello2, &offer2, &mut host, now).is_err());
    assert_eq!(host.device_list().list.seq, 1);
}

#[test]
fn k03_relay_in_the_middle_learns_nothing_and_breaks_the_run() {
    let mut host = host_vault();
    let now = Instant::now();
    let (mut h, code, host_hello) = PairingHost::start(7, now).unwrap();
    let (mut g, guest_hello) = PairingGuest::start(&code).unwrap();
    // The relay talks to the host itself, with a guessed code.
    let guess = Code::random(code.channel()).unwrap();
    let (mut evil, evil_hello) = PairingGuest::start(&guess).unwrap();
    let evil_offer = evil.offer(&host_hello).unwrap();
    assert!(h.finish(&evil_hello, &evil_offer, &mut host, now).is_err());
    // The real guest's run is now dead too.
    let offer = g.offer(&host_hello).unwrap();
    assert!(h.finish(&guest_hello, &offer, &mut host, now).is_err());
    assert_eq!(host.device_list().list.seq, 1);
    // Nothing readable went over the relay.
    for msg in [&host_hello, &guest_hello, &offer] {
        let text = String::from_utf8_lossy(msg);
        assert!(!text.contains("Julian") && !text.contains(host.account().user()));
    }
}

#[test]
fn k04_relay_swaps_the_hellos_of_two_runs() {
    let mut host = host_vault();
    let now = Instant::now();
    let (mut h, code, _host_hello) = PairingHost::start(7, now).unwrap();
    let (_h2, _code2, other_host_hello) = PairingHost::start(8, now).unwrap();
    let (mut g, guest_hello) = PairingGuest::start(&code).unwrap();
    // The guest gets the hello of another run.
    let offer = g.offer(&other_host_hello).unwrap();
    assert!(h.finish(&guest_hello, &offer, &mut host, now).is_err());
}

#[test]
fn k05_changed_messages_are_refused() {
    let now = Instant::now();
    for target in ["offer", "bundle", "guest_hello"] {
        let mut host = host_vault();
        let (mut h, code, host_hello) = PairingHost::start(7, now).unwrap();
        let (mut g, mut guest_hello) = PairingGuest::start(&code).unwrap();
        let mut offer = g.offer(&host_hello).unwrap();
        match target {
            "offer" => *offer.last_mut().unwrap() ^= 1,
            "guest_hello" => guest_hello[5] ^= 1,
            _ => {}
        }
        let result = h.finish(&guest_hello, &offer, &mut host, now);
        if target != "bundle" {
            assert!(result.is_err(), "{target}");
            continue;
        }
        let mut bundle = result.unwrap();
        bundle[30] ^= 1;
        assert!(g.finish(&bundle).is_err(), "bundle");
    }
}

#[test]
fn k06_code_expires_after_ten_minutes() {
    let mut host = host_vault();
    let start = Instant::now();
    let (mut h, code, host_hello) = PairingHost::start(7, start).unwrap();
    let (mut g, guest_hello) = PairingGuest::start(&code).unwrap();
    let offer = g.offer(&host_hello).unwrap();
    let late = start + LIFETIME + Duration::from_secs(1);
    assert!(h.finish(&guest_hello, &offer, &mut host, late).is_err());
}

#[test]
fn k07_old_messages_do_not_work_in_a_new_run() {
    let mut host = host_vault();
    let now = Instant::now();
    // A complete run, recorded by the relay.
    let (mut h1, code1, hh1) = PairingHost::start(7, now).unwrap();
    let (mut g1, gh1) = PairingGuest::start(&code1).unwrap();
    let off1 = g1.offer(&hh1).unwrap();
    let _bundle1 = h1.finish(&gh1, &off1, &mut host, now).unwrap();
    // New run: the relay replays the old guest messages.
    let (mut h2, _code2, _hh2) = PairingHost::start(7, now).unwrap();
    let seq = host.device_list().list.seq;
    assert!(h2.finish(&gh1, &off1, &mut host, now).is_err());
    assert_eq!(host.device_list().list.seq, seq);
}

#[test]
fn k08_codes_are_forgiving_to_type_but_strict_in_content() {
    let code = Code::random(42).unwrap();
    let text = code.text();
    assert_eq!(Code::parse(&text).unwrap(), code);
    assert_eq!(
        Code::parse(&text.to_uppercase().replace('-', "  ")).unwrap(),
        code
    );
    for bad in [
        "",
        "42",
        "42-anker",
        "x-anker-birne-kerze",
        "42-anker-birne-kerze-dach",
        "42-anker-birne-xyz",
        "70000-anker-birne-dach",
    ] {
        assert!(Code::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(
        format!("{code:?}"),
        "Code(42-…)",
        "debug output hides the words"
    );
}

#[test]
fn k09_word_list_and_randomness() {
    let mut sorted = WORDS.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 256, "256 different words");
    for a in WORDS {
        assert!(a.chars().all(|c| c.is_ascii_lowercase()), "{a}");
        assert!(
            !WORDS.iter().any(|b| *b != a && b.starts_with(a)),
            "{a} starts another word"
        );
    }
    let codes: std::collections::HashSet<String> =
        (0..2000).map(|_| Code::random(1).unwrap().text()).collect();
    assert!(
        codes.len() > 1990,
        "codes repeat too often: {}",
        codes.len()
    );
}

#[test]
fn k10_new_device_vault_opens_with_its_own_password_only() {
    let mut host = host_vault();
    let now = Instant::now();
    let (mut h, code, host_hello) = PairingHost::start(7, now).unwrap();
    let (mut g, guest_hello) = PairingGuest::start(&code).unwrap();
    let offer = g.offer(&host_hello).unwrap();
    let bundle = h.finish(&guest_hello, &offer, &mut host, now).unwrap();
    let joined = g.finish(&bundle).unwrap();
    let own_pw = secret(18);
    let (_, file) = Vault::create_joined_with(&own_pw, joined, CHEAP).unwrap();
    assert!(
        Vault::open_with_limits(&file, &PW, TEST_LIMITS).is_err(),
        "host password does not open it"
    );
    let v = Vault::open_with_limits(&file, &own_pw, TEST_LIMITS).unwrap();
    assert_eq!(v.account().fingerprint(), host.account().fingerprint());
}

#[test]
fn k11_paired_device_can_be_withdrawn_by_the_first() {
    let mut host = host_vault();
    let guest = pair(&mut host);
    host.withdraw_device(&guest.device().id).unwrap();
    let list = host
        .device_list()
        .verify(host.account().public_key(), None)
        .unwrap();
    assert!(list.is_withdrawn(&guest.device().id));
    let mut a = Device::from_identity(host.account(), host.device()).unwrap();
    a.apply_device_list(list);
    let b = Device::from_identity(guest.account(), guest.device()).unwrap();
    a.create_group("g").unwrap();
    let kp = b.key_packages(1).unwrap().remove(0);
    let offer = trair_core::KeyPackageOffer {
        claimed_device: b.id().to_string(),
        bytes: kp,
    };
    assert!(a.add_members("g", &[offer]).is_err());
}
