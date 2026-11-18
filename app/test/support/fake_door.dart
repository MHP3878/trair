// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:trair_app/core/door.dart';

/// Stands in for the Rust core in widget tests. Follows the protocol of
/// crates/trair-ffi/src/api.rs, without encryption and without waiting.
/// The real core has its own tests (crates/trair-ffi/tests/tuer.rs) and
/// test/door_native_test.dart runs the app's door against it.
class FakeDoor implements CoreDoor {
  FakeDoor({this.password, Map<String, Object?>? profile}) : profile = profile ?? {};

  /// Null: there is no vault yet (first start).
  String? password;
  Map<String, Object?> profile;
  bool unlocked = false;
  int failures = 0;

  /// Every request, without the passwords.
  final List<String> ops = [];

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
    }
    return _fail('bad_request');
  }

  Map<String, Object?> _open() => {'ok': true, 'fingerprint': 'a1b2 c3d4 e5f6 0718 293a 4b5c 6d7e 8f90', 'profile': profile};

  Map<String, Object?> _wrong() {
    failures += 1;
    final wait = failures < 3 ? 0 : 5 << (failures - 3);
    return {'ok': false, 'error': 'wrong_password', 'wait': wait};
  }

  Map<String, Object?> _fail(String code) => {'ok': false, 'error': code};
}
