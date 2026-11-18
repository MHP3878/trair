// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! The narrow door between the Flutter app and the Rust core.
//!
//! The app sees exactly four C functions (see [`abi`]). Requests and
//! answers are small JSON messages; the whole protocol is in [`api`].
//! Secrets (the account key, the vault key) stay on this side. The app
//! only ever gets results: unlocked or not, the profile, the fingerprint.

pub mod abi;
pub mod api;
mod platform;
