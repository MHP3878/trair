// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../core/accounts.dart';
import '../core/vault.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';

/// Launcher: nothing is decrypted while this screen is shown. The password
/// goes straight to the Rust core; a wrong one opens nothing.
class LockScreen extends StatefulWidget {
  const LockScreen({super.key});

  @override
  State<LockScreen> createState() => _LockScreenState();
}

class _LockScreenState extends State<LockScreen> {
  final _pw = TextEditingController();
  bool _show = false;
  bool _busy = false;
  String _error = '';
  int _wait = 0;
  Timer? _countdown;

  @override
  void dispose() {
    _countdown?.cancel();
    _pw.dispose();
    super.dispose();
  }

  Future<void> _unlock(AppState state) async {
    if (_busy || _wait > 0 || _pw.text.isEmpty || state.account == null) return;
    setState(() {
      _busy = true;
      _error = '';
    });
    final answer = await state.unlock(_pw.text);
    if (!mounted) return;
    // The password leaves the field either way.
    _pw.clear();
    setState(() {
      _busy = false;
      // "vanished": an emergency PIN took the account off the list.
      if (!answer.ok && answer.error != 'vanished') {
        _error = vaultErrorText(answer);
        _startWait(answer.wait);
      }
    });
  }

  /// One account in the list: picture or glowing initials, name.
  Widget _accountTile(AppState state, AccountCard c) {
    final selected = state.account?.dir == c.dir;
    final color = c.glow != null ? Color(c.glow!) : TrairColors.accentLight;
    final initials = c.name.isEmpty ? '${c.number}' : c.name.characters.first.toUpperCase();
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          borderRadius: BorderRadius.circular(20),
          onTap: _busy
              ? null
              : () {
                  _pw.clear();
                  setState(() => _error = '');
                  state.chooseAccount(c);
                },
          child: AnimatedContainer(
            duration: const Duration(milliseconds: 200),
            padding: const EdgeInsets.fromLTRB(10, 10, 14, 10),
            decoration: BoxDecoration(
              color: selected ? const Color(0x14FFFFFF) : const Color(0x06FFFFFF),
              borderRadius: BorderRadius.circular(20),
              border: Border.all(color: selected ? color.withAlpha(0x80) : TrairColors.borderSoft),
              boxShadow: selected ? [BoxShadow(color: color.withAlpha(0x30), blurRadius: 22)] : const [],
            ),
            child: Row(
              children: [
                Avatar(initials: initials, color: color, size: 42, picture: c.picture),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        c.title,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 15,
                          fontWeight: FontWeight.w600,
                          color: c.name.isEmpty ? TrairColors.text : color,
                          shadows: c.name.isEmpty ? null : glowShadows(color, strength: 18),
                        ),
                      ),
                      Text(c.name.isEmpty ? 'Name verborgen' : 'Konto ${c.number}', style: TrairText.small),
                    ],
                  ),
                ),
                AnimatedOpacity(
                  opacity: selected ? 1 : 0,
                  duration: const Duration(milliseconds: 200),
                  child: Icon(TrairIcons.check, size: 18, color: color),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  void _startWait(int seconds) {
    _countdown?.cancel();
    _wait = seconds;
    if (seconds <= 0) return;
    _countdown = Timer.periodic(const Duration(seconds: 1), (t) {
      if (!mounted) return;
      setState(() {
        _wait -= 1;
        if (_wait <= 0) {
          t.cancel();
          _error = '';
        }
      });
    });
  }

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final blocked = _busy || _wait > 0;
    return Backdrop(
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(vertical: 48, horizontal: 20),
          child: Column(
            children: [
              SizedBox(
                width: 400,
                child: GlassPanel(
                  radius: 16,
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
                      const SizedBox(height: 22),
                      if (state.accountCards.isNotEmpty) ...[
                        for (final c in state.accountCards) _accountTile(state, c),
                        const SizedBox(height: 14),
                      ],
                      const FieldLabel('Passwort'),
                      PillField(
                        controller: _pw,
                        hint: 'Passwort eingeben',
                        obscure: !_show,
                        autofocus: true,
                        semanticLabel: 'Launcher-Passwort',
                        onSubmitted: (_) => _unlock(state),
                        trailing: RoundIconButton(
                          icon: _show ? TrairIcons.eyeOff : TrairIcons.eye,
                          tooltip: _show ? 'Passwort verbergen' : 'Passwort anzeigen',
                          filled: false,
                          onPressed: () => setState(() => _show = !_show),
                        ),
                      ),
                      const SizedBox(height: 14),
                      PillButton(
                        label: _busy ? 'Entsperre …' : (_wait > 0 ? 'Noch $_wait s warten' : 'Entsperren'),
                        icon: TrairIcons.arrowRight,
                        height: 50,
                        expand: true,
                        onPressed: blocked ? null : () => _unlock(state),
                      ),
                      if (state.accountMessage.isNotEmpty)
                        Padding(
                          padding: const EdgeInsets.only(top: 12),
                          child: Text(
                            state.accountMessage,
                            textAlign: TextAlign.center,
                            style: const TextStyle(fontSize: 13.5, height: 1.45, color: TrairColors.amber),
                          ),
                        ),
                      AnimatedSize(
                        duration: const Duration(milliseconds: 200),
                        child: _error.isEmpty
                            ? const SizedBox(width: double.infinity)
                            : Padding(
                                padding: const EdgeInsets.only(top: 12),
                                child: Text(
                                  _error,
                                  textAlign: TextAlign.center,
                                  style: const TextStyle(fontSize: 13.5, height: 1.45, color: TrairColors.amber),
                                ),
                              ),
                      ),
                      const SizedBox(height: 18),
                      const Center(
                        child: SecurityCapsule(text: 'Gesperrt · nichts ist entschlüsselt', tone: CapsuleTone.locked),
                      ),
                      const SizedBox(height: 18),
                      const Divider(color: TrairColors.borderSoft, height: 1),
                      const SizedBox(height: 16),
                      PillButton(
                        label: 'Neues Konto erstellen',
                        icon: TrairIcons.userPlus,
                        kind: PillKind.ghost,
                        expand: true,
                        onPressed: _busy
                            ? null
                            : () {
                                _pw.clear();
                                state.newAccount();
                              },
                      ),
                      const SizedBox(height: 14),
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
                  Text(
                    state.screenProtectionSupported
                        ? 'Geschützt vor Bildschirmfotos · Open Source unter AGPL-3.0'
                        : 'Open Source unter AGPL-3.0',
                    style: TrairText.small,
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
