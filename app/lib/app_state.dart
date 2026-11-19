// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';
import 'dart:convert';

import 'package:flutter/widgets.dart';

import 'backend/backend.dart';
import 'core/vault.dart';
import 'theme.dart';

enum TrairScreen { starting, coreMissing, lock, onboarding, chats, community, profile }

/// Minutes without use before Trair locks itself.
const autoLockChoices = [5, 15, 30];

/// Longest "Über mich", in characters (like Discord).
const maxAboutChars = 190;

/// App-wide state. Holds no secrets: the keys stay in the Rust core
/// behind [TrairVault]; this layer only gets the profile and results.
class AppState extends ChangeNotifier {
  AppState(this.backend, this.vault);

  final TrairBackend backend;

  /// Null when the security core could not be loaded.
  final TrairVault? vault;

  TrairScreen screen = TrairScreen.starting;
  String startupError = '';
  bool screenProtectionSupported = true;

  // The profile. Stored encrypted in the vault and kept here only while
  // Trair is unlocked; locking resets it to these defaults.
  String userName = '';
  Color glow = TrairColors.accentLight;
  double glowStrength = 35;
  bool strict = true;
  int autoLockMinutes = 15;
  bool screenProtection = true;
  String fingerprint = '';

  /// "Über mich" of the own profile.
  String about = '';

  /// Who can see the own profile: alle, gemeinsam or kontakte. Takes effect
  /// with the server (M3); stored now.
  String profileVisibility = 'gemeinsam';

  /// The account's devices, from the vault. Empty while locked.
  List<TrairDevice> devices = const [];

  /// Public account key in hex, for the team tool. Not secret.
  String accountKey = '';

  /// Own badges from the vault, checked by the core.
  List<ProfileBadge> realBadges = const [];

  /// Own badges to show: the real ones, plus community badges the demo
  /// still makes up. A team badge is only ever real.
  List<ProfileBadge> get ownBadges {
    final shown = [...realBadges];
    for (final b in backend.ownBadges()) {
      final known = shown.any((r) => r.kind == b.kind && r.community == b.community);
      if (b.kind != BadgeKind.team && !known) shown.add(b);
    }
    shown.sort((a, b) => a.kind.index.compareTo(b.kind.index));
    return shown;
  }

  /// True if some of [ownBadges] are only demo.
  bool get ownBadgesIncludeDemo => ownBadges.length > realBadges.length;

  /// Privacy switches; all off by default (decision 10).
  final Map<String, bool> privacy = {'lese': false, 'tippt': false, 'online': false, 'vorschau': false};

  Timer? _autoLock;
  Timer? _saveSoon;

  bool get unlocked => const {TrairScreen.chats, TrairScreen.community, TrairScreen.profile}.contains(screen);

  /// Open the vault folder, then show the first start or the lock screen.
  Future<void> start() async {
    final v = vault;
    if (v == null) {
      go(TrairScreen.coreMissing);
      return;
    }
    final status = await v.init();
    if (!status.ok) {
      startupError = vaultErrorText(status);
      go(TrairScreen.coreMissing);
      return;
    }
    screenProtectionSupported = await v.protectWindow(true);
    go(status.raw['exists'] == true ? TrairScreen.lock : TrairScreen.onboarding);
  }

  void go(TrairScreen next) {
    screen = next;
    notifyListeners();
  }

  /// Create the account. On success Trair is unlocked.
  Future<CoreAnswer> createAccount({required String name, required Color glowColor, required String password}) async {
    userName = name.trim().isEmpty ? 'Ich' : name.trim();
    glow = glowColor;
    final answer = await vault!.create(password, _profileJson());
    if (answer.ok) _unlocked(answer);
    return answer;
  }

  Future<CoreAnswer> unlock(String password) async {
    final answer = await vault!.unlock(password);
    if (answer.ok) _unlocked(answer);
    return answer;
  }

  void _unlocked(CoreAnswer answer) {
    _applyProfile(answer.profile);
    fingerprint = answer.fingerprint;
    devices = answer.devices;
    accountKey = answer.accountKey;
    realBadges = answer.badges;
    go(TrairScreen.chats);
    noteActivity();
    unawaited(_applyScreenProtection());
  }

  /// Lock now: the core forgets the key and the profile leaves memory.
  Future<void> lock() async {
    if (!unlocked) return;
    _autoLock?.cancel();
    await _flushSave();
    await vault?.lock();
    _applyProfile(const {});
    fingerprint = '';
    devices = const [];
    accountKey = '';
    realBadges = const [];
    go(TrairScreen.lock);
    // The lock screen is always protected, whatever the setting says.
    screenProtectionSupported = await vault?.protectWindow(true) ?? false;
  }

  /// Pointer or key activity: restart the auto-lock timer.
  void noteActivity() {
    _autoLock?.cancel();
    if (!unlocked) return;
    _autoLock = Timer(Duration(minutes: autoLockMinutes), () => unawaited(lock()));
  }

  Future<CoreAnswer> changePassword(String old, String fresh) => vault!.changePassword(old, fresh);

  /// Delete the account for good. On success the first start follows.
  Future<CoreAnswer> deleteAccount(String password) async {
    final answer = await vault!.deleteAccount(password);
    if (answer.ok) {
      _autoLock?.cancel();
      _saveSoon?.cancel();
      _applyProfile(const {});
      fingerprint = '';
      devices = const [];
      accountKey = '';
      realBadges = const [];
      go(TrairScreen.onboarding);
    }
    return answer;
  }

  /// Name a device. The name stays on this device, never in the list
  /// other people see.
  Future<CoreAnswer> renameDevice(String id, String label) => _devices(vault!.renameDevice(id, label.trim()));

  /// Withdraw another device for good. Needs the password again.
  Future<CoreAnswer> withdrawDevice(String id, String password) => _devices(vault!.withdrawDevice(id, password));

  /// Keep a signed badge (pasted as JSON, for example from the team
  /// tool). The core checks it; only valid badges are stored.
  Future<CoreAnswer> addBadge(String json) async {
    Object? badge;
    try {
      badge = jsonDecode(json.trim());
    } on FormatException {
      return CoreAnswer({'ok': false, 'error': 'bad_request'});
    }
    final answer = await vault!.addBadge(badge);
    if (answer.ok) {
      realBadges = answer.badges;
      notifyListeners();
    }
    return answer;
  }

  Future<CoreAnswer> _devices(Future<CoreAnswer> call) async {
    final answer = await call;
    if (answer.ok) {
      devices = answer.devices;
      notifyListeners();
    }
    return answer;
  }

  void setGlow(Color c) => _change(() => glow = c);

  void setGlowStrength(double v) => _change(() => glowStrength = v);

  void setName(String name) => _change(() => userName = name);

  void setAbout(String text) => _change(() => about = text.characters.take(maxAboutChars).toString());

  void setProfileVisibility(String choice) => _change(() => profileVisibility = choice);

  void setStrict(bool on) => _change(() => strict = on);

  void setPrivacy(String key, bool on) => _change(() => privacy[key] = on);

  void setAutoLock(int minutes) => _change(() {
    autoLockMinutes = minutes;
    noteActivity();
  });

  void setScreenProtection(bool on) => _change(() {
    screenProtection = on;
    unawaited(_applyScreenProtection());
  });

  Future<void> _applyScreenProtection() async {
    screenProtectionSupported = await vault?.protectWindow(screenProtection) ?? false;
    notifyListeners();
  }

  /// Change a setting and store it, encrypted, shortly after.
  void _change(void Function() update) {
    update();
    notifyListeners();
    _saveSoon?.cancel();
    _saveSoon = Timer(const Duration(milliseconds: 600), () => unawaited(_save()));
  }

  Future<void> _flushSave() async {
    if (_saveSoon?.isActive ?? false) {
      _saveSoon!.cancel();
      await _save();
    }
  }

  Future<void> _save() async {
    if (!unlocked) return;
    await vault?.saveProfile(_profileJson());
  }

  Map<String, Object?> _profileJson() => {
    'v': 1,
    'name': userName,
    'glow': glow.toARGB32(),
    'glowStrength': glowStrength,
    'strict': strict,
    'autoLockMinutes': autoLockMinutes,
    'screenProtection': screenProtection,
    'privacy': Map<String, bool>.of(privacy),
    'about': about,
    'profileVisibility': profileVisibility,
  };

  void _applyProfile(Map<String, Object?> p) {
    userName = p['name'] as String? ?? '';
    final glowValue = p['glow'];
    glow = glowValue is int ? Color(glowValue) : TrairColors.accentLight;
    final strength = p['glowStrength'];
    glowStrength = strength is num ? strength.toDouble().clamp(0.0, 100.0).toDouble() : 35;
    strict = p['strict'] as bool? ?? true;
    final minutes = p['autoLockMinutes'];
    autoLockMinutes = minutes is int && autoLockChoices.contains(minutes) ? minutes : 15;
    screenProtection = p['screenProtection'] as bool? ?? true;
    final storedAbout = p['about'];
    about = storedAbout is String ? storedAbout.characters.take(maxAboutChars).toString() : '';
    final visibility = p['profileVisibility'];
    profileVisibility = visibility is String && const {'alle', 'gemeinsam', 'kontakte'}.contains(visibility)
        ? visibility
        : 'gemeinsam';
    final storedPrivacy = p['privacy'];
    for (final key in privacy.keys) {
      privacy[key] = storedPrivacy is Map && storedPrivacy[key] == true;
    }
  }

  /// Something in the backend changed (message sent, number confirmed).
  void touch() => notifyListeners();

  @override
  void dispose() {
    _autoLock?.cancel();
    _saveSoon?.cancel();
    super.dispose();
  }
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
