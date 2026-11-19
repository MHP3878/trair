// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';

import 'package:trair_app/core/server.dart';

/// Stands in for crates/trair-server in widget tests: answers at once,
/// without network. The real server has its own end-to-end tests, and
/// test/server_native_test.dart runs the app against it.
class FakeServer implements ServerTransport {
  /// Every request as "METHOD path".
  final List<String> calls = [];

  /// False: nothing answers (server not started).
  bool online = true;
  bool _registered = false;

  /// What the new device sends when it arrives ("YmFk" = typed the code
  /// wrong, the fake core then refuses it).
  String guestHello = 'Z2hlbGxv';

  /// Completed by the test to let the new device arrive later.
  Completer<void>? holdGuest;

  @override
  Future<ServerReply> send(
    String method,
    String path, {
    String? token,
    Map<String, Object?>? body,
    int wait = 0,
  }) async {
    calls.add('$method $path');
    if (!online) return const ServerReply(0, {});
    ServerReply ok([Map<String, Object?> extra = const {}]) => ServerReply(200, {'ok': true, ...extra});
    final route = path.split('?').first;
    switch ((method, route)) {
      case ('GET', '/v1/health'):
        return ok();
      case ('POST', '/v1/accounts'):
        if (_registered) return const ServerReply(409, {'ok': false, 'error': 'user_taken'});
        _registered = true;
        return ok();
      case ('POST', '/v1/login/challenge'):
        return ok({'challenge': 'c' * 64});
      case ('POST', '/v1/login'):
        return ok({'token': 'session'});
      case ('PUT', '/v1/devices'):
      case ('POST', '/v1/accounts/delete'):
        return ok();
      case ('POST', '/v1/pairing'):
        return ok({'channel': 7});
    }
    if (method == 'GET' && route.startsWith('/v1/accounts/')) {
      return ok({
        'device_list': {
          'list': {'seq': 0},
        },
      });
    }
    if (route.startsWith('/v1/pairing/')) {
      final part = route.split('/').last;
      if (method == 'PUT') return ok();
      if (part == 'guest') {
        await holdGuest?.future;
        return ok({'ready': true, 'hello': guestHello, 'offer': 'b2ZmZXI='});
      }
      if (part == 'host') return ok({'ready': true, 'hello': 'aGVsbG8='});
      if (part == 'bundle') return ok({'ready': true, 'bundle': 'YnVuZGxl'});
    }
    return const ServerReply(404, {'ok': false, 'error': 'not_found'});
  }
}
