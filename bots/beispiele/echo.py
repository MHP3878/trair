#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 The Trair contributors
"""Ein Echo-Bot für Trair.

Nimmt jede Anfrage an und schreibt zurück, was man ihm schreibt.
Starten (trair-bot kümmert sich um Schlüssel, Verschlüsselung und Server):

    trair-bot start --daten MEIN-BOT -- python echo.py

trair-bot schickt eine JSON-Zeile pro Ereignis; der Bot antwortet mit
einer JSON-Zeile pro Befehl. Alles Weitere: README.md, Abschnitt „Bots“.
"""

import json
import sys

# Immer UTF-8, auch unter Windows.
sys.stdin.reconfigure(encoding="utf-8")
sys.stdout.reconfigure(encoding="utf-8")


def befehl(cmd):
    print(json.dumps(cmd, ensure_ascii=False), flush=True)


for zeile in sys.stdin:
    e = json.loads(zeile)
    art = e.get("event")
    if art == "ready":
        print(f"Echo-Bot läuft als {e['name']}.", file=sys.stderr)
    elif art == "request":
        # Jemand möchte chatten oder lädt in eine Gruppe ein.
        befehl({"op": "accept", "chat": e["chat"]})
    elif art == "message":
        befehl({"op": "send", "chat": e["chat"], "text": "Echo: " + e["text"]})
    elif "ok" in e and not e["ok"]:
        print(f"Befehl ging nicht: {e.get('error')}", file=sys.stderr)
