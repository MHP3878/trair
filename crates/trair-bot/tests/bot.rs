// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! A real bot: the `trair-bot` program in its own process, a real server
//! (in this test process, on a free port) and a person whose Trair is this
//! test process (the same door the app uses). Nothing is faked: MLS,
//! the vault, the certificates and the server checks are the real ones.

use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use trair_bot::client::{Client, open_vault};
use trair_bot::door::call;

/// A random password for the vaults of this test run (never a fixed one
/// in the code).
fn pw() -> &'static str {
    static PW: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PW.get_or_init(|| {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).unwrap();
        b.iter().map(|x| format!("{x:02x}")).collect()
    })
}
/// The door holds one account per process: the tests take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
const BOT: &str = env!("CARGO_BIN_EXE_trair-bot");

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!(
            "trair-bot-{}",
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A real server on a free port; it runs until the test process ends.
fn server(dir: &Path) -> String {
    let db = trair_server::Db::open(&dir.join("server.db")).unwrap();
    let server = trair_server::Server::new(db, trair_server::system_clock());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(
                listener,
                trair_server::router(server).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
    });
    let addr: SocketAddr = rx.recv().unwrap();
    format!("http://{addr}")
}

/// `trair-bot` with the password in its environment.
fn bot_command(args: &[&str]) -> Command {
    let mut c = Command::new(BOT);
    c.args(args).env("TRAIR_BOT_PASSWORT", pw());
    c
}

/// A running bot in stdio mode: its events, and a way to command it.
struct RunningBot {
    child: Child,
    stdin: ChildStdin,
    events: mpsc::Receiver<Value>,
    next_id: u64,
}

impl RunningBot {
    fn start(dir: &Path, server: &str) -> RunningBot {
        let mut child = bot_command(&[
            "start",
            "--daten",
            dir.to_str().unwrap(),
            "--server",
            server,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
        let stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take().unwrap();
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                let v: Value = serde_json::from_str(&line)
                    .unwrap_or_else(|_| panic!("no JSON from the bot: {line}"));
                if tx.send(v).is_err() {
                    return;
                }
            }
        });
        RunningBot {
            child,
            stdin,
            events,
            next_id: 1,
        }
    }

    /// The next line that fits, within `secs` seconds.
    fn wait(&mut self, secs: u64, what: &str, fits: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(v) if fits(&v) => return v,
                Ok(_) => {}
                Err(_) => panic!("the bot did not say: {what}"),
            }
        }
    }

    /// A command and its answer.
    fn ask(&mut self, mut cmd: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        cmd["id"] = json!(id);
        writeln!(self.stdin, "{cmd}").unwrap();
        self.stdin.flush().unwrap();
        self.wait(30, &format!("an answer to {cmd}"), |v| v["id"] == id)
    }
}

impl Drop for RunningBot {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Receive as the person until `done` holds (up to 30 seconds).
fn person_until(p: &mut Client, what: &str, mut done: impl FnMut(&mut Client) -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        if done(p) {
            return;
        }
        let batch = p.fetch(1).unwrap();
        p.process(&batch);
    }
    panic!("the person never saw: {what}");
}

fn texts(p: &Client, chat: &str) -> Vec<String> {
    p.messages(chat, 50)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn t01_a_bot_its_owner_and_a_chat_through_the_real_server() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let tmp = TempDir::new();
    let url = server(&tmp.0);
    let bot_dir = tmp.0.join("wetterbot");

    // Julian: a person, in this process.
    open_vault(&tmp.0.join("julian"), pw(), Some(json!({"name": "Julian"}))).unwrap();
    let mut julian = Client::connect(&url, false).unwrap();
    julian.online();

    // The bot is made; its code goes only to stdout, the rest to stderr.
    let made = bot_command(&[
        "neu",
        "--daten",
        bot_dir.to_str().unwrap(),
        "--name",
        "Wetterbot",
        "--server",
        &url,
    ])
    .output()
    .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let code = String::from_utf8(made.stdout).unwrap().trim().to_string();
    assert!(code.starts_with("trairbot1:"), "{code}");
    // Without a password the bot does not open (and never asks the bot's
    // program for it).
    let no_pw = Command::new(BOT)
        .args([
            "start",
            "--daten",
            bot_dir.to_str().unwrap(),
            "--server",
            &url,
        ])
        .env_remove("TRAIR_BOT_PASSWORT")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!no_pw.status.success());
    // A second bot in the same folder: refused.
    let again = bot_command(&[
        "neu",
        "--daten",
        bot_dir.to_str().unwrap(),
        "--name",
        "Zweiter",
        "--server",
        &url,
    ])
    .output()
    .unwrap();
    assert!(!again.status.success());

    // Julian vouches with the code; the bot starts and is ready.
    julian.vouch(&code).unwrap();
    assert!(julian.vouch(&code).is_err(), "the code works once");
    let mut bot = RunningBot::start(&bot_dir, &url);
    let ready = bot.wait(60, "ready", |v| v["event"] == "ready");
    assert_eq!(ready["name"], "Wetterbot");
    assert_eq!(ready["owner"], julian.user);
    let bot_user = ready["user"].as_str().unwrap().to_string();

    // Julian adds the bot and writes; for the bot it is a request.
    let added = julian
        .add_contact(ready["contact_code"].as_str().unwrap(), "Wetter")
        .unwrap();
    assert_eq!(added, bot_user);
    let chat = julian.start_chat(&bot_user).unwrap();
    let request = bot.wait(30, "the request", |v| v["event"] == "request");
    assert_eq!(request["chat"], chat);
    assert_eq!(request["from"], julian.user);
    assert_eq!(bot.ask(json!({"op": "accept", "chat": chat}))["ok"], true);

    // The bot's profile reaches Julian: "Wetterbot", a bot of Julian's,
    // and the certificate holds against Julian's key from the server.
    person_until(&mut julian, "the bot's profile", |_| {
        call(json!({"op": "profiles"}))["profiles"]
            .as_array()
            .is_some_and(|l| l.iter().any(|p| p["user"] == bot_user))
    });
    let profiles = call(json!({"op": "profiles"}));
    let p = profiles["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["user"] == bot_user)
        .unwrap()
        .clone();
    assert_eq!(p["name"], "Wetterbot");
    assert_eq!(p["bot"]["owner"], julian.user);
    let owner_key = julian.api.account(&julian.user).body["account_key"].clone();
    let checked = call(json!({"op": "check_bot", "user": bot_user, "owner_key": owner_key}));
    assert_eq!(checked["valid"], true, "{checked}");
    let other_key = julian.api.account(&bot_user).body["account_key"].clone();
    let wrong = call(json!({"op": "check_bot", "user": bot_user, "owner_key": other_key}));
    assert_eq!(wrong["valid"], false, "another key does not hold");

    // Julian writes, the bot hears it and answers.
    julian.send(&chat, "Wie wird das Wetter?").unwrap();
    let heard = bot.wait(30, "Julian's text", |v| v["event"] == "message");
    assert_eq!(heard["text"], "Wie wird das Wetter?");
    assert_eq!(heard["from"], julian.user);
    assert_eq!(heard["chat"], chat);
    assert_eq!(heard["kind"], "direct");
    let sent = bot.ask(json!({"op": "send", "chat": chat, "text": "Sonnig, 21 Grad."}));
    assert_eq!(sent["ok"], true, "{sent}");
    person_until(&mut julian, "the bot's answer", |j| {
        texts(j, &chat).contains(&"Sonnig, 21 Grad.".to_string())
    });

    // Commands that make no sense are answered, not ignored.
    assert_eq!(bot.ask(json!({"op": "fliegen"}))["error"], "unknown_op");
    assert_eq!(
        bot.ask(json!({"op": "send", "chat": chat}))["error"],
        "missing_text"
    );
    let long = "x".repeat(4001);
    assert_eq!(
        bot.ask(json!({"op": "send", "chat": chat, "text": long}))["error"],
        "too_long"
    );
    assert_eq!(
        bot.ask(json!({"op": "profile", "name": ""}))["error"],
        "bad_name"
    );
    // The bot's history and a new name.
    let m = bot.ask(json!({"op": "messages", "chat": chat}));
    let list: Vec<&str> = m["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["text"].as_str().unwrap())
        .collect();
    assert_eq!(list, ["Wie wird das Wetter?", "Sonnig, 21 Grad."]);
    assert_eq!(
        bot.ask(json!({"op": "profile", "name": "Wetterfrosch"}))["ok"],
        true
    );
    person_until(&mut julian, "the new name", |_| {
        call(json!({"op": "profiles"}))["profiles"]
            .as_array()
            .is_some_and(|l| l.iter().any(|p| p["name"] == "Wetterfrosch"))
    });

    assert_eq!(bot.ask(json!({"op": "stop"}))["ok"], true);
    let status = bot.child.wait().unwrap();
    assert!(status.success());
    call(json!({"op": "lock"}));
}

/// The example bots in `bots/beispiele`, run by `trair-bot` as its own
/// program would be, in every language this computer has.
#[test]
fn t02_the_example_bots_answer_in_python_and_javascript() {
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bots/beispiele");
    let mut ran = 0;
    for (program, file) in [
        ("python3", "echo.py"),
        ("python", "echo.py"),
        ("node", "echo.js"),
    ] {
        if Command::new(program).arg("--version").output().is_err() {
            eprintln!("{program} fehlt hier: {file} nicht getestet");
            continue;
        }
        if program == "python" && ran > 0 {
            continue;
        }
        let tmp = TempDir::new();
        let url = server(&tmp.0);
        let bot_dir = tmp.0.join("echo");
        open_vault(&tmp.0.join("ada"), pw(), Some(json!({"name": "Ada"}))).unwrap();
        let mut ada = Client::connect(&url, false).unwrap();
        ada.online();
        let made = bot_command(&[
            "neu",
            "--daten",
            bot_dir.to_str().unwrap(),
            "--name",
            "Echo",
            "--server",
            &url,
        ])
        .output()
        .unwrap();
        assert!(made.status.success());
        let code = String::from_utf8(made.stdout).unwrap().trim().to_string();
        ada.vouch(&code).unwrap();
        // Name and key are in the bot's code: enough for its contact code.
        let req = trair_core::bots::parse_request(&code).unwrap();
        let user = req.bot.clone();
        let contact = trair_core::ContactCode {
            user: req.bot,
            fingerprint: trair_core::Fingerprint::of(&req.key),
        }
        .text();
        let script = examples.join(file);
        let child = bot_command(&[
            "start",
            "--daten",
            bot_dir.to_str().unwrap(),
            "--server",
            &url,
            "--",
            program,
            script.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
        let bot = Running(child);
        ada.add_contact(&contact, "Echo").unwrap();
        let chat = ada.start_chat(&user).unwrap();
        ada.send(&chat, "Grüß dich, Bot!").unwrap();
        person_until(&mut ada, &format!("the echo from {file}"), |a| {
            texts(a, &chat).contains(&"Echo: Grüß dich, Bot!".to_string())
        });
        drop(bot);
        call(json!({"op": "lock"}));
        ran += 1;
    }
    assert!(ran > 0, "neither Python nor Node.js here");
}

/// A bot program that ends with the test, also when the test fails.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
