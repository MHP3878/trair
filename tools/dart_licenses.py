#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
"""Lizenzen der Dart-Pakete aus app/pubspec.lock.

  python3 tools/dart_licenses.py --report   # erkannte Lizenzen anzeigen (CI, nach "flutter pub get")
  python3 tools/dart_licenses.py --verify   # LICENSE-Dateien im Pub-Cache gegen KNOWN pruefen

KNOWN ist von Hand gepflegt. --verify liest die echten LICENSE-Dateien aus
dem Pub-Cache und bricht ab, wenn ein Paket fehlt oder die Lizenz anders
aussieht. So kann kein neues Paket unbemerkt dazukommen.
Nutzt nur die Python-Standardbibliothek.
"""
import os
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOCK = ROOT / "app" / "pubspec.lock"

# Paket -> SPDX-Lizenz. Nur Lizenzen, die mit AGPL-3.0 vereinbar sind.
# Enthaelt auch Pakete, die nur die Tests brauchen (flutter_test).
KNOWN = {
    "async": "BSD-3-Clause",
    "boolean_selector": "BSD-3-Clause",
    "characters": "BSD-3-Clause",
    "clock": "Apache-2.0",
    "collection": "BSD-3-Clause",
    "cupertino_ui": "BSD-3-Clause",
    "fake_async": "Apache-2.0",
    "intl": "BSD-3-Clause",
    "leak_tracker": "BSD-3-Clause",
    "leak_tracker_flutter_testing": "BSD-3-Clause",
    "leak_tracker_testing": "BSD-3-Clause",
    "listen": "BSD-3-Clause",
    "matcher": "BSD-3-Clause",
    "material_color_utilities": "Apache-2.0",
    "material_ui": "BSD-3-Clause",
    "meta": "BSD-3-Clause",
    "path": "BSD-3-Clause",
    "source_span": "BSD-3-Clause",
    "stack_trace": "BSD-3-Clause",
    "stream_channel": "BSD-3-Clause",
    "string_scanner": "BSD-3-Clause",
    "term_glyph": "BSD-3-Clause",
    "test_api": "BSD-3-Clause",
    "vector_math": "BSD-3-Clause",
    "vm_service": "BSD-3-Clause",
}


def hosted_packages(lock_text):
    """Return [(name, version)] of all pub.dev packages in pubspec.lock."""
    packages = []
    blocks = re.split(r"\n  (?=[a-z0-9_]+:\n)", lock_text.split("\npackages:\n", 1)[-1])
    for block in blocks:
        name = re.match(r"\s*([a-z0-9_]+):\n", block + "\n")
        source = re.search(r"\n    source: (\w+)", block)
        version = re.search(r'\n    version: "([^"]+)"', block)
        if name and source and version and source.group(1) == "hosted":
            packages.append((name.group(1), version.group(1)))
    return sorted(packages)


def classify(text):
    t = " ".join(text.split())
    if "Apache License" in t and "Version 2.0" in t:
        return "Apache-2.0"
    if "Permission is hereby granted, free of charge" in t:
        return "MIT"
    if "Redistribution and use in source and binary forms" in t:
        return "BSD-3-Clause" if "Neither the name" in t or "names of its contributors" in t else "BSD-2-Clause"
    return "UNBEKANNT"


def cache_dir():
    base = os.environ.get("PUB_CACHE") or os.path.expanduser("~/.pub-cache")
    return pathlib.Path(base) / "hosted" / "pub.dev"


def detected(name, version):
    folder = cache_dir() / f"{name}-{version}"
    for candidate in ("LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"):
        f = folder / candidate
        if f.exists():
            return classify(f.read_text(encoding="utf-8", errors="replace"))
    return "KEINE LICENSE-DATEI"


def main():
    if not LOCK.exists():
        print("app/pubspec.lock fehlt.")
        return 1
    packages = hosted_packages(LOCK.read_text(encoding="utf-8"))
    if "--report" in sys.argv:
        for name, version in packages:
            print(f"{name} {version}: {detected(name, version)}")
        return 0
    if "--verify" in sys.argv:
        bad = []
        for name, version in packages:
            found = detected(name, version)
            if KNOWN.get(name) != found:
                bad.append(f"  {name} {version}: im Cache {found}, in KNOWN {KNOWN.get(name)}")
        if bad:
            print("Dart-Lizenzen stimmen nicht mit KNOWN in tools/dart_licenses.py ueberein:")
            print("\n".join(bad))
            return 1
        print(f"{len(packages)} Dart-Pakete, alle Lizenzen bekannt und geprueft.")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
