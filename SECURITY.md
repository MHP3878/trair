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

## Rules for this repository

No private keys, passwords, recovery keys, tokens or user data are ever
committed. CI scans for secrets on every push.
