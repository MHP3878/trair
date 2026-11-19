// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';
import 'dart:convert';

import 'package:flutter/widgets.dart';

import 'backend/backend.dart';
import 'core/server.dart';
import 'core/vault.dart';
import 'theme.dart';

enum TrairScreen { starting, coreMissing, lock, onboarding, chats, community, profile }

/// Connection to the Trair server.
enum ServerStatus { off, connecting, connected, failed }

/// Where a pairing stands, on the unlocked device.
enum PairStage { waiting, finishing, done, failed }

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
  AppState(this.backend, this.vault, {ServerTransport Function(Uri)? transport})
    : _transport = transport ?? HttpTransport.new;

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
    _forgetServer();
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
      _forgetServer();
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

  /// Withdraw another device for good. Needs the password again. When
  /// connected, the server learns it at once and drops that device.
  Future<CoreAnswer> withdrawDevice(String id, String password) async {
    final answer = await _devices(vault!.withdrawDevice(id, password));
    if (answer.ok) await _pushDeviceList();
    return answer;
  }

  /// What happened to the account on the server when it was deleted.
  String serverDeletion = '';

  // ------------------------------------------------------------ server (M3)

  void setServerUrl(String url) => _change(() => serverUrl = url.trim());

  void _forgetServer() {
    _api = null;
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
    if (!(await api.health()).ok) {
      _serverFailed('Server nicht erreichbar. Läuft trair-server?');
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
      return _serverFailed(
        'Der Server kennt dieses Gerät nicht. Wurde es von einem anderen Gerät entzogen?',
      );
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
    serverStatus = ServerStatus.connected;
    serverMessage = 'Verbunden mit ${uri.host}:${uri.port}';
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
    final progress = PairingProgress(
      started.raw['code'] as String,
      DateTime.now().add(const Duration(minutes: 10)),
    );
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
    if (uri == null) return fail('bad_server', 'Bis TLS fertig ist, nur ein Server auf diesem PC: http://127.0.0.1:PORT');
    final api = ServerApi(_transport(uri));
    joinMessage = 'Verbinde …';
    notifyListeners();
    final started = await vault!.pairGuestStart(code);
    if (!started.ok) {
      return fail(started.error, started.error == 'bad_code' ? 'Der Code passt nicht: Zahl und drei Wörter.' : vaultErrorText(started));
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
      return fail(answer.error, answer.error == 'pairing_failed' ? 'Die Antwort passte nicht zum Code.' : vaultErrorText(answer));
    }
    joinMessage = '';
    serverUrl = url.trim();
    serverAuto = true;
    _unlocked(answer);
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
    final server = p['server'];
    serverUrl = server is String && server.isNotEmpty ? server : defaultServerUrl;
    serverAuto = p['serverAuto'] as bool? ?? false;
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
