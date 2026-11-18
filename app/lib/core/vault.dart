// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:io';

import 'door.dart';

/// Answer of the core, see crates/trair-ffi/src/api.rs.
class CoreAnswer {
  CoreAnswer(this.raw);

  final Map<String, Object?> raw;

  bool get ok => raw['ok'] == true;
  String get error => raw['error'] as String? ?? '';

  /// Seconds to wait before the next attempt (after wrong passwords).
  int get wait => (raw['wait'] ?? raw['seconds']) as int? ?? 0;

  String get fingerprint => raw['fingerprint'] as String? ?? '';

  Map<String, Object?> get profile {
    final p = raw['profile'];
    return p is Map<String, Object?> ? p : const {};
  }
}

/// The vault as the app sees it: typed calls through the [CoreDoor]. The
/// keys never come out; the app only learns results.
class TrairVault {
  TrairVault(this._door);

  final CoreDoor _door;

  Future<CoreAnswer> _call(Map<String, Object?> request) async => CoreAnswer(await _door.call(request));

  /// Open the data folder. Answer has `exists` and `unlocked`.
  Future<CoreAnswer> init([String? dir]) => _call({'op': 'init', 'dir': dir ?? dataDir()});

  Future<CoreAnswer> create(String password, Map<String, Object?> profile) =>
      _call({'op': 'create', 'password': password, 'profile': profile});

  Future<CoreAnswer> unlock(String password) => _call({'op': 'unlock', 'password': password});

  Future<CoreAnswer> lock() => _call({'op': 'lock'});

  Future<CoreAnswer> saveProfile(Map<String, Object?> profile) => _call({'op': 'save_profile', 'profile': profile});

  Future<CoreAnswer> changePassword(String old, String fresh) =>
      _call({'op': 'change_password', 'old': old, 'new': fresh});

  Future<CoreAnswer> deleteAccount(String password) => _call({'op': 'delete_account', 'password': password});

  /// Hide the window from screenshots. Returns false where unsupported.
  Future<bool> protectWindow(bool on) async => (await _call({'op': 'protect_window', 'on': on})).ok;

  /// Where the vault lives: %APPDATA%\Trair on Windows.
  static String dataDir() {
    final env = Platform.environment;
    final override = env['TRAIR_DATA_DIR'];
    if (override != null && override.isNotEmpty) return override;
    if (Platform.isWindows) {
      final base = env['APPDATA'] ?? env['LOCALAPPDATA'] ?? '';
      return '$base\\Trair';
    }
    final home = env['HOME'] ?? '';
    if (Platform.isMacOS) return '$home/Library/Application Support/Trair';
    final xdg = env['XDG_DATA_HOME'];
    return '${xdg != null && xdg.isNotEmpty ? xdg : '$home/.local/share'}/trair';
  }
}

/// Text for the user, by error code of the core.
String vaultErrorText(CoreAnswer a) {
  switch (a.error) {
    case 'wrong_password':
      return a.wait > 0
          ? 'Passwort falsch. Zu viele Versuche: bitte ${a.wait} Sekunden warten.'
          : 'Passwort falsch.';
    case 'wait':
      return 'Zu viele Versuche. Bitte noch ${a.wait} Sekunden warten.';
    case 'weak_password':
      return 'Das Passwort braucht mindestens 12 Zeichen.';
    case 'damaged':
      return 'Der Tresor ist beschädigt oder gehört zu einem anderen Windows-Konto.';
    case 'refused':
      return 'Der Tresor hat unsichere Einstellungen und wird nicht geöffnet.';
    case 'exists':
      return 'Auf diesem PC gibt es schon ein Trair-Konto.';
    case 'io':
      return 'Der Tresor konnte nicht gespeichert werden.';
    case 'too_large':
      return 'Das Profil ist zu groß.';
    default:
      return 'Unerwarteter Fehler im Sicherheitskern (${a.error}).';
  }
}
