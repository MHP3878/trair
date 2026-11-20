// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';

import 'package:material_ui/material_ui.dart';
import 'package:flutter/services.dart';

import '../app_state.dart';
import '../core/clipboard.dart';
import '../backend/backend.dart';
import '../theme.dart';
import '../widgets/badges.dart';
import '../widgets/common.dart';
import '../widgets/profile_popup.dart';
import '../widgets/qr.dart';
import '../widgets/trair_icons.dart';
import 'community_dialogs.dart';
import 'group_panel.dart';

enum _Filter { all, direct, group }

class ChatsScreen extends StatefulWidget {
  const ChatsScreen({super.key});

  @override
  State<ChatsScreen> createState() => _ChatsScreenState();
}

class _ChatsScreenState extends State<ChatsScreen> {
  String _active = 'lena';
  _Filter _filter = _Filter.all;
  bool _safetyOpen = false;
  final Set<String> _read = {};
  final _search = TextEditingController();
  final _searchFocus = FocusNode(debugLabel: 'search');
  final _screenFocus = FocusNode(debugLabel: 'chats');
  final _draft = TextEditingController();

  /// Selected real chat (M4); null shows the demo chat.
  String? _real;
  bool _contactsOpen = false;
  int _keyMinutes = 60;
  final _code = TextEditingController();
  final _contactName = TextEditingController();

  /// The group column next to a group chat.
  bool _groupOpen = false;

  /// The "Neue Gruppe" page instead of a chat.
  bool _newGroupOpen = false;
  final _groupName = TextEditingController();
  final Set<String> _groupPick = {};
  int _groupMinutes = 0;

  @override
  void initState() {
    super.initState();
    // Take keyboard focus once the screen is shown, so Ctrl+K works right
    // away (the lock screen field had it before and is gone now).
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _screenFocus.requestFocus();
    });
  }

  @override
  void dispose() {
    _search.dispose();
    _searchFocus.dispose();
    _screenFocus.dispose();
    _draft.dispose();
    _code.dispose();
    _contactName.dispose();
    _groupName.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final backend = state.backend;
    final chats = backend.chats();
    final chat = chats.firstWhere((c) => c.id == _active, orElse: () => chats.first);
    return CallbackShortcuts(
      bindings: {const SingleActivator(LogicalKeyboardKey.keyK, control: true): _searchFocus.requestFocus},
      child: Focus(
        focusNode: _screenFocus,
        child: Backdrop(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Container(width: 300, color: TrairColors.sidebar, child: _sidebar(state, chats)),
              Expanded(
                child: _newGroupOpen
                    ? _newGroupPanel(state)
                    : _contactsOpen
                    ? _contactsPanel(state)
                    : (_realChat(state) != null
                          ? _realConversation(state, _realChat(state)!)
                          : _conversation(state, chat)),
              ),
              if (!_contactsOpen &&
                  !_newGroupOpen &&
                  _groupOpen &&
                  _realChat(state) != null &&
                  _realChat(state)!.isGroup &&
                  _realChat(state)!.state == 'active')
                GroupPanel(
                  chat: _realChat(state)!,
                  onClose: () => setState(() => _groupOpen = false),
                  onGone: () => setState(() {
                    _groupOpen = false;
                    _real = null;
                  }),
                ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _sidebar(AppState state, List<ChatSummary> chats) {
    final q = _search.text.trim().toLowerCase();
    final visible = chats.where((c) {
      final kindOk = switch (_filter) {
        _Filter.all => true,
        _Filter.direct => c.kind == ChatKind.direct,
        _Filter.group => c.kind == ChatKind.group,
      };
      return kindOk && (q.isEmpty || c.name.toLowerCase().contains(q));
    }).toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(14, 12, 10, 0),
          child: SidebarHeader(
            title: 'Direktnachrichten',
            trailing: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                RoundIconButton(
                  icon: TrairIcons.users,
                  tooltip: 'Neue Gruppe',
                  onPressed: () => setState(() {
                    _newGroupOpen = true;
                    _contactsOpen = false;
                    _real = null;
                  }),
                ),
                const SizedBox(width: 6),
                RoundIconButton(
                  icon: TrairIcons.squarePen,
                  tooltip: 'Neuer Chat',
                  onPressed: () => setState(() {
                    _contactsOpen = true;
                    _newGroupOpen = false;
                    _real = null;
                  }),
                ),
              ],
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(10, 8, 10, 0),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PillField(
                controller: _search,
                focusNode: _searchFocus,
                hint: 'Suchen',
                height: 38,
                semanticLabel: 'Chats durchsuchen (nur auf diesem Gerät)',
                leading: const Icon(TrairIcons.search, size: 18, color: TrairColors.textMuted),
                onChanged: (_) => setState(() {}),
                trailing: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
                  decoration: BoxDecoration(color: const Color(0x0FFFFFFF), borderRadius: BorderRadius.circular(999)),
                  child: const Text(
                    'Strg K',
                    style: TextStyle(fontFamily: 'GeistMono', fontSize: 11, color: TrairColors.textMuted),
                  ),
                ),
              ),
              const SizedBox(height: 10),
              GlassPanel(
                lite: true,
                radius: 999,
                padding: const EdgeInsets.all(3),
                child: Row(
                  children: [
                    for (final f in _Filter.values)
                      Expanded(
                        child: GestureDetector(
                          onTap: () => setState(() => _filter = f),
                          child: AnimatedContainer(
                            duration: const Duration(milliseconds: 200),
                            height: 32,
                            alignment: Alignment.center,
                            decoration: BoxDecoration(
                              color: _filter == f ? const Color(0x1AFFFFFF) : Colors.transparent,
                              borderRadius: BorderRadius.circular(999),
                            ),
                            child: Text(
                              switch (f) {
                                _Filter.all => 'Alle',
                                _Filter.direct => 'Direkt',
                                _Filter.group => 'Gruppen',
                              },
                              style: TextStyle(
                                fontSize: 13,
                                fontWeight: FontWeight.w500,
                                color: _filter == f ? TrairColors.text : TrairColors.textMuted,
                              ),
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 12),
        Expanded(
          child: ListView(
            padding: const EdgeInsets.symmetric(horizontal: 8),
            children: [
              if (state.chatsReady) ...[
                _sectionLabel('Echt verschlüsselt'),
                // Closed community channels live in their community.
                for (final c in state.realChats.where((c) => !c.isChannel)) _realRow(state, c),
                if (!state.realChats.any((c) => !c.isChannel))
                  const Padding(
                    padding: EdgeInsets.fromLTRB(12, 2, 12, 8),
                    child: Text('Noch keine. Oben auf „Neuer Chat“.', style: TrairText.small),
                  ),
                const SizedBox(height: 8),
                _sectionLabel('Demo (nicht verschlüsselt)'),
              ],
              for (final c in visible) _chatRow(state, c),
              if (visible.isEmpty)
                const Padding(
                  padding: EdgeInsets.all(12),
                  child: Text('Kein Chat gefunden.', style: TrairText.muted),
                ),
              const SizedBox(height: 10),
            ],
          ),
        ),
        const ProfileCard(),
      ],
    );
  }

  Widget _chatRow(AppState state, ChatSummary c) {
    final backend = state.backend;
    final active = c.id == _active && _real == null && !_contactsOpen;
    final warn = backend.safetyNumberChanged(c.id);
    final msgs = backend.messages(c.id).where((m) => m.kind != MessageKind.system).toList();
    final last = msgs.isEmpty ? null : msgs.last;
    final prefix = last == null
        ? ''
        : last.kind == MessageKind.outgoing
        ? 'Du: '
        : (last.sender != null ? '${last.sender}: ' : '');
    final preview = warn ? 'Neue Sicherheitsnummer' : (last == null ? '' : '$prefix${last.text}');
    final unread = active || _read.contains(c.id) ? 0 : c.unread;
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Material(
        color: active ? TrairColors.cardActive : Colors.transparent,
        borderRadius: BorderRadius.circular(8),
        child: InkWell(
          borderRadius: BorderRadius.circular(8),
          onTap: () => setState(() {
            _active = c.id;
            _real = null;
            _contactsOpen = false;
            _newGroupOpen = false;
            _read.add(c.id);
            _safetyOpen = false;
            _draft.clear();
          }),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(8, 7, 10, 7),
            child: Row(
              children: [
                Avatar(initials: c.initials, color: c.color, size: 40),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              c.name,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: 14.5,
                                fontWeight: FontWeight.w600,
                                color: active ? c.color : TrairColors.text,
                              ),
                            ),
                          ),
                          Text(c.time, style: TrairText.small),
                        ],
                      ),
                      const SizedBox(height: 3),
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              preview,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(fontSize: 13, color: warn ? TrairColors.amber : TrairColors.textMuted),
                            ),
                          ),
                          if (unread > 0)
                            Container(
                              padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
                              decoration: BoxDecoration(
                                color: TrairColors.accent,
                                borderRadius: BorderRadius.circular(999),
                              ),
                              child: Text(
                                '$unread',
                                style: const TextStyle(fontSize: 11, fontWeight: FontWeight.w600, color: Colors.white),
                              ),
                            ),
                        ],
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  // ------------------------------------------------------- real chats (M4)

  Widget _sectionLabel(String text) => Padding(
    padding: const EdgeInsets.only(left: 10, bottom: 6, top: 6),
    child: Text(
      text,
      style: const TextStyle(
        fontSize: 11.5,
        fontWeight: FontWeight.w600,
        letterSpacing: 0.3,
        color: TrairColors.textMuted,
      ),
    ),
  );

  RealChat? _realChat(AppState state) => _real == null ? null : state.realChats.where((c) => c.id == _real).firstOrNull;

  void _openReal(AppState state, String id) {
    setState(() {
      _real = id;
      _contactsOpen = false;
      _newGroupOpen = false;
      _draft.clear();
    });
    unawaited(state.loadMessages(id));
    if (state.realChats.any((c) => c.id == id && c.isGroup)) unawaited(state.loadGroup(id));
  }

  /// A chat partner's name: yours for them, in the colour they chose.
  TextStyle _peerName(PeerProfile? p, double size) => TextStyle(
    fontSize: size,
    fontWeight: FontWeight.w600,
    color: p?.glow,
    shadows: p?.glow == null ? null : glowShadows(p!.glow!, strength: 20),
  );

  Widget _realRow(AppState state, RealChat c) {
    if (c.kind != 'direct') return _groupRow(state, c);
    final active = c.id == _real && !_contactsOpen;
    final p = state.profileFor(c);
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Material(
        color: active ? TrairColors.cardActive : Colors.transparent,
        borderRadius: BorderRadius.circular(8),
        child: InkWell(
          borderRadius: BorderRadius.circular(8),
          onTap: () => _openReal(state, c.id),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(8, 7, 10, 7),
            child: Row(
              children: [
                Avatar(
                  initials: _initials(c.name),
                  color: p?.glow ?? TrairColors.accentLight,
                  size: 40,
                  picture: p?.picture,
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Flexible(
                            child: Text(c.name, overflow: TextOverflow.ellipsis, style: _peerName(p, 14.5)),
                          ),
                          if (p != null && p.badges.isNotEmpty) ...[
                            const SizedBox(width: 6),
                            BadgeRow(badges: p.badges, size: 14, spacing: 3),
                          ],
                        ],
                      ),
                      Text(
                        switch (c.state) {
                          'request' => 'Anfrage: kein Kontakt',
                          'left' => 'Dieses Gerät ist nicht mehr dabei',
                          _ when c.ends != null => 'Verschwindet ${vanishesIn(c.ends!)}',
                          _ => 'Ende-zu-Ende verschlüsselt',
                        },
                        style: c.ends != null && c.state == 'active'
                            ? const TextStyle(fontSize: 12, color: TrairColors.amber)
                            : TrairText.small,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ],
                  ),
                ),
                if (c.isRequest) const Icon(TrairIcons.triangleAlert, size: 16, color: TrairColors.amber),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// A group or a join request in the list.
  Widget _groupRow(AppState state, RealChat c) {
    final active = c.id == _real && !_contactsOpen;
    final sub = switch ((c.kind, c.state)) {
      ('join', _) => 'Beitritt: wartet auf ${state.displayName(c.peer)}',
      (_, 'request') => 'Gruppeneinladung',
      (_, 'left') => 'Nicht mehr in der Gruppe',
      _ when c.ends != null => 'Verschwindet ${vanishesIn(c.ends!)}',
      _ => [
        '${c.members} ${c.members == 1 ? 'Mitglied' : 'Mitglieder'}',
        if (c.role == 'owner') 'deine',
        if (c.role == 'admin') 'Admin',
        if (c.role == 'guest') 'Gast',
      ].join(' · '),
    };
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Material(
        color: active ? TrairColors.cardActive : Colors.transparent,
        borderRadius: BorderRadius.circular(8),
        child: InkWell(
          borderRadius: BorderRadius.circular(8),
          onTap: () => _openReal(state, c.id),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(8, 7, 10, 7),
            child: Row(
              children: [
                Container(
                  width: 40,
                  height: 40,
                  alignment: Alignment.center,
                  decoration: BoxDecoration(color: const Color(0x246E4FF6), borderRadius: BorderRadius.circular(13)),
                  child: c.isJoin
                      ? const Icon(TrairIcons.clock, size: 18, color: TrairColors.accentLight)
                      : Text(
                          _initials(c.name),
                          style: const TextStyle(
                            color: TrairColors.accentLight,
                            fontWeight: FontWeight.w700,
                            fontSize: 14,
                          ),
                        ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Flexible(
                            child: Text(
                              c.isJoin ? 'Gruppenbeitritt' : c.name,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(
                                fontSize: 14.5,
                                fontWeight: FontWeight.w600,
                                color: TrairColors.text,
                              ),
                            ),
                          ),
                          if (c.isGroup) ...[
                            const SizedBox(width: 6),
                            const Icon(TrairIcons.users, size: 13, color: TrairColors.textMuted),
                          ],
                        ],
                      ),
                      Text(
                        sub,
                        style: c.ends != null && c.state == 'active' && c.isGroup
                            ? const TextStyle(fontSize: 12, color: TrairColors.amber)
                            : TrairText.small,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ],
                  ),
                ),
                if (c.isRequest) const Icon(TrairIcons.triangleAlert, size: 16, color: TrairColors.amber),
              ],
            ),
          ),
        ),
      ),
    );
  }

  static String _initials(String name) {
    final parts = name.trim().split(RegExp(r'\s+')).where((p) => p.isNotEmpty).toList();
    if (parts.isEmpty) return '?';
    return parts.take(2).map((p) => String.fromCharCode(p.runes.first).toUpperCase()).join();
  }

  static const _keyChoices = {10: '10 Min.', 60: '1 Std.', 360: '6 Std.', 1440: '1 Tag', 10080: '7 Tage'};

  /// Make a friendship key: once, for the chosen time; or a key for a chat
  /// that vanishes at the same time.
  List<Widget> _friendKey(AppState state) {
    final key = state.friendKey;
    String two(int n) => n.toString().padLeft(2, '0');
    String when(DateTime t) {
      final now = DateTime.now();
      final day = t.day == now.day && t.month == now.month ? 'heute' : '${two(t.day)}.${two(t.month)}.';
      return '$day um ${two(t.hour)}:${two(t.minute)}';
    }

    return [
      const FieldLabel('Freundschaftsschlüssel'),
      const Text(
        'Wer ihn rechtzeitig benutzt, ist sofort dein Kontakt, ohne Anfrage. Er gilt nur für eine Person; '
        'weitergeleitet oder zu spät ist er wertlos.',
        style: TrairText.muted,
      ),
      const SizedBox(height: 10),
      Wrap(
        spacing: 8,
        runSpacing: 8,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          const Text('Gilt', style: TrairText.small),
          for (final e in _keyChoices.entries)
            IntrinsicWidth(
              child: PillButton(
                label: e.value,
                height: 34,
                kind: e.key == _keyMinutes ? PillKind.primary : PillKind.ghost,
                onPressed: () => setState(() => _keyMinutes = e.key),
              ),
            ),
        ],
      ),
      const SizedBox(height: 10),
      Wrap(
        spacing: 8,
        runSpacing: 8,
        children: [
          IntrinsicWidth(
            child: PillButton(
              label: 'Schlüssel erstellen',
              icon: TrairIcons.userPlus,
              height: 40,
              onPressed: () => state.createFriendKey(_keyMinutes),
            ),
          ),
          IntrinsicWidth(
            child: PillButton(
              label: 'Verschwindender Chat',
              kind: PillKind.ghost,
              height: 40,
              onPressed: () => state.createFriendKey(_keyMinutes, vanishing: true),
            ),
          ),
        ],
      ),
      if (key != null) ...[
        const SizedBox(height: 12),
        Container(
          padding: const EdgeInsets.all(14),
          decoration: BoxDecoration(
            color: const Color(0x176E4FF6),
            borderRadius: BorderRadius.circular(12),
            border: Border.all(color: const Color(0x38A18DFF)),
          ),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      key.vanishing
                          ? 'Einmal nutzbar · der Chat verschwindet ${when(key.ends)}'
                          : 'Einmal nutzbar · gilt bis ${when(key.ends)}',
                      style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: TrairColors.accentPale),
                    ),
                    const SizedBox(height: 8),
                    Row(
                      children: [
                        Expanded(
                          child: SelectableText(
                            key.code,
                            style: const TextStyle(fontFamily: 'GeistMono', fontSize: 12, color: TrairColors.text),
                          ),
                        ),
                        RoundIconButton(
                          icon: TrairIcons.copy,
                          tooltip: 'Schlüssel kopieren',
                          filled: false,
                          onPressed: () => Clipboard.setData(ClipboardData(text: key.code)),
                        ),
                        RoundIconButton(
                          icon: TrairIcons.x,
                          tooltip: 'Schlüssel ausblenden',
                          filled: false,
                          onPressed: state.closeFriendKey,
                        ),
                      ],
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 14),
              QrBox(text: key.code, size: 112),
            ],
          ),
        ),
      ],
    ];
  }

  static const _groupEnds = {0: 'Bleibt', 1440: 'Nach 1 Tag weg', 10080: 'Nach 7 Tagen weg'};

  /// Start a group: a name, contacts to add now, and whether it vanishes.
  List<Widget> _newGroup(AppState state) {
    final name = _groupName.text.trim();
    return [
      const Text(
        'Du bist Besitzer. Name, Mitglieder und Rollen sind Ende-zu-Ende verschlüsselt; '
        'weitere Leute lädst du später mit einem Einladungsschlüssel ein.',
        style: TrairText.muted,
      ),
      const SizedBox(height: 10),
      PillField(
        controller: _groupName,
        hint: 'Gruppenname',
        height: 46,
        semanticLabel: 'Gruppenname',
        onChanged: (_) => setState(() {}),
      ),
      const SizedBox(height: 10),
      if (state.contacts.isNotEmpty) ...[
        Wrap(
          spacing: 8,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            const Text('Mit', style: TrairText.small),
            for (final c in state.contacts)
              IntrinsicWidth(
                child: PillButton(
                  label: c.name,
                  height: 34,
                  kind: _groupPick.contains(c.user) ? PillKind.primary : PillKind.ghost,
                  onPressed: () =>
                      setState(() => _groupPick.contains(c.user) ? _groupPick.remove(c.user) : _groupPick.add(c.user)),
                ),
              ),
          ],
        ),
        const SizedBox(height: 10),
      ],
      Wrap(
        spacing: 8,
        runSpacing: 8,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          for (final e in _groupEnds.entries)
            IntrinsicWidth(
              child: PillButton(
                label: e.value,
                height: 34,
                kind: e.key == _groupMinutes ? PillKind.primary : PillKind.ghost,
                onPressed: () => setState(() => _groupMinutes = e.key),
              ),
            ),
        ],
      ),
      const SizedBox(height: 10),
      Align(
        alignment: Alignment.centerLeft,
        child: IntrinsicWidth(
          child: PillButton(
            label: 'Gruppe erstellen',
            icon: TrairIcons.users,
            height: 40,
            onPressed: name.isEmpty || !state.chatsOnline
                ? null
                : () async {
                    final id = await state.createGroup(
                      name,
                      _groupPick.toList(),
                      minutes: _groupMinutes == 0 ? null : _groupMinutes,
                    );
                    if (id != null && mounted) {
                      _groupName.clear();
                      _groupPick.clear();
                      _groupMinutes = 0;
                      _openReal(state, id);
                      setState(() => _groupOpen = true);
                    }
                  },
          ),
        ),
      ),
    ];
  }

  Widget _newGroupPanel(AppState state) => GlassPanel(
    child: ListView(
      padding: const EdgeInsets.fromLTRB(28, 22, 28, 28),
      children: [
        Row(
          children: [
            const Expanded(child: Text('Neue Gruppe', style: TrairText.title)),
            RoundIconButton(
              icon: TrairIcons.x,
              tooltip: 'Schließen',
              filled: false,
              onPressed: () => setState(() => _newGroupOpen = false),
            ),
          ],
        ),
        const SizedBox(height: 14),
        if (!state.chatsReady)
          const Text('Die Chat-Datenbank ist nicht offen.', style: TrairText.muted)
        else ...[
          ..._newGroup(state),
          if (!state.chatsOnline)
            const Padding(
              padding: EdgeInsets.only(top: 10),
              child: Text('Gruppen erstellen geht, sobald der Server verbunden ist.', style: TrairText.small),
            ),
          if (state.chatMessage.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 10),
              child: Text(state.chatMessage, style: const TextStyle(color: TrairColors.amber, fontSize: 13)),
            ),
        ],
      ],
    ),
  );

  Widget _contactsPanel(AppState state) {
    final online = state.chatsOnline;
    return GlassPanel(
      child: ListView(
        padding: const EdgeInsets.fromLTRB(28, 22, 28, 28),
        children: [
          Row(
            children: [
              const Expanded(child: Text('Kontakte', style: TrairText.title)),
              RoundIconButton(
                icon: TrairIcons.x,
                tooltip: 'Schließen',
                filled: false,
                onPressed: () => setState(() => _contactsOpen = false),
              ),
            ],
          ),
          const SizedBox(height: 6),
          const Text(
            'Kein Telefonbuch, keine Suche: Ihr tauscht Kontaktcodes. Der Code legt den Schlüssel des Kontos fest. '
            'Persönlich oder per QR-Code weitergegeben, kann ihn niemand austauschen.',
            style: TrairText.muted,
          ),
          const SizedBox(height: 18),
          if (!state.chatsReady)
            const Text('Die Chat-Datenbank ist nicht offen.', style: TrairText.muted)
          else ...[
            const FieldLabel('Dein Kontaktcode'),
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Container(
                        padding: const EdgeInsets.fromLTRB(18, 8, 6, 8),
                        decoration: BoxDecoration(
                          color: TrairColors.field,
                          borderRadius: BorderRadius.circular(18),
                          border: Border.all(color: TrairColors.borderSoft),
                        ),
                        child: Row(
                          children: [
                            Expanded(
                              child: SelectableText(
                                state.contactCode,
                                style: const TextStyle(
                                  fontFamily: 'GeistMono',
                                  fontSize: 12.5,
                                  color: TrairColors.text,
                                ),
                              ),
                            ),
                            RoundIconButton(
                              icon: TrairIcons.copy,
                              tooltip: 'Code kopieren',
                              filled: false,
                              onPressed: () => Clipboard.setData(ClipboardData(text: state.contactCode)),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(height: 10),
                      const Text(
                        'Oder den QR-Code zeigen: Am Handy wird er gescannt, sobald es die Android-App gibt.',
                        style: TrairText.small,
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: 20),
                QrBox(text: state.contactCode, size: 132),
              ],
            ),
            const SizedBox(height: 22),
            ..._friendKey(state),
            const SizedBox(height: 22),
            const FieldLabel('Kontakt hinzufügen oder Gruppe beitreten'),
            PillField(controller: _code, hint: 'trair1:…', mono: true, height: 46, semanticLabel: 'Kontaktcode'),
            const SizedBox(height: 10),
            Row(
              children: [
                Expanded(
                  child: PillField(
                    controller: _contactName,
                    hint: 'Name (nur für dich)',
                    height: 46,
                    semanticLabel: 'Name',
                  ),
                ),
                const SizedBox(width: 10),
                PillButton(
                  label: 'Hinzufügen',
                  onPressed: () async {
                    final code = _code.text.trim();
                    // A group key: ask its admin to let this account in.
                    if (code.toLowerCase().startsWith('trair3:')) {
                      final id = await state.joinGroup(code);
                      if (id != null && mounted) {
                        _code.clear();
                        _openReal(state, id);
                      }
                      return;
                    }
                    if (await state.addContact(code, _contactName.text)) {
                      _code.clear();
                      _contactName.clear();
                      // A friendship key: the chat starts right away.
                      final parts = code.toLowerCase().split(':');
                      if (parts.first == 'trair2' && parts.length > 1 && state.chatsOnline) {
                        final id = await state.startChat(parts[1]);
                        if (id != null && mounted) _openReal(state, id);
                      }
                    }
                  },
                ),
              ],
            ),
            if (state.chatMessage.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 10),
                child: Text(state.chatMessage, style: const TextStyle(color: TrairColors.amber, fontSize: 13)),
              ),
            const SizedBox(height: 22),
            FieldLabel('Deine Kontakte · ${state.contacts.length}'),
            if (state.contacts.isEmpty) const Text('Noch keine.', style: TrairText.muted),
            for (final c in state.contacts)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Row(
                  children: [
                    Avatar(initials: _initials(c.name), color: TrairColors.accentLight, size: 36),
                    const SizedBox(width: 12),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(c.name, style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600)),
                          Text(
                            c.fingerprint,
                            style: const TextStyle(fontFamily: 'GeistMono', fontSize: 11, color: TrairColors.textMuted),
                          ),
                        ],
                      ),
                    ),
                    PillButton(
                      label: 'Chat starten',
                      kind: PillKind.ghost,
                      height: 38,
                      onPressed: online
                          ? () async {
                              final id = await state.startChat(c.user);
                              if (id != null && mounted) _openReal(state, id);
                            }
                          : null,
                    ),
                  ],
                ),
              ),
            if (!online && state.contacts.isNotEmpty)
              const Text(
                'Chats starten geht, sobald der Server verbunden ist (Einstellungen → Server).',
                style: TrairText.small,
              ),
          ],
        ],
      ),
    );
  }

  /// A note of the core in a group's history (joined, left, renamed).
  Widget _note(AppState state, RealMessage m) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 6),
    child: Center(
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 5),
        decoration: BoxDecoration(color: const Color(0x0DFFFFFF), borderRadius: BorderRadius.circular(999)),
        child: Text(
          groupNoteText(m.text, (u) => u == state.ownUser ? 'Du' : state.displayName(u)),
          style: TrairText.small,
          textAlign: TextAlign.center,
        ),
      ),
    ),
  );

  /// A real message, shaped like the demo ones: tail corner and time. In a
  /// group the sender's name stands above what others wrote.
  Widget _realBubble(RealMessage m, {String? sender, Color? senderColor}) {
    final t = DateTime.fromMillisecondsSinceEpoch(m.at * 1000);
    final time = '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}';
    const big = Radius.circular(22);
    const small = Radius.circular(8);
    return Align(
      alignment: m.outgoing ? Alignment.centerRight : Alignment.centerLeft,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 540),
        child: Padding(
          padding: const EdgeInsets.only(bottom: 8),
          child: Column(
            crossAxisAlignment: m.outgoing ? CrossAxisAlignment.end : CrossAxisAlignment.start,
            children: [
              if (sender != null)
                Padding(
                  padding: const EdgeInsets.fromLTRB(12, 0, 12, 3),
                  child: Text(
                    sender,
                    style: TextStyle(
                      fontSize: 12.5,
                      fontWeight: FontWeight.w600,
                      color: senderColor ?? TrairColors.accentLight,
                      shadows: senderColor == null ? null : glowShadows(senderColor, strength: 14),
                    ),
                  ),
                ),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
                decoration: BoxDecoration(
                  color: m.outgoing ? TrairColors.accent : TrairColors.bubbleOther,
                  borderRadius: m.outgoing
                      ? const BorderRadius.only(topLeft: big, topRight: big, bottomLeft: big, bottomRight: small)
                      : const BorderRadius.only(topLeft: big, topRight: big, bottomRight: big, bottomLeft: small),
                ),
                child: Text(m.text, style: m.outgoing ? TrairText.body.copyWith(color: Colors.white) : TrairText.body),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 4, 12, 0),
                child: Text(
                  m.missing > 0 ? '$time · ${m.missing} davor fehlen' : time,
                  style: TextStyle(fontSize: 11, color: m.missing > 0 ? TrairColors.amber : TrairColors.placeholder),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// A group chat, or this account's join request waiting for an admin.
  Widget _groupConversation(AppState state, RealChat chat) {
    final messages = state.realMessages[chat.id] ?? const <RealMessage>[];
    final info = state.groupInfos[chat.id];
    final changed =
        info?.members.where((m) => m.keyChanged).map((m) => state.displayName(m.user)).toList() ?? const <String>[];
    final canWrite = chat.isGroup && chat.state == 'active' && state.chatsOnline;
    final admin = state.displayName(chat.peer);
    Future<void> send() async {
      final text = _draft.text.trim();
      if (text.isEmpty || !canWrite) return;
      _draft.clear();
      await state.sendReal(chat.id, text);
    }

    Widget banner(String text, List<Widget> actions, {bool warn = false}) => Padding(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 10),
      child: GlassPanel(
        lite: true,
        padding: const EdgeInsets.all(14),
        child: Row(
          children: [
            if (warn) ...[
              const Icon(TrairIcons.triangleAlert, size: 18, color: TrairColors.amber),
              const SizedBox(width: 10),
            ],
            Expanded(child: Text(text, style: TrairText.muted)),
            for (final a in actions) ...[const SizedBox(width: 8), a],
          ],
        ),
      ),
    );

    return GlassPanel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            decoration: const BoxDecoration(
              border: Border(bottom: BorderSide(color: TrairColors.borderSoft)),
            ),
            padding: const EdgeInsets.fromLTRB(20, 11, 16, 11),
            child: Row(
              children: [
                Container(
                  width: 40,
                  height: 40,
                  alignment: Alignment.center,
                  decoration: BoxDecoration(color: const Color(0x246E4FF6), borderRadius: BorderRadius.circular(13)),
                  child: chat.isJoin
                      ? const Icon(TrairIcons.clock, size: 18, color: TrairColors.accentLight)
                      : Text(
                          _initials(chat.name),
                          style: const TextStyle(color: TrairColors.accentLight, fontWeight: FontWeight.w700),
                        ),
                ),
                const SizedBox(width: 14),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        chat.isJoin ? 'Gruppenbeitritt' : chat.name,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600, color: TrairColors.text),
                      ),
                      Text(
                        chat.isJoin
                            ? 'Wartet auf $admin'
                            : [
                                if (chat.ends != null) 'Verschwindet ${vanishesIn(chat.ends!)}',
                                '${chat.members} ${chat.members == 1 ? 'Mitglied' : 'Mitglieder'}',
                                'Gruppe',
                              ].join(' · '),
                        overflow: TextOverflow.ellipsis,
                        style: chat.ends != null && chat.isGroup
                            ? const TextStyle(fontSize: 12, color: TrairColors.amber)
                            : TrairText.small,
                      ),
                    ],
                  ),
                ),
                if (chat.isGroup && chat.state == 'active') ...[
                  RoundIconButton(
                    icon: TrairIcons.users,
                    tooltip: 'Gruppe verwalten',
                    filled: _groupOpen,
                    onPressed: () => setState(() => _groupOpen = !_groupOpen),
                  ),
                  const SizedBox(width: 10),
                ],
                SecurityCapsule(
                  text: chat.isRequest ? 'Einladung · unbekannter Admin' : 'Echt Ende-zu-Ende verschlüsselt',
                  tone: chat.isRequest || changed.isNotEmpty ? CapsuleTone.warn : CapsuleTone.safe,
                ),
              ],
            ),
          ),
          if (chat.isRequest)
            banner(
              'Jemand, der kein Kontakt ist, hat dich in diese Gruppe geholt. Schreiben geht erst nach dem Annehmen; '
              'Ablehnen löscht sie hier und sagt den anderen, dass du gehst.',
              [
                PillButton(label: 'Annehmen', height: 38, onPressed: () => state.acceptGroup(chat.id)),
                PillButton(
                  label: 'Ablehnen',
                  kind: PillKind.danger,
                  height: 38,
                  onPressed: () async {
                    if (await state.leaveGroup(chat.id) && mounted) setState(() => _real = null);
                  },
                ),
              ],
            ),
          if (chat.state == 'left')
            banner('Du bist nicht mehr in dieser Gruppe. Der Verlauf ist nur noch hier.', [
              PillButton(
                label: 'Chat löschen',
                icon: TrairIcons.trash,
                kind: PillKind.danger,
                height: 38,
                onPressed: () async {
                  await state.deleteChat(chat.id);
                  if (mounted) setState(() => _real = null);
                },
              ),
            ]),
          if (changed.isNotEmpty && state.strict && chat.state == 'active')
            banner(
              'Der Schlüssel von ${changed.join(', ')} hat sich geändert. Der strenge Modus sperrt das Senden, '
              'bis du es prüfst: „Gruppe verwalten“ → Mitglied → „Neuen Schlüssel annehmen“.',
              const [],
              warn: true,
            ),
          if (state.chatMessage.isNotEmpty)
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
              child: Text(state.chatMessage, style: const TextStyle(color: TrairColors.amber, fontSize: 13)),
            ),
          Expanded(
            child: chat.isJoin
                ? Center(
                    child: Padding(
                      padding: const EdgeInsets.all(32),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          const Icon(TrairIcons.clock, size: 34, color: TrairColors.accentLight),
                          const SizedBox(height: 12),
                          Text(
                            'Beitritt angefragt. Sobald das Trair von $admin deinen Schlüssel geprüft hat, '
                            'erscheint die Gruppe hier. Dafür muss Trair bei $admin laufen.',
                            textAlign: TextAlign.center,
                            style: TrairText.muted,
                          ),
                        ],
                      ),
                    ),
                  )
                : ListView(
                    padding: const EdgeInsets.fromLTRB(28, 8, 28, 16),
                    children: [
                      for (final m in messages)
                        if (m.isNote)
                          _note(state, m)
                        else
                          _realBubble(
                            m,
                            sender: m.outgoing ? null : state.displayName(m.user),
                            senderColor: m.outgoing ? null : state.peerProfiles[m.user]?.glow,
                          ),
                    ],
                  ),
          ),
          if (!chat.isJoin)
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 0, 20, 18),
              child: GlassPanel(
                lite: true,
                radius: 999,
                padding: const EdgeInsets.all(6),
                child: Row(
                  children: [
                    const SizedBox(width: 12),
                    Expanded(
                      child: TextField(
                        key: const Key('real-composer'),
                        controller: _draft,
                        enabled: canWrite,
                        onSubmitted: (_) => send(),
                        style: const TextStyle(fontSize: 14.5, color: TrairColors.text),
                        decoration: InputDecoration.collapsed(
                          hintText: switch (chat.state) {
                            'request' => 'Erst annehmen',
                            'left' => 'Du bist nicht mehr in der Gruppe',
                            _ => state.chatsOnline ? 'Nachricht an ${chat.name}' : 'Nicht verbunden',
                          },
                          hintStyle: const TextStyle(color: TrairColors.placeholder, fontSize: 14.5),
                        ),
                      ),
                    ),
                    RoundIconButton(
                      icon: TrairIcons.arrowUp,
                      tooltip: 'Senden',
                      size: 40,
                      color: TrairColors.accent,
                      iconColor: Colors.white,
                      onPressed: canWrite ? send : null,
                    ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }

  Widget _realConversation(AppState state, RealChat chat) {
    if (chat.kind != 'direct') return _groupConversation(state, chat);
    final messages = state.realMessages[chat.id] ?? const <RealMessage>[];
    final p = state.profileFor(chat);
    final chosen = p == null || p.name.trim().toLowerCase() == chat.name.trim().toLowerCase() ? null : p.name;
    final canWrite = chat.state == 'active' && state.chatsOnline;
    Future<void> send() async {
      final text = _draft.text.trim();
      if (text.isEmpty || !canWrite) return;
      _draft.clear();
      await state.sendReal(chat.id, text);
    }

    return GlassPanel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            decoration: const BoxDecoration(
              border: Border(bottom: BorderSide(color: TrairColors.borderSoft)),
            ),
            padding: const EdgeInsets.fromLTRB(20, 11, 16, 11),
            child: Row(
              children: [
                Expanded(
                  child: Material(
                    color: Colors.transparent,
                    child: InkWell(
                      borderRadius: BorderRadius.circular(16),
                      // The card shows what they sent; nothing from strangers.
                      onTap: p == null
                          ? null
                          : () => showProfileCard(
                              context,
                              ProfileView.peer(chat, p),
                              onReport: state.communities.online && !chat.isGroup
                                  ? () => showTeamReport(
                                      context,
                                      state,
                                      user: chat.peer,
                                      name: chat.name,
                                      chatMessages: [
                                        for (final m
                                            in (state.realMessages[chat.id] ?? const <RealMessage>[])
                                                .where((m) => !m.isNote && m.text.isNotEmpty)
                                                .toList()
                                                .reversed
                                                .take(20)
                                                .toList()
                                                .reversed)
                                          {'from': m.outgoing ? 'ich' : chat.peer, 'text': m.text, 'at': m.at},
                                      ],
                                    )
                                  : null,
                            ),
                      child: Padding(
                        padding: const EdgeInsets.symmetric(vertical: 2),
                        child: Row(
                          children: [
                            Avatar(
                              initials: _initials(chat.name),
                              color: p?.glow ?? TrairColors.accentLight,
                              size: 40,
                              picture: p?.picture,
                            ),
                            const SizedBox(width: 14),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Row(
                                    children: [
                                      Flexible(
                                        child: Text(
                                          chat.name,
                                          overflow: TextOverflow.ellipsis,
                                          style: _peerName(p, 16),
                                        ),
                                      ),
                                      if (p != null && p.badges.isNotEmpty) ...[
                                        const SizedBox(width: 8),
                                        BadgeRow(badges: p.badges, size: 16, spacing: 4),
                                      ],
                                    ],
                                  ),
                                  Text(
                                    [
                                      if (chat.ends != null) 'Verschwindet ${vanishesIn(chat.ends!)}',
                                      if (chosen != null) 'nennt sich „$chosen“',
                                      chat.peer,
                                    ].join(' · '),
                                    overflow: TextOverflow.ellipsis,
                                    style: chat.ends != null
                                        ? const TextStyle(fontSize: 12, color: TrairColors.amber)
                                        : TrairText.small,
                                  ),
                                ],
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 12),
                if (chat.state == 'active')
                  PopupMenuButton<int>(
                    tooltip: 'Chat verschwinden lassen',
                    color: TrairColors.glassPopup,
                    icon: const Icon(TrairIcons.ellipsis, size: 18, color: TrairColors.textSoft),
                    onSelected: (m) => state.setChatEnd(chat.id, m == 0 ? null : m),
                    itemBuilder: (_) => [
                      const PopupMenuItem(
                        enabled: false,
                        child: Text('Verschwindet für beide nach …', style: TrairText.small),
                      ),
                      for (final e in const {60: '1 Stunde', 1440: '1 Tag', 10080: '7 Tagen'}.entries)
                        PopupMenuItem(value: e.key, child: Text(e.value)),
                      if (chat.ends != null) const PopupMenuItem(value: 0, child: Text('Nicht verschwinden lassen')),
                    ],
                  ),
                const SizedBox(width: 6),
                SecurityCapsule(
                  text: chat.isRequest ? 'Anfrage · unbekannter Schlüssel' : 'Echt Ende-zu-Ende verschlüsselt',
                  tone: chat.isRequest ? CapsuleTone.warn : CapsuleTone.safe,
                ),
              ],
            ),
          ),
          if (chat.isRequest)
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 0, 20, 10),
              child: GlassPanel(
                lite: true,
                padding: const EdgeInsets.all(14),
                child: Row(
                  children: [
                    const Expanded(
                      child: Text(
                        'Diese Person ist kein Kontakt. Antworten geht erst nach dem Annehmen. Ablehnen löscht den Chat.',
                        style: TrairText.muted,
                      ),
                    ),
                    const SizedBox(width: 10),
                    PillButton(label: 'Annehmen', height: 38, onPressed: () => state.acceptRequest(chat.id, '')),
                    const SizedBox(width: 8),
                    PillButton(
                      label: 'Ablehnen',
                      kind: PillKind.danger,
                      height: 38,
                      onPressed: () async {
                        await state.declineRequest(chat.id);
                        if (mounted) setState(() => _real = null);
                      },
                    ),
                  ],
                ),
              ),
            ),
          if (chat.state == 'left')
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 0, 20, 10),
              child: GlassPanel(
                lite: true,
                padding: const EdgeInsets.all(14),
                child: Row(
                  children: [
                    const Expanded(
                      child: Text(
                        'Dieses Gerät ist nicht mehr im Chat. Der Verlauf ist nur noch hier.',
                        style: TrairText.muted,
                      ),
                    ),
                    const SizedBox(width: 10),
                    PillButton(
                      label: 'Chat löschen',
                      kind: PillKind.danger,
                      height: 38,
                      onPressed: () async {
                        await state.deleteChat(chat.id);
                        if (mounted) setState(() => _real = null);
                      },
                    ),
                  ],
                ),
              ),
            ),
          if (state.chatMessage.isNotEmpty)
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
              child: Text(state.chatMessage, style: const TextStyle(color: TrairColors.amber, fontSize: 13)),
            ),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.fromLTRB(28, 8, 28, 16),
              children: [for (final m in messages) _realBubble(m)],
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 0, 20, 18),
            child: GlassPanel(
              lite: true,
              radius: 999,
              padding: const EdgeInsets.all(6),
              child: Row(
                children: [
                  const SizedBox(width: 12),
                  Expanded(
                    child: TextField(
                      key: const Key('real-composer'),
                      controller: _draft,
                      enabled: canWrite,
                      onSubmitted: (_) => send(),
                      style: const TextStyle(fontSize: 14.5, color: TrairColors.text),
                      decoration: InputDecoration.collapsed(
                        hintText: switch (chat.state) {
                          'request' => 'Erst annehmen',
                          'left' => 'Dieses Gerät ist nicht mehr im Chat',
                          _ => state.chatsOnline ? 'Nachricht an ${chat.name}' : 'Nicht verbunden',
                        },
                        hintStyle: const TextStyle(color: TrairColors.placeholder, fontSize: 14.5),
                      ),
                    ),
                  ),
                  RoundIconButton(
                    icon: TrairIcons.arrowUp,
                    tooltip: 'Senden',
                    size: 40,
                    color: TrairColors.accent,
                    iconColor: Colors.white,
                    onPressed: canWrite ? send : null,
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _conversation(AppState state, ChatSummary chat) {
    final backend = state.backend;
    final changed = backend.safetyNumberChanged(chat.id);
    final blocked = changed && state.strict;
    final isGroup = chat.kind == ChatKind.group;
    final messages = backend.messages(chat.id);
    final capsuleText = changed
        ? 'Sicherheitsnummer geändert'
        : (isGroup ? 'Verschlüsselt · alle bestätigt' : 'Verschlüsselt · bestätigt');

    void send() {
      final text = _draft.text.trim();
      if (text.isEmpty || blocked) return;
      backend.send(chat.id, text);
      _draft.clear();
      state.touch();
    }

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
                padding: const EdgeInsets.fromLTRB(20, 11, 16, 11),
                child: Row(
                  children: [
                    Expanded(
                      child: Row(
                        children: [
                          Avatar(initials: chat.initials, color: chat.color, size: 40),
                          const SizedBox(width: 14),
                          Flexible(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  chat.name,
                                  overflow: TextOverflow.ellipsis,
                                  style: TextStyle(
                                    fontSize: 16,
                                    fontWeight: FontWeight.w600,
                                    color: chat.color,
                                    shadows: glowShadows(chat.color, strength: 20),
                                  ),
                                ),
                                Text(
                                  isGroup ? '${chat.members} Mitglieder · private Gruppe' : 'Direktnachricht',
                                  style: TrairText.small,
                                ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                    SecurityCapsule(
                      text: capsuleText,
                      tone: changed ? CapsuleTone.warn : CapsuleTone.safe,
                      open: _safetyOpen,
                      onTap: () => setState(() => _safetyOpen = !_safetyOpen),
                    ),
                    Expanded(
                      child: Row(
                        mainAxisAlignment: MainAxisAlignment.end,
                        children: [
                          RoundIconButton(
                            icon: TrairIcons.search,
                            tooltip: 'Im Chat suchen',
                            filled: false,
                            onPressed: () {},
                          ),
                          RoundIconButton(icon: TrairIcons.ellipsis, tooltip: 'Mehr', filled: false, onPressed: () {}),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
              AnimatedSize(
                duration: const Duration(milliseconds: 250),
                curve: Curves.easeOutCubic,
                child: _safetyOpen ? _safetyCard(backend, chat) : const SizedBox(width: double.infinity),
              ),
              if (changed) _warnBanner(state, chat, blocked),
              Expanded(
                child: ListView(
                  padding: const EdgeInsets.fromLTRB(28, 8, 28, 110),
                  children: [for (final m in messages) _bubble(m, chat)],
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
                constraints: const BoxConstraints(maxWidth: 720),
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 20),
                  child: GlassPanel(
                    lite: true,
                    radius: 999,
                    padding: const EdgeInsets.all(6),
                    child: Row(
                      children: [
                        RoundIconButton(
                          icon: TrairIcons.plus,
                          tooltip: 'Datei anhängen',
                          size: 40,
                          onPressed: blocked ? null : () {},
                        ),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Semantics(
                            label: 'Nachricht schreiben',
                            textField: true,
                            child: TextField(
                              key: const Key('composer'),
                              controller: _draft,
                              enabled: !blocked,
                              onSubmitted: (_) => send(),
                              style: const TextStyle(fontSize: 14.5, color: TrairColors.text),
                              decoration: InputDecoration.collapsed(
                                hintText: blocked
                                    ? 'Blockiert: erst Sicherheitsnummer prüfen'
                                    : 'Nachricht an ${chat.name}',
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
                          onPressed: blocked ? null : send,
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

  Widget _safetyCard(TrairBackend backend, ChatSummary chat) {
    final groups = backend.safetyNumber(chat.id);
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 12),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 460),
          child: GlassPanel(
            lite: true,
            radius: 24,
            color: TrairColors.glassPopup,
            padding: const EdgeInsets.all(18),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  chat.kind == ChatKind.group ? 'Sicherheitscode dieser Gruppe' : 'Sicherheitsnummer mit ${chat.name}',
                  style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
                ),
                const SizedBox(height: 4),
                const Text(
                  'Vergleicht die Zahlen persönlich oder bei einem Anruf. Stimmen sie, liest niemand dazwischen mit.',
                  style: TrairText.muted,
                ),
                const SizedBox(height: 12),
                Container(
                  padding: const EdgeInsets.all(14),
                  decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(16)),
                  child: Wrap(
                    alignment: WrapAlignment.center,
                    spacing: 18,
                    runSpacing: 8,
                    children: [
                      for (final g in groups)
                        Text(g, style: TrairText.mono.copyWith(color: const Color(0xFFE2DCFF), letterSpacing: 0.6)),
                    ],
                  ),
                ),
                const SizedBox(height: 10),
                Align(
                  alignment: Alignment.centerRight,
                  child: PillButton(
                    label: 'Kopieren',
                    icon: TrairIcons.copy,
                    kind: PillKind.ghost,
                    height: 36,
                    onPressed: () => copySensitive(groups.join(' ')),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _warnBanner(AppState state, ChatSummary chat, bool blocked) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 6),
      child: Container(
        padding: const EdgeInsets.fromLTRB(14, 12, 12, 12),
        decoration: BoxDecoration(
          color: const Color(0x12FFB547),
          borderRadius: BorderRadius.circular(20),
          border: Border.all(color: const Color(0x3DFFB547)),
        ),
        child: Row(
          children: [
            Container(
              width: 34,
              height: 34,
              decoration: const BoxDecoration(color: Color(0x24FFB547), shape: BoxShape.circle),
              child: const Icon(TrairIcons.triangleAlert, size: 18, color: TrairColors.amber),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                blocked
                    ? '${chat.name}s Sicherheitsnummer hat sich plötzlich geändert. Das passiert bei Trair nie von selbst. '
                          'Senden ist blockiert, bis ihr die Nummer persönlich verglichen habt.'
                    : '${chat.name}s Sicherheitsnummer hat sich geändert. Der strenge Modus ist aus, deshalb nur diese Warnung.',
                style: const TextStyle(color: TrairColors.amberText, fontSize: 14, height: 1.5),
              ),
            ),
            const SizedBox(width: 12),
            PillButton(
              label: 'Verglichen, stimmt',
              kind: PillKind.amber,
              height: 40,
              onPressed: () {
                state.backend.confirmSafetyNumber(chat.id);
                state.touch();
              },
            ),
          ],
        ),
      ),
    );
  }

  Widget _bubble(ChatMessage m, ChatSummary chat) {
    switch (m.kind) {
      case MessageKind.system:
        return Center(
          child: Container(
            margin: const EdgeInsets.symmetric(vertical: 10),
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 6),
            decoration: BoxDecoration(color: const Color(0x09FFFFFF), borderRadius: BorderRadius.circular(999)),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Icon(TrairIcons.lock, size: 12, color: TrairColors.accentLight),
                const SizedBox(width: 8),
                Flexible(
                  child: Text(m.text, style: TrairText.small, textAlign: TextAlign.center),
                ),
              ],
            ),
          ),
        );
      case MessageKind.incoming:
        final color = m.senderColor ?? chat.color;
        return Align(
          alignment: Alignment.centerLeft,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 540),
            child: Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (m.sender != null)
                    Padding(
                      padding: const EdgeInsets.only(left: 14, bottom: 4),
                      child: Text(
                        m.sender!,
                        style: TextStyle(
                          fontSize: 12.5,
                          fontWeight: FontWeight.w600,
                          color: color,
                          shadows: glowShadows(color, strength: 20),
                        ),
                      ),
                    ),
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
                    decoration: const BoxDecoration(
                      color: TrairColors.bubbleOther,
                      borderRadius: BorderRadius.only(
                        topLeft: Radius.circular(22),
                        topRight: Radius.circular(22),
                        bottomRight: Radius.circular(22),
                        bottomLeft: Radius.circular(8),
                      ),
                    ),
                    child: Text(m.text, style: TrairText.body),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(left: 14, top: 4),
                    child: Text(m.time, style: const TextStyle(fontSize: 11, color: TrairColors.placeholder)),
                  ),
                ],
              ),
            ),
          ),
        );
      case MessageKind.outgoing:
        return Align(
          alignment: Alignment.centerRight,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 540),
            child: Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.end,
                children: [
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
                    decoration: const BoxDecoration(
                      color: TrairColors.accent,
                      borderRadius: BorderRadius.only(
                        topLeft: Radius.circular(22),
                        topRight: Radius.circular(22),
                        bottomLeft: Radius.circular(22),
                        bottomRight: Radius.circular(8),
                      ),
                    ),
                    child: Text(m.text, style: TrairText.body.copyWith(color: Colors.white)),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(right: 12, top: 4),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          '${m.time} · zugestellt',
                          style: const TextStyle(fontSize: 11, color: TrairColors.placeholder),
                        ),
                        const SizedBox(width: 4),
                        const Icon(TrairIcons.check, size: 12, color: TrairColors.placeholder),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        );
    }
  }
}

/// "in 5 Min.", "in 3 Std.", "in 2 Tagen" until [ends] (seconds since 1970).
String vanishesIn(int ends) {
  final left = ends - DateTime.now().millisecondsSinceEpoch ~/ 1000;
  if (left < 60) return 'gleich';
  if (left < 3600) return 'in ${left ~/ 60} Min.';
  if (left < 2 * 86400) return 'in ${left ~/ 3600} Std.';
  return 'in ${left ~/ 86400} Tagen';
}
