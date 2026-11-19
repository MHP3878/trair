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

  /// The address of every connection the app opened.
  final List<Uri> addresses = [];

  /// False: nothing answers (server not started).
  bool online = true;
  bool _registered = false;

  /// What the new device sends when it arrives ("YmFk" = typed the code
  /// wrong, the fake core then refuses it).
  String guestHello = 'Z2hlbGxv';

  /// Completed by the test to let the new device arrive later.
  Completer<void>? holdGuest;

  // ----- chats (M4) -----

  /// One-time key packages of this device on the server.
  int keyPackages = 0;
  String? lastResort;

  /// Changes and ciphertext that went out, as the app sent them.
  final List<Map<String, Object?>> commits = [];
  final List<Map<String, Object?>> sent = [];

  /// Refuse the next change (another device was first) with this hash.
  String? refuseWithTaken;

  /// Waiting for this device; [deliver] adds to it and wakes a long poll.
  final List<Map<String, Object?>> _inbox = [];
  final List<int> acked = [];
  int _messageIds = 0;
  Completer<void>? _arrived;

  void deliver(String body) {
    _inbox.add({'id': ++_messageIds, 'body': body, 'at': 0});
    _arrived?.complete();
    _arrived = null;
  }

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
      case ('GET', '/v1/keypackages'):
        return ok({'count': keyPackages});
      case ('POST', '/v1/keypackages'):
        keyPackages += (body!['packages'] as List).length;
        lastResort = body['last_resort'] as String? ?? lastResort;
        return ok({'count': keyPackages});
      case ('POST', '/v1/keypackages/claim'):
        final user = body!['user'];
        if (user == 'local-julian') return ok({'packages': <Object?>[]});
        return ok({
          'packages': [
            {'device': 'd-peer', 'package': 'cGFja2FnZQ=='},
          ],
        });
      case ('POST', '/v1/commits'):
        commits.add(body!);
        final taken = refuseWithTaken;
        if (taken != null) {
          refuseWithTaken = null;
          return ServerReply(409, {'ok': false, 'error': 'wrong_epoch', 'epoch': 1, 'taken': taken});
        }
        return ok({'epoch': (body['epoch'] as int) + 1});
      case ('POST', '/v1/messages'):
        sent.add(body!);
        return ok();
      case ('GET', '/v1/messages'):
        // Like the real long poll: wait until something arrives.
        if (_inbox.isEmpty && wait > 0) await (_arrived ??= Completer<void>()).future;
        return ok({'messages': List.of(_inbox)});
      case ('POST', '/v1/messages/ack'):
        final ids = (body!['ids'] as List).cast<int>();
        acked.addAll(ids);
        _inbox.removeWhere((m) => ids.contains(m['id']));
        return ok();
    }
    if (method == 'GET' && route.startsWith('/v1/accounts/')) {
      return ok({
        'account_key': 'cd' * 32,
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
