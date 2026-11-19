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
  final _code = TextEditingController();
  final _contactName = TextEditingController();

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
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(0, 10, 10, 10),
                  child: _contactsOpen
                      ? _contactsPanel(state)
                      : (_realChat(state) != null
                            ? _realConversation(state, _realChat(state)!)
                            : _conversation(state, chat)),
                ),
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
            trailing: RoundIconButton(
              icon: TrairIcons.squarePen,
              tooltip: 'Neuer Chat',
              onPressed: () => setState(() {
                _contactsOpen = true;
                _real = null;
              }),
            ),
          ),
          const SizedBox(height: 18),
          PillField(
            controller: _search,
            focusNode: _searchFocus,
            hint: 'Suchen',
            height: 42,
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
                if (state.chatsReady) ...[
                  _sectionLabel('Echt verschlüsselt'),
                  for (final c in state.realChats) _realRow(state, c),
                  if (state.realChats.isEmpty)
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
        borderRadius: BorderRadius.circular(18),
        child: InkWell(
          borderRadius: BorderRadius.circular(18),
          onTap: () => setState(() {
            _active = c.id;
            _real = null;
            _contactsOpen = false;
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

  // ------------------------------------------------------- real chats (M4)

  Widget _sectionLabel(String text) => Padding(
    padding: const EdgeInsets.only(left: 12, bottom: 6, top: 2),
    child: Text(text, style: TrairText.small),
  );

  RealChat? _realChat(AppState state) => _real == null ? null : state.realChats.where((c) => c.id == _real).firstOrNull;

  void _openReal(AppState state, String id) {
    setState(() {
      _real = id;
      _contactsOpen = false;
      _draft.clear();
    });
    unawaited(state.loadMessages(id));
  }

  /// A chat partner's name: yours for them, in the colour they chose.
  TextStyle _peerName(PeerProfile? p, double size) => TextStyle(
    fontSize: size,
    fontWeight: FontWeight.w600,
    color: p?.glow,
    shadows: p?.glow == null ? null : glowShadows(p!.glow!, strength: 20),
  );

  Widget _realRow(AppState state, RealChat c) {
    final active = c.id == _real && !_contactsOpen;
    final p = state.profileFor(c);
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Material(
        color: active ? TrairColors.cardActive : Colors.transparent,
        borderRadius: BorderRadius.circular(18),
        child: InkWell(
          borderRadius: BorderRadius.circular(18),
          onTap: () => _openReal(state, c.id),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(10, 10, 12, 10),
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
                          _ => 'Ende-zu-Ende verschlüsselt',
                        },
                        style: TrairText.small,
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
            const SizedBox(height: 20),
            const FieldLabel('Kontakt hinzufügen'),
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
                    if (await state.addContact(_code.text, _contactName.text)) {
                      _code.clear();
                      _contactName.clear();
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

  /// A real message, shaped like the demo ones: tail corner and time.
  Widget _realBubble(RealMessage m) {
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

  Widget _realConversation(AppState state, RealChat chat) {
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
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 14, 16, 14),
            child: Row(
              children: [
                Expanded(
                  child: Material(
                    color: Colors.transparent,
                    child: InkWell(
                      borderRadius: BorderRadius.circular(16),
                      // The card shows what they sent; nothing from strangers.
                      onTap: p == null ? null : () => showProfileCard(context, ProfileView.peer(chat, p)),
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
                                    chosen == null ? chat.peer : 'nennt sich „$chosen“ · ${chat.peer}',
                                    overflow: TextOverflow.ellipsis,
                                    style: TrairText.small,
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
