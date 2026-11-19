// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'app_state.dart';
import 'backend/backend.dart';
import 'backend/demo_backend.dart';
import 'core/door.dart';
import 'core/server.dart';
import 'core/vault.dart';
import 'screens/chats_screen.dart';
import 'screens/community_screen.dart';
import 'screens/lock_screen.dart';
import 'screens/onboarding_screen.dart';
import 'screens/profile_screen.dart';
import 'screens/start_screen.dart';
import 'theme.dart';
import 'widgets/common.dart';

void main() {
  // The fonts and icons are bundled files, not Dart packages, so Flutter
  // does not list their licenses by itself. Both licenses require that they
  // travel along.
  LicenseRegistry.addLicense(() async* {
    final text = await rootBundle.loadString('assets/fonts/OFL.txt');
    yield LicenseEntryWithLineBreaks(const ['Geist', 'Geist Mono'], text);
    final icons = await rootBundle.loadString('assets/fonts/LUCIDE-LICENSE.txt');
    yield LicenseEntryWithLineBreaks(const ['Lucide'], icons);
  });
  TrairVault? vault;
  var problem = '';
  try {
    vault = TrairVault(NativeDoor.load());
  } on CoreMissing catch (e) {
    problem = e.message;
  }
  runApp(TrairApp(backend: DemoBackend(), vault: vault, startupError: problem));
}

class TrairApp extends StatefulWidget {
  const TrairApp({
    super.key,
    required this.backend,
    required this.vault,
    this.startupError = '',
    this.transport,
    this.accountsBase,
    this.pickPicture,
  });

  final TrairBackend backend;

  /// How to reach the server; null means real HTTP. Tests pass a fake.
  final ServerTransport Function(Uri)? transport;

  /// The security core. Null if it could not be loaded: the app then only
  /// shows [startupError] and never falls back to demo data.
  final TrairVault? vault;
  final String startupError;

  /// Folder with the accounts; null means the real data folder. Tests
  /// pass a temporary one.
  final String? accountsBase;

  /// Choose a picture; null means the Windows dialog. Tests pass bytes.
  final Future<Uint8List?> Function()? pickPicture;

  @override
  State<TrairApp> createState() => _TrairAppState();
}

class _TrairAppState extends State<TrairApp> {
  late final AppState _state = AppState(
    widget.backend,
    widget.vault,
    transport: widget.transport,
    accountsBase: widget.accountsBase,
    pickPicture: widget.pickPicture,
  )..startupError = widget.startupError;

  @override
  void initState() {
    super.initState();
    HardwareKeyboard.instance.addHandler(_onKey);
    unawaited(_state.start());
  }

  /// Every key counts as activity; Ctrl+L locks at once, wherever the
  /// focus is.
  bool _onKey(KeyEvent event) {
    _state.noteActivity();
    final control = HardwareKeyboard.instance.isControlPressed;
    if (event is KeyDownEvent && control && event.logicalKey == LogicalKeyboardKey.keyL && _state.unlocked) {
      unawaited(_state.lock());
      return true;
    }
    return false;
  }

  @override
  void dispose() {
    HardwareKeyboard.instance.removeHandler(_onKey);
    _state.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AppScope(
      state: _state,
      child: MaterialApp(
        title: 'Trair',
        debugShowCheckedModeBanner: false,
        theme: trairTheme(),
        home: const _Router(),
        builder: (context, child) {
          return Listener(
            behavior: HitTestBehavior.translucent,
            onPointerDown: (_) => _state.noteActivity(),
            onPointerSignal: (_) => _state.noteActivity(),
            child: Column(
              children: [
                if (_state.backend.isDemo) const DemoBar(),
                Expanded(child: MinSizeScaler(child: child ?? const SizedBox.shrink())),
              ],
            ),
          );
        },
      ),
    );
  }
}

class _Router extends StatelessWidget {
  const _Router();

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final Widget page = switch (state.screen) {
      TrairScreen.starting => const StartScreen(),
      TrairScreen.coreMissing => const CoreMissingScreen(),
      TrairScreen.lock => const LockScreen(),
      TrairScreen.onboarding => const OnboardingScreen(),
      TrairScreen.chats => const ChatsScreen(),
      TrairScreen.community => const CommunityScreen(),
      TrairScreen.profile => const ProfileScreen(),
    };
    return Scaffold(
      body: AnimatedSwitcher(
        duration: const Duration(milliseconds: 260),
        child: KeyedSubtree(key: ValueKey(state.screen), child: page),
      ),
    );
  }
}
