#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
"""Prueft, dass jede Quelldatei die Lizenz-Kopfzeile traegt.

Abschnitt 7 der AGPL verlangt spaeter Hinweise auf Zusatzbedingungen in
den Quelldateien; die einheitliche Kopfzeile ist die Grundlage dafuer.
"""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
HEADER = "SPDX-License-Identifier: AGPL-3.0-or-later"
CHECKED = {".rs", ".py", ".toml", ".yml", ".yaml", ".dart", ".sh"}
SKIP = {"Cargo.lock"}


def tracked_files():
    try:
        out = subprocess.run(["git", "ls-files"], cwd=ROOT, check=True,
                             capture_output=True, text=True).stdout
        return [ROOT / f for f in out.splitlines()]
    except (subprocess.CalledProcessError, FileNotFoundError):
        return [p for p in ROOT.rglob("*") if p.is_file() and "target" not in p.parts]


def main():
    missing = []
    for path in tracked_files():
        if path.suffix not in CHECKED or path.name in SKIP or not path.exists():
            continue
        head = path.read_text(encoding="utf-8", errors="replace").splitlines()[:5]
        if not any(HEADER in line for line in head):
            missing.append(path.relative_to(ROOT))
    if missing:
        print("Lizenz-Kopfzeile fehlt in:")
        for m in missing:
            print(f"  {m}")
        return 1
    print("Alle Quelldateien haben die Lizenz-Kopfzeile.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
