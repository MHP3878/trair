// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

// Renders every screen into build/screenshots/*.png, so each CI run shows
// what the app really looks like (CI uploads them as "Trair-Screenshots").

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:trair_app/app_state.dart';
import 'package:trair_app/backend/demo_backend.dart';
import 'package:trair_app/core/vault.dart';
import 'package:trair_app/main.dart';
import 'package:trair_app/screens/chats_screen.dart';
import 'package:trair_app/screens/lock_screen.dart';
import 'package:trair_app/widgets/common.dart';

import 'support/fake_communities.dart';
import 'support/fake_door.dart';
import 'support/fake_server.dart';
import 'support/multi_door.dart';

Future<void> _font(String family, List<String> files) async {
  final loader = FontLoader(family);
  for (final f in files) {
    final file = File(f);
    if (file.existsSync()) loader.addFont(file.readAsBytes().then(ByteData.sublistView));
  }
  await loader.load();
}

Future<void> _loadFonts() async {
  await _font('Geist', [
    for (final w in ['Regular', 'Medium', 'SemiBold', 'Bold']) 'assets/fonts/Geist-$w.ttf',
  ]);
  await _font('GeistMono', ['assets/fonts/GeistMono-Regular.ttf', 'assets/fonts/GeistMono-Medium.ttf']);
  await _font('Lucide', ['assets/fonts/Lucide.ttf']);
  // Icons: the font ships with the Flutter SDK. Without it icons are boxes.
  final root = Platform.environment['FLUTTER_ROOT'];
  if (root != null) {
    await _font('MaterialIcons', ['$root/bin/cache/artifacts/material_fonts/MaterialIcons-Regular.otf']);
  }
}

const _shotKey = Key('screenshot');

Future<void> _shot(WidgetTester tester, String name) async {
  await tester.pumpAndSettle();
  final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(_shotKey));
  await tester.runAsync(() async {
    final image = await boundary.toImage();
    final png = await image.toByteData(format: ui.ImageByteFormat.png);
    image.dispose();
    final dir = Directory('build/screenshots')..createSync(recursive: true);
    File('${dir.path}/$name.png').writeAsBytesSync(png!.buffer.asUint8List());
  });
}

const _pw = 'mein langes passwort';

Future<void> _start(WidgetTester tester, Size size, {FakeDoor? door, FakeServer? server}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final d = door ?? FakeDoor(password: _pw, profile: {'name': 'Julian', 'glow': 0xFFA18DFF});
  await tester.pumpWidget(
    RepaintBoundary(
      key: _shotKey,
      child: TrairApp(
        backend: DemoBackend(),
        vault: TrairVault(d),
        transport: server == null ? null : (_) => server,
        accountsBase: Directory.systemTemp.createTempSync('trair-shot-').path,
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> _unlock(WidgetTester tester) async {
  await tester.enterText(find.byType(TextField), _pw);
  await tester.tap(find.text('Entsperren'));
  await tester.pumpAndSettle();
}

void main() {
  setUpAll(_loadFonts);

  testWidgets('screenshots of the first start', (tester) async {
    await _start(tester, const Size(1440, 900), door: FakeDoor());
    await _shot(tester, '01-erststart-code');
    await tester.tap(find.text('Weiter'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Julian');
    await _shot(tester, '02-erststart-profil');
    await tester.tap(find.text('Weiter'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).at(0), 'ein langer satz');
    await _shot(tester, '03-erststart-passwort');
  });

  testWidgets('screenshots of every screen', (tester) async {
    await _start(tester, const Size(1440, 900));
    await tester.enterText(find.byType(TextField), 'falsches passwort');
    await tester.tap(find.text('Entsperren'));
    await _shot(tester, '04-sperre-falsches-passwort');
    await _unlock(tester);
    await _shot(tester, '05-chats');

    await tester.tap(find.text('Verschlüsselt · bestätigt'));
    await _shot(tester, '06-chats-sicherheitsnummer');

    await tester.tap(find.text('Malik').first);
    await _shot(tester, '07-chats-blockiert');

    await tester.tap(find.byTooltip('Trair Beta'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Leute einladen'));
    await _shot(tester, '08-community');

    await tester.tap(find.text('Chats'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Strenger Modus an'));
    await _shot(tester, '09-einstellungen');
    await tester.ensureVisible(find.text('Bildschirmfotos blockieren'));
    await _shot(tester, '10-einstellungen-sicherheit');
    await tester.ensureVisible(find.text('Konto endgültig löschen'));
    await _shot(tester, '11-einstellungen-konto');
  });

  testWidgets('screenshots of profiles and badges', (tester) async {
    await _start(tester, const Size(1440, 900));
    await _unlock(tester);
    await tester.tap(find.byTooltip('Trair Beta'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Lena').last);
    await _shot(tester, '13-profilkarte');
    await tester.tap(find.byTooltip('Schließen'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Tr4ir Support'));
    await _shot(tester, '14-profil-warnung');
    await tester.tap(find.byTooltip('Schließen'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Chats'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('So sehen dich andere'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('So sehen dich andere'));
    await _shot(tester, '15-eigenes-profil');
  });

  testWidgets('screenshots of the server and pairing', (tester) async {
    final server = FakeServer()..holdGuest = Completer<void>();
    await _start(tester, const Size(1440, 900), server: server);
    await _unlock(tester);
    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Verbinden'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Verbinden'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Verbunden'));
    await _shot(tester, '16-einstellungen-server');
    await tester.ensureVisible(find.text('Gerät koppeln'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Gerät koppeln'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('7-anker-birne-kerze'));
    await _shot(tester, '17-koppeln-code');
    // Let the waiting pairing finish, so no timer is left.
    server.holdGuest!.complete();
    await tester.pumpAndSettle();
  });

  testWidgets('screenshot of joining from a new device', (tester) async {
    await _start(tester, const Size(1440, 900), door: FakeDoor(), server: FakeServer());
    await tester.tap(find.text('Ich habe schon ein Konto'));
    await _shot(tester, '18-neues-geraet');
  });

  testWidgets('screenshots of contacts and a real chat', (tester) async {
    final server = FakeServer();
    final door = FakeDoor(password: _pw, profile: {'name': 'Julian', 'glow': 0xFFA18DFF})..chatsEnabled = true;
    await _start(tester, const Size(1440, 900), door: door, server: server);
    await _unlock(tester);
    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Verbinden'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Verbinden'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Chats'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Neuer Chat'));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, 'trair1:…'), 'trair1:local-lena:${'ff' * 16}:0000');
    await tester.enterText(find.widgetWithText(TextField, 'Name (nur für dich)'), 'Lena');
    await tester.tap(find.text('Hinzufügen'));
    await _shot(tester, '19-kontakte');
    await tester.tap(find.text('Chat starten'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('real-composer')), 'Hallo Lena, das hier ist echt verschlüsselt.');
    await tester.tap(find.byTooltip('Senden'));
    await tester.pumpAndSettle();
    server.deliver(
      FakeDoor.wire({'kind': 'message', 'chat': 'c1', 'text': 'Angekommen! Und der Server sieht nichts davon.'}),
    );
    server.deliver(FakeDoor.wire({'kind': 'chat', 'chat': 'x1', 'peer': 'local-unbekannt'}));
    // Lena's profile arrives: picture, colour, a badge, her chosen name.
    final pic = await tester.runAsync(() async {
      final rec = ui.PictureRecorder();
      final c = ui.Canvas(rec);
      c.drawRect(
        const ui.Rect.fromLTWH(0, 0, 128, 128),
        ui.Paint()
          ..shader = ui.Gradient.linear(ui.Offset.zero, const ui.Offset(128, 128), const [
            ui.Color(0xFF5EE6B0),
            ui.Color(0xFF2B7BFF),
          ]),
      );
      c.drawCircle(const ui.Offset(64, 50), 22, ui.Paint()..color = const ui.Color(0xEEFFFFFF));
      c.drawCircle(const ui.Offset(64, 128), 46, ui.Paint()..color = const ui.Color(0xEEFFFFFF));
      final img = await rec.endRecording().toImage(128, 128);
      final bytes = await img.toByteData(format: ui.ImageByteFormat.png);
      return bytes!.buffer.asUint8List();
    });
    server.deliver(
      FakeDoor.wire({
        'kind': 'profile',
        'user': 'local-lena',
        'profile': {
          'rev': 3,
          'name': 'Lena M.',
          'glow': 0xFF5EE6B0,
          'about': 'Fotografiert gern Züge und Sonnenuntergänge.',
          'picture': base64.encode(pic!),
          'badges': [
            {'kind': 'founder', 'community': 'Bahnfotos'},
          ],
          'name_warning': false,
        },
      }),
    );
    await tester.pumpAndSettle();
    final element = tester.element(find.byType(ChatsScreen));
    await tester.runAsync(() => precacheImage(MemoryImage(pic), element));
    await _shot(tester, '20-echter-chat');
    await tester.tap(find.textContaining('nennt sich „Lena M.“'));
    await tester.pumpAndSettle();
    await _shot(tester, '24-profil-eines-kontakts');
  });

  testWidgets('screenshots of the account list and a profile picture', (tester) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final base = Directory.systemTemp.createTempSync('trair-shot-').path;
    final door = MultiDoor(base);
    door.doors[base] = FakeDoor(password: _pw, profile: {'name': 'Julian', 'glow': 0xFFA18DFF});
    final sep = Platform.pathSeparator;
    for (final (n, card) in [(2, '{"v":1,"name":"Lena","glow":4283754938}'), (3, '{"v":1,"hidden":true}')]) {
      final dir = '$base${sep}konten$sep$n';
      Directory(dir).createSync(recursive: true);
      File('$dir${sep}vault.trair').writeAsStringSync('fake');
      File('$dir${sep}karte.json').writeAsStringSync(card);
    }
    await tester.pumpWidget(
      RepaintBoundary(
        key: _shotKey,
        child: TrairApp(backend: DemoBackend(), vault: TrairVault(door), accountsBase: base),
      ),
    );
    await tester.pumpAndSettle();
    await _unlock(tester);

    // A picture, as if chosen in the file dialog: cut and made small.
    final element = tester.element(find.byType(ChatsScreen));
    final state = AppScope.of(element);
    final raw = await tester.runAsync(() async {
      final rec = ui.PictureRecorder();
      final c = ui.Canvas(rec);
      c.drawRect(
        const ui.Rect.fromLTWH(0, 0, 300, 200),
        ui.Paint()
          ..shader = ui.Gradient.linear(ui.Offset.zero, const ui.Offset(300, 200), const [
            ui.Color(0xFF8B70FF),
            ui.Color(0xFF4DD4FF),
          ]),
      );
      c.drawCircle(const ui.Offset(150, 86), 44, ui.Paint()..color = const ui.Color(0xE6FFFFFF));
      c.drawCircle(const ui.Offset(150, 230), 90, ui.Paint()..color = const ui.Color(0xE6FFFFFF));
      final img = await rec.endRecording().toImage(300, 200);
      final bytes = await img.toByteData(format: ui.ImageByteFormat.png);
      return bytes!.buffer.asUint8List();
    });
    expect(await tester.runAsync(() => state.setPicture(raw!)), isNull);
    // The save timer started in real time (inside runAsync).
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 900)));
    await tester.pump(const Duration(seconds: 1));
    expect(File('$base${sep}karte.png').existsSync(), isTrue, reason: 'the start screen shows it');
    await tester.runAsync(() => precacheImage(MemoryImage(state.picture!), element));
    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    await _shot(tester, '22-profilbild');

    await tester.runAsync(state.lock);
    await tester.pumpAndSettle();
    for (final c in state.accountCards) {
      if (c.picture != null) {
        await tester.runAsync(() => precacheImage(MemoryImage(c.picture!), tester.element(find.byType(LockScreen))));
      }
    }
    await tester.pumpAndSettle();
    await _shot(tester, '21-kontoauswahl');
  });

  testWidgets('screenshot of the emergency PINs', (tester) async {
    final door = FakeDoor(password: _pw, profile: {'name': 'Julian', 'glow': 0xFFA18DFF})..chatsEnabled = true;
    door.chats.add({'id': 'c1', 'peer': 'local-lena', 'name': 'Lena', 'state': 'active', 'last': 0});
    door.notfall.addAll([
      {'id': 'p1', 'pin': 'loesch-123', 'after': 'vanish', 'chat': null, 'text': null},
      {
        'id': 'p2',
        'pin': 'hilfe-hilfe-12',
        'after': 'wrong_password',
        'chat': 'c1',
        'text': 'Ich brauche Hilfe, ruf mich bitte an.',
      },
    ]);
    await _start(tester, const Size(1440, 900), door: door);
    await _unlock(tester);
    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    unawaited(Scrollable.ensureVisible(tester.element(find.text('Notfall-PINs')), alignment: 0.02));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, 'Passwort eingeben'), _pw);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Anzeigen'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Erst schreiben, dann löschen'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Lena'));
    await tester.pumpAndSettle();
    await _shot(tester, '23-notfall-pins');
  });

  testWidgets('screenshots of a group with members, roles and a key', (tester) async {
    final server = FakeServer();
    final door = FakeDoor(password: _pw, profile: {'name': 'Julian', 'glow': 0xFFA18DFF})..chatsEnabled = true;
    for (final (user, name) in const [('local-lena', 'Lena'), ('local-malik', 'Malik'), ('local-aylin', 'Aylin')]) {
      door.contacts.add({'user': user, 'name': name, 'fingerprint': 'ff' * 16, 'confirmed': false});
    }
    door.peerProfiles.add({
      'user': 'local-lena',
      'rev': 1,
      'name': 'Lena M.',
      'glow': 0xFF5EE6B0,
      'about': '',
      'badges': [],
    });
    door.peerProfiles.add({
      'user': 'local-malik',
      'rev': 1,
      'name': 'Malik',
      'glow': 0xFFFFB547,
      'about': '',
      'badges': [],
    });
    await _start(tester, const Size(1440, 900), door: door, server: server);
    await _unlock(tester);
    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Verbinden'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Verbinden'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Chats'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Neue Gruppe'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.widgetWithText(TextField, 'Gruppenname'));
    await tester.enterText(find.widgetWithText(TextField, 'Gruppenname'), 'Abi 2026');
    await tester.pumpAndSettle();
    for (final name in ['Lena', 'Malik', 'Aylin']) {
      await tester.tap(find.widgetWithText(PillButton, name));
      await tester.pumpAndSettle();
    }
    await tester.tap(find.widgetWithText(PillButton, 'Nach 1 Tag weg'));
    await tester.pumpAndSettle();
    await _shot(tester, '25-neue-gruppe');
    await tester.tap(find.widgetWithText(PillButton, 'Gruppe erstellen'));
    await tester.pumpAndSettle();
    for (final (user, text) in const [
      ('local-lena', 'Wer bringt die Musikbox mit?'),
      ('local-malik', 'Ich! Und Snacks.'),
    ]) {
      server.deliver(FakeDoor.wire({'kind': 'group_message', 'chat': 'g1', 'user': user, 'text': text}));
    }
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('real-composer')), 'Perfekt, dann bis Samstag!');
    await tester.tap(find.byTooltip('Senden'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Mitglied verwalten').first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Zum Admin machen'));
    await tester.pumpAndSettle();
    await _shot(tester, '26-gruppe');
    await tester.ensureVisible(find.widgetWithText(PillButton, 'Einladung erstellen'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(PillButton, 'Einladung erstellen'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.textContaining('trair3:'));
    await tester.pumpAndSettle();
    await _shot(tester, '27-gruppe-einladen');
  });

  testWidgets('screenshots of a community with channels, roles, rules and reports', (tester) async {
    final server = FakeServer();
    final c = server.communities;
    final id = c.found('Abi 2026');
    for (final (user, roles) in const [
      ('local-lena', [2]),
      ('local-malik', <int>[]),
      ('local-aylin', <int>[]),
    ]) {
      c.addMember(id, user, roles: roles);
    }
    c.communities[id]!['rules'] = '1. Freundlich bleiben\n2. Kein Spam, keine Werbung\n3. Fotos nur mit Erlaubnis';
    c.communities[id]!['rules_rev'] = 1;
    c.postAs(id, 'allgemein', 'local-lena', 'Wer bringt die Musikbox zum Abiball mit?');
    c.postAs(id, 'allgemein', 'local-malik', 'Ich! Und Snacks.');
    c.postAs(id, 'allgemein', 'local-aylin', 'Kauft hier Follower, billig!!!');
    c.addReport(
      id,
      reporter: 'local-lena',
      target: 'local-aylin',
      reason: 'Werbung',
      text: 'Kauft hier Follower, billig!!!',
    );
    c.closedChannel(id, 'orga', readRoles: [2]);
    final other = c.found('Schach-AG', owner: 'local-olga');
    c.addMember(other, FakeCommunities.me);
    c.setRules(other, '1. Fair spielen\n2. Keine Hilfe von Programmen');
    final door = FakeDoor(password: _pw, profile: {'name': 'Julian', 'glow': 0xFFA18DFF})..chatsEnabled = true;
    for (final (user, name) in const [('local-lena', 'Lena'), ('local-malik', 'Malik'), ('local-aylin', 'Aylin')]) {
      door.contacts.add({'user': user, 'name': name, 'fingerprint': 'ff' * 16, 'confirmed': false});
    }
    door.openChecks['local-aylin'] = 'new';
    await _start(tester, const Size(1440, 900), door: door, server: server);
    await _unlock(tester);
    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Verbinden'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Verbinden'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Abi 2026'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('channel-composer')), 'Perfekt, dann bis Samstag!');
    await tester.tap(find.byTooltip('Senden'));
    await tester.pumpAndSettle();
    await _shot(tester, '28-community-kanal');
    // The closed channel: this Trair (a keeper) made its group.
    await tester.tap(find.byKey(const ValueKey('channel-orga')));
    await tester.pumpAndSettle();
    final orga = ((c.communities[id]!['channels'] as List).last as Map)['group'] as String;
    server.deliver(
      FakeDoor.wire({
        'kind': 'group_message',
        'chat': orga,
        'user': 'local-lena',
        'text': 'Budget für die Deko: 300 €',
      }),
    );
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('channel-composer')), 'Passt, ich frag beim Getränkemarkt.');
    await tester.tap(find.byTooltip('Senden'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Geschlossen · Ende-zu-Ende'));
    await tester.pumpAndSettle();
    await _shot(tester, '33-geschlossener-kanal');
    await tester.tap(find.text('Geschlossen · Ende-zu-Ende'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Community-Einstellungen'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-roles')));
    await tester.pumpAndSettle();
    await _shot(tester, '29-community-rollen');
    await tester.tap(find.byKey(const ValueKey('tab-reports')));
    await tester.pumpAndSettle();
    await _shot(tester, '30-community-meldungen');
    await tester.tap(find.byTooltip('Schach-AG'));
    await tester.pumpAndSettle();
    await _shot(tester, '31-community-regeln-zuerst');
    await tester.tap(find.byTooltip('Community gründen oder beitreten'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).last, 'Theater-AG');
    await _shot(tester, '32-community-gruenden');
  });

  testWidgets('screenshot of a small window', (tester) async {
    await _start(tester, const Size(1024, 640));
    await _unlock(tester);
    await _shot(tester, '12-kleines-fenster');
  });
}
