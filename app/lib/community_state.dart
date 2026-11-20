// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';
import 'dart:ui' show Color;

import 'core/server.dart';
import 'core/vault.dart';

/// Rights in a community, as bits (crates/trair-server/src/communities.rs).
abstract final class Perm {
  static const manageCommunity = 1;
  static const manageChannels = 2;
  static const manageRoles = 4;
  static const kick = 8;
  static const ban = 16;
  static const deleteMessages = 32;
  static const invite = 64;
  static const reports = 128;
  static const log = 256;
  static const all = 511;

  /// Each right with its name in the app, in display order.
  static const names = <int, String>{
    manageCommunity: 'Community verwalten (Name, Beschreibung, Regeln)',
    manageChannels: 'Kanäle verwalten und überall lesen',
    manageRoles: 'Rollen verwalten (nur unter der eigenen)',
    kick: 'Mitglieder entfernen',
    ban: 'Mitglieder bannen',
    deleteMessages: 'Nachrichten anderer löschen',
    invite: 'Einladungen erstellen',
    reports: 'Meldungen sehen und bearbeiten',
    log: 'Protokoll sehen',
  };
}

/// A community this account is in.
class CommunitySummary {
  const CommunitySummary({required this.id, required this.name, required this.owner});

  final String id;
  final String name;
  final String owner;
}

class CRole {
  const CRole({required this.id, required this.name, required this.color, required this.position, required this.perms});

  factory CRole.fromJson(Map<String, Object?> j) => CRole(
    id: (j['id'] as num).toInt(),
    name: j['name'] as String? ?? '',
    color: Color((j['color'] as num?)?.toInt() ?? 0xFF9A98AA),
    position: (j['position'] as num?)?.toInt() ?? 0,
    perms: (j['perms'] as num?)?.toInt() ?? 0,
  );

  /// 0 is "@alle".
  final int id;
  final String name;
  final Color color;
  final int position;
  final int perms;

  bool get everyone => id == 0;
}

class CChannel {
  const CChannel({
    required this.id,
    required this.category,
    required this.name,
    required this.topic,
    required this.position,
    required this.readRoles,
    required this.writeRoles,
    required this.canWrite,
  });

  factory CChannel.fromJson(Map<String, Object?> j) => CChannel(
    id: j['id'] as String,
    category: j['category'] as String? ?? '',
    name: j['name'] as String? ?? '',
    topic: j['topic'] as String? ?? '',
    position: (j['position'] as num?)?.toInt() ?? 0,
    readRoles: (j['read_roles'] as List?)?.map((e) => (e as num).toInt()).toList(),
    writeRoles: (j['write_roles'] as List?)?.map((e) => (e as num).toInt()).toList(),
    canWrite: j['can_write'] == true,
  );

  final String id;
  final String category;
  final String name;
  final String topic;
  final int position;

  /// null: everyone; else these roles (and who manages channels).
  final List<int>? readRoles;
  final List<int>? writeRoles;

  /// This account may write here now (rights and accepted rules).
  final bool canWrite;

  /// Everyone reads, only some write (announcements, rules).
  bool get readOnly => writeRoles != null && !writeRoles!.contains(0);

  /// Only some roles see it.
  bool get limited => readRoles != null && !readRoles!.contains(0);
}

class CMember {
  const CMember({required this.user, required this.roles, required this.owner, required this.power});

  factory CMember.fromJson(Map<String, Object?> j) => CMember(
    user: j['user'] as String,
    roles: [for (final r in (j['roles'] as List? ?? const [])) (r as num).toInt()],
    owner: j['owner'] == true,
    power: (j['power'] as num?)?.toInt() ?? 0,
  );

  final String user;
  final List<int> roles;
  final bool owner;
  final int power;
}

/// One community as this account sees it.
class CommunityInfo {
  const CommunityInfo({
    required this.id,
    required this.name,
    required this.about,
    required this.rules,
    required this.rulesRev,
    required this.owner,
    required this.myPerms,
    required this.myPower,
    required this.iAmOwner,
    required this.rulesOk,
    required this.roles,
    required this.channels,
    required this.members,
  });

  factory CommunityInfo.fromJson(Map<String, Object?> j) {
    final c = (j['community'] as Map).cast<String, Object?>();
    final me = (j['me'] as Map).cast<String, Object?>();
    return CommunityInfo(
      id: c['id'] as String,
      name: c['name'] as String? ?? '',
      about: c['about'] as String? ?? '',
      rules: c['rules'] as String? ?? '',
      rulesRev: (c['rules_rev'] as num?)?.toInt() ?? 0,
      owner: c['owner'] as String? ?? '',
      myPerms: (me['perms'] as num?)?.toInt() ?? 0,
      myPower: (me['power'] as num?)?.toInt() ?? 0,
      iAmOwner: me['owner'] == true,
      rulesOk: me['rules_ok'] == true,
      roles: [for (final r in (j['roles'] as List? ?? const [])) CRole.fromJson((r as Map).cast<String, Object?>())],
      channels: [
        for (final r in (j['channels'] as List? ?? const [])) CChannel.fromJson((r as Map).cast<String, Object?>()),
      ],
      members: [
        for (final r in (j['members'] as List? ?? const [])) CMember.fromJson((r as Map).cast<String, Object?>()),
      ],
    );
  }

  final String id;
  final String name;
  final String about;
  final String rules;
  final int rulesRev;
  final String owner;
  final int myPerms;
  final int myPower;
  final bool iAmOwner;
  final bool rulesOk;

  /// Highest first; "@alle" last.
  final List<CRole> roles;
  final List<CChannel> channels;
  final List<CMember> members;

  bool can(int perm) => myPerms & perm == perm;

  /// Anything to manage at all (shows the settings button).
  bool get manages => myPerms != 0 || iAmOwner;

  CRole? role(int id) => roles.where((r) => r.id == id).firstOrNull;

  /// The highest role of a member (not "@alle").
  CRole? topRole(CMember m) {
    CRole? top;
    for (final id in m.roles) {
      final r = role(id);
      if (r != null && (top == null || r.position > top.position)) top = r;
    }
    return top;
  }

  CMember? member(String user) => members.where((m) => m.user == user).firstOrNull;

  /// May this account act on [user] (kick, ban, roles, delete messages)?
  bool above(String user) {
    final m = member(user);
    if (m == null) return iAmOwner || myPower > 0;
    return !m.owner && myPower > m.power;
  }

  /// Roles this account may give or take: below its own height.
  List<CRole> get assignable => [
    for (final r in roles)
      if (!r.everyone && (iAmOwner || r.position < myPower)) r,
  ];

  /// Channels grouped by category, in order.
  List<(String, List<CChannel>)> get categories {
    final out = <(String, List<CChannel>)>[];
    for (final ch in channels) {
      final i = out.indexWhere((c) => c.$1 == ch.category);
      if (i < 0) {
        out.add((ch.category, [ch]));
      } else {
        out[i].$2.add(ch);
      }
    }
    return out;
  }
}

/// What the signature check says about an open message.
enum Signed { pending, own, verified, contact, known, fresh, changed, bad }

Signed _signed(String word) => switch (word) {
  'own' => Signed.own,
  'verified' => Signed.verified,
  'contact' => Signed.contact,
  'known' => Signed.known,
  'new' => Signed.fresh,
  'changed' => Signed.changed,
  _ => Signed.bad,
};

class OpenMessage {
  OpenMessage({
    required this.id,
    required this.user,
    required this.device,
    required this.text,
    required this.sent,
    required this.at,
    required this.sig,
    this.signed = Signed.pending,
  });

  factory OpenMessage.fromJson(Map<String, Object?> j) => OpenMessage(
    id: (j['id'] as num).toInt(),
    user: j['user'] as String,
    device: j['device'] as String? ?? '',
    text: j['text'] as String? ?? '',
    sent: (j['sent'] as num?)?.toInt() ?? 0,
    at: (j['at'] as num?)?.toInt() ?? 0,
    sig: j['sig'] as String? ?? '',
  );

  final int id;
  final String user;
  final String device;
  final String text;
  final int sent;
  final int at;
  final String sig;
  Signed signed;

  Map<String, Object?> toCheck() => {'user': user, 'device': device, 'text': text, 'sent': sent, 'sig': sig};
}

class CReport {
  const CReport({
    required this.id,
    required this.reporter,
    required this.target,
    required this.reason,
    required this.snapshot,
    required this.at,
    required this.status,
    required this.handledBy,
    required this.note,
  });

  factory CReport.fromJson(Map<String, Object?> j) => CReport(
    id: (j['id'] as num).toInt(),
    reporter: j['reporter'] as String? ?? '',
    target: j['target'] as String?,
    reason: j['reason'] as String? ?? '',
    snapshot: j['snapshot'],
    at: (j['at'] as num?)?.toInt() ?? 0,
    status: j['status'] as String? ?? 'open',
    handledBy: j['handled_by'] as String?,
    note: j['note'] as String? ?? '',
  );

  final int id;
  final String reporter;
  final String? target;
  final String reason;

  /// What was reported, as it was then: `{user, channel, text, sent}` for
  /// a message, `{forwarded: […]}` for messages a reporter attached.
  final Object? snapshot;
  final int at;

  /// open, done or dismissed.
  final String status;
  final String? handledBy;
  final String note;

  /// The reported text, if any.
  String get quoted {
    final s = snapshot;
    if (s is Map && s['text'] is String) return s['text'] as String;
    if (s is Map && s['forwarded'] is List) {
      return [
        for (final f in s['forwarded'] as List)
          if (f is Map) '${f['from'] ?? ''}: ${f['text'] ?? ''}',
      ].join('\n');
    }
    return '';
  }
}

class CLogEntry {
  const CLogEntry({
    required this.actor,
    required this.action,
    required this.target,
    required this.detail,
    required this.at,
  });

  factory CLogEntry.fromJson(Map<String, Object?> j) => CLogEntry(
    actor: j['actor'] as String? ?? '',
    action: j['action'] as String? ?? '',
    target: j['target'] as String? ?? '',
    detail: j['detail'] as String? ?? '',
    at: (j['at'] as num?)?.toInt() ?? 0,
  );

  final String actor;
  final String action;
  final String target;
  final String detail;
  final int at;
}

class CInvite {
  const CInvite({required this.id, required this.creator, required this.uses, required this.ends});

  factory CInvite.fromJson(Map<String, Object?> j) => CInvite(
    id: j['id'] as String,
    creator: j['creator'] as String? ?? '',
    uses: (j['uses'] as num?)?.toInt(),
    ends: (j['ends'] as num?)?.toInt() ?? 0,
  );

  final String id;
  final String creator;

  /// Uses left; null: unlimited until it ends.
  final int? uses;
  final int ends;
}

class CBan {
  const CBan({required this.user, required this.by, required this.reason, required this.at});

  factory CBan.fromJson(Map<String, Object?> j) => CBan(
    user: j['user'] as String,
    by: j['by'] as String? ?? '',
    reason: j['reason'] as String? ?? '',
    at: (j['at'] as num?)?.toInt() ?? 0,
  );

  final String user;
  final String by;
  final String reason;
  final int at;
}

/// A server answer as a sentence for the person.
String communityErrorText(String code) => switch (code) {
  'not_allowed' => 'Dafür fehlt dir das Recht.',
  'accept_rules' => 'Bitte erst die Regeln akzeptieren.',
  'rules_changed' => 'Die Regeln wurden gerade geändert. Bitte noch einmal lesen.',
  'bad_invite' => 'Diesen Einladungscode gibt es nicht.',
  'invite_expired' => 'Die Einladung ist abgelaufen oder aufgebraucht.',
  'banned' => 'Du bist aus dieser Community gebannt.',
  'slow_down' => 'Etwas langsamer, bitte. Gleich geht es wieder.',
  'hand_over_first' => 'Übergib die Community erst an jemand anderen.',
  'bad_name' => 'Der Name passt nicht (2 bis 32 Zeichen, keine offiziell wirkenden Namen).',
  'bad_text' => 'Der Text passt nicht (zu lang oder leer).',
  'too_many' => 'Das Limit ist erreicht.',
  'full' => 'Diese Community ist voll.',
  'clock' => 'Die Uhr dieses PCs geht falsch. Bitte Uhrzeit prüfen.',
  'bad_signature' => 'Die Unterschrift wurde nicht angenommen.',
  'reason_needed' => 'Bitte einen Grund angeben.',
  'not_found' => 'Gibt es nicht mehr.',
  'unknown_role' => 'Diese Rolle gibt es nicht.',
  'offline' => 'Server nicht erreichbar.',
  _ => 'Das ging nicht ($code).',
};

/// Communities of this account: the list, what is open, and the live
/// connection to the server. Lives inside the app state; [_notify] makes
/// the screens redraw.
class Communities {
  Communities(this._notify, {this.onGone});

  final void Function() _notify;

  /// The open community is gone (kicked, banned, deleted).
  final void Function(String name)? onGone;

  ServerApi? _api;
  TrairVault? _vault;
  String _me = '';
  int _seq = 0;
  int _run = 0;

  List<CommunitySummary> list = const [];
  final Map<String, CommunityInfo> infos = {};

  /// The community shown; null: the demo community.
  String? active;

  /// The channel shown per community.
  final Map<String, String> channelOf = {};

  /// Loaded messages per `community/channel`, oldest first.
  final Map<String, List<OpenMessage>> messages = {};

  /// Last problem or note, shown on the community screen.
  String message = '';

  /// Loaded on demand for the settings.
  List<CReport>? reports;
  List<CLogEntry>? log;
  List<CInvite>? invites;
  List<CBan>? bans;

  /// The last invitation code made here (shown once, to copy).
  String? newInvite;

  /// Team inbox (only with a valid team badge).
  List<CReport>? teamReports;
  Object? _teamBadge;

  final Map<String, Map<String, Object?>> _keys = {};

  bool get online => _api != null;
  String get me => _me;
  CommunityInfo? get current => active == null ? null : infos[active];
  String? get currentChannel => active == null ? null : channelOf[active];

  List<OpenMessage> messagesOf(String community, String channel) => messages['$community/$channel'] ?? const [];

  /// Start using the server: list, then follow changes.
  Future<void> attach(ServerApi api, TrairVault vault, String user) async {
    _api = api;
    _vault = vault;
    _me = user;
    final run = ++_run;
    final start = await api.communitySync(-1, 0);
    if (start.ok) _seq = (start.body['seq'] as num?)?.toInt() ?? 0;
    await refreshList();
    final badge = await vault.teamBadge();
    _teamBadge = badge.ok ? badge.raw['badge'] : null;
    if (run == _run) unawaited(_syncLoop(api, run));
  }

  /// Stop (locked, disconnected). Nothing of it stays in memory.
  void detach() {
    _run++;
    _api = null;
    _vault = null;
    list = const [];
    infos.clear();
    messages.clear();
    channelOf.clear();
    _keys.clear();
    active = null;
    reports = log = invites = bans = null;
    teamReports = null;
    _teamBadge = null;
    newInvite = null;
    message = '';
  }

  bool get hasTeamBadge => _teamBadge != null;

  Future<ServerReply> _op(Map<String, Object?> body) async {
    final api = _api;
    if (api == null) return const ServerReply(0, {'error': 'offline'});
    final r = await api.community(body);
    if (!r.ok) message = communityErrorText(r.error);
    return r;
  }

  Future<void> refreshList() async {
    final r = await _op({'op': 'list'});
    if (!r.ok) return _notify();
    list = [
      for (final c in (r.body['communities'] as List? ?? const []).cast<Map<String, Object?>>())
        CommunitySummary(id: c['id'] as String, name: c['name'] as String? ?? '', owner: c['owner'] as String? ?? ''),
    ];
    _notify();
  }

  /// Show a real community (or, with null, the demo).
  Future<void> open(String? id) async {
    active = id;
    message = '';
    reports = log = invites = bans = null;
    newInvite = null;
    _notify();
    if (id == null) return;
    await loadInfo(id);
    final info = infos[id];
    if (info == null) return;
    final chosen = channelOf[id];
    final ch = info.channels.any((c) => c.id == chosen)
        ? chosen!
        : (info.channels.where((c) => c.name == 'allgemein').firstOrNull ?? info.channels.firstOrNull)?.id;
    if (ch != null) await openChannel(id, ch);
  }

  Future<void> loadInfo(String id) async {
    final r = await _op({'op': 'info', 'community': id});
    if (r.ok) {
      infos[id] = CommunityInfo.fromJson(r.body);
    } else if (r.error == 'not_allowed' || r.error == 'not_found') {
      _gone(id);
      return;
    }
    _notify();
  }

  Future<void> openChannel(String id, String channel) async {
    channelOf[id] = channel;
    _notify();
    await loadMessages(id, channel);
  }

  Future<void> loadMessages(String id, String channel) async {
    final r = await _op({'op': 'messages', 'community': id, 'channel': channel, 'limit': 100});
    if (!r.ok) return _notify();
    final list = [
      for (final m in (r.body['messages'] as List? ?? const []).cast<Map<String, Object?>>()) OpenMessage.fromJson(m),
    ];
    await _check(id, channel, list);
    messages['$id/$channel'] = list;
    _notify();
  }

  /// Let the core check signatures and keys. Authors' keys come from the
  /// server once per session; a bad result fetches them once more (a new
  /// device of theirs).
  Future<void> _check(String id, String channel, List<OpenMessage> list) async {
    final api = _api;
    final vault = _vault;
    if (api == null || vault == null || list.isEmpty) return;
    Future<void> fetch(Iterable<String> users) async {
      for (final u in users) {
        final a = await api.account(u);
        if (a.ok) _keys[u] = {'account_key': a.body['account_key'], 'device_list': a.body['device_list']};
      }
    }

    final fresh = {for (final m in list) m.user}.where((u) => !_keys.containsKey(u)).toSet();
    await fetch(fresh);
    Future<void> run(List<OpenMessage> part) async {
      final users = {for (final m in part) m.user};
      final r = await vault.checkOpenMessages(
        id,
        channel,
        [for (final m in part) m.toCheck()],
        {
          for (final u in users)
            if (_keys[u] != null) u: _keys[u],
        },
      );
      final results = (r.raw['results'] as List? ?? const []).cast<String>();
      for (var i = 0; i < part.length && i < results.length; i++) {
        part[i].signed = _signed(results[i]);
      }
    }

    await run(list);
    final doubtful = [
      for (final m in list)
        if ((m.signed == Signed.bad || m.signed == Signed.changed) && !fresh.contains(m.user)) m,
    ];
    if (doubtful.isNotEmpty) {
      await fetch({for (final m in doubtful) m.user});
      await run(doubtful);
    }
  }

  /// Write in the open channel; the device signs it.
  Future<bool> post(String text) async {
    final id = active;
    final ch = currentChannel;
    final vault = _vault;
    final t = text.trim();
    if (id == null || ch == null || vault == null || t.isEmpty) return false;
    final sent = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final s = await vault.signOpenMessage(id, ch, t, sent);
    if (!s.ok) {
      message = 'Unterschreiben ging nicht (${s.error}).';
      _notify();
      return false;
    }
    final r = await _op({
      'op': 'post',
      'community': id,
      'channel': ch,
      'text': t,
      'sent': sent,
      'sig': s.raw['signature'],
    });
    if (!r.ok) {
      if (r.error == 'accept_rules') await loadInfo(id);
      _notify();
      return false;
    }
    final mid = (r.body['id'] as num).toInt();
    final list = messages.putIfAbsent('$id/$ch', () => []);
    if (!list.any((m) => m.id == mid)) {
      list.add(
        OpenMessage(
          id: mid,
          user: _me,
          device: s.raw['device'] as String? ?? '',
          text: t,
          sent: sent,
          at: (r.body['at'] as num?)?.toInt() ?? sent,
          sig: s.raw['signature'] as String,
          signed: Signed.own,
        ),
      );
    }
    message = '';
    _notify();
    return true;
  }

  Future<bool> deleteMessage(int messageId) async {
    final id = active;
    if (id == null) return false;
    final r = await _op({'op': 'delete_message', 'community': id, 'id': messageId});
    if (r.ok) {
      for (final l in messages.values) {
        l.removeWhere((m) => m.id == messageId);
      }
    }
    _notify();
    return r.ok;
  }

  // ----------------------------------------------------- joining and owning

  Future<String?> create(String name) async {
    final r = await _op({'op': 'create', 'name': name.trim()});
    if (!r.ok) {
      _notify();
      return null;
    }
    await refreshList();
    final id = r.body['community'] as String;
    await open(id);
    return id;
  }

  /// Join with a `trairc:` code.
  Future<String?> join(String code) async {
    final r = await _op({'op': 'join', 'code': code.trim()});
    if (!r.ok) {
      _notify();
      return null;
    }
    await refreshList();
    final id = r.body['community'] as String;
    await open(id);
    return id;
  }

  Future<bool> acceptRules() async {
    final info = current;
    if (info == null) return false;
    final r = await _op({'op': 'accept_rules', 'community': info.id, 'rev': info.rulesRev});
    await loadInfo(info.id);
    return r.ok;
  }

  Future<bool> leave() async {
    final id = active;
    if (id == null) return false;
    final r = await _op({'op': 'leave', 'community': id});
    if (r.ok) _drop(id);
    _notify();
    return r.ok;
  }

  Future<bool> deleteCommunity() async {
    final id = active;
    if (id == null) return false;
    final r = await _op({'op': 'delete', 'community': id});
    if (r.ok) _drop(id);
    _notify();
    return r.ok;
  }

  Future<bool> transfer(String user) => _change({'op': 'transfer', 'user': user});

  Future<bool> edit({required String name, required String about, required String rules}) =>
      _change({'op': 'edit', 'name': name, 'about': about, 'rules': rules});

  // ------------------------------------------------------------- moderation

  /// A change in the open community, then its new state.
  Future<bool> _change(Map<String, Object?> body) async {
    final id = active;
    if (id == null) return false;
    final r = await _op({...body, 'community': id});
    if (r.ok) message = '';
    await loadInfo(id);
    return r.ok;
  }

  Future<bool> kick(String user, String reason) => _change({'op': 'kick', 'user': user, 'reason': reason});

  Future<bool> ban(String user, String reason) async {
    final ok = await _change({'op': 'ban', 'user': user, 'reason': reason});
    if (ok && bans != null) await loadBans();
    return ok;
  }

  Future<bool> unban(String user) async {
    final ok = await _change({'op': 'unban', 'user': user});
    if (ok) await loadBans();
    return ok;
  }

  Future<bool> setRoles(String user, List<int> roles) => _change({'op': 'set_roles', 'user': user, 'roles': roles});

  Future<bool> createRole(String name, Color color, int perms) =>
      _change({'op': 'create_role', 'name': name, 'color': color.toARGB32(), 'perms': perms});

  Future<bool> editRole(CRole role, {String? name, Color? color, int? position, int? perms}) => _change({
    'op': 'edit_role',
    'role': role.id,
    'name': name ?? role.name,
    'color': (color ?? role.color).toARGB32(),
    'position': position ?? role.position,
    'perms': perms ?? role.perms,
  });

  Future<bool> deleteRole(int role) => _change({'op': 'delete_role', 'role': role});

  Future<bool> createChannel(String name, String category, {String topic = '', List<int>? read, List<int>? write}) =>
      _change({
        'op': 'create_channel',
        'name': name,
        'category': category,
        'topic': topic,
        'read_roles': read,
        'write_roles': write,
      });

  Future<bool> editChannel(
    CChannel ch, {
    String? name,
    String? category,
    String? topic,
    int? position,
    List<int>? read,
    List<int>? write,
    bool keepAccess = true,
  }) => _change({
    'op': 'edit_channel',
    'channel': ch.id,
    'name': name ?? ch.name,
    'category': category ?? ch.category,
    'topic': topic ?? ch.topic,
    'position': position ?? ch.position,
    'read_roles': keepAccess ? ch.readRoles : read,
    'write_roles': keepAccess ? ch.writeRoles : write,
  });

  Future<bool> deleteChannel(String channel) async {
    final id = active;
    final ok = await _change({'op': 'delete_channel', 'channel': channel});
    if (ok && id != null) {
      messages.remove('$id/$channel');
      if (channelOf[id] == channel) await open(id);
    }
    return ok;
  }

  Future<String?> createInvite(int minutes, int? uses) async {
    final id = active;
    if (id == null) return null;
    final r = await _op({'op': 'create_invite', 'community': id, 'minutes': minutes, 'uses': uses});
    newInvite = r.ok ? r.body['code'] as String : null;
    if (r.ok && invites != null) await loadInvites();
    _notify();
    return newInvite;
  }

  Future<void> loadInvites() async {
    final id = active;
    if (id == null) return;
    final r = await _op({'op': 'invites', 'community': id});
    if (r.ok) {
      invites = [
        for (final j in (r.body['invites'] as List? ?? const []).cast<Map<String, Object?>>()) CInvite.fromJson(j),
      ];
    }
    _notify();
  }

  Future<void> revokeInvite(String invite) async {
    final id = active;
    if (id == null) return;
    await _op({'op': 'revoke_invite', 'community': id, 'id': invite});
    await loadInvites();
  }

  Future<void> loadBans() async {
    final id = active;
    if (id == null) return;
    final r = await _op({'op': 'bans', 'community': id});
    if (r.ok) {
      bans = [for (final j in (r.body['bans'] as List? ?? const []).cast<Map<String, Object?>>()) CBan.fromJson(j)];
    }
    _notify();
  }

  Future<void> loadReports({bool all = false}) async {
    final id = active;
    if (id == null) return;
    final r = await _op({'op': 'reports', 'community': id, 'all': all});
    if (r.ok) {
      reports = [
        for (final j in (r.body['reports'] as List? ?? const []).cast<Map<String, Object?>>()) CReport.fromJson(j),
      ];
    }
    _notify();
  }

  Future<void> handleReport(int report, String status, {String note = ''}) async {
    final id = active;
    if (id == null) return;
    await _op({'op': 'handle_report', 'community': id, 'id': report, 'status': status, 'note': note});
    await loadReports();
  }

  Future<void> loadLog() async {
    final id = active;
    if (id == null) return;
    final r = await _op({'op': 'log', 'community': id});
    if (r.ok) {
      log = [for (final j in (r.body['log'] as List? ?? const []).cast<Map<String, Object?>>()) CLogEntry.fromJson(j)];
    }
    _notify();
  }

  /// Report a message of the open community to its moderators.
  Future<bool> reportMessage(int messageId, String reason) async {
    final id = active;
    if (id == null) return false;
    final r = await _op({'op': 'report', 'community': id, 'message': messageId, 'reason': reason});
    if (r.ok) message = 'Gemeldet. Die Moderation der Community sieht es jetzt.';
    _notify();
    return r.ok;
  }

  /// Report a member of the open community to its moderators.
  Future<bool> reportMember(String user, String reason) async {
    final id = active;
    if (id == null) return false;
    final r = await _op({'op': 'report', 'community': id, 'user': user, 'reason': reason});
    if (r.ok) message = 'Gemeldet. Die Moderation der Community sieht es jetzt.';
    _notify();
    return r.ok;
  }

  /// Report a person to the Trair team, optionally with messages the
  /// reporter chose to forward (they are no longer end-to-end encrypted
  /// then: the team has to read them).
  Future<bool> reportToTeam(String user, String reason, {List<Map<String, Object?>>? evidence}) async {
    final r = await _op({
      'op': 'report',
      'community': null,
      'user': user,
      'reason': reason,
      'team': true,
      'evidence': ?evidence,
    });
    _notify();
    return r.ok;
  }

  Future<void> loadTeamReports({bool all = false}) async {
    final badge = _teamBadge;
    if (badge == null) return;
    final r = await _op({'op': 'team_reports', 'badge': badge, 'all': all});
    if (r.ok) {
      teamReports = [
        for (final j in (r.body['reports'] as List? ?? const []).cast<Map<String, Object?>>()) CReport.fromJson(j),
      ];
    }
    _notify();
  }

  Future<void> teamHandle(int report, String status, {String note = ''}) async {
    final badge = _teamBadge;
    if (badge == null) return;
    await _op({'op': 'team_handle', 'badge': badge, 'id': report, 'status': status, 'note': note});
    await loadTeamReports();
  }

  /// Take a new key for someone who is not a contact (after a warning).
  Future<void> acceptKey(String user) async {
    final k = _keys[user];
    final vault = _vault;
    if (k == null || vault == null) return;
    await vault.acceptOpenKey(user, k['account_key'] as String);
    final id = active;
    final ch = currentChannel;
    if (id != null && ch != null) await loadMessages(id, ch);
  }

  // ------------------------------------------------------------ live

  void _drop(String id) {
    list = [
      for (final c in list)
        if (c.id != id) c,
    ];
    infos.remove(id);
    messages.removeWhere((k, _) => k.startsWith('$id/'));
    channelOf.remove(id);
    if (active == id) active = null;
  }

  void _gone(String id) {
    final name = list.where((c) => c.id == id).firstOrNull?.name ?? infos[id]?.name ?? 'Community';
    final wasActive = active == id;
    _drop(id);
    message = 'Du bist nicht mehr in „$name“.';
    _notify();
    if (wasActive) onGone?.call(name);
  }

  Future<void> _syncLoop(ServerApi api, int run) async {
    var failures = 0;
    while (run == _run && identical(api, _api)) {
      final r = await api.communitySync(_seq, 25);
      if (run != _run || !identical(api, _api)) return;
      if (!r.ok) {
        if (++failures >= 3) return;
        await Future<void>.delayed(const Duration(seconds: 2));
        continue;
      }
      failures = 0;
      _seq = (r.body['seq'] as num?)?.toInt() ?? _seq;
      final reload = <String>{};
      var listChanged = false;
      for (final e in (r.body['events'] as List? ?? const []).cast<Map<String, Object?>>()) {
        final id = e['community'] as String;
        final ch = e['channel'] as String? ?? '';
        switch (e['kind']) {
          case 'message':
            final key = '$id/$ch';
            final loaded = messages[key];
            final m = e['message'];
            if (loaded != null && m is Map) {
              final msg = OpenMessage.fromJson(m.cast<String, Object?>());
              if (!loaded.any((x) => x.id == msg.id)) {
                await _check(id, ch, [msg]);
                if (run != _run) return;
                loaded.add(msg);
              }
            }
          case 'deleted':
            messages['$id/$ch']?.removeWhere((x) => x.id == (e['ref'] as num).toInt());
          case 'removed' when e['target'] == _me:
            _gone(id);
            listChanged = true;
          case 'changed' || 'joined' || 'left' || 'removed':
            reload.add(id);
            listChanged = true;
          case 'report':
            if (active == id && reports != null) reload.add('reports');
        }
      }
      if (listChanged) await refreshList();
      for (final id in reload) {
        if (id == 'reports') {
          await loadReports();
        } else if (infos.containsKey(id)) {
          await loadInfo(id);
        }
      }
      _notify();
    }
  }
}
