// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:trair_app/backend/demo_backend.dart';
import 'package:trair_app/main.dart';
import 'package:trair_app/widgets/common.dart';

/// Loads the bundled fonts so text has the real width, as in the app.
Future<void> _loadFonts() async {
  Future<ByteData> bytes(String file) async {
    final data = await File('assets/fonts/$file').readAsBytes();
    return ByteData.sublistView(data);
  }

  final geist = FontLoader('Geist');
  for (final f in ['Geist-Regular.ttf', 'Geist-Medium.ttf', 'Geist-SemiBold.ttf', 'Geist-Bold.ttf']) {
    geist.addFont(bytes(f));
  }
  await geist.load();
  final mono = FontLoader('GeistMono');
  for (final f in ['GeistMono-Regular.ttf', 'GeistMono-Medium.ttf']) {
    mono.addFont(bytes(f));
  }
  await mono.load();
}

Future<void> _start(WidgetTester tester, {Size size = const Size(1440, 900)}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(TrairApp(backend: DemoBackend()));
  await tester.pumpAndSettle();
}

Future<void> _unlock(WidgetTester tester) async {
  await tester.enterText(find.byType(TextField), 'irgendwas');
  await tester.tap(find.text('Entsperren'));
  await tester.pumpAndSettle();
}

TextField _composer(WidgetTester tester) => tester.widget<TextField>(find.byKey(const Key('composer')));

void main() {
  setUpAll(_loadFonts);

  testWidgets('starts locked and always shows the demo badge', (tester) async {
    await _start(tester);
    expect(find.text('Willkommen zurück'), findsOneWidget);
    expect(find.text('Demo – nicht verschlüsselt'), findsOneWidget);

    await _unlock(tester);
    expect(find.text('Demo – nicht verschlüsselt'), findsOneWidget);
  });

  testWidgets('unlocking opens the chats', (tester) async {
    await _start(tester);
    await _unlock(tester);
    expect(find.text('Lena'), findsWidgets);
    expect(find.text('Verschlüsselt · bestätigt'), findsOneWidget);
  });

  testWidgets('sending a message shows it in the chat', (tester) async {
    await _start(tester);
    await _unlock(tester);
    await tester.enterText(find.byKey(const Key('composer')), 'Hallo aus dem Test');
    await tester.tap(find.byTooltip('Senden'));
    await tester.pumpAndSettle();
    expect(find.text('Hallo aus dem Test'), findsOneWidget);
    expect(_composer(tester).controller!.text, isEmpty);
  });

  testWidgets('strict mode blocks sending after a safety number change', (tester) async {
    await _start(tester);
    await _unlock(tester);
    await tester.tap(find.text('Malik').first);
    await tester.pumpAndSettle();

    expect(_composer(tester).enabled, isFalse);
    expect(find.textContaining('Senden ist blockiert'), findsOneWidget);

    await tester.tap(find.text('Verglichen, stimmt'));
    await tester.pumpAndSettle();
    expect(_composer(tester).enabled, isTrue);
    expect(find.textContaining('Senden ist blockiert'), findsNothing);
  });

  testWidgets('without strict mode the change only warns', (tester) async {
    await _start(tester);
    await _unlock(tester);

    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    final strictSwitch = find.byType(PillSwitch).first;
    await tester.ensureVisible(strictSwitch);
    await tester.pumpAndSettle();
    await tester.tap(strictSwitch);
    await tester.pumpAndSettle();
    expect(find.textContaining('Ohne strengen Modus warnt Trair nur noch'), findsOneWidget);

    await tester.tap(find.text('Chats'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Malik').first);
    await tester.pumpAndSettle();
    expect(_composer(tester).enabled, isTrue);
    expect(find.textContaining('Der strenge Modus ist aus'), findsOneWidget);
  });

  testWidgets('first start: no way forward without the no-recovery rules', (tester) async {
    await _start(tester);
    await tester.tap(find.text('Neu hier? Mit Einladungscode starten'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Weiter'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Weiter'));
    await tester.pumpAndSettle();

    PillButton start() => tester.widget<PillButton>(find.widgetWithText(PillButton, 'Trair starten'));
    Future<void> tick() async {
      await tester.ensureVisible(find.byType(Checkbox));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(Checkbox));
      await tester.pumpAndSettle();
    }

    final fields = find.byType(TextField);
    expect(fields, findsNWidgets(2));
    expect(start().onPressed, isNull);

    // Too short, even with the box ticked.
    await tester.enterText(fields.at(0), 'kurz');
    await tester.enterText(fields.at(1), 'kurz');
    await tick();
    expect(start().onPressed, isNull);

    // Long enough but not the same twice.
    await tester.enterText(fields.at(0), 'ein langer satz mit 7');
    await tester.enterText(fields.at(1), 'ein langer satz mit 8');
    await tester.pumpAndSettle();
    expect(start().onPressed, isNull);
    expect(find.text('Die Passwörter stimmen nicht überein.'), findsOneWidget);

    // Same twice, but the box unticked.
    await tester.enterText(fields.at(1), 'ein langer satz mit 7');
    await tick();
    expect(start().onPressed, isNull);

    await tick();
    expect(start().onPressed, isNotNull);
    await tester.ensureVisible(find.text('Trair starten'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Trair starten'));
    await tester.pumpAndSettle();
    expect(find.text('Lena'), findsWidgets);
  });

  testWidgets('community shows open and closed channels', (tester) async {
    await _start(tester);
    await _unlock(tester);
    await tester.tap(find.text('Trair Beta'));
    await tester.pumpAndSettle();
    expect(find.text('Geschlossen · Ende-zu-Ende'), findsOneWidget);

    await tester.tap(find.text('memes'));
    await tester.pumpAndSettle();
    expect(find.text('Offen · für Moderation lesbar'), findsOneWidget);
  });

  testWidgets('Ctrl+K jumps into the chat search', (tester) async {
    await _start(tester);
    await _unlock(tester);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pumpAndSettle();
    final search = tester.widget<TextField>(
      find.descendant(of: find.byType(PillField), matching: find.byType(TextField)).first,
    );
    expect(search.focusNode!.hasFocus, isTrue);
  });

  testWidgets('every screen fits a small window without overflow', (tester) async {
    await _start(tester, size: const Size(1024, 640));
    await _unlock(tester);
    await tester.tap(find.text('Trair Beta'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Chats'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Strenger Modus an'));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });
}
