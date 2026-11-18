// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:trair_app/backend/demo_backend.dart';
import 'package:trair_app/core/vault.dart';
import 'package:trair_app/main.dart';
import 'package:trair_app/backend/backend.dart';
import 'package:trair_app/widgets/badges.dart';
import 'package:trair_app/widgets/common.dart';
import 'package:trair_app/widgets/profile_popup.dart';

import 'support/fake_door.dart';

const pw = 'mein langes passwort';

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
  await (FontLoader('Lucide')..addFont(bytes('Lucide.ttf'))).load();
}

/// Start the app. Without [door]: a vault exists and Trair is locked.
Future<FakeDoor> _start(WidgetTester tester, {FakeDoor? door, Size size = const Size(1440, 900)}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final d = door ?? FakeDoor(password: pw, profile: {'name': 'Julian'});
  await tester.pumpWidget(TrairApp(backend: DemoBackend(), vault: TrairVault(d)));
  await tester.pumpAndSettle();
  return d;
}

Future<void> _type(WidgetTester tester, String password) async {
  await tester.enterText(find.byType(TextField).first, password);
  await tester.tap(find.text('Entsperren'));
  await tester.pumpAndSettle();
}

Future<void> _unlock(WidgetTester tester) => _type(tester, pw);

Future<void> _ctrl(WidgetTester tester, LogicalKeyboardKey key) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyEvent(key);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await tester.pumpAndSettle();
}

Future<void> _openSettings(WidgetTester tester) async {
  await tester.tap(find.textContaining('Strenger Modus'));
  await tester.pumpAndSettle();
}

TextField _composer(WidgetTester tester) => tester.widget<TextField>(find.byKey(const Key('composer')));

/// A text field, found by its hint (shown while the field is empty).
Finder _field(String hint) => find.widgetWithText(TextField, hint);

void main() {
  setUpAll(_loadFonts);

  group('Tresor und Sperre', () {
    testWidgets('first start without a vault shows the setup, with no way back', (tester) async {
      await _start(tester, door: FakeDoor());
      expect(find.text('Einladungscode eingeben'), findsOneWidget);
      expect(find.text('Zurück'), findsNothing);
      expect(find.text('Willkommen zurück'), findsNothing);
    });

    testWidgets('first start creates the vault and opens Trair', (tester) async {
      final door = await _start(tester, door: FakeDoor());
      await tester.tap(find.text('Weiter'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'Lena');
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
      // Same twice, box unticked, then ticked.
      await tester.enterText(fields.at(1), 'ein langer satz mit 7');
      await tick();
      expect(start().onPressed, isNull);
      await tick();
      expect(start().onPressed, isNotNull);
      await tester.ensureVisible(find.text('Trair starten'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Trair starten'));
      await tester.pumpAndSettle();

      expect(door.password, 'ein langer satz mit 7');
      expect(door.profile['name'], 'Lena');
      expect(find.text('Demo-Chats – nicht verschlüsselt'), findsOneWidget);
      expect(find.text('Strenger Modus an'), findsOneWidget);
    });

    testWidgets('a wrong password opens nothing, the right one restores the profile', (tester) async {
      await _start(tester);
      expect(find.text('Willkommen zurück'), findsOneWidget);
      expect(find.text('Julian'), findsNothing, reason: 'nothing personal on the lock screen');
      await _type(tester, 'falsches passwort');
      expect(find.text('Passwort falsch.'), findsOneWidget);
      expect(find.text('Willkommen zurück'), findsOneWidget);
      final field = tester.widget<TextField>(find.byType(TextField).first);
      expect(field.controller!.text, isEmpty, reason: 'the password leaves the field');
      await _unlock(tester);
      expect(find.text('Julian'), findsOneWidget);
    });

    testWidgets('after three wrong passwords Trair makes you wait', (tester) async {
      await _start(tester);
      for (var i = 0; i < 3; i++) {
        await _type(tester, 'falsches passwort $i');
      }
      expect(find.textContaining('bitte 5 Sekunden warten'), findsOneWidget);
      expect(tester.widget<PillButton>(find.byType(PillButton)).onPressed, isNull);
      await tester.pump(const Duration(seconds: 6));
      await tester.pumpAndSettle();
      expect(find.text('Entsperren'), findsOneWidget);
      await _unlock(tester);
      expect(find.text('Julian'), findsOneWidget);
    });

    testWidgets('Ctrl+L locks at once and the profile leaves the screen', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await _ctrl(tester, LogicalKeyboardKey.keyL);
      expect(find.text('Willkommen zurück'), findsOneWidget);
      expect(find.text('Julian'), findsNothing);
      expect(door.ops, contains('lock'));
      expect(door.unlocked, isFalse);
    });

    testWidgets('Trair locks itself after the chosen time without use', (tester) async {
      await _start(tester);
      await _unlock(tester);
      await tester.pump(const Duration(minutes: 14));
      expect(find.text('Willkommen zurück'), findsNothing);
      await tester.pump(const Duration(minutes: 1, seconds: 1));
      await tester.pumpAndSettle();
      expect(find.text('Willkommen zurück'), findsOneWidget);
    });

    testWidgets('settings are stored in the vault and survive locking', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await _openSettings(tester);
      await tester.ensureVisible(find.text('5 Min.'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('5 Min.'));
      final strictSwitch = find.byType(PillSwitch).first;
      await tester.ensureVisible(strictSwitch);
      await tester.pumpAndSettle();
      await tester.tap(strictSwitch);
      await tester.pump(const Duration(seconds: 1));
      expect(door.profile['strict'], isFalse);
      expect(door.profile['autoLockMinutes'], 5);

      await _ctrl(tester, LogicalKeyboardKey.keyL);
      await _unlock(tester);
      expect(find.text('Strenger Modus aus'), findsOneWidget);
      // The new auto-lock time is used.
      await tester.pump(const Duration(minutes: 5, seconds: 1));
      await tester.pumpAndSettle();
      expect(find.text('Willkommen zurück'), findsOneWidget);
    });

    testWidgets('changing the password needs the old one', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await _openSettings(tester);
      Future<void> fill(String label, String text) async {
        await tester.ensureVisible(_field(label));
        await tester.enterText(_field(label), text);
      }

      await fill('Altes Passwort', 'nicht das alte');
      await fill('Neues Passwort (mind. 12 Zeichen)', 'ein neues passwort');
      await fill('Neues Passwort wiederholen', 'ein neues passwort');
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Passwort ändern'));
      await tester.pumpAndSettle();
      expect(find.text('Passwort falsch.'), findsOneWidget);
      expect(door.password, pw);

      await fill('Altes Passwort', pw);
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Passwort ändern'));
      await tester.pumpAndSettle();
      expect(find.text('Neues Passwort gilt ab jetzt.'), findsOneWidget);
      expect(door.password, 'ein neues passwort');
    });

    testWidgets('deleting the account needs the password and ends at the setup', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await _openSettings(tester);
      final delete = find.text('Konto endgültig löschen');
      await tester.ensureVisible(delete);
      await tester.pumpAndSettle();
      await tester.tap(delete);
      await tester.pumpAndSettle();
      await tester.ensureVisible(_field('Passwort'));
      await tester.enterText(_field('Passwort'), 'falsch falsch');
      await tester.pumpAndSettle();
      await tester.tap(find.text('Endgültig löschen'));
      await tester.pumpAndSettle();
      expect(door.password, pw, reason: 'a wrong password deletes nothing');
      expect(find.text('Passwort falsch.'), findsOneWidget);
      await tester.enterText(_field('Passwort'), pw);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Endgültig löschen'));
      await tester.pumpAndSettle();
      expect(door.password, isNull);
      expect(find.text('Einladungscode eingeben'), findsOneWidget);
    });

    testWidgets('copied codes leave the clipboard after 60 seconds', (tester) async {
      String? clipboard;
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
        if (call.method == 'Clipboard.setData') {
          clipboard = (call.arguments as Map<Object?, Object?>)['text'] as String?;
        }
        if (call.method == 'Clipboard.getData') return {'text': clipboard};
        return null;
      });
      addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
      await _start(tester);
      await _unlock(tester);
      await tester.tap(find.text('Trair Beta'));
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('Leute einladen'));
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('Kopieren (verschwindet nach 60 Sekunden)'));
      await tester.pump();
      expect(clipboard, 'TRAIR-7KQ2-M9XD');
      await tester.pump(const Duration(seconds: 61));
      expect(clipboard, isEmpty);
    });

    testWidgets('a missing core shows an error, never the demo', (tester) async {
      tester.view.physicalSize = const Size(1440, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(TrairApp(backend: DemoBackend(), vault: null, startupError: 'nicht gefunden'));
      await tester.pumpAndSettle();
      expect(find.text('Sicherheitskern fehlt'), findsOneWidget);
      expect(find.text('Willkommen zurück'), findsNothing);
      expect(find.text('Lena'), findsNothing);
    });
  });

  group('Chats', () {
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
      await _openSettings(tester);
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
      await _ctrl(tester, LogicalKeyboardKey.keyK);
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
      await _openSettings(tester);
      expect(tester.takeException(), isNull);
    });
  });

  group('Geräte', () {
    testWidgets('the devices card shows this device from the vault and renames it', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await _openSettings(tester);
      final card = find.text('Dieses Gerät · vom Konto unterschrieben');
      await tester.ensureVisible(card);
      await tester.pumpAndSettle();
      expect(find.text('Dieser PC'), findsOneWidget);
      expect(find.text('1 Gerät'), findsOneWidget);
      // Pairing needs the server: the button is there but cannot be pressed.
      final pairing = find.widgetWithText(PillButton, 'Gerät koppeln · kommt mit dem Server');
      expect(tester.widget<PillButton>(pairing).onPressed, isNull);

      await tester.tap(find.byTooltip('Gerät umbenennen'));
      await tester.pumpAndSettle();
      await tester.enterText(_field('Dieser PC'), '  Julians Laptop  ');
      await tester.pumpAndSettle();
      await tester.tap(find.text('Name speichern'));
      await tester.pumpAndSettle();
      expect(find.text('Julians Laptop'), findsOneWidget);
      expect(door.devices.first['label'], 'Julians Laptop', reason: 'trimmed and stored in the vault');
      expect(door.ops, contains('rename_device'));

      await _ctrl(tester, LogicalKeyboardKey.keyL);
      expect(find.text('Julians Laptop'), findsNothing, reason: 'devices leave the screen when locked');
      await _unlock(tester);
      await _openSettings(tester);
      expect(find.text('Julians Laptop'), findsOneWidget);
    });

    testWidgets('withdrawing another device needs the password', (tester) async {
      final door = await _start(
        tester,
        door: FakeDoor(password: pw, profile: {}, otherDevices: ['Altes Handy']),
      );
      await _unlock(tester);
      await _openSettings(tester);
      await tester.ensureVisible(find.text('Altes Handy'));
      await tester.pumpAndSettle();
      expect(find.text('2 Geräte'), findsOneWidget);
      // This device has no withdraw button, only the other one.
      expect(find.widgetWithText(PillButton, 'Entziehen'), findsOneWidget);
      await tester.tap(find.widgetWithText(PillButton, 'Entziehen'));
      await tester.pumpAndSettle();
      await tester.enterText(_field('Dein Passwort'), 'falsch falsch');
      await tester.pumpAndSettle();
      await tester.tap(find.text('Endgültig entziehen'));
      await tester.pumpAndSettle();
      expect(find.text('Passwort falsch.'), findsOneWidget);
      expect(door.devices, hasLength(2), reason: 'a wrong password withdraws nothing');

      await tester.enterText(_field('Dein Passwort'), pw);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Endgültig entziehen'));
      await tester.pumpAndSettle();
      expect(find.text('Altes Handy'), findsNothing);
      expect(find.text('1 Gerät'), findsOneWidget);
      expect(door.devices, hasLength(1));
      expect(door.devices.single['this'], isTrue);
    });
  });

  group('Profile und Abzeichen', () {
    Finder inPopup(Finder f) => find.descendant(of: find.byType(ProfilePopup), matching: f);

    Future<void> openCommunity(WidgetTester tester) async {
      await _start(tester);
      await _unlock(tester);
      await tester.tap(find.text('Trair Beta'));
      await tester.pumpAndSettle();
    }

    testWidgets('members show their badges and open a profile card', (tester) async {
      await openCommunity(tester);
      // The member list is the last place a name appears.
      await tester.tap(find.text('Lena').last);
      await tester.pumpAndSettle();
      expect(inPopup(find.text('Kümmert sich um die Beta. Fragen gern in #allgemein.')), findsOneWidget);
      final badges = tester.widgetList<TrairBadge>(inPopup(find.byType(TrairBadge))).toList();
      expect(badges.map((b) => b.badge.kind), [BadgeKind.admin]);

      // The tooltip says where the role comes from.
      await tester.longPress(inPopup(find.byType(TrairBadge)));
      await tester.pumpAndSettle();
      expect(find.textContaining('Admin in „Trair Beta“', findRichText: true), findsOneWidget);
      await tester.pump(const Duration(seconds: 2));
      await tester.tap(find.byTooltip('Schließen'));
      await tester.pumpAndSettle();
      expect(find.byType(ProfilePopup), findsNothing);
    });

    testWidgets('a name that pretends to be official gets a warning', (tester) async {
      await openCommunity(tester);
      expect(find.byTooltip('Name wirkt offiziell, ist aber nicht vom Trair-Team'), findsWidgets);
      await tester.tap(find.text('Tr4ir Support'));
      await tester.pumpAndSettle();
      expect(inPopup(find.textContaining('Vorsicht: Der Name wirkt offiziell')), findsOneWidget);
      expect(inPopup(find.byType(TrairBadge)), findsNothing);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byType(ProfilePopup), findsNothing);
    });

    testWidgets('own about text and visibility are stored and shown to others', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await _openSettings(tester);
      final about = _field('Ein, zwei Sätze über dich');
      await tester.ensureVisible(about);
      await tester.enterText(about, 'Ich baue Trair.');
      await tester.ensureVisible(find.text('Nur Kontakte'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Nur Kontakte'));
      await tester.pump(const Duration(seconds: 1));
      expect(door.profile['about'], 'Ich baue Trair.');
      expect(door.profile['profileVisibility'], 'kontakte');

      await tester.ensureVisible(find.text('So sehen dich andere'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('So sehen dich andere'));
      await tester.pumpAndSettle();
      expect(inPopup(find.text('Ich baue Trair.')), findsOneWidget);
      expect(inPopup(find.text('Sichtbar für: Nur Kontakte')), findsOneWidget);
      expect(inPopup(find.byType(TrairBadge)), findsNWidgets(2));
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();

      await _ctrl(tester, LogicalKeyboardKey.keyL);
      await _unlock(tester);
      await _openSettings(tester);
      expect(find.text('Ich baue Trair.'), findsOneWidget, reason: 'restored from the vault');
    });
  });
}
