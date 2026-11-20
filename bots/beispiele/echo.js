#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors
//
// Ein Echo-Bot für Trair, in JavaScript (Node.js).
// Nimmt jede Anfrage an und schreibt zurück, was man ihm schreibt.
//
//     trair-bot start --daten MEIN-BOT -- node echo.js
//
// trair-bot schickt eine JSON-Zeile pro Ereignis; der Bot antwortet mit
// einer JSON-Zeile pro Befehl. Alles Weitere: README.md, Abschnitt „Bots“.

"use strict";
const readline = require("node:readline");

const befehl = (cmd) => process.stdout.write(JSON.stringify(cmd) + "\n");

readline.createInterface({ input: process.stdin }).on("line", (zeile) => {
  const e = JSON.parse(zeile);
  if (e.event === "ready") {
    console.error(`Echo-Bot läuft als ${e.name}.`);
  } else if (e.event === "request") {
    befehl({ op: "accept", chat: e.chat });
  } else if (e.event === "message") {
    befehl({ op: "send", chat: e.chat, text: "Echo: " + e.text });
  } else if (e.ok === false) {
    console.error(`Befehl ging nicht: ${e.error}`);
  }
});
