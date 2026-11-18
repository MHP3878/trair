// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

/// Colors of the Pro look (dark, frosted glass, one accent).
class TrairColors {
  static const background = Color(0xFF09090D);
  static const glass = Color(0x80161620);
  static const glassLite = Color(0x8C222230);
  static const glassPopup = Color(0xB81E1E2C);
  static const card = Color(0x0BFFFFFF);
  static const cardActive = Color(0x0FFFFFFF);
  static const field = Color(0x42000000);
  static const border = Color(0x14FFFFFF);
  static const borderSoft = Color(0x0FFFFFFF);
  static const hover = Color(0x09FFFFFF);
  static const bubbleOther = Color(0x13FFFFFF);
  static const text = Color(0xFFEDECF4);
  static const textSoft = Color(0xFFC8C6D4);
  static const textMuted = Color(0xFF9A98AA);
  static const placeholder = Color(0xFF8F8CA0);
  static const accent = Color(0xFF6E4FF6);
  static const accentHover = Color(0xFF7B5DFF);
  static const accentLight = Color(0xFFA18DFF);
  static const accentPale = Color(0xFFCFC4FF);
  static const mint = Color(0xFF5EE6B0);
  static const mintText = Color(0xFFBFF5DD);
  static const amber = Color(0xFFFFB547);
  static const amberText = Color(0xFFFFE2B8);
  static const amberDark = Color(0xFF1A1206);
  static const rose = Color(0xFFFF8A98);
  static const roseText = Color(0xFFFFD6DB);

  /// Glow colors a user can pick for the name.
  static const glowChoices = <String, Color>{
    'Violett': Color(0xFFA18DFF),
    'Eisblau': Color(0xFF5CC8FF),
    'Minze': Color(0xFF5EE6B0),
    'Rosa': Color(0xFFFF7AC6),
    'Bernstein': Color(0xFFFFB547),
    'Hell': Color(0xFFF5F3FF),
  };
}

/// The app-wide theme.
ThemeData trairTheme() {
  final base = ThemeData(
    useMaterial3: true,
    brightness: Brightness.dark,
    fontFamily: 'Geist',
    scaffoldBackgroundColor: TrairColors.background,
    colorScheme: ColorScheme.fromSeed(
      seedColor: TrairColors.accent,
      brightness: Brightness.dark,
      surface: TrairColors.background,
    ),
  );
  return base.copyWith(
    textSelectionTheme: const TextSelectionThemeData(
      cursorColor: TrairColors.accentLight,
      selectionColor: Color(0x556E4FF6),
    ),
    sliderTheme: base.sliderTheme.copyWith(
      activeTrackColor: TrairColors.accentLight,
      thumbColor: TrairColors.accentPale,
      inactiveTrackColor: const Color(0x22FFFFFF),
    ),
    checkboxTheme: CheckboxThemeData(shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(5))),
    tooltipTheme: const TooltipThemeData(waitDuration: Duration(milliseconds: 600)),
  );
}

/// Text styles used everywhere.
class TrairText {
  static const title = TextStyle(
    fontSize: 26,
    fontWeight: FontWeight.w600,
    letterSpacing: -0.6,
    color: TrairColors.text,
  );
  static const heading = TextStyle(
    fontSize: 16,
    fontWeight: FontWeight.w600,
    letterSpacing: -0.2,
    color: TrairColors.text,
  );
  static const body = TextStyle(fontSize: 14.5, height: 1.5, color: TrairColors.text);
  static const muted = TextStyle(fontSize: 13, height: 1.45, color: TrairColors.textMuted);
  static const small = TextStyle(fontSize: 12, color: TrairColors.textMuted);
  static const label = TextStyle(fontSize: 13, fontWeight: FontWeight.w500, color: TrairColors.textSoft);
  static const mono = TextStyle(fontFamily: 'GeistMono', fontSize: 14, color: TrairColors.text);
}

/// Soft glow around a name, as text shadows. `strength` is 0..100.
List<Shadow> glowShadows(Color color, {double strength = 35}) {
  if (strength <= 0) return const [];
  return [
    Shadow(color: color.withAlpha(0x66), blurRadius: 2 + strength * 0.14),
    Shadow(color: color.withAlpha(0x26), blurRadius: 6 + strength * 0.3),
  ];
}
