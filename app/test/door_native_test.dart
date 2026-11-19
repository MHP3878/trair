// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

// Runs the app's door against the real Rust core (libtrair_ffi.so), with
// the real Argon2id settings. CI builds the library and sets
// TRAIR_FFI_LIB; without it the test is skipped.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:trair_app/core/door.dart';
import 'package:trair_app/core/vault.dart';

void main() {
  final lib = Platform.environment['TRAIR_FFI_LIB'];

  test(
    'the app talks to the real core through the narrow door',
    () async {
      final dir = await Directory.systemTemp.createTemp('trair-door-');
      addTearDown(() => dir.delete(recursive: true));
      final vault = TrairVault(NativeDoor.load());

      final status = await vault.init(dir.path);
      expect(status.ok, isTrue, reason: '${status.raw}');
      expect(status.raw['exists'], isFalse);

      expect((await vault.create('kurz', {})).error, 'weak_password');
      final created = await vault.create('ein sehr langes passwort', {'name': 'Julian'});
      expect(created.ok, isTrue, reason: '${created.raw}');
      expect(created.fingerprint, matches(RegExp(r'^[0-9a-f]{4}( [0-9a-f]{4}){7}$')));
      final file = File('${dir.path}${Platform.pathSeparator}vault.trair');
      expect(file.existsSync(), isTrue);
      expect(String.fromCharCodes(file.readAsBytesSync()).contains('Julian'), isFalse);

      // M2: the vault has this device, signed by the account, with a name.
      expect(created.devices, hasLength(1));
      final me = created.devices.single;
      expect(me.isThis, isTrue);
      expect(me.label, 'Dieser PC');
      expect(me.id, matches(RegExp(r'^d-[0-9a-f]{8}$')));
      expect(created.raw['device'], me.id);
      expect((await vault.renameDevice(me.id, ' vorne')).error, 'refused');
      final renamed = await vault.renameDevice(me.id, 'Julians Laptop');
      expect(renamed.devices.single.label, 'Julians Laptop');
      expect((await vault.withdrawDevice(me.id, 'ein sehr langes passwort')).error, 'refused');
      expect(String.fromCharCodes(file.readAsBytesSync()).contains('Julians Laptop'), isFalse);

      // The public account key, and no badges yet.
      expect(created.accountKey, matches(RegExp(r'^[0-9a-f]{64}$')));
      expect(created.badges, isEmpty);
      expect((await vault.addBadge({'claim': 'kaputt'})).error, 'bad_request');

      // Badges: the core lets nothing through that it cannot check.
      final checked = await vault.verifyBadges('00' * 32, 'Tr4ir Support', [
        {'claim': 'kaputt'},
      ]);
      expect(checked.ok, isTrue, reason: '${checked.raw}');
      expect(checked.raw['badges'], isEmpty);
      expect(checked.raw['refused'], 1);
      expect(checked.raw['name_warning'], isTrue);
      expect((await vault.verifyBadges('zz', 'Lena', [])).error, 'bad_request');

      expect((await vault.saveProfile({'name': 'Jule'})).ok, isTrue);
      expect((await vault.lock()).ok, isTrue);
      expect((await vault.saveProfile({})).error, 'locked');

      final wrong = await vault.unlock('falsches passwort 123');
      expect(wrong.error, 'wrong_password');
      final right = await vault.unlock('ein sehr langes passwort');
      expect(right.ok, isTrue);
      expect(right.profile['name'], 'Jule');
      expect(right.fingerprint, created.fingerprint);
      expect(right.devices.single.label, 'Julians Laptop');
      expect(right.upgraded, isFalse);

      expect((await vault.changePassword('nicht das alte!', 'neues langes passwort')).error, 'wrong_password');
      expect((await vault.changePassword('ein sehr langes passwort', 'neues langes passwort')).ok, isTrue);
      expect((await vault.deleteAccount('ein sehr langes passwort')).error, 'wrong_password');
      expect((await vault.deleteAccount('neues langes passwort')).ok, isTrue);
      expect(file.existsSync(), isFalse);
      expect((await vault.init(dir.path)).raw['exists'], isFalse);
    },
    skip: lib == null ? 'TRAIR_FFI_LIB not set (CI builds the core and sets it)' : false,
    timeout: const Timeout(Duration(minutes: 2)),
  );

  test('a missing core is reported, never replaced by demo data', () {
    expect(() => NativeDoor.load('/no/such/libtrair_ffi.so'), throwsA(isA<CoreMissing>()));
  });
}
