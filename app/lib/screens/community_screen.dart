// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../backend/backend.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';

class CommunityScreen extends StatefulWidget {
  const CommunityScreen({super.key});

  @override
  State<CommunityScreen> createState() => _CommunityScreenState();
}

class _CommunityScreenState extends State<CommunityScreen> {
  String _active = 'allgemein';
  bool _invite = false;
  bool _info = false;
  final _draft = TextEditingController();

  @override
  void dispose() {
    _draft.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final community = state.backend.community();
    final channel = community.categories.expand((c) => c.channels).firstWhere((c) => c.id == _active);
    return Backdrop(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SizedBox(width: 288, child: _sidebar(state, community)),
          Expanded(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(0, 10, 10, 10),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Expanded(child: _channel(state, channel)),
                  const SizedBox(width: 10),
                  SizedBox(width: 260, child: _members(community)),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _sidebar(AppState state, Community community) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(18, 18, 14, 14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const SidebarHeader(trailing: BackToChats()),
          const SizedBox(height: 18),
          GlassPanel(
            lite: true,
            radius: 22,
            padding: const EdgeInsets.all(12),
            child: Row(
              children: [
                Avatar(initials: community.initials, color: TrairColors.accentPale, size: 44, square: true),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(community.name, style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600)),
                      Text('${community.memberCount} Mitglieder · geschlossene Beta', style: TrairText.small),
                    ],
                  ),
                ),
                RoundIconButton(
                  icon: TrairIcons.userPlus,
                  tooltip: 'Leute einladen',
                  color: _invite ? const Color(0x336E4FF6) : null,
                  onPressed: () => setState(() => _invite = !_invite),
                ),
              ],
            ),
          ),
          AnimatedSize(
            duration: const Duration(milliseconds: 220),
            child: _invite
                ? Padding(
                    padding: const EdgeInsets.only(top: 10),
                    child: Container(
                      padding: const EdgeInsets.all(14),
                      decoration: BoxDecoration(
                        color: const Color(0x176E4FF6),
                        borderRadius: BorderRadius.circular(22),
                        border: Border.all(color: const Color(0x38A18DFF)),
                      ),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          const Text('Einladungscode · nur einmal nutzbar, 24 Std. gültig', style: TrairText.small),
                          const SizedBox(height: 6),
                          Text('TRAIR-7KQ2-M9XD', style: TrairText.mono.copyWith(fontSize: 16, letterSpacing: 1.4)),
                        ],
                      ),
                    ),
                  )
                : const SizedBox(width: double.infinity),
          ),
          const SizedBox(height: 14),
          Expanded(
            child: ListView(
              children: [
                for (final cat in community.categories) ...[
                  Padding(
                    padding: const EdgeInsets.fromLTRB(12, 10, 12, 4),
                    child: Text(cat.name, style: TrairText.small),
                  ),
                  for (final ch in cat.channels) _channelRow(ch),
                ],
              ],
            ),
          ),
          const SizedBox(height: 10),
          const ProfileCard(subtitle: 'Gründer · Trust 3'),
        ],
      ),
    );
  }

  Widget _channelRow(Channel ch) {
    final active = ch.id == _active;
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Material(
        color: active ? TrairColors.cardActive : Colors.transparent,
        borderRadius: BorderRadius.circular(14),
        child: InkWell(
          borderRadius: BorderRadius.circular(14),
          onTap: () => setState(() {
            _active = ch.id;
            _info = false;
            _draft.clear();
          }),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            child: Row(
              children: [
                Icon(
                  ch.open ? TrairIcons.eye : TrairIcons.lock,
                  size: 16,
                  color: ch.open ? TrairColors.amber : TrairColors.accentLight,
                  semanticLabel: ch.open ? 'offen' : 'geschlossen',
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    ch.name,
                    style: TextStyle(
                      fontSize: 14.5,
                      fontWeight: active ? FontWeight.w600 : FontWeight.w400,
                      color: active ? TrairColors.text : const Color(0xFFB4B2C2),
                    ),
                  ),
                ),
                if (ch.teamOnly)
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                    decoration: BoxDecoration(color: const Color(0x0FFFFFFF), borderRadius: BorderRadius.circular(999)),
                    child: const Text('Team', style: TextStyle(fontSize: 11, color: TrairColors.textSoft)),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _channel(AppState state, Channel channel) {
    final messages = state.backend.channelMessages(channel.id);

    void send() {
      final text = _draft.text.trim();
      if (text.isEmpty) return;
      state.backend.sendToChannel(channel.id, text);
      _draft.clear();
      state.touch();
    }

    return GlassPanel(
      child: Stack(
        children: [
          Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(22, 16, 16, 12),
                child: Row(
                  children: [
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text.rich(
                            TextSpan(
                              children: [
                                const TextSpan(
                                  text: '# ',
                                  style: TextStyle(color: TrairColors.textMuted),
                                ),
                                TextSpan(text: channel.name),
                              ],
                            ),
                            style: TrairText.heading,
                          ),
                          Text(channel.topic, style: TrairText.small),
                        ],
                      ),
                    ),
                    SecurityCapsule(
                      text: channel.open ? 'Offen · für Moderation lesbar' : 'Geschlossen · Ende-zu-Ende',
                      tone: channel.open ? CapsuleTone.warn : CapsuleTone.safe,
                      open: _info,
                      onTap: () => setState(() => _info = !_info),
                    ),
                    Expanded(
                      child: Row(
                        mainAxisAlignment: MainAxisAlignment.end,
                        children: [
                          RoundIconButton(
                            icon: TrairIcons.search,
                            tooltip: 'Im Kanal suchen',
                            filled: false,
                            onPressed: () {},
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
              AnimatedSize(
                duration: const Duration(milliseconds: 250),
                child: _info
                    ? Padding(
                        padding: const EdgeInsets.fromLTRB(20, 0, 20, 12),
                        child: Center(
                          child: ConstrainedBox(
                            constraints: const BoxConstraints(maxWidth: 440),
                            child: GlassPanel(
                              lite: true,
                              radius: 22,
                              color: TrairColors.glassPopup,
                              padding: const EdgeInsets.all(16),
                              child: Text(
                                channel.open
                                    ? 'Dieser Kanal ist offen: Nachrichten liegen lesbar auf dem Server, damit Moderation '
                                          'möglich ist. Schreib hier nichts Privates.'
                                    : 'Dieser Kanal ist geschlossen: Nur Mitglieder mit Schlüssel können mitlesen. '
                                          'Auch Admins und der Server sehen den Inhalt nicht.',
                                style: const TextStyle(fontSize: 13.5, height: 1.5, color: TrairColors.textSoft),
                              ),
                            ),
                          ),
                        ),
                      )
                    : const SizedBox(width: double.infinity),
              ),
              Expanded(
                child: ListView(
                  padding: const EdgeInsets.fromLTRB(26, 8, 26, 110),
                  children: [for (final m in messages) _message(m)],
                ),
              ),
            ],
          ),
          Positioned(
            left: 0,
            right: 0,
            bottom: 18,
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 680),
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 20),
                  child: GlassPanel(
                    lite: true,
                    radius: 999,
                    padding: const EdgeInsets.fromLTRB(20, 6, 6, 6),
                    child: Row(
                      children: [
                        Expanded(
                          child: Semantics(
                            label: 'Nachricht schreiben',
                            textField: true,
                            child: TextField(
                              key: const Key('channel-composer'),
                              controller: _draft,
                              onSubmitted: (_) => send(),
                              style: const TextStyle(fontSize: 14.5, color: TrairColors.text),
                              decoration: InputDecoration.collapsed(
                                hintText: 'Nachricht an #${channel.name}',
                                hintStyle: const TextStyle(color: TrairColors.placeholder, fontSize: 14.5),
                              ),
                            ),
                          ),
                        ),
                        RoundIconButton(
                          icon: TrairIcons.arrowUp,
                          tooltip: 'Senden',
                          size: 40,
                          color: TrairColors.accent,
                          iconColor: Colors.white,
                          onPressed: send,
                        ),
                      ],
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

  Widget _message(CommunityMessage m) {
    final p = m.author;
    return Padding(
      padding: const EdgeInsets.only(bottom: 20),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Avatar(initials: p.name.substring(0, 1), color: p.color, size: 38),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Wrap(
                  spacing: 8,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Text(
                      p.name,
                      style: TextStyle(
                        fontSize: 14.5,
                        fontWeight: FontWeight.w600,
                        color: p.color,
                        shadows: glowShadows(p.color, strength: 20),
                      ),
                    ),
                    if (p.role.isNotEmpty)
                      Container(
                        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                        decoration: BoxDecoration(
                          color: const Color(0x0FFFFFFF),
                          borderRadius: BorderRadius.circular(999),
                        ),
                        child: Text(p.role, style: const TextStyle(fontSize: 11, color: TrairColors.textSoft)),
                      ),
                    Text(m.time, style: const TextStyle(fontSize: 12, color: TrairColors.placeholder)),
                  ],
                ),
                const SizedBox(height: 3),
                Text(m.text, style: TrairText.body.copyWith(color: const Color(0xFFE3E1EC))),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _members(Community community) {
    return GlassPanel(
      padding: const EdgeInsets.fromLTRB(12, 18, 12, 14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Expanded(
            child: ListView(
              children: [
                for (final r in community.roles) ...[
                  Padding(
                    padding: const EdgeInsets.fromLTRB(10, 8, 10, 4),
                    child: Text('${r.name} · ${r.members.length}', style: TrairText.small),
                  ),
                  for (final p in r.members)
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                      child: Row(
                        children: [
                          Avatar(initials: p.name.substring(0, 1), color: p.color, size: 32),
                          const SizedBox(width: 10),
                          Expanded(
                            child: Text(
                              p.name,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500, color: p.color),
                            ),
                          ),
                          Tooltip(
                            message: 'Trust-Stufe ${p.trust} von 3',
                            child: Container(
                              padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 5),
                              decoration: BoxDecoration(
                                color: const Color(0x0AFFFFFF),
                                borderRadius: BorderRadius.circular(999),
                              ),
                              child: Row(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  for (var i = 1; i <= 3; i++)
                                    Container(
                                      width: 6,
                                      height: 6,
                                      margin: const EdgeInsets.symmetric(horizontal: 1.5),
                                      decoration: BoxDecoration(
                                        color: i <= p.trust ? TrairColors.mint : const Color(0x24FFFFFF),
                                        shape: BoxShape.circle,
                                      ),
                                    ),
                                ],
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                ],
              ],
            ),
          ),
          const SizedBox(height: 8),
          Container(
            padding: const EdgeInsets.all(12),
            decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(18)),
            child: const Text(
              'Die Punkte zeigen die Trust-Stufe (0 bis 3). Wer online ist, wird nicht angezeigt.',
              style: TextStyle(fontSize: 12, height: 1.45, color: TrairColors.textMuted),
            ),
          ),
        ],
      ),
    );
  }
}
