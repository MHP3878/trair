// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

// Renders every screen into build/screenshots/*.png, so each CI run shows
// what the app really looks like (CI uploads them as "Trair-Screenshots").

import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:trair_app/backend/demo_backend.dart';
import 'package:trair_app/core/vault.dart';
import 'package:trair_app/main.dart';

import 'support/fake_door.dart';
import 'support/fake_server.dart';

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
      child: TrairApp(backend: DemoBackend(), vault: TrairVault(d), transport: server == null ? null : (_) => server),
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

    await tester.tap(find.text('Trair Beta'));
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
    await tester.tap(find.text('Trair Beta'));
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
    await _shot(tester, '20-echter-chat');
  });

  testWidgets('screenshot of a small window', (tester) async {
    await _start(tester, const Size(1024, 640));
    await _unlock(tester);
    await _shot(tester, '12-kleines-fenster');
  });
}
