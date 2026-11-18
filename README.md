# Trair

**Private communication.**

Trair wird ein Messenger, der sich so einfach bedienen lässt wie WhatsApp oder Discord, bei dem Sicherheit und Privatsphäre aber an erster Stelle stehen. Private Nachrichten sind Ende-zu-Ende-verschlüsselt mit MLS (RFC 9420). Der Server stellt nur zu und kann private Inhalte nicht lesen.

> **Status: Vorbereitung (Meilenstein M0).** Trair ist noch nicht benutzbar. Bitte nichts davon für echte Kommunikation verwenden.

## Grundsätze

- **Überprüfbar statt versprochen:** Sicherheitsaussagen gelten nur, wenn ein automatischer Test sie belegt.
- **Keine Geheimnisse im Code:** Die Sicherheit hängt nicht davon ab, dass der Quellcode geheim bleibt. Schlüssel entstehen auf den Geräten und verlassen sie nie.
- **Kostenlos und frei:** nur kostenlose, möglichst freie Werkzeuge. Lizenz: AGPL-3.0-or-later.
- **Kein Mitlesen durch Betreiber oder Admins:** Admin-Rechte regeln, wer dabei ist, nicht, wer mitlesen darf.

## Aufbau

| Ordner | Inhalt |
| --- | --- |
| `crates/trair-core` | Rust-Kern der App: Schlüssel, MLS, lokale Speicherung (ab M1) |
| `crates/trair-server` | Rust-Server: Konten, Zustellung, Communities (ab M3) |
| `tools/` | Prüfskripte für CI: Lizenzliste, Datei-Kopfzeilen, DCO |
| `.github/workflows/` | automatische Prüfungen bei jedem Push |

Die Flutter-App kommt mit Meilenstein M5 dazu.

## Prüfungen

Bei jedem Push laufen automatisch: Formatierung, Clippy, Tests, Lizenz- und Schwachstellencheck (`cargo-deny`), Geheimnis-Scanner (`detect-secrets`), aktuelle Drittanbieter-Lizenzliste, Lizenz-Kopfzeilen und DCO-Unterschriften.

Lokal ausführen:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny check
python3 tools/third_party_licenses.py --check
python3 tools/check_headers.py
bash tools/scan_secrets.sh
```

## Lizenz

Trair ist freie Software unter der GNU Affero General Public License, Version 3 oder neuer. Siehe `LICENSE` und `COPYING`.

---

## English summary

Trair is an end-to-end encrypted messenger built on MLS (RFC 9420), with a Rust core, a minimal-trust Rust server and a Flutter desktop app. It is in its earliest preparation stage and **not usable yet**. License: AGPL-3.0-or-later. Security reports: see `SECURITY.md`. Contributions: see `CONTRIBUTING.md` (not accepted yet).
