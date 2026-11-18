// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';

/// Shown for a moment while the vault folder is opened.
class StartScreen extends StatelessWidget {
  const StartScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return const Backdrop(
      child: Center(child: TrairLogo(size: 64)),
    );
  }
}

/// The security core could not be loaded. Trair does not start without
/// it: no demo fallback, no unprotected mode.
class CoreMissingScreen extends StatelessWidget {
  const CoreMissingScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    return Backdrop(
      child: Center(
        child: SizedBox(
          width: 460,
          child: GlassPanel(
            radius: 32,
            padding: const EdgeInsets.all(32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Icon(TrairIcons.triangleAlert, size: 32, color: TrairColors.amber),
                const SizedBox(height: 16),
                const Text('Sicherheitskern fehlt', style: TrairText.title, textAlign: TextAlign.center),
                const SizedBox(height: 10),
                const Text(
                  'Trair startet nur mit seinem Sicherheitskern (trair_ffi.dll im selben Ordner wie Trair.exe). '
                  'Bitte die ZIP-Datei vollständig neu entpacken.',
                  textAlign: TextAlign.center,
                  style: TextStyle(fontSize: 14.5, height: 1.5, color: TrairColors.textMuted),
                ),
                if (state.startupError.isNotEmpty) ...[
                  const SizedBox(height: 14),
                  SelectableText(state.startupError, textAlign: TextAlign.center, style: TrairText.small),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}
