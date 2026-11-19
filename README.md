# Trair

**Private communication.**

Trair wird ein Messenger, der sich so einfach bedienen lässt wie WhatsApp oder Discord, bei dem Sicherheit und Privatsphäre aber an erster Stelle stehen. Private Nachrichten sind Ende-zu-Ende-verschlüsselt mit MLS (RFC 9420). Der Server stellt nur zu und kann private Inhalte nicht lesen.

> **Status: Krypto-Kern (M1), App mit echtem Tresor, Geräte im Kern (M2).** Konto, Launcher-Passwort, Sperre, Einstellungen und die Geräteliste sind echt und verschlüsselt (siehe `SECURITY.md`). Das Koppeln eines zweiten Geräts ist im Kern fertig und getestet; der Knopf in der App kommt mit dem Server (M3). Chats und Kontakte sind noch ausgedacht, weil es noch keinen Server gibt; jedes Fenster zeigt deshalb „Demo-Chats – nicht verschlüsselt“. Bitte noch nicht für echte Kommunikation verwenden.

## Grundsätze

- **Überprüfbar statt versprochen:** Sicherheitsaussagen gelten nur, wenn ein automatischer Test sie belegt.
- **Keine Geheimnisse im Code:** Die Sicherheit hängt nicht davon ab, dass der Quellcode geheim bleibt. Schlüssel entstehen auf den Geräten und verlassen sie nie.
- **Kostenlos und frei:** nur kostenlose, möglichst freie Werkzeuge. Lizenz: AGPL-3.0-or-later.
- **Kein Mitlesen durch Betreiber oder Admins:** Admin-Rechte regeln, wer dabei ist, nicht, wer mitlesen darf.

## Aufbau

| Ordner | Inhalt |
| --- | --- |
| `crates/trair-core` | Rust-Kern der App: Konten, Geräte und unterschriebene Geräteliste, Koppeln mit Code (SPAKE2), unterschriebene Abzeichen, MLS-Gruppen, strenger Modus, Tresor. Angriffstests: `tests/angriffe.rs`, `tests/koppeln.rs`, `tests/abzeichen.rs` |
| `crates/trair-team` | Team-Werkzeug: erzeugt den Trair-Team-Schlüssel auf dem eigenen PC und unterschreibt Team-Abzeichen (`trair-team neu`, `zeigen`, `abzeichen`) |
| `crates/trair-ffi` | Die schmale Tür zwischen App und Kern: vier C-Funktionen, JSON-Nachrichten, unter Windows DPAPI und Bildschirmfoto-Schutz |
| `crates/trair-server` | Server (M3): Konten, Anmeldung per Geräteschlüssel, Zustellung verschlüsselter Nachrichten ohne Absender, Kopplungs-Relay. Läuft vorerst nur lokal: `cargo run -p trair-server` |
| `app/` | Flutter-App für Windows im Pro-Look. Zeigt bis zur Anbindung an den Rust-Kern nur Demo-Daten (`lib/backend/demo_backend.dart`) |
| `tools/` | Prüfskripte für CI: Lizenzliste, Datei-Kopfzeilen, DCO. `make_branding.py` erzeugt Logo, Windows-Symbol und Symbol-Schrift neu |
| `.github/workflows/` | automatische Prüfungen bei jedem Push |

## Windows-Demo herunterladen

GitHub baut die App bei jedem Push. Unter **Actions → App → letzter grüner Lauf → Artifacts → Trair-Windows-Demo** liegt eine ZIP-Datei. Entpacken und `Trair.exe` starten (`trair_ffi.dll` muss daneben liegen, das ist der Sicherheitskern). Die Datei ist noch nicht signiert, deshalb warnt Windows beim ersten Start („Weitere Informationen → Trotzdem ausführen“).

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

## Koppeln ausprobieren (auf einem PC)

1. `trair-server.exe` starten (Download aus der CI: Artefakt `Trair-Server`). Er lauscht auf `http://127.0.0.1:8471`.
2. Trair starten, Konto anlegen, Einstellungen → Server → Verbinden, dann Geräte → Gerät koppeln.
3. Eine zweite Trair-Instanz mit eigenem Datenordner starten: in PowerShell `$env:TRAIR_DATA_DIR = "$env:TEMP\Trair-Zweitgeraet"; .\Trair.exe`. Dort „Ich habe schon ein Konto“ wählen und den Code eingeben.

## Lizenz

Trair ist freie Software unter der GNU Affero General Public License, Version 3 oder neuer. Siehe `LICENSE` und `COPYING`.

---

## English summary

Trair is an end-to-end encrypted messenger built on MLS (RFC 9420), with a Rust core, a minimal-trust Rust server and a Flutter desktop app. It is in its earliest preparation stage and **not usable yet**. License: AGPL-3.0-or-later. Security reports: see `SECURITY.md`. Contributions: see `CONTRIBUTING.md` (not accepted yet).
