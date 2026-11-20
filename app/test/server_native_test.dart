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
import 'package:trair_app/community_state.dart';
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
      final server = await Process.start(serverBin!, [
        '--adresse',
        '127.0.0.1:$port',
        '--daten',
        '${tmp.path}/server.db',
      ]);
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
      final julian = AppState(DemoBackend(), juliaVault, accountsBase: '${tmp.path}/julian');
      final bob = AppState(DemoBackend(), bobVault, accountsBase: '${tmp.path}/bob');
      final julianPw = _randomWords();
      for (final (s, name, password) in [(julian, 'Julian', julianPw), (bob, 'Bob', _randomWords())]) {
        final made = await s.createAccount(name: name, glowColor: TrairColors.accentLight, password: password);
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

      // Each got the other's profile through the chat, checked by the core.
      final bobUser2 = bob.contactCode.split(':')[1];
      final julianUser2 = julian.contactCode.split(':')[1];
      await until(() => bob.peerProfiles[julianUser2]?.name == 'Julian', "Bob gets Julian's profile");
      await until(() => julian.peerProfiles[bobUser2]?.name == 'Bob', "Julian gets Bob's profile");
      expect(bob.peerProfiles[julianUser2]!.glow, TrairColors.accentLight);

      // An emergency PIN: one message to Bob, then Julian's account is gone,
      // on this computer and on the server.
      final julianUser = julian.contactCode.split(':')[1];
      expect((await julian.showNotfallPins(julianPw)).ok, isTrue);
      final set = await julian.addNotfallPin(
        pin: 'notfall pin lang',
        password: julianPw,
        after: 'vanish',
        chat: chat,
        text: 'Notfall: bitte ruf mich an',
      );
      expect(set.ok, isTrue, reason: '${set.raw}');
      expect(julian.notfallPins!.single.chatName, 'Bob');
      await julian.lock();
      final fired = await julian.unlock('notfall pin lang');
      expect(fired.ok, isFalse, reason: 'from outside: a wrong password');
      await until(
        () => bob.realMessages[chat]?.any((m) => m.text == 'Notfall: bitte ruf mich an') ?? false,
        'Bob gets the emergency message',
      );
      final left = Directory('${tmp.path}/julian').listSync().map((e) => e.uri.pathSegments.last).toList();
      expect(
        left,
        everyElement(anyOf('trair.lock', 'karte.json', 'karte.png')),
        reason: 'nothing of the account: $left',
      );
      // Bob asks the server about Julian: unknown now.
      final me = await bobVault.serverIdentity();
      final asBob = ServerApi(HttpTransport(Uri.parse(url)));
      final c = await asBob.challenge(me.raw['user'] as String, me.raw['device'] as String);
      final sig = await bobVault.signLogin(c.body['challenge'] as String);
      final login = await asBob.login(
        me.raw['user'] as String,
        me.raw['device'] as String,
        c.body['challenge'] as String,
        sig.raw['signature'] as String,
      );
      asBob.token = login.body['token'] as String;
      expect((await asBob.account(julianUser)).ok, isFalse, reason: 'deleted on the server');

      // The server only ever had ciphertext.
      await bob.lock();
      // The database and its write-ahead log.
      final stored = [
        for (final name in ['server.db', 'server.db-wal'])
          if (File('${tmp.path}/$name').existsSync()) ...await File('${tmp.path}/$name').readAsBytes(),
      ];
      final text = latin1.decode(stored, allowInvalid: true);
      for (final secret in ['Hallo Bob', 'Angekommen', 'Notfall', 'Julian', 'Bob']) {
        expect(text.contains(secret), isFalse, reason: '"$secret" is readable on the server');
      }
    },
    skip: lib == null || serverBin == null ? 'TRAIR_FFI_LIB und TRAIR_SERVER_BIN fehlen' : false,
    timeout: const Timeout(Duration(minutes: 3)),
  );

  test(
    'a community through the real server: rules, signed messages, reports and kicking',
    () async {
      final tmp = await Directory.systemTemp.createTemp('trair-community-');
      final file = File(lib!);
      final copy = File('${tmp.path}/b_${file.uri.pathSegments.last}');
      await file.copy(copy.path);
      final port = 20000 + (pid + 13) % 20000;
      final url = 'http://127.0.0.1:$port';
      final server = await Process.start(serverBin!, [
        '--adresse',
        '127.0.0.1:$port',
        '--daten',
        '${tmp.path}/server.db',
      ]);
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

      final olgaVault = TrairVault(NativeDoor.load(lib));
      final maxVault = TrairVault(NativeDoor.load(copy.path));
      expect((await olgaVault.init('${tmp.path}/olga')).ok, isTrue);
      expect((await maxVault.init('${tmp.path}/max')).ok, isTrue);
      final olga = AppState(DemoBackend(), olgaVault, accountsBase: '${tmp.path}/olga');
      final max = AppState(DemoBackend(), maxVault, accountsBase: '${tmp.path}/max');
      for (final (s, name) in [(olga, 'Olga'), (max, 'Max')]) {
        final made = await s.createAccount(name: name, glowColor: TrairColors.accentLight, password: _randomWords());
        expect(made.ok, isTrue, reason: '${made.raw}');
        s.serverUrl = url;
        await s.connect();
        expect(s.serverStatus, ServerStatus.connected, reason: '${s.serverMessage} | $log');
        await s.refreshChats();
      }
      Future<void> until(bool Function() done, String what) async {
        for (var i = 0; i < 200 && !done(); i++) {
          await Future<void>.delayed(const Duration(milliseconds: 50));
        }
        expect(done(), isTrue, reason: '$what | ${olga.communities.message} | ${max.communities.message}');
      }

      await until(() => olga.communities.online && max.communities.online, 'both follow their communities');
      final o = olga.communities;
      final m = max.communities;

      // Olga founds it, sets rules and makes a one-time invitation.
      final made = await o.create('Abi 2026');
      expect(made, isNotNull, reason: o.message);
      final id = made!;
      expect(o.current!.iAmOwner, isTrue);
      expect([for (final c in o.current!.channels) c.name], ['regeln', 'allgemein']);
      expect(
        await o.edit(name: 'Abi 2026', about: 'Unser Jahrgang', rules: '1. Nett sein'),
        isTrue,
        reason: o.message,
      );
      final code = await o.createInvite(60, 1);
      expect(code, startsWith('trairc:'));

      // Max joins; he writes only after accepting the rules.
      expect(await m.join(code!), id);
      expect(m.current!.rulesOk, isFalse);
      expect(await m.post('Hallo zusammen'), isFalse);
      expect(m.message, 'Bitte erst die Regeln akzeptieren.');
      expect(await m.acceptRules(), isTrue, reason: m.message);
      expect(await m.post('Hallo zusammen'), isTrue, reason: m.message);

      // Olga sees it live, checked by her core: signed, a name seen for the
      // first time.
      await until(() => o.messagesOf(id, 'allgemein').any((x) => x.text == 'Hallo zusammen'), 'Olga gets it live');
      final fromMax = o.messagesOf(id, 'allgemein').firstWhere((x) => x.text == 'Hallo zusammen');
      expect(fromMax.signed, Signed.fresh);
      expect(await o.post('Willkommen, Max!'), isTrue, reason: o.message);
      await until(() => m.messagesOf(id, 'allgemein').any((x) => x.text == 'Willkommen, Max!'), 'Max gets it');
      expect(m.messagesOf(id, 'allgemein').firstWhere((x) => x.text == 'Willkommen, Max!').signed, Signed.fresh);
      // Loaded again: the same key as the first time.
      await m.loadMessages(id, 'allgemein');
      expect(m.messagesOf(id, 'allgemein').firstWhere((x) => x.text == 'Willkommen, Max!').signed, Signed.known);
      expect(m.messagesOf(id, 'allgemein').firstWhere((x) => x.text == 'Hallo zusammen').signed, Signed.own);
      // Once Olga is a contact, her contact key counts.
      expect(await max.addContact(olga.contactCode, 'Olga'), isTrue, reason: max.chatMessage);
      await m.loadMessages(id, 'allgemein');
      expect(m.messagesOf(id, 'allgemein').firstWhere((x) => x.text == 'Willkommen, Max!').signed, Signed.contact);

      // Max has no rights: he cannot delete Olga's message or see reports.
      final olgas = m.messagesOf(id, 'allgemein').firstWhere((x) => x.text == 'Willkommen, Max!');
      expect(await m.deleteMessage(olgas.id), isFalse);
      expect(m.message, 'Dafür fehlt dir das Recht.');
      await m.loadReports();
      expect(m.reports, isNull);

      // Max reports a message; Olga's moderation sees what was written.
      expect(await m.post('Kauft Follower bei mir'), isTrue);
      final ad = m.messagesOf(id, 'allgemein').firstWhere((x) => x.text == 'Kauft Follower bei mir');
      expect(await m.reportMessage(olgas.id, 'Test-Meldung'), isTrue, reason: m.message);
      await o.loadReports();
      expect(o.reports!.single.reason, 'Test-Meldung');
      expect(o.reports!.single.quoted, 'Willkommen, Max!');
      await o.handleReport(o.reports!.single.id, 'dismissed', note: 'nur ein Test');
      expect(o.reports, isEmpty);

      // Olga deletes Max's ad and removes him: Max is told and loses it.
      expect(await o.deleteMessage(ad.id), isTrue, reason: o.message);
      await until(() => !m.messagesOf(id, 'allgemein').any((x) => x.id == ad.id), 'the deletion reaches Max');
      final maxUser = max.contactCode.split(':')[1];
      expect(await o.kick(maxUser, 'Werbung'), isTrue, reason: o.message);
      await until(() => m.list.isEmpty && m.active == null, 'Max is out');
      expect(m.message, 'Du bist nicht mehr in „Abi 2026“.');
      await o.loadLog();
      expect([for (final e in o.log!) e.action], containsAll(['kick', 'message_delete', 'rules', 'invite']));

      await olga.lock();
      await max.lock();
      expect(o.online, isFalse, reason: 'locking forgets the communities');
    },
    skip: lib == null || serverBin == null ? 'TRAIR_FFI_LIB und TRAIR_SERVER_BIN fehlen' : false,
    timeout: const Timeout(Duration(minutes: 3)),
  );

  test(
    'a closed channel through the real server: keepers let people in, the server never reads it',
    () async {
      final tmp = await Directory.systemTemp.createTemp('trair-closed-');
      final file = File(lib!);
      final copies = [
        for (final p in ['b', 'c']) await file.copy('${tmp.path}/${p}_${file.uri.pathSegments.last}'),
      ];
      final port = 20000 + (pid + 19) % 20000;
      final url = 'http://127.0.0.1:$port';
      final server = await Process.start(serverBin!, [
        '--adresse',
        '127.0.0.1:$port',
        '--daten',
        '${tmp.path}/server.db',
      ]);
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

      final apps = <AppState>[];
      for (final (name, path) in [('olga', lib), ('max', copies[0].path), ('carl', copies[1].path)]) {
        final v = TrairVault(NativeDoor.load(path));
        expect((await v.init('${tmp.path}/$name')).ok, isTrue);
        final s = AppState(DemoBackend(), v, accountsBase: '${tmp.path}/$name');
        final made = await s.createAccount(name: name, glowColor: TrairColors.accentLight, password: _randomWords());
        expect(made.ok, isTrue, reason: '${made.raw}');
        s.serverUrl = url;
        await s.connect();
        expect(s.serverStatus, ServerStatus.connected, reason: '${s.serverMessage} | $log');
        await s.refreshChats();
        apps.add(s);
      }
      final [olga, max, carl] = apps;
      Future<void> until(bool Function() done, String what) async {
        for (var i = 0; i < 300 && !done(); i++) {
          await Future<void>.delayed(const Duration(milliseconds: 50));
        }
        expect(done(), isTrue, reason: '$what | ${olga.communities.message} | ${olga.chatMessage} | $log');
      }

      await until(() => apps.every((a) => a.communities.online), 'all follow their communities');
      final o = olga.communities;
      final id = (await o.create('Abi 2026'))!;
      final code = (await o.createInvite(60, null))!;
      expect(await max.communities.join(code), id);

      // Olga makes a closed channel; her Trair makes its group with Max.
      expect(await o.createChannel('team', 'Intern', closed: true), isTrue, reason: o.message);
      final ch = o.current!.channels.firstWhere((c) => c.name == 'team');
      expect(ch.closed, isTrue);
      final group = ch.group!;
      await until(() => max.channelChat(group) != null, 'Max is let in');
      expect(olga.channelChat(group), isNotNull);

      // Max writes; Olga reads it. The server never has the text.
      await olga.loadMessages(group);
      expect(await max.sendReal(group, 'Nur für uns, ganz geheim'), isTrue, reason: max.chatMessage);
      await until(
        () => olga.realMessages[group]?.any((m) => m.text == 'Nur für uns, ganz geheim') ?? false,
        'Olga reads it',
      );
      final fromMax = olga.realMessages[group]!.firstWhere((m) => m.text == 'Nur für uns, ganz geheim');
      expect(fromMax.sig, isNotNull);
      expect(fromMax.sent, isNotNull);
      Future<String> serverBytes() async => latin1.decode([
        for (final name in ['server.db', 'server.db-wal'])
          if (File('${tmp.path}/$name').existsSync()) ...await File('${tmp.path}/$name').readAsBytes(),
      ], allowInvalid: true);
      expect((await serverBytes()).contains('ganz geheim'), isFalse, reason: 'the server never had the text');

      // Carl joins the community: Olga's Trair lets him in and hands him
      // the texts so far, which his core checks.
      expect(await carl.communities.join(code), id);
      await until(() => carl.channelChat(group) != null, 'Carl is let in');
      await carl.loadMessages(group);
      await until(() {
        unawaited(carl.loadMessages(group));
        return carl.realMessages[group]?.any((m) => m.text == 'Nur für uns, ganz geheim') ?? false;
      }, 'Carl gets the history');

      // Carl reports Max's text: the server checks its signature.
      final carlsCopy = carl.realMessages[group]!.firstWhere((m) => m.text == 'Nur für uns, ganz geheim');
      await carl.communities.open(id);
      expect(
        await carl.communities.reportClosed(ch.id, carlsCopy.evidence, 'Test'),
        isTrue,
        reason: carl.communities.message,
      );
      await o.loadReports();
      expect(o.reports!.single.quoted, 'Nur für uns, ganz geheim');
      final forged = Map.of(carlsCopy.evidence)..['text'] = 'Max hat was anderes gesagt';
      expect(await carl.communities.reportClosed(ch.id, forged, 'Fälschung'), isFalse);

      // Max is removed from the community: Olga's Trair takes him out.
      final maxUser = max.ownUser;
      expect(await o.kick(maxUser, 'Test'), isTrue);
      await until(() => max.realChats.any((c) => c.id == group && c.state == 'left'), 'Max is out of the channel');

      // The forged report was refused: its text is nowhere on the server.
      expect((await serverBytes()).contains('Max hat was anderes gesagt'), isFalse);

      // Olga hands the community to Carl. Carl becomes a keeper inside the
      // group first; afterwards his Trair keeps the channel and takes
      // Olga's keeper role away.
      final carlUser = carl.ownUser;
      final olgaUser = olga.ownUser;
      await olga.keepersBeforeTransfer(id, carlUser);
      expect(await o.transfer(carlUser), isTrue, reason: o.message);
      Future<List<GroupMember>> membersSeenBy(AppState a) async {
        final v = await a.vault!.groupInfo(group);
        return GroupInfo.fromJson((v.raw['group'] as Map).cast<String, Object?>()).members;
      }

      var seen = <GroupMember>[];
      for (var i = 0; i < 300; i++) {
        seen = await membersSeenBy(olga);
        if (seen.any((x) => x.user == carlUser && x.isAdmin) && !seen.any((x) => x.user == olgaUser && x.isAdmin)) {
          break;
        }
        await Future<void>.delayed(const Duration(milliseconds: 50));
      }
      expect([for (final x in seen) if (x.isAdmin) x.user], [carlUser], reason: 'Carl keeps the channel now | $log');
      for (final a in apps) {
        await a.lock();
      }
    },
    skip: lib == null || serverBin == null ? 'TRAIR_FFI_LIB und TRAIR_SERVER_BIN fehlen' : false,
    timeout: const Timeout(Duration(minutes: 4)),
  );
}

String _randomWords() {
  final random = Random.secure();
  return List.generate(20, (_) => String.fromCharCode(97 + random.nextInt(26))).join();
}
