// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../community_state.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';
import 'community_dialogs.dart';
import 'group_panel.dart' show confirmAction, whenText;

/// The settings of a community, in tabs. Only tabs this account has a
/// right for are shown; the server checks every change again.
class CommunitySettings extends StatefulWidget {
  const CommunitySettings({super.key, required this.info, required this.onClose});

  final CommunityInfo info;
  final VoidCallback onClose;

  @override
  State<CommunitySettings> createState() => _CommunitySettingsState();
}

class _CommunitySettingsState extends State<CommunitySettings> {
  String _tab = 'overview';
  bool _allReports = false;
  final _name = TextEditingController();
  final _about = TextEditingController();
  final _rules = TextEditingController();
  String _loadedFor = '';

  @override
  void dispose() {
    _name.dispose();
    _about.dispose();
    _rules.dispose();
    super.dispose();
  }

  void _fill(CommunityInfo info) {
    final key = '${info.id}/${info.name}/${info.about}/${info.rulesRev}';
    if (key == _loadedFor) return;
    _loadedFor = key;
    _name.text = info.name;
    _about.text = info.about;
    _rules.text = info.rules;
  }

  List<(String, String, IconData)> _tabs(CommunityInfo info) => [
    ('overview', 'Übersicht & Regeln', TrairIcons.scrollText),
    if (info.can(Perm.manageRoles)) ('roles', 'Rollen', TrairIcons.shield),
    if (info.can(Perm.manageChannels)) ('channels', 'Kanäle', TrairIcons.hash),
    if (info.can(Perm.invite)) ('invites', 'Einladungen', TrairIcons.link),
    if (info.can(Perm.ban)) ('bans', 'Banns', TrairIcons.ban),
    if (info.can(Perm.reports)) ('reports', 'Meldungen', TrairIcons.flag),
    if (info.can(Perm.log)) ('log', 'Protokoll', TrairIcons.fileText),
    ('leave', info.iAmOwner ? 'Löschen' : 'Verlassen', TrairIcons.logOut),
  ];

  Future<void> _open(String tab) async {
    setState(() => _tab = tab);
    final c = AppScope.of(context).communities;
    switch (tab) {
      case 'invites':
        await c.loadInvites();
      case 'bans':
        await c.loadBans();
      case 'reports':
        await c.loadReports(all: _allReports);
      case 'log':
        await c.loadLog();
    }
  }

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final info = widget.info;
    final tabs = _tabs(info);
    if (!tabs.any((t) => t.$1 == _tab)) _tab = 'overview';
    _fill(info);
    return GlassPanel(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            width: 220,
            padding: const EdgeInsets.fromLTRB(12, 16, 12, 16),
            decoration: const BoxDecoration(
              border: Border(right: BorderSide(color: TrairColors.borderSoft)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(8, 0, 8, 12),
                  child: Text(info.name, overflow: TextOverflow.ellipsis, style: TrairText.heading),
                ),
                for (final (id, label, icon) in tabs)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 2),
                    child: Material(
                      color: _tab == id ? TrairColors.cardActive : Colors.transparent,
                      borderRadius: BorderRadius.circular(12),
                      child: InkWell(
                        key: ValueKey('tab-$id'),
                        borderRadius: BorderRadius.circular(12),
                        onTap: () => _open(id),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
                          child: Row(
                            children: [
                              Icon(icon, size: 16, color: id == 'leave' ? TrairColors.rose : TrairColors.textSoft),
                              const SizedBox(width: 10),
                              Expanded(child: Text(label, style: const TextStyle(fontSize: 14))),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                const Spacer(),
                PillButton(
                  label: 'Zurück zum Kanal',
                  icon: TrairIcons.chevronLeft,
                  kind: PillKind.ghost,
                  height: 38,
                  onPressed: widget.onClose,
                ),
              ],
            ),
          ),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.fromLTRB(28, 20, 28, 28),
              children: [
                if (state.communities.message.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: Text(
                      state.communities.message,
                      style: const TextStyle(fontSize: 13, color: TrairColors.amber),
                    ),
                  ),
                ...switch (_tab) {
                  'roles' => _roles(state, info),
                  'channels' => _channels(state, info),
                  'invites' => _invites(state),
                  'bans' => _bans(state),
                  'reports' => _reports(state, info),
                  'log' => _log(state),
                  'leave' => _leave(state, info),
                  _ => _overview(state, info),
                },
              ],
            ),
          ),
        ],
      ),
    );
  }

  String _name2(AppState state, String user) => user == state.communities.me ? 'Du' : state.displayName(user);

  // ------------------------------------------------------------ overview

  List<Widget> _overview(AppState state, CommunityInfo info) {
    final edit = info.can(Perm.manageCommunity);
    if (!edit) {
      return [
        const Text('Beschreibung', style: TrairText.small),
        const SizedBox(height: 6),
        Text(info.about.isEmpty ? '—' : info.about, style: TrairText.body),
        const SizedBox(height: 18),
        const Text('Regeln', style: TrairText.small),
        const SizedBox(height: 6),
        Text(info.rules.isEmpty ? 'Keine Regeln.' : info.rules, style: TrairText.body),
        const SizedBox(height: 18),
        Text(
          info.rulesOk
              ? 'Du hast die aktuellen Regeln akzeptiert.'
              : 'Du hast die aktuellen Regeln noch nicht akzeptiert.',
          style: TrairText.small,
        ),
      ];
    }
    return [
      const Text('Name', style: TrairText.small),
      const SizedBox(height: 6),
      PillField(controller: _name, hint: 'Name der Community', semanticLabel: 'Name der Community'),
      const SizedBox(height: 16),
      const Text('Beschreibung', style: TrairText.small),
      const SizedBox(height: 6),
      NoteField(controller: _about, hint: 'Worum geht es hier?', lines: 2, maxLength: 500),
      const SizedBox(height: 16),
      const Text('Regeln', style: TrairText.small),
      const SizedBox(height: 6),
      NoteField(
        controller: _rules,
        hint: '1. Freundlich bleiben\n2. Kein Spam',
        lines: 6,
        maxLength: 4000,
        fieldKey: const Key('rules-field'),
      ),
      const SizedBox(height: 6),
      const Text(
        'Ändern sich die Regeln, muss jedes Mitglied sie neu akzeptieren, bevor es wieder schreibt.',
        style: TrairText.small,
      ),
      const SizedBox(height: 16),
      Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          PillButton(
            label: 'Speichern',
            height: 40,
            onPressed: () => state.communities.edit(name: _name.text, about: _about.text, rules: _rules.text),
          ),
        ],
      ),
    ];
  }

  // ------------------------------------------------------------ roles

  List<Widget> _roles(AppState state, CommunityInfo info) {
    final c = state.communities;
    final mine = info.iAmOwner ? Perm.all : info.myPerms;
    return [
      Row(
        children: [
          const Expanded(
            child: Text(
              'Höhere Rollen stehen oben. Du änderst nur Rollen unter deiner eigenen und gibst nur Rechte weiter, '
              'die du selbst hast.',
              style: TrairText.small,
            ),
          ),
          const SizedBox(width: 12),
          PillButton(
            label: 'Neue Rolle',
            icon: TrairIcons.plus,
            height: 38,
            onPressed: () async {
              final made = await _roleDialog(context, null, mine);
              if (made != null) await c.createRole(made.$1, made.$2, made.$3);
            },
          ),
        ],
      ),
      const SizedBox(height: 14),
      for (final (i, r) in info.roles.indexed)
        _row(
          key: ValueKey('role-${r.name}'),
          leading: Container(
            width: 14,
            height: 14,
            decoration: BoxDecoration(color: r.color, shape: BoxShape.circle),
          ),
          title: r.name,
          subtitle: r.everyone
              ? 'Rechte, die jedes Mitglied hat · ${_permCount(r.perms)}'
              : '${info.members.where((m) => m.roles.contains(r.id)).length} Mitglieder · ${_permCount(r.perms)}',
          actions: [
            if (!r.everyone && (info.iAmOwner || r.position < info.myPower)) ...[
              if (i > 0 && (info.iAmOwner || info.roles[i - 1].position + 1 < info.myPower))
                RoundIconButton(
                  icon: TrairIcons.arrowUp,
                  tooltip: 'Höher',
                  size: 32,
                  onPressed: () => c.editRole(r, position: info.roles[i - 1].position + 1),
                ),
              RoundIconButton(
                icon: TrairIcons.pencil,
                tooltip: 'Bearbeiten',
                size: 32,
                onPressed: () async {
                  final e = await _roleDialog(context, r, mine);
                  if (e != null) await c.editRole(r, name: e.$1, color: e.$2, perms: e.$3);
                },
              ),
              RoundIconButton(
                icon: TrairIcons.trash,
                tooltip: 'Löschen',
                size: 32,
                onPressed: () async {
                  if (await confirmAction(context, '„${r.name}“ löschen?', 'Alle verlieren diese Rolle.', 'Löschen')) {
                    await c.deleteRole(r.id);
                  }
                },
              ),
            ] else if (r.everyone)
              RoundIconButton(
                icon: TrairIcons.pencil,
                tooltip: 'Rechte von @alle',
                size: 32,
                onPressed: () async {
                  final e = await _roleDialog(context, r, mine);
                  if (e != null) await c.editRole(r, perms: e.$3);
                },
              ),
          ],
        ),
    ];
  }

  String _permCount(int perms) {
    final n = Perm.names.keys.where((p) => perms & p == p).length;
    return n == 0 ? 'keine Rechte' : (n == Perm.names.length ? 'alle Rechte' : '$n Rechte');
  }

  /// Name, colour and rights of a role; null: cancelled.
  Future<(String, Color, int)?> _roleDialog(BuildContext context, CRole? role, int mine) =>
      showDialog<(String, Color, int)>(
        context: context,
        barrierColor: const Color(0x9905050A),
        builder: (_) => _RoleDialog(role: role, mine: mine),
      );

  // ------------------------------------------------------------ channels

  List<Widget> _channels(AppState state, CommunityInfo info) {
    final c = state.communities;
    return [
      Row(
        children: [
          const Expanded(
            child: Text(
              'Offene Kanäle (#) sind für die Moderation lesbar. Geschlossene (Schloss) sind Ende-zu-Ende '
              'verschlüsselt; das Trair eines Hüters nimmt Berechtigte auf.',
              style: TrairText.small,
            ),
          ),
          const SizedBox(width: 12),
          PillButton(
            label: 'Neuer Kanal',
            icon: TrairIcons.plus,
            height: 38,
            onPressed: () => _channelDialog(state, info, null),
          ),
        ],
      ),
      const SizedBox(height: 14),
      for (final ch in info.channels)
        _row(
          key: ValueKey('channel-setting-${ch.name}'),
          leading: ch.closed
              ? const Icon(TrairIcons.lock, size: 16, color: TrairColors.accentLight)
              : const Icon(TrairIcons.hash, size: 16, color: TrairColors.amber),
          title: ch.name,
          subtitle: [
            ch.category,
            if (ch.closed) 'geschlossen',
            if (ch.limited) 'nur ${_roleNames(info, ch.readRoles!)}',
            if (ch.readOnly) 'nur Moderation schreibt',
          ].join(' · '),
          actions: [
            RoundIconButton(
              icon: TrairIcons.pencil,
              tooltip: 'Bearbeiten',
              size: 32,
              onPressed: () => _channelDialog(state, info, ch),
            ),
            RoundIconButton(
              icon: TrairIcons.trash,
              tooltip: 'Löschen',
              size: 32,
              onPressed: () async {
                if (await confirmAction(
                  context,
                  '#${ch.name} löschen?',
                  'Alle Nachrichten darin werden vom Server gelöscht.',
                  'Löschen',
                )) {
                  await c.deleteChannel(ch.id);
                }
              },
            ),
          ],
        ),
    ];
  }

  String _roleNames(CommunityInfo info, List<int> ids) => ids.map((id) => info.role(id)?.name ?? '?').join(', ');

  Future<void> _channelDialog(AppState state, CommunityInfo info, CChannel? ch) async {
    final r = await showDialog<_ChannelValues>(
      context: context,
      barrierColor: const Color(0x9905050A),
      builder: (_) => _ChannelDialog(info: info, channel: ch),
    );
    if (r == null) return;
    final c = state.communities;
    if (ch == null) {
      await c.createChannel(r.name, r.category, topic: r.topic, read: r.read, write: r.write, closed: r.closed);
    } else {
      await c.editChannel(
        ch,
        name: r.name,
        category: r.category,
        topic: r.topic,
        read: r.read,
        write: r.write,
        keepAccess: false,
      );
    }
  }

  // ------------------------------------------------------------ invites

  List<Widget> _invites(AppState state) {
    final c = state.communities;
    final list = c.invites;
    return [
      const Text(
        'Einladungen zum Weitergeben. Gespeichert wird nur ein Fingerabdruck des Codes; '
        'den Code selbst siehst du nur beim Erstellen.',
        style: TrairText.small,
      ),
      const SizedBox(height: 12),
      Row(
        children: [
          PillButton(label: '24 Std. · einmal', height: 38, onPressed: () => c.createInvite(24 * 60, 1)),
          const SizedBox(width: 8),
          PillButton(
            label: '7 Tage · unbegrenzt',
            height: 38,
            kind: PillKind.ghost,
            onPressed: () => c.createInvite(7 * 24 * 60, null),
          ),
        ],
      ),
      if (c.newInvite != null) ...[
        const SizedBox(height: 12),
        SelectableText(c.newInvite!, key: const Key('new-invite'), style: TrairText.mono),
      ],
      const SizedBox(height: 18),
      if (list == null)
        const Text('Lädt …', style: TrairText.muted)
      else if (list.isEmpty)
        const Text('Keine aktiven Einladungen.', style: TrairText.muted)
      else
        for (final i in list)
          _row(
            leading: const Icon(TrairIcons.link, size: 16, color: TrairColors.textSoft),
            title: 'Einladung ${i.id}',
            subtitle:
                'von ${_name2(state, i.creator)} · ${i.uses == null ? 'unbegrenzt' : 'noch ${i.uses}×'} · bis ${whenText(i.ends)}',
            actions: [
              RoundIconButton(
                icon: TrairIcons.x,
                tooltip: 'Zurückziehen',
                size: 32,
                onPressed: () => c.revokeInvite(i.id),
              ),
            ],
          ),
    ];
  }

  // ------------------------------------------------------------ bans

  List<Widget> _bans(AppState state) {
    final c = state.communities;
    final list = c.bans;
    if (list == null) return const [Text('Lädt …', style: TrairText.muted)];
    if (list.isEmpty) return const [Text('Niemand ist gebannt.', style: TrairText.muted)];
    return [
      for (final b in list)
        _row(
          leading: const Icon(TrairIcons.ban, size: 16, color: TrairColors.rose),
          title: _name2(state, b.user),
          subtitle: [
            'von ${_name2(state, b.by)}',
            whenText(b.at),
            if (b.reason.isNotEmpty) '„${b.reason}“',
          ].join(' · '),
          actions: [PillButton(label: 'Entbannen', kind: PillKind.ghost, height: 32, onPressed: () => c.unban(b.user))],
        ),
    ];
  }

  // ------------------------------------------------------------ reports

  List<Widget> _reports(AppState state, CommunityInfo info) {
    final c = state.communities;
    final list = c.reports;
    return [
      LabeledSwitch(
        value: _allReports,
        label: 'Auch erledigte zeigen',
        onChanged: (v) {
          setState(() => _allReports = v);
          c.loadReports(all: v);
        },
      ),
      const SizedBox(height: 12),
      if (list == null)
        const Text('Lädt …', style: TrairText.muted)
      else if (list.isEmpty)
        const Text('Keine offenen Meldungen.', style: TrairText.muted)
      else
        for (final r in list) _reportCard(state, info, r),
    ];
  }

  Widget _reportCard(AppState state, CommunityInfo info, CReport r) {
    final c = state.communities;
    final target = r.target;
    return Padding(
      key: ValueKey('report-${r.id}'),
      padding: const EdgeInsets.only(bottom: 10),
      child: SoftCard(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              [
                '#${r.id}',
                if (target != null) 'über ${_name2(state, target)}',
                'von ${_name2(state, r.reporter)}',
                whenText(r.at),
                if (r.status == 'done') 'erledigt',
                if (r.status == 'dismissed') 'abgelehnt',
              ].join(' · '),
              style: TrairText.small,
            ),
            const SizedBox(height: 6),
            Text(r.reason, style: TrairText.body),
            if (r.quoted.isNotEmpty) ...[
              const SizedBox(height: 8),
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(10),
                decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(10)),
                child: Text(r.quoted, style: const TextStyle(fontSize: 13.5, color: TrairColors.textSoft)),
              ),
            ],
            if (r.note.isNotEmpty) ...[const SizedBox(height: 6), Text('Notiz: ${r.note}', style: TrairText.small)],
            if (r.status == 'open') ...[
              const SizedBox(height: 10),
              Row(
                children: [
                  PillButton(
                    label: 'Erledigt',
                    icon: TrairIcons.check,
                    height: 34,
                    onPressed: () async {
                      final note = await askText(
                        context,
                        title: 'Meldung #${r.id} erledigt',
                        text: 'Was wurde getan? (optional, sehen nur Moderatoren)',
                        hint: 'Notiz',
                        yes: 'Erledigt',
                        needed: false,
                      );
                      if (note != null) await c.handleReport(r.id, 'done', note: note);
                    },
                  ),
                  const SizedBox(width: 8),
                  PillButton(
                    label: 'Ablehnen',
                    kind: PillKind.ghost,
                    height: 34,
                    onPressed: () => c.handleReport(r.id, 'dismissed'),
                  ),
                  const SizedBox(width: 8),
                  if (target != null && info.member(target) != null && info.can(Perm.kick) && info.above(target))
                    PillButton(
                      label: 'Entfernen',
                      kind: PillKind.danger,
                      height: 34,
                      onPressed: () async {
                        if (await confirmAction(
                          context,
                          '${_name2(state, target)} entfernen?',
                          'Grund im Protokoll: Meldung #${r.id}.',
                          'Entfernen',
                        )) {
                          await c.kick(target, 'Meldung #${r.id}');
                        }
                      },
                    ),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }

  // ------------------------------------------------------------ log

  static const _actions = {
    'kick': 'hat entfernt',
    'ban': 'hat gebannt',
    'unban': 'hat entbannt',
    'roles': 'hat Rollen geändert von',
    'role_create': 'hat eine Rolle erstellt:',
    'role_edit': 'hat eine Rolle geändert:',
    'role_delete': 'hat eine Rolle gelöscht:',
    'channel_create': 'hat einen Kanal erstellt:',
    'channel_edit': 'hat einen Kanal geändert:',
    'channel_delete': 'hat einen Kanal gelöscht:',
    'message_delete': 'hat eine Nachricht gelöscht von',
    'invite': 'hat eine Einladung erstellt:',
    'invite_revoke': 'hat eine Einladung zurückgezogen:',
    'edit': 'hat die Community bearbeitet',
    'rules': 'hat die Regeln geändert',
    'report': 'hat eine Meldung bearbeitet:',
    'transfer': 'hat die Community übergeben an',
    'successor': 'ist weg; neuer Gründer:',
  };

  List<Widget> _log(AppState state) {
    final list = state.communities.log;
    if (list == null) return const [Text('Lädt …', style: TrairText.muted)];
    if (list.isEmpty) return const [Text('Noch nichts passiert.', style: TrairText.muted)];
    return [
      for (final e in list)
        Padding(
          padding: const EdgeInsets.only(bottom: 10),
          child: Text.rich(
            TextSpan(
              children: [
                TextSpan(
                  text: '${_name2(state, e.actor)} ',
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                TextSpan(text: '${_actions[e.action] ?? e.action} '),
                if (e.target.isNotEmpty)
                  TextSpan(
                    text:
                        const {
                          'kick',
                          'ban',
                          'unban',
                          'roles',
                          'message_delete',
                          'transfer',
                          'successor',
                        }.contains(e.action)
                        ? _name2(state, e.target)
                        : e.target,
                    style: const TextStyle(fontWeight: FontWeight.w600),
                  ),
                if (e.detail.isNotEmpty && !const {'roles', 'role_create', 'role_edit'}.contains(e.action))
                  TextSpan(
                    text: ' · ${e.detail}',
                    style: const TextStyle(color: TrairColors.textMuted),
                  ),
                TextSpan(
                  text: '  ${whenText(e.at)}',
                  style: const TextStyle(color: TrairColors.placeholder, fontSize: 12),
                ),
              ],
            ),
            style: const TextStyle(fontSize: 13.5, color: TrairColors.textSoft),
          ),
        ),
    ];
  }

  // ------------------------------------------------------------ leave

  List<Widget> _leave(AppState state, CommunityInfo info) {
    final c = state.communities;
    final alone = info.members.length <= 1;
    return [
      if (info.iAmOwner) ...[
        Text(
          alone
              ? 'Du bist allein hier. Verlassen löscht die Community.'
              : 'Du bist Gründer. Zum Verlassen erst übergeben (Mitglied anklicken → „Community übergeben“) '
                    'oder die Community für alle löschen.',
          style: TrairText.muted,
        ),
        const SizedBox(height: 14),
        PillButton(
          label: 'Community für alle löschen',
          icon: TrairIcons.trash,
          kind: PillKind.danger,
          height: 40,
          onPressed: () async {
            if (await confirmAction(
              context,
              '„${info.name}“ löschen?',
              'Alle Kanäle, Nachrichten, Rollen und Einladungen werden vom Server gelöscht. Das geht nicht rückgängig.',
              'Löschen',
            )) {
              if (await c.deleteCommunity()) state.go(TrairScreen.chats);
            }
          },
        ),
      ] else ...[
        const Text(
          'Du verlässt die Community. Deine Nachrichten bleiben stehen; mit einer neuen Einladung kannst du '
          'wiederkommen.',
          style: TrairText.muted,
        ),
        const SizedBox(height: 14),
        PillButton(
          label: 'Community verlassen',
          icon: TrairIcons.logOut,
          kind: PillKind.danger,
          height: 40,
          onPressed: () async {
            if (await confirmAction(
              context,
              '„${info.name}“ verlassen?',
              'Du kannst mit einer Einladung wiederkommen.',
              'Verlassen',
            )) {
              if (await c.leave()) state.go(TrairScreen.chats);
            }
          },
        ),
      ],
    ];
  }

  Widget _row({
    Key? key,
    required Widget leading,
    required String title,
    required String subtitle,
    List<Widget> actions = const [],
  }) => Padding(
    key: key,
    padding: const EdgeInsets.only(bottom: 8),
    child: SoftCard(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
      child: Row(
        children: [
          leading,
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title, style: const TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600)),
                if (subtitle.isNotEmpty) Text(subtitle, style: TrairText.small),
              ],
            ),
          ),
          ...actions,
        ],
      ),
    ),
  );
}

/// A role: name, colour, rights. Rights this account lacks are shown but
/// cannot be switched (the server refuses them anyway).
class _RoleDialog extends StatefulWidget {
  const _RoleDialog({required this.role, required this.mine});

  final CRole? role;
  final int mine;

  @override
  State<_RoleDialog> createState() => _RoleDialogState();
}

class _RoleDialogState extends State<_RoleDialog> {
  late final _name = TextEditingController(text: widget.role?.name ?? '');
  late Color _color = widget.role?.color ?? TrairColors.glowChoices.values.first;
  late int _perms = widget.role?.perms ?? 0;

  bool get _everyone => widget.role?.everyone ?? false;

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final role = widget.role;
    final mine = widget.mine;
    return DialogShell(
      title: role == null ? 'Neue Rolle' : (_everyone ? 'Rechte von @alle' : 'Rolle bearbeiten'),
      width: 460,
      children: [
        if (!_everyone) ...[
          PillField(
            controller: _name,
            hint: 'Name der Rolle',
            autofocus: role == null,
            semanticLabel: 'Name der Rolle',
          ),
          const SizedBox(height: 12),
          GlowSwatches(selected: _color, onPick: (c) => setState(() => _color = c)),
          const SizedBox(height: 14),
        ],
        for (final e in Perm.names.entries)
          LabeledSwitch(
            value: _perms & e.key == e.key,
            label: e.value,
            enabled: mine & e.key == e.key,
            onChanged: (v) => setState(() => _perms = v ? _perms | e.key : _perms & ~e.key),
          ),
        const SizedBox(height: 14),
        ListenableBuilder(
          listenable: _name,
          builder: (_, _) => Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              PillButton(label: 'Abbrechen', kind: PillKind.ghost, height: 38, onPressed: () => Navigator.pop(context)),
              const SizedBox(width: 8),
              PillButton(
                label: 'Speichern',
                height: 38,
                onPressed: !_everyone && _name.text.trim().isEmpty
                    ? null
                    : () => Navigator.pop(context, (_everyone ? role!.name : _name.text.trim(), _color, _perms)),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

/// What the channel dialog gives back.
class _ChannelValues {
  const _ChannelValues(this.name, this.category, this.topic, this.read, this.write, {this.closed = false});

  final String name;
  final String category;
  final String topic;
  final List<int>? read;
  final List<int>? write;

  /// End-to-end encrypted (only when making a channel).
  final bool closed;
}

class _ChannelDialog extends StatefulWidget {
  const _ChannelDialog({required this.info, required this.channel});

  final CommunityInfo info;
  final CChannel? channel;

  @override
  State<_ChannelDialog> createState() => _ChannelDialogState();
}

class _ChannelDialogState extends State<_ChannelDialog> {
  late final _name = TextEditingController(text: widget.channel?.name ?? '');
  late final _category = TextEditingController(
    text: widget.channel?.category ?? (widget.info.categories.firstOrNull?.$1 ?? 'Allgemein'),
  );
  late final _topic = TextEditingController(text: widget.channel?.topic ?? '');

  /// all (everyone reads and writes), read (managers write), roles (only these roles see it).
  late String _who = switch (widget.channel) {
    null => 'all',
    CChannel ch when ch.limited => 'roles',
    CChannel ch when ch.readOnly => 'read',
    _ => 'all',
  };
  late final Set<int> _roles = {...?widget.channel?.readRoles};
  bool _closed = false;

  @override
  void dispose() {
    _name.dispose();
    _category.dispose();
    _topic.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final ch = widget.channel;
    return DialogShell(
      title: ch == null ? 'Neuer Kanal' : '#${ch.name} bearbeiten',
      width: 460,
      children: [
        PillField(controller: _name, hint: 'name-des-kanals', autofocus: ch == null, semanticLabel: 'Kanalname'),
        const SizedBox(height: 10),
        PillField(controller: _category, hint: 'Kategorie', semanticLabel: 'Kategorie'),
        const SizedBox(height: 10),
        PillField(controller: _topic, hint: 'Thema (optional)', semanticLabel: 'Thema'),
        if (ch == null) ...[
          const SizedBox(height: 10),
          LabeledSwitch(
            key: const ValueKey('channel-closed'),
            value: _closed,
            label: 'Geschlossen (Ende-zu-Ende verschlüsselt)',
            onChanged: (v) => setState(() => _closed = v),
          ),
          Text(
            _closed
                ? 'Weder der Server noch die Moderation können mitlesen; nur ein gemeldeter Text wird sichtbar. '
                      'Das Trair eines Hüters nimmt Berechtigte auf, Neue bekommen die letzten 50 Nachrichten.'
                : 'Offen: Der Server kann die Nachrichten lesen, damit Moderation geht.',
            style: TrairText.small,
          ),
        ] else if (ch.closed)
          const Text('Geschlossen (Ende-zu-Ende) – das lässt sich nicht mehr ändern.', style: TrairText.small),
        const SizedBox(height: 14),
        const Text('Wer darf was?', style: TrairText.small),
        const SizedBox(height: 6),
        for (final (v, label) in const [
          ('all', 'Alle lesen und schreiben'),
          ('read', 'Alle lesen, nur Moderation schreibt'),
          ('roles', 'Nur bestimmte Rollen sehen ihn'),
        ])
          LabeledSwitch(value: _who == v, label: label, onChanged: (_) => setState(() => _who = v)),
        if (_who == 'roles')
          for (final r in widget.info.roles.where((r) => !r.everyone))
            Padding(
              padding: const EdgeInsets.only(left: 18),
              child: LabeledSwitch(
                value: _roles.contains(r.id),
                label: r.name,
                onChanged: (v) => setState(() => v ? _roles.add(r.id) : _roles.remove(r.id)),
              ),
            ),
        const SizedBox(height: 14),
        ListenableBuilder(
          listenable: Listenable.merge([_name, _category]),
          builder: (_, _) => Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              PillButton(label: 'Abbrechen', kind: PillKind.ghost, height: 38, onPressed: () => Navigator.pop(context)),
              const SizedBox(width: 8),
              PillButton(
                label: 'Speichern',
                height: 38,
                onPressed: _name.text.trim().isEmpty || _category.text.trim().isEmpty
                    ? null
                    : () => Navigator.pop(
                        context,
                        _ChannelValues(
                          _name.text.trim().toLowerCase().replaceAll(' ', '-'),
                          _category.text.trim(),
                          _topic.text.trim(),
                          _who == 'roles' ? _roles.toList() : null,
                          _who == 'read' ? <int>[] : null,
                          closed: _closed,
                        ),
                      ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}
