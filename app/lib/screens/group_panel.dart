// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';
import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/qr.dart';
import '../widgets/trair_icons.dart';

/// Initials of a name, for an avatar.
String initialsOf(String name) {
  final parts = name.trim().split(RegExp(r'\s+')).where((p) => p.isNotEmpty).toList();
  if (parts.isEmpty) return '?';
  return parts.take(2).map((p) => String.fromCharCode(p.runes.first).toUpperCase()).join();
}

String _two(int n) => n.toString().padLeft(2, '0');

/// "heute um 18:30" or "24.12. um 18:30".
String whenText(int seconds) {
  final t = DateTime.fromMillisecondsSinceEpoch(seconds * 1000);
  final now = DateTime.now();
  final day = t.day == now.day && t.month == now.month && t.year == now.year ? 'heute' : '${_two(t.day)}.${_two(t.month)}.';
  return '$day um ${_two(t.hour)}:${_two(t.minute)}';
}

/// A note of the core in a group's history, as a sentence.
String groupNoteText(String text, String Function(String user) name) {
  Map<String, Object?> j;
  try {
    j = (jsonDecode(text) as Map).cast<String, Object?>();
  } on Object {
    return text;
  }
  final users = [for (final u in (j['users'] as List? ?? const []).cast<String>()) name(u)];
  final who = users.join(', ');
  final by = name(j['by'] as String? ?? '');
  final many = users.length > 1;
  final you = users.length == 1 && users.single == 'Du';
  switch (j['event']) {
    case 'created':
      return by == 'Du' ? 'Du hast die Gruppe erstellt' : '$by hat die Gruppe erstellt';
    case 'joined':
      return by == 'Du' ? 'Du bist in der Gruppe' : '$by hat dich hinzugefügt';
    case 'added':
      return '$by hat $who hinzugefügt';
    case 'removed':
      if (you) return 'Du wurdest von $by entfernt';
      return '$by hat $who entfernt';
    case 'left':
      if (you) return 'Du hast die Gruppe verlassen';
      return many ? '$who haben die Gruppe verlassen' : '$who hat die Gruppe verlassen';
    case 'renamed':
      return '$by hat die Gruppe in „${j['name']}“ umbenannt';
    case 'owner':
      return you ? 'Dir gehört die Gruppe jetzt' : '$who gehört die Gruppe jetzt';
    case 'admin':
      return '$by hat $who zum Admin gemacht';
    case 'not_admin':
      return many ? '$who sind keine Admins mehr' : (you ? 'Du bist kein Admin mehr' : '$who ist kein Admin mehr');
    case 'ends':
      final ends = (j['ends'] as num?)?.toInt();
      return ends == null ? '$by: Die Gruppe bleibt' : '$by: Die Gruppe verschwindet ${whenText(ends)}';
  }
  return 'Etwas in der Gruppe hat sich geändert';
}

/// Ask before something that cannot be taken back.
Future<bool> confirmAction(BuildContext context, String title, String text, String yes) async {
  final ok = await showDialog<bool>(
    context: context,
    barrierColor: const Color(0x9905050A),
    builder: (ctx) => Center(
      child: Material(
        type: MaterialType.transparency,
        child: Container(
          width: 380,
          padding: const EdgeInsets.all(22),
          decoration: BoxDecoration(
            color: TrairColors.glassPopup,
            borderRadius: BorderRadius.circular(20),
            border: Border.all(color: TrairColors.border),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(title, style: TrairText.heading),
              const SizedBox(height: 8),
              Text(text, style: TrairText.muted),
              const SizedBox(height: 18),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  PillButton(label: 'Abbrechen', kind: PillKind.ghost, height: 38, onPressed: () => Navigator.pop(ctx, false)),
                  const SizedBox(width: 8),
                  PillButton(label: yes, kind: PillKind.danger, height: 38, onPressed: () => Navigator.pop(ctx, true)),
                ],
              ),
            ],
          ),
        ),
      ),
    ),
  );
  return ok == true;
}

/// The right column of a group chat: members with their roles, inviting,
/// and the group's settings. What a button does is checked again by the
/// core (and by every other member's Trair); the panel only offers what
/// this account's role allows.
class GroupPanel extends StatefulWidget {
  const GroupPanel({super.key, required this.chat, required this.onClose, required this.onGone});

  final RealChat chat;
  final VoidCallback onClose;

  /// The group is gone here (left, closed).
  final VoidCallback onGone;

  @override
  State<GroupPanel> createState() => _GroupPanelState();
}

class _GroupPanelState extends State<GroupPanel> {
  final _name = TextEditingController();
  bool _renaming = false;
  int _keyMinutes = 1440;
  bool _guests = false;
  final Set<String> _adding = {};

  static const _keyChoices = {60: '1 Std.', 1440: '1 Tag', 10080: '7 Tage'};

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) unawaited(AppScope.of(context).loadGroup(widget.chat.id));
    });
  }

  @override
  void didUpdateWidget(GroupPanel old) {
    super.didUpdateWidget(old);
    if (old.chat.id != widget.chat.id) {
      _renaming = false;
      _adding.clear();
      unawaited(AppScope.of(context).loadGroup(widget.chat.id));
    }
  }

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  String _nameOf(AppState state, String user) => user == state.ownUser ? 'Du' : state.displayName(user);

  Widget _section(String text) => Padding(
    padding: const EdgeInsets.only(top: 18, bottom: 8),
    child: Text(
      text.toUpperCase(),
      style: const TextStyle(fontSize: 11, fontWeight: FontWeight.w600, letterSpacing: 0.6, color: TrairColors.textMuted),
    ),
  );

  Widget _roleIcon(String role) => switch (role) {
    'owner' => const Tooltip(
      message: 'Besitzer',
      child: Icon(TrairIcons.crown, size: 15, color: TrairColors.amber),
    ),
    'admin' => const Tooltip(
      message: 'Admin',
      child: Icon(TrairIcons.shield, size: 15, color: TrairColors.accentLight),
    ),
    'guest' => const Tooltip(
      message: 'Gast',
      child: Icon(TrairIcons.clock, size: 15, color: TrairColors.textMuted),
    ),
    _ => const SizedBox.shrink(),
  };

  Widget _member(AppState state, GroupInfo g, GroupMember m) {
    final me = m.user == state.ownUser;
    final p = state.peerProfiles[m.user];
    final canKick = !me && g.isAdmin && m.role != 'owner' && (g.isOwner || !m.isAdmin);
    final canRole = !me && g.isOwner && m.role != 'owner';
    final sub = [
      switch (m.role) {
        'owner' => 'Besitzer',
        'admin' => 'Admin',
        'guest' => m.guestUntil == null ? 'Gast' : 'Gast bis ${whenText(m.guestUntil!)}',
        _ => 'Mitglied',
      },
      if (m.devices > 1) '${m.devices} Geräte',
      if (m.leaving) 'geht gerade',
    ].join(' · ');
    return Padding(
      padding: const EdgeInsets.only(bottom: 6),
      child: Row(
        children: [
          Avatar(
            initials: initialsOf(_nameOf(state, m.user)),
            color: p?.glow ?? TrairColors.accentLight,
            size: 34,
            picture: p?.picture,
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Flexible(
                      child: Text(
                        _nameOf(state, m.user),
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 13.5,
                          fontWeight: FontWeight.w600,
                          color: p?.glow ?? TrairColors.text,
                          shadows: p?.glow == null ? null : glowShadows(p!.glow!, strength: 16),
                        ),
                      ),
                    ),
                    const SizedBox(width: 6),
                    _roleIcon(m.role),
                  ],
                ),
                Text(sub, style: TrairText.small, overflow: TextOverflow.ellipsis),
                if (m.keyChanged)
                  const Text(
                    'Schlüssel geändert: erst prüfen',
                    style: TextStyle(fontSize: 12, color: TrairColors.amber),
                  ),
              ],
            ),
          ),
          if (canKick || canRole || m.keyChanged)
            PopupMenuButton<String>(
              tooltip: 'Mitglied verwalten',
              color: TrairColors.glassPopup,
              icon: const Icon(TrairIcons.ellipsis, size: 16, color: TrairColors.textSoft),
              onSelected: (what) => _memberAction(state, m, what),
              itemBuilder: (_) => [
                if (m.keyChanged) const PopupMenuItem(value: 'key', child: Text('Neuen Schlüssel annehmen')),
                if (canRole && m.role != 'admin') const PopupMenuItem(value: 'admin', child: Text('Zum Admin machen')),
                if (canRole && m.role == 'admin') const PopupMenuItem(value: 'unadmin', child: Text('Admin entziehen')),
                if (canRole) const PopupMenuItem(value: 'owner', child: Text('Gruppe übergeben')),
                if (canKick) const PopupMenuItem(value: 'kick', child: Text('Aus der Gruppe entfernen')),
              ],
            ),
        ],
      ),
    );
  }

  Future<void> _memberAction(AppState state, GroupMember m, String what) async {
    final chat = widget.chat.id;
    final name = state.displayName(m.user);
    switch (what) {
      case 'key':
        await state.acceptNewKey(chat, m.user);
      case 'admin':
        await state.setAdmin(chat, m.user, true);
      case 'unadmin':
        await state.setAdmin(chat, m.user, false);
      case 'owner':
        if (await confirmAction(
          context,
          'Gruppe übergeben?',
          '$name gehört die Gruppe dann. Du bleibst Admin.',
          'Übergeben',
        )) {
          await state.handOver(chat, m.user);
        }
      case 'kick':
        if (await confirmAction(
          context,
          '$name entfernen?',
          'Neue Nachrichten erreichen $name dann nicht mehr. Was schon da ist, bleibt bei $name.',
          'Entfernen',
        )) {
          await state.kick(chat, m.user);
        }
    }
  }

  List<Widget> _invite(AppState state, GroupInfo g) {
    final chat = widget.chat.id;
    final key = state.groupKeys[chat];
    final inGroup = {for (final m in g.members) m.user};
    final addable = state.contacts.where((c) => !inGroup.contains(c.user)).toList();
    return [
      _section('Einladen'),
      if (addable.isNotEmpty) ...[
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [
            for (final c in addable)
              IntrinsicWidth(
                child: PillButton(
                  label: c.name,
                  height: 32,
                  kind: _adding.contains(c.user) ? PillKind.primary : PillKind.ghost,
                  onPressed: () => setState(() => _adding.contains(c.user) ? _adding.remove(c.user) : _adding.add(c.user)),
                ),
              ),
          ],
        ),
        const SizedBox(height: 8),
        PillButton(
          label: _adding.isEmpty ? 'Kontakte auswählen' : 'Hinzufügen (${_adding.length})',
          icon: TrairIcons.userPlus,
          height: 38,
          expand: true,
          onPressed: _adding.isEmpty || !state.chatsOnline
              ? null
              : () async {
                  final users = _adding.toList();
                  if (await state.addToGroup(chat, users) && mounted) setState(_adding.clear);
                },
        ),
        const SizedBox(height: 12),
      ],
      const Text(
        'Ein Einladungsschlüssel gilt für viele, bis zur gewählten Zeit. Dein Trair prüft ihn und fügt die Person dann hinzu.',
        style: TrairText.small,
      ),
      const SizedBox(height: 8),
      Wrap(
        spacing: 6,
        runSpacing: 6,
        children: [
          for (final e in _keyChoices.entries)
            IntrinsicWidth(
              child: PillButton(
                label: e.value,
                height: 32,
                kind: e.key == _keyMinutes ? PillKind.primary : PillKind.ghost,
                onPressed: () => setState(() => _keyMinutes = e.key),
              ),
            ),
        ],
      ),
      const SizedBox(height: 8),
      Row(
        children: [
          PillSwitch(value: _guests, label: 'Als Gäste einladen', onChanged: (v) => setState(() => _guests = v)),
          const SizedBox(width: 10),
          const Expanded(child: Text('Als Gäste: gehen am Ende wieder', style: TrairText.small)),
        ],
      ),
      const SizedBox(height: 8),
      PillButton(
        label: 'Einladungsschlüssel erstellen',
        icon: TrairIcons.link,
        height: 38,
        expand: true,
        onPressed: () => state.createGroupKey(chat, _keyMinutes, guest: _guests),
      ),
      if (key != null) ...[
        const SizedBox(height: 10),
        Container(
          padding: const EdgeInsets.all(12),
          decoration: BoxDecoration(
            color: const Color(0x176E4FF6),
            borderRadius: BorderRadius.circular(12),
            border: Border.all(color: const Color(0x38A18DFF)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                key.guest
                    ? 'Gäste · gilt bis ${whenText(key.ends.millisecondsSinceEpoch ~/ 1000)}, dann gehen sie'
                    : 'Gilt bis ${whenText(key.ends.millisecondsSinceEpoch ~/ 1000)}',
                style: const TextStyle(fontSize: 12.5, fontWeight: FontWeight.w600, color: TrairColors.accentPale),
              ),
              const SizedBox(height: 6),
              SelectableText(
                key.code,
                style: const TextStyle(fontFamily: 'GeistMono', fontSize: 11, color: TrairColors.text),
              ),
              const SizedBox(height: 8),
              Row(
                children: [
                  QrBox(text: key.code, size: 96),
                  const Spacer(),
                  RoundIconButton(
                    icon: TrairIcons.copy,
                    tooltip: 'Einladungsschlüssel kopieren',
                    filled: false,
                    onPressed: () => Clipboard.setData(ClipboardData(text: key.code)),
                  ),
                  RoundIconButton(
                    icon: TrairIcons.x,
                    tooltip: 'Einladungsschlüssel ausblenden',
                    filled: false,
                    onPressed: () => state.closeGroupKey(chat),
                  ),
                ],
              ),
            ],
          ),
        ),
      ],
      const SizedBox(height: 6),
      Align(
        alignment: Alignment.centerLeft,
        child: TextButton(
          onPressed: () => state.revokeGroupKeys(chat),
          child: const Text('Alle Schlüssel zurückziehen', style: TextStyle(fontSize: 12.5, color: TrairColors.rose)),
        ),
      ),
    ];
  }

  List<Widget> _settings(AppState state, GroupInfo g) {
    final chat = widget.chat.id;
    final others = g.members.length > 1;
    return [
      _section('Einstellungen'),
      if (g.isAdmin) ...[
        const Text('Verschwindet für alle nach …', style: TrairText.small),
        const SizedBox(height: 6),
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [
            for (final e in const {60: '1 Std.', 1440: '1 Tag', 10080: '7 Tagen'}.entries)
              IntrinsicWidth(
                child: PillButton(
                  label: e.value,
                  height: 32,
                  kind: PillKind.ghost,
                  onPressed: () => state.setGroupEnd(chat, e.key),
                ),
              ),
            if (g.ends != null)
              IntrinsicWidth(
                child: PillButton(
                  label: 'Bleibt',
                  height: 32,
                  kind: PillKind.ghost,
                  onPressed: () => state.setGroupEnd(chat, null),
                ),
              ),
          ],
        ),
        const SizedBox(height: 12),
      ],
      PillButton(
        label: g.isOwner && others ? 'Übergeben und verlassen' : 'Gruppe verlassen',
        icon: TrairIcons.logOut,
        kind: PillKind.danger,
        height: 38,
        expand: true,
        onPressed: () async {
          if (!await confirmAction(
            context,
            'Gruppe verlassen?',
            g.isOwner && others
                ? 'Die Gruppe geht an einen Admin (oder ein Mitglied). Danach bist du draußen; der Verlauf bleibt bei dir.'
                : 'Du bist sofort draußen; der Verlauf bleibt bei dir.',
            'Verlassen',
          )) {
            return;
          }
          if (await state.leaveGroup(chat)) widget.onClose();
        },
      ),
      if (g.isOwner) ...[
        const SizedBox(height: 8),
        PillButton(
          label: 'Für alle auflösen',
          icon: TrairIcons.trash,
          kind: PillKind.danger,
          height: 38,
          expand: true,
          onPressed: () async {
            if (!await confirmAction(
              context,
              'Gruppe für alle auflösen?',
              'Jedes Trair in der Gruppe löscht sie dann, mit dem ganzen Verlauf. Bildschirmfotos kann niemand verhindern.',
              'Auflösen',
            )) {
              return;
            }
            if (await state.closeGroup(chat)) widget.onGone();
          },
        ),
      ],
    ];
  }

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final g = state.groupInfos[widget.chat.id];
    return Container(
      width: 300,
      decoration: const BoxDecoration(
        color: TrairColors.sidebar,
        border: Border(left: BorderSide(color: TrairColors.borderSoft)),
      ),
      child: g == null
          ? const Center(child: Text('Lade die Gruppe …', style: TrairText.muted))
          : ListView(
              padding: const EdgeInsets.fromLTRB(18, 14, 18, 22),
              children: [
                Row(
                  children: [
                    const Expanded(child: Text('Gruppe', style: TrairText.heading)),
                    RoundIconButton(
                      icon: TrairIcons.x,
                      tooltip: 'Gruppe schließen',
                      filled: false,
                      onPressed: widget.onClose,
                    ),
                  ],
                ),
                const SizedBox(height: 10),
                if (_renaming)
                  Row(
                    children: [
                      Expanded(
                        child: PillField(controller: _name, hint: 'Gruppenname', height: 40, semanticLabel: 'Gruppenname'),
                      ),
                      const SizedBox(width: 6),
                      RoundIconButton(
                        icon: TrairIcons.check,
                        tooltip: 'Namen speichern',
                        onPressed: () async {
                          if (await state.renameGroup(widget.chat.id, _name.text) && mounted) {
                            setState(() => _renaming = false);
                          }
                        },
                      ),
                    ],
                  )
                else
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          g.name,
                          style: const TextStyle(fontSize: 18, fontWeight: FontWeight.w700, color: TrairColors.text),
                        ),
                      ),
                      if (g.isAdmin)
                        RoundIconButton(
                          icon: TrairIcons.pencil,
                          tooltip: 'Gruppe umbenennen',
                          filled: false,
                          size: 32,
                          onPressed: () => setState(() {
                            _name.text = g.name;
                            _renaming = true;
                          }),
                        ),
                    ],
                  ),
                const SizedBox(height: 4),
                Text(
                  [
                    '${g.members.length} ${g.members.length == 1 ? 'Mitglied' : 'Mitglieder'}',
                    switch (g.me) {
                      'owner' => 'dir gehört sie',
                      'admin' => 'du bist Admin',
                      'guest' => 'du bist Gast',
                      _ => 'du bist Mitglied',
                    },
                  ].join(' · '),
                  style: TrairText.small,
                ),
                if (g.myEnd != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Text(
                      g.ends == g.myEnd ? 'Verschwindet ${whenText(g.myEnd!)}' : 'Für dich vorbei ${whenText(g.myEnd!)}',
                      style: const TextStyle(fontSize: 12, color: TrairColors.amber),
                    ),
                  ),
                _section('Mitglieder · ${g.members.length}'),
                for (final m in g.members) _member(state, g, m),
                if (g.isAdmin) ..._invite(state, g),
                ..._settings(state, g),
                const SizedBox(height: 16),
                const Text(
                  'Name, Mitglieder, Rollen und Nachrichten sind Ende-zu-Ende verschlüsselt. Der Server weiß nur, '
                  'welche Konten diese Gruppe ändern dürfen.',
                  style: TrairText.small,
                ),
              ],
            ),
    );
  }
}
