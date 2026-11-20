// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The same door the app uses (`trair_ffi::api`), called directly. One
//! account per process, like one app window.

use serde_json::{Value, json};

/// One request to the door, one answer.
pub fn call(request: Value) -> Value {
    let bytes = serde_json::to_vec(&request).unwrap_or_default();
    serde_json::from_slice(&trair_ffi::api::handle(&bytes))
        .unwrap_or_else(|_| json!({"ok": false, "error": "internal"}))
}

/// Did it work?
pub fn ok(answer: &Value) -> bool {
    answer["ok"] == true
}

/// The error code of a failed answer.
pub fn error(answer: &Value) -> String {
    answer["error"].as_str().unwrap_or("internal").to_string()
}
