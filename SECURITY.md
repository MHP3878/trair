# Security policy

## Status

Trair is in an early preparation stage. **It is not ready for real
communication.** No release is supported yet.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub:
**Security → Report a vulnerability** in this repository.

Do not open a public issue for security problems. Please include:

- what is affected (file, function, protocol step),
- how to reproduce it,
- what an attacker could achieve.

There is no bug bounty; Trair is a volunteer project without a budget.
You will get an answer as soon as possible, and you will be credited in
the fix unless you prefer not to be.

## Scope and threat model

Trair aims to protect private messages against:

- the server operator and anyone with access to the server,
- group administrators and moderators (they manage membership, they
  cannot read private content),
- network attackers.

Known limits, documented on purpose:

- A contact whose key was never verified can be impersonated by a
  malicious server; Trair warns and, in strict mode (default), blocks.
- The server necessarily learns some metadata (accounts, devices, group
  identifiers, recipients, timing, message sizes).
- Malware on an unlocked device can read everything on that device.

## The local vault (launcher password)

The account key and the profile are stored in one encrypted file,
`%APPDATA%\Trair\vault.trair` on Windows (code: `crates/trair-core/src/vault.rs`
and `crates/trair-ffi`).

- The key is derived from the launcher password with Argon2id (256 MiB,
  4 passes, about one second). The file is encrypted with
  XChaCha20-Poly1305; its header is authenticated, so any change to the
  file is noticed. Settings outside fixed limits are refused before any
  work is done (no downgrade, no memory exhaustion).
- On Windows the file is additionally encrypted with DPAPI for the
  current Windows user: a copy is useless on another PC or user account.
- Passwords need at least 12 characters (checked in the core too). After
  three wrong passwords the core makes you wait, doubling up to 5 minutes.
- Locking (button, Ctrl+L, auto-lock after 5, 15 or 30 minutes) drops the
  key from memory. Secret buffers in the core are overwritten when freed.
- The Trair window is hidden from screenshots and screen recording by
  default (Windows `SetWindowDisplayAffinity`).

Known limits of the vault:

- Someone who has the file *and* can log in as the same Windows user can
  try passwords offline; every guess costs about one second and 256 MiB.
- Strings in the Dart layer (the password while you type it) cannot be
  wiped reliably from memory.
- On SSDs the overwritten old file may survive in free blocks; it is
  encrypted with a key nobody knows after deletion.

## Devices and pairing (M2)

Code: `crates/trair-core/src/devices.rs`, `pairing.rs`; tests:
`tests/geraete.rs`, `tests/koppeln.rs`.

- Every device has its own signing key, stored in the vault. The account
  key signs the list of the account's devices (a running number, device
  ids and keys, withdrawn devices). Names given to devices stay in the
  local vault and never appear in the list.
- A new list must be signed by the account and carry a higher number;
  older or changed lists are refused, so a server cannot roll back a
  withdrawal.
- A withdrawn device is refused everywhere (receiving, sending, adding to
  a group), even with strict mode off. A device cannot withdraw itself.
  Withdrawing asks for the password again.
- Vaults made before devices existed get a device key and a first signed
  list on the first unlock and are stored again right away. The account
  and its fingerprint stay the same.
- Pairing is built and tested in the core; the button in the app comes
  with the server (M3). The unlocked device shows a code such as
  `7-anker-birne-kerze`: a channel number and three words out of 256
  (24 random bits). Both devices run SPAKE2 (Ed25519 group, symmetric)
  with the code. One key per direction is derived from the SPAKE2 key and
  a hash of both hello messages; everything else is encrypted with
  XChaCha20-Poly1305. The relay learns neither the code nor the content.
  It gets exactly one guess per code: after the first failed attempt the
  code is used up. Codes expire after 10 minutes.
- The new device receives the full account key (decision for M2), so it
  can add and withdraw devices itself and keeps the account alive if the
  first PC fails.

Known limits of devices and pairing:

- Withdrawing does not help against someone who has a device's vault
  *and* its password: they hold the account key. Then only a new account
  is safe.
- The SPAKE2 implementation (`spake2` 0.4.0, RustCrypto) has had no
  independent audit. It is on the list for the expert review before any
  release.
- One guess per code means a relay could guess right with a chance of
  1 in 16.7 million per pairing attempt; failed attempts are visible to
  the person pairing.

## Profiles and badges

Code: `crates/trair-core/src/badges.rs`; tests: `tests/abzeichen.rs`.

- A badge (Trair team, founder, admin, moderator) is never a word in a
  profile but a signed statement for one account key. The app only shows
  badges the core has checked; everything else is dropped.
- Team badges must be signed by a public team key built into the app.
  There is none yet: the project owner creates the team key on their own
  computer with `trair-team` (below), and only its public half is ever
  added to the code.
- A community's id is derived from its founder's key, so nobody else can
  claim to have founded it. Admins are signed by the founder; moderators
  by the founder or by an admin who attaches their own admin badge (one
  step only). Badges are shown everywhere; the tooltip always names the
  community they come from.
- Community badges expire after at most 14 days (the server will renew
  them, M3); team badges after at most 366 days.
- Own badges are kept in the vault and checked again on every unlock;
  expired or foreign ones are never shown.
- Names that look official ("Trair Support", also with look-alike letters
  such as `Tr4ir` or Cyrillic `а`) get a warning unless a team badge backs
  them. Community names that look official are marked as well.
- "Über mich" and who may see the profile are stored in the vault; the
  visibility setting takes effect with the server.

Known limits of badges:

- A removed admin or moderator keeps the badge until it expires.
- Look-alike detection covers common tricks, not every Unicode character.
- Profile pictures are not supported yet: images from strangers need a
  separate review of the image decoder first.

## The team key and `trair-team`

Code: `crates/trair-core/src/teamkey.rs`, `crates/trair-team`; tests:
`crates/trair-core/tests/teamschluessel.rs`, `crates/trair-team/tests`.

- `trair-team neu` makes the team key on the owner's computer. It is
  stored encrypted with a passphrase of at least 16 characters (Argon2id
  with 1 GiB and 3 passes, XChaCha20-Poly1305; header authenticated).
  Files with cheaper settings are refused when opened.
- The tool never overwrites a file and refuses folders inside a Git
  repository or synced to a cloud (OneDrive, Dropbox, Google Drive,
  iCloud). The default is `%LOCALAPPDATA%\Trair\Team`. `*.trairkey` is
  in `.gitignore`.
- The passphrase is read without echo and never printed. Backup: a copy
  of the file on a USB stick, the passphrase on paper.
- If the key is lost, a new one is made and the app is updated; old team
  badges stop working. If it is stolen together with the passphrase,
  its public key is removed from the app.

Known limit: while the tool runs, malware on the same computer could
read the key. The tool is needed about once a year.

## Rules for this repository

No private keys, passwords, recovery keys, tokens or user data are ever
committed. CI scans for secrets on every push.
