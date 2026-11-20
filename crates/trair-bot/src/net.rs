// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The Trair server's HTTP interface, as the app uses it (see
//! `app/lib/core/server.dart`). Plain HTTP and only to this computer until
//! TLS is done, like the app.

use std::time::Duration;

use serde_json::{Value, json};

/// What the server answered. `status` 0: no answer (offline, timeout).
#[derive(Debug, Clone)]
pub struct Reply {
    /// HTTP status, 0 without an answer.
    pub status: u16,
    /// The JSON answer (`{}` if there was none).
    pub body: Value,
}

impl Reply {
    fn offline() -> Reply {
        Reply {
            status: 0,
            body: json!({}),
        }
    }

    /// 200 and `"ok": true`.
    pub fn ok(&self) -> bool {
        self.status == 200 && self.body["ok"] == true
    }

    /// The server's error code, else `offline` or `http_<status>`.
    pub fn error(&self) -> String {
        match self.body["error"].as_str() {
            Some(e) => e.to_string(),
            None if self.status == 0 => "offline".into(),
            None => format!("http_{}", self.status),
        }
    }
}

/// Check a server address: `http://127.0.0.1:PORT` (or `localhost`,
/// `[::1]`). Returns it without a trailing slash.
pub fn parse_server(text: &str) -> Option<String> {
    let rest = text.trim().strip_prefix("http://")?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = rest.rsplit_once(':')?;
    if !matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        return None;
    }
    let port: u16 = port.parse().ok().filter(|p| *p > 0)?;
    Some(format!("http://{host}:{port}"))
}

/// One device's connection to the server.
#[derive(Clone)]
pub struct Api {
    agent: ureq::Agent,
    base: String,
    /// The session token after logging in.
    pub token: Option<String>,
}

/// A recipient as the server wants it.
fn targets(ids: &Value) -> Value {
    Value::Array(
        ids.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter_map(|id| id.split_once(':'))
                    .map(|(user, device)| json!({"user": user, "device": device}))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

impl Api {
    /// A connection to `server` (see [`parse_server`]).
    pub fn new(server: &str) -> Option<Api> {
        let base = parse_server(server)?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(5)))
            .build()
            .into();
        Some(Api {
            agent,
            base,
            token: None,
        })
    }

    fn send(&self, method: &str, path: &str, body: Option<&Value>, wait: u64) -> Reply {
        let url = format!("{}{path}", self.base);
        let timeout = Some(Duration::from_secs(15 + wait));
        let auth = self.token.as_ref().map(|t| format!("Bearer {t}"));
        let result = match (method, body) {
            ("GET", _) => {
                let mut r = self.agent.get(&url);
                if let Some(a) = &auth {
                    r = r.header("authorization", a);
                }
                r.config().timeout_global(timeout).build().call()
            }
            (m, b) => {
                let mut r = match m {
                    "PUT" => self.agent.put(&url),
                    _ => self.agent.post(&url),
                };
                if let Some(a) = &auth {
                    r = r.header("authorization", a);
                }
                let r = r.config().timeout_global(timeout).build();
                r.send_json(b.cloned().unwrap_or_else(|| json!({})))
            }
        };
        let Ok(mut response) = result else {
            return Reply::offline();
        };
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        Reply { status, body }
    }

    /// Is the server there?
    pub fn health(&self) -> Reply {
        self.send("GET", "/v1/health", None, 0)
    }

    /// Register the account (`user_taken` if it is known already).
    pub fn register(&self, identity: &Value, bot: bool) -> Reply {
        self.send(
            "POST",
            "/v1/accounts",
            Some(&json!({
                "user": identity["user"],
                "account_key": identity["account_key"],
                "device_list": identity["device_list"],
                "bot": bot,
            })),
            0,
        )
    }

    /// Key and device list of an account (and its bot certificate).
    pub fn account(&self, user: &str) -> Reply {
        self.send("GET", &format!("/v1/accounts/{user}"), None, 0)
    }

    /// A login challenge for this device.
    pub fn challenge(&self, user: &str, device: &str) -> Reply {
        self.send(
            "POST",
            "/v1/login/challenge",
            Some(&json!({"user": user, "device": device})),
            0,
        )
    }

    /// Log in with the signed challenge.
    pub fn login(&self, user: &str, device: &str, challenge: &str, signature: &str) -> Reply {
        self.send(
            "POST",
            "/v1/login",
            Some(
                &json!({"user": user, "device": device, "challenge": challenge,
                "signature": signature}),
            ),
            0,
        )
    }

    /// Hand the server this account's device list.
    pub fn put_devices(&self, list: &Value) -> Reply {
        self.send("PUT", "/v1/devices", Some(&json!({"device_list": list})), 0)
    }

    /// How many key packages the server holds for this device.
    pub fn key_package_count(&self) -> Reply {
        self.send("GET", "/v1/keypackages", None, 0)
    }

    /// Upload key packages.
    pub fn upload_key_packages(&self, packages: &Value, last_resort: &Value) -> Reply {
        let mut body = json!({"packages": packages});
        if last_resort.is_string() {
            body["last_resort"] = last_resort.clone();
        }
        self.send("POST", "/v1/keypackages", Some(&body), 0)
    }

    /// Key packages of every device of `user`, as offers for the core.
    pub fn claim(&self, user: &str) -> Vec<Value> {
        let r = self.send(
            "POST",
            "/v1/keypackages/claim",
            Some(&json!({"user": user})),
            0,
        );
        if !r.ok() {
            return Vec::new();
        }
        r.body["packages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|p| {
                        json!({
                            "device": format!("{user}:{}", p["device"].as_str().unwrap_or("")),
                            "package": p["package"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Hand a change of a chat to the server.
    pub fn post_commit(&self, c: &Value) -> Reply {
        let mut body = json!({
            "group": c["chat"],
            "epoch": c["epoch"],
            "commit": c["commit"],
            "to": targets(&c["to"]),
            "welcome": c["welcome"],
            "welcome_to": targets(&c["welcome_to"]),
        });
        if !c["committers"].is_null() {
            body["committers"] = c["committers"].clone();
        }
        self.send("POST", "/v1/commits", Some(&body), 0)
    }

    /// Deliver an encrypted message (`to`: device ids `user:d-…`).
    pub fn send_message(&self, to: &Value, body: &str) -> Reply {
        self.send(
            "POST",
            "/v1/messages",
            Some(&json!({"to": targets(to), "body": body})),
            0,
        )
    }

    /// Wait up to `wait` seconds for messages.
    pub fn fetch(&self, wait: u64) -> Reply {
        self.send("GET", &format!("/v1/messages?wait={wait}"), None, wait)
    }

    /// The server may delete these messages now.
    pub fn ack(&self, ids: &[i64]) -> Reply {
        self.send("POST", "/v1/messages/ack", Some(&json!({"ids": ids})), 0)
    }

    /// As a bot: the hash of the secret in a new request code.
    pub fn bot_request(&self, hash: &str) -> Reply {
        self.send("POST", "/v1/bots/request", Some(&json!({"hash": hash})), 0)
    }

    /// As an owner: vouch for a bot.
    pub fn vouch(&self, cert: &Value, secret: &str) -> Reply {
        self.send(
            "POST",
            "/v1/bots/vouch",
            Some(&json!({"cert": cert, "secret": secret})),
            0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_server_on_this_computer() {
        assert_eq!(
            parse_server("http://127.0.0.1:8471/").as_deref(),
            Some("http://127.0.0.1:8471")
        );
        assert_eq!(
            parse_server(" http://localhost:9 ").as_deref(),
            Some("http://localhost:9")
        );
        assert_eq!(
            parse_server("http://[::1]:8471").as_deref(),
            Some("http://[::1]:8471")
        );
        for bad in [
            "https://127.0.0.1:8471",
            "http://example.org:8471",
            "http://127.0.0.1",
            "http://127.0.0.1:0",
            "http://user@127.0.0.1:8471",
            "http://127.0.0.1:8471/pfad",
            "127.0.0.1:8471",
        ] {
            assert_eq!(parse_server(bad), None, "{bad}");
        }
    }

    #[test]
    fn targets_are_split_at_the_first_colon() {
        assert_eq!(
            targets(&json!(["bob:d-1", "kaputt", "ada:d-2"])),
            json!([{"user": "bob", "device": "d-1"}, {"user": "ada", "device": "d-2"}])
        );
    }
}
