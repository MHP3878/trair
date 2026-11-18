// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:flutter/widgets.dart';

/// The app's icons: Lucide (ISC, some icons MIT via Feather), bundled as
/// assets/fonts/Lucide.ttf with only the icons listed here. The file is made
/// by tools/make_branding.py; a new icon needs an entry there and here.
abstract final class TrairIcons {
  static const _font = 'Lucide';

  static const arrowRight = IconData(0xE049, fontFamily: _font);
  static const arrowUp = IconData(0xE04A, fontFamily: _font);
  static const badgeCheck = IconData(0xE241, fontFamily: _font);
  static const check = IconData(0xE06C, fontFamily: _font);
  static const chevronDown = IconData(0xE06D, fontFamily: _font);
  static const chevronLeft = IconData(0xE06E, fontFamily: _font);
  static const ellipsis = IconData(0xE0B6, fontFamily: _font);
  static const eye = IconData(0xE0BA, fontFamily: _font);
  static const eyeOff = IconData(0xE0BB, fontFamily: _font);
  static const fileText = IconData(0xE0CC, fontFamily: _font);
  static const flaskConical = IconData(0xE0D5, fontFamily: _font);
  static const lock = IconData(0xE10B, fontFamily: _font);
  static const monitor = IconData(0xE11D, fontFamily: _font);
  static const monitorSmartphone = IconData(0xE3A2, fontFamily: _font);
  static const plus = IconData(0xE13D, fontFamily: _font);
  static const qrCode = IconData(0xE1DF, fontFamily: _font);
  static const search = IconData(0xE151, fontFamily: _font);
  static const shieldCheck = IconData(0xE1FF, fontFamily: _font);
  static const squarePen = IconData(0xE172, fontFamily: _font);
  static const triangleAlert = IconData(0xE193, fontFamily: _font);
  static const user = IconData(0xE19F, fontFamily: _font);
  static const userCog = IconData(0xE342, fontFamily: _font);
  static const userPlus = IconData(0xE1A2, fontFamily: _font);
}
