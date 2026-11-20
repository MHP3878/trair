// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../core/clipboard.dart';
import '../core/vault.dart';
import '../theme.dart';
import '../widgets/badges.dart';
import '../widgets/common.dart';
import '../widgets/profile_popup.dart';
import '../widgets/trair_icons.dart';
import 'team_inbox.dart';

class ProfileScreen extends StatefulWidget {
  const ProfileScreen({super.key});

  @override
  State<ProfileScreen> createState() => _ProfileScreenState();
}

class _ProfileScreenState extends State<ProfileScreen> {
  late final TextEditingController _name;
  late final TextEditingController _about;
  late final TextEditingController _serverUrl;
  bool _askDelete = false;
  String _pictureMessage = '';
  final _oldPw = TextEditingController();
  final _newPw1 = TextEditingController();
  final _newPw2 = TextEditingController();
  final _deletePw = TextEditingController();
  bool _pwBusy = false;
  String _pwMessage = '';
  Color _pwMessageColor = TrairColors.mintText;
  bool _deleteBusy = false;
  String _deleteError = '';
  final _badgeInput = TextEditingController();
  final _botCode = TextEditingController();
  final _botName = TextEditingController();
  bool _botBusy = false;
  bool _badgeBusy = false;
  String _badgeMessage = '';
  bool _badgeOk = false;
  final _deviceName = TextEditingController();
  final _withdrawPw = TextEditingController();
  String? _renaming;
  String? _withdrawing;
  bool _deviceBusy = false;
  String _deviceError = '';
  final _pinPw = TextEditingController();
  final _newPin1 = TextEditingController();
  final _newPin2 = TextEditingController();
  final _pinText = TextEditingController();
  bool _pinSends = false;
  String? _pinChat;
  String _pinAfter = 'vanish';
  bool _pinBusy = false;
  String _pinMessage = '';
  bool _pinOk = false;
  final _keys = {
    'Profil': GlobalKey(),
    'Trust': GlobalKey(),
    'Geräte': GlobalKey(),
    'Server': GlobalKey(),
    'Bots': GlobalKey(),
    'Sicherheit': GlobalKey(),
    'Notfall': GlobalKey(),
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
    final state = AppScope.of(context);
    _name = TextEditingController(text: state.userName);
    _about = TextEditingController(text: state.about);
    _serverUrl = TextEditingController(text: state.serverUrl);
    _nameReady = true;
  }

  @override
  void dispose() {
    if (_nameReady) {
      _name.dispose();
      _about.dispose();
      _serverUrl.dispose();
    }
    for (final c in [
      _oldPw,
      _newPw1,
      _newPw2,
      _deletePw,
      _deviceName,
      _withdrawPw,
      _badgeInput,
      _botCode,
      _botName,
      _pinPw,
      _newPin1,
      _newPin2,
      _pinText,
    ]) {
      c.dispose();
    }
    super.dispose();
  }

  static const _navIcons = <String, IconData>{
    'Profil': TrairIcons.user,
    'Trust': TrairIcons.badgeCheck,
    'Geräte': TrairIcons.monitorSmartphone,
    'Server': TrairIcons.server,
    'Bots': TrairIcons.bot,
    'Sicherheit': TrairIcons.shieldCheck,
    'Notfall': TrairIcons.triangleAlert,
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
          Container(width: 280, color: TrairColors.sidebar, child: _sidebar(state)),
          Expanded(
            child: Padding(
              padding: EdgeInsets.zero,
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
                          _trustCard(),
                          const SizedBox(height: 14),
                          _devicesCard(state),
                          const SizedBox(height: 14),
                          _serverCard(state),
                          const SizedBox(height: 14),
                          _botsCard(state),
                          const SizedBox(height: 14),
                          _securityCard(state),
                          const SizedBox(height: 14),
                          _notfallCard(state),
                          const SizedBox(height: 14),
                          _privacyCard(state),
                          const SizedBox(height: 14),
                          if (state.communities.hasTeamBadge) ...[const TeamInbox(), const SizedBox(height: 14)],
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
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          child: Padding(
            padding: const EdgeInsets.fromLTRB(10, 12, 10, 10),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const Padding(
                  padding: EdgeInsets.only(left: 4),
                  child: SidebarHeader(title: 'Einstellungen', trailing: BackToChats()),
                ),
                const SizedBox(height: 12),
                for (final entry in _navIcons.entries)
                  InkWell(
                    borderRadius: BorderRadius.circular(8),
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
                      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
                      child: Row(
                        children: [
                          Icon(entry.value, size: 18, color: TrairColors.textMuted),
                          const SizedBox(width: 12),
                          Text(entry.key, style: const TextStyle(fontSize: 14.5, color: TrairColors.textSoft)),
                        ],
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
        const ProfileCard(),
      ],
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
            child: Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Avatar(
                  initials: state.userName.trim().isEmpty ? '?' : state.userName.trim().characters.first.toUpperCase(),
                  color: state.glow,
                  size: 76,
                  picture: state.picture,
                ),
                const SizedBox(width: 22),
                Flexible(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      GlowName(
                        name: state.userName.trim().isEmpty ? 'Dein Name' : state.userName.trim(),
                        color: state.glow,
                        strength: state.glowStrength,
                        size: 40,
                      ),
                      if (state.ownBadges.isNotEmpty) ...[
                        const SizedBox(height: 10),
                        BadgeRow(badges: state.ownBadges, size: 24, spacing: 7),
                      ],
                    ],
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              PillButton(
                label: state.picture == null ? 'Profilbild wählen' : 'Anderes Bild',
                icon: TrairIcons.user,
                kind: PillKind.ghost,
                height: 40,
                onPressed: () async {
                  final message = await state.pickPicture();
                  if (mounted) setState(() => _pictureMessage = message ?? '');
                },
              ),
              if (state.picture != null) ...[
                const SizedBox(width: 10),
                PillButton(label: 'Bild entfernen', kind: PillKind.ghost, height: 40, onPressed: state.removePicture),
              ],
              if (_pictureMessage.isNotEmpty) ...[
                const SizedBox(width: 12),
                Flexible(
                  child: Text(_pictureMessage, style: const TextStyle(fontSize: 13, color: TrairColors.amber)),
                ),
              ],
            ],
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
          const SizedBox(height: 18),
          FieldLabel('Über mich · ${_about.text.characters.length}/$maxAboutChars'),
          Container(
            padding: const EdgeInsets.fromLTRB(18, 12, 18, 12),
            decoration: BoxDecoration(
              color: TrairColors.field,
              borderRadius: BorderRadius.circular(20),
              border: Border.all(color: TrairColors.border),
            ),
            child: Semantics(
              label: 'Über mich',
              textField: true,
              child: TextField(
                controller: _about,
                minLines: 2,
                maxLines: 4,
                maxLength: maxAboutChars,
                onChanged: (text) {
                  state.setAbout(text);
                  setState(() {});
                },
                style: const TextStyle(fontSize: 14.5, height: 1.45, color: TrairColors.text),
                decoration: const InputDecoration.collapsed(
                  hintText: 'Ein, zwei Sätze über dich',
                  hintStyle: TextStyle(color: TrairColors.placeholder, fontSize: 14.5),
                ).copyWith(counterText: ''),
              ),
            ),
          ),
          _row(
            title: 'Auf dem Startbildschirm zeigen',
            hint: state.showOnStart
                ? 'Name, Farbe und Bild stehen vor dem Passwort in der Kontoliste, unverschlüsselt auf diesem PC.'
                : 'Vor dem Passwort steht nur „Konto ${state.account?.number ?? 1}“, ohne Name und Bild.',
            divider: false,
            trailing: PillSwitch(
              value: state.showOnStart,
              onChanged: state.setShowOnStart,
              label: 'Auf dem Startbildschirm zeigen',
            ),
          ),
          const SizedBox(height: 6),
          Align(
            alignment: Alignment.centerLeft,
            child: PillButton(
              label: 'So sehen dich andere',
              icon: TrairIcons.eye,
              kind: PillKind.ghost,
              height: 40,
              onPressed: () => showProfileCard(context, ProfileView.self(state)),
            ),
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

  Widget _devicesCard(AppState state) {
    final devices = state.devices;
    return SoftCard(
      key: _keys['Geräte'],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title(
            'Geräte',
            trailing: Text(devices.length == 1 ? '1 Gerät' : '${devices.length} Geräte', style: TrairText.small),
          ),
          for (final d in devices) ...[_deviceTile(state, d), const SizedBox(height: 8)],
          if (_deviceError.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Text(_deviceError, style: const TextStyle(fontSize: 13, color: TrairColors.amber)),
            ),
          const SizedBox(height: 4),
          if (state.pairing case final p?)
            _pairingPanel(state, p)
          else
            PillButton(
              label: 'Gerät koppeln',
              icon: TrairIcons.qrCode,
              kind: PillKind.dashed,
              expand: true,
              onPressed: state.canPair ? () => state.startPairing() : null,
            ),
          const SizedBox(height: 10),
          Text(
            state.serverStatus == ServerStatus.connected
                ? 'Auf dem neuen Gerät: Trair öffnen, „Ich habe schon ein Konto“ wählen und den Code eingeben. '
                      'Ein zweites Gerät rettet dein Konto, wenn dieser PC ausfällt.'
                : 'Koppeln geht über den Trair-Server: erst unten unter „Server“ verbinden. '
                      'Ein zweites Gerät rettet dein Konto, wenn dieser PC ausfällt.',
            style: TrairText.small,
          ),
        ],
      ),
    );
  }

  Widget _pairingPanel(AppState state, PairingProgress p) {
    final color = switch (p.stage) {
      PairStage.done => TrairColors.mint,
      PairStage.failed => TrairColors.amber,
      _ => TrairColors.textSoft,
    };
    final until = p.validUntil;
    final clock = '${until.hour.toString().padLeft(2, '0')}:${until.minute.toString().padLeft(2, '0')}';
    return Container(
      padding: const EdgeInsets.fromLTRB(18, 16, 18, 16),
      decoration: BoxDecoration(
        color: const Color(0x146E4FF6),
        borderRadius: BorderRadius.circular(20),
        border: Border.all(color: const Color(0x4D8B70FF)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (p.stage == PairStage.waiting || p.stage == PairStage.finishing) ...[
            const Text('Code für das neue Gerät', style: TrairText.label),
            const SizedBox(height: 8),
            SelectableText(
              p.code,
              textAlign: TextAlign.center,
              style: TrairText.mono.copyWith(
                fontSize: 26,
                fontWeight: FontWeight.w600,
                color: TrairColors.text,
                shadows: glowShadows(TrairColors.accentLight, strength: 30),
              ),
            ),
            const SizedBox(height: 6),
            Text('Gültig bis $clock Uhr, nur einmal.', textAlign: TextAlign.center, style: TrairText.small),
            const SizedBox(height: 12),
          ],
          Row(
            children: [
              if (p.stage == PairStage.done)
                const Icon(TrairIcons.check, size: 16, color: TrairColors.mint)
              else if (p.stage == PairStage.failed)
                const Icon(TrairIcons.triangleAlert, size: 16, color: TrairColors.amber),
              if (p.stage == PairStage.done || p.stage == PairStage.failed) const SizedBox(width: 8),
              Expanded(
                child: Text(p.message, style: TextStyle(fontSize: 13.5, color: color)),
              ),
              PillButton(
                label: p.stage == PairStage.done || p.stage == PairStage.failed ? 'Schließen' : 'Abbrechen',
                kind: PillKind.ghost,
                height: 36,
                onPressed: () => state.closePairing(),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _serverCard(AppState state) {
    final (color, label) = switch (state.serverStatus) {
      ServerStatus.connected => (TrairColors.mint, 'Verbunden'),
      ServerStatus.connecting => (TrairColors.accentLight, 'Verbinde …'),
      ServerStatus.failed => (TrairColors.amber, 'Fehler'),
      ServerStatus.off => (TrairColors.textMuted, 'Nicht verbunden'),
    };
    final connected = state.serverStatus == ServerStatus.connected;
    return SoftCard(
      key: _keys['Server'],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title(
            'Server',
            trailing: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  width: 8,
                  height: 8,
                  decoration: BoxDecoration(color: color, shape: BoxShape.circle),
                ),
                const SizedBox(width: 6),
                Text(label, style: TextStyle(fontSize: 13, color: color)),
              ],
            ),
          ),
          const FieldLabel('Adresse'),
          Row(
            children: [
              Expanded(
                child: PillField(
                  controller: _serverUrl,
                  hint: 'http://127.0.0.1:PORT',
                  mono: true,
                  height: 46,
                  semanticLabel: 'Server-Adresse',
                  onChanged: state.setServerUrl,
                ),
              ),
              const SizedBox(width: 10),
              PillButton(
                label: connected ? 'Trennen' : 'Verbinden',
                kind: connected ? PillKind.ghost : PillKind.primary,
                height: 46,
                onPressed: state.serverStatus == ServerStatus.connecting
                    ? null
                    : () => connected ? state.disconnect() : state.connect(),
              ),
            ],
          ),
          if (state.serverMessage.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(state.serverMessage, style: TextStyle(fontSize: 13, color: color)),
            ),
          const SizedBox(height: 10),
          const Text(
            'Vorerst läuft der Server nur auf diesem PC: starte trair-server.exe und verbinde dich. Er sieht nie '
            'deine Passwörter, Schlüssel oder Nachrichten, nur verschlüsselte Pakete.',
            style: TrairText.small,
          ),
        ],
      ),
    );
  }

  /// Bots (M9): vouch for a bot with the code its trair-bot showed.
  Widget _botsCard(AppState state) {
    final mine = state.ownBots;
    Future<void> vouch() async {
      if (_botCode.text.trim().isEmpty || _botBusy) return;
      setState(() => _botBusy = true);
      final ok = await state.vouchBot(_botCode.text, _botName.text);
      if (!mounted) return;
      setState(() => _botBusy = false);
      if (ok) {
        _botCode.clear();
        _botName.clear();
      }
    }

    return SoftCard(
      key: _keys['Bots'],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title('Bots'),
          const Text(
            'Ein Bot ist ein eigenes Konto, das das Programm trair-bot auf einem PC laufen lässt. Mit seinem Code '
            'bestätigst du, dass er dir gehört: Alle sehen dann „Bot von dir“, und erst dann darf er schreiben. Er '
            'liest nur Chats und Gruppen, in die man ihn holt.',
            style: TrairText.small,
          ),
          const SizedBox(height: 12),
          const FieldLabel('Bot-Code'),
          PillField(
            key: const ValueKey('bot-code'),
            controller: _botCode,
            hint: 'trairbot1:…',
            mono: true,
            height: 46,
            semanticLabel: 'Bot-Code',
          ),
          const SizedBox(height: 10),
          const FieldLabel('Name in deinen Kontakten'),
          Row(
            children: [
              Expanded(
                child: PillField(
                  key: const ValueKey('bot-name'),
                  controller: _botName,
                  hint: 'Mein Bot',
                  height: 46,
                  semanticLabel: 'Name des Bots',
                  onSubmitted: (_) => vouch(),
                ),
              ),
              const SizedBox(width: 10),
              PillButton(
                label: 'Bot bestätigen',
                icon: TrairIcons.bot,
                height: 46,
                onPressed: _botBusy || state.serverStatus != ServerStatus.connected ? null : vouch,
              ),
            ],
          ),
          if (state.botMessage.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                state.botMessage,
                style: TextStyle(
                  fontSize: 13,
                  color: state.botMessage.startsWith('Bestätigt') ? TrairColors.mint : TrairColors.amber,
                ),
              ),
            ),
          if (mine.isNotEmpty) ...[
            const SizedBox(height: 14),
            const FieldLabel('Deine Bots'),
            for (final b in mine)
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Row(
                  children: [
                    const BotChip(),
                    const SizedBox(width: 8),
                    Flexible(child: Text(state.displayName(b.user), overflow: TextOverflow.ellipsis)),
                    const SizedBox(width: 8),
                    Text('nennt sich „${b.name}“', style: TrairText.small),
                  ],
                ),
              ),
          ],
          const SizedBox(height: 10),
          const Text(
            'Einen Bot anlegen: trair-bot neu --daten MEIN-BOT --name "Wetterbot", dann den Code hier einfügen. '
            'Anleitung und Beispiele in Python und JavaScript: LIESMICH und README.',
            style: TrairText.small,
          ),
        ],
      ),
    );
  }

  Widget _deviceTile(AppState state, TrairDevice d) {
    final renaming = _renaming == d.id;
    final withdrawing = _withdrawing == d.id;
    return Container(
      padding: const EdgeInsets.fromLTRB(12, 12, 10, 12),
      decoration: BoxDecoration(
        color: TrairColors.field,
        borderRadius: BorderRadius.circular(18),
        border: withdrawing ? Border.all(color: const Color(0x42FF8A98)) : null,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Container(
                width: 38,
                height: 38,
                decoration: const BoxDecoration(color: Color(0x246E4FF6), shape: BoxShape.circle),
                child: Icon(
                  d.isThis ? TrairIcons.monitor : TrairIcons.monitorSmartphone,
                  size: 19,
                  color: TrairColors.accentPale,
                ),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      d.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      d.isThis ? 'Dieses Gerät · vom Konto unterschrieben' : 'Vom Konto unterschrieben',
                      style: TrairText.small,
                    ),
                    const SizedBox(height: 4),
                    SelectableText(
                      d.fingerprint,
                      style: TrairText.mono.copyWith(fontSize: 11.5, letterSpacing: 0.4, color: TrairColors.textMuted),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              if (d.isThis)
                RoundIconButton(
                  icon: TrairIcons.squarePen,
                  tooltip: 'Gerät umbenennen',
                  filled: false,
                  onPressed: _deviceBusy ? null : () => _startRename(d, renaming),
                )
              else
                PillButton(
                  label: 'Entziehen',
                  kind: PillKind.danger,
                  height: 34,
                  onPressed: _deviceBusy || withdrawing ? null : () => _startWithdraw(d),
                ),
            ],
          ),
          if (renaming) ...[
            const SizedBox(height: 10),
            PillField(
              controller: _deviceName,
              hint: 'Name, z. B. Julians Laptop',
              height: 42,
              autofocus: true,
              semanticLabel: 'Gerätename',
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) => _renameDevice(state, d),
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 10,
              runSpacing: 8,
              children: [
                PillButton(
                  label: 'Abbrechen',
                  kind: PillKind.ghost,
                  height: 38,
                  onPressed: () => setState(() {
                    _renaming = null;
                    _deviceError = '';
                  }),
                ),
                PillButton(
                  label: _deviceBusy ? 'Speichere …' : 'Name speichern',
                  height: 38,
                  onPressed: _deviceBusy || _deviceName.text.trim().isEmpty ? null : () => _renameDevice(state, d),
                ),
              ],
            ),
            const SizedBox(height: 6),
            const Text('Der Name bleibt auf diesem Gerät. Andere sehen ihn nie.', style: TrairText.small),
          ],
          if (withdrawing) ...[
            const SizedBox(height: 12),
            Text(
              '„${d.name}“ entziehen? Das Gerät wird danach überall abgelehnt und kann nichts mehr lesen oder senden. '
              'Rückgängig geht nicht. Hat jemand das Gerät und sein Passwort, hilft Entziehen nicht ganz: Dann besser '
              'ein neues Konto anlegen. Zur Sicherheit dein Passwort:',
              style: const TextStyle(fontSize: 13.5, height: 1.5, color: TrairColors.roseText),
            ),
            const SizedBox(height: 10),
            PillField(
              controller: _withdrawPw,
              hint: 'Dein Passwort',
              obscure: true,
              height: 42,
              semanticLabel: 'Passwort zum Entziehen',
              onChanged: (_) => setState(() {}),
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 10,
              runSpacing: 8,
              children: [
                PillButton(
                  label: 'Abbrechen',
                  kind: PillKind.ghost,
                  height: 38,
                  onPressed: () => setState(() {
                    _withdrawing = null;
                    _withdrawPw.clear();
                    _deviceError = '';
                  }),
                ),
                PillButton(
                  label: _deviceBusy ? 'Entziehe …' : 'Endgültig entziehen',
                  kind: PillKind.danger,
                  height: 38,
                  onPressed: _deviceBusy || _withdrawPw.text.isEmpty ? null : () => _withdrawDevice(state, d),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }

  void _startRename(TrairDevice d, bool open) => setState(() {
    _renaming = open ? null : d.id;
    _withdrawing = null;
    _deviceName.text = d.label ?? '';
    _deviceError = '';
  });

  void _startWithdraw(TrairDevice d) => setState(() {
    _withdrawing = d.id;
    _renaming = null;
    _withdrawPw.clear();
    _deviceError = '';
  });

  Future<void> _addBadge(AppState state) async {
    if (_badgeBusy || _badgeInput.text.trim().isEmpty) return;
    setState(() => _badgeBusy = true);
    final answer = await state.addBadge(_badgeInput.text);
    if (!mounted) return;
    setState(() {
      _badgeBusy = false;
      _badgeOk = answer.ok;
      _badgeMessage = answer.ok
          ? 'Abzeichen geprüft und gespeichert.'
          : switch (answer.error) {
              'bad_request' => 'Das ist keine Abzeichen-Zeile. Kopiere die ganze Zeile, die mit { beginnt.',
              'refused' => 'Abgelehnt: nicht für dein Konto, abgelaufen oder nicht echt unterschrieben.',
              _ => vaultErrorText(answer),
            };
      if (answer.ok) _badgeInput.clear();
    });
  }

  Future<void> _renameDevice(AppState state, TrairDevice d) async {
    if (_deviceBusy || _deviceName.text.trim().isEmpty) return;
    setState(() {
      _deviceBusy = true;
      _deviceError = '';
    });
    final answer = await state.renameDevice(d.id, _deviceName.text);
    if (!mounted) return;
    setState(() {
      _deviceBusy = false;
      if (answer.ok) {
        _renaming = null;
      } else {
        _deviceError = answer.error == 'refused'
            ? 'Der Name braucht 1 bis 40 Zeichen, ohne Steuerzeichen.'
            : vaultErrorText(answer);
      }
    });
  }

  Future<void> _withdrawDevice(AppState state, TrairDevice d) async {
    setState(() {
      _deviceBusy = true;
      _deviceError = '';
    });
    final answer = await state.withdrawDevice(d.id, _withdrawPw.text);
    if (!mounted) return;
    setState(() {
      _deviceBusy = false;
      _withdrawPw.clear();
      if (answer.ok) {
        _withdrawing = null;
      } else {
        _deviceError = answer.error == 'refused' ? 'Dieses Gerät kann nicht entzogen werden.' : vaultErrorText(answer);
      }
    });
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
            title: 'Automatisch sperren',
            hint: 'Nach so vielen Minuten ohne Benutzung. Strg+L sperrt sofort.',
            trailing: Wrap(
              spacing: 6,
              children: [
                for (final m in autoLockChoices)
                  PillButton(
                    label: '$m Min.',
                    kind: m == state.autoLockMinutes ? PillKind.primary : PillKind.ghost,
                    height: 36,
                    onPressed: () => state.setAutoLock(m),
                  ),
              ],
            ),
          ),
          _row(
            title: 'Bildschirmfotos blockieren',
            hint: state.screenProtectionSupported || !state.screenProtection
                ? 'Andere Programme und Bildschirmaufnahmen sehen das Trair-Fenster nur schwarz.'
                : 'Gibt es nur unter Windows. Hier wirkt der Schalter nicht.',
            trailing: PillSwitch(
              value: state.screenProtection,
              onChanged: state.setScreenProtection,
              label: 'Bildschirmfotos blockieren',
            ),
          ),
          _row(
            title: 'Jetzt sperren',
            hint: 'Trair vergisst den Schlüssel sofort. Zum Öffnen braucht es wieder das Passwort.',
            trailing: PillButton(label: 'Sperren', icon: TrairIcons.lock, height: 42, onPressed: state.lock),
          ),
          _passwordChange(state),
          _row(
            title: 'Keine Wiederherstellung',
            hint:
                'Passwort vergessen und kein zweites Gerät gekoppelt heißt: Konto für immer weg. Niemand kann es zurückholen.',
            divider: false,
            leading: Container(
              width: 34,
              height: 34,
              decoration: const BoxDecoration(color: Color(0x1FFF8A98), shape: BoxShape.circle),
              child: const Icon(TrairIcons.lock, size: 16, color: TrairColors.rose),
            ),
          ),
        ],
      ),
    );
  }

  Widget _passwordChange(AppState state) {
    final ready = !_pwBusy && _oldPw.text.isNotEmpty && _newPw1.text.runes.length >= 12 && _newPw1.text == _newPw2.text;
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 14),
      decoration: const BoxDecoration(
        border: Border(bottom: BorderSide(color: TrairColors.borderSoft)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text('Passwort ändern', style: TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600)),
          const SizedBox(height: 3),
          const Text('Braucht das alte Passwort, auch wenn Trair gerade offen ist.', style: TrairText.muted),
          const SizedBox(height: 12),
          PillField(
            controller: _oldPw,
            hint: 'Altes Passwort',
            obscure: true,
            height: 44,
            semanticLabel: 'Altes Passwort',
            onChanged: (_) => setState(() {}),
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              Expanded(
                child: PillField(
                  controller: _newPw1,
                  hint: 'Neues Passwort (mind. 12 Zeichen)',
                  obscure: true,
                  height: 44,
                  semanticLabel: 'Neues Passwort',
                  onChanged: (_) => setState(() {}),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: PillField(
                  controller: _newPw2,
                  hint: 'Neues Passwort wiederholen',
                  obscure: true,
                  height: 44,
                  semanticLabel: 'Neues Passwort wiederholen',
                  onChanged: (_) => setState(() {}),
                ),
              ),
            ],
          ),
          const SizedBox(height: 10),
          Row(
            children: [
              PillButton(
                label: _pwBusy ? 'Ändere …' : 'Passwort ändern',
                height: 40,
                onPressed: ready ? () => _changePassword(state) : null,
              ),
              const SizedBox(width: 12),
              Flexible(
                child: Text(_pwMessage, style: TextStyle(fontSize: 13, color: _pwMessageColor)),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Future<void> _changePassword(AppState state) async {
    setState(() {
      _pwBusy = true;
      _pwMessage = '';
    });
    final answer = await state.changePassword(_oldPw.text, _newPw1.text);
    if (!mounted) return;
    setState(() {
      _pwBusy = false;
      if (answer.ok) {
        _oldPw.clear();
        _newPw1.clear();
        _newPw2.clear();
        _pwMessage = 'Neues Passwort gilt ab jetzt.';
        _pwMessageColor = TrairColors.mintText;
      } else {
        _oldPw.clear();
        _pwMessage = vaultErrorText(answer);
        _pwMessageColor = TrairColors.amber;
      }
    });
  }

  // ------------------------------------------------------- emergency PINs

  Widget _notfallCard(AppState state) {
    final pins = state.notfallPins;
    return SoftCard(
      key: _keys['Notfall'],
      borderColor: const Color(0x29FFB547),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _title(
            'Notfall-PINs',
            trailing: pins == null
                ? null
                : PillButton(label: 'Verbergen', kind: PillKind.ghost, height: 36, onPressed: () => _hidePins(state)),
          ),
          const Text(
            'Tippst du auf dem Startbildschirm eine Notfall-PIN statt des Passworts ein, löscht Trair dieses Konto '
            'mit allen Nachrichten auf diesem PC. Vorher kann die PIN noch eine Nachricht an einen Chat schicken. '
            'Für alle anderen sieht es aus wie ein falsch getipptes Passwort.',
            style: TrairText.muted,
          ),
          const SizedBox(height: 14),
          if (pins == null) ..._pinsLocked(state) else ..._pinsOpen(state, pins),
        ],
      ),
    );
  }

  /// Hidden until the password: nobody at an open Trair sees whether
  /// PINs are set.
  List<Widget> _pinsLocked(AppState state) => [
    const FieldLabel('Passwort, um die Notfall-PINs zu zeigen'),
    Row(
      children: [
        Expanded(
          child: PillField(
            controller: _pinPw,
            hint: 'Passwort eingeben',
            obscure: true,
            height: 44,
            semanticLabel: 'Passwort für Notfall-PINs',
            onChanged: (_) => setState(() => _pinMessage = ''),
            onSubmitted: (_) => _showPins(state),
          ),
        ),
        const SizedBox(width: 10),
        PillButton(
          label: _pinBusy ? 'Prüfe …' : 'Anzeigen',
          height: 44,
          onPressed: _pinBusy || _pinPw.text.isEmpty ? null : () => _showPins(state),
        ),
      ],
    ),
    if (_pinMessage.isNotEmpty) ...[
      const SizedBox(height: 8),
      Text(_pinMessage, style: TextStyle(fontSize: 13, color: _pinOk ? TrairColors.mintText : TrairColors.amber)),
    ],
  ];

  List<Widget> _pinsOpen(AppState state, List<NotfallPin> pins) => [
    if (pins.isEmpty)
      const Padding(
        padding: EdgeInsets.only(bottom: 6),
        child: Text('Noch keine Notfall-PIN.', style: TextStyle(fontSize: 13.5, color: TrairColors.textSoft)),
      ),
    for (final pin in pins) _pinTile(state, pin),
    const SizedBox(height: 10),
    if (pins.length < 3)
      ..._pinForm(state)
    else
      const Text('3 von 3 Plätzen belegt. Entferne eine PIN, um eine neue zu setzen.', style: TrairText.muted),
    if (_pinMessage.isNotEmpty) ...[
      const SizedBox(height: 10),
      Text(_pinMessage, style: TextStyle(fontSize: 13, color: _pinOk ? TrairColors.mintText : TrairColors.amber)),
    ],
  ];

  Widget _pinTile(AppState state, NotfallPin pin) {
    final what = pin.sends ? 'Schreibt an ${pin.chatName ?? 'einen Chat'}, dann löschen' : 'Löscht alles';
    final after = pin.after == 'vanish'
        ? 'Danach verschwindet das Konto aus der Liste.'
        : 'Danach bleibt das Konto stehen, jedes Passwort gilt als falsch.';
    return _row(
      title: what,
      hint: pin.sends ? '„${pin.text}“\n$after' : after,
      leading: Container(
        width: 34,
        height: 34,
        decoration: const BoxDecoration(color: Color(0x1FFFB547), shape: BoxShape.circle),
        child: Icon(pin.sends ? TrairIcons.arrowUp : TrairIcons.triangleAlert, size: 16, color: TrairColors.amber),
      ),
      trailing: PillButton(
        label: 'Entfernen',
        kind: PillKind.ghost,
        height: 36,
        onPressed: _pinBusy ? null : () => _removePin(state, pin),
      ),
    );
  }

  List<Widget> _pinForm(AppState state) {
    final chats = state.realChats.where((c) => c.state == 'active' && !c.isChannel).toList();
    if (_pinChat != null && !chats.any((c) => c.id == _pinChat)) _pinChat = null;
    final min = _pinSends ? 12 : 6;
    final text = _pinText.text.trim();
    final ready =
        !_pinBusy &&
        _newPin1.text.runes.length >= min &&
        _newPin1.text == _newPin2.text &&
        (!_pinSends || (_pinChat != null && text.isNotEmpty && text.runes.length <= 500));
    // As wide as the label: in a Wrap a button would otherwise fill the row.
    Widget choice(String label, bool on, VoidCallback pick) => IntrinsicWidth(
      child: PillButton(
        label: label,
        kind: on ? PillKind.primary : PillKind.ghost,
        height: 36,
        onPressed: () => setState(() {
          pick();
          _pinMessage = '';
        }),
      ),
    );
    return [
      const FieldLabel('Was die neue PIN tut'),
      Wrap(
        spacing: 8,
        runSpacing: 8,
        children: [
          choice('Nur löschen', !_pinSends, () => _pinSends = false),
          choice('Erst schreiben, dann löschen', _pinSends, () => _pinSends = true),
        ],
      ),
      if (_pinSends) ...[
        const SizedBox(height: 14),
        const FieldLabel('An welchen Chat'),
        if (chats.isEmpty)
          const Text(
            'Noch kein echter Chat. Starte erst einen, dann kann eine PIN dorthin schreiben.',
            style: TrairText.muted,
          )
        else
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [for (final c in chats) choice(c.name, _pinChat == c.id, () => _pinChat = c.id)],
          ),
        const SizedBox(height: 10),
        PillField(
          controller: _pinText,
          hint: 'Nachricht, zum Beispiel: Ich brauche Hilfe, ruf mich an.',
          height: 44,
          semanticLabel: 'Nachricht der Notfall-PIN',
          onChanged: (_) => setState(() => _pinMessage = ''),
        ),
      ],
      const SizedBox(height: 14),
      const FieldLabel('Danach'),
      Wrap(
        spacing: 8,
        runSpacing: 8,
        children: [
          choice('Konto verschwindet', _pinAfter == 'vanish', () => _pinAfter = 'vanish'),
          choice('Konto bleibt, Passwort „falsch“', _pinAfter == 'wrong_password', () => _pinAfter = 'wrong_password'),
        ],
      ),
      const SizedBox(height: 14),
      FieldLabel(_pinSends ? 'PIN (mindestens 12 Zeichen, sie öffnet die Schlüssel)' : 'PIN (mindestens 6 Zeichen)'),
      Row(
        children: [
          Expanded(
            child: PillField(
              controller: _newPin1,
              hint: 'Neue PIN',
              obscure: true,
              height: 44,
              semanticLabel: 'Neue Notfall-PIN',
              onChanged: (_) => setState(() => _pinMessage = ''),
            ),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: PillField(
              controller: _newPin2,
              hint: 'PIN wiederholen',
              obscure: true,
              height: 44,
              semanticLabel: 'Notfall-PIN wiederholen',
              onChanged: (_) => setState(() => _pinMessage = ''),
            ),
          ),
        ],
      ),
      const SizedBox(height: 10),
      Row(
        children: [
          PillButton(
            label: _pinBusy ? 'Speichere …' : 'PIN speichern',
            icon: TrairIcons.plus,
            height: 40,
            onPressed: ready ? () => _addPin(state) : null,
          ),
        ],
      ),
    ];
  }

  Future<void> _showPins(AppState state) async {
    if (_pinBusy || _pinPw.text.isEmpty) return;
    setState(() {
      _pinBusy = true;
      _pinMessage = '';
    });
    final answer = await state.showNotfallPins(_pinPw.text);
    if (!mounted) return;
    setState(() {
      _pinBusy = false;
      if (!answer.ok) {
        _pinPw.clear();
        _pinMessage = vaultErrorText(answer);
        _pinOk = false;
      }
    });
  }

  void _hidePins(AppState state) {
    for (final c in [_pinPw, _newPin1, _newPin2, _pinText]) {
      c.clear();
    }
    setState(() {
      _pinMessage = '';
      _pinSends = false;
      _pinChat = null;
      _pinAfter = 'vanish';
    });
    state.hideNotfallPins();
  }

  Future<void> _addPin(AppState state) async {
    setState(() {
      _pinBusy = true;
      _pinMessage = '';
    });
    final answer = await state.addNotfallPin(
      pin: _newPin1.text,
      password: _pinPw.text,
      after: _pinAfter,
      chat: _pinSends ? _pinChat : null,
      text: _pinSends ? _pinText.text.trim() : null,
    );
    if (!mounted) return;
    setState(() {
      _pinBusy = false;
      _pinOk = answer.ok;
      if (answer.ok) {
        _newPin1.clear();
        _newPin2.clear();
        _pinText.clear();
        _pinMessage = 'Notfall-PIN gespeichert. Probier sie nicht aus: Sie löscht wirklich.';
      } else {
        _pinMessage = vaultErrorText(answer);
      }
    });
  }

  Future<void> _removePin(AppState state, NotfallPin pin) async {
    setState(() {
      _pinBusy = true;
      _pinMessage = '';
    });
    final answer = await state.removeNotfallPin(pin.id, _pinPw.text);
    if (!mounted) return;
    setState(() {
      _pinBusy = false;
      _pinOk = answer.ok;
      _pinMessage = answer.ok ? 'Notfall-PIN entfernt.' : vaultErrorText(answer);
    });
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
          _row(
            title: 'Profil sichtbar für',
            hint: 'Wer Name, „Über mich“ und Abzeichen sieht. Gilt, sobald der Server läuft.',
            trailing: Wrap(
              spacing: 6,
              runSpacing: 6,
              children: [
                for (final c in profileVisibilityChoices.entries)
                  PillButton(
                    label: c.value,
                    kind: c.key == state.profileVisibility ? PillKind.primary : PillKind.ghost,
                    height: 36,
                    onPressed: () => state.setProfileVisibility(c.key),
                  ),
              ],
            ),
          ),
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
          const Text('Fingerabdruck deines Kontos', style: TrairText.label),
          const SizedBox(height: 8),
          Container(
            padding: const EdgeInsets.fromLTRB(16, 10, 8, 10),
            decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(16)),
            child: Row(
              children: [
                Expanded(
                  child: SelectableText(
                    state.fingerprint,
                    style: TrairText.mono.copyWith(fontSize: 13, letterSpacing: 0.6, color: const Color(0xFFE2DCFF)),
                  ),
                ),
                RoundIconButton(
                  icon: TrairIcons.copy,
                  tooltip: 'Kopieren (verschwindet nach 60 Sekunden)',
                  filled: false,
                  onPressed: () => copySensitive(state.fingerprint),
                ),
              ],
            ),
          ),
          const SizedBox(height: 8),
          const Text(
            'Andere prüfen dich über diese Zahlen. Meldest du dich 6 Monate lang nicht an, löscht der Server dein Konto '
            'automatisch und entfernt es aus allen Gruppen und Communities.',
            style: TrairText.muted,
          ),
          const SizedBox(height: 16),
          const Text('Öffentlicher Kontoschlüssel', style: TrairText.label),
          const SizedBox(height: 8),
          Container(
            padding: const EdgeInsets.fromLTRB(16, 10, 8, 10),
            decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(16)),
            child: Row(
              children: [
                Expanded(
                  child: SelectableText(
                    state.accountKey,
                    style: TrairText.mono.copyWith(fontSize: 11.5, letterSpacing: 0.3, color: TrairColors.textSoft),
                  ),
                ),
                RoundIconButton(
                  icon: TrairIcons.copy,
                  tooltip: 'Kontoschlüssel kopieren',
                  filled: false,
                  onPressed: () => copySensitive(state.accountKey),
                ),
              ],
            ),
          ),
          const SizedBox(height: 8),
          const Text(
            'Nicht geheim. Das Team-Werkzeug braucht ihn, um dir ein Abzeichen zu unterschreiben.',
            style: TrairText.muted,
          ),
          const SizedBox(height: 16),
          const FieldLabel('Abzeichen hinzufügen'),
          Row(
            children: [
              Expanded(
                child: PillField(
                  controller: _badgeInput,
                  hint: 'Zeile aus dem Team-Werkzeug einfügen',
                  height: 46,
                  semanticLabel: 'Abzeichen einfügen',
                  onChanged: (_) => setState(() => _badgeMessage = ''),
                  onSubmitted: (_) => _addBadge(state),
                ),
              ),
              const SizedBox(width: 10),
              PillButton(
                label: _badgeBusy ? 'Prüfe …' : 'Hinzufügen',
                height: 46,
                onPressed: _badgeBusy || _badgeInput.text.trim().isEmpty ? null : () => _addBadge(state),
              ),
            ],
          ),
          if (_badgeMessage.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                _badgeMessage,
                style: TextStyle(fontSize: 13, color: _badgeOk ? TrairColors.mint : TrairColors.amber),
              ),
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
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        const Text(
                          'Wirklich löschen? Der Tresor mit deinem Kontoschlüssel wird überschrieben und gelöscht. '
                          'Das kann niemand rückgängig machen. Zur Sicherheit noch einmal das Passwort:',
                          style: TextStyle(fontSize: 14, height: 1.5, color: TrairColors.roseText),
                        ),
                        const SizedBox(height: 12),
                        PillField(
                          controller: _deletePw,
                          hint: 'Passwort',
                          obscure: true,
                          height: 44,
                          semanticLabel: 'Passwort zum Löschen',
                          onChanged: (_) => setState(() {}),
                        ),
                        if (_deleteError.isNotEmpty)
                          Padding(
                            padding: const EdgeInsets.only(top: 8),
                            child: Text(_deleteError, style: const TextStyle(fontSize: 13, color: TrairColors.amber)),
                          ),
                        const SizedBox(height: 12),
                        Wrap(
                          spacing: 10,
                          children: [
                            PillButton(
                              label: 'Abbrechen',
                              kind: PillKind.ghost,
                              height: 42,
                              onPressed: () => setState(() {
                                _askDelete = false;
                                _deletePw.clear();
                                _deleteError = '';
                              }),
                            ),
                            PillButton(
                              label: _deleteBusy ? 'Lösche …' : 'Endgültig löschen',
                              kind: PillKind.danger,
                              height: 42,
                              onPressed: _deletePw.text.isEmpty || _deleteBusy ? null : () => _delete(state),
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

  Future<void> _delete(AppState state) async {
    setState(() {
      _deleteBusy = true;
      _deleteError = '';
    });
    final answer = await state.deleteAccount(_deletePw.text);
    if (!mounted) return;
    setState(() {
      _deleteBusy = false;
      _deletePw.clear();
      if (!answer.ok) _deleteError = vaultErrorText(answer);
    });
  }
}
