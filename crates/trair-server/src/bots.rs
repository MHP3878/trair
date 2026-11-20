// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Bots (M9, step 4a). A bot registers as one; until its owner vouched
//! for it (a certificate signed by the owner's account key, see
//! `trair_core::bots`), it may log in but not write: no messages, no
//! changes to chats. Its certificate is handed out with its account, so
//! everyone can check who it belongs to. Bots do not take part in
//! communities yet (step 4b brings the right "add bots").
//!
//! Stored per bot: its name, the owner, the certificate and the hash of
//! the one-time secret of its current request code. Only who brings that
//! secret can vouch (bot names and keys are public). When the owner's
//! account goes, its bots lose the certificate and fall silent.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use trair_core::bots::BotCert;

use crate::{ApiError, ApiResult, Server, Session, bad, internal};

/// Bots one account may vouch for.
pub const MAX_BOTS_PER_OWNER: i64 = 10;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS bots (
    user TEXT PRIMARY KEY,
    owner TEXT,
    cert TEXT,
    request TEXT
);
CREATE INDEX IF NOT EXISTS bots_owner ON bots (owner);
";

/// Is `user` a bot?
pub(crate) fn is_bot(c: &Connection, user: &str) -> rusqlite::Result<bool> {
    c.query_row("SELECT 1 FROM bots WHERE user = ?1", [user], |_| Ok(()))
        .optional()
        .map(|r| r.is_some())
}

/// Is `user` a bot nobody vouched for (yet)?
pub(crate) fn unconfirmed(c: &Connection, user: &str) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT 1 FROM bots WHERE user = ?1 AND cert IS NULL",
        [user],
        |_| Ok(()),
    )
    .optional()
    .map(|r| r.is_some())
}

/// What others learn about a bot with its account: `null` for people,
/// `{"cert": …}` for bots (`null` until its owner vouched).
pub(crate) fn info(c: &Connection, user: &str) -> rusqlite::Result<Value> {
    let row: Option<Option<String>> = c
        .query_row("SELECT cert FROM bots WHERE user = ?1", [user], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(match row {
        None => Value::Null,
        Some(cert) => json!({
            "cert": cert.and_then(|c| serde_json::from_str::<Value>(&c).ok()),
        }),
    })
}

/// An account goes: its own bot entry, and the certificates it gave.
pub(crate) fn forget(c: &Connection, user: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM bots WHERE user = ?1", [user])?;
    c.execute(
        "UPDATE bots SET owner = NULL, cert = NULL WHERE owner = ?1",
        [user],
    )?;
    Ok(())
}

/// Refuse what an unconfirmed bot may not do.
pub(crate) fn may_write(s: &Server, user: &str) -> Result<(), ApiError> {
    if unconfirmed(&s.db().conn, user).map_err(internal)? {
        return Err(ApiError(StatusCode::FORBIDDEN, "bot_unconfirmed"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ask {
    hash: String,
}

/// A bot announces the hash of the secret in its new request code. A new
/// one replaces the last; it is how a bot moves to another owner.
pub(crate) async fn ask(
    State(s): State<Arc<Server>>,
    me: Session,
    Json(r): Json<Ask>,
) -> ApiResult {
    if r.hash.len() != 64 || hex::decode(&r.hash).is_err() {
        return Err(bad("hash"));
    }
    let n = s
        .db()
        .conn
        .execute(
            "UPDATE bots SET request = ?2 WHERE user = ?1",
            params![me.user, r.hash],
        )
        .map_err(internal)?;
    if n == 0 {
        return Err(ApiError(StatusCode::FORBIDDEN, "not_a_bot"));
    }
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Vouch {
    cert: BotCert,
    /// The secret from the request code, hex.
    secret: String,
}

/// The owner hands in the certificate for one of its bots.
pub(crate) async fn vouch(
    State(s): State<Arc<Server>>,
    me: Session,
    Json(r): Json<Vouch>,
) -> ApiResult {
    let cert = r.cert;
    if cert.owner != me.user {
        return Err(ApiError(StatusCode::FORBIDDEN, "not_owner"));
    }
    let (owner_key, _, _) = crate::load_account(&s, &me.user)?;
    let (bot_key, _, _) = crate::load_account(&s, &cert.bot)
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "unknown_bot"))?;
    trair_core::bots::verify(&cert, &owner_key, Some(&cert.bot), Some(&bot_key))
        .map_err(|_| bad("bad_cert"))?;
    let db = s.db();
    let c = &db.conn;
    if is_bot(c, &me.user).map_err(internal)? {
        return Err(ApiError(StatusCode::FORBIDDEN, "bot_owner"));
    }
    let row: Option<(Option<String>, Option<String>)> = c
        .query_row(
            "SELECT owner, request FROM bots WHERE user = ?1",
            [&cert.bot],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let Some((owner, request)) = row else {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown_bot"));
    };
    let secret = hex::decode(&r.secret).map_err(|_| bad("bad_secret"))?;
    if request.as_deref() != Some(trair_core::bots::secret_hash(&secret).as_str()) {
        return Err(ApiError(StatusCode::FORBIDDEN, "bad_secret"));
    }
    match owner {
        Some(o) if o == me.user => {}
        _ => {
            let n: i64 = c
                .query_row(
                    "SELECT COUNT(*) FROM bots WHERE owner = ?1",
                    [&me.user],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if n >= MAX_BOTS_PER_OWNER {
                return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too_many_bots"));
            }
        }
    }
    let text = serde_json::to_string(&cert).map_err(internal)?;
    c.execute(
        "UPDATE bots SET owner = ?2, cert = ?3, request = NULL WHERE user = ?1",
        params![cert.bot, me.user, text],
    )
    .map_err(internal)?;
    Ok(Json(json!({"ok": true})))
}
