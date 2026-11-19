// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

// Two Trair devices pair through the real server, with the real Rust
// core and the app's real code (AppState, HTTP, the door). The library
// is loaded twice under two file names: each copy has its own core and
// its own vault, like two computers. CI builds the library and the
// server and sets TRAIR_FFI_LIB and TRAIR_SERVER_BIN; without them the
// test is skipped.

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:flutter_test/flutter_test.dart';
import 'package:trair_app/app_state.dart';
import 'package:trair_app/backend/demo_backend.dart';
import 'package:trair_app/core/door.dart';
import 'package:trair_app/core/server.dart';
import 'package:trair_app/core/vault.dart';
import 'package:trair_app/theme.dart';

void main() {
  final lib = Platform.environment['TRAIR_FFI_LIB'];
  final serverBin = Platform.environment['TRAIR_SERVER_BIN'];

  test(
    'a second device pairs through the real server, and loses access when withdrawn',
    () async {
      final tmp = await Directory.systemTemp.createTemp('trair-e2e-');
      final file = File(lib!);
      final copy = File('${tmp.path}/b_${file.uri.pathSegments.last}');
      await file.copy(copy.path);

      final port = 20000 + pid % 20000;
      final url = 'http://127.0.0.1:$port';
      final server = await Process.start(serverBin!, [
        '--adresse',
        '127.0.0.1:$port',
        '--daten',
        '${tmp.path}/server.db',
      ]);
      // Everything the server says, for the failure messages below.
      final log = StringBuffer();
      server.stdout.transform(const SystemEncoding().decoder).listen(log.write);
      server.stderr.transform(const SystemEncoding().decoder).listen(log.write);
      var exited = 'läuft';
      unawaited(server.exitCode.then((code) => exited = 'beendet mit $code'));
      addTearDown(() async {
        server.kill();
        await server.exitCode;
        await tmp.delete(recursive: true);
      });
      final api = ServerApi(HttpTransport(Uri.parse(url)));
      for (var i = 0; i < 100 && !(await api.health()).ok; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      expect((await api.health()).ok, isTrue, reason: 'server did not start: $log');

      final pcVault = TrairVault(NativeDoor.load(lib));
      final laptopVault = TrairVault(NativeDoor.load(copy.path));
      expect((await pcVault.init('${tmp.path}/pc')).ok, isTrue);
      expect((await laptopVault.init('${tmp.path}/laptop')).ok, isTrue);
      final pc = AppState(DemoBackend(), pcVault);
      final laptop = AppState(DemoBackend(), laptopVault);

      // Random each run: nothing fixed is used as a password.
      final pcPassword = _randomWords();
      final laptopPassword = _randomWords();
      final created = await pc.createAccount(name: 'Julian', glowColor: TrairColors.accentLight, password: pcPassword);
      expect(created.ok, isTrue, reason: '${created.raw}');
      final stillThere = await api.health();
      expect(stillThere.ok, isTrue, reason: 'server gone ($exited): ${stillThere.error} $log');
      pc.serverUrl = url;
      await pc.connect();
      expect(pc.serverStatus, ServerStatus.connected, reason: '${pc.serverMessage} | server $exited | $log');

      // The PC shows a code; the laptop types it.
      final pairing = pc.startPairing();
      for (var i = 0; i < 100 && pc.pairing == null; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 50));
      }
      final code = pc.pairing!.code;
      expect(code, matches(RegExp(r'^\d+-[a-z]+-[a-z]+-[a-z]+$')));
      final joined = await laptop.joinWithCode(url: url, code: code, password: laptopPassword);
      expect(joined.ok, isTrue, reason: laptop.joinMessage);
      await pairing;
      expect(pc.pairing!.stage, PairStage.done, reason: pc.pairing!.message);

      // Same account, two devices on both sides, and the laptop logs in
      // with its own device key.
      expect(laptop.fingerprint, pc.fingerprint);
      expect(laptop.userName, 'Julian');
      expect(pc.devices, hasLength(2));
      expect(laptop.devices, hasLength(2));
      // Joining connects the laptop on its own, to the same server.
      for (var i = 0; i < 100 && laptop.serverStatus == ServerStatus.connecting; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 50));
      }
      expect(laptop.serverUrl, url);
      expect(laptop.serverStatus, ServerStatus.connected, reason: laptop.serverMessage);

      // The PC withdraws the laptop: the server refuses it from now on.
      final laptopId = laptop.devices.firstWhere((d) => d.isThis).id;
      expect((await pc.withdrawDevice(laptopId, pcPassword)).ok, isTrue);
      expect(pc.devices, hasLength(1));
      await laptop.connect();
      expect(laptop.serverStatus, ServerStatus.failed);
    },
    skip: lib == null || serverBin == null ? 'TRAIR_FFI_LIB und TRAIR_SERVER_BIN fehlen' : false,
    timeout: const Timeout(Duration(minutes: 3)),
  );

  test(
    'two accounts exchange codes and chat through the real server, end to end encrypted',
    () async {
      final tmp = await Directory.systemTemp.createTemp('trair-chat-');
      final file = File(lib!);
      final copy = File('${tmp.path}/b_${file.uri.pathSegments.last}');
      await file.copy(copy.path);
      final port = 20000 + (pid + 7) % 20000;
      final url = 'http://127.0.0.1:$port';
      final server = await Process.start(serverBin!, ['--adresse', '127.0.0.1:$port', '--daten', '${tmp.path}/server.db']);
      final log = StringBuffer();
      server.stdout.transform(const SystemEncoding().decoder).listen(log.write);
      server.stderr.transform(const SystemEncoding().decoder).listen(log.write);
      addTearDown(() async {
        server.kill();
        await server.exitCode;
        await tmp.delete(recursive: true);
      });
      final api = ServerApi(HttpTransport(Uri.parse(url)));
      for (var i = 0; i < 100 && !(await api.health()).ok; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      expect((await api.health()).ok, isTrue, reason: 'server did not start: $log');

      // Two people, each with their own vault and chat database.
      final juliaVault = TrairVault(NativeDoor.load(lib));
      final bobVault = TrairVault(NativeDoor.load(copy.path));
      expect((await juliaVault.init('${tmp.path}/julian')).ok, isTrue);
      expect((await bobVault.init('${tmp.path}/bob')).ok, isTrue);
      final julian = AppState(DemoBackend(), juliaVault);
      final bob = AppState(DemoBackend(), bobVault);
      for (final (s, name) in [(julian, 'Julian'), (bob, 'Bob')]) {
        final made = await s.createAccount(name: name, glowColor: TrairColors.accentLight, password: _randomWords());
        expect(made.ok, isTrue, reason: '${made.raw}');
        expect(s.chatsReady, isTrue, reason: 'the chat database opened');
        s.serverUrl = url;
        await s.connect();
        expect(s.serverStatus, ServerStatus.connected, reason: '${s.serverMessage} | $log');
        await s.refreshChats();
      }
      expect(julian.contactCode, startsWith('trair1:local-'));

      // Codes exchanged; Julian starts the chat.
      expect(await julian.addContact(bob.contactCode, 'Bob'), isTrue, reason: julian.chatMessage);
      expect(await bob.addContact(julian.contactCode, 'Julian'), isTrue, reason: bob.chatMessage);
      final bobUser = bob.contactCode.split(':')[1];
      final chat = await julian.startChat(bobUser);
      expect(chat, isNotNull, reason: julian.chatMessage);
      Future<void> until(bool Function() done, String what) async {
        for (var i = 0; i < 200 && !done(); i++) {
          await Future<void>.delayed(const Duration(milliseconds: 50));
        }
        expect(done(), isTrue, reason: what);
      }

      await until(() => bob.realChats.any((c) => c.id == chat), 'Bob gets the chat');
      expect(bob.realChats.single.state, 'active', reason: 'Julian is a contact, not a request');
      await bob.loadMessages(chat!);

      expect(await julian.sendReal(chat, 'Hallo Bob, ganz echt'), isTrue, reason: julian.chatMessage);
      await until(() => bob.realMessages[chat]?.any((m) => m.text == 'Hallo Bob, ganz echt') ?? false, 'Bob reads it');
      await julian.loadMessages(chat);
      expect(await bob.sendReal(chat, 'Angekommen!'), isTrue, reason: bob.chatMessage);
      await until(() => julian.realMessages[chat]?.any((m) => m.text == 'Angekommen!') ?? false, 'Julian reads it');
      expect(julian.realMessages[chat]!.first.outgoing, isTrue);

      // The server only ever had ciphertext.
      await julian.lock();
      await bob.lock();
      // The database and its write-ahead log.
      final stored = [
        for (final name in ['server.db', 'server.db-wal'])
          if (File('${tmp.path}/$name').existsSync()) ...await File('${tmp.path}/$name').readAsBytes(),
      ];
      final text = latin1.decode(stored, allowInvalid: true);
      for (final secret in ['Hallo Bob', 'Angekommen', 'Julian', 'Bob']) {
        expect(text.contains(secret), isFalse, reason: '"$secret" is readable on the server');
      }
    },
    skip: lib == null || serverBin == null ? 'TRAIR_FFI_LIB und TRAIR_SERVER_BIN fehlen' : false,
    timeout: const Timeout(Duration(minutes: 3)),
  );
}

String _randomWords() {
  final random = Random.secure();
  return List.generate(20, (_) => String.fromCharCode(97 + random.nextInt(26))).join();
}
