// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Terminal front of the team tool; the logic is in `lib.rs`.

use trair_team::{Console, Settings, run};
use zeroize::Zeroizing;

struct Terminal;

impl Console for Terminal {
    fn passphrase(&mut self, prompt: &str) -> std::io::Result<Zeroizing<String>> {
        // The input is not shown on screen.
        rpassword::prompt_password(prompt).map(Zeroizing::new)
    }

    fn say(&mut self, text: &str) {
        println!("{text}");
    }

    fn pause(&mut self) {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args, &mut Terminal, &Settings::real()));
}
