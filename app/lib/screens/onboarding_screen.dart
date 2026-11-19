// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../core/vault.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';

/// First start: invite code, profile, launcher password. No recovery key:
/// losing the password means losing the account (decision 2).
class OnboardingScreen extends StatefulWidget {
  const OnboardingScreen({super.key});

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  int _step = 1;
  final _code = TextEditingController();
  final _name = TextEditingController();
  final _pw1 = TextEditingController();
  final _pw2 = TextEditingController();
  Color _glow = TrairColors.accentLight;
  bool _understood = false;
  bool _busy = false;
  String _error = '';

  // Joining an existing account as a new device.
  bool _joining = false;
  final _joinServer = TextEditingController(text: defaultServerUrl);
  final _joinCode = TextEditingController();
  final _joinPw1 = TextEditingController();
  final _joinPw2 = TextEditingController();

  @override
  void dispose() {
    for (final c in [_code, _name, _pw1, _pw2, _joinServer, _joinCode, _joinPw1, _joinPw2]) {
      c.dispose();
    }
    super.dispose();
  }

  int get _score {
    final p = _pw1.text;
    var s = 0;
    final length = p.runes.length;
    if (length >= 8) s++;
    if (length >= 12) s++;
    if (RegExp(r'[0-9]').hasMatch(p) && RegExp(r'[A-Za-z]').hasMatch(p)) s++;
    if (length >= 16 || RegExp(r'[^A-Za-z0-9]').hasMatch(p)) s++;
    return s;
  }

  bool get _canStart => _understood && _pw1.text.runes.length >= 12 && _pw1.text == _pw2.text;

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    return Backdrop(
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(vertical: 40, horizontal: 20),
          child: SizedBox(
            width: 560,
            child: GlassPanel(
              radius: 32,
              padding: const EdgeInsets.all(36),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      const TrairLogo(size: 34),
                      const SizedBox(width: 12),
                      Text(
                        state.addingAccount ? 'Neues Konto' : 'Trair einrichten',
                        style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
                      ),
                      const Spacer(),
                      if (state.addingAccount)
                        Padding(
                          padding: const EdgeInsets.only(right: 10),
                          child: RoundIconButton(
                            icon: TrairIcons.x,
                            tooltip: 'Zurück zur Kontoauswahl',
                            filled: false,
                            size: 32,
                            onPressed: state.backToAccounts,
                          ),
                        ),
                      if (!_joining)
                        Semantics(
                          label: 'Schritt $_step von 3',
                          child: Row(
                            children: [
                              for (var i = 1; i <= 3; i++)
                                AnimatedContainer(
                                  duration: const Duration(milliseconds: 350),
                                  curve: Curves.easeOutCubic,
                                  margin: const EdgeInsets.only(left: 6),
                                  width: i == _step ? 28 : 8,
                                  height: 8,
                                  decoration: BoxDecoration(
                                    color: i <= _step ? const Color(0xFF8B70FF) : const Color(0x1FFFFFFF),
                                    borderRadius: BorderRadius.circular(999),
                                  ),
                                ),
                            ],
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 28),
                  AnimatedSwitcher(
                    duration: const Duration(milliseconds: 250),
                    child: KeyedSubtree(
                      key: ValueKey(_joining ? 0 : _step),
                      child: _joining
                          ? _joinPanel(state)
                          : switch (_step) {
                              1 => _stepInvite(state),
                              2 => _stepProfile(),
                              _ => _stepPassword(state),
                            },
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _nav({required VoidCallback? onBack, required Widget next}) {
    return Row(
      children: [
        if (onBack != null) PillButton(label: 'Zurück', kind: PillKind.ghost, onPressed: onBack),
        const Spacer(),
        next,
      ],
    );
  }

  Future<void> _start(AppState state) async {
    setState(() {
      _busy = true;
      _error = '';
    });
    final answer = await state.createAccount(name: _name.text, glowColor: _glow, password: _pw1.text);
    if (!mounted) return;
    if (!answer.ok) {
      setState(() {
        _busy = false;
        _error = vaultErrorText(answer);
      });
    }
  }

  bool get _canJoin =>
      _joinCode.text.trim().isNotEmpty && _joinPw1.text.runes.length >= 12 && _joinPw1.text == _joinPw2.text;

  Future<void> _join(AppState state) async {
    setState(() => _busy = true);
    await state.joinWithCode(url: _joinServer.text, code: _joinCode.text, password: _joinPw1.text);
    if (mounted) setState(() => _busy = false);
  }

  Widget _joinPanel(AppState state) {
    final mismatch = _joinPw2.text.isNotEmpty && _joinPw1.text != _joinPw2.text;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Text('Dieses Gerät koppeln', style: TrairText.title),
        const SizedBox(height: 8),
        const Text(
          'Öffne Trair auf deinem anderen, entsperrten Gerät: Einstellungen → Geräte → Gerät koppeln. '
          'Gib den Code ein, der dort steht. Dieses Gerät bekommt dann dein Konto.',
          style: TextStyle(fontSize: 15, height: 1.55, color: TrairColors.textMuted),
        ),
        const SizedBox(height: 20),
        const FieldLabel('Server'),
        PillField(
          controller: _joinServer,
          hint: 'http://127.0.0.1:PORT',
          mono: true,
          height: 46,
          semanticLabel: 'Server',
        ),
        const SizedBox(height: 14),
        const FieldLabel('Code vom anderen Gerät'),
        PillField(
          controller: _joinCode,
          hint: '7-anker-birne-kerze',
          mono: true,
          height: 52,
          semanticLabel: 'Kopplungs-Code',
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 14),
        const FieldLabel('Passwort für dieses Gerät'),
        PillField(
          controller: _joinPw1,
          hint: 'Mindestens 12 Zeichen',
          obscure: true,
          semanticLabel: 'Passwort für dieses Gerät',
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 10),
        PillField(
          controller: _joinPw2,
          hint: 'Noch einmal',
          obscure: true,
          semanticLabel: 'Passwort wiederholen',
          onChanged: (_) => setState(() {}),
        ),
        if (mismatch)
          const Padding(
            padding: EdgeInsets.only(left: 18, top: 6),
            child: Text(
              'Die Passwörter stimmen nicht überein.',
              style: TextStyle(fontSize: 12, color: TrairColors.amber),
            ),
          ),
        const SizedBox(height: 8),
        const Text('Jedes Gerät hat sein eigenes Passwort. Es verlässt dieses Gerät nie.', style: TrairText.small),
        if (state.joinMessage.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 14),
            child: Text(
              state.joinMessage,
              style: TextStyle(fontSize: 13.5, color: _busy ? TrairColors.textSoft : TrairColors.amber),
            ),
          ),
        const SizedBox(height: 22),
        _nav(
          onBack: _busy ? null : () => setState(() => _joining = false),
          next: PillButton(
            label: _busy ? 'Kopple …' : 'Koppeln',
            icon: TrairIcons.arrowRight,
            onPressed: _canJoin && !_busy ? () => _join(state) : null,
          ),
        ),
      ],
    );
  }

  Widget _stepInvite(AppState state) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Text('Einladungscode eingeben', style: TrairText.title),
        const SizedBox(height: 8),
        const Text(
          'Trair ist im Moment eine geschlossene Beta. Jeden Code kann man nur einmal benutzen. '
          'Geprüft wird er, sobald der Trair-Server läuft.',
          style: TextStyle(fontSize: 15, height: 1.55, color: TrairColors.textMuted),
        ),
        const SizedBox(height: 22),
        const FieldLabel('Einladungscode'),
        PillField(controller: _code, hint: 'TRAIR-XXXX-XXXX', mono: true, height: 52, semanticLabel: 'Einladungscode'),
        const SizedBox(height: 24),
        if (state.serverDeletion.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 14),
            child: Text(state.serverDeletion, style: TrairText.small),
          ),
        const SizedBox(height: 4),
        Row(
          children: [
            PillButton(
              label: 'Ich habe schon ein Konto',
              kind: PillKind.ghost,
              onPressed: () => setState(() => _joining = true),
            ),
            const Spacer(),
            PillButton(label: 'Weiter', onPressed: () => setState(() => _step = 2)),
          ],
        ),
      ],
    );
  }

  Widget _stepProfile() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Text('Wie sollen dich andere sehen?', style: TrairText.title),
        const SizedBox(height: 8),
        const Text(
          'Name und Leuchtfarbe kannst du später jederzeit ändern.',
          style: TextStyle(fontSize: 15, height: 1.55, color: TrairColors.textMuted),
        ),
        const SizedBox(height: 22),
        Container(
          height: 128,
          alignment: Alignment.center,
          padding: const EdgeInsets.symmetric(horizontal: 20),
          decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(24)),
          child: GlowName(name: _name.text.trim().isEmpty ? 'Dein Name' : _name.text.trim(), color: _glow),
        ),
        const SizedBox(height: 20),
        const FieldLabel('Anzeigename'),
        PillField(
          controller: _name,
          hint: 'Dein Name',
          semanticLabel: 'Anzeigename',
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 20),
        const FieldLabel('Leuchtfarbe'),
        Align(
          alignment: Alignment.centerLeft,
          child: GlowSwatches(selected: _glow, onPick: (c) => setState(() => _glow = c)),
        ),
        const SizedBox(height: 24),
        _nav(
          onBack: () => setState(() => _step = 1),
          next: PillButton(label: 'Weiter', onPressed: () => setState(() => _step = 3)),
        ),
      ],
    );
  }

  Widget _stepPassword(AppState state) {
    const labels = ['Noch zu kurz', 'Schwach', 'Okay', 'Gut', 'Stark'];
    const colors = [TrairColors.amber, TrairColors.amber, TrairColors.accentLight, TrairColors.mint];
    final score = _score;
    final mismatch = _pw2.text.isNotEmpty && _pw1.text != _pw2.text;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Text('Launcher-Passwort festlegen', style: TrairText.title),
        const SizedBox(height: 8),
        const Text(
          'Es schützt Trair auf diesem PC. Es verlässt dein Gerät nie, der Server kennt es nicht.',
          style: TextStyle(fontSize: 15, height: 1.55, color: TrairColors.textMuted),
        ),
        const SizedBox(height: 20),
        Container(
          padding: const EdgeInsets.fromLTRB(16, 16, 18, 16),
          decoration: BoxDecoration(
            color: const Color(0x12FF5C70),
            borderRadius: BorderRadius.circular(22),
            border: Border.all(color: const Color(0x3DFF8A98)),
          ),
          child: const Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Icon(TrairIcons.triangleAlert, color: TrairColors.rose, size: 22),
              SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Es gibt kein Zurück.',
                      style: TextStyle(color: Color(0xFFFFE9EC), fontWeight: FontWeight.w600, fontSize: 15),
                    ),
                    SizedBox(height: 4),
                    Text(
                      'Vergisst du dieses Passwort, ist dein Konto für immer weg. Dasselbe gilt bei kaputter '
                      'Festplatte oder neu installiertem Windows. Niemand kann es wiederherstellen. '
                      'Ein zweites gekoppeltes Gerät rettet dein Konto.',
                      style: TextStyle(color: TrairColors.roseText, fontSize: 14, height: 1.5),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 20),
        const FieldLabel('Passwort'),
        PillField(
          controller: _pw1,
          hint: 'Mindestens 12 Zeichen',
          obscure: true,
          semanticLabel: 'Passwort',
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 8),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 18),
          child: Row(
            children: [
              for (var i = 0; i < 4; i++)
                Container(
                  width: 32,
                  height: 4,
                  margin: const EdgeInsets.only(right: 4),
                  decoration: BoxDecoration(
                    color: i < score ? colors[score - 1] : const Color(0x1AFFFFFF),
                    borderRadius: BorderRadius.circular(999),
                  ),
                ),
              const SizedBox(width: 8),
              Flexible(
                child: Text(
                  _pw1.text.isEmpty
                      ? 'Tipp: ein Satz aus mehreren Wörtern'
                      : labels[score] + (_pw1.text.runes.length < 12 ? ' · mind. 12 Zeichen' : ''),
                  style: TrairText.small,
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 16),
        const FieldLabel('Passwort wiederholen'),
        PillField(
          controller: _pw2,
          hint: 'Noch einmal',
          obscure: true,
          semanticLabel: 'Passwort wiederholen',
          onChanged: (_) => setState(() {}),
        ),
        if (mismatch)
          const Padding(
            padding: EdgeInsets.only(left: 18, top: 6),
            child: Text(
              'Die Passwörter stimmen nicht überein.',
              style: TextStyle(fontSize: 12, color: TrairColors.amber),
            ),
          ),
        const SizedBox(height: 16),
        // Own Material, so the tile's hover and ink effects stay visible.
        Material(
          color: TrairColors.field,
          shape: const StadiumBorder(side: BorderSide(color: TrairColors.borderSoft)),
          clipBehavior: Clip.antiAlias,
          child: CheckboxListTile(
            value: _understood,
            onChanged: (v) => setState(() => _understood = v ?? false),
            controlAffinity: ListTileControlAffinity.leading,
            dense: true,
            shape: const StadiumBorder(),
            activeColor: TrairColors.accent,
            title: const Text(
              'Verstanden: Passwort weg heißt Konto für immer weg.',
              style: TextStyle(fontSize: 14, color: TrairColors.text),
            ),
          ),
        ),
        if (_error.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 14),
            child: Text(_error, style: const TextStyle(fontSize: 13.5, color: TrairColors.amber)),
          ),
        const SizedBox(height: 22),
        _nav(
          onBack: _busy ? null : () => setState(() => _step = 2),
          next: PillButton(
            label: _busy ? 'Schlüssel wird erzeugt …' : 'Trair starten',
            icon: TrairIcons.arrowRight,
            onPressed: _canStart && !_busy ? () => _start(state) : null,
          ),
        ),
      ],
    );
  }
}
