// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! `trair-team`: makes the Trair team key on the owner's own computer and
//! signs team badges with it (decisions of 19.11.2026).
//!
//! - `trair-team neu` makes the key and stores it encrypted with a long
//!   passphrase. It never overwrites a file and refuses folders that are
//!   part of a Git repository or synced to a cloud.
//! - `trair-team zeigen` shows the public key (the only part that goes
//!   into the app's code).
//! - `trair-team abzeichen --fuer <Kontoschlüssel>` signs a team badge,
//!   valid for a year by default.
//!
//! The logic lives here so the tests can drive it; `main.rs` only
//! connects it to the terminal.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use trair_core::teamkey::{MIN_PASSPHRASE_CHARS, TEAM_KDF, TEAM_LIMITS, TeamKey, check_passphrase};
use trair_core::vault::{KdfParams, Limits, VaultError};
use zeroize::Zeroizing;

/// File name of the team key.
pub const FILE_NAME: &str = "team-schluessel.trairkey";
/// Default validity of a team badge, in days.
pub const DEFAULT_DAYS: u64 = 365;

/// What the tool needs from the terminal. The tests use a fake one.
pub trait Console {
    /// Read a passphrase without showing it.
    fn passphrase(&mut self, prompt: &str) -> std::io::Result<Zeroizing<String>>;
    /// Show a line of text.
    fn say(&mut self, text: &str);
    /// Wait until the person presses Enter (when started by double-click).
    fn pause(&mut self);
}

/// Argon2id settings and the clock. Tests use cheap settings and a fixed
/// time.
pub struct Settings {
    /// Settings for a new key file.
    pub kdf: KdfParams,
    /// Settings a key file must have to be opened.
    pub limits: Limits,
    /// Unix time in seconds.
    pub now: u64,
}

impl Settings {
    /// The settings of the real tool.
    pub fn real() -> Settings {
        Settings {
            kdf: TEAM_KDF,
            limits: TEAM_LIMITS,
            now: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        }
    }
}

/// Run the tool. Returns the exit code: 0 done, 1 refused or failed,
/// 2 wrong use.
pub fn run(args: &[String], console: &mut dyn Console, settings: &Settings) -> i32 {
    let Some(command) = args.first() else {
        console.say(HELP);
        console.say("");
        console.say("Dieses Programm läuft im Terminal. Drücke Enter zum Schließen.");
        console.pause();
        return 0;
    };
    let options = match Options::parse(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            console.say(&e);
            return 2;
        }
    };
    let result = match command.as_str() {
        "neu" => make_key(&options, console, settings),
        "zeigen" => show_key(&options, console, settings),
        "abzeichen" => sign_badge(&options, console, settings),
        "hilfe" | "--help" | "-h" => {
            console.say(HELP);
            Ok(())
        }
        other => {
            console.say(&format!("Unbekannter Befehl „{other}“.\n\n{HELP}"));
            return 2;
        }
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            console.say(&format!("Abgebrochen: {e}"));
            1
        }
    }
}

const HELP: &str = "trair-team – der Trair-Team-Schlüssel

  trair-team neu [--datei PFAD]
      Erzeugt den Team-Schlüssel und speichert ihn mit einer Passphrase verschlüsselt.
  trair-team zeigen [--datei PFAD]
      Zeigt den öffentlichen Schlüssel. Nur der kommt in den Trair-Code.
  trair-team abzeichen --fuer KONTOSCHLÜSSEL [--tage 365] [--datei PFAD] [--ausgabe DATEI]
      Unterschreibt ein Trair-Team-Abzeichen für ein Konto (höchstens 366 Tage).

Den Kontoschlüssel findest du in Trair unter Einstellungen → Konto.";

#[derive(Default)]
struct Options {
    file: Option<PathBuf>,
    holder: Option<String>,
    days: Option<u64>,
    output: Option<PathBuf>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, String> {
        let mut o = Options::default();
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            let value = it
                .next()
                .ok_or_else(|| format!("Bei {flag} fehlt der Wert."))?;
            match flag.as_str() {
                "--datei" => o.file = Some(PathBuf::from(value)),
                "--fuer" | "--für" => o.holder = Some(value.clone()),
                "--tage" => {
                    o.days = Some(
                        value
                            .parse()
                            .map_err(|_| "--tage braucht eine Zahl.".to_string())?,
                    )
                }
                "--ausgabe" => o.output = Some(PathBuf::from(value)),
                other => {
                    return Err(format!(
                        "Unbekannte Angabe „{other}“. Hilfe: trair-team hilfe"
                    ));
                }
            }
        }
        Ok(o)
    }

    fn key_file(&self) -> Result<PathBuf, String> {
        match &self.file {
            Some(f) => Ok(f.clone()),
            None => default_path()
                .ok_or_else(|| "Kein Standardordner gefunden. Gib --datei an.".to_string()),
        }
    }
}

/// Where the key goes by default: a local, not synced folder of the user
/// (`%LOCALAPPDATA%\Trair\Team` on Windows).
pub fn default_path() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(
            PathBuf::from(base)
                .join("Trair")
                .join("Team")
                .join(FILE_NAME),
        )
    } else {
        let base = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(base)
                .join(".local")
                .join("share")
                .join("trair-team")
                .join(FILE_NAME),
        )
    }
}

/// A new key file may only go where it cannot leak by accident: not over
/// an existing file, not into a Git repository, not into a cloud folder.
pub fn check_new_key_path(path: &Path) -> Result<(), String> {
    if path.extension().and_then(|e| e.to_str()) != Some("trairkey") {
        return Err("Die Datei muss auf .trairkey enden (Git ignoriert solche Dateien).".into());
    }
    if path.exists() {
        return Err(format!(
            "{} gibt es schon. Ein Team-Schlüssel wird nie überschrieben.",
            path.display()
        ));
    }
    let lower = path.to_string_lossy().to_lowercase();
    for cloud in [
        "onedrive",
        "dropbox",
        "google drive",
        "googledrive",
        "icloud",
    ] {
        if lower.contains(cloud) {
            return Err(
                "Dieser Ordner wird in eine Cloud hochgeladen. Nimm einen lokalen Ordner.".into(),
            );
        }
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    if let Some(repo) = absolute
        .ancestors()
        .skip(1)
        .find(|dir| dir.join(".git").exists())
    {
        return Err(format!(
            "{} ist ein Git-Repository. Der Team-Schlüssel darf nie in Git landen.",
            repo.display()
        ));
    }
    Ok(())
}

fn make_key(o: &Options, console: &mut dyn Console, settings: &Settings) -> Result<(), String> {
    let path = o.key_file()?;
    check_new_key_path(&path)?;
    console.say("Du erzeugst jetzt den Trair-Team-Schlüssel.");
    console.say(&format!(
        "Wähle eine Passphrase mit mindestens {MIN_PASSPHRASE_CHARS} Zeichen, am besten fünf zufällige Wörter."
    ));
    let first = console
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    let second = console
        .passphrase("Noch einmal: ")
        .map_err(|e| e.to_string())?;
    if *first != *second {
        return Err("Die beiden Eingaben sind verschieden.".into());
    }
    check_passphrase(&first).map_err(|_| {
        format!("Passphrase zu schwach: mindestens {MIN_PASSPHRASE_CHARS} Zeichen, nicht nur ein Zeichen.")
    })?;
    console.say("Verschlüssele … (ein paar Sekunden, braucht 1 GiB Arbeitsspeicher)");
    let key = TeamKey::generate().map_err(|e| e.to_string())?;
    let sealed = key
        .seal_with(&first, settings.kdf)
        .map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Ordner {}: {e}", dir.display()))?;
    }
    write_new(&path, &sealed)?;
    // Read it back before telling anyone it worked.
    let check = std::fs::read(&path).map_err(|e| e.to_string())?;
    let reopened =
        TeamKey::open_with_limits(&check, &first, settings.limits).map_err(|e| e.to_string())?;
    if reopened.public_key_hex() != key.public_key_hex() {
        return Err("Die gespeicherte Datei passt nicht. Bitte noch einmal versuchen.".into());
    }
    console.say("");
    console.say(&format!("Gespeichert: {}", path.display()));
    say_public(console, &key);
    console.say("");
    console.say("Jetzt bitte:");
    console.say("  1. Kopiere die Datei auf einen USB-Stick und bewahre ihn sicher auf.");
    console.say("  2. Schreib die Passphrase auf Papier. Ohne sie ist der Schlüssel verloren.");
    console.say("  3. Gib nur den öffentlichen Schlüssel weiter. Er ist nicht geheim.");
    Ok(())
}

fn show_key(o: &Options, console: &mut dyn Console, settings: &Settings) -> Result<(), String> {
    let key = open_key(o, console, settings)?;
    say_public(console, &key);
    Ok(())
}

fn sign_badge(o: &Options, console: &mut dyn Console, settings: &Settings) -> Result<(), String> {
    let holder = o
        .holder
        .as_deref()
        .ok_or("--fuer fehlt: der öffentliche Kontoschlüssel aus Trair (Einstellungen → Konto).")?;
    let holder = hex::decode(holder.trim()).ok().filter(|k| k.len() == 32).ok_or(
        "Der Kontoschlüssel muss aus 64 Zeichen 0-9 und a-f bestehen (Einstellungen → Konto → Kopieren).",
    )?;
    let days = o.days.unwrap_or(DEFAULT_DAYS);
    if !(1..=366).contains(&days) {
        return Err("--tage muss zwischen 1 und 366 liegen.".into());
    }
    if let Some(out) = &o.output
        && out.exists()
    {
        return Err(format!("{} gibt es schon.", out.display()));
    }
    let key = open_key(o, console, settings)?;
    let badge = key
        .sign_badge(&holder, settings.now, days)
        .map_err(|e| e.to_string())?;
    let json = serde_json::to_string(&badge).map_err(|e| e.to_string())?;
    console.say(&format!(
        "Team-Abzeichen gültig bis {}.",
        date(settings.now + days * 24 * 3600)
    ));
    match &o.output {
        Some(out) => {
            write_new(out, json.as_bytes())?;
            console.say(&format!("Gespeichert: {}", out.display()));
        }
        None => {
            console.say(
                "Füge diese Zeile in Trair ein (Einstellungen → Konto → Abzeichen hinzufügen):",
            );
            console.say(&json);
        }
    }
    Ok(())
}

fn open_key(
    o: &Options,
    console: &mut dyn Console,
    settings: &Settings,
) -> Result<TeamKey, String> {
    let path = o.key_file()?;
    let file = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let pass = console
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    TeamKey::open_with_limits(&file, &pass, settings.limits).map_err(|e| match e {
        VaultError::WrongPassword => {
            "Passphrase falsch, oder die Datei wurde verändert.".to_string()
        }
        other => other.to_string(),
    })
}

fn say_public(console: &mut dyn Console, key: &TeamKey) {
    console.say(&format!("Öffentlicher Schlüssel: {}", key.public_key_hex()));
    console.say(&format!(
        "Fingerabdruck:          {}",
        key.fingerprint().grouped()
    ));
}

/// Write a file that must not exist yet; on Unix only the owner may read
/// it.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(|e| e.to_string())
}

/// Unix time as `TT.MM.JJJJ` (UTC).
pub fn date(unix: u64) -> String {
    // Days to civil date, after Howard Hinnant's algorithm.
    let z = (unix / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{day:02}.{month:02}.{year}")
}
