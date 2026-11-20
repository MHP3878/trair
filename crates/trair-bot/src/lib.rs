// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Trair bots (M9, step 4a). `trair-bot` runs one bot account on its
//! owner's computer: it keeps the keys in an encrypted vault (the same as
//! the app's), does all encryption and talks to the server. The bot's own
//! code, in any language, only reads and writes JSON lines (see
//! [`protocol`]).

pub mod client;
pub mod door;
pub mod net;
pub mod protocol;
