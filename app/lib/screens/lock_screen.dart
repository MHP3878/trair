// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';

/// Launcher: nothing is decrypted while this screen is shown.
class LockScreen extends StatefulWidget {
  const LockScreen({super.key});

  @override
  State<LockScreen> createState() => _LockScreenState();
}

class _LockScreenState extends State<LockScreen> {
  final _pw = TextEditingController();
  bool _show = false;

  @override
  void dispose() {
    _pw.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    void unlock() => state.go(TrairScreen.chats);
    return Backdrop(
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(vertical: 48, horizontal: 20),
          child: Column(
            children: [
              SizedBox(
                width: 400,
                child: GlassPanel(
                  radius: 32,
                  padding: const EdgeInsets.fromLTRB(32, 40, 32, 30),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      const Center(child: TrairLogo(size: 60)),
                      const SizedBox(height: 16),
                      const Text('Willkommen zurück', textAlign: TextAlign.center, style: TrairText.title),
                      const SizedBox(height: 4),
                      const Text(
                        'Trair · Private communication.',
                        textAlign: TextAlign.center,
                        style: TextStyle(fontSize: 14, color: TrairColors.textMuted),
                      ),
                      const SizedBox(height: 26),
                      const FieldLabel('Launcher-Passwort'),
                      PillField(
                        controller: _pw,
                        hint: 'Passwort eingeben',
                        obscure: !_show,
                        semanticLabel: 'Launcher-Passwort',
                        onSubmitted: (_) => unlock(),
                        trailing: RoundIconButton(
                          icon: _show ? TrairIcons.eyeOff : TrairIcons.eye,
                          tooltip: _show ? 'Passwort verbergen' : 'Passwort anzeigen',
                          filled: false,
                          onPressed: () => setState(() => _show = !_show),
                        ),
                      ),
                      const SizedBox(height: 14),
                      PillButton(
                        label: 'Entsperren',
                        icon: TrairIcons.arrowRight,
                        height: 50,
                        expand: true,
                        onPressed: unlock,
                      ),
                      const SizedBox(height: 8),
                      const Text(
                        'Demo: Es gibt noch kein echtes Passwort, jede Eingabe öffnet.',
                        textAlign: TextAlign.center,
                        style: TrairText.small,
                      ),
                      const SizedBox(height: 18),
                      const Center(
                        child: SecurityCapsule(text: 'Gesperrt · nichts wird entschlüsselt', tone: CapsuleTone.locked),
                      ),
                      const SizedBox(height: 18),
                      const Divider(color: TrairColors.borderSoft, height: 1),
                      const SizedBox(height: 12),
                      TextButton(
                        onPressed: () => state.go(TrairScreen.onboarding),
                        child: const Text(
                          'Neu hier? Mit Einladungscode starten',
                          style: TextStyle(color: TrairColors.accentPale, fontWeight: FontWeight.w500),
                        ),
                      ),
                      const SizedBox(height: 4),
                      const Text(
                        'Passwort vergessen? Dann ist dieses Konto für immer weg. Es gibt keine Wiederherstellung.',
                        textAlign: TextAlign.center,
                        style: TextStyle(fontSize: 12.5, height: 1.5, color: TrairColors.textMuted),
                      ),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 24),
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Container(
                    width: 6,
                    height: 6,
                    decoration: const BoxDecoration(color: TrairColors.mint, shape: BoxShape.circle),
                  ),
                  const SizedBox(width: 8),
                  const Text('Strenger Modus an · Open Source unter AGPL-3.0', style: TrairText.small),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
