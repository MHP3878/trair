// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

/// Several accounts on one computer. Each account has its own folder with
/// its own vault and chat database: the data folder itself is account 1,
/// further accounts live in `konten/2`, `konten/3` and so on.
///
/// The start screen shows the accounts before any password is entered, so
/// what it shows (name, colour, picture) lies unencrypted in `karte.json`
/// and `karte.png` of each folder. An account can hide itself there: then
/// only "Konto N" is shown and nothing else is stored outside the vault.
///
/// Only synchronous file calls: they are small, and widget tests run them
/// without real time passing.
class Accounts {
  Accounts(this.base);

  /// The data folder (account 1).
  final String base;

  static const folder = 'konten';
  static const cardFile = 'karte.json';
  static const pictureFile = 'karte.png';
  static const vaultFile = 'vault.trair';

  static String join(String a, String b) => '$a${Platform.pathSeparator}$b';

  String get _more => join(base, folder);

  bool _hasVault(String dir) => File(join(dir, vaultFile)).existsSync();

  /// Number of an account folder: 1 for the data folder, N for konten/N.
  int? _number(String dir) {
    if (dir == base) return 1;
    final name = dir.split(Platform.pathSeparator).last;
    final n = int.tryParse(name);
    return n != null && n >= 2 && join(_more, name) == dir ? n : null;
  }

  List<String> _folders() {
    final more = Directory(_more);
    if (!more.existsSync()) return const [];
    final out = <String>[];
    for (final e in more.listSync()) {
      if (e is Directory && _number(e.path) != null) out.add(e.path);
    }
    out.sort((a, b) => _number(a)!.compareTo(_number(b)!));
    return out;
  }

  /// Every account that has a vault, account 1 first. [baseExists] is
  /// the core's answer for the data folder, if it was asked.
  List<AccountCard> list({bool? baseExists}) => [
    if (baseExists ?? _hasVault(base)) read(base),
    for (final dir in _folders())
      if (_hasVault(dir)) read(dir),
  ];

  /// What the start screen shows for an account folder.
  AccountCard read(String dir) {
    final number = _number(dir) ?? 0;
    var name = '';
    int? glow;
    var hidden = false;
    Uint8List? picture;
    try {
      final j = jsonDecode(File(join(dir, cardFile)).readAsStringSync());
      if (j is Map<String, Object?>) {
        hidden = j['hidden'] == true;
        if (!hidden) {
          final n = j['name'];
          name = n is String ? n.characters64() : '';
          final g = j['glow'];
          glow = g is int ? g : null;
        }
      }
    } on Object {
      // No card (yet) or an unreadable one: "Konto N".
    }
    if (!hidden) {
      final f = File(join(dir, pictureFile));
      // A picture the app wrote is small; anything larger is ignored.
      if (f.existsSync() && f.lengthSync() <= 512 * 1024) picture = f.readAsBytesSync();
    }
    return AccountCard(dir: dir, number: number, name: name, glow: glow, picture: picture, hidden: hidden);
  }

  /// A folder for a new account: the next free number.
  String newDir() {
    final used = _folders().map((d) => _number(d)!).toList();
    var n = 2;
    while (used.contains(n)) {
      n++;
    }
    final dir = join(_more, '$n');
    Directory(dir).createSync(recursive: true);
    return dir;
  }

  /// Store what the start screen shows. With [hidden] nothing about the
  /// person stays outside the vault.
  void writeCard(String dir, {required String name, required int glow, Uint8List? picture, required bool hidden}) {
    final card = File(join(dir, cardFile));
    final pic = File(join(dir, pictureFile));
    if (hidden) {
      card.writeAsStringSync(jsonEncode({'v': 1, 'hidden': true}));
      if (pic.existsSync()) pic.deleteSync();
      return;
    }
    card.writeAsStringSync(jsonEncode({'v': 1, 'name': name, 'glow': glow}));
    if (picture != null) {
      pic.writeAsBytesSync(picture);
    } else if (pic.existsSync()) {
      pic.deleteSync();
    }
  }

  /// Forget what the start screen showed (the account was deleted).
  void removeCard(String dir) {
    for (final name in [cardFile, pictureFile]) {
      final f = File(join(dir, name));
      if (f.existsSync()) f.deleteSync();
    }
  }

  /// Remove folders of further accounts that have no vault (a new account
  /// that was never finished, or a deleted one). A folder another Trair
  /// window still holds stays.
  void removeEmpty({String? except}) {
    for (final dir in _folders()) {
      if (dir == except || _hasVault(dir)) continue;
      try {
        Directory(dir).deleteSync(recursive: true);
      } on FileSystemException {
        // Still open in another window.
      }
    }
  }
}

extension on String {
  /// At most 64 characters, as the profile allows.
  String characters64() => runes.length <= 64 ? this : String.fromCharCodes(runes.take(64));
}

/// One account as the start screen shows it.
class AccountCard {
  const AccountCard({
    required this.dir,
    required this.number,
    this.name = '',
    this.glow,
    this.picture,
    this.hidden = false,
  });

  final String dir;

  /// 1 for the data folder, N for konten/N.
  final int number;
  final String name;
  final int? glow;
  final Uint8List? picture;

  /// The account chose to show only "Konto N".
  final bool hidden;

  String get title => name.isEmpty ? 'Konto $number' : name;
}
