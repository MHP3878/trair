// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:io';

import 'package:trair_app/core/door.dart';

import 'fake_door.dart';

/// Several accounts behind one fake core, one [FakeDoor] per folder, as
/// the real core does with `init`. A created vault leaves a file in its
/// folder, so the account list finds it like the real one.
class MultiDoor implements CoreDoor {
  MultiDoor(this.base);

  final String base;
  final Map<String, FakeDoor> doors = {};

  /// Folders another Trair window holds.
  final Set<String> inUse = {};
  String? dir;

  FakeDoor door(String d) => doors.putIfAbsent(d, FakeDoor.new);

  @override
  Future<Map<String, Object?>> call(Map<String, Object?> r) async {
    if (r['op'] == 'init') {
      final d = r['dir'] as String;
      if (inUse.contains(d)) return {'ok': false, 'error': 'in_use'};
      // Changing the folder closes the open account, as in the core.
      if (dir != d) doors[dir]?.unlocked = false;
      dir = d;
      door(d);
    }
    final current = dir;
    if (current == null) return {'ok': false, 'error': 'not_initialized'};
    final answer = await door(current).call(r);
    final file = File('$current${Platform.pathSeparator}vault.trair');
    if (answer['ok'] == true && (r['op'] == 'create' || r['op'] == 'pair_guest_finish')) {
      file.writeAsStringSync('fake');
    }
    if (answer['ok'] == true && r['op'] == 'delete_account' && file.existsSync()) file.deleteSync();
    return answer;
  }
}
