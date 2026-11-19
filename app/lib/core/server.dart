// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';
import 'dart:convert';
import 'dart:io';

/// One answer of the Trair server.
class ServerReply {
  const ServerReply(this.status, this.body);

  /// HTTP status; 0 if the server could not be reached.
  final int status;
  final Map<String, Object?> body;

  bool get ok => status == 200 && body['ok'] == true;

  /// The server's error code, `offline`, or `http_<status>`.
  String get error => body['error'] as String? ?? (status == 0 ? 'offline' : 'http_$status');

  /// Long polls answer `ready: false` when nothing arrived in time.
  bool get ready => ok && body['ready'] != false;
}

/// Sends requests. The app only ever sends what the core computed; the
/// session token is the only thing kept here, in memory.
abstract class ServerTransport {
  Future<ServerReply> send(String method, String path, {String? token, Map<String, Object?>? body, int wait = 0});
}

/// The real transport over HTTP (dart:io, no extra package).
class HttpTransport implements ServerTransport {
  HttpTransport(this.base);

  final Uri base;

  @override
  Future<ServerReply> send(
    String method,
    String path, {
    String? token,
    Map<String, Object?>? body,
    int wait = 0,
  }) async {
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 5);
    try {
      final request = await client.openUrl(method, base.resolve(path));
      request.headers.contentType = ContentType.json;
      if (token != null) request.headers.set('authorization', 'Bearer $token');
      if (body != null) request.add(utf8.encode(jsonEncode(body)));
      final response = await request.close().timeout(Duration(seconds: 15 + wait));
      final text = await response.transform(utf8.decoder).join();
      Object? decoded;
      try {
        decoded = jsonDecode(text);
      } on FormatException {
        decoded = null;
      }
      return ServerReply(response.statusCode, decoded is Map<String, Object?> ? decoded : const {});
    } on IOException {
      return const ServerReply(0, {});
    } on TimeoutException {
      return const ServerReply(0, {});
    } finally {
      client.close(force: true);
    }
  }
}

/// Until the server speaks TLS, Trair only talks to a server on this
/// computer: `http://127.0.0.1:PORT`, `http://localhost:PORT` or
/// `http://[::1]:PORT`. Anything else gives null.
Uri? parseServerUrl(String text) {
  final uri = Uri.tryParse(text.trim());
  if (uri == null || uri.scheme != 'http' || !uri.hasPort) return null;
  if (!const {'127.0.0.1', 'localhost', '::1'}.contains(uri.host)) return null;
  if (uri.userInfo.isNotEmpty) return null;
  return uri.replace(path: '/', query: null, fragment: null);
}

/// The server's routes, one method each (crates/trair-server).
class ServerApi {
  ServerApi(this.transport);

  final ServerTransport transport;
  String? token;

  Future<ServerReply> health() => transport.send('GET', '/v1/health');

  Future<ServerReply> register(Map<String, Object?> identity) => transport.send(
    'POST',
    '/v1/accounts',
    body: {'user': identity['user'], 'account_key': identity['account_key'], 'device_list': identity['device_list']},
  );

  Future<ServerReply> account(String user) =>
      transport.send('GET', '/v1/accounts/${Uri.encodeComponent(user)}', token: token);

  Future<ServerReply> challenge(String user, String device) =>
      transport.send('POST', '/v1/login/challenge', body: {'user': user, 'device': device});

  Future<ServerReply> login(String user, String device, String challenge, String signature) => transport.send(
    'POST',
    '/v1/login',
    body: {'user': user, 'device': device, 'challenge': challenge, 'signature': signature},
  );

  Future<ServerReply> putDevices(Object? list) =>
      transport.send('PUT', '/v1/devices', token: token, body: {'device_list': list});

  Future<ServerReply> deleteAccount(int now, String signature) =>
      transport.send('POST', '/v1/accounts/delete', token: token, body: {'now': now, 'signature': signature});

  Future<ServerReply> openPairing() => transport.send('POST', '/v1/pairing', token: token);

  Future<ServerReply> putHostHello(int channel, String hello) =>
      transport.send('PUT', '/v1/pairing/$channel/host', token: token, body: {'hello': hello});

  Future<ServerReply> getGuest(int channel, int wait) =>
      transport.send('GET', '/v1/pairing/$channel/guest?wait=$wait', token: token, wait: wait);

  Future<ServerReply> putBundle(int channel, String bundle) =>
      transport.send('PUT', '/v1/pairing/$channel/bundle', token: token, body: {'bundle': bundle});

  Future<ServerReply> getHostHello(int channel, int wait) =>
      transport.send('GET', '/v1/pairing/$channel/host?wait=$wait', wait: wait);

  Future<ServerReply> putGuest(int channel, String hello, String offer) =>
      transport.send('PUT', '/v1/pairing/$channel/guest', body: {'hello': hello, 'offer': offer});

  Future<ServerReply> getBundle(int channel, int wait) =>
      transport.send('GET', '/v1/pairing/$channel/bundle?wait=$wait', wait: wait);

  // ----- chats (M4) -----

  /// One-time key packages and the last-resort package of this device.
  /// With [replace] the old one-time packages go first (they lack what
  /// groups need).
  Future<ServerReply> uploadKeyPackages(List<Object?> packages, String? lastResort, {bool replace = false}) =>
      transport.send(
        'POST',
        '/v1/keypackages',
        token: token,
        body: {'packages': packages, 'last_resort': ?lastResort, if (replace) 'replace': true},
      );

  /// How many one-time key packages of this device are left.
  Future<ServerReply> keyPackageCount() => transport.send('GET', '/v1/keypackages', token: token);

  /// One key package per device of [user], as `{device: user:id, package}`.
  Future<List<Map<String, Object?>>> claim(String user) async {
    final r = await transport.send('POST', '/v1/keypackages/claim', token: token, body: {'user': user});
    if (!r.ok) return const [];
    return [
      for (final p in (r.body['packages'] as List? ?? const []).cast<Map<String, Object?>>())
        {'device': '$user:${p['device']}', 'package': p['package']},
    ];
  }

  /// A change of a chat as the core made it; the server takes the first
  /// per chat and epoch, and for a group only from its owner and admins
  /// (`committers`, sent along with every change of a group).
  Future<ServerReply> postCommit(Map<String, Object?> c) => transport.send(
    'POST',
    '/v1/commits',
    token: token,
    body: {
      'group': c['chat'],
      'epoch': c['epoch'],
      'commit': c['commit'],
      'to': targets(c['to']),
      'welcome': c['welcome'],
      'welcome_to': targets(c['welcome_to']),
      'committers': ?c['committers'],
    },
  );

  /// Ciphertext for devices (`user:device`).
  Future<ServerReply> sendMessage(Object? to, String body) =>
      transport.send('POST', '/v1/messages', token: token, body: {'to': targets(to), 'body': body});

  /// Waiting messages; with [wait] the server holds the answer up to that
  /// many seconds until something arrives.
  Future<ServerReply> fetchMessages(int wait) =>
      transport.send('GET', '/v1/messages?wait=$wait', token: token, wait: wait);

  Future<ServerReply> ack(List<int> ids) =>
      transport.send('POST', '/v1/messages/ack', token: token, body: {'ids': ids});
}

/// `user:device` ids as the server wants them.
List<Map<String, String>> targets(Object? ids) => [
  for (final id in (ids as List? ?? const []).cast<String>())
    if (id.contains(':')) {'user': id.substring(0, id.indexOf(':')), 'device': id.substring(id.indexOf(':') + 1)},
];
