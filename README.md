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
| `crates/trair-core` | Rust-Kern der App: Konten, Geräte und unterschriebene Geräteliste, Koppeln mit Code (SPAKE2), unterschriebene Abzeichen, MLS-Gruppen, strenger Modus, Tresor, Kontakte per Code und Chats in einer verschlüsselten Datenbank (SQLCipher). Angriffstests: `tests/angriffe.rs`, `tests/koppeln.rs`, `tests/abzeichen.rs`, `tests/chats.rs` |
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

## Echt chatten ausprobieren (zwei Konten auf einem PC)

1. `trair-server.exe` starten.
2. Trair starten, Konto anlegen, Einstellungen → Server → Verbinden.
3. Ein zweites Trair-Fenster öffnen (Trair.exe noch einmal starten). Auf dem Startbildschirm „Neues Konto erstellen“ wählen und ebenfalls verbinden. Ein Konto kann immer nur in einem Fenster offen sein.
4. In beiden Fenstern oben auf „Neuer Chat“: den eigenen Kontaktcode kopieren und im anderen Fenster einfügen.
5. „Chat starten“, schreiben. Die Nachrichten laufen verschlüsselt über den Server; er sieht nur Geheimtext.

Sobald ein Chat läuft, bekommt das andere Konto automatisch dein Profil: Name, Leuchtfarbe, Profilbild, „Über mich“ und deine Abzeichen, verschlüsselt durch den Chat. Groß steht immer der Name, den *du* dem Kontakt gegeben hast; seinen selbst gewählten Namen zeigt Trair klein darunter. Fremde mit einer Anfrage sehen dein Profil erst, wenn du annimmst, und du ihres auch erst dann.

**Freundschaftsschlüssel:** Unter „Neuer Chat“ einen Schlüssel erstellen, der 10 Minuten bis 7 Tage gilt und nur für eine Person. Wer ihn rechtzeitig einfügt, ist sofort Kontakt und der Chat startet ohne Anfrage. Mit „Verschwindender Chat“ verschwindet der Chat zur selben Zeit bei beiden; in jedem Chat lässt sich das auch oben über „…“ einstellen.

**Gruppen:** Unter „Neuer Chat“ → „Neue Gruppe“: Name, Kontakte auswählen, wahlweise „Nach 1 Tag weg“ (Eintagsgruppe). Du bist Besitzer. Über „Gruppe verwalten“ siehst du alle Mitglieder mit Rolle (Krone = Besitzer, Schild = Admin, Uhr = Gast), machst Leute zu Admins, entfernst sie, benennst um, lässt die Gruppe verschwinden oder löst sie für alle auf. Mit einem Einladungsschlüssel (`trair3:…`, 1 Std. bis 7 Tage, mehrfach nutzbar, zurückziehbar) kommen auch Leute dazu, die keine Kontakte sind; als Gäste gehen sie am Ende wieder. Dein Trair prüft jeden Schlüssel und fügt die Person dann hinzu, dafür muss es laufen. Nur Besitzer und Admins können eine Gruppe ändern; das prüft jedes Trair selbst und auch der Server. Details: [SECURITY.md](SECURITY.md#groups-m5-step-2).

**Communities:** Mit dem Server verbunden, links auf „+“: Community gründen (du wirst Gründer, mit den Rollen Admin und Moderator und den Kanälen #regeln und #allgemein) oder mit einem Einladungscode (`trairc:…`) beitreten. Die Kanäle sind offen: Der Server kann sie lesen, damit Moderation geht; jede Nachricht ist aber vom Gerät unterschrieben, fälschen kann der Server sie nicht (Haken neben dem Namen, Warnung bei fremdem Schlüssel). Neue Mitglieder schreiben erst, wenn sie die Regeln akzeptiert haben. Über das Zahnrad: Beschreibung und Regeln, Rollen mit Farben und Rechten, Kanäle (für alle, nur lesen oder nur für bestimmte Rollen), Einladungen, Banns, Meldungen und Protokoll – je nachdem, welche Rechte du hast. Mitglied anklicken: Rollen geben, entfernen, bannen, melden oder die Community übergeben. „Melden“ an einer Nachricht geht an die Moderatoren der Community, „Melden“ im Profil an das Trair-Team. Details: [SECURITY.md](SECURITY.md#communities-m8-step-3a).

**Team-Meldungen:** Wer ein gültiges Trair-Team-Abzeichen hat, sieht sie in den Einstellungen. Auf dem Server-PC geht es auch ohne App: `trair-team meldungen --daten trair.db` und `trair-team erledigt --nummer 3 --daten trair.db`.

## Notfall-PINs

Einstellungen → Notfall-PINs → Passwort eingeben → „Anzeigen“. Bis zu 3 PINs. Eine PIN tippst du auf dem Startbildschirm statt des Passworts ein: Trair löscht dann das Konto mit allen Nachrichten auf diesem PC, nach außen sieht es aus wie ein falsches Passwort. Pro PIN einstellbar: nur löschen oder vorher eine Nachricht an einen Chat schicken (diese PIN braucht 12 Zeichen), und ob das Konto danach aus der Liste verschwindet oder stehen bleibt und jedes Passwort als falsch gilt. Nicht zum Ausprobieren am echten Konto: Sie löscht wirklich. Details: [SECURITY.md](SECURITY.md#emergency-pins).

## Lizenz

Trair ist freie Software unter der GNU Affero General Public License, Version 3 oder neuer. Siehe `LICENSE` und `COPYING`.

---

## English summary

Trair is an end-to-end encrypted messenger built on MLS (RFC 9420), with a Rust core, a minimal-trust Rust server and a Flutter desktop app. It is in its earliest preparation stage and **not usable yet**. License: AGPL-3.0-or-later. Security reports: see `SECURITY.md`. Contributions: see `CONTRIBUTING.md` (not accepted yet).
