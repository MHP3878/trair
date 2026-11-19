// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

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
        return {'ok': true, 'user': 'local-julian', 'device': thisId, 'account_key': accountKey, 'device_list': _list()};
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
  };

  List<Map<String, Object?>> _copy() => [for (final d in devices) Map.of(d)];

  Map<String, Object?> _wrong() {
    failures += 1;
    final wait = failures < 3 ? 0 : 5 << (failures - 3);
    return {'ok': false, 'error': 'wrong_password', 'wait': wait};
  }

  Map<String, Object?> _fail(String code) => {'ok': false, 'error': code};
}
