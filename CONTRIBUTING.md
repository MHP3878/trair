# Contributing to Trair

Thank you for your interest in Trair.

## Current status: external contributions are not accepted yet

Trair is licensed under AGPL-3.0-or-later. An additional permission for
distribution through Apple's App Store (AGPL section 7) is planned and is
currently under review. To make sure that this permission can cover the
whole code base, **pull requests from people outside the project are not
accepted until the licensing is final.** Issues and security reports are
welcome.

This section will be removed once `APP-STORE-EXCEPTION.md` is published.

## Rules for all changes

1. **Sign off every commit (DCO).** By adding a `Signed-off-by:` line you
   certify the Developer Certificate of Origin in the file `DCO`. Use
   `git commit -s`. The e-mail in the sign-off must match the commit
   author. A pseudonym and a GitHub no-reply address are fine. CI rejects
   commits without a matching sign-off.
2. **No secrets in the repository.** Never commit private keys, passwords,
   recovery keys, tokens, real user data or local configuration. CI runs a
   secret scanner. If a secret was ever pushed, treat it as compromised
   and replace it; deleting it from history is not enough.
3. **License header.** Every source file starts with
   `SPDX-License-Identifier: AGPL-3.0-or-later`. CI checks this.
4. **Tests.** Security-relevant behaviour needs an automated test. Do not
   describe a test as passing unless it actually ran.
5. **Dependencies.** Only free, AGPL-compatible licenses (see `deny.toml`).
   No GPL- or AGPL-licensed third-party code, so that the planned App Store
   permission can apply. Prefer fewer dependencies. After changing
   dependencies, run `python3 tools/third_party_licenses.py` to update
   `THIRD_PARTY_LICENSES.md`.
6. **Small steps.** One topic per pull request. Explain what changed and
   which files are affected.

## Reporting security issues

Please do not open a public issue for vulnerabilities. See `SECURITY.md`.
