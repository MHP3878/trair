// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:flutter/painting.dart' show Color;

import 'backend.dart';

/// Demo data only. Nothing here is encrypted or sent anywhere.
class DemoBackend implements TrairBackend {
  DemoBackend() {
    _messages.addAll({
      'lena': [
        const ChatMessage(kind: MessageKind.system, text: 'Ende-zu-Ende verschlüsselt. Nur ihr beide könnt das lesen.'),
        const ChatMessage(kind: MessageKind.incoming, text: 'Hey, die Einladung hat geklappt!', time: '14:21'),
        const ChatMessage(
          kind: MessageKind.outgoing,
          text: 'Super. Lass uns morgen kurz die Sicherheitsnummer vergleichen.',
          time: '14:24',
        ),
        const ChatMessage(
          kind: MessageKind.incoming,
          text: 'Haben wir doch schon, bei mir steht jetzt „Bestätigt“.',
          time: '14:26',
        ),
        const ChatMessage(
          kind: MessageKind.incoming,
          text: 'Koppelst du noch dein Handy? Sonst ist bei einem PC-Defekt alles weg.',
          time: '14:32',
        ),
        const ChatMessage(kind: MessageKind.outgoing, text: 'Mach ich heute Abend.', time: '14:33'),
      ],
      'malik': [
        const ChatMessage(kind: MessageKind.system, text: 'Ende-zu-Ende verschlüsselt.'),
        const ChatMessage(kind: MessageKind.incoming, text: 'Kommst du morgen zum Training?', time: '12:58'),
        const ChatMessage(kind: MessageKind.outgoing, text: 'Ja, bin dabei.', time: '13:01'),
        const ChatMessage(kind: MessageKind.incoming, text: 'Super, bis dann.', time: '13:05'),
      ],
      'projekt': [
        const ChatMessage(kind: MessageKind.system, text: 'Private Gruppe · Ende-zu-Ende verschlüsselt'),
        const ChatMessage(kind: MessageKind.system, text: 'Du hast Jonas hinzugefügt.'),
        const ChatMessage(
          kind: MessageKind.incoming,
          text: 'Das Design steht fast. Violett bleibt.',
          time: 'Gestern 19:40',
          sender: 'Aylin',
          senderColor: _rosa,
        ),
        const ChatMessage(
          kind: MessageKind.incoming,
          text: 'Und was kommt als Erstes?',
          time: 'Gestern 19:42',
          sender: 'Jonas',
          senderColor: _bernstein,
        ),
        const ChatMessage(
          kind: MessageKind.outgoing,
          text: 'Der Launcher. Danach Chats und Gruppen.',
          time: 'Gestern 19:45',
        ),
      ],
      'aylin': [
        const ChatMessage(kind: MessageKind.system, text: 'Ende-zu-Ende verschlüsselt.'),
        const ChatMessage(kind: MessageKind.incoming, text: 'Bis morgen!', time: 'Mo 21:10'),
      ],
    });
    for (final ch in _community.categories.expand((c) => c.channels)) {
      _channel[ch.id] = List.of(_channelSeed[ch.id] ?? const <CommunityMessage>[]);
    }
  }

  static const _blau = Color(0xFF5CC8FF);
  static const _minze = Color(0xFF5EE6B0);
  static const _violett = Color(0xFFA18DFF);
  static const _rosa = Color(0xFFFF7AC6);
  static const _bernstein = Color(0xFFFFB547);
  static const _hell = Color(0xFFE8E4F7);

  static const julian = Person(name: 'Julian', color: _violett, trust: 3, role: 'Gründer');
  static const lena = Person(name: 'Lena', color: _blau, trust: 3, role: 'Admin');
  static const malik = Person(name: 'Malik', color: _minze, trust: 2, role: 'Moderation');
  static const aylin = Person(name: 'Aylin', color: _rosa, trust: 2);
  static const jonas = Person(name: 'Jonas', color: _bernstein, trust: 1);
  static const noah = Person(name: 'Noah', color: _hell, trust: 0);

  final Map<String, List<ChatMessage>> _messages = {};
  final Map<String, List<CommunityMessage>> _channel = {};
  final Set<String> _changedNumbers = {'malik'};

  static const _chats = [
    ChatSummary(id: 'lena', name: 'Lena', initials: 'L', color: _blau, kind: ChatKind.direct, time: '14:33'),
    ChatSummary(
      id: 'malik',
      name: 'Malik',
      initials: 'M',
      color: _minze,
      kind: ChatKind.direct,
      time: '13:05',
      unread: 2,
    ),
    ChatSummary(
      id: 'projekt',
      name: 'Projekt Trair',
      initials: 'PT',
      color: _violett,
      kind: ChatKind.group,
      time: 'Gestern',
      members: 4,
    ),
    ChatSummary(id: 'aylin', name: 'Aylin', initials: 'A', color: _rosa, kind: ChatKind.direct, time: 'Mo'),
  ];

  static const _community = Community(
    name: 'Trair Beta',
    initials: 'TB',
    categories: [
      ChannelCategory('Info', [
        Channel(id: 'willkommen', name: 'willkommen', topic: 'Für alle sichtbar', open: true),
        Channel(id: 'regeln', name: 'regeln', topic: 'Bitte einmal lesen', open: true),
      ]),
      ChannelCategory('Gespräche', [
        Channel(id: 'allgemein', name: 'allgemein', topic: 'Alles rund um die Beta', open: false),
        Channel(id: 'projekte', name: 'projekte', topic: 'Woran ihr gerade baut', open: false),
        Channel(id: 'memes', name: 'memes', topic: 'Locker bleiben', open: true),
      ]),
      ChannelCategory('Team', [
        Channel(
          id: 'moderation',
          name: 'moderation',
          topic: 'Nur Gründer, Admins, Moderation',
          open: false,
          teamOnly: true,
        ),
      ]),
    ],
    roles: [
      RoleGroup('Gründer', [julian]),
      RoleGroup('Admin', [lena]),
      RoleGroup('Moderation', [malik]),
      RoleGroup('Mitglieder', [aylin, jonas, noah]),
    ],
  );

  static const _channelSeed = <String, List<CommunityMessage>>{
    'willkommen': [
      CommunityMessage(
        author: julian,
        time: 'Mo 18:02',
        text: 'Willkommen in der Trair-Beta! Schaut kurz in #regeln und schreibt dann in #allgemein.',
      ),
    ],
    'regeln': [
      CommunityMessage(
        author: lena,
        time: 'Mo 18:10',
        text: '1. Respektvoll bleiben. 2. Kein Spam. 3. Sicherheitslücken bitte privat melden, nicht öffentlich posten.',
      ),
    ],
    'allgemein': [
      CommunityMessage(author: aylin, time: '15:02', text: 'Kann man die Leuchtfarbe pro Community ändern?'),
      CommunityMessage(author: lena, time: '15:04', text: 'Noch nicht, aber gute Idee. Kommt auf die Liste.'),
      CommunityMessage(author: jonas, time: '15:06', text: 'Bei mir hat der Launcher beim ersten Start etwas gebraucht.'),
      CommunityMessage(
        author: julian,
        time: '15:07',
        text: 'Das ist Argon2, der macht das Passwort absichtlich langsam zu knacken. Danke fürs Melden!',
      ),
    ],
    'projekte': [
      CommunityMessage(author: malik, time: '12:30', text: 'Ich teste gerade, ob große Gruppen flüssig bleiben.'),
    ],
    'memes': [
      CommunityMessage(author: noah, time: '11:15', text: 'Hallo zusammen, bin neu hier.'),
      CommunityMessage(author: aylin, time: '11:16', text: 'Willkommen, Noah!'),
    ],
    'moderation': [
      CommunityMessage(author: malik, time: '10:41', text: 'Eine Meldung aus #memes, schon erledigt.'),
    ],
  };

  @override
  bool get isDemo => true;

  @override
  List<ChatSummary> chats() => _chats;

  @override
  List<ChatMessage> messages(String chatId) => List.unmodifiable(_messages[chatId] ?? const <ChatMessage>[]);

  @override
  bool safetyNumberChanged(String chatId) => _changedNumbers.contains(chatId);

  @override
  void confirmSafetyNumber(String chatId) {
    if (_changedNumbers.remove(chatId)) {
      _messages[chatId]?.add(
        const ChatMessage(
          kind: MessageKind.system,
          text: 'Du hast die Sicherheitsnummer persönlich geprüft. Senden ist wieder möglich.',
        ),
      );
    }
  }

  @override
  List<String> safetyNumber(String chatId) {
    var seed = 0;
    for (final c in chatId.codeUnits) {
      seed = (seed * 31 + c) % 99991;
    }
    final groups = <String>[];
    for (var i = 0; i < 12; i++) {
      seed = (seed * 48271 + 11) % 2147483647;
      groups.add((seed % 100000).toString().padLeft(5, '0'));
    }
    return groups;
  }

  @override
  void send(String chatId, String text) {
    _messages.putIfAbsent(chatId, () => []).add(ChatMessage(kind: MessageKind.outgoing, text: text, time: 'jetzt'));
  }

  @override
  Community community() => _community;

  @override
  List<CommunityMessage> channelMessages(String channelId) =>
      List.unmodifiable(_channel[channelId] ?? const <CommunityMessage>[]);

  @override
  void sendToChannel(String channelId, String text) {
    _channel.putIfAbsent(channelId, () => []).add(CommunityMessage(author: julian, time: 'jetzt', text: text));
  }
}
