#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
"""Erzeugt THIRD_PARTY_LICENSES.md aus den Rust-Abhaengigkeiten.

  python3 tools/third_party_licenses.py          # Datei neu schreiben
  python3 tools/third_party_licenses.py --check  # nur pruefen (fuer CI)

Nutzt nur die Python-Standardbibliothek und `cargo metadata`.
"""
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = ROOT / "THIRD_PARTY_LICENSES.md"


def collect():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
    )
    own = set(meta["workspace_members"])
    rows = []
    for p in meta["packages"]:
        if p["id"] in own:
            continue
        lic = p.get("license") or (
            "siehe " + p["license_file"] if p.get("license_file") else "KEINE ANGABE"
        )
        rows.append((p["name"], p["version"], lic, p.get("repository") or ""))
    return sorted(rows)


def render(rows):
    out = [
        "# Third-party licenses",
        "",
        "<!-- Automatisch erzeugt von tools/third_party_licenses.py. Nicht von Hand bearbeiten. -->",
        "",
        "Trair itself is licensed under AGPL-3.0-or-later (see LICENSE and COPYING).",
        "This file lists every third-party component and its license.",
        "",
        "## Rust",
        "",
    ]
    if not rows:
        out.append("No third-party Rust dependencies yet.")
    else:
        out += ["| Package | Version | License | Source |", "| --- | --- | --- | --- |"]
        out += [f"| {n} | {v} | {l} | {r} |" for n, v, l, r in rows]
    out += ["", "## Flutter / Dart", "", "Not part of the project yet (planned for milestone M5).", ""]
    return "\n".join(out)


def main():
    text = render(collect())
    if "--check" in sys.argv:
        current = TARGET.read_text(encoding="utf-8") if TARGET.exists() else ""
        if current != text:
            print("THIRD_PARTY_LICENSES.md ist veraltet. Bitte ausfuehren: python3 tools/third_party_licenses.py")
            return 1
        print("THIRD_PARTY_LICENSES.md ist aktuell.")
        return 0
    TARGET.write_text(text, encoding="utf-8")
    print(f"{TARGET.name} geschrieben.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
