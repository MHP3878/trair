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

## The server (M3, step 2)

Code: `crates/trair-server`, `crates/trair-core/src/proofs.rs`; tests:
`crates/trair-server/tests/server.rs` (real Trair clients against the
server: registration, login, MLS chat, pairing through the relay,
withdrawal, clean-up, limits).

- **No passwords on the server.** A device logs in by signing a random
  challenge (60 seconds, single use, only for the device it was made
  for). Only devices on the account's current signed device list can log
  in. Session tokens last 12 hours and are stored only as SHA-256 hashes.
- **Accounts** are registered with the public account key and a device
  list signed by it. Device lists only move forward; when a device leaves
  the list, its sessions, key packages and waiting messages are deleted
  at once. Deleting an account needs a fresh signature of the account
  key, not only a session.
- **Messages** are MLS ciphertext, stored without sender, deleted when
  the device acknowledges them, at the latest after 30 days. Accounts
  unused for 6 months are deleted with all their data. SQLite runs with
  `secure_delete`, so deleted rows are overwritten.
- **Pairing relay:** the server hands out a channel number; the new
  device (no account yet) gets exactly one attempt per channel; channels
  expire after 10 minutes; only the owner reads the guest's messages.
  The relay only sees SPAKE2 messages and ciphertext.
- **Limits:** 256 KiB per message, 1 MiB per request, 10,000 waiting
  messages per device, 100 key packages per device, registrations,
  login challenges and pairing attempts per IP address and hour. IP
  addresses are kept only in memory for these limits, never stored.
- In this step the server only listens on 127.0.0.1 / ::1; other
  addresses are refused until TLS is built in.

Known limits of the server:

- It sees metadata: which device receives messages, from which account a
  session sends, and when. "Sealed sender" can reduce this later.
- A stranger who guesses a pairing channel number can use up its one
  attempt and so spoil that pairing (not take it over); the person then
  starts a new code.
- The database is not encrypted at rest; it holds public keys, hashes
  and ciphertext.

## The app and the server (M3, step 3)

Code: `app/lib/core/server.dart`, `app/lib/app_state.dart`, the server
ops in `crates/trair-ffi/src/api.rs`; tests: `crates/trair-ffi/tests/
tuer.rs` (d12 to d14), `app/test/widget_test.dart` (group "Server und
Koppeln") and `app/test/server_native_test.dart` (two devices pair
through the real server with the real core).

- The app only sends; the core computes. Login signatures, pairing keys
  (SPAKE2 state) and the new device's vault never leave the Rust core.
  The app sees the code it has to show, public data and ciphertext.
- Until the server has TLS, the app only talks to a server on the same
  computer (`http://127.0.0.1`, `localhost` or `[::1]`) and refuses any
  other address before sending anything.
- The session token lives only in memory and is gone when Trair locks.
- A device list from the server is taken only if the account signed it,
  it is newer, and this device is still on it. If another device
  withdrew this one, the app says so plainly.
- Deleting the account signs the server deletion first, deletes the
  vault with the password, and only then sends the deletion, so a wrong
  password never deletes the server side.

## Contacts and chats on the device (M4, step 1)

Code: `crates/trair-core/src/chats.rs`, `contacts.rs`, `store.rs`;
tests: `crates/trair-core/tests/chats.rs` (real core, every device with
its own database file), `tests/tresor.rs` (t16, t17).

- **Chat database:** SQLCipher 4 (whole file encrypted: pages, indexes,
  journal; HMAC per page). The key is 32 random bytes from the vault, so
  the database opens only after the vault password, and changing the
  password does not touch it. A wrong key opens nothing; a database of
  another device or account is refused. Deleted rows are overwritten
  (`secure_delete`), temporary data stays in memory. SQLCipher's crypto
  is OpenSSL 3, built from source with Trair (no system library).
- **What is stored:** contacts (user name, pinned key fingerprint, name),
  chats, messages, and the MLS state (group keys, private key-package
  keys). Every change is stored in one transaction *before* anything is
  sent, and an incoming message is stored before the server may delete
  it. A message that arrives twice is noticed (hash of the ciphertext).
- **Contacts only by code:** no phone book, no search. A contact code is
  `trair1:<user>:<key fingerprint>:<check>`. Adding a contact pins that
  account key; devices signed by any other key are refused (adding them,
  and following a chat that contains them). A code protects only as well
  as the way it was passed on: in person or by QR code it cannot be
  swapped. "Confirmed" marks a fingerprint compared in person.
- **Requests:** a chat started by someone who is not a contact is a
  request. Nothing can be sent into it until it is accepted (accepting
  pins the key its devices carry). Declining deletes the chat, its keys
  and its messages on this device; later messages for it are refused.
  A chat set up by one of the account's own devices (signature checked)
  is not a request.
- **Only the two accounts:** a one-to-one chat takes only devices of the
  two accounts; group chats are refused until M5.
- **One change at a time:** every chat has a random id. A change (devices
  added or removed) is a commit for one epoch; the server takes only the
  first commit per chat and epoch. The loser throws its commit away,
  reads the winner and tries again. A change waiting for the server's
  answer stays on disk (outbox); after a crash it is sent again, and the
  server's hash of the commit it took tells whether it was this one.
- **Withdrawn devices:** nothing is sent into a chat that still holds a
  withdrawn device; it has to be removed first. A device removed from a
  chat deletes its keys for it and keeps only the history.

Known limits:

- Accepting a request, confirming a contact or a name are not yet passed
  on to the account's other devices.
- Message length is limited to 32 KiB of text; no files or pictures.

## Chats through the server and the door (M4, step 2a)

Code: `crates/trair-server/src/lib.rs` (`/v1/commits`, key packages),
`crates/trair-ffi/src/chats.rs`; tests: `crates/trair-server/tests/
server.rs` (s10 to s13, two real chat databases through the server),
`crates/trair-ffi/tests/tuer.rs` (d15, d16).

- **Order of changes:** `POST /v1/commits` takes a commit only if it is
  the first for its chat and epoch, and then queues it for the chat's
  devices and the welcome for new devices, all in one transaction. A
  refused commit gets the server's epoch and the SHA-256 of the commit
  it took. The server stores per chat only the random id, the next epoch,
  that hash and the time of the last change, not who is in the chat;
  chats unchanged for 6 months are forgotten.
- **Key packages:** besides one-time packages each device keeps one
  last-resort package on the server. It is handed out when the one-time
  packages are gone and is not deleted, so a person can always be
  reached; the core then renews its keys in that chat. The app asks how
  many one-time packages are left and uploads more.
- **The door:** the chat database opens with the vault and closes when
  Trair locks; deleting the account deletes it. The app gets texts, names
  and ciphertext to send, never keys. A contact's device list is taken
  only if the account key fits the pinned fingerprint and signed it; its
  withdrawn devices are refused from then on.

Known limits:

- A chat partner (not a stranger) can block further changes of a chat by
  sending a commit that the others refuse: the server's epoch moves on,
  theirs does not. Messages still flow; a new chat helps. A dishonest
  server can block anything anyway.
- The server sees which devices get which ciphertext and when, and which
  account sent it (sealed sender: later).

## Real chats in the app (M4, step 2b)

Code: `app/lib/app_state.dart` (chats section), `app/lib/screens/
chats_screen.dart`, `app/lib/core/server.dart`; tests: `app/test/
widget_test.dart` (group "Echte Chats") and `app/test/server_native_test
.dart` (two accounts chat through the real server with the real core).

- The app only moves what the core made: codes, ciphertext, commits.
  Texts it shows come from the encrypted chat database.
- After connecting, the app uploads 20 one-time key packages when fewer
  than 10 are left, and one
  last-resort package on the server, sends waiting changes again, checks
  contacts' device lists and removes withdrawn devices from chats.
- Receiving: long poll; each message is processed and stored by the core
  before the server is told to delete it. Messages the core could not
  store (disk full, locked) stay on the server.
- Real chats and demo chats are shown apart; demo chats stay marked
  "nicht verschlüsselt".

Known limits:

- A message that was stored but could not be delivered (server away) is
  not sent again by itself yet; the app says so.
- A newly paired device sees new chats, not the existing ones.

## Several accounts on one computer

Code: `app/lib/core/accounts.dart`, `init` in `crates/trair-ffi/src/
api.rs`; tests: `tuer.rs` (d17), `widget_test.dart` (group "Mehrere
Konten").

- Each account has its own folder (the data folder, then `konten/2`,
  `konten/3`, ...) with its own vault, chat database and keys. Accounts
  share nothing.
- The start screen lists the accounts before any password. What it shows
  lies **unencrypted** in `karte.json` and `karte.png` of each folder:
  name, colour, profile picture (128 × 128). An account can switch this
  off in its settings; then only "Konto N" is stored and shown. Anyone
  with access to the user's files can see how many accounts there are.
- The core locks an account's folder for its process (`trair.lock`); a
  second Trair window cannot open the same account at the same time.

## Rules for this repository

No private keys, passwords, recovery keys, tokens or user data are ever
committed. CI scans for secrets on every push.
