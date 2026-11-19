// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The team tool, driven like a person in a terminal would.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use trair_core::Account;
use trair_core::badges::{Badge, BadgeKind, verify};
use trair_core::teamkey::TeamKey;
use trair_core::vault::{KdfParams, Limits};
use trair_team::{Console, Settings, check_new_key_path, date, run};
use zeroize::Zeroizing;

const NOW: u64 = 1_800_000_000;
const CHEAP: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};

#[derive(Default)]
struct Fake {
    answers: VecDeque<String>,
    out: Vec<String>,
}

impl Console for Fake {
    fn passphrase(&mut self, _prompt: &str) -> std::io::Result<Zeroizing<String>> {
        Ok(Zeroizing::new(
            self.answers
                .pop_front()
                .expect("a passphrase was asked for"),
        ))
    }
    fn say(&mut self, text: &str) {
        self.out.push(text.to_string());
    }
    fn pause(&mut self) {}
}

impl Fake {
    fn with(answers: &[&str]) -> Fake {
        Fake {
            answers: answers.iter().map(|s| s.to_string()).collect(),
            out: Vec::new(),
        }
    }
    fn text(&self) -> String {
        self.out.join("\n")
    }
    fn line_after(&self, label: &str) -> String {
        self.out
            .iter()
            .find_map(|l| l.strip_prefix(label))
            .expect(label)
            .trim()
            .to_string()
    }
}

fn settings() -> Settings {
    Settings {
        kdf: CHEAP,
        limits: Limits {
            min: CHEAP,
            max: Limits::PRODUCTION.max,
        },
        now: NOW,
    }
}

fn secret() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("random");
    bytes.iter().map(|b| char::from(b'a' + b % 26)).collect()
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("trair-team-test-{}", hex::encode(secret())));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn key_path(dir: &Path) -> String {
    dir.join("team.trairkey").to_string_lossy().into_owned()
}

#[test]
fn w01_make_show_and_sign() {
    let dir = temp_dir();
    let file = key_path(&dir);
    let pass = secret();
    let mut c = Fake::with(&[&pass, &pass]);
    assert_eq!(
        run(&args(&["neu", "--datei", &file]), &mut c, &settings()),
        0,
        "{}",
        c.text()
    );
    let public = c.line_after("Öffentlicher Schlüssel:");
    assert_eq!(public.len(), 64);
    assert!(c.text().contains("USB-Stick"));
    assert!(!c.text().contains(&pass), "the passphrase is never printed");

    let mut c = Fake::with(&[&pass]);
    assert_eq!(
        run(&args(&["zeigen", "--datei", &file]), &mut c, &settings()),
        0
    );
    assert_eq!(c.line_after("Öffentlicher Schlüssel:"), public);

    let julian = Account::generate("julian").unwrap();
    let holder = hex::encode(julian.public_key());
    let mut c = Fake::with(&[&pass]);
    assert_eq!(
        run(
            &args(&["abzeichen", "--fuer", &holder, "--datei", &file]),
            &mut c,
            &settings()
        ),
        0,
        "{}",
        c.text()
    );
    let json = c
        .out
        .iter()
        .find(|l| l.starts_with('{'))
        .expect("badge line");
    let badge: Badge = serde_json::from_str(json).unwrap();
    let pinned = [public.as_str()];
    let v = verify(&badge, julian.public_key(), NOW + 300 * 24 * 3600, &pinned).unwrap();
    assert_eq!(v.kind, BadgeKind::Team);
    assert!(
        c.text()
            .contains(&format!("gültig bis {}", date(NOW + 365 * 24 * 3600)))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn w02_never_overwrites_and_never_into_git_or_cloud() {
    let dir = temp_dir();
    let file = key_path(&dir);
    std::fs::write(&file, b"schon da").unwrap();
    let mut c = Fake::default();
    assert_eq!(
        run(&args(&["neu", "--datei", &file]), &mut c, &settings()),
        1
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"schon da", "untouched");

    let repo = dir.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    assert!(
        check_new_key_path(&repo.join("deep").join("k.trairkey"))
            .unwrap_err()
            .contains("Git")
    );
    assert!(check_new_key_path(&dir.join("OneDrive").join("k.trairkey")).is_err());
    assert!(
        check_new_key_path(&dir.join("k.txt")).is_err(),
        "must end in .trairkey"
    );
    assert!(check_new_key_path(&dir.join("ok.trairkey")).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn w03_weak_or_mistyped_passphrases_make_no_file() {
    let dir = temp_dir();
    let file = key_path(&dir);
    let pass = secret();
    for answers in [
        vec![pass.clone(), secret()],
        vec!["kurz".to_string(), "kurz".to_string()],
    ] {
        let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
        let mut c = Fake::with(&refs);
        assert_eq!(
            run(&args(&["neu", "--datei", &file]), &mut c, &settings()),
            1
        );
        assert!(!Path::new(&file).exists());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn w04_wrong_passphrase_and_bad_input_sign_nothing() {
    let dir = temp_dir();
    let file = key_path(&dir);
    let pass = secret();
    let sealed = TeamKey::generate()
        .unwrap()
        .seal_with(&pass, CHEAP)
        .unwrap();
    std::fs::write(&file, sealed).unwrap();
    let holder = "ab".repeat(32);

    let mut c = Fake::with(&[&secret()]);
    assert_eq!(
        run(
            &args(&["abzeichen", "--fuer", &holder, "--datei", &file]),
            &mut c,
            &settings()
        ),
        1
    );
    assert!(c.text().contains("Passphrase falsch"));
    assert!(!c.out.iter().any(|l| l.starts_with('{')));

    for bad in [
        vec!["abzeichen", "--datei", &file],
        vec!["abzeichen", "--fuer", "1234", "--datei", &file],
        vec![
            "abzeichen",
            "--fuer",
            &holder,
            "--tage",
            "400",
            "--datei",
            &file,
        ],
    ] {
        let mut c = Fake::default();
        assert_eq!(run(&args(&bad), &mut c, &settings()), 1, "{bad:?}");
    }
    let mut c = Fake::default();
    assert_eq!(run(&args(&["loeschen"]), &mut c, &settings()), 2);
    assert_eq!(run(&args(&["neu", "--wohin", "x"]), &mut c, &settings()), 2);
    // A real key file is refused with the cheap test settings' opposite:
    // the real limits do not open a cheap file.
    let mut c = Fake::with(&[&pass]);
    let real = Settings {
        now: NOW,
        ..Settings::real()
    };
    assert_eq!(run(&args(&["zeigen", "--datei", &file]), &mut c, &real), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn w05_dates_and_help() {
    assert_eq!(date(0), "01.01.1970");
    assert_eq!(date(1_763_510_400), "19.11.2025");
    assert_eq!(date(1_709_164_800), "29.02.2024");
    let mut c = Fake::default();
    assert_eq!(run(&[], &mut c, &settings()), 0);
    assert!(c.text().contains("trair-team neu"));
}
