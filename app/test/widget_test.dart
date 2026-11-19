// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:trair_app/app_state.dart';
import 'package:trair_app/backend/backend.dart';
import 'package:trair_app/backend/demo_backend.dart';
import 'package:trair_app/core/vault.dart';
import 'package:trair_app/main.dart';
import 'package:trair_app/widgets/badges.dart';
import 'package:trair_app/widgets/common.dart';
import 'package:trair_app/widgets/profile_popup.dart';

import 'support/fake_door.dart';
import 'support/fake_server.dart';
import 'support/multi_door.dart';

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
/// An empty folder for the account list, deleted after the test.
String _tempBase() {
  final dir = Directory.systemTemp.createTempSync('trair-konten-');
  addTearDown(() {
    if (dir.existsSync()) dir.deleteSync(recursive: true);
  });
  return dir.path;
}

Future<FakeDoor> _start(
  WidgetTester tester, {
  FakeDoor? door,
  FakeServer? server,
  Size size = const Size(1440, 900),
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final d = door ?? FakeDoor(password: pw, profile: {'name': 'Julian'});
  final s = server ?? FakeServer();
  await tester.pumpWidget(
    TrairApp(
      backend: DemoBackend(),
      vault: TrairVault(d),
      accountsBase: _tempBase(),
      transport: (uri) {
        s.addresses.add(uri);
        return s;
      },
    ),
  );
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

/// A switch, found by its label.
Finder _switch(String label) => find.byWidgetPredicate((w) => w is PillSwitch && w.label == label);

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
      expect(tester.widget<PillButton>(find.byType(PillButton).first).onPressed, isNull);
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
      // The profile is gone; the name stays only on the account card.
      expect(find.textContaining('Strenger Modus'), findsNothing);
      expect(find.text('Julian'), findsOneWidget);
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
      final strictSwitch = _switch('Strenger Modus');
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
      await tester.tap(find.byTooltip('Trair Beta'));
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
      final strictSwitch = _switch('Strenger Modus');
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
      await tester.tap(find.byTooltip('Trair Beta'));
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
      await tester.tap(find.byTooltip('Trair Beta'));
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
      final pairing = find.widgetWithText(PillButton, 'Gerät koppeln');
      expect(tester.widget<PillButton>(pairing).onPressed, isNull, reason: 'only with the server');

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
      await tester.tap(find.byTooltip('Trair Beta'));
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
      // Leave the field, or it scrolls itself back into view.
      FocusManager.instance.primaryFocus?.unfocus();
      await tester.pumpAndSettle();
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
      expect(inPopup(find.byType(TrairBadge)), findsOneWidget, reason: 'only the demo founder badge, no team badge');
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();

      await _ctrl(tester, LogicalKeyboardKey.keyL);
      await _unlock(tester);
      await _openSettings(tester);
      expect(find.text('Ich baue Trair.'), findsOneWidget, reason: 'restored from the vault');
    });
  });

  group('Konto und Team-Abzeichen', () {
    testWidgets('the account key is shown and a pasted badge is checked and kept', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await _openSettings(tester);
      final key = find.byWidgetPredicate((w) => w is SelectableText && w.data == FakeDoor.accountKey);
      await tester.ensureVisible(key);
      await tester.pumpAndSettle();
      expect(key, findsOneWidget);

      // No team badge before: the demo only makes up community badges.
      List<BadgeKind> ownKinds() => [
        for (final b in tester.widget<BadgeRow>(find.byType(BadgeRow).first).badges) b.kind,
      ];
      expect(ownKinds(), [BadgeKind.founder]);

      final field = _field('Zeile aus dem Team-Werkzeug einfügen');
      await tester.ensureVisible(field);
      await tester.enterText(field, 'kein abzeichen');
      await tester.pumpAndSettle();
      await tester.tap(find.text('Hinzufügen'));
      await tester.pumpAndSettle();
      expect(find.textContaining('keine Abzeichen-Zeile'), findsOneWidget);

      final foreign = '{"claim": {"kind": "team", "holder": "${'cd' * 32}"}, "signature": ""}';
      await tester.enterText(field, foreign);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Hinzufügen'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Abgelehnt'), findsOneWidget);
      expect(door.badges, isEmpty);

      final mine = '{"claim": {"kind": "team", "holder": "${FakeDoor.accountKey}"}, "signature": ""}';
      await tester.enterText(field, mine);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Hinzufügen'));
      await tester.pumpAndSettle();
      expect(find.text('Abzeichen geprüft und gespeichert.'), findsOneWidget);
      expect(ownKinds(), [BadgeKind.team, BadgeKind.founder]);
      expect(door.ops, contains('add_badge'));

      // Kept in the vault: back after locking.
      await _ctrl(tester, LogicalKeyboardKey.keyL);
      await _unlock(tester);
      await _openSettings(tester);
      expect(ownKinds(), [BadgeKind.team, BadgeKind.founder]);
    });
  });

  group('Server und Koppeln', () {
    Future<void> connect(WidgetTester tester) async {
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
    }

    testWidgets('connecting only to this computer, logging in with the device key', (tester) async {
      final server = FakeServer();
      final door = await _start(tester, server: server);
      await _unlock(tester);
      await _openSettings(tester);
      final address = _field(defaultServerUrl);
      await tester.ensureVisible(address);
      await tester.enterText(address, 'http://example.com:8471');
      await connect(tester);
      expect(find.textContaining('nur ein Server auf diesem PC'), findsOneWidget);
      expect(server.calls, isEmpty, reason: 'nothing is sent to other addresses');

      await tester.enterText(_field('http://example.com:8471'), defaultServerUrl);
      await connect(tester);
      expect(find.text('Verbunden'), findsOneWidget);
      expect(server.calls, containsAll(['POST /v1/accounts', 'POST /v1/login', 'PUT /v1/devices']));
      expect(door.ops, containsAll(['server_identity', 'sign_login', 'apply_device_list']));
      await tester.pump(const Duration(seconds: 1));
      expect(door.profile['serverAuto'], isTrue, reason: 'connects again after the next unlock');
      final pairing = find.widgetWithText(PillButton, 'Gerät koppeln');
      expect(tester.widget<PillButton>(pairing).onPressed, isNotNull);
    });

    testWidgets('a server that is not running is reported, not hidden', (tester) async {
      final server = FakeServer()..online = false;
      await _start(tester, server: server);
      await _unlock(tester);
      await _openSettings(tester);
      await connect(tester);
      expect(find.textContaining('Server nicht erreichbar'), findsOneWidget);
    });

    testWidgets('pairing shows a code, waits, and adds the new device', (tester) async {
      final server = FakeServer()..holdGuest = Completer<void>();
      final door = await _start(tester, server: server);
      await _unlock(tester);
      await _openSettings(tester);
      await connect(tester);
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Gerät koppeln'));
      await tester.tap(find.widgetWithText(PillButton, 'Gerät koppeln'));
      await tester.pumpAndSettle();
      expect(find.text('7-anker-birne-kerze'), findsOneWidget);
      expect(find.textContaining('Warte auf das neue Gerät'), findsOneWidget);

      server.holdGuest!.complete();
      await tester.pumpAndSettle();
      expect(find.text('Gerät gekoppelt.'), findsOneWidget);
      expect(find.text('2 Geräte'), findsOneWidget);
      expect(door.devices, hasLength(2));
      expect(server.calls, containsAll(['PUT /v1/pairing/7/host', 'PUT /v1/pairing/7/bundle']));
      expect(server.calls.last, 'PUT /v1/devices', reason: 'the server learns the new list');
      await tester.tap(find.widgetWithText(PillButton, 'Schließen'));
      await tester.pumpAndSettle();
      expect(find.text('7-anker-birne-kerze'), findsNothing);
    });

    testWidgets('a mistyped code on the new device adds nothing', (tester) async {
      final server = FakeServer()..guestHello = 'YmFk';
      final door = await _start(tester, server: server);
      await _unlock(tester);
      await _openSettings(tester);
      await connect(tester);
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Gerät koppeln'));
      await tester.tap(find.widgetWithText(PillButton, 'Gerät koppeln'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Nichts wurde hinzugefügt'), findsOneWidget);
      expect(door.devices, hasLength(1));
      expect(server.calls.where((c) => c.endsWith('/bundle')), isEmpty);
    });

    testWidgets('a new device joins with the code from the other device', (tester) async {
      final server = FakeServer();
      final door = await _start(tester, door: FakeDoor(), server: server);
      await tester.tap(find.text('Ich habe schon ein Konto'));
      await tester.pumpAndSettle();
      await tester.enterText(_field(defaultServerUrl), 'http://127.0.0.1:9000');
      await tester.enterText(_field('7-anker-birne-kerze'), '7-Anker-Birne-Kerze');
      await tester.enterText(_field('Mindestens 12 Zeichen'), pw);
      await tester.enterText(_field('Noch einmal'), pw);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Koppeln'));
      await tester.pumpAndSettle();
      expect(door.password, pw, reason: 'own vault with its own password');
      expect(door.unlocked, isTrue);
      expect(server.calls, containsAll(['GET /v1/pairing/7/host?wait=25', 'PUT /v1/pairing/7/guest']));
      expect(find.text('Ich habe schon ein Konto'), findsNothing, reason: 'Trair is open');

      // The new device stays with the server it paired through: it logs
      // in there at once and keeps the address for the next unlock.
      await tester.pump(const Duration(seconds: 1));
      expect(server.calls, contains('POST /v1/login'));
      expect(server.addresses.map((a) => a.port).toSet(), {9000});
      expect(door.profile['server'], 'http://127.0.0.1:9000');
      expect(door.profile['serverAuto'], isTrue);
    });

    testWidgets('withdrawing a device tells the server, and a withdrawn device learns it', (tester) async {
      final server = FakeServer();
      final door = await _start(
        tester,
        door: FakeDoor(password: pw, profile: {}, otherDevices: ['Altes Handy']),
        server: server,
      );
      await _unlock(tester);
      await _openSettings(tester);
      await connect(tester);
      server.calls.clear();
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Entziehen'));
      await tester.tap(find.widgetWithText(PillButton, 'Entziehen'));
      await tester.pumpAndSettle();
      await tester.enterText(_field('Dein Passwort'), pw);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Endgültig entziehen'));
      await tester.pumpAndSettle();
      expect(server.calls, ['PUT /v1/devices']);

      door.withdrawnElsewhere = true;
      await tester.tap(find.widgetWithText(PillButton, 'Trennen'));
      await tester.pumpAndSettle();
      await connect(tester);
      expect(find.textContaining('von einem anderen Gerät entzogen'), findsOneWidget);
    });
  });

  group('Echte Chats', () {
    Future<(FakeDoor, FakeServer)> online(WidgetTester tester) async {
      final server = FakeServer();
      final door = FakeDoor(password: pw, profile: {'name': 'Julian'})..chatsEnabled = true;
      await _start(tester, door: door, server: server);
      await _unlock(tester);
      await _openSettings(tester);
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      expect(find.text('Verbunden'), findsOneWidget);
      await tester.tap(find.text('Chats'));
      await tester.pumpAndSettle();
      return (door, server);
    }

    testWidgets('a contact by code, a real chat through the server, both directions', (tester) async {
      final (door, server) = await online(tester);
      // Connecting put key packages and a last-resort package on the server.
      expect(server.keyPackages, 20);
      expect(server.lastResort, isNotNull);
      expect(find.text('Echt verschlüsselt'), findsOneWidget);
      expect(find.text('Demo (nicht verschlüsselt)'), findsOneWidget);

      await tester.tap(find.byTooltip('Neuer Chat'));
      await tester.pumpAndSettle();
      expect(find.textContaining('trair1:local-julian:'), findsOneWidget);
      await tester.enterText(_field('trair1:…'), 'trair1:local-bob:${'ff' * 16}:0000');
      await tester.enterText(_field('Name (nur für dich)'), 'Bob');
      await tester.tap(find.widgetWithText(PillButton, 'Hinzufügen'));
      await tester.pumpAndSettle();
      expect(door.contacts.single['name'], 'Bob');
      expect(find.textContaining('Deine Kontakte · 1'), findsOneWidget);

      await tester.tap(find.widgetWithText(PillButton, 'Chat starten'));
      await tester.pumpAndSettle();
      expect(server.commits.single['welcome_to'], [
        {'user': 'local-bob', 'device': 'd-peer'},
      ]);
      expect(door.pending, isEmpty, reason: 'the server took the change');
      expect(find.text('Echt Ende-zu-Ende verschlüsselt'), findsWidgets);
      // The new chat got the own profile right away, once.
      expect(server.sent, hasLength(1));
      expect(FakeDoor.unwire(server.sent.single['body'] as String)['kind'], 'profile');
      expect(door.profileSent, {'c1': door.shared!['rev']});

      await tester.enterText(find.byKey(const Key('real-composer')), 'Hallo Bob');
      await tester.tap(find.byTooltip('Senden'));
      await tester.pumpAndSettle();
      expect(server.sent, hasLength(2));
      expect(server.sent.last['to'], [
        {'user': 'local-bob', 'device': 'd-peer'},
      ]);
      expect(find.text('Hallo Bob'), findsOneWidget);

      // Bob answers: the long poll wakes up, the core stores it, the
      // server is told to delete it.
      server.deliver(FakeDoor.wire({'kind': 'message', 'chat': 'c1', 'text': 'Hallo Julian'}));
      await tester.pumpAndSettle();
      expect(find.text('Hallo Julian'), findsOneWidget);
      expect(server.acked, [1]);
    });

    testWidgets('a stranger is a request: no answer before accepting, declining deletes it', (tester) async {
      final (door, server) = await online(tester);
      server.deliver(FakeDoor.wire({'kind': 'chat', 'chat': 'x1', 'peer': 'local-fremd'}));
      server.deliver(FakeDoor.wire({'kind': 'message', 'chat': 'x1', 'text': 'Hallo, kennst du mich?'}));
      await tester.pumpAndSettle();
      expect(find.text('Anfrage: kein Kontakt'), findsOneWidget);
      await tester.tap(find.text('local-fremd'));
      await tester.pumpAndSettle();
      expect(find.text('Hallo, kennst du mich?'), findsOneWidget);
      expect(tester.widget<TextField>(find.byKey(const Key('real-composer'))).enabled, isFalse);
      await tester.tap(find.widgetWithText(PillButton, 'Annehmen'));
      await tester.pumpAndSettle();
      expect(door.contacts.single['user'], 'local-fremd');
      expect(tester.widget<TextField>(find.byKey(const Key('real-composer'))).enabled, isTrue);

      server.deliver(FakeDoor.wire({'kind': 'chat', 'chat': 'x2', 'peer': 'local-spam'}));
      await tester.pumpAndSettle();
      await tester.tap(find.text('local-spam'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Ablehnen'));
      await tester.pumpAndSettle();
      expect(door.chats.map((c) => c['id']), ['x1']);
      expect(find.text('local-spam'), findsNothing);
    });

    testWidgets('a wrong or own code is refused with a message', (tester) async {
      final (door, _) = await online(tester);
      await tester.tap(find.byTooltip('Neuer Chat'));
      await tester.pumpAndSettle();
      await tester.enterText(_field('trair1:…'), 'trair1:local-julian:${'a1' * 16}:beef');
      await tester.tap(find.widgetWithText(PillButton, 'Hinzufügen'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Das passt nicht'), findsOneWidget);
      expect(door.contacts, isEmpty);
    });
  });

  group('Mehrere Konten', () {
    Future<(MultiDoor, String)> startMany(WidgetTester tester, {bool second = false}) async {
      tester.view.physicalSize = const Size(1440, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final base = _tempBase();
      final door = MultiDoor(base);
      door.doors[base] = FakeDoor(password: pw, profile: {'name': 'Julian', 'glow': 0xFFA18DFF});
      if (second) {
        final dir = '$base${Platform.pathSeparator}konten${Platform.pathSeparator}2';
        Directory(dir).createSync(recursive: true);
        File('$dir${Platform.pathSeparator}vault.trair').writeAsStringSync('fake');
        File('$dir${Platform.pathSeparator}karte.json').writeAsStringSync('{"v":1,"name":"Bob","glow":4285529343}');
        door.doors[dir] = FakeDoor(password: '$pw 2', profile: {'name': 'Bob'});
      }
      await tester.pumpWidget(TrairApp(backend: DemoBackend(), vault: TrairVault(door), accountsBase: base));
      await tester.pumpAndSettle();
      return (door, base);
    }

    Future<void> makeAccount(WidgetTester tester, String name, String password) async {
      await tester.tap(find.text('Weiter'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), name);
      await tester.tap(find.text('Weiter'));
      await tester.pumpAndSettle();
      final fields = find.byType(TextField);
      await tester.enterText(fields.at(0), password);
      await tester.enterText(fields.at(1), password);
      await tester.ensureVisible(find.byType(Checkbox));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(Checkbox));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Trair starten'));
      await tester.pumpAndSettle();
    }

    testWidgets('a new account is made from the start screen and both are listed by name', (tester) async {
      final (door, base) = await startMany(tester);
      // An account from before has no card yet: "Konto 1" until unlocked.
      expect(find.text('Konto 1'), findsOneWidget);
      await _unlock(tester);
      await _ctrl(tester, LogicalKeyboardKey.keyL);
      expect(find.text('Julian'), findsOneWidget, reason: 'the card shows the name now');

      await tester.tap(find.text('Neues Konto erstellen'));
      await tester.pumpAndSettle();
      expect(find.text('Neues Konto'), findsOneWidget);
      await makeAccount(tester, 'Bob', 'bobs langes passwort');
      expect(door.dir, '$base${Platform.pathSeparator}konten${Platform.pathSeparator}2');
      await _ctrl(tester, LogicalKeyboardKey.keyL);
      expect(find.text('Julian'), findsOneWidget);
      expect(find.text('Bob'), findsOneWidget);

      // Back to Julian: his own password opens his account only.
      await tester.tap(find.text('Julian'));
      await tester.pumpAndSettle();
      await _type(tester, 'bobs langes passwort');
      expect(find.text('Passwort falsch.'), findsOneWidget);
      await _unlock(tester);
      expect(door.dir, base);
      expect(find.textContaining('Strenger Modus'), findsOneWidget);
    });

    testWidgets('an account open in another window is refused, the others still work', (tester) async {
      final (door, base) = await startMany(tester, second: true);
      expect(find.text('Bob'), findsOneWidget);
      final bobDir = '$base${Platform.pathSeparator}konten${Platform.pathSeparator}2';
      door.inUse.add(bobDir);
      await tester.tap(find.text('Bob'));
      await tester.pumpAndSettle();
      expect(find.textContaining('schon in einem anderen Trair-Fenster offen'), findsOneWidget);
      expect(door.dir, base, reason: 'Julian stays selected');
      door.inUse.clear();
      await tester.tap(find.text('Bob'));
      await tester.pumpAndSettle();
      await _type(tester, '$pw 2');
      expect(door.dir, bobDir);
      expect(door.doors[bobDir]!.unlocked, isTrue);
    });

    testWidgets('leaving a new account unfinished leaves no folder behind', (tester) async {
      final (_, base) = await startMany(tester);
      await tester.tap(find.text('Neues Konto erstellen'));
      await tester.pumpAndSettle();
      final dir = Directory('$base${Platform.pathSeparator}konten${Platform.pathSeparator}2');
      expect(dir.existsSync(), isTrue);
      await tester.tap(find.byTooltip('Zurück zur Kontoauswahl'));
      await tester.pumpAndSettle();
      expect(dir.existsSync(), isFalse);
      expect(find.text('Neues Konto erstellen'), findsOneWidget);
    });

    testWidgets('an account can hide its name and colour from the start screen', (tester) async {
      final (_, base) = await startMany(tester);
      await _unlock(tester);
      await _openSettings(tester);
      final toggle = _switch('Auf dem Startbildschirm zeigen');
      await tester.ensureVisible(toggle);
      await tester.pumpAndSettle();
      await tester.tap(toggle);
      await tester.pump(const Duration(seconds: 1));
      final card = File('$base${Platform.pathSeparator}karte.json').readAsStringSync();
      expect(card, isNot(contains('Julian')), reason: 'nothing about the person outside the vault');
      await _ctrl(tester, LogicalKeyboardKey.keyL);
      expect(find.text('Konto 1'), findsOneWidget);
      expect(find.text('Name verborgen'), findsOneWidget);
      expect(find.text('Julian'), findsNothing);
    });

    testWidgets('an emergency PIN takes only its own account off the list', (tester) async {
      final (door, base) = await startMany(tester, second: true);
      door.doors[base]!.notfall.add({'id': 'p1', 'pin': 'loesch-123', 'after': 'vanish', 'chat': null, 'text': null});
      await _unlock(tester);
      await _ctrl(tester, LogicalKeyboardKey.keyL);
      expect(find.text('Julian'), findsOneWidget);
      await tester.tap(find.text('Julian'));
      await tester.pumpAndSettle();
      await _type(tester, 'loesch-123');
      expect(door.doors[base]!.password, isNull, reason: 'the vault is gone');
      expect(File('$base${Platform.pathSeparator}karte.json').existsSync(), isFalse);
      expect(find.text('Julian'), findsNothing);
      expect(find.text('Bob'), findsOneWidget, reason: 'the other account stays');
      expect(find.text('Passwort falsch.'), findsNothing, reason: 'no hint under the other account');
      expect(find.text('Neues Konto erstellen'), findsOneWidget);
    });
  });

  group('Notfall-PINs', () {
    /// Settings, scrolled to the card, PINs shown with the password.
    Future<void> openPins(WidgetTester tester) async {
      await _openSettings(tester);
      await tester.ensureVisible(find.text('Notfall-PINs'));
      await tester.pumpAndSettle();
      await tester.enterText(_field('Passwort eingeben'), pw);
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Anzeigen'));
      await tester.pumpAndSettle();
    }

    Future<void> setPin(WidgetTester tester, String pin) async {
      await tester.enterText(_field('Neue PIN'), pin);
      await tester.enterText(_field('PIN wiederholen'), pin);
      await tester.pumpAndSettle();
    }

    PillButton save(WidgetTester tester) => tester.widget<PillButton>(find.widgetWithText(PillButton, 'PIN speichern'));

    /// Scroll to a button, tap it, let the screen settle.
    Future<void> press(WidgetTester tester, String label) async {
      final button = find.widgetWithText(PillButton, label);
      await tester.ensureVisible(button);
      await tester.pumpAndSettle();
      await tester.tap(button);
      await tester.pumpAndSettle();
    }

    testWidgets('the PINs stay hidden until the password; a PIN that only deletes leaves nothing', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await _openSettings(tester);
      await tester.ensureVisible(find.text('Notfall-PINs'));
      await tester.pumpAndSettle();
      expect(find.text('Noch keine Notfall-PIN.'), findsNothing, reason: 'hidden at an open Trair');
      await tester.enterText(_field('Passwort eingeben'), 'falsch geraten');
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Anzeigen'));
      await tester.pumpAndSettle();
      expect(find.text('Passwort falsch.'), findsOneWidget);
      expect(find.text('Noch keine Notfall-PIN.'), findsNothing);

      await tester.enterText(_field('Passwort eingeben'), pw);
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Anzeigen'));
      await tester.pumpAndSettle();
      expect(find.text('Noch keine Notfall-PIN.'), findsOneWidget);
      await setPin(tester, 'kurz');
      expect(save(tester).onPressed, isNull, reason: 'shorter than 6');
      await setPin(tester, 'loesch-123');
      await tester.enterText(_field('PIN wiederholen'), 'loesch-124');
      await tester.pumpAndSettle();
      expect(save(tester).onPressed, isNull, reason: 'the two differ');
      await setPin(tester, 'loesch-123');
      await press(tester, 'PIN speichern');
      expect(door.notfall.single['after'], 'vanish');
      expect(find.text('Löscht alles'), findsOneWidget);
      expect(find.textContaining('Notfall-PIN gespeichert'), findsOneWidget);
      await press(tester, 'Verbergen');
      expect(find.text('Löscht alles'), findsNothing);

      // On the start screen the PIN looks like a typo, and everything goes.
      await _ctrl(tester, LogicalKeyboardKey.keyL);
      await _type(tester, 'loesch-123');
      expect(door.password, isNull);
      expect(door.ops, contains('unlock'));
      expect(find.text('Passwort falsch.'), findsNothing);
      expect(find.text('Weiter'), findsOneWidget, reason: 'the only account is gone: first start');
    });

    testWidgets('a PIN can leave the account standing, where every password is wrong', (tester) async {
      final door = await _start(tester);
      await _unlock(tester);
      await openPins(tester);
      await press(tester, 'Konto bleibt, Passwort „falsch“');
      await setPin(tester, 'bleibt-123');
      await press(tester, 'PIN speichern');
      expect(find.textContaining('jedes Passwort gilt als falsch'), findsOneWidget);
      await _ctrl(tester, LogicalKeyboardKey.keyL);
      await _type(tester, 'bleibt-123');
      expect(find.text('Passwort falsch.'), findsOneWidget);
      expect(find.text('Julian'), findsOneWidget, reason: 'name and picture stay on the list');
      expect(door.notfall, isEmpty);
      await _unlock(tester);
      expect(find.text('Passwort falsch.'), findsOneWidget, reason: 'the real password opens nothing now');
      expect(find.textContaining('Strenger Modus'), findsNothing);
    });

    testWidgets('a PIN writes to the chosen chat through the server, then deletes there and here', (tester) async {
      final server = FakeServer();
      final door = FakeDoor(password: pw, profile: {'name': 'Julian', 'server': 'http://127.0.0.1:8471'})
        ..chatsEnabled = true;
      door.contacts.add({'user': 'local-bob', 'name': 'Bob', 'fingerprint': 'ff' * 16, 'confirmed': true});
      door.chats.add({'id': 'c1', 'peer': 'local-bob', 'name': 'Bob', 'state': 'active', 'last': 0});
      await _start(tester, door: door, server: server);
      await _unlock(tester);
      await openPins(tester);
      await press(tester, 'Erst schreiben, dann löschen');
      await press(tester, 'Bob');
      await tester.enterText(_field('Nachricht, zum Beispiel: Ich brauche Hilfe, ruf mich an.'), 'Hilfe, ruf mich an');
      await setPin(tester, 'hilfe-12');
      expect(save(tester).onPressed, isNull, reason: 'a PIN that opens the keys needs 12 characters');
      expect(find.textContaining('mindestens 12 Zeichen'), findsOneWidget);
      await setPin(tester, 'hilfe-hilfe-12');
      await press(tester, 'PIN speichern');
      expect(door.notfall.single['chat'], 'c1');
      expect(find.text('Schreibt an Bob, dann löschen'), findsOneWidget);

      await _ctrl(tester, LogicalKeyboardKey.keyL);
      server.calls.clear();
      await _type(tester, 'hilfe-hilfe-12');
      expect(server.calls, [
        'POST /v1/login/challenge',
        'POST /v1/login',
        'POST /v1/messages',
        'POST /v1/accounts/delete',
      ]);
      expect(server.sent.last['to'], [
        {'user': 'local-bob', 'device': 'd-peer'},
      ]);
      expect(utf8.decode(base64.decode(server.sent.last['body'] as String)), 'notfall:Hilfe, ruf mich an');
      expect(door.ops.last, 'notfall_finish');
      expect(door.password, isNull);
      expect(door.chats, isEmpty);
      expect(find.text('Weiter'), findsOneWidget);
    });

    testWidgets('without a server the PIN still deletes everything', (tester) async {
      final server = FakeServer()..online = false;
      final door = FakeDoor(password: pw, profile: {'name': 'Julian', 'server': 'http://127.0.0.1:8471'})
        ..chatsEnabled = true;
      door.chats.add({'id': 'c1', 'peer': 'local-bob', 'name': 'Bob', 'state': 'active', 'last': 0});
      door.notfall.add({'id': 'p1', 'pin': 'hilfe-hilfe-12', 'after': 'vanish', 'chat': 'c1', 'text': 'Hilfe'});
      await _start(tester, door: door, server: server);
      await _type(tester, 'hilfe-hilfe-12');
      expect(server.sent, isEmpty);
      expect(door.ops.last, 'notfall_finish');
      expect(door.password, isNull);
      expect(find.text('Weiter'), findsOneWidget);
    });
  });

  group('Profile an Kontakte', () {
    /// A valid 1 × 1 PNG.
    const dot = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

    /// Julian with a chat with his contact Bob and a request from a
    /// stranger, connected to the server.
    Future<(FakeDoor, FakeServer)> online(WidgetTester tester) async {
      final server = FakeServer();
      final door = FakeDoor(password: pw, profile: {'name': 'Julian', 'glow': 0xFFA18DFF})..chatsEnabled = true;
      door.contacts.add({'user': 'local-bob', 'name': 'Bob', 'fingerprint': 'ff' * 16, 'confirmed': false});
      door.chats.addAll([
        {'id': 'c1', 'peer': 'local-bob', 'name': 'Bob', 'state': 'active', 'last': 0},
        {'id': 'x1', 'peer': 'local-fremd', 'name': 'local-fremd', 'state': 'request', 'last': 0},
      ]);
      await _start(tester, door: door, server: server);
      await _unlock(tester);
      await _openSettings(tester);
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      expect(find.text('Verbunden'), findsOneWidget);
      return (door, server);
    }

    List<Map<String, Object?>> profilesSent(FakeServer server) => [
      for (final m in server.sent)
        if (FakeDoor.unwire(m['body'] as String)['kind'] == 'profile') FakeDoor.unwire(m['body'] as String),
    ];

    testWidgets('the own profile goes to chats once per change, never to a request', (tester) async {
      final (door, server) = await online(tester);
      expect(profilesSent(server), hasLength(1));
      expect(profilesSent(server).single['chat'], 'c1', reason: 'not to the stranger');
      expect(door.shared!['name'], 'Julian');
      expect(door.shared!['badges'], isEmpty, reason: 'the core adds the badges from the vault');
      final first = door.shared!['rev'] as int;

      // Something else changes: nothing new goes out.
      final strictSwitch = _switch('Strenger Modus');
      await tester.ensureVisible(strictSwitch);
      await tester.pumpAndSettle();
      await tester.tap(strictSwitch);
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();
      expect(profilesSent(server), hasLength(1));

      // The name changes: a newer version goes to Bob.
      final name = find.byWidgetPredicate((w) => w is TextField && w.controller?.text == 'Julian');
      await tester.ensureVisible(name);
      await tester.pumpAndSettle();
      await tester.enterText(name, 'Jules');
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();
      expect(profilesSent(server), hasLength(2));
      expect(profilesSent(server).last['name'], 'Jules');
      expect(door.shared!['rev'] as int, greaterThan(first));
      expect(door.profileSent['c1'], door.shared!['rev']);
    });

    testWidgets("a partner's picture, colour and badges show; your name for them stays the big one", (tester) async {
      final (door, server) = await online(tester);
      await tester.tap(find.text('Chats'));
      await tester.pumpAndSettle();
      for (final (user, name) in [('local-bob', 'Robert'), ('local-fremd', 'Trair Support')]) {
        server.deliver(
          FakeDoor.wire({
            'kind': 'profile',
            'user': user,
            'profile': {
              'rev': 9,
              'name': name,
              'glow': 0xFF5EE6B0,
              'about': 'Ich mag Züge.',
              'picture': dot,
              'badges': [
                {'kind': 'founder', 'community': 'Bobs Runde'},
              ],
              'name_warning': false,
            },
          }),
        );
        await tester.pumpAndSettle();
      }
      expect(door.peerProfiles, hasLength(2));
      // Only Bob's picture: nothing from a stranger before accepting.
      expect(find.byWidgetPredicate((w) => w is Avatar && w.picture != null), findsOneWidget);
      expect(find.text('Robert'), findsNothing, reason: 'Bob stays Bob in the list');
      expect(find.text('Trair Support'), findsNothing);

      await tester.tap(find.text('Bob').first);
      await tester.pumpAndSettle();
      expect(find.text('nennt sich „Robert“ · local-bob'), findsOneWidget);
      expect(find.bySemanticsLabel(RegExp('Abzeichen: ')), findsWidgets);
      await tester.tap(find.text('nennt sich „Robert“ · local-bob'));
      await tester.pumpAndSettle();
      expect(find.text('Ich mag Züge.'), findsOneWidget);
      expect(find.text('nennt sich „Robert“'), findsOneWidget);
      expect(find.text('Trust'), findsNothing, reason: 'no made-up trust level for real people');
      await tester.tap(find.byTooltip('Schließen').last);
      await tester.pumpAndSettle();

      // The request: no card, no chosen name.
      await tester.tap(find.text('local-fremd').first);
      await tester.pumpAndSettle();
      expect(find.textContaining('nennt sich'), findsNothing);
    });
  });

  group('Freundschaftsschlüssel und verschwindende Chats', () {
    Future<(FakeDoor, FakeServer)> online(WidgetTester tester, {bool chat = false}) async {
      final server = FakeServer();
      final door = FakeDoor(password: pw, profile: {'name': 'Julian'})..chatsEnabled = true;
      if (chat) {
        door.contacts.add({'user': 'local-bob', 'name': 'Bob', 'fingerprint': 'ff' * 16, 'confirmed': false});
        door.chats.add({'id': 'c1', 'peer': 'local-bob', 'name': 'Bob', 'state': 'active', 'last': 0});
      }
      await _start(tester, door: door, server: server);
      await _unlock(tester);
      await _openSettings(tester);
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Chats'));
      await tester.pumpAndSettle();
      return (door, server);
    }

    String kindOf(Map<String, Object?> sent) => FakeDoor.unwire(sent['body'] as String)['kind'] as String;

    testWidgets('a key with a chosen time; using one starts the chat at once', (tester) async {
      final (door, server) = await online(tester);
      await tester.tap(find.byTooltip('Neuer Chat'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, '10 Min.'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Schlüssel erstellen'));
      await tester.pumpAndSettle();
      expect(door.invites.single, {'minutes': 10, 'vanishing': false});
      expect(find.textContaining('Einmal nutzbar · gilt bis heute um'), findsOneWidget);
      expect(find.textContaining('trair2:local-julian:'), findsOneWidget);

      // Too late: refused before the core even sees it.
      final past = DateTime.now().millisecondsSinceEpoch ~/ 1000 - 5;
      await tester.enterText(_field('trair1:…'), 'trair2:local-bob:${'ff' * 16}:${'11' * 16}:f$past:0000');
      await tester.tap(find.widgetWithText(PillButton, 'Hinzufügen'));
      await tester.pumpAndSettle();
      expect(find.text('Dieser Freundschaftsschlüssel ist abgelaufen.'), findsOneWidget);
      expect(door.contacts, isEmpty);

      final soon = DateTime.now().millisecondsSinceEpoch ~/ 1000 + 600;
      await tester.enterText(
        find.byWidgetPredicate((w) => w is TextField && (w.controller?.text.startsWith('trair2:') ?? false)),
        'trair2:local-bob:${'ff' * 16}:${'11' * 16}:f$soon:0000',
      );
      await tester.tap(find.widgetWithText(PillButton, 'Hinzufügen'));
      await tester.pumpAndSettle();
      expect(door.contacts.single['user'], 'local-bob');
      expect(server.commits, hasLength(1), reason: 'the chat started by itself');
      expect(server.sent.map(kindOf), contains('invite'), reason: 'the token went into the chat');
      expect(find.text('Echt Ende-zu-Ende verschlüsselt'), findsWidgets);
    });

    testWidgets('a chat vanishes for both after the chosen time and then is gone', (tester) async {
      final (door, server) = await online(tester, chat: true);
      await tester.tap(find.text('Bob').first);
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('Chat verschwinden lassen'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('1 Tag'));
      await tester.pumpAndSettle();
      expect(kindOf(server.sent.last), 'ends', reason: 'Bob learns it too');
      expect(find.textContaining(RegExp(r'Verschwindet in 2[34] Std\.')), findsWidgets);

      // Time is up: the next sweep takes the chat away.
      door.chats.single['ends'] = DateTime.now().millisecondsSinceEpoch ~/ 1000 - 1;
      await tester.pump(const Duration(seconds: 31));
      await tester.pumpAndSettle();
      expect(door.chats, isEmpty);
      expect(find.text('Bob'), findsNothing);
    });
  });

  group('Gruppen', () {
    Future<(FakeDoor, FakeServer)> online(WidgetTester tester, {void Function(FakeDoor)? setup}) async {
      final server = FakeServer();
      final door = FakeDoor(password: pw, profile: {'name': 'Julian'})..chatsEnabled = true;
      door.contacts.add({'user': 'local-bob', 'name': 'Bob', 'fingerprint': 'ff' * 16, 'confirmed': false});
      setup?.call(door);
      await _start(tester, door: door, server: server);
      await _unlock(tester);
      await _openSettings(tester);
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Verbinden'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Chats'));
      await tester.pumpAndSettle();
      return (door, server);
    }

    String kindOf(Map<String, Object?> sent) => FakeDoor.unwire(sent['body'] as String)['kind'] as String;

    /// Bob's group "Band" with Julian as a plain member.
    void bobsBand(FakeDoor door) {
      door.groups['g9'] = {
        'name': 'Band',
        'owner': 'local-bob',
        'admins': <String>[],
        'guests': <String, int>{},
        'members': ['local-bob', 'local-julian'],
        'ends': null,
        'changed': <String>[],
        'epoch': 3,
      };
      door.chats.add({
        'id': 'g9',
        'peer': '',
        'kind': 'group',
        'name': 'Band',
        'state': 'active',
        'role': 'member',
        'members': 2,
        'last': 0,
      });
    }

    testWidgets('a group with a contact: made, roles shown, a key for guests', (tester) async {
      final (door, server) = await online(tester);
      await tester.tap(find.byTooltip('Neue Gruppe'));
      await tester.pumpAndSettle();
      await tester.ensureVisible(_field('Gruppenname'));
      await tester.enterText(_field('Gruppenname'), 'Abi 2026');
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Bob'));
      await tester.tap(find.widgetWithText(PillButton, 'Bob'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Gruppe erstellen'));
      await tester.pumpAndSettle();
      expect(door.groups.values.single['members'], ['local-julian', 'local-bob']);
      expect(server.commits.single['committers'], ['local-julian'], reason: 'the server learns who may change it');
      expect(server.commits.single['welcome_to'], [
        {'user': 'local-bob', 'device': 'd-peer'},
      ]);
      // The group column is open: members with their roles, the history.
      expect(find.text('MITGLIEDER · 2'), findsOneWidget);
      expect(find.text('Besitzer'), findsOneWidget);
      expect(find.text('Du hast die Gruppe erstellt'), findsOneWidget);
      expect(find.text('Abi 2026'), findsWidgets);

      // A key for guests, valid for a day.
      await tester.ensureVisible(find.byWidgetPredicate((w) => w is PillSwitch && w.label == 'Als Gäste einladen'));
      await tester.tap(find.byWidgetPredicate((w) => w is PillSwitch && w.label == 'Als Gäste einladen'));
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.widgetWithText(PillButton, 'Einladung erstellen'));
      await tester.tap(find.widgetWithText(PillButton, 'Einladung erstellen'));
      await tester.pumpAndSettle();
      expect(door.groupInvites.single, {'chat': 'g1', 'minutes': 1440, 'guest': true});
      expect(find.textContaining('trair3:local-julian:'), findsOneWidget);
      expect(find.textContaining('dann gehen sie'), findsOneWidget);

      // Bob becomes admin: the server learns it with the change.
      await tester.ensureVisible(find.byTooltip('Mitglied verwalten'));
      await tester.tap(find.byTooltip('Mitglied verwalten'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Zum Admin machen'));
      await tester.pumpAndSettle();
      expect(server.commits.last['committers'], ['local-julian', 'local-bob']);
      expect(find.text('Admin'), findsOneWidget);
    });

    testWidgets('a pasted group key asks the admin and waits; an expired one is refused', (tester) async {
      final (door, server) = await online(tester);
      await tester.tap(find.byTooltip('Neuer Chat'));
      await tester.pumpAndSettle();
      final past = DateTime.now().millisecondsSinceEpoch ~/ 1000 - 5;
      await tester.enterText(_field('trair1:…'), 'trair3:local-olga:${'ff' * 16}:${'11' * 16}:m$past:0000');
      await tester.tap(find.widgetWithText(PillButton, 'Hinzufügen'));
      await tester.pumpAndSettle();
      expect(find.text('Dieser Gruppenschlüssel ist abgelaufen.'), findsOneWidget);
      expect(server.commits, isEmpty);

      final soon = DateTime.now().millisecondsSinceEpoch ~/ 1000 + 600;
      await tester.enterText(
        find.byWidgetPredicate((w) => w is TextField && (w.controller?.text.startsWith('trair3:') ?? false)),
        'trair3:local-olga:${'ff' * 16}:${'11' * 16}:m$soon:0000',
      );
      await tester.tap(find.widgetWithText(PillButton, 'Hinzufügen'));
      await tester.pumpAndSettle();
      expect(server.commits.single['committers'], ['local-julian', 'local-olga']);
      expect(server.sent.map(kindOf), contains('join'), reason: 'the token went to the admin');
      expect(door.contacts.map((c) => c['user']), isNot(contains('local-olga')), reason: 'no contact from a group key');
      expect(find.text('Beitritt: wartet auf local-olga'), findsOneWidget);
      expect(find.textContaining('Beitritt angefragt'), findsOneWidget);
      expect(find.byKey(const Key('real-composer')), findsNothing);
    });

    testWidgets("an admin's Trair lets in whoever brings a valid key", (tester) async {
      final (door, server) = await online(
        tester,
        setup: (d) {
          d.groups['g1'] = {
            'name': 'Party',
            'owner': 'local-julian',
            'admins': <String>[],
            'guests': <String, int>{},
            'members': ['local-julian'],
            'ends': null,
            'changed': <String>[],
            'epoch': 1,
          };
          d.chats.add({'id': 'g1', 'peer': '', 'kind': 'group', 'name': 'Party', 'state': 'active', 'last': 0});
        },
      );
      server.deliver(FakeDoor.wire({'kind': 'join_request', 'group': 'g1', 'user': 'local-gus'}));
      await tester.pumpAndSettle();
      expect(door.ops, containsAll(['admit', 'close_join']));
      expect(door.joinRequests, isEmpty);
      expect(server.commits.last['welcome_to'], [
        {'user': 'local-gus', 'device': 'd-peer'},
      ]);
      expect(server.commits.last['committers'], ['local-julian']);
      expect(door.groups['g1']!['members'], ['local-julian', 'local-gus']);
      expect(find.text('2 Mitglieder · deine'), findsOneWidget);
    });

    testWidgets('a member sees names and notes, has no admin buttons, and can leave', (tester) async {
      final (door, server) = await online(tester, setup: bobsBand);
      server.deliver(
        FakeDoor.wire({'kind': 'group_message', 'chat': 'g9', 'user': 'local-bob', 'text': 'Probe um acht'}),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Band'));
      await tester.pumpAndSettle();
      expect(find.text('Probe um acht'), findsOneWidget);
      expect(find.text('Bob'), findsWidgets, reason: 'the sender stands above the bubble');
      await tester.tap(find.byTooltip('Gruppe verwalten'));
      await tester.pumpAndSettle();
      expect(find.text('MITGLIEDER · 2'), findsOneWidget);
      expect(find.widgetWithText(PillButton, 'Einladung erstellen'), findsNothing);
      expect(find.byTooltip('Gruppe umbenennen'), findsNothing);
      expect(find.byTooltip('Mitglied verwalten'), findsNothing);

      await tester.ensureVisible(find.widgetWithText(PillButton, 'Gruppe verlassen'));
      await tester.tap(find.widgetWithText(PillButton, 'Gruppe verlassen'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(PillButton, 'Verlassen'));
      await tester.pumpAndSettle();
      expect(door.ops, containsAll(['leave_prepare', 'leave_group']));
      expect(server.sent.map(kindOf), contains('leave'));
      expect(find.text('Du hast die Gruppe verlassen'), findsOneWidget);
      expect(find.textContaining('Du bist nicht mehr in dieser Gruppe'), findsOneWidget);
      await tester.tap(find.widgetWithText(PillButton, 'Chat löschen'));
      await tester.pumpAndSettle();
      expect(door.chats.where((c) => c['id'] == 'g9'), isEmpty);
      expect(find.text('Band'), findsNothing);
    });

    testWidgets("strict mode stops a group where someone's key changed until it is checked", (tester) async {
      final (door, server) = await online(tester, setup: bobsBand);
      server.deliver(FakeDoor.wire({'kind': 'key_changed', 'chat': 'g9', 'user': 'local-bob'}));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Band'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Der Schlüssel von Bob hat sich geändert'), findsOneWidget);
      await tester.tap(find.byTooltip('Gruppe verwalten'));
      await tester.pumpAndSettle();
      expect(find.text('Schlüssel geändert: erst prüfen'), findsOneWidget);
      await tester.tap(find.byTooltip('Mitglied verwalten'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Neuen Schlüssel annehmen'));
      await tester.pumpAndSettle();
      expect(door.ops, contains('accept_new_key'));
      expect(find.textContaining('Der Schlüssel von Bob hat sich geändert'), findsNothing);
    });
  });
}
