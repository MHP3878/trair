// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! `trair-bot`: runs a Trair bot. See `trair-bot hilfe`.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use trair_bot::client::{Client, open_vault};
use trair_bot::door;
use trair_bot::net::Api;
use trair_bot::protocol::{After, Profile, execute};

const HELP: &str = "\
trair-bot – ein Trair-Bot auf diesem PC

  trair-bot neu --daten ORDNER --name NAME [--server URL]
      Legt den Bot an (eigenes Konto, verschlüsselter Tresor im ORDNER),
      meldet ihn beim Server an und zeigt den Bot-Code. Den gibst du dem
      Besitzer: In Trair unter Einstellungen -> Bots -> „Bot bestätigen“.

  trair-bot code --daten ORDNER [--server URL]
      Ein neuer Bot-Code (der alte gilt dann nicht mehr), z. B. für einen
      anderen Besitzer.

  trair-bot start --daten ORDNER [--server URL] [-- PROGRAMM ARGUMENTE...]
      Startet den Bot. Mit PROGRAMM startet trair-bot dein Bot-Programm
      und spricht mit ihm über dessen Ein- und Ausgabe: eine JSON-Zeile
      pro Ereignis bzw. Befehl. Ohne PROGRAMM über die eigene Ein- und
      Ausgabe.

Server: Standard http://127.0.0.1:8471 (bis TLS fertig ist nur dieser PC).
Passwort des Tresors: Umgebungsvariable TRAIR_BOT_PASSWORT, oder
--passwort-datei DATEI, sonst fragt trair-bot (ohne es anzuzeigen).
Dein Bot-Programm bekommt das Passwort nie zu sehen.";

const PASSWORD_VAR: &str = "TRAIR_BOT_PASSWORT";

struct Args {
    what: String,
    data: Option<PathBuf>,
    server: String,
    name: Option<String>,
    password_file: Option<PathBuf>,
    program: Vec<String>,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let what = args.next().unwrap_or_else(|| "hilfe".into());
    let mut a = Args {
        what,
        data: None,
        server: "http://127.0.0.1:8471".into(),
        name: None,
        password_file: None,
        program: Vec::new(),
    };
    while let Some(flag) = args.next() {
        if flag == "--" {
            a.program = args.by_ref().collect();
            break;
        }
        let value = args.next().ok_or(format!("bei {flag} fehlt der Wert"))?;
        match flag.as_str() {
            "--daten" => a.data = Some(PathBuf::from(value)),
            "--server" => a.server = value,
            "--name" => a.name = Some(value),
            "--passwort-datei" => a.password_file = Some(PathBuf::from(value)),
            other => return Err(format!("unbekannte Angabe {other} (siehe trair-bot hilfe)")),
        }
    }
    Ok(a)
}

fn password(a: &Args, new: bool) -> Result<String, String> {
    if let Ok(p) = std::env::var(PASSWORD_VAR) {
        return Ok(p);
    }
    if let Some(f) = &a.password_file {
        let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        return Ok(text.trim_end_matches(['\r', '\n']).to_string());
    }
    if !std::io::stdin().is_terminal() {
        return Err(format!(
            "kein Passwort: {PASSWORD_VAR} setzen oder --passwort-datei angeben"
        ));
    }
    let p =
        rpassword::prompt_password("Passwort für den Bot-Tresor: ").map_err(|e| e.to_string())?;
    if new {
        let again = rpassword::prompt_password("Noch einmal: ").map_err(|e| e.to_string())?;
        if again != p {
            return Err("die Passwörter sind nicht gleich".into());
        }
    }
    Ok(p)
}

fn data_dir(a: &Args) -> Result<PathBuf, String> {
    a.data
        .clone()
        .ok_or("--daten ORDNER fehlt (siehe trair-bot hilfe)".into())
}

fn show_code(code: &str) {
    eprintln!();
    eprintln!("Bot-Code (für den Besitzer, gilt einmal):");
    println!("{code}");
    eprintln!();
    eprintln!("Der Besitzer fügt ihn in Trair ein: Einstellungen -> Bots -> „Bot bestätigen“.");
}

fn main() {
    if let Err(e) = run() {
        eprintln!("trair-bot: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let a = parse(std::env::args().skip(1))?;
    match a.what.as_str() {
        "neu" => {
            let name = a.name.clone().ok_or("--name NAME fehlt")?;
            let name = name.trim().to_string();
            if name.is_empty() || name.chars().count() > 64 {
                return Err("der Name muss 1 bis 64 Zeichen haben".into());
            }
            let dir = data_dir(&a)?;
            let pw = password(&a, true)?;
            open_vault(&dir, &pw, Some(json!({"name": name})))?;
            let mut client = Client::connect(&a.server, true)?;
            // Key packages right away: people can start a chat with the bot
            // before it runs for the first time.
            client.online();
            let code = client.ask_owner()?;
            eprintln!("Bot „{name}“ angelegt ({}).", client.user);
            show_code(&code);
            door::call(json!({"op": "lock"}));
            Ok(())
        }
        "code" => {
            let dir = data_dir(&a)?;
            let pw = password(&a, false)?;
            open_vault(&dir, &pw, None)?;
            let client = Client::connect(&a.server, true)?;
            if let Some(cert) = client.bot_cert() {
                eprintln!(
                    "Jetzt bestätigt von {}. Ein neuer Code lässt einen anderen Besitzer übernehmen.",
                    cert["owner"].as_str().unwrap_or("?")
                );
            }
            show_code(&client.ask_owner()?);
            door::call(json!({"op": "lock"}));
            Ok(())
        }
        "start" => start(&a),
        "hilfe" | "--help" | "-h" => {
            println!("{HELP}");
            Ok(())
        }
        other => Err(format!("unbekannt: {other} (siehe trair-bot hilfe)")),
    }
}

/// What the main thread waits for.
enum Work {
    /// A batch from the server (or why there is none).
    Fetched(Result<Vec<(i64, String)>, String>),
    /// A line from the bot's program.
    Line(String),
    /// The bot's program is gone.
    Closed,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn start(a: &Args) -> Result<(), String> {
    let dir = data_dir(a)?;
    let pw = password(a, false)?;
    let stored = open_vault(&dir, &pw, None)?;
    drop(pw);
    let mut client = Client::connect(&a.server, true)?;
    client.online();

    // Without its owner's word the server lets the bot write nothing.
    let mut told = false;
    let cert = loop {
        if let Some(c) = client.bot_cert() {
            break c;
        }
        if !told {
            eprintln!(
                "trair-bot: Der Besitzer hat den Bot noch nicht bestätigt. Warte … \
                 (neuer Code: trair-bot code --daten {})",
                dir.display()
            );
            told = true;
        }
        thread::sleep(Duration::from_secs(5));
    };
    let mut profile = Profile {
        name: stored["name"].as_str().unwrap_or("Bot").to_string(),
        about: stored["about"].as_str().unwrap_or("").to_string(),
        rev: now_ms(),
        cert: cert.clone(),
    };
    client.share_profile(&profile.wire())?;

    // The bot's program: a child, or this process's own input and output.
    let (tx, rx) = mpsc::channel::<Work>();
    let mut child: Option<Child> = None;
    let mut out: Box<dyn Write> = if a.program.is_empty() {
        let tx = tx.clone();
        thread::spawn(move || read_lines(std::io::stdin().lock(), &tx));
        Box::new(std::io::stdout())
    } else {
        let mut c = Command::new(&a.program[0])
            .args(&a.program[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env_remove(PASSWORD_VAR)
            .spawn()
            .map_err(|e| format!("{}: {e}", a.program[0]))?;
        let stdout = c.stdout.take().ok_or("keine Ausgabe vom Programm")?;
        let stdin = c.stdin.take().ok_or("keine Eingabe zum Programm")?;
        let tx = tx.clone();
        thread::spawn(move || read_lines(BufReader::new(stdout), &tx));
        child = Some(c);
        Box::new(stdin)
    };

    // Receiving runs on its own; the main thread processes one batch at a
    // time and then lets it go on (with a new connection after a reconnect).
    let (go_tx, go_rx) = mpsc::channel::<Option<Api>>();
    {
        let tx = tx.clone();
        let mut api = client.api.clone();
        thread::spawn(move || {
            loop {
                if tx.send(Work::Fetched(fetch(&api))).is_err() {
                    return;
                }
                match go_rx.recv() {
                    Ok(Some(new)) => api = new,
                    Ok(None) => {}
                    Err(_) => return,
                }
            }
        });
    }
    drop(tx);

    let owner = cert["owner"].as_str().unwrap_or_default().to_string();
    let ready = json!({"event": "ready", "user": client.user, "name": profile.name,
        "contact_code": client.contact_code(), "owner": owner});
    emit(&mut out, &ready)?;
    eprintln!("trair-bot: „{}“ läuft ({}).", profile.name, client.user);

    let mut failures = 0u32;
    let result = loop {
        let work = match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(w) => w,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                client.top_up();
                client.sweep();
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
        };
        match work {
            Work::Fetched(Ok(batch)) => {
                failures = 0;
                let events = client.process(&batch);
                let mut sent = Ok(());
                for e in &events {
                    sent = emit(&mut out, e);
                    if sent.is_err() {
                        break;
                    }
                }
                if events.iter().any(|e| e["event"] == "joined") {
                    let _ = client.share_profile(&profile.wire());
                }
                let _ = go_tx.send(None);
                if let Err(e) = sent {
                    break Err(e);
                }
            }
            Work::Fetched(Err(e)) => {
                failures += 1;
                thread::sleep(Duration::from_secs(2));
                if failures >= 3 {
                    // Logged out (or the server restarted): log in again.
                    match Client::connect(&a.server, true) {
                        Ok(c) => {
                            client = c;
                            failures = 0;
                            let _ = go_tx.send(Some(client.api.clone()));
                            continue;
                        }
                        Err(why) => {
                            let _ = emit(
                                &mut out,
                                &json!({"event": "problem",
                                "text": format!("Verbindung: {e} / {why}")}),
                            );
                            thread::sleep(Duration::from_secs(5));
                        }
                    }
                }
                let _ = go_tx.send(None);
            }
            Work::Line(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                let (answer, after) = execute(&mut client, &mut profile, &line);
                if let Err(e) = emit(&mut out, &answer) {
                    break Err(e);
                }
                if let After::Stop = after {
                    break Ok(());
                }
            }
            Work::Closed => break Ok(()),
        }
    };
    door::call(json!({"op": "lock"}));
    if let Some(mut c) = child {
        drop(out);
        // Give the program a moment to end by itself, then end it.
        for _ in 0..20 {
            if matches!(c.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = c.kill();
        let _ = c.wait();
    }
    result
}

fn fetch(api: &Api) -> Result<Vec<(i64, String)>, String> {
    let r = api.fetch(25);
    if !r.ok() {
        return Err(r.error());
    }
    Ok(r.body["messages"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| Some((m["id"].as_i64()?, m["body"].as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default())
}

fn read_lines(reader: impl BufRead, tx: &mpsc::Sender<Work>) {
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if tx.send(Work::Line(line)).is_err() {
            return;
        }
    }
    let _ = tx.send(Work::Closed);
}

fn emit(out: &mut Box<dyn Write>, v: &Value) -> Result<(), String> {
    writeln!(out, "{v}")
        .and_then(|()| out.flush())
        .map_err(|_| "das Bot-Programm hört nicht mehr zu".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn the_command_line() {
        let a = args(&["start", "--daten", "bot", "--", "python", "bot.py", "--x"]).unwrap();
        assert_eq!(a.what, "start");
        assert_eq!(a.data, Some(PathBuf::from("bot")));
        assert_eq!(a.server, "http://127.0.0.1:8471");
        assert_eq!(a.program, ["python", "bot.py", "--x"]);
        assert!(args(&["start", "--daten"]).is_err());
        assert!(args(&["start", "--passwort", "geheim"]).is_err());
        assert_eq!(args(&[]).unwrap().what, "hilfe");
    }
}
