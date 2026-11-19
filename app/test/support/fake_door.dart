// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:convert';

import 'package:trair_app/core/door.dart';

/// Stands in for the Rust core in widget tests. Follows the protocol of
/// crates/trair-ffi/src/api.rs, without encryption and without waiting.
/// The real core has its own tests (crates/trair-ffi/tests/tuer.rs) and
/// test/door_native_test.dart runs the app's door against it.
class FakeDoor implements CoreDoor {
  FakeDoor({this.password, Map<String, Object?>? profile, List<String> otherDevices = const []})
    : profile = profile ?? {},
      devices = [
        _device(thisId, 'Dieser PC', 0, isThis: true),
        for (var i = 0; i < otherDevices.length; i++) _device('d-0000000${i + 1}', otherDevices[i], i + 1),
      ];

  static const thisId = 'd-1a2b3c4d';

  static Map<String, Object?> _device(String id, String? label, int n, {bool isThis = false}) => {
    'id': id,
    'label': label,
    'fingerprint': '${n}c0d e1f2 0a1b 2c3d 4e5f 6071 8293 a4b5',
    'this': isThis,
  };

  /// The account's devices, as the core reports them.
  final List<Map<String, Object?>> devices;

  /// Own badges the fake core accepted.
  final List<Map<String, Object?>> badges = [];

  /// A made-up public account key (64 hex characters).
  static final accountKey = 'ab' * 32;

  /// Null: there is no vault yet (first start).
  String? password;
  Map<String, Object?> profile;
  bool unlocked = false;
  int failures = 0;

  /// Every request, without the passwords.
  final List<String> ops = [];

  /// The server's list says this device was withdrawn.
  bool withdrawnElsewhere = false;

  // ----- emergency PINs -----

  /// The PINs with what they do (the real core never shows a PIN).
  final List<Map<String, Object?>> notfall = [];
  Map<String, Object?>? _running;
  int _pinIds = 0;

  /// Everything goes, like crates/trair-ffi/src/api.rs wipe_account.
  void _wipe(String after) {
    // "wrong_password": another vault stands there that nobody can open.
    password = after == 'wrong_password' ? '\u0000niemand' : null;
    profile = {};
    unlocked = false;
    notfall.clear();
    contacts.clear();
    chats.clear();
    messages.clear();
    pending.clear();
    groups.clear();
    _running = null;
  }

  Map<String, Object?> _pins() => {
    'ok': true,
    'pins': [
      for (final p in notfall)
        {
          'id': p['id'],
          'after': p['after'],
          'chat': p['chat'],
          'chat_name': p['chat'] == null ? null : _chat(p['chat'])?['name'],
          'text': p['text'],
        },
    ],
    'max': 3,
  };

  Map<String, Object?> _notfallOp(String op, Map<String, Object?> r) {
    switch (op) {
      case 'notfall_sign_login' || 'notfall_sign_deletion':
        return _running == null ? _fail('no_session') : {'ok': true, 'signature': 'notfall'};
      case 'notfall_finish':
        final running = _running;
        if (running == null) return _fail('no_session');
        _wipe(running['after'] as String);
        return {'ok': true};
    }
    if (!unlocked) return _fail('locked');
    if (r['password'] != password) return _wrong();
    switch (op) {
      case 'notfall_list':
        return _pins();
      case 'notfall_remove':
        notfall.removeWhere((p) => p['id'] == r['id']);
        return _pins();
      case 'notfall_add':
        final pin = r['pin'] as String;
        final chat = r['chat'] as String?;
        if (pin.runes.length < (chat == null ? 6 : 12)) return _fail(chat == null ? 'weak_pin' : 'weak_sending_pin');
        if (pin == password) return _fail('pin_is_password');
        if (chat != null && _chat(chat)?['state'] != 'active') return _fail('unknown_chat');
        if (notfall.any((p) => p['pin'] == pin)) return _fail('pin_taken');
        if (notfall.length >= 3) return _fail('full');
        notfall.add({'id': 'p${++_pinIds}', 'pin': pin, 'after': r['after'], 'chat': chat, 'text': r['text']});
        return _pins();
    }
    return _fail('bad_request');
  }

  // ----- chats (M4): a fake core without encryption -----

  /// The chat database opens with the vault.
  bool chatsEnabled = false;
  final List<Map<String, Object?>> contacts = [];
  final List<Map<String, Object?>> chats = [];
  final Map<String, List<Map<String, Object?>>> messages = {};
  final Map<String, Map<String, Object?>> pending = {};
  int _ids = 0;

  /// What a fake peer sends: base64 of JSON, read by `receive`.
  static String wire(Map<String, Object?> m) => base64.encode(utf8.encode(jsonEncode(m)));
  static Map<String, Object?> unwire(String b) => jsonDecode(utf8.decode(base64.decode(b))) as Map<String, Object?>;

  // ----- profiles to contacts -----

  // ----- friendship keys, vanishing chats -----

  /// Keys made here: minutes and whether the chat vanishes.
  final List<Map<String, Object?>> invites = [];

  /// Accounts whose key was used here; the token goes into the first chat.
  final Set<Object?> redeem = {};

  static int get _now => DateTime.now().millisecondsSinceEpoch ~/ 1000;

  /// Profiles of other accounts, as the core's `profiles` answers.
  final List<Map<String, Object?>> peerProfiles = [];

  /// The own profile as the app last shared it.
  Map<String, Object?>? shared;

  /// Version each chat already has.
  final Map<String, int> profileSent = {};

  Map<String, Object?>? _chat(Object? id) => chats.where((c) => c['id'] == id).firstOrNull;
  bool _isContact(Object? user) => contacts.any((c) => c['user'] == user);

  Map<String, Object?> _chatOp(String op, Map<String, Object?> r) {
    if (!unlocked) return _fail('locked');
    if (!chatsEnabled) return _fail('chats_unavailable');
    switch (op) {
      case 'contact_code':
        return {'ok': true, 'code': 'trair1:local-julian:${'a1' * 16}:beef'};
      case 'add_contact':
        final parts = (r['code'] as String).trim().split(':');
        final key = parts.length == 6 && parts[0] == 'trair2';
        if (!(key || (parts.length == 4 && parts[0] == 'trair1')) || parts[1] == 'local-julian') {
          return _fail('rejected');
        }
        if (key) redeem.add(parts[1]);
        if (!_isContact(parts[1])) {
          final name = (r['name'] as String).trim();
          contacts.add({
            'user': parts[1],
            'name': name.isEmpty ? parts[1] : name,
            'fingerprint': parts[2],
            'confirmed': false,
          });
        }
        for (final c in chats.where((c) => c['peer'] == parts[1])) {
          c['state'] = 'active';
        }
        return {'ok': true, 'contact': contacts.firstWhere((c) => c['user'] == parts[1])};
      case 'contacts':
        return {
          'ok': true,
          'contacts': [for (final c in contacts) Map.of(c)],
        };
      case 'chats':
        return {
          'ok': true,
          'chats': [for (final c in chats) Map.of(c)],
        };
      case 'messages':
        return {
          'ok': true,
          'messages': [for (final m in messages[r['chat']] ?? const <Map<String, Object?>>[]) Map.of(m)],
        };
      case 'start_chat':
        final user = r['user'];
        if (!_isContact(user)) return _fail('rejected');
        final offers = (r['offers'] as List).cast<Map<String, Object?>>();
        final theirs = [
          for (final o in offers)
            if ((o['device'] as String).startsWith('$user:')) o['device'],
        ];
        if (theirs.isEmpty) return _fail('bad_package');
        final id = 'c${chats.length + 1}';
        final name = contacts.firstWhere((c) => c['user'] == user)['name'];
        chats.add({'id': id, 'peer': user, 'name': name, 'state': 'active', 'last': 0});
        final commit = {
          'chat': id,
          'epoch': 0,
          'commit': 'Y29tbWl0',
          'to': <String>[],
          'welcome': 'd2VsY29tZQ==',
          'welcome_to': theirs,
          'removed': <String>[],
          'hash': 'h-$id',
        };
        pending[id] = commit;
        return {'ok': true, 'chat': Map.of(chats.last), 'commit': commit};
      case 'pending_commits':
        return {'ok': true, 'commits': pending.values.toList()};
      case 'commit_answer':
        final c = pending.remove(r['chat']);
        if (c == null) return _fail('rejected');
        return {'ok': true, 'applied': r['accepted'] == true || r['taken'] == c['hash']};
      case 'send_message':
        final chat = _chat(r['chat']);
        if (chat == null) return _fail('unknown_chat');
        if (chat['state'] != 'active') return _fail('blocked');
        final m = {'id': ++_ids, 'chat': chat['id'], 'text': r['text'], 'outgoing': true, 'at': 0, 'missing': 0};
        (messages[chat['id'] as String] ??= []).add(m);
        return {
          'ok': true,
          'body': wire({'kind': 'message', 'chat': chat['id'], 'text': r['text']}),
          'to': ['${chat['peer']}:d-peer'],
          'message': m,
        };
      case 'receive':
        final Map<String, Object?> w;
        try {
          w = unwire(r['body'] as String);
        } on FormatException {
          return _fail('rejected');
        }
        final grouped = _groupReceive(w);
        if (grouped != null) return grouped;
        if (w['kind'] == 'ends') {
          final chat = _chat(w['chat']);
          if (chat == null) return _fail('unknown_chat');
          chat['ends'] = w['ends'];
          return {
            'ok': true,
            'incoming': {'kind': 'ends', 'chat': chat['id'], 'ends': chat['ends']},
          };
        }
        if (w['kind'] == 'profile') {
          final user = w['user'] as String;
          peerProfiles
            ..removeWhere((p) => p['user'] == user)
            ..add({...(w['profile'] as Map).cast<String, Object?>(), 'user': user});
          return {
            'ok': true,
            'incoming': {'kind': 'profile', 'user': user},
          };
        }
        if (w['kind'] == 'chat') {
          final peer = w['peer'];
          chats.add({
            'id': w['chat'],
            'peer': peer,
            'name': peer,
            'state': _isContact(peer) ? 'active' : 'request',
            'last': 0,
          });
          return {
            'ok': true,
            'incoming': {'kind': 'chat', 'chat': Map.of(chats.last), 'update_keys': false},
          };
        }
        final chat = _chat(w['chat']);
        if (chat == null) return _fail('unknown_chat');
        final m = {'id': ++_ids, 'chat': chat['id'], 'text': w['text'], 'outgoing': false, 'at': 0, 'missing': 0};
        (messages[chat['id'] as String] ??= []).add(m);
        return {
          'ok': true,
          'incoming': {'kind': 'message', 'message': m},
        };
      case 'create_invite':
        final minutes = (r['minutes'] as num).toInt();
        if (minutes <= 0 || minutes > 30 * 24 * 60) return _fail('rejected');
        final ends = _now + minutes * 60;
        final v = r['vanishing'] == true;
        invites.add({'minutes': minutes, 'vanishing': v});
        return {
          'ok': true,
          'code': 'trair2:local-julian:${'a1' * 16}:${'0f' * 16}:${v ? 'v' : 'f'}$ends:beef',
          'ends': ends,
          'vanishing': v,
        };
      case 'invite_message':
        final chat = _chat(r['chat']);
        if (chat == null) return _fail('unknown_chat');
        if (!redeem.remove(chat['peer'])) return {'ok': true};
        return {
          'ok': true,
          'body': wire({'kind': 'invite', 'chat': chat['id']}),
          'to': ['${chat['peer']}:d-peer'],
        };
      case 'set_chat_end':
        final chat = _chat(r['chat']);
        if (chat == null) return _fail('unknown_chat');
        if (chat['state'] != 'active') return _fail('blocked');
        final minutes = (r['minutes'] as num?)?.toInt();
        chat['ends'] = minutes == null ? null : _now + minutes * 60;
        return {
          'ok': true,
          'body': wire({'kind': 'ends', 'chat': chat['id'], 'ends': chat['ends']}),
          'to': ['${chat['peer']}:d-peer'],
          'ends': chat['ends'],
        };
      case 'sweep':
        final gone = [
          for (final c in chats)
            if (c['ends'] is int && (c['ends'] as int) <= _now) c['id'],
        ];
        chats.removeWhere((c) => gone.contains(c['id']));
        for (final id in gone) {
          messages.remove(id);
        }
        return {'ok': true, 'deleted': gone};
      case 'share_profile':
        final p = (r['profile'] as Map).cast<String, Object?>();
        if ((p['name'] as String? ?? '').trim().isEmpty || (p['rev'] as num? ?? 0) <= 0) return _fail('rejected');
        shared = p;
        final rev = (p['rev'] as num).toInt();
        return {
          'ok': true,
          'out': [
            for (final c in chats)
              if (c['state'] == 'active' &&
                  (r['only_contacts'] != true || _isContact(c['peer'])) &&
                  (profileSent[c['id']] ?? 0) < rev)
                {
                  'chat': c['id'],
                  'rev': rev,
                  'body': wire({'kind': 'profile', 'chat': c['id'], 'name': p['name']}),
                  'to': ['${c['peer']}:d-peer'],
                },
          ],
        };
      case 'profile_sent':
        final chat = r['chat'] as String;
        final rev = (r['rev'] as num).toInt();
        if (rev > (profileSent[chat] ?? 0)) profileSent[chat] = rev;
        return {'ok': true};
      case 'profiles':
        return {
          'ok': true,
          'profiles': [for (final p in peerProfiles) Map.of(p)],
        };
      case 'accept_request':
        final chat = _chat(r['chat']);
        if (chat == null || chat['state'] != 'request') return _fail('rejected');
        chat['state'] = 'active';
        contacts.add({'user': chat['peer'], 'name': chat['peer'], 'fingerprint': 'ff' * 16, 'confirmed': false});
        return {'ok': true, 'contact': contacts.last};
      case 'decline_request':
        final chat = _chat(r['chat']);
        if (chat == null || chat['state'] != 'request') return _fail('rejected');
        chats.remove(chat);
        messages.remove(r['chat']);
        return {'ok': true};
      case 'key_packages':
        return {
          'ok': true,
          'packages': List.filled(r['n'] as int, 'a2V5'),
          'last_resort': r['last_resort'] == true ? 'bGFzdA==' : null,
        };
      case 'apply_contact_list':
        return {'ok': true, 'withdrawn': <String>[]};
      case 'withdrawn_members':
        return {'ok': true, 'ids': <String>[]};
    }
    return _groupOp(op, r) ?? _fail('bad_request');
  }

  // ----- groups (M5) -----

  /// Groups by chat: name, owner, admins, guests (user → until), members,
  /// ends, and members whose key changed.
  final Map<String, Map<String, Object?>> groups = {};

  /// Group keys made here: chat, minutes, guest.
  final List<Map<String, Object?>> groupInvites = [];

  /// Join requests waiting for this admin: join chat → {group, user}.
  final Map<String, Map<String, Object?>> joinRequests = {};

  /// Tokens of group keys used here, by join chat.
  final Map<String, String> joinTokens = {};

  static const me = 'local-julian';

  List<String> _members(String chat) => (groups[chat]!['members'] as List).cast<String>();
  List<String> _admins(String chat) => (groups[chat]!['admins'] as List).cast<String>();

  String _role(String chat, String user) {
    final g = groups[chat]!;
    if (g['owner'] == user) return 'owner';
    if (_admins(chat).contains(user)) return 'admin';
    if ((g['guests'] as Map).containsKey(user)) return 'guest';
    return 'member';
  }

  bool _isAdmin(String chat) => const {'owner', 'admin'}.contains(_role(chat, me));

  void _syncChat(String chat) {
    final c = _chat(chat);
    final g = groups[chat];
    if (c == null || g == null) return;
    c['name'] = g['name'];
    c['role'] = _role(chat, me);
    c['members'] = _members(chat).length;
    c['ends'] = g['ends'];
  }

  void _note(String chat, Map<String, Object?> what) {
    (messages[chat] ??= []).add({
      'id': ++_ids,
      'chat': chat,
      'user': '',
      'from': 'trair',
      'text': jsonEncode(what),
      'outgoing': false,
      'at': _now,
      'missing': 0,
    });
  }

  Map<String, Object?> _groupCommit(String chat, {List<String> welcomeTo = const [], List<String> removed = const []}) {
    final g = groups[chat]!;
    final commit = {
      'chat': chat,
      'epoch': (g['epoch'] as int?) ?? 0,
      'commit': 'Y29tbWl0',
      'to': [for (final u in _members(chat)) if (u != me) '$u:d-peer'],
      'welcome': welcomeTo.isEmpty ? null : 'd2VsY29tZQ==',
      'welcome_to': welcomeTo,
      'removed': removed,
      'committers': [g['owner'], ..._admins(chat)],
      'hash': 'h-$chat-${g['epoch']}',
    };
    g['epoch'] = ((g['epoch'] as int?) ?? 0) + 1;
    pending[chat] = commit;
    return {'ok': true, 'commit': commit};
  }

  Map<String, Object?>? _groupOp(String op, Map<String, Object?> r) {
    final chatId = r['chat'] as String? ?? '';
    final g = groups[chatId];
    switch (op) {
      case 'create_group':
        final name = (r['name'] as String).trim();
        if (name.isEmpty) return _fail('rejected');
        final offers = (r['offers'] as List).cast<Map<String, Object?>>();
        final users = {
          for (final o in offers)
            if (!(o['device'] as String).startsWith('$me:')) (o['device'] as String).split(':').first,
        };
        if (users.any((u) => !_isContact(u))) return _fail('rejected');
        final id = 'g${groups.length + 1}';
        final minutes = (r['minutes'] as num?)?.toInt();
        groups[id] = {
          'name': name,
          'owner': me,
          'admins': <String>[],
          'guests': <String, int>{},
          'members': [me, ...users],
          'ends': minutes == null ? null : _now + minutes * 60,
          'changed': <String>[],
          'epoch': 0,
        };
        chats.add({'id': id, 'peer': '', 'kind': 'group', 'state': 'active', 'last': 0});
        _syncChat(id);
        _note(id, {'event': 'created', 'by': me});
        final out = users.isEmpty
            ? <String, Object?>{'ok': true, 'commit': null}
            : _groupCommit(id, welcomeTo: [for (final u in users) '$u:d-peer']);
        return {...out, 'chat': Map.of(_chat(id)!)};
      case 'group_info':
        if (g == null) return _fail('rejected');
        return {
          'ok': true,
          'group': {
            'chat': chatId,
            'name': g['name'],
            'me': _role(chatId, me),
            'ends': g['ends'],
            'my_end': (g['guests'] as Map)[me] ?? g['ends'],
            'members': [
              for (final u in _members(chatId))
                {
                  'user': u,
                  'role': _role(chatId, u),
                  'guest_until': (g['guests'] as Map)[u],
                  'devices': ['$u:d-peer'],
                  'leaving': false,
                  'key_changed': (g['changed'] as List).contains(u),
                },
            ],
          },
        };
      case 'kick' || 'set_admin' || 'hand_over' || 'rename_group' || 'set_group_end' || 'close_group' || 'add_to_group':
        if (g == null) return _fail('rejected');
        if (!_isAdmin(chatId)) return _fail('not_allowed');
        final user = r['user'] as String? ?? '';
        var removed = <String>[];
        var welcomeTo = <String>[];
        switch (op) {
          case 'kick':
            if (!_members(chatId).contains(user)) return _fail('rejected');
            if (_role(chatId, user) != 'member' && _role(chatId, user) != 'guest' && _role(chatId, me) != 'owner') {
              return _fail('not_allowed');
            }
            _members(chatId).remove(user);
            _admins(chatId).remove(user);
            (g['guests'] as Map).remove(user);
            removed = ['$user:d-peer'];
          case 'set_admin':
            if (_role(chatId, me) != 'owner') return _fail('not_allowed');
            _admins(chatId).remove(user);
            if (r['admin'] == true) _admins(chatId).add(user);
          case 'hand_over':
            if (_role(chatId, me) != 'owner') return _fail('not_allowed');
            _admins(chatId)
              ..remove(user)
              ..add(me);
            g['owner'] = user;
          case 'rename_group':
            final name = (r['name'] as String).trim();
            if (name.isEmpty) return _fail('rejected');
            g['name'] = name;
          case 'set_group_end':
            final minutes = (r['minutes'] as num?)?.toInt();
            g['ends'] = minutes == null ? null : _now + minutes * 60;
          case 'close_group':
            if (_role(chatId, me) != 'owner') return _fail('not_allowed');
          case 'add_to_group':
            final offers = (r['offers'] as List).cast<Map<String, Object?>>();
            final users = {for (final o in offers) (o['device'] as String).split(':').first};
            if (users.any((u) => !_isContact(u))) return _fail('rejected');
            _members(chatId).addAll(users.where((u) => !_members(chatId).contains(u)));
            welcomeTo = [for (final u in users) '$u:d-peer'];
        }
        final out = _groupCommit(chatId, welcomeTo: welcomeTo, removed: removed);
        if (op == 'close_group') {
          chats.removeWhere((c) => c['id'] == chatId);
          groups.remove(chatId);
          messages.remove(chatId);
          pending.remove(chatId);
          return {...out, 'ok': true};
        }
        _syncChat(chatId);
        return out;
      case 'leave_prepare':
        if (g == null) return _fail('rejected');
        if (_role(chatId, me) == 'owner' && _members(chatId).length > 1) {
          final next = _admins(chatId).isNotEmpty ? _admins(chatId).first : _members(chatId).firstWhere((u) => u != me);
          _admins(chatId).remove(next);
          g['owner'] = next;
          return _groupCommit(chatId);
        }
        if (_role(chatId, me) == 'admin') {
          _admins(chatId).remove(me);
          return _groupCommit(chatId);
        }
        return {'ok': true, 'commit': null};
      case 'leave_group':
        final c = _chat(chatId);
        if (c == null || g == null) return _fail('rejected');
        if (_isAdmin(chatId) && _members(chatId).length > 1) return _fail('not_allowed');
        final to = [for (final u in _members(chatId)) if (u != me) '$u:d-peer'];
        if (c['state'] == 'request') {
          chats.remove(c);
          messages.remove(chatId);
        } else {
          c['state'] = 'left';
          _note(chatId, {'event': 'left', 'users': [me]});
        }
        groups.remove(chatId);
        return {'ok': true, 'body': wire({'kind': 'leave', 'chat': chatId}), 'to': to};
      case 'accept_group':
        final c = _chat(chatId);
        if (c == null || c['state'] != 'request') return _fail('rejected');
        c['state'] = 'active';
        return {'ok': true};
      case 'delete_chat':
        final c = _chat(chatId);
        if (c == null || c['state'] != 'left') return _fail('rejected');
        chats.remove(c);
        messages.remove(chatId);
        return {'ok': true};
      case 'accept_new_key':
        if (g == null) return _fail('rejected');
        (g['changed'] as List).remove(r['user']);
        return {'ok': true};
      case 'propose_update' || 'tidy_group':
        if (g == null) return _fail('rejected');
        return {'ok': true};
      case 'create_group_invite':
        if (g == null) return _fail('rejected');
        if (!_isAdmin(chatId)) return _fail('not_allowed');
        final minutes = (r['minutes'] as num).toInt();
        if (minutes <= 0 || minutes > 30 * 24 * 60) return _fail('bad_request');
        final guest = r['guest'] == true;
        final ends = _now + minutes * 60;
        groupInvites.add({'chat': chatId, 'minutes': minutes, 'guest': guest});
        return {
          'ok': true,
          'code': 'trair3:$me:${'a1' * 16}:${'0e' * 16}:${guest ? 'g' : 'm'}$ends:beef',
          'ends': ends,
          'guest': guest,
        };
      case 'revoke_group_invites':
        final n = groupInvites.where((i) => i['chat'] == chatId).length;
        groupInvites.removeWhere((i) => i['chat'] == chatId);
        return {'ok': true, 'revoked': n};
      case 'group_invites':
        return {'ok': true, 'count': groupInvites.where((i) => i['chat'] == chatId).length, 'last': null};
      case 'join_group':
        final parts = (r['code'] as String).trim().split(':');
        if (parts.length != 6 || parts[0] != 'trair3') return _fail('rejected');
        final admin = parts[1];
        final offers = (r['offers'] as List).cast<Map<String, Object?>>();
        if (!offers.any((o) => (o['device'] as String).startsWith('$admin:'))) return _fail('bad_package');
        final id = 'j${chats.length + 1}';
        chats.add({'id': id, 'peer': admin, 'kind': 'join', 'name': admin, 'state': 'active', 'last': 0});
        joinTokens[id] = parts[3];
        pending[id] = {
          'chat': id,
          'epoch': 0,
          'commit': 'Y29tbWl0',
          'to': <String>[],
          'welcome': 'd2VsY29tZQ==',
          'welcome_to': ['$admin:d-peer'],
          'removed': <String>[],
          'committers': [me, admin]..sort(),
          'hash': 'h-$id',
        };
        return {'ok': true, 'chat': Map.of(chats.last), 'commit': pending[id]};
      case 'join_message':
        final token = joinTokens.remove(chatId);
        if (token == null) return {'ok': true};
        return {
          'ok': true,
          'body': wire({'kind': 'join', 'chat': chatId, 'token': token}),
          'to': ['${_chat(chatId)!['peer']}:d-peer'],
        };
      case 'join_requests':
        return {
          'ok': true,
          'requests': [
            for (final e in joinRequests.entries) {'chat': e.key, ...e.value},
          ],
        };
      case 'admit':
        final req = joinRequests[chatId];
        if (req == null) return _fail('rejected');
        final group = req['group'] as String;
        final user = req['user'] as String;
        if (_members(group).contains(user)) return _fail('rejected');
        _members(group).add(user);
        _note(group, {'event': 'added', 'users': [user], 'by': me});
        _syncChat(group);
        return _groupCommit(group, welcomeTo: ['$user:d-peer']);
      case 'close_join':
        joinRequests.remove(chatId);
        return {'ok': true};
      case 'deny_join':
        joinRequests.remove(chatId);
        return {'ok': true, 'body': wire({'kind': 'join_denied', 'chat': chatId}), 'to': ['x:d-peer']};
    }
    return null;
  }

  /// What a fake peer's group message does here (see `receive`).
  Map<String, Object?>? _groupReceive(Map<String, Object?> w) {
    switch (w['kind']) {
      case 'group_welcome':
        // Someone added this account to a group.
        final id = w['chat'] as String;
        final by = w['by'] as String;
        final members = (w['members'] as List).cast<String>();
        groups[id] = {
          'name': w['name'],
          'owner': by,
          'admins': <String>[],
          'guests': <String, int>{},
          'members': members,
          'ends': null,
          'changed': <String>[],
          'epoch': 1,
        };
        chats.add({'id': id, 'peer': '', 'kind': 'group', 'state': _isContact(by) ? 'active' : 'request', 'last': 0});
        _syncChat(id);
        _note(id, {'event': 'joined', 'by': by});
        return {
          'ok': true,
          'incoming': {'kind': 'chat', 'chat': Map.of(chats.last), 'update_keys': false},
        };
      case 'join_request':
        // A token of a key made here arrived in a join chat.
        final chat = 'a${joinRequests.length + 1}';
        joinRequests[chat] = {'group': w['group'], 'user': w['user']};
        return {
          'ok': true,
          'incoming': {'kind': 'join_request', 'chat': chat, 'group': w['group'], 'user': w['user']},
        };
      case 'group_message':
        final chat = w['chat'] as String;
        final m = {
          'id': ++_ids,
          'chat': chat,
          'user': w['user'],
          'text': w['text'],
          'outgoing': false,
          'at': _now,
          'missing': 0,
        };
        (messages[chat] ??= []).add(m);
        return {
          'ok': true,
          'incoming': {'kind': 'message', 'message': m},
        };
      case 'key_changed':
        (groups[w['chat']]!['changed'] as List).add(w['user']);
        return {
          'ok': true,
          'incoming': {'kind': 'group', 'chat': w['chat']},
        };
    }
    return null;
  }

  Map<String, Object?> _list() => {
    'list': {'seq': devices.length},
    'signature': 'fake',
  };

  @override
  Future<Map<String, Object?>> call(Map<String, Object?> r) async {
    final op = r['op'] as String;
    ops.add(op);
    switch (op) {
      case 'init':
      case 'status':
        return {'ok': true, 'exists': password != null, 'unlocked': unlocked};
      case 'create':
        if (password != null) return _fail('exists');
        final pw = r['password'] as String;
        if (pw.runes.length < 12) return _fail('weak_password');
        password = pw;
        profile = Map.of(r['profile'] as Map<String, Object?>);
        devices.removeWhere((d) => d['this'] != true);
        unlocked = true;
        return _open();
      case 'unlock':
        if (password == null) return _fail('no_vault');
        if (r['password'] != password) {
          final wrong = _wrong();
          final pin = notfall.where((p) => p['pin'] == r['password']).firstOrNull;
          if (pin == null) return wrong;
          final after = pin['after'] as String;
          if (pin['chat'] == null) {
            _wipe(after);
            return {
              ...wrong,
              'notfall': {'after': after, 'send': false},
            };
          }
          _running = pin;
          return {
            ...wrong,
            'notfall': {
              'after': after,
              'send': true,
              'server': profile['server'],
              'user': 'local-julian',
              'device': thisId,
              'body': base64.encode(utf8.encode('notfall:${pin['text']}')),
              'to': ['${_chat(pin['chat'])?['peer']}:d-peer'],
            },
          };
        }
        failures = 0;
        unlocked = true;
        return _open();
      case 'lock':
        unlocked = false;
        return {'ok': true};
      case 'save_profile':
        if (!unlocked) return _fail('locked');
        profile = Map.of(r['profile'] as Map<String, Object?>);
        return {'ok': true};
      case 'change_password':
        if (!unlocked) return _fail('locked');
        if (r['old'] != password) return _wrong();
        if ((r['new'] as String).runes.length < 12) return _fail('weak_password');
        if (notfall.any((p) => p['pin'] == r['new'])) return _fail('pin_is_password');
        password = r['new'] as String;
        return {'ok': true};
      case 'delete_account':
        if (r['password'] != password) return _wrong();
        notfall.clear();
        password = null;
        profile = {};
        unlocked = false;
        return {'ok': true};
      case 'protect_window':
        return {'ok': true, 'windows': 1};
      case 'rename_device':
        if (!unlocked) return _fail('locked');
        final label = r['label'] as String;
        final valid =
            label.runes.isNotEmpty &&
            label.runes.length <= 40 &&
            label.trim() == label &&
            !label.runes.any((c) => c < 0x20 || c == 0x7f);
        final device = devices.where((d) => d['id'] == r['id']).firstOrNull;
        if (!valid || device == null) return _fail('refused');
        device['label'] = label;
        return {'ok': true, 'devices': _copy()};
      case 'server_identity':
        if (!unlocked) return _fail('locked');
        return {
          'ok': true,
          'user': 'local-julian',
          'device': thisId,
          'account_key': accountKey,
          'device_list': _list(),
        };
      case 'sign_login':
        if (!unlocked) return _fail('locked');
        return {'ok': true, 'signature': 'sig'};
      case 'sign_deletion':
        if (!unlocked) return _fail('locked');
        return {'ok': true, 'signature': 'del'};
      case 'apply_device_list':
        if (withdrawnElsewhere) return _fail('withdrawn');
        return {'ok': true, 'changed': false, 'devices': _copy()};
      case 'pair_host_start':
        if (!unlocked) return _fail('locked');
        return {'ok': true, 'code': '${r['channel']}-anker-birne-kerze', 'hello': 'aGVsbG8='};
      case 'pair_host_finish':
        // The fake guest that typed the wrong code sends "bad".
        if (r['hello'] == 'YmFk') return _fail('pairing_failed');
        devices.add(_device('d-0000beef', null, devices.length));
        return {'ok': true, 'bundle': 'YnVuZGxl', 'device_list': _list(), 'devices': _copy()};
      case 'pair_guest_start':
        if (password != null) return _fail('exists');
        final code = r['code'] as String;
        if (!RegExp(r'^\d+-[a-z]+-[a-z]+-[a-z]+$').hasMatch(code.trim().toLowerCase())) return _fail('bad_code');
        return {'ok': true, 'channel': int.parse(code.trim().split('-').first), 'hello': 'Z2hlbGxv'};
      case 'pair_guest_offer':
        return {'ok': true, 'offer': 'b2ZmZXI='};
      case 'pair_guest_finish':
        final pw = r['password'] as String;
        if (pw.runes.length < 12) return _fail('weak_password');
        password = pw;
        unlocked = true;
        devices.add(_device('d-0000cafe', null, devices.length));
        return _open();
      case 'pair_cancel':
        return {'ok': true};
      case 'add_badge':
        if (!unlocked) return _fail('locked');
        final b = r['badge'];
        if (b is! Map<String, Object?> || b['claim'] is! Map<String, Object?>) return _fail('bad_request');
        final claim = b['claim'] as Map<String, Object?>;
        // The fake accepts badges made for its own account key only.
        if (claim['holder'] != accountKey) return _fail('refused');
        badges.add({'kind': claim['kind'], 'community': null, 'community_warning': false, 'expires': 0});
        return {
          'ok': true,
          'badges': [for (final x in badges) Map.of(x)],
        };
      case 'withdraw_device':
        if (!unlocked) return _fail('locked');
        if (r['password'] != password) return _wrong();
        final device = devices.where((d) => d['id'] == r['id']).firstOrNull;
        if (device == null || device['this'] == true) return _fail('refused');
        devices.remove(device);
        return {'ok': true, 'devices': _copy()};
      case 'qr':
        // A fixed pattern with the three corner squares, enough to draw.
        return {
          'ok': true,
          'size': 21,
          'rows': [
            for (var y = 0; y < 21; y++)
              [
                for (var x = 0; x < 21; x++)
                  ((x < 7 && y < 7) || (x > 13 && y < 7) || (x < 7 && y > 13))
                      ? ((x % 6 == 0 || y % 6 == 0 || (x % 7 > 1 && x % 7 < 5 && y % 7 > 1 && y % 7 < 5)) ? '1' : '0')
                      : ((x * 7 + y * 3 + x * y) % 3 == 0 ? '1' : '0'),
              ].join(),
          ],
        };
      case 'contact_code' ||
          'add_contact' ||
          'contacts' ||
          'chats' ||
          'messages' ||
          'start_chat' ||
          'pending_commits' ||
          'commit_answer' ||
          'send_message' ||
          'receive' ||
          'accept_request' ||
          'decline_request' ||
          'key_packages' ||
          'apply_contact_list' ||
          'withdrawn_members' ||
          'create_invite' ||
          'invite_message' ||
          'set_chat_end' ||
          'sweep' ||
          'share_profile' ||
          'profile_sent' ||
          'profiles' ||
          'create_group' ||
          'group_info' ||
          'add_to_group' ||
          'kick' ||
          'set_admin' ||
          'hand_over' ||
          'rename_group' ||
          'set_group_end' ||
          'close_group' ||
          'leave_prepare' ||
          'leave_group' ||
          'accept_group' ||
          'delete_chat' ||
          'accept_new_key' ||
          'propose_update' ||
          'tidy_group' ||
          'create_group_invite' ||
          'revoke_group_invites' ||
          'group_invites' ||
          'join_group' ||
          'join_message' ||
          'join_requests' ||
          'admit' ||
          'close_join' ||
          'deny_join':
        return _chatOp(op, r);
      case 'notfall_list' ||
          'notfall_add' ||
          'notfall_remove' ||
          'notfall_sign_login' ||
          'notfall_sign_deletion' ||
          'notfall_finish':
        return _notfallOp(op, r);
    }
    return _fail('bad_request');
  }

  Map<String, Object?> _open() => {
    'ok': true,
    'fingerprint': 'a1b2 c3d4 e5f6 0718 293a 4b5c 6d7e 8f90',
    'profile': profile,
    'device': thisId,
    'devices': _copy(),
    'upgraded': false,
    'account_key': accountKey,
    'badges': [for (final x in badges) Map.of(x)],
    'chats': chatsEnabled,
  };

  List<Map<String, Object?>> _copy() => [for (final d in devices) Map.of(d)];

  Map<String, Object?> _wrong() {
    failures += 1;
    final wait = failures < 3 ? 0 : 5 << (failures - 3);
    return {'ok': false, 'error': 'wrong_password', 'wait': wait};
  }

  Map<String, Object?> _fail(String code) => {'ok': false, 'error': code};
}
