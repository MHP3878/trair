// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

// Renders every screen into build/screenshots/*.png, so each CI run shows
// what the app really looks like (CI uploads them as "Trair-Screenshots").

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:trair_app/backend/demo_backend.dart';
import 'package:trair_app/main.dart';

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

Future<void> _start(WidgetTester tester, Size size) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    RepaintBoundary(
      key: _shotKey,
      child: TrairApp(backend: DemoBackend()),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> _unlock(WidgetTester tester) async {
  await tester.enterText(find.byType(TextField), 'irgendwas');
  await tester.tap(find.text('Entsperren'));
  await tester.pumpAndSettle();
}

void main() {
  setUpAll(_loadFonts);

  testWidgets('screenshots of every screen', (tester) async {
    await _start(tester, const Size(1440, 900));
    await _shot(tester, '01-sperrbildschirm');

    await tester.tap(find.text('Neu hier? Mit Einladungscode starten'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Weiter'));
    await tester.pumpAndSettle();
    await _shot(tester, '02-erststart-profil');
    await tester.tap(find.text('Weiter'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).at(0), 'ein langer satz');
    await _shot(tester, '03-erststart-passwort');

    await tester.tap(find.text('Zurück'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Zurück'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Zurück'));
    await tester.pumpAndSettle();
    await _unlock(tester);
    await _shot(tester, '04-chats');

    await tester.tap(find.text('Verschlüsselt · bestätigt'));
    await _shot(tester, '05-chats-sicherheitsnummer');

    await tester.tap(find.text('Malik').first);
    await _shot(tester, '06-chats-blockiert');

    await tester.tap(find.text('Projekt Trair'));
    await _shot(tester, '07-gruppe');

    await tester.tap(find.text('Trair Beta'));
    await _shot(tester, '08-community');

    await tester.tap(find.text('Chats'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Strenger Modus an'));
    await _shot(tester, '09-einstellungen');
  });

  testWidgets('screenshot of a small window', (tester) async {
    await _start(tester, const Size(1024, 640));
    await _unlock(tester);
    await _shot(tester, '10-kleines-fenster');
  });
}
