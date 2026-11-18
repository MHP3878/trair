// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';
import 'package:flutter/services.dart';

import '../app_state.dart';
import '../backend/backend.dart';
import '../theme.dart';
import '../widgets/common.dart';

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
              SizedBox(width: 304, child: _sidebar(state, chats)),
              Expanded(
                child: Padding(padding: const EdgeInsets.fromLTRB(0, 10, 10, 10), child: _conversation(state, chat)),
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
    final community = state.backend.community();
    return Padding(
      padding: const EdgeInsets.fromLTRB(18, 18, 14, 14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SidebarHeader(
            trailing: RoundIconButton(icon: Icons.edit_outlined, tooltip: 'Neuer Chat', onPressed: () {}),
          ),
          const SizedBox(height: 18),
          PillField(
            controller: _search,
            focusNode: _searchFocus,
            hint: 'Suchen',
            height: 42,
            semanticLabel: 'Chats durchsuchen (nur auf diesem Gerät)',
            leading: const Icon(Icons.search_rounded, size: 18, color: TrairColors.textMuted),
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
          const SizedBox(height: 12),
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
          const SizedBox(height: 14),
          Expanded(
            child: ListView(
              children: [
                for (final c in visible) _chatRow(state, c),
                if (visible.isEmpty)
                  const Padding(
                    padding: EdgeInsets.all(12),
                    child: Text('Kein Chat gefunden.', style: TrairText.muted),
                  ),
                const SizedBox(height: 14),
                const Padding(
                  padding: EdgeInsets.only(left: 12, bottom: 6),
                  child: Text('Communities', style: TrairText.small),
                ),
                InkWell(
                  borderRadius: BorderRadius.circular(18),
                  onTap: () => state.go(TrairScreen.community),
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(10, 8, 12, 8),
                    child: Row(
                      children: [
                        Avatar(initials: community.initials, color: TrairColors.accentPale, size: 36, square: true),
                        const SizedBox(width: 12),
                        Expanded(
                          child: Text(
                            community.name,
                            style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w500),
                          ),
                        ),
                        Text('${community.memberCount} Mitglieder', style: TrairText.small),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 10),
          const ProfileCard(),
        ],
      ),
    );
  }

  Widget _chatRow(AppState state, ChatSummary c) {
    final backend = state.backend;
    final active = c.id == _active;
    final warn = backend.safetyNumberChanged(c.id);
    final msgs = backend.messages(c.id).where((m) => m.kind != MessageKind.system).toList();
    final last = msgs.isEmpty ? null : msgs.last;
    final prefix = last == null
        ? ''
        : last.kind == MessageKind.outgoing
        ? 'Du: '
        : (last.sender != null ? '${last.sender}: ' : '');
    final preview = warn ? 'Sicherheitsnummer geändert' : (last == null ? '' : '$prefix${last.text}');
    final unread = active || _read.contains(c.id) ? 0 : c.unread;
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Material(
        color: active ? TrairColors.cardActive : Colors.transparent,
        borderRadius: BorderRadius.circular(18),
        child: InkWell(
          borderRadius: BorderRadius.circular(18),
          onTap: () => setState(() {
            _active = c.id;
            _read.add(c.id);
            _safetyOpen = false;
            _draft.clear();
          }),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(10, 10, 12, 10),
            child: Row(
              children: [
                Avatar(initials: c.initials, color: c.color, size: 44),
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
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 14, 16, 14),
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
                            icon: Icons.search_rounded,
                            tooltip: 'Im Chat suchen',
                            filled: false,
                            onPressed: () {},
                          ),
                          RoundIconButton(
                            icon: Icons.more_horiz_rounded,
                            tooltip: 'Mehr',
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
                          icon: Icons.add_rounded,
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
                          icon: Icons.arrow_upward_rounded,
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
              child: const Icon(Icons.warning_amber_rounded, size: 18, color: TrairColors.amber),
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
                const Icon(Icons.lock_outline_rounded, size: 12, color: TrairColors.accentLight),
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
                        const Icon(Icons.check_rounded, size: 12, color: TrairColors.placeholder),
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
