#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
"""Prueft die DCO-Unterschrift jedes Commits in einem Bereich.

  python3 tools/check_dco.py <bereich>   z. B. origin/main..HEAD oder ein einzelner Commit

Jeder Commit (ausser Merge-Commits) braucht eine Zeile
"Signed-off-by: Name <email>", deren E-Mail zur Autor-Adresse passt.
"""
import re
import subprocess
import sys

SIGNOFF = re.compile(r"^Signed-off-by: .+ <([^>]+)>\s*$", re.MULTILINE)


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    commits = git("rev-list", "--no-merges", sys.argv[1]).split()
    bad = []
    for c in commits:
        author = git("log", "-1", "--format=%ae", c).strip().lower()
        body = git("log", "-1", "--format=%B", c)
        signed = {m.lower() for m in SIGNOFF.findall(body)}
        if author not in signed:
            subject = git("log", "-1", "--format=%s", c).strip()
            bad.append(f"{c[:10]} {subject} (Autor: {author}, Signed-off-by: {sorted(signed) or 'fehlt'})")
    if bad:
        print("Commits ohne passende DCO-Unterschrift:")
        for b in bad:
            print(f"  {b}")
        print("Loesung: git commit --amend -s (bzw. git rebase --signoff)")
        return 1
    print(f"DCO ok: {len(commits)} Commit(s) geprueft.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
