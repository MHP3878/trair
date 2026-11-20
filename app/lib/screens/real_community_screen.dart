// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../backend/backend.dart' show Person;
import '../community_state.dart';
import '../core/clipboard.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/profile_popup.dart';
import '../widgets/trair_icons.dart';
import 'community_dialogs.dart';
import 'community_settings.dart';
import 'group_panel.dart' show confirmAction, initialsOf, whenText;

/// A real community (M8): channels on the left, the open channel in the
/// middle, members on the right. Everything a button does is decided again
/// by the server; the screen only offers what this account's rights allow.
class RealCommunityScreen extends StatefulWidget {
  const RealCommunityScreen({super.key});

  @override
  State<RealCommunityScreen> createState() => _RealCommunityScreenState();
}

class _RealCommunityScreenState extends State<RealCommunityScreen> {
  final _draft = TextEditingController();
  bool _invite = false;
  bool _info = false;
  bool _settings = false;

  @override
  void dispose() {
    _draft.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final c = state.communities;
    final info = c.current;
    if (info == null) {
      return Backdrop(
        child: Center(
          child: Text(c.message.isEmpty ? 'Lädt …' : c.message, style: TrairText.muted),
        ),
      );
    }
    final channel = info.channels.where((ch) => ch.id == c.currentChannel).firstOrNull;
    return Backdrop(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(width: 280, color: TrairColors.sidebar, child: _sidebar(state, info)),
          Expanded(
            child: _settings
                ? CommunitySettings(info: info, onClose: () => setState(() => _settings = false))
                : channel == null
                ? const GlassPanel(child: Center(child: Text('Kein Kanal', style: TrairText.muted)))
                : _channel(state, info, channel),
          ),
          if (!_settings) SizedBox(width: 248, child: _members(state, info)),
        ],
      ),
    );
  }

  String _myRole(CommunityInfo info, String me) {
    if (info.iAmOwner) return 'Gründer';
    final m = info.member(me);
    return (m == null ? null : info.topRole(m))?.name ?? 'Mitglied';
  }

  // ------------------------------------------------------------ sidebar

  Widget _sidebar(AppState state, CommunityInfo info) {
    final c = state.communities;
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
                  child: SidebarHeader(title: 'Community', trailing: BackToChats()),
                ),
                const SizedBox(height: 10),
                GlassPanel(
                  lite: true,
                  radius: 12,
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      Avatar(initials: initialsOf(info.name), color: TrairColors.accentPale, size: 44, square: true),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              info.name,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
                            ),
                            Text('${info.members.length} Mitglieder · offen', style: TrairText.small),
                          ],
                        ),
                      ),
                      if (info.can(Perm.invite))
                        RoundIconButton(
                          icon: TrairIcons.userPlus,
                          tooltip: 'Leute einladen',
                          size: 34,
                          color: _invite ? const Color(0x336E4FF6) : null,
                          onPressed: () => setState(() => _invite = !_invite),
                        ),
                      RoundIconButton(
                        icon: TrairIcons.settings,
                        tooltip: 'Community-Einstellungen',
                        size: 34,
                        color: _settings ? const Color(0x336E4FF6) : null,
                        onPressed: () => setState(() => _settings = !_settings),
                      ),
                    ],
                  ),
                ),
                AnimatedSize(
                  duration: const Duration(milliseconds: 220),
                  child: _invite ? _inviteBox(c) : const SizedBox(width: double.infinity),
                ),
                const SizedBox(height: 14),
                Expanded(
                  child: ListView(
                    children: [
                      for (final (cat, chans) in info.categories) ...[
                        Padding(
                          padding: const EdgeInsets.fromLTRB(12, 10, 12, 4),
                          child: Text(cat, style: TrairText.small),
                        ),
                        for (final ch in chans) _channelRow(state, info, ch),
                      ],
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
        ProfileCard(subtitle: _myRole(info, c.me)),
      ],
    );
  }

  Widget _inviteBox(Communities c) {
    final code = c.newInvite;
    return Padding(
      padding: const EdgeInsets.only(top: 10),
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: BoxDecoration(
          color: const Color(0x176E4FF6),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: const Color(0x38A18DFF)),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (code != null) ...[
              const Text('Einladungscode', style: TrairText.small),
              const SizedBox(height: 6),
              Row(
                children: [
                  Expanded(
                    child: SelectableText(code, style: TrairText.mono.copyWith(fontSize: 13.5)),
                  ),
                  RoundIconButton(
                    icon: TrairIcons.copy,
                    tooltip: 'Kopieren (verschwindet nach 60 Sekunden)',
                    size: 32,
                    onPressed: () => copySensitive(code),
                  ),
                ],
              ),
              const SizedBox(height: 8),
            ] else
              const Padding(
                padding: EdgeInsets.only(bottom: 8),
                child: Text(
                  'Wer den Code hat, kommt rein (außer Gebannte).',
                  style: TrairText.small,
                ),
              ),
            Row(
              children: [
                Expanded(
                  child: PillButton(
                    label: '24 Std. · 1×',
                    height: 34,
                    kind: PillKind.ghost,
                    onPressed: () => c.createInvite(24 * 60, 1),
                  ),
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: PillButton(
                    label: '7 Tage · offen',
                    height: 34,
                    kind: PillKind.ghost,
                    onPressed: () => c.createInvite(7 * 24 * 60, null),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _channelRow(AppState state, CommunityInfo info, CChannel ch) {
    final c = state.communities;
    final active = ch.id == c.currentChannel;
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Material(
        color: active ? TrairColors.cardActive : Colors.transparent,
        borderRadius: BorderRadius.circular(14),
        child: InkWell(
          key: ValueKey('channel-${ch.name}'),
          borderRadius: BorderRadius.circular(14),
          onTap: () {
            setState(() {
              _info = false;
              _settings = false;
              _draft.clear();
            });
            c.openChannel(info.id, ch.id);
          },
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            child: Row(
              children: [
                const Icon(TrairIcons.hash, size: 16, color: TrairColors.amber, semanticLabel: 'offen'),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    ch.name,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 14.5,
                      fontWeight: active ? FontWeight.w600 : FontWeight.w400,
                      color: active ? TrairColors.text : const Color(0xFFB4B2C2),
                    ),
                  ),
                ),
                if (ch.limited)
                  const _Tag('Team')
                else if (ch.readOnly)
                  const Tooltip(
                    message: 'Hier schreibt nur die Moderation',
                    child: Icon(TrairIcons.eye, size: 14, color: TrairColors.textMuted),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  // ------------------------------------------------------------ channel

  Widget _channel(AppState state, CommunityInfo info, CChannel channel) {
    final c = state.communities;
    final messages = c.messagesOf(info.id, channel.id);

    Future<void> send() async {
      final text = _draft.text.trim();
      if (text.isEmpty) return;
      if (await c.post(text)) _draft.clear();
    }

    final mustAccept = !info.rulesOk && info.rules.isNotEmpty;
    return GlassPanel(
      child: Stack(
        children: [
          Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Container(
                decoration: const BoxDecoration(
                  border: Border(bottom: BorderSide(color: TrairColors.borderSoft)),
                ),
                padding: const EdgeInsets.fromLTRB(22, 12, 16, 10),
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
                          if (channel.topic.isNotEmpty) Text(channel.topic, style: TrairText.small),
                        ],
                      ),
                    ),
                    SecurityCapsule(
                      text: 'Offen · für Moderation lesbar',
                      tone: CapsuleTone.warn,
                      open: _info,
                      onTap: () => setState(() => _info = !_info),
                    ),
                    Expanded(
                      child: Row(
                        mainAxisAlignment: MainAxisAlignment.end,
                        children: [
                          RoundIconButton(
                            icon: TrairIcons.scrollText,
                            tooltip: 'Regeln',
                            filled: false,
                            onPressed: () => showRules(context, state, info),
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
                            constraints: const BoxConstraints(maxWidth: 460),
                            child: const GlassPanel(
                              lite: true,
                              radius: 22,
                              color: TrairColors.glassPopup,
                              padding: EdgeInsets.all(16),
                              child: Text(
                                'Dieser Kanal ist offen: Nachrichten liegen lesbar auf dem Server, damit Moderation '
                                'möglich ist. Schreib hier nichts Privates. Jede Nachricht ist vom Gerät des '
                                'Absenders unterschrieben: Der Server kann sie lesen und löschen, aber nicht fälschen.',
                                style: TextStyle(fontSize: 13.5, height: 1.5, color: TrairColors.textSoft),
                              ),
                            ),
                          ),
                        ),
                      )
                    : const SizedBox(width: double.infinity),
              ),
              Expanded(
                child: messages.isEmpty
                    ? const Center(child: Text('Noch keine Nachrichten.', style: TrairText.muted))
                    : ListView(
                        padding: const EdgeInsets.fromLTRB(26, 8, 26, 130),
                        children: [for (final m in messages) _message(state, info, m)],
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
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      if (c.message.isNotEmpty)
                        Padding(
                          padding: const EdgeInsets.only(bottom: 8, left: 12),
                          child: Text(c.message, style: const TextStyle(fontSize: 13, color: TrairColors.amber)),
                        ),
                      if (mustAccept)
                        _bar(
                          'Bevor du schreibst: Regeln lesen und akzeptieren.',
                          PillButton(
                            label: 'Regeln lesen',
                            height: 38,
                            onPressed: () => showRules(context, state, info),
                          ),
                        )
                      else if (!channel.canWrite)
                        _bar('In diesem Kanal schreibt nur die Moderation.', null)
                      else
                        _composer(channel, send),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _bar(String text, Widget? action) => GlassPanel(
    lite: true,
    radius: 999,
    padding: const EdgeInsets.fromLTRB(20, 8, 8, 8),
    child: Row(
      children: [
        Expanded(child: Text(text, style: TrairText.muted)),
        ?action,
        if (action == null) const SizedBox(height: 38),
      ],
    ),
  );

  Widget _composer(CChannel channel, Future<void> Function() send) => GlassPanel(
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
  );

  Color _colorOf(CommunityInfo info, String user) {
    final m = info.member(user);
    final role = m == null ? null : info.topRole(m);
    return role?.color ?? TrairColors.accentLight;
  }

  String _nameOf(AppState state, String user) {
    if (user != state.communities.me) return state.displayName(user);
    return state.userName.trim().isEmpty ? 'Du' : state.userName.trim();
  }

  Widget _message(AppState state, CommunityInfo info, OpenMessage m) {
    final c = state.communities;
    final color = _colorOf(info, m.user);
    final name = _nameOf(state, m.user);
    final own = m.user == c.me;
    final canDelete = own || (info.can(Perm.deleteMessages) && info.above(m.user));
    final bad = m.signed == Signed.bad;
    return Padding(
      key: ValueKey('open-${m.id}'),
      padding: const EdgeInsets.only(bottom: 18),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            customBorder: const CircleBorder(),
            onTap: () => _memberMenu(state, info, m.user),
            child: Avatar(initials: initialsOf(name), color: color, size: 38),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Wrap(
                  spacing: 8,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    InkWell(
                      borderRadius: BorderRadius.circular(6),
                      onTap: () => _memberMenu(state, info, m.user),
                      child: Text(
                        name,
                        style: TextStyle(
                          fontSize: 14.5,
                          fontWeight: FontWeight.w600,
                          color: color,
                          shadows: glowShadows(color, strength: 20),
                        ),
                      ),
                    ),
                    SignedMark(m.signed),
                    Text(whenText(m.sent), style: const TextStyle(fontSize: 12, color: TrairColors.placeholder)),
                  ],
                ),
                const SizedBox(height: 3),
                Text(
                  m.text,
                  style: TrairText.body.copyWith(
                    color: bad ? TrairColors.textMuted : const Color(0xFFE3E1EC),
                    decoration: bad ? TextDecoration.lineThrough : null,
                  ),
                ),
                if (m.signed == Signed.changed && !own)
                  Padding(
                    padding: const EdgeInsets.only(top: 6),
                    child: Wrap(
                      spacing: 8,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        const Text(
                          'Anderer Schlüssel als beim ersten Mal. Frag die Person auf einem anderen Weg.',
                          style: TextStyle(fontSize: 12.5, color: TrairColors.amber),
                        ),
                        PillButton(
                          label: 'Neuen Schlüssel annehmen',
                          height: 30,
                          kind: PillKind.ghost,
                          onPressed: () async {
                            if (await confirmAction(
                              context,
                              'Neuen Schlüssel annehmen?',
                              'Nur wenn $name dir auf einem anderen Weg bestätigt hat, dass das ein neues Konto ist.',
                              'Annehmen',
                            )) {
                              await c.acceptKey(m.user);
                            }
                          },
                        ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
          PopupMenuButton<String>(
            tooltip: 'Mehr',
            color: TrairColors.glassPopup,
            icon: const Icon(TrairIcons.ellipsis, size: 16, color: TrairColors.textMuted),
            onSelected: (v) async {
              if (v == 'delete') {
                if (await confirmAction(context, 'Nachricht löschen?', 'Sie verschwindet für alle.', 'Löschen')) {
                  await c.deleteMessage(m.id);
                }
              } else if (v == 'report') {
                final reason = await askText(
                  context,
                  title: 'Nachricht melden',
                  text: 'Die Moderation von „${info.name}“ sieht die Nachricht und deinen Grund.',
                  hint: 'Warum?',
                  yes: 'Melden',
                  kind: PillKind.danger,
                );
                if (reason != null) await c.reportMessage(m.id, reason);
              } else if (v == 'copy') {
                await copySensitive(m.text);
              }
            },
            itemBuilder: (_) => [
              const PopupMenuItem(value: 'copy', child: Text('Text kopieren')),
              if (!own) const PopupMenuItem(value: 'report', child: Text('Melden')),
              if (canDelete) const PopupMenuItem(value: 'delete', child: Text('Löschen')),
            ],
          ),
        ],
      ),
    );
  }

  // ------------------------------------------------------------ members

  Widget _members(AppState state, CommunityInfo info) {
    final groups = <(CRole?, List<CMember>)>[];
    for (final role in info.roles.where((r) => !r.everyone)) {
      final ms = [
        for (final m in info.members)
          if (info.topRole(m)?.id == role.id) m,
      ];
      if (ms.isNotEmpty) groups.add((role, ms));
    }
    final rest = [
      for (final m in info.members)
        if (info.topRole(m) == null) m,
    ];
    if (rest.isNotEmpty) groups.add((null, rest));
    return GlassPanel(
      color: TrairColors.sidebar,
      padding: const EdgeInsets.fromLTRB(8, 14, 8, 14),
      child: ListView(
        children: [
          for (final (role, ms) in groups) ...[
            Padding(
              padding: const EdgeInsets.fromLTRB(10, 8, 10, 4),
              child: Text('${role?.name ?? 'Mitglieder'} · ${ms.length}', style: TrairText.small),
            ),
            for (final m in ms) _memberRow(state, info, m),
          ],
        ],
      ),
    );
  }

  Widget _memberRow(AppState state, CommunityInfo info, CMember m) {
    final color = _colorOf(info, m.user);
    final name = _nameOf(state, m.user);
    return InkWell(
      key: ValueKey('member-${m.user}'),
      borderRadius: BorderRadius.circular(14),
      onTap: () => _memberMenu(state, info, m.user),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        child: Row(
          children: [
            Avatar(initials: initialsOf(name), color: color, size: 32),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                name,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500, color: color),
              ),
            ),
            if (m.owner)
              const Tooltip(
                message: 'Gründer',
                child: Icon(TrairIcons.crown, size: 15, color: TrairColors.amber),
              ),
          ],
        ),
      ),
    );
  }

  /// What can be done with a member: profile, roles, kick, ban, report.
  Future<void> _memberMenu(AppState state, CommunityInfo info, String user) async {
    final c = state.communities;
    final m = info.member(user);
    final self = user == c.me;
    final name = _nameOf(state, user);
    if (self) {
      return showProfileCard(context, ProfileView.self(state));
    }
    final above = info.above(user);
    final choice = await showDialog<String>(
      context: context,
      barrierColor: const Color(0x9905050A),
      builder: (ctx) => DialogShell(
        title: name,
        width: 380,
        children: [
          Text(
            [
              if (name != user) '@$user',
              if (m == null) 'nicht mehr in der Community',
              if (m != null && m.owner) 'Gründer',
              if (m != null) ...[for (final id in m.roles) ?info.role(id)?.name],
            ].join(' · '),
            style: TrairText.small,
          ),
          const SizedBox(height: 14),
          for (final (value, label, icon, show) in [
            ('profile', 'Profil ansehen', TrairIcons.user, true),
            ('roles', 'Rollen geben oder nehmen', TrairIcons.shield, m != null && info.can(Perm.manageRoles) && above),
            ('kick', 'Aus der Community entfernen', TrairIcons.userMinus, m != null && info.can(Perm.kick) && above),
            ('ban', 'Bannen', TrairIcons.ban, info.can(Perm.ban) && above),
            ('transfer', 'Community übergeben', TrairIcons.crown, m != null && info.iAmOwner),
            ('report', 'Bei der Moderation melden', TrairIcons.flag, true),
            ('team', 'Ans Trair-Team melden', TrairIcons.shieldAlert, true),
          ])
            if (show)
              Padding(
                padding: const EdgeInsets.only(bottom: 6),
                child: PillButton(
                  key: ValueKey('member-action-$value'),
                  label: label,
                  icon: icon,
                  height: 38,
                  kind: value == 'kick' || value == 'ban' ? PillKind.danger : PillKind.ghost,
                  onPressed: () => Navigator.pop(ctx, value),
                ),
              ),
        ],
      ),
    );
    if (!mounted || choice == null) return;
    switch (choice) {
      case 'profile':
        await showProfileCard(
          context,
          ProfileView.of(Person(name: name, color: _colorOf(info, user), trust: 0), state),
        );
      case 'roles':
        await _roles(state, info, user);
      case 'kick':
        final reason = await askText(
          context,
          title: '$name entfernen?',
          text: '$name kann mit einer neuen Einladung wiederkommen.',
          hint: 'Grund (steht im Protokoll)',
          yes: 'Entfernen',
          needed: false,
          kind: PillKind.danger,
        );
        if (reason != null) await c.kick(user, reason);
      case 'ban':
        final reason = await askText(
          context,
          title: '$name bannen?',
          text: '$name fliegt raus und kommt auch mit Einladung nicht wieder, bis jemand entbannt.',
          hint: 'Grund (steht im Protokoll)',
          yes: 'Bannen',
          needed: false,
          kind: PillKind.danger,
        );
        if (reason != null) await c.ban(user, reason);
      case 'transfer':
        if (await confirmAction(
          context,
          'An $name übergeben?',
          '$name wird Gründer und hat alle Rechte. Du bleibst Mitglied, aber ohne die Rechte des Gründers.',
          'Übergeben',
        )) {
          await c.transfer(user);
        }
      case 'report':
        final reason = await askText(
          context,
          title: '$name melden',
          text: 'Die Moderation von „${info.name}“ sieht die Meldung.',
          hint: 'Was ist passiert?',
          yes: 'Melden',
          kind: PillKind.danger,
        );
        if (reason != null) await c.reportMember(user, reason);
      case 'team':
        if (mounted) await showTeamReport(context, state, user: user, name: name);
    }
  }

  Future<void> _roles(AppState state, CommunityInfo info, String user) async {
    final m = info.member(user);
    if (m == null) return;
    final chosen = {...m.roles};
    final roles = info.assignable;
    final ok = await showDialog<bool>(
      context: context,
      barrierColor: const Color(0x9905050A),
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, set) => DialogShell(
          title: 'Rollen von ${_nameOf(state, user)}',
          children: [
            if (roles.isEmpty)
              const Text('Du kannst keine Rollen vergeben.', style: TrairText.muted)
            else
              for (final r in roles)
                PillSwitch(
                  key: ValueKey('role-switch-${r.name}'),
                  value: chosen.contains(r.id),
                  label: r.name,
                  onChanged: (v) => set(() => v ? chosen.add(r.id) : chosen.remove(r.id)),
                ),
            const SizedBox(height: 14),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                PillButton(label: 'Abbrechen', kind: PillKind.ghost, height: 38, onPressed: () => Navigator.pop(ctx)),
                const SizedBox(width: 8),
                PillButton(label: 'Speichern', height: 38, onPressed: () => Navigator.pop(ctx, true)),
              ],
            ),
          ],
        ),
      ),
    );
    if (ok == true) {
      // Roles this account may not touch stay as they were.
      final keep = [
        for (final id in m.roles)
          if (!roles.any((r) => r.id == id)) id,
      ];
      await state.communities.setRoles(user, [...keep, ...chosen.where((id) => roles.any((r) => r.id == id))]);
    }
  }
}

class _Tag extends StatelessWidget {
  const _Tag(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
    decoration: BoxDecoration(color: const Color(0x0FFFFFFF), borderRadius: BorderRadius.circular(999)),
    child: Text(text, style: const TextStyle(fontSize: 11, color: TrairColors.textSoft)),
  );
}

/// What the signature check found, as a small sign next to the name.
class SignedMark extends StatelessWidget {
  const SignedMark(this.signed, {super.key});

  final Signed signed;

  @override
  Widget build(BuildContext context) {
    final (IconData? icon, Color color, String text) = switch (signed) {
      Signed.pending => (null, TrairColors.textMuted, ''),
      Signed.own => (TrairIcons.check, TrairColors.mint, 'Von deinem Gerät unterschrieben'),
      Signed.verified => (TrairIcons.badgeCheck, TrairColors.mint, 'Unterschrieben · Schlüssel persönlich geprüft'),
      Signed.contact => (TrairIcons.check, TrairColors.mint, 'Unterschrieben · Schlüssel aus deinen Kontakten'),
      Signed.known => (
        TrairIcons.check,
        TrairColors.textMuted,
        'Unterschrieben · derselbe Schlüssel wie beim ersten Mal (nicht persönlich geprüft)',
      ),
      Signed.fresh => (
        TrairIcons.check,
        TrairColors.textMuted,
        'Unterschrieben · zum ersten Mal gesehen, Schlüssel gemerkt (nicht persönlich geprüft)',
      ),
      Signed.changed => (TrairIcons.shieldAlert, TrairColors.amber, 'Unterschrieben, aber mit einem anderen Schlüssel'),
      Signed.bad => (TrairIcons.triangleAlert, TrairColors.rose, 'Unterschrift ungültig: nicht von dieser Person'),
    };
    if (icon == null) return const SizedBox.shrink();
    return Tooltip(
      message: text,
      child: Icon(icon, size: 14, color: color, semanticLabel: text),
    );
  }
}
