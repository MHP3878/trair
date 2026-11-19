// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! `trair-server [--adresse 127.0.0.1:8471] [--daten data/trair.db]`
//!
//! Step 2 of M3 runs only on this computer: addresses other than
//! loopback are refused until TLS is built in. The database file lies
//! under `data/`, which Git ignores.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use trair_server::{Db, Server, router, system_clock};

fn main() {
    if let Err(e) = run() {
        eprintln!("trair-server: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut address: SocketAddr = ([127, 0, 0, 1], 8471).into();
    let mut data = PathBuf::from("data").join("trair.db");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it.next().ok_or(format!("bei {flag} fehlt der Wert"))?;
        match flag.as_str() {
            "--adresse" => {
                address = value
                    .parse()
                    .map_err(|_| format!("Adresse {value} ist ungültig"))?
            }
            "--daten" => data = PathBuf::from(value),
            other => return Err(format!("unbekannte Angabe {other}")),
        }
    }
    if !address.ip().is_loopback() {
        return Err(
            "nur 127.0.0.1 oder ::1: ohne TLS ist der Server nicht fürs Netz gedacht".into(),
        );
    }
    if let Some(dir) = data.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let db = Db::open(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    let server = Server::new(db, system_clock());
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|e| format!("{address}: {e}"))?;
        println!(
            "trair-server {} läuft auf http://{address} (Daten: {})",
            trair_core::VERSION,
            data.display()
        );
        let cleaner = server.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(3600));
            loop {
                every.tick().await;
                if let Err(e) = cleaner.cleanup() {
                    eprintln!("Aufräumen fehlgeschlagen: {e}");
                }
            }
        });
        axum::serve(
            listener,
            router(server).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            // Stop on Ctrl+C. If the signal cannot be watched, keep
            // running instead of stopping at once.
            if tokio::signal::ctrl_c().await.is_err() {
                std::future::pending::<()>().await;
            }
        })
        .await
        .map_err(|e| e.to_string())
    })
}
