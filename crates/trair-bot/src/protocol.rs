// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The bot's side: one JSON object per line.
//!
//! **Events** (from `trair-bot`) carry `"event"`:
//!
//! | event     | fields                                              |
//! |-----------|-----------------------------------------------------|
//! | `ready`   | `user`, `name`, `contact_code`, `owner`             |
//! | `message` | `chat`, `kind` (`direct`/`group`), `from`, `name`, `text`, `id`, `at` |
//! | `request` | `chat`, `kind`, `from`, `name` (a chat request or a group invitation) |
//! | `joined`  | `chat`, `kind`, `name`                              |
//! | `left`    | `chat`, `kind`, `name`                              |
//! | `changed` | `chat`, `added`, `removed`, `left`                  |
//! | `problem` | `text`                                              |
//!
//! **Commands** (to `trair-bot`) carry `"op"` and may carry an `"id"`;
//! every command gets an answer with the same `id`, `"ok"` and, on
//! failure, `"error"`:
//!
//! | op            | fields             | answer                 |
//! |---------------|--------------------|------------------------|
//! | `send`        | `chat`, `text`     | `message` (its id)     |
//! | `accept`      | `chat`             |                        |
//! | `decline`     | `chat`             |                        |
//! | `leave`       | `chat`             |                        |
//! | `add_contact` | `code`, `name`     | `user`                 |
//! | `start_chat`  | `user`             | `chat`                 |
//! | `chats`       |                    | `chats`                |
//! | `messages`    | `chat`, `limit`?   | `messages`             |
//! | `contact_code`|                    | `code`                 |
//! | `profile`     | `name`?, `about`?  |                        |
//! | `stop`        |                    |                        |

use serde_json::{Value, json};

use crate::client::Client;
use crate::door;

/// Longest text a bot may send, in characters (as in the app's chats).
pub const MAX_TEXT_CHARS: usize = 4000;

/// The bot's own profile as it is shared, with its owner's certificate.
pub struct Profile {
    /// The chosen name.
    pub name: String,
    /// "About me".
    pub about: String,
    /// Newer versions have higher numbers.
    pub rev: u64,
    /// The owner's certificate (from the server).
    pub cert: Value,
}

impl Profile {
    /// As it travels (see `trair_core::profiles`).
    pub fn wire(&self) -> Value {
        json!({
            "v": 1,
            "rev": self.rev,
            "name": self.name,
            "about": self.about,
            "badges": [],
            "bot": self.cert,
        })
    }
}

/// What a command line asks for after it ran.
pub enum After {
    /// Go on.
    Go,
    /// Stop the bot.
    Stop,
}

fn text_of<'a>(cmd: &'a Value, field: &str) -> Result<&'a str, String> {
    cmd[field]
        .as_str()
        .ok_or_else(|| format!("missing_{field}"))
}

/// Run one command line; returns the answer and what comes next.
pub fn execute(client: &mut Client, profile: &mut Profile, line: &str) -> (Value, After) {
    let Ok(cmd) = serde_json::from_str::<Value>(line) else {
        return (json!({"ok": false, "error": "bad_json"}), After::Go);
    };
    let id = cmd["id"].clone();
    let mut after = After::Go;
    let result: Result<Value, String> = (|| {
        match cmd["op"].as_str().unwrap_or("") {
            "send" => {
                let text = text_of(&cmd, "text")?;
                if text.chars().count() > MAX_TEXT_CHARS {
                    return Err("too_long".into());
                }
                let m = client.send(text_of(&cmd, "chat")?, text)?;
                Ok(json!({"message": m}))
            }
            "accept" => {
                client.accept(text_of(&cmd, "chat")?)?;
                // A new chat partner gets the bot's profile (and so its owner).
                let _ = client.share_profile(&profile.wire());
                Ok(json!({}))
            }
            "decline" => client.decline(text_of(&cmd, "chat")?).map(|()| json!({})),
            "leave" => client.leave(text_of(&cmd, "chat")?).map(|()| json!({})),
            "add_contact" => {
                let name = cmd["name"].as_str().unwrap_or("");
                let user = client.add_contact(text_of(&cmd, "code")?, name)?;
                Ok(json!({"user": user}))
            }
            "start_chat" => {
                let chat = client.start_chat(text_of(&cmd, "user")?)?;
                let _ = client.share_profile(&profile.wire());
                Ok(json!({"chat": chat}))
            }
            "chats" => Ok(json!({"chats": client.chats()})),
            "messages" => {
                let limit = cmd["limit"].as_u64().unwrap_or(50).min(500);
                let m = client.messages(text_of(&cmd, "chat")?, limit)?;
                Ok(json!({"messages": m}))
            }
            "contact_code" => Ok(json!({"code": client.contact_code()})),
            "profile" => {
                let mut changed = false;
                if let Some(n) = cmd["name"].as_str() {
                    let n = n.trim();
                    if n.is_empty() || n.chars().count() > 64 || n.chars().any(char::is_control) {
                        return Err("bad_name".into());
                    }
                    profile.name = n.to_string();
                    changed = true;
                }
                if let Some(a) = cmd["about"].as_str() {
                    let a = a.trim();
                    if a.chars().count() > 500 {
                        return Err("bad_about".into());
                    }
                    profile.about = a.to_string();
                    changed = true;
                }
                if changed {
                    profile.rev += 1;
                    let saved = door::call(json!({"op": "save_profile",
                        "profile": {"name": profile.name, "about": profile.about}}));
                    if !door::ok(&saved) {
                        return Err(door::error(&saved));
                    }
                }
                client.share_profile(&profile.wire())?;
                Ok(json!({}))
            }
            "stop" => {
                after = After::Stop;
                Ok(json!({}))
            }
            "" => Err("missing_op".into()),
            _ => Err("unknown_op".into()),
        }
    })();
    let mut answer = match result {
        Ok(mut v) => {
            v["ok"] = json!(true);
            v
        }
        Err(e) => json!({"ok": false, "error": e}),
    };
    if !id.is_null() {
        answer["id"] = id;
    }
    (answer, after)
}
