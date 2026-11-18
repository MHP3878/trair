// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Trair server.
//!
//! Milestone M0 only provides the project skeleton. Delivery, accounts and
//! key packages follow in milestone M3.

fn banner() -> String {
    format!(
        "trair-server {} - {} (not ready for use)",
        trair_core::VERSION,
        trair_core::SLOGAN
    )
}

fn main() {
    println!("{}", banner());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_mentions_version_and_status() {
        let b = banner();
        assert!(b.contains(trair_core::VERSION));
        assert!(b.contains("not ready for use"));
    }
}
