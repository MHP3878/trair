// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:flutter/widgets.dart';

import 'backend/backend.dart';
import 'theme.dart';

enum TrairScreen { lock, onboarding, chats, community, profile }

/// App-wide state. Holds no secrets: in the real app all keys stay in the
/// Rust core and only results reach this layer.
class AppState extends ChangeNotifier {
  AppState(this.backend);

  final TrairBackend backend;

  TrairScreen screen = TrairScreen.lock;
  String userName = 'Julian';
  Color glow = TrairColors.accentLight;
  double glowStrength = 35;
  bool strict = true;

  /// Privacy switches; all off by default (decision 10).
  final Map<String, bool> privacy = {'lese': false, 'tippt': false, 'online': false, 'vorschau': false};

  void go(TrairScreen next) {
    screen = next;
    notifyListeners();
  }

  void lock() => go(TrairScreen.lock);

  void finishOnboarding({required String name, required Color glowColor}) {
    userName = name.trim().isEmpty ? userName : name.trim();
    glow = glowColor;
    go(TrairScreen.chats);
  }

  void setGlow(Color c) {
    glow = c;
    notifyListeners();
  }

  void setGlowStrength(double v) {
    glowStrength = v;
    notifyListeners();
  }

  void setName(String name) {
    userName = name;
    notifyListeners();
  }

  void setStrict(bool on) {
    strict = on;
    notifyListeners();
  }

  void setPrivacy(String key, bool on) {
    privacy[key] = on;
    notifyListeners();
  }

  /// Something in the backend changed (message sent, number confirmed).
  void touch() => notifyListeners();
}

/// Makes [AppState] available to every widget below it.
class AppScope extends InheritedNotifier<AppState> {
  const AppScope({super.key, required AppState state, required super.child}) : super(notifier: state);

  static AppState of(BuildContext context) {
    final scope = context.dependOnInheritedWidgetOfExactType<AppScope>();
    assert(scope != null, 'AppScope missing');
    return scope!.notifier!;
  }
}
