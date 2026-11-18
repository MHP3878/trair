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

## Rules for this repository

No private keys, passwords, recovery keys, tokens or user data are ever
committed. CI scans for secrets on every push.
