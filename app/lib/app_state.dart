// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math' show max;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';

import 'backend/backend.dart';
import 'core/accounts.dart';
import 'core/server.dart';
import 'core/vault.dart';
import 'theme.dart';

enum TrairScreen { starting, coreMissing, lock, onboarding, chats, community, profile }

/// Connection to the Trair server.
enum ServerStatus { off, connecting, connected, failed }

/// Where a pairing stands, on the unlocked device.
enum PairStage { waiting, finishing, done, failed }

/// A contact: pinned account key, from a contact code (M4).
class RealContact {
  const RealContact({required this.user, required this.name, required this.fingerprint, this.confirmed = false});

  factory RealContact.fromJson(Map<String, Object?> j) => RealContact(
    user: j['user'] as String,
    name: j['name'] as String? ?? j['user'] as String,
    fingerprint: j['fingerprint'] as String? ?? '',
    confirmed: j['confirmed'] == true,
  );

  final String user;
  final String name;
  final String fingerprint;
  final bool confirmed;
}

/// A real, end-to-end encrypted one-to-one chat (M4).
class RealChat {
  const RealChat({required this.id, required this.peer, required this.name, required this.state, this.last = 0});

  factory RealChat.fromJson(Map<String, Object?> j) => RealChat(
    id: j['id'] as String,
    peer: j['peer'] as String,
    name: j['name'] as String? ?? j['peer'] as String,
    state: j['state'] as String? ?? 'active',
    last: (j['last'] as num?)?.toInt() ?? 0,
  );

  final String id;
  final String peer;
  final String name;

  /// active, request (someone who is no contact wrote first) or left.
  final String state;
  final int last;

  bool get isRequest => state == 'request';
}

/// What a chat partner sent about themselves, checked by the core.
class PeerProfile {
  const PeerProfile({
    required this.user,
    required this.rev,
    required this.name,
    this.glow,
    this.about = '',
    this.picture,
    this.badges = const [],
    this.nameWarning = false,
  });

  factory PeerProfile.fromJson(Map<String, Object?> j) => PeerProfile(
    user: j['user'] as String,
    rev: (j['rev'] as num).toInt(),
    name: j['name'] as String,
    glow: j['glow'] is num ? Color((j['glow'] as num).toInt()) : null,
    about: j['about'] as String? ?? '',
    picture: j['picture'] is String ? base64.decode(j['picture'] as String) : null,
    badges: [
      for (final b in (j['badges'] as List? ?? const []).whereType<Map<String, Object?>>()) ?ProfileBadge.fromJson(b),
    ],
    nameWarning: j['name_warning'] == true,
  );

  final String user;
  final int rev;

  /// The name they chose (yours for them stays the big one).
  final String name;
  final Color? glow;
  final String about;
  final Uint8List? picture;
  final List<ProfileBadge> badges;

  /// The chosen name looks official without a team badge.
  final bool nameWarning;
}

/// An emergency PIN as the settings show it (never the PIN itself).
class NotfallPin {
  const NotfallPin({required this.id, required this.after, this.chat, this.chatName, this.text});

  factory NotfallPin.fromJson(Map<String, Object?> j) => NotfallPin(
    id: j['id'] as String,
    after: j['after'] as String,
    chat: j['chat'] as String?,
    chatName: j['chat_name'] as String?,
    text: j['text'] as String?,
  );

  final String id;

  /// vanish: the account leaves the list. wrong_password: it stays, but
  /// no password opens it any more.
  final String after;
  final String? chat;
  final String? chatName;
  final String? text;

  bool get sends => chat != null;
}

class RealMessage {
  const RealMessage({required this.id, required this.text, required this.outgoing, required this.at, this.missing = 0});

  factory RealMessage.fromJson(Map<String, Object?> j) => RealMessage(
    id: (j['id'] as num).toInt(),
    text: j['text'] as String? ?? '',
    outgoing: j['outgoing'] == true,
    at: (j['at'] as num?)?.toInt() ?? 0,
    missing: (j['missing'] as num?)?.toInt() ?? 0,
  );

  final int id;
  final String text;
  final bool outgoing;
  final int at;

  /// Earlier messages of the sender that never arrived.
  final int missing;
}

/// A running pairing, as the unlocked device shows it.
class PairingProgress {
  PairingProgress(this.code, this.validUntil);

  /// The code to type on the new device, e.g. `7-anker-birne-kerze`.
  final String code;
  final DateTime validUntil;
  PairStage stage = PairStage.waiting;
  String message = 'Warte auf das neue Gerät …';
}

/// Default address of a server on this computer.
const defaultServerUrl = 'http://127.0.0.1:8471';

/// Minutes without use before Trair locks itself.
const autoLockChoices = [5, 15, 30];

/// Longest "Über mich", in characters (like Discord).
const maxAboutChars = 190;

/// App-wide state. Holds no secrets: the keys stay in the Rust core
/// behind [TrairVault]; this layer only gets the profile and results.
class AppState extends ChangeNotifier {
  AppState(
    this.backend,
    this.vault, {
    ServerTransport Function(Uri)? transport,
    String? accountsBase,
    Future<Uint8List?> Function()? pickPicture,
  }) : _transport = transport ?? HttpTransport.new,
       accounts = Accounts(accountsBase ?? TrairVault.dataDir()),
       _pickPictureFile = pickPicture;

  /// The accounts on this computer (one folder each).
  final Accounts accounts;
  final Future<Uint8List?> Function()? _pickPictureFile;

  /// What the start screen lists; [account] is the one chosen there.
  List<AccountCard> accountCards = const [];
  AccountCard? account;
  String accountMessage = '';

  /// Folder of the account the core has open now.
  String? _dir;

  /// The core's word on whether the data folder holds a vault (null: it
  /// could not be asked, another window has it; then the file decides).
  bool? _baseExists;

  /// A new account is being made; the first start can go back.
  bool addingAccount = false;

  /// Profile picture, 128 × 128 PNG (stored in the vault).
  Uint8List? picture;

  /// Show name and picture on the start screen (outside the vault).
  bool showOnStart = true;

  final TrairBackend backend;
  final ServerTransport Function(Uri) _transport;

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

  /// Address of the Trair server (stored in the vault).
  String serverUrl = defaultServerUrl;

  /// Connect on its own after unlocking (set once connected by hand).
  bool serverAuto = false;
  ServerStatus serverStatus = ServerStatus.off;
  String serverMessage = '';
  ServerApi? _api;

  /// The running pairing on this (unlocked) device, if any.
  PairingProgress? pairing;
  int _pairingRun = 0;

  /// Progress of joining as a new device (shown in the first start).
  String joinMessage = '';

  // Real chats (M4). Empty while locked.
  bool chatsReady = false;
  String contactCode = '';
  List<RealContact> contacts = const [];
  List<RealChat> realChats = const [];
  final Map<String, List<RealMessage>> realMessages = {};

  /// Last problem with chats, for the screen. Empty if all is well.
  String chatMessage = '';
  int _receiveRun = 0;

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
    accounts.removeEmpty();
    // The data folder first: the core says whether it holds a vault.
    final status = await v.init(accounts.base);
    if (!status.ok && status.error != 'in_use') {
      startupError = vaultErrorText(status);
      go(TrairScreen.coreMissing);
      return;
    }
    final baseFree = status.ok;
    _baseExists = baseFree ? status.raw['exists'] == true : null;
    accountCards = accounts.list(baseExists: _baseExists);
    screenProtectionSupported = await v.protectWindow(true);
    if (accountCards.isEmpty && baseFree) {
      _dir = accounts.base;
      go(TrairScreen.onboarding);
      return;
    }
    if (baseFree && accountCards.isNotEmpty && accountCards.first.dir == accounts.base) {
      account = accountCards.first;
      _dir = accounts.base;
    } else {
      for (final c in accountCards) {
        if (await _open(c)) break;
      }
    }
    if (account == null) accountMessage = 'Alle Konten sind schon in anderen Trair-Fenstern offen.';
    go(TrairScreen.lock);
  }

  // ------------------------------------------------------------ accounts

  /// Let the core open an account's folder. Fails if another Trair window
  /// has it.
  Future<bool> _open(AccountCard c) async {
    final s = await vault!.init(c.dir);
    if (!s.ok) {
      accountMessage = s.error == 'in_use'
          ? '${c.title} ist schon in einem anderen Trair-Fenster offen.'
          : vaultErrorText(s);
      return false;
    }
    account = c;
    _dir = c.dir;
    accountMessage = '';
    return true;
  }

  /// Choose another account on the start screen.
  Future<void> chooseAccount(AccountCard c) async {
    if (account?.dir == c.dir) return;
    await _open(c);
    notifyListeners();
  }

  /// Make another account on this computer: a fresh folder, then the
  /// first start.
  Future<void> newAccount() async {
    final dir = accounts.newDir();
    final s = await vault!.init(dir);
    if (!s.ok) {
      accountMessage = vaultErrorText(s);
      notifyListeners();
      return;
    }
    _dir = dir;
    account = null;
    addingAccount = true;
    go(TrairScreen.onboarding);
  }

  /// Leave the first start of a new account and go back to the list.
  Future<void> backToAccounts() async {
    accountCards = accounts.list(baseExists: _baseExists);
    for (final c in accountCards) {
      if (await _open(c)) break;
    }
    accounts.removeEmpty(except: _dir);
    addingAccount = false;
    go(TrairScreen.lock);
  }

  /// Store what the start screen shows for this account.
  void _writeCard() {
    final dir = _dir;
    if (dir == null || !unlocked) return;
    try {
      accounts.writeCard(dir, name: userName, glow: glow.toARGB32(), picture: picture, hidden: !showOnStart);
    } on FileSystemException {
      return;
    }
    account = accounts.read(dir);
    if (dir == accounts.base) _baseExists = true;
    accountCards = accounts.list(baseExists: _baseExists);
  }

  void setShowOnStart(bool on) => _change(() => showOnStart = on);

  /// Take a picture: cut to a square from the middle, 128 × 128 pixels.
  /// Returns an error text, or null.
  Future<String?> setPicture(Uint8List raw) async {
    if (raw.length > 20 * 1024 * 1024) return 'Das Bild ist zu groß (höchstens 20 MB).';
    try {
      final codec = await ui.instantiateImageCodec(raw);
      final frame = await codec.getNextFrame();
      final img = frame.image;
      final side = img.width < img.height ? img.width : img.height;
      final src = ui.Rect.fromLTWH((img.width - side) / 2, (img.height - side) / 2, side.toDouble(), side.toDouble());
      final recorder = ui.PictureRecorder();
      ui.Canvas(recorder).drawImageRect(
        img,
        src,
        const ui.Rect.fromLTWH(0, 0, 128, 128),
        ui.Paint()..filterQuality = ui.FilterQuality.high,
      );
      final small = await recorder.endRecording().toImage(128, 128);
      final png = await small.toByteData(format: ui.ImageByteFormat.png);
      img.dispose();
      small.dispose();
      if (png == null) return 'Das Bild ließ sich nicht lesen.';
      picture = png.buffer.asUint8List();
    } on Object {
      return 'Das Bild ließ sich nicht lesen.';
    }
    notifyListeners();
    _scheduleSave();
    return null;
  }

  void removePicture() => _change(() => picture = null);

  /// Choose a picture file (Windows dialog) and take it.
  Future<String?> pickPicture() async {
    Uint8List? raw;
    final pick = _pickPictureFile;
    if (pick != null) {
      raw = await pick();
    } else {
      final chosen = await vault!.pickImage();
      if (!chosen.ok) return chosen.error == 'cancelled' ? null : 'Bilder wählen geht hier nicht.';
      try {
        raw = await File(chosen.raw['path'] as String).readAsBytes();
      } on FileSystemException {
        return 'Die Datei ließ sich nicht lesen.';
      }
    }
    if (raw == null) return null;
    return setPicture(raw);
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
    if (answer.ok) {
      _unlocked(answer);
      addingAccount = false;
      _writeCard();
    }
    return answer;
  }

  Future<CoreAnswer> unlock(String password) async {
    final answer = await vault!.unlock(password);
    if (answer.ok) {
      _unlocked(answer);
      return answer;
    }
    final emergency = answer.raw['notfall'];
    if (emergency is Map<String, Object?>) {
      await _emergency(emergency);
      // The account left the list: no "wrong password" under another one.
      if (emergency['after'] == 'vanish') return CoreAnswer(const {'ok': false, 'error': 'vanished'});
    }
    return answer;
  }

  /// An emergency PIN was typed instead of the password. The core has
  /// deleted everything already, or it holds the keys for exactly one
  /// message and deletes once that is out (or the server does not answer
  /// within a few seconds). From outside it looks like a wrong password.
  Future<void> _emergency(Map<String, Object?> n) async {
    if (n['send'] == true) {
      try {
        await _emergencySend(n).timeout(const Duration(seconds: 12));
      } on Object {
        // Not reachable or too slow: deleted here all the same.
      } finally {
        await vault!.notfallFinish();
      }
    }
    if (n['after'] == 'vanish') await _accountGone();
  }

  /// Send the PIN's message, then delete the account on the server too.
  Future<void> _emergencySend(Map<String, Object?> n) async {
    final uri = parseServerUrl(n['server'] as String? ?? '');
    if (uri == null) return;
    final api = ServerApi(_transport(uri));
    final user = n['user'] as String;
    final device = n['device'] as String;
    final challenge = await api.challenge(user, device);
    if (!challenge.ok) return;
    final c = challenge.body['challenge'] as String;
    final signed = await vault!.notfallSignLogin(c);
    if (!signed.ok) return;
    final login = await api.login(user, device, c, signed.raw['signature'] as String);
    if (!login.ok) return;
    api.token = login.body['token'] as String;
    await api.sendMessage(n['to'], n['body'] as String);
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final deletion = await vault!.notfallSignDeletion(now);
    if (deletion.ok) await api.deleteAccount(now, deletion.raw['signature'] as String);
  }

  // ------------------------------------------------------- emergency PINs

  /// Null: hidden. The list needs the password, every time the settings
  /// open it.
  List<NotfallPin>? notfallPins;

  Future<CoreAnswer> showNotfallPins(String password) => _pins(vault!.notfallList(password));

  void hideNotfallPins() {
    notfallPins = null;
    notifyListeners();
  }

  Future<CoreAnswer> addNotfallPin({
    required String pin,
    required String password,
    required String after,
    String? chat,
    String? text,
  }) => _pins(vault!.notfallAdd(pin: pin, password: password, after: after, chat: chat, text: text));

  Future<CoreAnswer> removeNotfallPin(String id, String password) => _pins(vault!.notfallRemove(id, password));

  Future<CoreAnswer> _pins(Future<CoreAnswer> call) async {
    final answer = await call;
    if (answer.ok) {
      notfallPins = [for (final j in (answer.raw['pins'] as List).cast<Map<String, Object?>>()) NotfallPin.fromJson(j)];
      notifyListeners();
    }
    return answer;
  }

  /// [server]: the address a new device just paired through. Its vault
  /// has no profile yet, so the address would otherwise be lost.
  void _unlocked(CoreAnswer answer, {String? server}) {
    chatsReady = answer.raw['chats'] == true;
    unawaited(refreshChats());
    _applyProfile(answer.profile);
    if (server != null) {
      serverUrl = server;
      serverAuto = true;
    }
    fingerprint = answer.fingerprint;
    devices = answer.devices;
    accountKey = answer.accountKey;
    realBadges = answer.badges;
    notfallPins = null;
    go(TrairScreen.chats);
    // The start screen shows the current name and picture from now on.
    _writeCard();
    noteActivity();
    unawaited(_applyScreenProtection());
    if (serverAuto) unawaited(connect());
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
    notfallPins = null;
    _forgetChats();
    _forgetServer();
    accountCards = accounts.list(baseExists: _baseExists);
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
  /// When connected, the account is deleted on the server first (with a
  /// signature of the account key); [serverDeletion] tells how that went.
  Future<CoreAnswer> deleteAccount(String password) async {
    serverDeletion = '';
    final api = serverStatus == ServerStatus.connected ? _api : null;
    // Sign first (the key is gone afterwards), send only once the local
    // deletion worked: a wrong password must not delete the server side.
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final signed = api == null ? null : await vault!.signDeletion(now);
    final answer = await vault!.deleteAccount(password);
    if (answer.ok && api != null) {
      final reply = signed != null && signed.ok
          ? await api.deleteAccount(now, signed.raw['signature'] as String)
          : null;
      serverDeletion = reply != null && reply.ok
          ? 'Auch auf dem Server gelöscht.'
          : 'Der Server war nicht erreichbar. Er löscht das Konto nach 6 Monaten ohne Anmeldung selbst.';
    }
    if (answer.ok) {
      _forgetChats();
      _forgetServer();
      _autoLock?.cancel();
      _saveSoon?.cancel();
      _applyProfile(const {});
      fingerprint = '';
      devices = const [];
      accountKey = '';
      realBadges = const [];
      await _accountGone(message: serverDeletion.isEmpty ? 'Konto gelöscht.' : 'Konto gelöscht. $serverDeletion');
    }
    return answer;
  }

  /// The open account is gone: its card leaves the list. Other accounts
  /// stay (back to the list), otherwise the first start follows.
  Future<void> _accountGone({String message = ''}) async {
    final gone = _dir;
    if (gone != null) accounts.removeCard(gone);
    if (gone == accounts.base) _baseExists = false;
    account = null;
    accountCards = accounts.list(baseExists: _baseExists);
    if (accountCards.isEmpty) {
      go(TrairScreen.onboarding);
    } else {
      // The empty folder goes once the core has let go of it.
      for (final c in accountCards) {
        if (await _open(c)) break;
      }
      accounts.removeEmpty(except: _dir);
      accountMessage = message;
      go(TrairScreen.lock);
    }
  }

  /// Name a device. The name stays on this device, never in the list
  /// other people see.
  Future<CoreAnswer> renameDevice(String id, String label) => _devices(vault!.renameDevice(id, label.trim()));

  /// Withdraw another device for good. Needs the password again. When
  /// connected, the server learns it at once and drops that device.
  Future<CoreAnswer> withdrawDevice(String id, String password) async {
    final answer = await _devices(vault!.withdrawDevice(id, password));
    if (answer.ok) {
      await _pushDeviceList();
      // The withdrawn device leaves every chat, or nothing can be sent.
      final api = _api;
      if (api != null) await _removeWithdrawn(api);
    }
    return answer;
  }

  /// What happened to the account on the server when it was deleted.
  String serverDeletion = '';

  // ------------------------------------------------------------ server (M3)

  void setServerUrl(String url) => _change(() => serverUrl = url.trim());

  void _forgetServer() {
    _api = null;
    _receiveRun++;
    _pairingRun++;
    pairing = null;
    serverStatus = ServerStatus.off;
    serverMessage = '';
  }

  void _serverFailed(String message) {
    _api = null;
    serverStatus = ServerStatus.failed;
    serverMessage = message;
    notifyListeners();
  }

  /// Connect: register the account if the server does not know it yet,
  /// log in with the device key, and bring the device lists in line.
  Future<void> connect() async {
    final uri = parseServerUrl(serverUrl);
    if (uri == null) {
      _serverFailed('Bis TLS fertig ist, nur ein Server auf diesem PC: http://127.0.0.1:PORT');
      return;
    }
    serverStatus = ServerStatus.connecting;
    serverMessage = 'Verbinde …';
    notifyListeners();
    final api = ServerApi(_transport(uri));
    final health = await api.health();
    if (!health.ok) {
      _serverFailed('Server nicht erreichbar (${health.error}). Läuft trair-server?');
      return;
    }
    final me = await vault!.serverIdentity();
    if (!me.ok) return _serverFailed(vaultErrorText(me));
    final user = me.raw['user'] as String;
    final device = me.raw['device'] as String;
    final registered = await api.register(me.raw);
    if (!registered.ok && registered.error != 'user_taken') {
      return _serverFailed('Anmelden beim Server ging nicht (${registered.error}).');
    }
    final challenge = await api.challenge(user, device);
    final signed = challenge.ok ? await vault!.signLogin(challenge.body['challenge'] as String) : null;
    final login = signed != null && signed.ok
        ? await api.login(user, device, challenge.body['challenge'] as String, signed.raw['signature'] as String)
        : null;
    if (login == null || !login.ok) {
      return _serverFailed('Der Server kennt dieses Gerät nicht. Wurde es von einem anderen Gerät entzogen?');
    }
    api.token = login.body['token'] as String;
    // Newer list on the server (another device paired or withdrew)?
    final account = await api.account(user);
    if (account.ok) {
      final applied = await vault!.applyDeviceList(account.body['device_list']);
      if (applied.error == 'withdrawn') {
        return _serverFailed('Dieses Gerät wurde von einem anderen Gerät entzogen.');
      }
      if (applied.ok) devices = applied.devices;
    }
    _api = api;
    await _pushDeviceList();
    if (chatsReady) await _chatsOnline(api);
    serverStatus = ServerStatus.connected;
    serverMessage = 'Verbunden mit ${uri.host}:${uri.port}';
    if (chatsReady) unawaited(_shareProfile());
    if (!serverAuto) {
      serverAuto = true;
      _scheduleSave();
    }
    notifyListeners();
  }

  /// Stop using the server (also after the next unlock).
  void disconnect() {
    _forgetServer();
    serverAuto = false;
    _scheduleSave();
    notifyListeners();
  }

  /// Send this device's current list to the server. The server keeps only
  /// newer lists, so sending the same one again is harmless.
  Future<void> _pushDeviceList() async {
    final api = _api;
    if (api == null) return;
    final me = await vault!.serverIdentity();
    if (me.ok) await api.putDevices(me.raw['device_list']);
  }

  bool get canPair => serverStatus == ServerStatus.connected && pairing == null;

  // ------------------------------------------------------------ chats (M4)

  bool get chatsOnline => chatsReady && _api != null && serverStatus == ServerStatus.connected;

  void _forgetChats() {
    _receiveRun++;
    chatsReady = false;
    peerProfiles = const {};
    _profilesStale = true;
    contactCode = '';
    contacts = const [];
    realChats = const [];
    realMessages.clear();
    chatMessage = '';
  }

  /// Read contacts, chats and the own code from the core again.
  Future<void> refreshChats() async {
    final v = vault;
    if (v == null || !chatsReady) return;
    final code = await v.contactCode();
    final c = await v.contacts();
    final ch = await v.chats();
    if (!chatsReady) return;
    if (code.ok) contactCode = code.raw['code'] as String;
    if (c.ok) {
      contacts = [for (final j in (c.raw['contacts'] as List).cast<Map<String, Object?>>()) RealContact.fromJson(j)];
    }
    if (ch.ok) {
      realChats = [for (final j in (ch.raw['chats'] as List).cast<Map<String, Object?>>()) RealChat.fromJson(j)];
    }
    if (_profilesStale) {
      final p = await v.profiles();
      if (!chatsReady) return;
      if (p.ok) {
        _profilesStale = false;
        peerProfiles = {
          for (final j in (p.raw['profiles'] as List).cast<Map<String, Object?>>())
            j['user'] as String: PeerProfile.fromJson(j),
        };
      }
    }
    notifyListeners();
    // A new chat may still lack the own profile.
    if (chatsOnline) unawaited(_shareProfile());
  }

  // ------------------------------------------------- profiles to contacts

  /// Profiles of chat partners, by user.
  Map<String, PeerProfile> peerProfiles = const {};
  bool _profilesStale = true;

  /// What the start of a chat shows from the partner's profile. Nothing
  /// for a request: no picture from strangers before accepting.
  PeerProfile? profileFor(RealChat chat) => chat.isRequest ? null : peerProfiles[chat.peer];

  /// Version of the own shared profile and a check value of its content.
  int _shareRev = 0;
  String _shareHash = '';
  bool _sharing = false;

  Map<String, Object?> _shareable() => {
    'name': userName.trim().isEmpty ? 'Trair' : userName.trim(),
    'glow': glow.toARGB32(),
    'about': about.trim(),
    'picture': picture == null ? null : base64.encode(picture!),
  };

  /// FNV-1a: notices that the profile changed (not for security).
  static String _check(String text) {
    var h = 0x811c9dc5;
    for (final c in utf8.encode(text)) {
      h = ((h ^ c) * 0x01000193) & 0xFFFFFFFF;
    }
    return h.toRadixString(16);
  }

  /// Send the own profile to every chat that does not have this version
  /// yet. Only once the server took it is a chat marked; the rest is
  /// tried again next time. Who gets it follows "Profil sichtbar für".
  Future<void> _shareProfile() async {
    final api = _api;
    if (_sharing || api == null || !chatsOnline || !unlocked) return;
    _sharing = true;
    try {
      final mine = _shareable();
      final check = _check(jsonEncode(mine));
      if (check != _shareHash || _shareRev == 0) {
        _shareHash = check;
        _shareRev = max(DateTime.now().millisecondsSinceEpoch, _shareRev + 1);
        _scheduleSave();
      }
      final out = await vault!.shareProfile({
        'v': 1,
        'rev': _shareRev,
        ...mine,
        'badges': const <Object?>[],
      }, onlyContacts: profileVisibility == 'kontakte');
      if (!out.ok) return;
      for (final o in (out.raw['out'] as List).cast<Map<String, Object?>>()) {
        if (!identical(api, _api)) return;
        final sent = await api.sendMessage(o['to'], o['body'] as String);
        if (sent.ok) await vault!.profileSent(o['chat'] as String, (o['rev'] as num).toInt());
      }
    } finally {
      _sharing = false;
    }
  }

  Future<void> loadMessages(String chat) async {
    final m = await vault!.messages(chat);
    if (!m.ok) return;
    realMessages[chat] = [
      for (final j in (m.raw['messages'] as List).cast<Map<String, Object?>>()) RealMessage.fromJson(j),
    ];
    notifyListeners();
  }

  String _chatError(CoreAnswer a) => switch (a.error) {
    'rejected' => 'Das passt nicht (Code falsch oder schon da?).',
    'blocked' => 'Blockiert: der Schlüssel passt nicht zum bekannten Kontakt.',
    'bad_package' => 'Vom anderen Konto kam kein passendes Gerät.',
    'withdrawn_member' => 'Im Chat ist noch ein entzogenes Gerät.',
    'storage' => 'Die Chat-Datenbank ist nicht lesbar oder voll.',
    'chats_unavailable' => 'Die Chat-Datenbank ließ sich nicht öffnen.',
    _ => 'Fehler: ${a.error}',
  };

  /// Add a contact from their code. [name] is how they appear here.
  Future<bool> addContact(String code, String name) async {
    final a = await vault!.addContact(code.trim(), name.trim());
    chatMessage = a.ok ? '' : _chatError(a);
    await refreshChats();
    return a.ok;
  }

  /// Start a chat with a contact: key packages for their devices and for
  /// this account's other devices, then the change through the server.
  Future<String?> startChat(String user) async {
    final api = _api;
    if (api == null || !chatsReady) {
      chatMessage = 'Erst mit dem Server verbinden.';
      notifyListeners();
      return null;
    }
    final me = await vault!.serverIdentity();
    final offers = [...await api.claim(user), if (me.ok) ...await api.claim(me.raw['user'] as String)];
    final started = await vault!.startChat(user, offers);
    if (!started.ok) {
      chatMessage = _chatError(started);
      notifyListeners();
      return null;
    }
    final commit = (started.raw['commit'] as Map).cast<String, Object?>();
    if (!await _pushCommit(api, commit)) {
      chatMessage = 'Der Server hat den neuen Chat nicht angenommen.';
    } else {
      chatMessage = '';
    }
    await refreshChats();
    return (started.raw['chat'] as Map)['id'] as String;
  }

  /// Send a text. It is stored first; then the ciphertext goes out.
  Future<bool> sendReal(String chat, String text) async {
    final api = _api;
    final t = text.trim();
    if (t.isEmpty) return false;
    if (api == null) {
      chatMessage = 'Nicht verbunden: erst mit dem Server verbinden.';
      notifyListeners();
      return false;
    }
    final sent = await vault!.sendMessage(chat, t);
    if (!sent.ok) {
      chatMessage = _chatError(sent);
      notifyListeners();
      return false;
    }
    final r = await api.sendMessage(sent.raw['to'], sent.raw['body'] as String);
    chatMessage = r.ok ? '' : 'Gespeichert, aber nicht zugestellt (${r.error}).';
    await loadMessages(chat);
    await refreshChats();
    return r.ok;
  }

  Future<void> acceptRequest(String chat, String name) async {
    final a = await vault!.acceptRequest(chat, name);
    chatMessage = a.ok ? '' : _chatError(a);
    await refreshChats();
  }

  Future<void> declineRequest(String chat) async {
    final a = await vault!.declineRequest(chat);
    chatMessage = a.ok ? '' : _chatError(a);
    realMessages.remove(chat);
    await refreshChats();
  }

  /// Hand a change to the server and tell the core the answer. Without
  /// an answer (offline) the change stays waiting and goes out again on
  /// the next connect.
  Future<bool> _pushCommit(ServerApi api, Map<String, Object?> commit) async {
    final r = await api.postCommit(commit);
    if (r.status == 0) return false;
    final taken = r.body['taken'] as String?;
    final a = await vault!.commitAnswer(commit['chat'] as String, r.ok, taken);
    return a.ok && a.raw['applied'] == true;
  }

  /// After connecting: key packages, waiting changes, contacts' device
  /// lists, then receive in the background.
  Future<void> _chatsOnline(ServerApi api) async {
    final count = await api.keyPackageCount();
    final have = (count.body['count'] as num?)?.toInt() ?? 0;
    if (count.ok && have < 10) {
      final kps = await vault!.keyPackages(20, lastResort: have == 0);
      if (kps.ok) await api.uploadKeyPackages(kps.raw['packages'] as List, kps.raw['last_resort'] as String?);
    }
    final pending = await vault!.pendingCommits();
    for (final c in (pending.raw['commits'] as List? ?? const []).cast<Map<String, Object?>>()) {
      await _pushCommit(api, c);
    }
    for (final c in contacts) {
      final acc = await api.account(c.user);
      if (acc.ok) await vault!.applyContactList(acc.body['account_key'] as String, acc.body['device_list']);
    }
    await _removeWithdrawn(api);
    await refreshChats();
    final run = ++_receiveRun;
    unawaited(_receiveLoop(api, run));
  }

  /// Remove withdrawn devices (own or a contact's) from every chat.
  Future<void> _removeWithdrawn(ServerApi api) async {
    for (final chat in realChats.where((c) => c.state == 'active')) {
      final w = await vault!.withdrawnMembers(chat.id);
      final ids = (w.raw['ids'] as List? ?? const []).cast<String>();
      if (ids.isEmpty) continue;
      final out = await vault!.removeDevices(chat.id, ids);
      if (out.ok) await _pushCommit(api, (out.raw['commit'] as Map).cast<String, Object?>());
    }
  }

  /// Fetch what arrives, let the core process and store it, then tell
  /// the server to delete it. Stops when locked or disconnected.
  Future<void> _receiveLoop(ServerApi api, int run) async {
    var failures = 0;
    while (run == _receiveRun && identical(api, _api)) {
      final r = await api.fetchMessages(25);
      if (run != _receiveRun || !identical(api, _api)) return;
      if (!r.ok) {
        if (++failures >= 3) {
          _serverFailed('Verbindung zum Server verloren (${r.error}).');
          return;
        }
        await Future<void>.delayed(const Duration(seconds: 2));
        continue;
      }
      failures = 0;
      final ids = <int>[];
      final touched = <String>{};
      for (final m in (r.body['messages'] as List? ?? const []).cast<Map<String, Object?>>()) {
        final got = await vault!.receive(m['body'] as String);
        if (run != _receiveRun) return;
        // Keep it on the server only if this device could not store it.
        if (!const {'storage', 'internal', 'locked', 'chats_unavailable'}.contains(got.error)) {
          ids.add((m['id'] as num).toInt());
        }
        final incoming = (got.raw['incoming'] as Map?)?.cast<String, Object?>();
        if (incoming == null) continue;
        switch (incoming['kind']) {
          case 'message':
            touched.add((incoming['message'] as Map)['chat'] as String);
          case 'chat':
            final chat = (incoming['chat'] as Map)['id'] as String;
            touched.add(chat);
            if (incoming['update_keys'] == true) {
              final out = await vault!.updateKeys(chat);
              if (out.ok) await _pushCommit(api, (out.raw['commit'] as Map).cast<String, Object?>());
            }
          case 'changed':
            touched.add(incoming['chat'] as String);
          case 'profile':
            _profilesStale = true;
            touched.add('');
        }
      }
      if (ids.isNotEmpty) await api.ack(ids);
      if (touched.isNotEmpty) {
        await refreshChats();
        for (final chat in touched) {
          if (chat.isNotEmpty && realMessages.containsKey(chat)) await loadMessages(chat);
        }
      }
    }
  }

  /// Pair a new device: get a channel, show the code, wait for the new
  /// device, add it, and hand over the account. Runs until done, failed,
  /// cancelled or expired.
  Future<void> startPairing() async {
    final api = _api;
    if (api == null || pairing != null) return;
    final run = ++_pairingRun;
    final opened = await api.openPairing();
    if (!opened.ok) {
      serverMessage = 'Koppeln gerade nicht möglich (${opened.error}).';
      notifyListeners();
      return;
    }
    final channel = opened.body['channel'] as int;
    final started = await vault!.pairHostStart(channel);
    if (!started.ok || !(await api.putHostHello(channel, started.raw['hello'] as String)).ok) {
      serverMessage = 'Koppeln konnte nicht starten.';
      notifyListeners();
      return;
    }
    final progress = PairingProgress(started.raw['code'] as String, DateTime.now().add(const Duration(minutes: 10)));
    pairing = progress;
    notifyListeners();
    final guest = await _poll(
      () => api.getGuest(channel, 25),
      () => run == _pairingRun && DateTime.now().isBefore(progress.validUntil),
    );
    if (run != _pairingRun) return;
    if (guest == null) {
      return _pairingEnded(progress, PairStage.failed, 'Abgelaufen oder Verbindung weg. Starte einen neuen Code.');
    }
    progress
      ..stage = PairStage.finishing
      ..message = 'Neues Gerät meldet sich, prüfe …';
    notifyListeners();
    final finished = await vault!.pairHostFinish(guest.body['hello'] as String, guest.body['offer'] as String);
    if (!finished.ok) {
      return _pairingEnded(
        progress,
        PairStage.failed,
        'Abgelehnt: falscher Code oder verändert unterwegs. Nichts wurde hinzugefügt.',
      );
    }
    devices = finished.devices;
    await api.putBundle(channel, finished.raw['bundle'] as String);
    await api.putDevices(finished.raw['device_list']);
    _pairingEnded(progress, PairStage.done, 'Gerät gekoppelt.');
  }

  /// Long poll until something arrives. A server that answers "not yet"
  /// at once is not asked again faster than once a second.
  Future<ServerReply?> _poll(Future<ServerReply> Function() ask, bool Function() keepGoing) async {
    while (keepGoing()) {
      final started = DateTime.now();
      final reply = await ask();
      if (reply.ready) return reply;
      if (!reply.ok) return null;
      final spent = DateTime.now().difference(started);
      if (spent < const Duration(seconds: 1)) {
        await Future<void>.delayed(const Duration(seconds: 1) - spent);
      }
    }
    return null;
  }

  void _pairingEnded(PairingProgress progress, PairStage stage, String message) {
    progress
      ..stage = stage
      ..message = message;
    notifyListeners();
  }

  /// Close the pairing panel; a running pairing is given up.
  Future<void> closePairing() async {
    _pairingRun++;
    pairing = null;
    notifyListeners();
    await vault?.pairCancel();
  }

  /// First start on a new device: join an existing account with the code
  /// shown on the unlocked device. Makes this device's vault with its own
  /// [password] and unlocks it.
  Future<CoreAnswer> joinWithCode({required String url, required String code, required String password}) async {
    CoreAnswer fail(String code, String message) {
      joinMessage = message;
      notifyListeners();
      return CoreAnswer({'ok': false, 'error': code});
    }

    final uri = parseServerUrl(url);
    if (uri == null)
      return fail('bad_server', 'Bis TLS fertig ist, nur ein Server auf diesem PC: http://127.0.0.1:PORT');
    final api = ServerApi(_transport(uri));
    joinMessage = 'Verbinde …';
    notifyListeners();
    final started = await vault!.pairGuestStart(code);
    if (!started.ok) {
      return fail(
        started.error,
        started.error == 'bad_code' ? 'Der Code passt nicht: Zahl und drei Wörter.' : vaultErrorText(started),
      );
    }
    final channel = started.raw['channel'] as int;
    final hostHello = await api.getHostHello(channel, 25);
    if (!hostHello.ready) {
      await vault!.pairCancel();
      return fail('no_host', 'Kein Gerät wartet mit diesem Code. Läuft der Server, ist der Code noch gültig?');
    }
    final offer = await vault!.pairGuestOffer(hostHello.body['hello'] as String);
    if (!offer.ok || !(await api.putGuest(channel, started.raw['hello'] as String, offer.raw['offer'] as String)).ok) {
      await vault!.pairCancel();
      return fail('pairing_failed', 'Koppeln abgelehnt. Starte einen neuen Code.');
    }
    joinMessage = 'Warte auf das andere Gerät …';
    notifyListeners();
    final until = DateTime.now().add(const Duration(minutes: 10));
    final bundle = await _poll(() => api.getBundle(channel, 25), () => DateTime.now().isBefore(until));
    if (bundle == null) {
      await vault!.pairCancel();
      return fail('pairing_failed', 'Das andere Gerät hat abgelehnt: wahrscheinlich ein Tippfehler im Code.');
    }
    joinMessage = 'Lege den Tresor an …';
    notifyListeners();
    final answer = await vault!.pairGuestFinish(bundle.body['bundle'] as String, password);
    if (!answer.ok) {
      return fail(
        answer.error,
        answer.error == 'pairing_failed' ? 'Die Antwort passte nicht zum Code.' : vaultErrorText(answer),
      );
    }
    joinMessage = '';
    // Unlocking connects at once, through the server used for pairing.
    _unlocked(answer, server: url.trim());
    addingAccount = false;
    _writeCard();
    _scheduleSave();
    return answer;
  }

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
    _scheduleSave();
  }

  void _scheduleSave() {
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
    _writeCard();
    // Name, colour, picture or "about" may have changed.
    unawaited(_shareProfile());
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
    'server': serverUrl,
    'serverAuto': serverAuto,
    'startCard': showOnStart,
    'picture': ?(picture == null ? null : base64.encode(picture!)),
    'shareRev': _shareRev,
    'shareHash': _shareHash,
  };

  void _applyProfile(Map<String, Object?> p) {
    userName = p['name'] as String? ?? '';
    _shareRev = (p['shareRev'] as num?)?.toInt() ?? 0;
    _shareHash = p['shareHash'] as String? ?? '';
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
    final server = p['server'];
    serverUrl = server is String && server.isNotEmpty ? server : defaultServerUrl;
    serverAuto = p['serverAuto'] as bool? ?? false;
    showOnStart = p['startCard'] as bool? ?? true;
    final pic = p['picture'];
    try {
      picture = pic is String ? base64.decode(pic) : null;
    } on FormatException {
      picture = null;
    }
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
