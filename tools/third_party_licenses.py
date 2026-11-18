#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
"""Erzeugt THIRD_PARTY_LICENSES.md aus den Rust- und Dart-Abhaengigkeiten.

  python3 tools/third_party_licenses.py          # Datei neu schreiben
  python3 tools/third_party_licenses.py --check  # nur pruefen (fuer CI)

Nutzt nur die Python-Standardbibliothek und `cargo metadata`.
"""
import json
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import dart_licenses  # noqa: E402  (liegt im selben Ordner)

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = ROOT / "THIRD_PARTY_LICENSES.md"


# Parts of the app that are not pub.dev packages.
FLUTTER_BASE = [
    "## Flutter / Dart (app/)",
    "",
    "| Component | License | Source |",
    "| --- | --- | --- |",
    "| Flutter SDK | BSD-3-Clause | https://github.com/flutter/flutter |",
    "| Material Icons font (bundled by Flutter) | Apache-2.0 | https://github.com/google/material-design-icons |",
    "| Geist and Geist Mono fonts (app/assets/fonts) | OFL-1.1 | https://github.com/vercel/geist-font |",
    "",
    "Packages from pub.dev (from app/pubspec.lock, including packages only the tests use;",
    "CI compares each license with the LICENSE file of the package):",
    "",
]
FLUTTER_END = [
    "",
    "Flutter writes the license texts of the SDK and all packages into the built app.",
    "The app shows them, together with the font license, under",
    "Einstellungen > Konto > Open-Source-Lizenzen.",
]


def flutter_section():
    lock = dart_licenses.LOCK
    packages = dart_licenses.hosted_packages(lock.read_text(encoding="utf-8")) if lock.exists() else []
    missing = [n for n, _ in packages if n not in dart_licenses.KNOWN]
    if missing:
        raise SystemExit("Lizenz unbekannt fuer Dart-Pakete: " + ", ".join(missing)
                         + " (in tools/dart_licenses.py unter KNOWN eintragen)")
    out = list(FLUTTER_BASE)
    out += ["| Package | Version | License | Source |", "| --- | --- | --- | --- |"]
    out += [f"| {n} | {v} | {dart_licenses.KNOWN[n]} | https://pub.dev/packages/{n} |" for n, v in packages]
    return out + FLUTTER_END


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
    out += ["", *flutter_section(), ""]
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
