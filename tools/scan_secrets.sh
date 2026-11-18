#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
#
# Sucht in allen Dateien des Repositorys nach Geheimnissen (Schluessel,
# Passwoerter, Tokens). Bricht mit Fehler ab, wenn etwas Neues auftaucht.
# Lokal:  bash tools/scan_secrets.sh
set -euo pipefail

VENV="${SCANNER_VENV:-/tmp/trair-scanner}"
if [ ! -x "$VENV/bin/detect-secrets-hook" ]; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install --quiet "detect-secrets==1.5.0"
fi

git ls-files -z | xargs -0 "$VENV/bin/detect-secrets-hook" --baseline .secrets.baseline
echo "Keine neuen Geheimnisse gefunden."
