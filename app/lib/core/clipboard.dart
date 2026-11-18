// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';

import 'package:flutter/services.dart';

/// How long copied codes stay in the clipboard.
const clipboardLifetime = Duration(seconds: 60);

/// Copy [text] and remove it from the clipboard after [clipboardLifetime],
/// unless something else was copied in the meantime.
Future<void> copySensitive(String text) async {
  await Clipboard.setData(ClipboardData(text: text));
  Timer(clipboardLifetime, () async {
    final now = await Clipboard.getData(Clipboard.kTextPlain);
    if (now?.text == text) {
      await Clipboard.setData(const ClipboardData(text: ''));
    }
  });
}
