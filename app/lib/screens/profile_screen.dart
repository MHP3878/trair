// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';

class ProfileScreen extends StatefulWidget {
  const ProfileScreen({super.key});

  @override
  State<ProfileScreen> createState() => _ProfileScreenState();
}

class _ProfileScreenState extends State<ProfileScreen> {
  late final TextEditingController _name;
  bool _askDelete = false;
  final _keys = {
    'Profil': GlobalKey(),
    'Trust': GlobalKey(),
    'Geräte': GlobalKey(),
    'Sicherheit': GlobalKey(),
    'Privatsphäre': GlobalKey(),
    'Konto': GlobalKey(),
  };

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _nameInit();
  }

  bool _nameReady = false;

  void _nameInit() {
    if (_nameReady) return;
    _name = TextEditingController(text: AppScope.of(context).userName);
    _nameReady = true;
  }

  @override
  void dispose() {
    if (_nameReady) _name.dispose();
    super.dispose();
  }

  static const _navIcons = <String, IconData>{
    'Profil': TrairIcons.user,
    'Trust': TrairIcons.badgeCheck,
    'Geräte': TrairIcons.monitorSmartphone,
    'Sicherheit': TrairIcons.shieldCheck,
    'Privatsphäre': TrairIcons.eyeOff,
    'Konto': TrairIcons.userCog,
  };

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    return Backdrop(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SizedBox(width: 288, child: _sidebar(state)),
          Expanded(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(0, 10, 10, 10),
              child: GlassPanel(
                child: SingleChildScrollView(
                  padding: const EdgeInsets.fromLTRB(24, 26, 24, 44),
                  child: Center(
                    child: ConstrainedBox(
                      constraints: const BoxConstraints(maxWidth: 820),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          _profileCard(state),
                          const SizedBox(height: 14),
                          LayoutBuilder(
                            builder: (context, c) {
                              final trust = _trustCard();
                              final devices = _devicesCard();
                              if (c.maxWidth < 700) {
                                return Column(children: [trust, const SizedBox(height: 14), devices]);
                              }
                              return IntrinsicHeight(
                                child: Row(
                                  crossAxisAlignment: CrossAxisAlignment.stretch,
                                  children: [
                                    Expanded(child: trust),
                                    const SizedBox(width: 14),
                                    Expanded(child: devices),
                                  ],
                                ),
                              );
                            },
                          ),
                          const SizedBox(height: 14),
                          _securityCard(state),
                          const SizedBox(height: 14),
                          _privacyCard(state),
                          const SizedBox(height: 14),
                          _accountCard(state),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _sidebar(AppState state) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(18, 18, 14, 14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const SidebarHeader(trailing: BackToChats()),
          const SizedBox(height: 22),
          const Padding(
            padding: EdgeInsets.only(left: 12, bottom: 10),
            child: Text(
              'Einstellungen',
              style: TextStyle(fontSize: 22, fontWeight: FontWeight.w600, letterSpacing: -0.6),
            ),
          ),
          for (final entry in _navIcons.entries)
            InkWell(
              borderRadius: BorderRadius.circular(14),
              onTap: () {
                final ctx = _keys[entry.key]?.currentContext;
                if (ctx != null) {
                  Scrollable.ensureVisible(
                    ctx,
                    duration: const Duration(milliseconds: 350),
                    curve: Curves.easeOutCubic,
                  );
                }
              },
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 11),
                child: Row(
                  children: [
                    Icon(entry.value, size: 18, color: TrairColors.textMuted),
                    const SizedBox(width: 12),
                    Text(entry.key, style: const TextStyle(fontSize: 14.5, color: TrairColors.textSoft)),
                  ],
                ),
              ),
            ),
          const Spacer(),
          const ProfileCard(),
        ],
      ),
    );
  }

  Widget _title(String text, {Widget? trailing}) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 14),
      child: Row(
        children: [
          Text(text, style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600)),
          const Spacer(),
          if (trailing != null) trailing,
        ],
      ),
    );
  }

  Widget _profileCard(AppState state) {
    return SoftCard(
      key: _keys['Profil'],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title('Profil'),
          Container(
            height: 140,
            alignment: Alignment.center,
            padding: const EdgeInsets.symmetric(horizontal: 20),
            decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(20)),
            child: GlowName(
              name: state.userName.trim().isEmpty ? 'Dein Name' : state.userName.trim(),
              color: state.glow,
              strength: state.glowStrength,
              size: 44,
            ),
          ),
          const SizedBox(height: 18),
          Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    const FieldLabel('Anzeigename'),
                    PillField(
                      controller: _name,
                      hint: 'Dein Name',
                      semanticLabel: 'Anzeigename',
                      height: 46,
                      onChanged: state.setName,
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 16),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    FieldLabel('Leuchtkraft · ${state.glowStrength.round()} %'),
                    Container(
                      height: 46,
                      padding: const EdgeInsets.symmetric(horizontal: 6),
                      decoration: BoxDecoration(
                        color: TrairColors.field,
                        borderRadius: BorderRadius.circular(999),
                        border: Border.all(color: TrairColors.border),
                      ),
                      child: Slider(
                        value: state.glowStrength,
                        min: 0,
                        max: 100,
                        divisions: 20,
                        semanticFormatterCallback: (v) => 'Leuchtkraft ${v.round()} Prozent',
                        onChanged: state.setGlowStrength,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 18),
          const FieldLabel('Leuchtfarbe'),
          Align(
            alignment: Alignment.centerLeft,
            child: GlowSwatches(selected: state.glow, onPick: state.setGlow),
          ),
        ],
      ),
    );
  }

  Widget _trustCard() {
    const level = 2;
    const labels = ['0 Neu', '1 Dabei', '2 Vertraut', '3 Verlässlich'];
    return SoftCard(
      key: _keys['Trust'],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title(
            'Trust',
            trailing: Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
              decoration: BoxDecoration(color: const Color(0x1F5EE6B0), borderRadius: BorderRadius.circular(999)),
              child: const Text('Stufe 2 · Vertraut', style: TextStyle(fontSize: 12, color: TrairColors.mintText)),
            ),
          ),
          Row(
            children: [
              for (var i = 0; i < 4; i++)
                Expanded(
                  child: Padding(
                    padding: EdgeInsets.only(right: i < 3 ? 6 : 0),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Container(
                          height: 8,
                          decoration: BoxDecoration(
                            color: i <= level ? TrairColors.mint : const Color(0x14FFFFFF),
                            borderRadius: BorderRadius.circular(999),
                          ),
                        ),
                        const SizedBox(height: 8),
                        Text(
                          labels[i],
                          style: TextStyle(
                            fontSize: 11.5,
                            color: i == level ? TrairColors.text : TrairColors.textMuted,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
            ],
          ),
          const SizedBox(height: 14),
          const Text(
            'Steigt mit der Zeit und durch Bürgschaften. Sinkt nur bei bestätigten Verstößen. Viel schreiben bringt nichts.',
            style: TrairText.muted,
          ),
        ],
      ),
    );
  }

  Widget _devicesCard() {
    return SoftCard(
      key: _keys['Geräte'],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title('Geräte'),
          Container(
            padding: const EdgeInsets.fromLTRB(12, 12, 14, 12),
            decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(18)),
            child: Row(
              children: [
                Container(
                  width: 38,
                  height: 38,
                  decoration: const BoxDecoration(color: Color(0x246E4FF6), shape: BoxShape.circle),
                  child: const Icon(TrairIcons.monitor, size: 19, color: TrairColors.accentPale),
                ),
                const SizedBox(width: 12),
                const Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('Dieser PC', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w600)),
                      Text('Windows · vom Konto unterschrieben', style: TrairText.small),
                    ],
                  ),
                ),
                Container(
                  width: 8,
                  height: 8,
                  decoration: const BoxDecoration(color: TrairColors.mint, shape: BoxShape.circle),
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          PillButton(
            label: 'Gerät per QR-Code koppeln',
            icon: TrairIcons.qrCode,
            kind: PillKind.dashed,
            expand: true,
            onPressed: () {},
          ),
          const SizedBox(height: 10),
          const Text(
            'Nur von einem entsperrten Gerät aus. Ein zweites Gerät rettet dein Konto, wenn dieser PC ausfällt.',
            style: TrairText.small,
          ),
        ],
      ),
    );
  }

  Widget _row({required String title, required String hint, Widget? leading, Widget? trailing, bool divider = true}) {
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 14),
      decoration: BoxDecoration(
        border: divider ? const Border(bottom: BorderSide(color: TrairColors.borderSoft)) : null,
      ),
      child: Row(
        children: [
          if (leading != null) ...[leading, const SizedBox(width: 14)],
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title, style: const TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600)),
                const SizedBox(height: 3),
                Text(hint, style: TrairText.muted),
              ],
            ),
          ),
          if (trailing != null) ...[const SizedBox(width: 16), trailing],
        ],
      ),
    );
  }

  Widget _securityCard(AppState state) {
    return SoftCard(
      key: _keys['Sicherheit'],
      padding: const EdgeInsets.fromLTRB(22, 18, 22, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title('Sicherheit'),
          _row(
            title: 'Strenger Modus',
            hint: 'Blockiert fremde Geräte und das Senden bei geänderter Sicherheitsnummer.',
            trailing: PillSwitch(value: state.strict, onChanged: state.setStrict, label: 'Strenger Modus'),
          ),
          if (!state.strict)
            Container(
              margin: const EdgeInsets.only(top: 12),
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
              decoration: BoxDecoration(
                color: const Color(0x12FFB547),
                borderRadius: BorderRadius.circular(18),
                border: Border.all(color: const Color(0x3DFFB547)),
              ),
              child: const Text(
                'Ohne strengen Modus warnt Trair nur noch. Ein fremdes Gerät könnte mitlesen, wenn du die Warnung übersiehst.',
                style: TextStyle(fontSize: 13, height: 1.45, color: TrairColors.amberText),
              ),
            ),
          _row(
            title: 'Keine Wiederherstellung',
            hint:
                'Passwort vergessen und kein zweites Gerät gekoppelt heißt: Konto für immer weg. Niemand kann es zurückholen.',
            leading: Container(
              width: 34,
              height: 34,
              decoration: const BoxDecoration(color: Color(0x1FFF8A98), shape: BoxShape.circle),
              child: const Icon(TrairIcons.lock, size: 16, color: TrairColors.rose),
            ),
          ),
          _row(
            title: 'Automatisch sperren',
            hint: 'Nach 15 Minuten ohne Benutzung und wenn Windows gesperrt wird.',
            divider: false,
            trailing: PillButton(label: 'Jetzt sperren', icon: TrairIcons.lock, height: 42, onPressed: state.lock),
          ),
        ],
      ),
    );
  }

  Widget _privacyCard(AppState state) {
    const items = [
      ('lese', 'Lesebestätigungen', 'Andere sehen nicht, ob du ihre Nachricht gelesen hast.'),
      ('tippt', '„Schreibt gerade“ anzeigen', 'Andere sehen nicht, wenn du tippst.'),
      ('online', 'Online-Status zeigen', 'Niemand sieht, ob oder wann du online bist.'),
      ('vorschau', 'Inhalt in Benachrichtigungen', 'Benachrichtigungen zeigen nur „Neue Nachricht“, nie den Text.'),
    ];
    return SoftCard(
      key: _keys['Privatsphäre'],
      padding: const EdgeInsets.fromLTRB(22, 18, 22, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title('Privatsphäre'),
          for (var i = 0; i < items.length; i++)
            _row(
              title: items[i].$2,
              hint: items[i].$3,
              divider: i < items.length - 1,
              trailing: PillSwitch(
                value: state.privacy[items[i].$1] ?? false,
                onChanged: (v) => state.setPrivacy(items[i].$1, v),
                label: items[i].$2,
              ),
            ),
        ],
      ),
    );
  }

  Widget _accountCard(AppState state) {
    return SoftCard(
      key: _keys['Konto'],
      borderColor: const Color(0x29FF8A98),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title('Konto'),
          const Text(
            'Meldest du dich 6 Monate lang nicht an, löscht der Server dein Konto automatisch und entfernt es aus allen Gruppen und Communities.',
            style: TrairText.muted,
          ),
          const SizedBox(height: 14),
          Align(
            alignment: Alignment.centerLeft,
            child: PillButton(
              label: 'Open-Source-Lizenzen',
              icon: TrairIcons.fileText,
              kind: PillKind.ghost,
              height: 42,
              onPressed: () => showLicensePage(
                context: context,
                applicationName: 'Trair',
                applicationLegalese: 'AGPL-3.0-or-later · Quellcode: github.com/MHP3878/trair',
              ),
            ),
          ),
          const SizedBox(height: 10),
          AnimatedSwitcher(
            duration: const Duration(milliseconds: 220),
            child: _askDelete
                ? Container(
                    key: const ValueKey('confirm'),
                    padding: const EdgeInsets.all(16),
                    decoration: BoxDecoration(
                      color: const Color(0x12FF5C70),
                      borderRadius: BorderRadius.circular(20),
                      border: Border.all(color: const Color(0x42FF8A98)),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        const Text(
                          'Wirklich löschen? Dein Konto verschwindet sofort vom Server und von diesem Gerät. Das kann niemand rückgängig machen.',
                          style: TextStyle(fontSize: 14, height: 1.5, color: TrairColors.roseText),
                        ),
                        const SizedBox(height: 12),
                        Wrap(
                          spacing: 10,
                          children: [
                            PillButton(
                              label: 'Abbrechen',
                              kind: PillKind.ghost,
                              height: 42,
                              onPressed: () => setState(() => _askDelete = false),
                            ),
                            PillButton(
                              label: 'Endgültig löschen',
                              kind: PillKind.danger,
                              height: 42,
                              onPressed: () => state.go(TrairScreen.onboarding),
                            ),
                          ],
                        ),
                      ],
                    ),
                  )
                : Align(
                    key: const ValueKey('button'),
                    alignment: Alignment.centerLeft,
                    child: PillButton(
                      label: 'Konto endgültig löschen',
                      kind: PillKind.danger,
                      height: 42,
                      onPressed: () => setState(() => _askDelete = true),
                    ),
                  ),
          ),
        ],
      ),
    );
  }
}
