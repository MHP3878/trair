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

## Profiles between contacts (M4, step 3)

Code: `crates/trair-core/src/profiles.rs`, `share_profile`,
`profile_sent` and `got_profile` in `crates/trair-core/src/chats.rs`,
`profiles` in `crates/trair-ffi/src/chats.rs`; tests: `chats.rs` (three
profile tests), `tuer.rs` (d23), `widget_test.dart` (group "Profile an
Kontakte"), `server_native_test.dart` (two accounts get each other's
profile through the real server).

- **What travels:** the chosen name, glow colour, "about me", the profile
  picture and the badges from the vault that are valid now. Inside the
  normal MLS application message, so the server cannot tell a profile
  from a chat message.
- **Who gets it:** every active chat. With "Profil sichtbar für: Nur
  Kontakte" only chats with contacts. A stranger's request gets nothing
  until it is accepted. Each version once per chat; a chat is marked only
  after the server took the message. A profile already sent cannot be
  taken back by changing the setting later.
- **What is kept:** only the chat partner's own profile (not one from
  another member or from an own device), only if it passes the checks
  and only if its version number is higher than the stored one. Stored in
  the encrypted chat database.
- **Checks:** name 1 to 64 characters without control characters, "about
  me" up to 500, at most 16 badges of up to 2 KiB, no unknown fields. The
  picture must be a PNG of at most 96 KiB and 128 × 128 pixels; it is
  decoded with a 1 MiB memory limit and written anew as a plain RGBA PNG.
  Hidden chunks (text, location, colour profiles) do not survive, and
  the app's image decoder only ever sees a file the core wrote.
- **Badges** are checked each time the profiles are listed (they
  expire), against the account key the profile came with, which comes
  from the sender's MLS certificate. A badge copied from someone else is
  not shown.
- **Names:** the name you gave a contact stays the big one; the name they
  chose is shown smaller ("nennt sich …"), so nobody can pass as someone
  else by renaming. A chosen name that looks official without a team
  badge gets the warning.
- **Requests:** a stranger's profile is stored, but the app shows no
  picture, colour or chosen name until the request is accepted.
- **Message count:** a profile is not counted in the "messages missing"
  numbers, so a profile that never left the device does not look like a
  lost message.

## Friendship keys and vanishing chats

Code: `InviteCode` in `crates/trair-core/src/contacts.rs`, `create_invite`,
`invite_message`, `set_chat_end`, `sweep`, `got_control` and
`redeem_token` in `crates/trair-core/src/chats.rs`; tests: `chats.rs`
(three tests), `contacts.rs`, `tuer.rs` (d24), `widget_test.dart` (group
"Freundschaftsschlüssel und verschwindende Chats").

- **A friendship key** is a contact code with a random 128-bit token and
  an end time (10 minutes to 30 days). The account that made it keeps only
  the token's SHA-256 hash. Whoever uses the key adds the owner as a
  contact, starts a chat and sends the token inside it (an MLS control
  message, invisible to the server). The owner's core accepts it once,
  before the end, and only from the chat partner: that account becomes a
  contact with the key its devices show in the chat, and the chat opens
  without a request. Used a second time (forwarded), too late or with a
  made-up token, the chat stays a normal request.
- **The key is not secret from whoever holds it**: like a contact code it
  names the account. After the end it only works as a contact code
  (a request).
- **A vanishing chat** ends at a fixed time on both sides: a vanishing key
  sets it for the chat it opens; in any active chat either partner (or
  an own device) can set or clear it. Strangers with a request cannot.
  At the end each core deletes the chat's keys, messages and settings,
  and the contact that came with a vanishing key; this runs every 30
  seconds and whenever the chat database opens.
- **Limits:** deleting relies on the other side running Trair; nobody can
  stop screenshots or photos of the screen. Messages still waiting on the
  server expire there as usual. The end time is at most 30 days away.

## Groups (M5, step 2)

Code: `crates/trair-core/src/groups.rs` (the rules), `group_chats.rs`,
`mls.rs` (`change`, `create_trair_group`, the checks in `process` and
`join`), the server's `commit` in `crates/trair-server/src/{lib,db}.rs`;
tests: `groups.rs` (unit), `tests/gruppen.rs` (g01-g12, real MLS),
`server.rs` (s14-s16), `tuer.rs` (d25), `widget_test.dart` (group
"Gruppen").

- **The group state is part of the keys.** Name, owner, admins, guests,
  end time and "closed" are a JSON value in a private MLS group context
  extension (type 0xF7A1, listed as required). Every member's epoch keys
  depend on the group context, so members with a different state cannot
  read each other. It changes only by a commit.
- **Every device checks every change before following it**
  (`groups::check_change`): only owner and admins commit; admins remove
  members and guests, only the owner removes admins; only the owner names
  admins, hands the group on or closes it; an admin may step down; owner,
  admins and guests must be members, each account under one key; Trair
  uses no other proposal types. A change that breaks a rule is not
  followed. A welcome is taken only from an admin of the state it brings.
  The committer's own core runs the same check before anything is sent.
- **The server only lets owner and admins change a group.** Each change of
  a group carries `committers`, the account names of owner and admins.
  Once a group has such a list, the server takes changes only from those
  accounts (403 `not_allowed`). Without this, a member with a modified
  Trair could post a forbidden change: everyone would refuse it, but the
  server would hold it as the change of that epoch and nobody could change
  the group any more. The server learns which accounts administer a
  random group id (it already sees which devices get a group's messages);
  name, members, roles and texts stay encrypted. Old one-to-one chats
  without a list work as before; new ones name both accounts.
- **Members who are not admins never commit.** Leaving is a "leave"
  control message: the device deletes the group's keys at once, an
  admin's device removes the account's devices later (`tidy_group`). A
  member asks for the removal of its own withdrawn devices the same way.
  New keys after joining through the last-resort package go as an MLS
  update proposal, which the next admin commit carries. Owners hand the
  group on and admins step down (a commit) before they leave.
- **Strict mode in groups:** an admin vouches for whoever it adds, so
  members who are not contacts do not block sending. The account key each
  member had when first seen in the group is pinned for that group; if it
  changes (someone removed and re-added under the same name with another
  key), strict mode blocks the group until the user takes the new key. A
  verified contact under another key blocks as before. Contacts added by
  an admin must come with the key pinned for them, strict mode or not.
- **Group keys** (`trair3:`) name the admin who made them, carry a random
  128-bit token (the admin's device keeps only its hash) and an end time,
  and work for many people until the end or until taken back. The joiner
  opens a short join chat (kind "join" in the state) with that admin's
  devices and sends the token there; the admin's device checks it (made
  on this device, not expired, not taken back, the group still run by
  this account), takes the joiner's account key from the join chat and
  adds exactly that account's devices. A key this device never made gets
  no answer; a refused one gets a "join_denied". Joining needs the
  admin's Trair to run. Guests (`g` keys) are listed with an end time:
  from then on their messages are dropped by every device, their own
  device deletes the group, and an admin's device removes them.
- **Vanishing and closing:** an admin can set an end for everyone; the
  owner can close the group, which every device deletes on following the
  change. Like vanishing chats this relies on the other devices running
  Trair; nobody can stop screenshots.
- **Limits:** a malicious admin can still freeze or split a group (it is
  trusted by the owner); the server's list of administering accounts is
  forgotten with the group after the idle limit; adding an account's new
  device to existing groups and keeping one's own devices' contacts in
  step are not built yet.

## Communities (M8, step 3a)

Code: `crates/trair-server/src/communities.rs` (rights, rules, reports,
log, sync), `crates/trair-core/src/proofs.rs` (`OpenMessage`),
`crates/trair-core/src/open_chats.rs` (checking authors),
`crates/trair-ffi/src/communities.rs`, `crates/trair-team/src/lib.rs`
(`meldungen`, `erledigt`), `app/lib/community_state.dart`; tests:
`communities.rs` (unit, rights), `tests/communities.rs` (k01-k09, real
server), `tuer.rs` (d26), `werkzeug.rs` (w06), `widget_test.dart` (group
"Communities"), `server_native_test.dart` (two apps, real server).

- **Open channels are readable on the server, on purpose.** Moderation
  needs it (planning decision 9); the app says so on every channel and
  when founding. Closed channels are end-to-end encrypted (below).
  Direct chats and groups are untouched by this.
- **The server cannot forge.** Every message carries a signature of the
  sender's device over community, channel, account, device, time and
  text (`trair/open-message/v1`). The server checks it before storing and
  refuses a time more than 5 minutes off. Every reader's core checks it
  again against the sender's device list, signed by the account key, and
  says how far that key is known: a contact's pinned key, else the key
  first seen for that name (pinned in the reader's encrypted chat
  database), else "new". Another key for a known name is shown as a
  warning until the reader accepts it; a bad signature is struck through.
  What remains: for a name nobody has seen, the server could invent an
  account; only contacts are shown as trusted. The server can delete
  messages and leave things out.
- **The server decides every right.** The owner has all rights; everyone
  else those of "@alle" and of their roles. A member stands as high as
  their highest role. Nobody acts on someone as high or higher or on the
  owner, shapes a role at or above their own, or gives (or takes) a right
  they do not have. Channels can be limited to roles for reading and
  writing; managing channels sees everything. Members write only after
  accepting the current rules; changed rules must be accepted again.
  Messages from channels a member may not read are never sent to them,
  also not by the long poll.
- **Invitations** are `trairc:` plus 80 random bits; only the SHA-256 is
  stored. At most 30 days, optionally limited uses, revocable. Banned
  accounts cannot join with any code.
- **Reports.** A report on a message keeps a copy of what was written
  (so deleting the message does not delete the evidence). Reports in a
  community go to members with the right "Meldungen"; reports from a
  profile go to the Trair team only: the server shows them against a team
  badge that is valid for the asking account (`team_badge`, checked with
  the team keys), or the team tool reads them from the server's database.
  A reporter may attach the last messages of a direct chat; the dialog
  says that they leave the end-to-end encryption then. Text from reports
  reaches the team tool's terminal without control characters. Handled
  reports are deleted after 90 days, the moderators' log after 180.
- **Limits.** 30 messages per minute and 20 reports per hour per account;
  20 communities per owner, 100 channels, 50 roles, 10 000 members,
  4000 characters per message and rules text.
- **Deleting an account** removes its memberships, messages, bans and
  invitations. A community it owned goes to the member who stands highest
  (the earliest to join on a tie); with nobody left it is deleted with
  everything in it. The same happens when the server deletes an idle
  account.
- **Team keys:** until the real public team key is in `TEAM_KEYS`, nobody
  has a team badge, so the team inbox is empty in the app; the team tool
  works without it.

## Closed channels (M8, step 3b)

Code: `crates/trair-core/src/channel_chats.rs`, `groups.rs`
(`GroupKind::Channel`), `mls.rs` (signed texts inside the encryption),
`proofs.rs` (`trair/closed-message/v1`), `crates/trair-server/src/
communities.rs` (`channel_members`, evidence), `db.rs` (`commit`),
`crates/trair-ffi/src/groups.rs`, `app/lib/app_state.dart`
(`keepChannels`); tests: `kanaele.rs` (c01-c04, real MLS, with a forging
device), `groups.rs` (unit), `proofs.rs` (unit), `tests/communities.rs`
(k10-k12), `tests/server.rs` (s17), `tuer.rs` (d27), `widget_test.dart`
(group "Geschlossene Kanäle"), `server_native_test.dart` (three apps,
real server).

- **A closed channel is an MLS group** bound to its community and
  channel. The server never gets its texts: posting to or reading a closed
  channel through the community API is refused (`closed_channel`).
- **Keepers.** The MLS admins of the group are the channel's keepers: the
  owner and everyone with the right to manage channels, as equals. The
  server picks the group id when the channel is made and takes changes to
  that group only from the keepers as the community's rights say *now*;
  what a change claims about its committers does not count. A member who
  loses the right can no longer change the group, also not with an older
  client.
- **Admission is automatic and visible** (Julian's choice). The Trair of
  a keeper (whichever runs) lets in everyone the server lists as allowed
  to read, removes who may no longer, and keeps the keepers in the group
  up to date, after every change in the community. Every admission and
  removal is a note in the channel. Strict mode: a keeper vouches for the
  people it lets in (`allow`); a known contact or an own account under a
  different key is still refused.
- **Signed inside the encryption.** Every text carries a signature of
  the sender's device over community, channel, account, device, time and
  text, with its own context so it never passes for an open message (and
  the other way round). Receivers keep a text only if the signature holds
  against the sender's key in the MLS group; anything else is dropped.
- **History for newcomers** (Julian's choice): a keeper's Trair sends the
  last 50 texts, at most 256 KiB, to the newcomer's devices only. Each
  text is checked against its author's device in the channel; a text a
  keeper changed or invented is dropped. History from anyone but a keeper
  is ignored.
- **Deleting** a text for everyone: its author or a keeper, by its
  signature.
- **Reports** hand the moderators one text with its signature. The
  server checks it against the author's device list (also withdrawn
  devices) and stores the report only if it holds; a changed text is
  refused. The reporter's dialog says that this one text leaves the
  end-to-end encryption.
- **What remains:** the server sees who is in which channel and when
  something is sent, and could list an invented account as a reader; it
  would show up as a note in the channel. Who may write is enforced by
  the apps, not inside MLS: a modified client of a reader can write.
  Before a community is handed over, the old owner's Trair makes the new
  owner a keeper inside every closed channel. If the last keeper inside a
  channel loses the right some other way (the only keeper who is an admin
  is removed while no other keeper is in it), nobody can change that
  channel any more; it has to be made again. History holds only texts of authors who are still in the
  channel. At most 1000 members per channel.

## Bots (M9, step 4a)

Code: `crates/trair-core/src/bots.rs` (certificate, request code),
`profiles.rs` and `chats.rs` (certificate in the profile,
`check_bot`), `crates/trair-server/src/bots.rs`, `crates/trair-bot`
(the program), `crates/trair-ffi` (`bot_request`, `vouch_bot`,
`check_bot`), `app/lib/app_state.dart` (`vouchBot`, `_checkBots`);
tests: `bots.rs` (unit), `gruppen.rs` (a forging device, a swapped owner
key), `tests/bots.rs` (b01-b02, real server), `tuer.rs` (d28),
`trair-bot/tests/bot.rs` (t01-t02: real server, `trair-bot` in its own
process, the example bots in Python and Node.js), `widget_test.dart`
(group "Bots"), `server_native_test.dart` (app, core, server and
`trair-bot`).

- **A bot is an account of its own**, run by `trair-bot` on its owner's
  computer. It has the same encrypted vault, the same core and the same
  checks as the app (strict mode included); `trair-bot` only adds the
  network. The bot's own program, in any language, speaks JSON lines
  with `trair-bot` and never sees keys or the vault password (it is
  removed from the program's environment). There is no open port:
  only standard input and output.
- **The owner vouches** (Julian's choice "Bot + Besitzer"). The owner's
  account key signs the bot's account name, the bot's account key and
  the owner's name (`trair/bot-owner/v1`). A bot's name and key are
  public, so the request code also carries a one-time secret of 128 bits;
  the server stores only its SHA-256 and takes a certificate only with
  the matching secret. Each secret works once; a new code (from the
  bot's operator) replaces the last and is how a bot moves to another
  owner. At most 10 bots per owner; a bot cannot own bots.
- **Until vouched, a bot is silent:** it may log in and receive, but the
  server refuses its messages and chat changes (`bot_unconfirmed`). When
  the owner's account goes, its bots lose the certificate and fall
  silent again.
- **Everyone sees whose a bot is, without trusting the server:** the
  certificate travels in the bot's end-to-end profile. A receiver keeps
  such a profile only if the certificate names the sending account and
  its key. The app checks the owner's signature with the owner's key from
  the server; if the owner is a contact, that key must also be the
  contact's, so a server that swaps keys cannot make a bot look like
  someone's. The app shows "BOT" and "Bot von …"; a certificate that does
  not hold is shown as such. Before a chat request is accepted, only the
  server says whether its sender is a bot (shown as "laut Server").
- **No communities yet:** the server refuses every community operation
  from a bot. Step 4b brings the right "add bots".
- **What remains:** someone who automates a normal account is not marked;
  no messenger can prevent that. A bot reads everything in the chats and
  groups it is in, like any member. `trair-bot` talks only to a server on
  the same computer until TLS is done, like the app.

## Emergency PINs

Code: `crates/trair-core/src/notfall.rs`, `crates/trair-ffi/src/
notfall.rs`, `unlock` and `wipe_account` in `api.rs`; tests:
`crates/trair-core/tests/notfall.rs` (n01–n03), `tuer.rs` (d19–d22),
`widget_test.dart` (group "Notfall-PINs"), `server_native_test.dart`
(a PIN sends through the real server).

An emergency PIN is typed on the start screen instead of the password.
From outside it looks like a wrong password; in fact Trair deletes the
account on this computer.

- **What is deleted:** the vault, the chat database (and its journal)
  and the emergency files are overwritten and removed, as with "delete
  account". Afterwards, as each PIN is set up: the account leaves the
  start screen (its card goes too), or a new vault takes its place that
  no password opens, so the account stays listed and every password is
  "wrong".
- **A PIN that sends** first writes one message to a chosen chat, logs
  in, deletes the account on the server, then deletes everything here.
  If the server does not answer within 12 seconds, it deletes here all
  the same.
- **A PIN that only deletes cannot delete the server account**: it opens
  no key, so it cannot sign the deletion. The server forgets an account
  after 6 months without login.
- **Files:** every vault has `notfall.trair` with exactly 3 slots, all
  the same size, PIN or not: unused slots are random filler. Each slot is
  sealed with XChaCha20-Poly1305 under a key from Argon2id (same strength
  as the vault: 256 MiB, 4 passes) over the PIN; one Argon2id run per
  attempt tries all three slots. `notfall-daten.trair` holds what a
  sending PIN needs (account key, device key, chat database key, server
  address; no name, picture or other profile data), sealed with a random
  key that only sending slots and the vault know; without a sending PIN
  it is filler of the same size. Both files are wrapped with DPAPI on
  Windows, like the vault.
- **Strength:** a PIN that only deletes needs 6 characters; guessing it
  from a copy of the files gives away no key. A PIN that sends opens the
  keys, so it needs 12 characters, like the password. A PIN can never
  be the password, and the password cannot be changed to one of the
  PINs.
- **Hidden at an open Trair:** the settings show the PINs only after the
  password. Setting or removing one needs the password too. At most 3.
- **Limits:** someone who copied the files before the PIN was typed
  still has them; deleting cannot reach copies. Deleting on an SSD is not
  guaranteed to remove every physical trace (see "The local vault"). A
  message already sent cannot be taken back.

## Rules for this repository

No private keys, passwords, recovery keys, tokens or user data are ever
committed. CI scans for secrets on every push.
