// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'app_state.dart';
import 'backend/backend.dart';
import 'backend/demo_backend.dart';
import 'screens/chats_screen.dart';
import 'screens/community_screen.dart';
import 'screens/lock_screen.dart';
import 'screens/onboarding_screen.dart';
import 'screens/profile_screen.dart';
import 'theme.dart';
import 'widgets/common.dart';

void main() {
  // The fonts are bundled files, not Dart packages, so Flutter does not
  // list their license by itself. The OFL requires that it travels along.
  LicenseRegistry.addLicense(() async* {
    final text = await rootBundle.loadString('assets/fonts/OFL.txt');
    yield LicenseEntryWithLineBreaks(const ['Geist', 'Geist Mono'], text);
  });
  runApp(TrairApp(backend: DemoBackend()));
}

class TrairApp extends StatefulWidget {
  const TrairApp({super.key, required this.backend});

  final TrairBackend backend;

  @override
  State<TrairApp> createState() => _TrairAppState();
}

class _TrairAppState extends State<TrairApp> {
  late final AppState _state = AppState(widget.backend);

  @override
  void dispose() {
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
          return Column(
            children: [
              if (_state.backend.isDemo) const DemoBar(),
              Expanded(child: MinSizeScaler(child: child ?? const SizedBox.shrink())),
            ],
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
