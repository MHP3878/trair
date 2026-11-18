// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:flutter/painting.dart' show Color;

/// What the app needs from a backend. Today only [DemoBackend] implements
/// it. Later the Rust core implements the same interface through a
/// bridge, so the screens do not have to be rebuilt.
abstract class TrairBackend {
  /// True while no real encryption is connected. The app then shows the
  /// "Demo – nicht verschlüsselt" badge on every screen.
  bool get isDemo;

  List<ChatSummary> chats();
  List<ChatMessage> messages(String chatId);

  /// The safety number of a contact changed and was not checked yet.
  bool safetyNumberChanged(String chatId);

  /// The user compared the number in person and confirms it.
  void confirmSafetyNumber(String chatId);

  /// Twelve groups of five digits.
  List<String> safetyNumber(String chatId);

  void send(String chatId, String text);

  Community community();
  List<CommunityMessage> channelMessages(String channelId);
  void sendToChannel(String channelId, String text);
}

enum ChatKind { direct, group }

class ChatSummary {
  const ChatSummary({
    required this.id,
    required this.name,
    required this.initials,
    required this.color,
    required this.kind,
    required this.time,
    this.unread = 0,
    this.members = 2,
  });

  final String id;
  final String name;
  final String initials;
  final Color color;
  final ChatKind kind;
  final String time;
  final int unread;
  final int members;
}

enum MessageKind { system, incoming, outgoing }

class ChatMessage {
  const ChatMessage({required this.kind, required this.text, this.time = '', this.sender, this.senderColor});

  final MessageKind kind;
  final String text;
  final String time;

  /// Only for group messages from others.
  final String? sender;
  final Color? senderColor;
}

class Community {
  const Community({required this.name, required this.initials, required this.categories, required this.roles});

  final String name;
  final String initials;
  final List<ChannelCategory> categories;
  final List<RoleGroup> roles;

  int get memberCount => roles.fold(0, (n, r) => n + r.members.length);
}

class ChannelCategory {
  const ChannelCategory(this.name, this.channels);
  final String name;
  final List<Channel> channels;
}

class Channel {
  const Channel({required this.id, required this.name, required this.topic, required this.open, this.teamOnly = false});

  final String id;
  final String name;
  final String topic;

  /// Open channels are readable by the server for moderation; closed
  /// channels are end-to-end encrypted.
  final bool open;
  final bool teamOnly;
}

class RoleGroup {
  const RoleGroup(this.name, this.members);
  final String name;
  final List<Person> members;
}

class Person {
  const Person({required this.name, required this.color, required this.trust, this.role = ''});

  final String name;
  final Color color;

  /// Trust level 0..3.
  final int trust;
  final String role;
}

class CommunityMessage {
  const CommunityMessage({required this.author, required this.time, required this.text});

  final Person author;
  final String time;
  final String text;
}
