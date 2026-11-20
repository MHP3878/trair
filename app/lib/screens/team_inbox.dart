// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../community_state.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';
import 'community_dialogs.dart';
import 'group_panel.dart' show whenText;

/// Reports to the Trair team, for accounts with a valid team badge. The
/// server shows them only against that badge; the team tool on the
/// server's computer sees the same list.
class TeamInbox extends StatefulWidget {
  const TeamInbox({super.key});

  @override
  State<TeamInbox> createState() => _TeamInboxState();
}

class _TeamInboxState extends State<TeamInbox> {
  bool _all = false;
  bool _asked = false;

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final c = state.communities;
    if (!_asked) {
      _asked = true;
      WidgetsBinding.instance.addPostFrameCallback((_) => c.loadTeamReports());
    }
    final list = c.teamReports;
    String name(String user) => state.displayName(user);
    return SoftCard(
      key: const Key('team-inbox'),
      borderColor: const Color(0x335EE6B0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              const Icon(TrairIcons.shieldCheck, size: 18, color: TrairColors.mint),
              const SizedBox(width: 8),
              const Expanded(
                child: Text('Team-Meldungen', style: TextStyle(fontSize: 15, fontWeight: FontWeight.w600)),
              ),
              RoundIconButton(
                icon: TrairIcons.search,
                tooltip: 'Neu laden',
                size: 32,
                filled: false,
                onPressed: () => c.loadTeamReports(all: _all),
              ),
            ],
          ),
          const SizedBox(height: 6),
          const Text(
            'Meldungen an das Trair-Team. Du siehst sie, weil dein Team-Abzeichen gültig ist.',
            style: TrairText.muted,
          ),
          const SizedBox(height: 10),
          LabeledSwitch(
            value: _all,
            label: 'Auch erledigte zeigen',
            onChanged: (v) {
              setState(() => _all = v);
              c.loadTeamReports(all: v);
            },
          ),
          const SizedBox(height: 10),
          if (list == null)
            const Text('Lädt …', style: TrairText.muted)
          else if (list.isEmpty)
            const Text('Keine offenen Meldungen.', style: TrairText.muted)
          else
            for (final r in list) _card(c, r, name),
        ],
      ),
    );
  }

  Widget _card(Communities c, CReport r, String Function(String) name) {
    final target = r.target;
    return Container(
      margin: const EdgeInsets.only(bottom: 10),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(14)),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            [
              '#${r.id}',
              if (target != null) 'über ${name(target)} (@$target)',
              'von ${name(r.reporter)}',
              whenText(r.at),
              if (r.status == 'done') 'erledigt',
              if (r.status == 'dismissed') 'abgelehnt',
            ].join(' · '),
            style: TrairText.small,
          ),
          const SizedBox(height: 6),
          Text(r.reason, style: TrairText.body),
          if (r.quoted.isNotEmpty) ...[
            const SizedBox(height: 8),
            Text(r.quoted, style: const TextStyle(fontSize: 13, color: TrairColors.textSoft)),
          ],
          if (r.note.isNotEmpty) Text('Notiz: ${r.note}', style: TrairText.small),
          if (r.status == 'open') ...[
            const SizedBox(height: 10),
            Row(
              children: [
                PillButton(
                  label: 'Erledigt',
                  icon: TrairIcons.check,
                  height: 34,
                  onPressed: () async {
                    final note = await askText(
                      context,
                      title: 'Meldung #${r.id} erledigt',
                      text: 'Was wurde getan? (optional)',
                      hint: 'Notiz',
                      yes: 'Erledigt',
                      needed: false,
                    );
                    if (note != null) await c.teamHandle(r.id, 'done', note: note);
                  },
                ),
                const SizedBox(width: 8),
                PillButton(
                  label: 'Ablehnen',
                  kind: PillKind.ghost,
                  height: 34,
                  onPressed: () => c.teamHandle(r.id, 'dismissed'),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}
