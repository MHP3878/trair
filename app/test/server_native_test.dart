// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

// Two Trair devices pair through the real server, with the real Rust
// core and the app's real code (AppState, HTTP, the door). The library
// is loaded twice under two file names: each copy has its own core and
// its own vault, like two computers. CI builds the library and the
// server and sets TRAIR_FFI_LIB and TRAIR_SERVER_BIN; without them the
// test is skipped.

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
      final server = await Process.start(serverBin!, ['--adresse', '127.0.0.1:$port', '--daten', '${tmp.path}/server.db']);
      addTearDown(() async {
        server.kill();
        await server.exitCode;
        await tmp.delete(recursive: true);
      });
      final api = ServerApi(HttpTransport(Uri.parse(url)));
      for (var i = 0; i < 100 && !(await api.health()).ok; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      expect((await api.health()).ok, isTrue, reason: 'server did not start');

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
      pc.serverUrl = url;
      await pc.connect();
      expect(pc.serverStatus, ServerStatus.connected, reason: pc.serverMessage);

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
      await laptop.connect();
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
}

String _randomWords() {
  final random = Random.secure();
  return List.generate(20, (_) => String.fromCharCode(97 + random.nextInt(26))).join();
}
