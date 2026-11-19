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
        if (parts.length != 4 || parts[0] != 'trair1' || parts[1] == 'local-julian') return _fail('rejected');
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
    return _fail('bad_request');
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
        if (r['password'] != password) return _wrong();
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
        password = r['new'] as String;
        return {'ok': true};
      case 'delete_account':
        if (r['password'] != password) return _wrong();
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
          'withdrawn_members':
        return _chatOp(op, r);
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
