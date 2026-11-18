# Trair

**Private communication.**

Trair wird ein Messenger, der sich so einfach bedienen lässt wie WhatsApp oder Discord, bei dem Sicherheit und Privatsphäre aber an erster Stelle stehen. Private Nachrichten sind Ende-zu-Ende-verschlüsselt mit MLS (RFC 9420). Der Server stellt nur zu und kann private Inhalte nicht lesen.

> **Status: Krypto-Kern (M1) und App-Oberfläche mit Demo-Daten.** Die App verschlüsselt noch nichts und verbindet sich mit keinem Server; jedes Fenster zeigt das Schild „Demo – nicht verschlüsselt“. Bitte nichts davon für echte Kommunikation verwenden.

## Grundsätze

- **Überprüfbar statt versprochen:** Sicherheitsaussagen gelten nur, wenn ein automatischer Test sie belegt.
- **Keine Geheimnisse im Code:** Die Sicherheit hängt nicht davon ab, dass der Quellcode geheim bleibt. Schlüssel entstehen auf den Geräten und verlassen sie nie.
- **Kostenlos und frei:** nur kostenlose, möglichst freie Werkzeuge. Lizenz: AGPL-3.0-or-later.
- **Kein Mitlesen durch Betreiber oder Admins:** Admin-Rechte regeln, wer dabei ist, nicht, wer mitlesen darf.

## Aufbau

| Ordner | Inhalt |
| --- | --- |
| `crates/trair-core` | Rust-Kern der App: Konten, Geräte-Zertifikate, MLS-Gruppen, strenger Modus. Die Angriffstests liegen in `tests/angriffe.rs` |
| `crates/trair-server` | Rust-Server: Konten, Zustellung, Communities (ab M3) |
| `app/` | Flutter-App für Windows im Pro-Look. Zeigt bis zur Anbindung an den Rust-Kern nur Demo-Daten (`lib/backend/demo_backend.dart`) |
| `tools/` | Prüfskripte für CI: Lizenzliste, Datei-Kopfzeilen, DCO |
| `.github/workflows/` | automatische Prüfungen bei jedem Push |

## Windows-Demo herunterladen

GitHub baut die App bei jedem Push. Unter **Actions → App → letzter grüner Lauf → Artifacts → Trair-Windows-Demo** liegt eine ZIP-Datei. Entpacken und `Trair.exe` starten. Die Datei ist noch nicht signiert, deshalb warnt Windows beim ersten Start („Weitere Informationen → Trotzdem ausführen“).

## Prüfungen

Bei jedem Push laufen automatisch: Formatierung, Clippy, Tests (darunter die Angriffstests aus dem Prototyp), Lizenz- und Schwachstellencheck (`cargo-deny`), Geheimnis-Scanner (`detect-secrets`), aktuelle Drittanbieter-Lizenzliste, Lizenz-Kopfzeilen, DCO-Unterschriften und die statische Sicherheitsanalyse CodeQL. Für die App außerdem `flutter analyze`, die Widget-Tests, `dart format` und der Windows-Build.

Lokal ausführen:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny check
python3 tools/third_party_licenses.py --check
python3 tools/check_headers.py
bash tools/scan_secrets.sh

cd app
flutter analyze
flutter test
dart format --output=none --set-exit-if-changed .
```

## Lizenz

Trair ist freie Software unter der GNU Affero General Public License, Version 3 oder neuer. Siehe `LICENSE` und `COPYING`.

---

## English summary

Trair is an end-to-end encrypted messenger built on MLS (RFC 9420), with a Rust core, a minimal-trust Rust server and a Flutter desktop app. It is in its earliest preparation stage and **not usable yet**. License: AGPL-3.0-or-later. Security reports: see `SECURITY.md`. Contributions: see `CONTRIBUTING.md` (not accepted yet).
