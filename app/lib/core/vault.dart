// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:io';

import '../backend/backend.dart';
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

  /// The account's devices (after unlock, rename and withdraw).
  List<TrairDevice> get devices {
    final list = raw['devices'];
    if (list is! List<Object?>) return const [];
    return list.whereType<Map<String, Object?>>().map(TrairDevice.fromJson).toList();
  }

  /// True once, when a vault from before devices existed was upgraded.
  bool get upgraded => raw['upgraded'] == true;

  /// The public account key in hex (64 characters). Not secret.
  String get accountKey => raw['account_key'] as String? ?? '';

  /// The own badges the core checked (after unlock and add_badge).
  List<ProfileBadge> get badges {
    final list = raw['badges'];
    if (list is! List<Object?>) return const [];
    return list.whereType<Map<String, Object?>>().map(ProfileBadge.fromJson).nonNulls.toList();
  }
}

/// One device of the account. The name is only stored on this device.
class TrairDevice {
  const TrairDevice({required this.id, required this.label, required this.fingerprint, required this.isThis});

  factory TrairDevice.fromJson(Map<String, Object?> j) => TrairDevice(
    id: j['id'] as String? ?? '',
    label: j['label'] as String?,
    fingerprint: j['fingerprint'] as String? ?? '',
    isThis: j['this'] == true,
  );

  final String id;
  final String? label;
  final String fingerprint;
  final bool isThis;

  String get name => label ?? 'Gerät ohne Namen';
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

  /// Give a device a name (1 to 40 characters). Only stored on this device.
  Future<CoreAnswer> renameDevice(String id, String label) => _call({'op': 'rename_device', 'id': id, 'label': label});

  /// Withdraw another device of the account for good. Needs the password.
  Future<CoreAnswer> withdrawDevice(String id, String password) =>
      _call({'op': 'withdraw_device', 'id': id, 'password': password});

  /// Keep a signed badge for the own account (for example from the team
  /// tool). The core checks it first.
  Future<CoreAnswer> addBadge(Object? badge) => _call({'op': 'add_badge', 'badge': badge});

  // -------------------------------------- friendship keys, vanishing chats

  /// A friendship key: works once, for [minutes]. [vanishing]: the chat it
  /// opens vanishes at the same time.
  Future<CoreAnswer> createInvite(int minutes, {bool vanishing = false}) =>
      _call({'op': 'create_invite', 'minutes': minutes, 'vanishing': vanishing});

  /// After starting a chat: the token of a used friendship key, if any.
  Future<CoreAnswer> inviteMessage(String chat) => _call({'op': 'invite_message', 'chat': chat});

  /// Let a chat vanish after [minutes]; null: it stays.
  Future<CoreAnswer> setChatEnd(String chat, int? minutes) =>
      _call({'op': 'set_chat_end', 'chat': chat, 'minutes': minutes});

  /// Delete what ran out (vanished chats, old keys).
  Future<CoreAnswer> sweep() => _call({'op': 'sweep'});

  // ------------------------------------------------- profiles to contacts

  /// This account's profile, encrypted for every chat that lacks this
  /// version. The core adds the vault's badges and checks everything.
  Future<CoreAnswer> shareProfile(Map<String, Object?> profile, {required bool onlyContacts}) =>
      _call({'op': 'share_profile', 'profile': profile, 'only_contacts': onlyContacts});

  /// The server took the profile for [chat].
  Future<CoreAnswer> profileSent(String chat, int rev) => _call({'op': 'profile_sent', 'chat': chat, 'rev': rev});

  /// Profiles other accounts sent (badges checked by the core just now).
  Future<CoreAnswer> profiles() => _call({'op': 'profiles'});

  // ---------------------------------------------------------- emergency PINs

  /// The emergency PINs. Only with the password: someone at an open
  /// Trair cannot see whether any are set.
  Future<CoreAnswer> notfallList(String password) => _call({'op': 'notfall_list', 'password': password});

  /// Set an emergency PIN. [after]: `vanish` or `wrong_password`. With
  /// [chat] and [text] it first sends that message.
  Future<CoreAnswer> notfallAdd({
    required String pin,
    required String password,
    required String after,
    String? chat,
    String? text,
  }) => _call({'op': 'notfall_add', 'pin': pin, 'password': password, 'after': after, 'chat': ?chat, 'text': ?text});

  Future<CoreAnswer> notfallRemove(String id, String password) =>
      _call({'op': 'notfall_remove', 'id': id, 'password': password});

  /// While a sending PIN runs: log in and delete with the account's keys.
  Future<CoreAnswer> notfallSignLogin(String challengeHex) =>
      _call({'op': 'notfall_sign_login', 'challenge': challengeHex});
  Future<CoreAnswer> notfallSignDeletion(int now) => _call({'op': 'notfall_sign_deletion', 'now': now});

  /// Sent or not: now everything goes.
  Future<CoreAnswer> notfallFinish() => _call({'op': 'notfall_finish'});

  // ------------------------------------------------------------ server (M3)

  /// Public data the server needs: user, device, account key, device list.
  Future<CoreAnswer> serverIdentity() => _call({'op': 'server_identity'});

  /// Sign the server's login challenge with this device's key.
  Future<CoreAnswer> signLogin(String challengeHex) => _call({'op': 'sign_login', 'challenge': challengeHex});

  /// The account key agrees to delete the account on the server.
  Future<CoreAnswer> signDeletion(int now) => _call({'op': 'sign_deletion', 'now': now});

  /// Start pairing as the unlocked device; gives the code to show.
  Future<CoreAnswer> pairHostStart(int channel) => _call({'op': 'pair_host_start', 'channel': channel});

  /// Finish as the unlocked device with the new device's messages.
  Future<CoreAnswer> pairHostFinish(String hello, String offer) =>
      _call({'op': 'pair_host_finish', 'hello': hello, 'offer': offer});

  /// Start pairing as the new device with the typed code.
  Future<CoreAnswer> pairGuestStart(String code) => _call({'op': 'pair_guest_start', 'code': code});

  /// The new device answers the host's hello.
  Future<CoreAnswer> pairGuestOffer(String hello) => _call({'op': 'pair_guest_offer', 'hello': hello});

  /// The new device takes the bundle and makes its vault.
  Future<CoreAnswer> pairGuestFinish(String bundle, String password) =>
      _call({'op': 'pair_guest_finish', 'bundle': bundle, 'password': password});

  /// Forget a running pairing.
  Future<CoreAnswer> pairCancel() => _call({'op': 'pair_cancel'});

  /// Take a newer device list from the server (checked by the core).
  Future<CoreAnswer> applyDeviceList(Object? list) => _call({'op': 'apply_device_list', 'device_list': list});

  /// Check the badges of a profile in the core. Only valid ones come back
  /// (`badges`), plus `refused` and `name_warning`. Works while locked.
  Future<CoreAnswer> verifyBadges(String holderKeyHex, String name, List<Object?> badges) =>
      _call({'op': 'verify_badges', 'holder': holderKeyHex, 'name': name, 'badges': badges});

  /// Hide the window from screenshots. Returns false where unsupported.
  Future<bool> protectWindow(bool on) async => (await _call({'op': 'protect_window', 'on': on})).ok;

  /// The squares of a QR code for [text], computed by the core.
  Future<CoreAnswer> qr(String text) => _call({'op': 'qr', 'text': text});

  /// The Windows file dialog for a picture: `path`, or error `cancelled`.
  Future<CoreAnswer> pickImage() => _call({'op': 'pick_image'});

  // ----- chats (M4): the core keeps keys, contacts and history -----

  Future<CoreAnswer> contactCode() => _call({'op': 'contact_code'});
  Future<CoreAnswer> addContact(String code, String name) => _call({'op': 'add_contact', 'code': code, 'name': name});
  Future<CoreAnswer> contacts() => _call({'op': 'contacts'});
  Future<CoreAnswer> confirmContact(String user) => _call({'op': 'confirm_contact', 'user': user});
  Future<CoreAnswer> chats() => _call({'op': 'chats'});
  Future<CoreAnswer> messages(String chat, {int limit = 200}) =>
      _call({'op': 'messages', 'chat': chat, 'limit': limit});
  Future<CoreAnswer> startChat(String user, List<Object?> offers) =>
      _call({'op': 'start_chat', 'user': user, 'offers': offers});
  Future<CoreAnswer> removeDevices(String chat, List<String> ids) =>
      _call({'op': 'remove_devices', 'chat': chat, 'ids': ids});
  Future<CoreAnswer> updateKeys(String chat) => _call({'op': 'update_keys', 'chat': chat});
  Future<CoreAnswer> withdrawnMembers(String chat) => _call({'op': 'withdrawn_members', 'chat': chat});
  Future<CoreAnswer> pendingCommits() => _call({'op': 'pending_commits'});
  Future<CoreAnswer> commitAnswer(String chat, bool accepted, String? taken) =>
      _call({'op': 'commit_answer', 'chat': chat, 'accepted': accepted, 'taken': ?taken});
  Future<CoreAnswer> sendMessage(String chat, String text) => _call({'op': 'send_message', 'chat': chat, 'text': text});
  Future<CoreAnswer> receive(String body) => _call({'op': 'receive', 'body': body});
  Future<CoreAnswer> acceptRequest(String chat, String name) =>
      _call({'op': 'accept_request', 'chat': chat, 'name': name});
  Future<CoreAnswer> declineRequest(String chat) => _call({'op': 'decline_request', 'chat': chat});
  Future<CoreAnswer> keyPackages(int n, {bool lastResort = false}) =>
      _call({'op': 'key_packages', 'n': n, 'last_resort': lastResort});
  Future<CoreAnswer> applyContactList(String accountKey, Object? list) =>
      _call({'op': 'apply_contact_list', 'account_key': accountKey, 'device_list': list});

  // ----- groups (M5): the core checks every change against the rules -----

  Future<CoreAnswer> createGroup(String name, int? minutes, List<Object?> offers) =>
      _call({'op': 'create_group', 'name': name, 'minutes': minutes, 'offers': offers});
  Future<CoreAnswer> groupInfo(String chat) => _call({'op': 'group_info', 'chat': chat});
  Future<CoreAnswer> addToGroup(String chat, List<Object?> offers) =>
      _call({'op': 'add_to_group', 'chat': chat, 'offers': offers});
  Future<CoreAnswer> kick(String chat, String user) => _call({'op': 'kick', 'chat': chat, 'user': user});
  Future<CoreAnswer> setAdmin(String chat, String user, bool admin) =>
      _call({'op': 'set_admin', 'chat': chat, 'user': user, 'admin': admin});
  Future<CoreAnswer> handOver(String chat, String user) => _call({'op': 'hand_over', 'chat': chat, 'user': user});
  Future<CoreAnswer> renameGroup(String chat, String name) => _call({'op': 'rename_group', 'chat': chat, 'name': name});
  Future<CoreAnswer> setGroupEnd(String chat, int? minutes) =>
      _call({'op': 'set_group_end', 'chat': chat, 'minutes': minutes});
  Future<CoreAnswer> closeGroup(String chat) => _call({'op': 'close_group', 'chat': chat});
  Future<CoreAnswer> leavePrepare(String chat) => _call({'op': 'leave_prepare', 'chat': chat});
  Future<CoreAnswer> leaveGroup(String chat) => _call({'op': 'leave_group', 'chat': chat});
  Future<CoreAnswer> acceptGroup(String chat) => _call({'op': 'accept_group', 'chat': chat});

  // Bots (M9)

  /// Sign the request code of a bot this account vouches for.
  Future<CoreAnswer> vouchBot(String code) => _call({'op': 'vouch_bot', 'code': code});

  /// Check a bot's certificate against its owner's key (hex, from the
  /// server; a contact's key must match).
  Future<CoreAnswer> checkBot(String user, String ownerKey) =>
      _call({'op': 'check_bot', 'user': user, 'owner_key': ownerKey});
  Future<CoreAnswer> deleteChat(String chat) => _call({'op': 'delete_chat', 'chat': chat});
  Future<CoreAnswer> acceptNewKey(String chat, String user) =>
      _call({'op': 'accept_new_key', 'chat': chat, 'user': user});
  Future<CoreAnswer> proposeUpdate(String chat) => _call({'op': 'propose_update', 'chat': chat});
  Future<CoreAnswer> tidyGroup(String chat) => _call({'op': 'tidy_group', 'chat': chat});
  Future<CoreAnswer> createGroupInvite(String chat, int minutes, {bool guest = false}) =>
      _call({'op': 'create_group_invite', 'chat': chat, 'minutes': minutes, 'guest': guest});
  Future<CoreAnswer> revokeGroupInvites(String chat) => _call({'op': 'revoke_group_invites', 'chat': chat});
  Future<CoreAnswer> groupInvites(String chat) => _call({'op': 'group_invites', 'chat': chat});
  Future<CoreAnswer> joinGroup(String code, List<Object?> offers) =>
      _call({'op': 'join_group', 'code': code, 'offers': offers});
  Future<CoreAnswer> joinMessage(String chat) => _call({'op': 'join_message', 'chat': chat});
  Future<CoreAnswer> joinRequests() => _call({'op': 'join_requests'});
  Future<CoreAnswer> admit(String chat, List<Object?> offers) => _call({'op': 'admit', 'chat': chat, 'offers': offers});
  Future<CoreAnswer> closeJoin(String chat) => _call({'op': 'close_join', 'chat': chat});
  Future<CoreAnswer> denyJoin(String chat) => _call({'op': 'deny_join', 'chat': chat});

  // ----- communities (M8): signatures of open channels -----

  /// Sign a message for an open channel with this device.
  Future<CoreAnswer> signOpenMessage(String community, String channel, String text, int sent) =>
      _call({'op': 'sign_open_message', 'community': community, 'channel': channel, 'text': text, 'sent': sent});

  /// One word per message: own, verified, contact, known, new, changed, bad.
  Future<CoreAnswer> checkOpenMessages(
    String community,
    String channel,
    List<Map<String, Object?>> messages,
    Map<String, Object?> authors,
  ) => _call({
    'op': 'check_open_messages',
    'community': community,
    'channel': channel,
    'messages': messages,
    'authors': authors,
  });

  Future<CoreAnswer> acceptOpenKey(String user, String accountKey) =>
      _call({'op': 'accept_open_key', 'user': user, 'account_key': accountKey});

  /// The own team badge as signed (public), for the team inbox.
  Future<CoreAnswer> teamBadge() => _call({'op': 'team_badge'});

  // ----- closed channels (M8, step 3b) -----

  Future<CoreAnswer> createChannelGroup(
    String group,
    String name,
    String community,
    String channel,
    List<Object?> offers,
  ) => _call({
    'op': 'create_channel_group',
    'group': group,
    'name': name,
    'community': community,
    'channel': channel,
    'offers': offers,
  });
  Future<CoreAnswer> addToChannel(String chat, List<Object?> offers) =>
      _call({'op': 'add_to_channel', 'chat': chat, 'offers': offers});
  Future<CoreAnswer> channelRemove(String chat, String user) =>
      _call({'op': 'channel_remove', 'chat': chat, 'user': user});
  Future<CoreAnswer> setKeepers(String chat, List<String> keepers) =>
      _call({'op': 'set_keepers', 'chat': chat, 'keepers': keepers});
  Future<CoreAnswer> channelHistory(String chat, String user) =>
      _call({'op': 'channel_history', 'chat': chat, 'user': user});
  Future<CoreAnswer> deleteChannelMessage(String chat, int id) =>
      _call({'op': 'delete_channel_message', 'chat': chat, 'id': id});

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
      return a.wait > 0 ? 'Passwort falsch. Zu viele Versuche: bitte ${a.wait} Sekunden warten.' : 'Passwort falsch.';
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
    case 'weak_pin':
      return 'Die PIN braucht mindestens 6 Zeichen.';
    case 'weak_sending_pin':
      return 'Eine PIN, die vorher schreibt, öffnet die Schlüssel. Sie braucht mindestens 12 Zeichen, wie das Passwort.';
    case 'pin_is_password':
      return 'Passwort und Notfall-PIN dürfen nicht gleich sein.';
    case 'pin_taken':
      return 'Diese PIN ist schon vergeben.';
    case 'full':
      return 'Mehr als 3 Notfall-PINs gehen nicht.';
    case 'unknown_chat':
      return 'Diesen Chat gibt es nicht mehr.';
    case 'chats_unavailable':
      return 'Die Chats sind gerade nicht offen.';
    default:
      return 'Unerwarteter Fehler im Sicherheitskern (${a.error}).';
  }
}
