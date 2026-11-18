# Testdaten

Tresor-Dateien, wie Paket 1 (Version 1 des Tresors, vor den Geräten) sie
geschrieben hat. `tests/geraete.rs` und `crates/trair-ffi/tests/tuer.rs`
prüfen damit, dass alte Tresore beim Öffnen sauber umziehen.

- `vault-v1-cheap.bin`: billige Argon2-Einstellungen, nur für schnelle Tests.
- `vault-v1-real.bin`: echte Einstellungen (256 MiB, 4 Durchläufe).
- `vault-v1.password`: das Passwort beider Dateien.
- `*.fingerprint`: Fingerabdruck des Kontos darin, zum Vergleich.

Das Konto darin ist ein Wegwerf-Konto, das nur für diese Tests erzeugt
wurde. Es war nie auf einem Server und gehört niemandem. Echte Schlüssel,
Passwörter oder Nutzerdaten kommen nie in dieses Repository.
