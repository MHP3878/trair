// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:async';

import 'package:trair_app/core/server.dart';

/// Stands in for the community API of crates/trair-server in widget tests.
/// Follows its answers and its main rules (rights, rules, bans); every
/// right is tested for real in crates/trair-server/tests/communities.rs and
/// against the real server in test/server_native_test.dart.
class FakeCommunities {
  /// The account the app is logged in as (FakeDoor's user).
  static const me = 'local-julian';

  final Map<String, Map<String, Object?>> communities = {};

  /// Every op as sent, in order.
  final List<Map<String, Object?>> ops = [];

  final List<Map<String, Object?>> _events = [];
  int _seq = 0;
  int _ids = 0;
  int _messageIds = 0;
  Completer<void>? _woken;

  ServerReply _ok([Map<String, Object?> extra = const {}]) => ServerReply(200, {'ok': true, ...extra});
  ServerReply _fail(String code, [int status = 403]) => ServerReply(status, {'ok': false, 'error': code});

  void _event(String id, String kind, {String channel = '', int ref = 0, String target = ''}) {
    _events.add({
      'seq': ++_seq,
      'community': id,
      'kind': kind,
      'channel': channel,
      'ref': ref,
      'target': target,
    });
    _woken?.complete();
    _woken = null;
  }

  // ----- what tests set up -----

  /// A community owned by [owner] with the two default roles and channels.
  String found(String name, {String owner = me}) {
    final id = 'k${++_ids}';
    communities[id] = {
      'name': name,
      'about': '',
      'rules': '',
      'rules_rev': 0,
      'owner': owner,
      'roles': <Map<String, Object?>>[
        {'id': 1, 'name': 'Admin', 'color': 0xFFFF8A98, 'position': 20, 'perms': 511},
        {'id': 2, 'name': 'Moderator', 'color': 0xFF5EE6B0, 'position': 10, 'perms': 8 | 32 | 128 | 256},
        {'id': 0, 'name': '@alle', 'color': 0xFF9A98AA, 'position': 0, 'perms': 0},
      ],
      'channels': <Map<String, Object?>>[
        {
          'id': 'regeln',
          'category': 'Allgemein',
          'name': 'regeln',
          'topic': 'Bitte einmal lesen',
          'position': 0,
          'read_roles': null,
          'write_roles': <int>[],
        },
        {
          'id': 'allgemein',
          'category': 'Allgemein',
          'name': 'allgemein',
          'topic': 'Für alles',
          'position': 1,
          'read_roles': null,
          'write_roles': null,
        },
      ],
      'members': <String, Map<String, Object?>>{
        owner: {'roles': <int>[], 'rules_rev': 0},
      },
      'messages': <Map<String, Object?>>[],
      'bans': <Map<String, Object?>>[],
      'reports': <Map<String, Object?>>[],
      'log': <Map<String, Object?>>[],
      'invites': <String, Map<String, Object?>>{},
    };
    return id;
  }

  Map<String, Map<String, Object?>> _members(String id) =>
      (communities[id]!['members'] as Map).cast<String, Map<String, Object?>>();

  /// [user] joins (or is put in) [id] with these roles.
  void addMember(String id, String user, {List<int> roles = const []}) {
    _members(id)[user] = {'roles': [...roles], 'rules_rev': 0};
    _event(id, 'joined', target: user);
  }

  void setRules(String id, String rules) {
    final c = communities[id]!;
    c['rules'] = rules;
    c['rules_rev'] = (c['rules_rev'] as int) + 1;
    _event(id, 'changed');
  }

  /// Someone else writes (signed by their device, as far as the fake goes).
  int postAs(String id, String channel, String user, String text, {int sent = 1800000000}) {
    final m = {
      'id': ++_messageIds,
      'channel': channel,
      'user': user,
      'device': 'd-$user',
      'text': text,
      'sent': sent,
      'sig': 'sig-$text',
      'at': sent,
    };
    (communities[id]!['messages'] as List).add(m);
    _event(id, 'message', channel: channel, ref: m['id'] as int);
    return m['id'] as int;
  }

  void addReport(String id, {required String reporter, required String target, required String reason, String text = ''}) {
    final reports = communities[id]!['reports'] as List;
    reports.add({
      'id': reports.length + 1,
      'community': id,
      'reporter': reporter,
      'target': target,
      'channel': null,
      'message': null,
      'reason': reason,
      'snapshot': text.isEmpty ? null : {'user': target, 'channel': 'allgemein', 'text': text, 'sent': 1800000000},
      'at': 1800000000,
      'status': 'open',
      'handled_by': null,
      'note': '',
    });
  }

  /// Team reports for the team inbox.
  final List<Map<String, Object?>> teamReports = [];

  // ----- the rules -----

  List<Map<String, Object?>> _roles(String id) => (communities[id]!['roles'] as List).cast<Map<String, Object?>>();

  int _perms(String id, String user) {
    final c = communities[id]!;
    if (c['owner'] == user) return 511;
    final mine = (_members(id)[user]?['roles'] as List? ?? const []).cast<int>();
    var p = 0;
    for (final r in _roles(id)) {
      if (r['id'] == 0 || mine.contains(r['id'])) p |= r['perms'] as int;
    }
    return p;
  }

  int _power(String id, String user) {
    final c = communities[id]!;
    if (c['owner'] == user) return 1 << 30;
    final mine = (_members(id)[user]?['roles'] as List? ?? const []).cast<int>();
    var p = 0;
    for (final r in _roles(id)) {
      if (mine.contains(r['id']) && (r['position'] as int) > p) p = r['position'] as int;
    }
    return p;
  }

  bool _above(String id, String actor, String target) =>
      communities[id]!['owner'] != target && _power(id, actor) > _power(id, target);

  bool _listed(String id, String user, Object? list) {
    if (list == null) return true;
    final mine = (_members(id)[user]?['roles'] as List? ?? const []).cast<int>();
    return (list as List).cast<int>().any((r) => r == 0 || mine.contains(r));
  }

  bool _canRead(String id, String user, Map<String, Object?> ch) =>
      _perms(id, user) & 2 != 0 || _listed(id, user, ch['read_roles']);

  bool _canWrite(String id, String user, Map<String, Object?> ch) =>
      _perms(id, user) & 2 != 0 || (_canRead(id, user, ch) && _listed(id, user, ch['write_roles']));

  bool _rulesOk(String id, String user) {
    final c = communities[id]!;
    return (c['rules'] as String).isEmpty ||
        c['owner'] == user ||
        (_members(id)[user]!['rules_rev'] as int) >= (c['rules_rev'] as int);
  }

  void _log(String id, String action, String target, [String detail = '']) {
    (communities[id]!['log'] as List).insert(0, {
      'id': (communities[id]!['log'] as List).length + 1,
      'actor': me,
      'action': action,
      'target': target,
      'detail': detail,
      'at': 1800000000,
    });
  }

  // ----- the API -----

  Future<ServerReply> sync(int since, int wait) async {
    if (since < 0) return _ok({'events': <Object?>[], 'seq': _seq});
    List<Map<String, Object?>> fresh() => [
      for (final e in _events)
        if ((e['seq'] as int) > since &&
            (e['target'] == me ||
                (communities.containsKey(e['community']) && _members(e['community'] as String).containsKey(me))))
          _withMessage(e),
    ];
    var out = fresh();
    if (out.isEmpty && wait > 0) {
      await (_woken ??= Completer<void>()).future;
      out = fresh();
    }
    return _ok({'events': out, 'seq': out.isEmpty ? since : out.last['seq']});
  }

  Map<String, Object?> _withMessage(Map<String, Object?> e) {
    if (e['kind'] != 'message') return e;
    final id = e['community'] as String;
    final m = (communities[id]?['messages'] as List? ?? const []).cast<Map<String, Object?>>().where(
      (m) => m['id'] == e['ref'],
    );
    return {...e, if (m.isNotEmpty) 'message': m.first};
  }

  Future<ServerReply> op(Map<String, Object?> r) async {
    ops.add(r);
    final kind = r['op'] as String;
    if (kind == 'create') {
      final name = (r['name'] as String).trim();
      if (name.length < 2) return _fail('bad_name', 400);
      final id = found(name);
      return _ok({'community': id});
    }
    if (kind == 'list') {
      return _ok({
        'communities': [
          for (final e in communities.entries)
            if (_members(e.key).containsKey(me)) {'id': e.key, 'name': e.value['name'], 'owner': e.value['owner']},
        ],
        'seq': _seq,
      });
    }
    if (kind == 'join') {
      final code = (r['code'] as String).trim();
      for (final e in communities.entries) {
        final inv = (e.value['invites'] as Map)[code];
        if (inv == null) continue;
        if ((e.value['bans'] as List).any((b) => (b as Map)['user'] == me)) return _fail('banned');
        if (!_members(e.key).containsKey(me)) addMember(e.key, me);
        return _ok({'community': e.key});
      }
      return _fail('bad_invite', 404);
    }
    if (kind == 'report' && r['team'] == true) {
      teamReports.add({...r, 'id': teamReports.length + 1, 'reporter': me, 'status': 'open'});
      return _ok({'id': teamReports.length});
    }
    if (kind == 'team_reports' || kind == 'team_handle') {
      if (r['badge'] == null) return _fail('not_allowed');
      if (kind == 'team_handle') {
        for (final t in teamReports) {
          if (t['id'] == r['id']) t['status'] = r['status'];
        }
        return _ok();
      }
      return _ok({
        'reports': [
          for (final t in teamReports)
            if (r['all'] == true || t['status'] == 'open')
              {
                'id': t['id'],
                'reporter': t['reporter'],
                'target': t['user'],
                'reason': t['reason'],
                'snapshot': t['evidence'] == null ? null : {'forwarded': t['evidence']},
                'at': 1800000000,
                'status': t['status'],
                'note': '',
              },
        ],
      });
    }
    final id = r['community'] as String;
    final c = communities[id];
    if (c == null) return _fail('not_found', 404);
    final members = _members(id);
    if (!members.containsKey(me)) return _fail('not_allowed');
    final perms = _perms(id, me);
    bool can(int p) => perms & p == p;
    final channels = (c['channels'] as List).cast<Map<String, Object?>>();
    final messages = (c['messages'] as List).cast<Map<String, Object?>>();
    switch (kind) {
      case 'info':
        final ok = _rulesOk(id, me);
        return _ok({
          'community': {
            'id': id,
            'name': c['name'],
            'about': c['about'],
            'rules': c['rules'],
            'rules_rev': c['rules_rev'],
            'owner': c['owner'],
            'created': 1800000000,
          },
          'me': {
            'perms': perms,
            'power': _power(id, me),
            'roles': members[me]!['roles'],
            'owner': c['owner'] == me,
            'rules_ok': ok,
          },
          'roles': _roles(id),
          'channels': [
            for (final ch in channels)
              if (_canRead(id, me, ch)) {...ch, 'can_write': ok && _canWrite(id, me, ch)},
          ],
          'members': [
            for (final e in members.entries)
              {'user': e.key, 'roles': e.value['roles'], 'owner': c['owner'] == e.key, 'power': _power(id, e.key)},
          ],
        });
      case 'accept_rules':
        if (r['rev'] != c['rules_rev']) return _fail('rules_changed', 409);
        members[me]!['rules_rev'] = c['rules_rev'];
        return _ok();
      case 'edit':
        if (!can(1)) return _fail('not_allowed');
        if (r['rules'] != c['rules']) c['rules_rev'] = (c['rules_rev'] as int) + 1;
        c['name'] = r['name'];
        c['about'] = r['about'];
        c['rules'] = r['rules'];
        _log(id, 'edit', '');
        _event(id, 'changed');
        return _ok({'rules_rev': c['rules_rev']});
      case 'messages':
        final ch = channels.where((x) => x['id'] == r['channel']).firstOrNull;
        if (ch == null) return _fail('not_found', 404);
        if (!_canRead(id, me, ch)) return _fail('not_allowed');
        return _ok({
          'messages': [
            for (final m in messages)
              if (m['channel'] == r['channel']) m,
          ],
        });
      case 'post':
        final ch = channels.where((x) => x['id'] == r['channel']).firstOrNull;
        if (ch == null) return _fail('not_found', 404);
        if (!_canWrite(id, me, ch)) return _fail('not_allowed');
        if (!_rulesOk(id, me)) return _fail('accept_rules');
        if (r['sig'] != 'sig-${r['text']}') return _fail('bad_signature');
        final m = {
          'id': ++_messageIds,
          'channel': r['channel'],
          'user': me,
          'device': 'd-1a2b3c4d',
          'text': r['text'],
          'sent': r['sent'],
          'sig': r['sig'],
          'at': r['sent'],
        };
        messages.add(m);
        _event(id, 'message', channel: r['channel'] as String, ref: m['id'] as int);
        return _ok({'id': m['id'], 'at': m['at']});
      case 'delete_message':
        final m = messages.where((x) => x['id'] == r['id']).firstOrNull;
        if (m == null) return _fail('not_found', 404);
        if (m['user'] != me && !(can(32) && _above(id, me, m['user'] as String))) return _fail('not_allowed');
        messages.remove(m);
        if (m['user'] != me) _log(id, 'message_delete', m['user'] as String);
        _event(id, 'deleted', channel: m['channel'] as String, ref: m['id'] as int);
        return _ok();
      case 'kick' || 'ban':
        final target = r['user'] as String;
        if (!can(kind == 'kick' ? 8 : 16) || !_above(id, me, target)) return _fail('not_allowed');
        members.remove(target);
        if (kind == 'ban') {
          (c['bans'] as List).add({'user': target, 'by': me, 'reason': r['reason'] ?? '', 'at': 1800000000});
        }
        _log(id, kind, target, r['reason'] as String? ?? '');
        _event(id, 'removed', target: target);
        _event(id, 'changed');
        return _ok();
      case 'unban':
        if (!can(16)) return _fail('not_allowed');
        (c['bans'] as List).removeWhere((b) => (b as Map)['user'] == r['user']);
        return _ok();
      case 'bans':
        if (!can(16)) return _fail('not_allowed');
        return _ok({'bans': c['bans']});
      case 'set_roles':
        final target = r['user'] as String;
        if (!can(4) || (target != me && !_above(id, me, target))) return _fail('not_allowed');
        members[target]!['roles'] = (r['roles'] as List).cast<int>();
        _log(id, 'roles', target);
        _event(id, 'changed');
        return _ok();
      case 'create_role':
        if (!can(4)) return _fail('not_allowed');
        final rid = _roles(id).map((x) => x['id'] as int).reduce((a, b) => a > b ? a : b) + 1;
        _roles(id).insert(0, {
          'id': rid,
          'name': r['name'],
          'color': r['color'],
          'position': 30 + rid,
          'perms': r['perms'],
        });
        _log(id, 'role_create', r['name'] as String);
        _event(id, 'changed');
        return _ok({'role': rid});
      case 'edit_role':
        if (!can(4)) return _fail('not_allowed');
        final role = _roles(id).firstWhere((x) => x['id'] == r['role']);
        role
          ..['name'] = r['name']
          ..['color'] = r['color']
          ..['position'] = r['position']
          ..['perms'] = r['perms'];
        _event(id, 'changed');
        return _ok();
      case 'create_channel':
        if (!can(2)) return _fail('not_allowed');
        final ch = 'c${channels.length + 1}';
        channels.add({
          'id': ch,
          'category': r['category'],
          'name': r['name'],
          'topic': r['topic'] ?? '',
          'position': channels.length,
          'read_roles': r['read_roles'],
          'write_roles': r['write_roles'],
        });
        _log(id, 'channel_create', r['name'] as String);
        _event(id, 'changed', channel: ch);
        return _ok({'channel': ch});
      case 'create_invite':
        if (!can(64)) return _fail('not_allowed');
        final code = 'trairc:${'0' * 18}${(c['invites'] as Map).length + 1}0';
        (c['invites'] as Map)[code] = {'id': 'i${(c['invites'] as Map).length + 1}', 'uses': r['uses']};
        return _ok({'code': code, 'id': 'i1', 'ends': 1800086400, 'uses': r['uses']});
      case 'invites':
        if (!can(64)) return _fail('not_allowed');
        return _ok({
          'invites': [
            for (final i in (c['invites'] as Map).values)
              {'id': (i as Map)['id'], 'creator': me, 'uses': i['uses'], 'ends': 1800086400},
          ],
        });
      case 'report':
        final reports = c['reports'] as List;
        final mid = r['message'] as int?;
        final m = messages.where((x) => x['id'] == mid).firstOrNull;
        reports.add({
          'id': reports.length + 1,
          'community': id,
          'reporter': me,
          'target': m?['user'] ?? r['user'],
          'channel': m?['channel'],
          'message': mid,
          'reason': r['reason'],
          'snapshot': m == null ? null : {'user': m['user'], 'channel': m['channel'], 'text': m['text']},
          'at': 1800000000,
          'status': 'open',
          'handled_by': null,
          'note': '',
        });
        _event(id, 'report', ref: reports.length);
        return _ok({'id': reports.length});
      case 'reports':
        if (!can(128)) return _fail('not_allowed');
        return _ok({
          'reports': [
            for (final x in (c['reports'] as List).reversed)
              if (r['all'] == true || (x as Map)['status'] == 'open') x,
          ],
        });
      case 'handle_report':
        if (!can(128)) return _fail('not_allowed');
        for (final x in (c['reports'] as List).cast<Map<String, Object?>>()) {
          if (x['id'] == r['id']) {
            x['status'] = r['status'];
            x['note'] = r['note'] ?? '';
            x['handled_by'] = me;
          }
        }
        _log(id, 'report', '${r['id']}', r['status'] as String);
        return _ok();
      case 'log':
        if (!can(256)) return _fail('not_allowed');
        return _ok({'log': c['log']});
      case 'leave':
        if (c['owner'] == me) {
          if (members.length > 1) return _fail('hand_over_first', 409);
          communities.remove(id);
          return _ok({'deleted': true});
        }
        members.remove(me);
        _event(id, 'removed', target: me);
        return _ok();
      case 'delete':
        if (c['owner'] != me) return _fail('not_allowed');
        communities.remove(id);
        _event(id, 'removed', target: me);
        return _ok();
      case 'transfer':
        if (c['owner'] != me) return _fail('not_allowed');
        c['owner'] = r['user'];
        _event(id, 'changed');
        return _ok();
    }
    return _fail('not_found', 404);
  }
}
