// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../community_state.dart';
import '../theme.dart';
import '../widgets/common.dart';
import '../widgets/trair_icons.dart';

/// The frame all community dialogs share.
class DialogShell extends StatelessWidget {
  const DialogShell({super.key, required this.title, required this.children, this.width = 420});

  final String title;
  final List<Widget> children;
  final double width;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Material(
        type: MaterialType.transparency,
        child: Container(
          width: width,
          constraints: const BoxConstraints(maxHeight: 620),
          padding: const EdgeInsets.all(22),
          decoration: BoxDecoration(
            color: TrairColors.glassPopup,
            borderRadius: BorderRadius.circular(20),
            border: Border.all(color: TrairColors.border),
          ),
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(title, style: TrairText.heading),
                const SizedBox(height: 10),
                ...children,
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// A multi-line text box in the app's look.
class NoteField extends StatelessWidget {
  const NoteField({
    super.key,
    required this.controller,
    required this.hint,
    this.lines = 3,
    this.maxLength,
    this.fieldKey,
    this.autofocus = false,
  });

  final TextEditingController controller;
  final String hint;
  final int lines;
  final int? maxLength;
  final Key? fieldKey;
  final bool autofocus;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: BoxDecoration(
        color: TrairColors.field,
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: TrairColors.borderSoft),
      ),
      child: TextField(
        key: fieldKey,
        controller: controller,
        autofocus: autofocus,
        minLines: lines,
        maxLines: lines + 4,
        maxLength: maxLength,
        style: const TextStyle(fontSize: 14.5, color: TrairColors.text, height: 1.45),
        decoration: InputDecoration.collapsed(
          hintText: hint,
          hintStyle: const TextStyle(color: TrairColors.placeholder, fontSize: 14.5),
        ),
      ),
    );
  }
}

/// A switch with its words in front; the whole row switches.
class LabeledSwitch extends StatelessWidget {
  const LabeledSwitch({
    super.key,
    required this.value,
    required this.label,
    required this.onChanged,
    this.enabled = true,
  });

  final bool value;
  final String label;
  final ValueChanged<bool> onChanged;

  /// Off: shown dimmed and cannot be switched (a right this account lacks).
  final bool enabled;

  @override
  Widget build(BuildContext context) => InkWell(
    borderRadius: BorderRadius.circular(12),
    onTap: enabled ? () => onChanged(!value) : null,
    child: Padding(
      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 5),
      child: Row(
        children: [
          Expanded(
            child: Text(
              label,
              style: TextStyle(fontSize: 14, color: enabled ? TrairColors.text : TrairColors.textMuted),
            ),
          ),
          const SizedBox(width: 12),
          Opacity(
            opacity: enabled ? 1 : 0.45,
            child: PillSwitch(value: value, label: label, onChanged: enabled ? onChanged : (_) {}),
          ),
        ],
      ),
    ),
  );
}

Widget _buttons(BuildContext ctx, String yes, VoidCallback? onYes, {PillKind kind = PillKind.primary}) => Row(
  mainAxisAlignment: MainAxisAlignment.end,
  children: [
    PillButton(label: 'Abbrechen', kind: PillKind.ghost, height: 38, onPressed: () => Navigator.pop(ctx)),
    const SizedBox(width: 8),
    PillButton(label: yes, kind: kind, height: 38, onPressed: onYes),
  ],
);

/// Ask for a text (a reason, a name). Null: cancelled.
Future<String?> askText(
  BuildContext context, {
  required String title,
  required String text,
  required String hint,
  required String yes,
  bool needed = true,
  int lines = 3,
  int maxLength = 1000,
  String initial = '',
  PillKind kind = PillKind.primary,
}) => showDialog<String>(
  context: context,
  barrierColor: const Color(0x9905050A),
  builder: (_) => _AskText(
    title: title,
    text: text,
    hint: hint,
    yes: yes,
    needed: needed,
    lines: lines,
    maxLength: maxLength,
    initial: initial,
    kind: kind,
  ),
);

/// The field lives as long as the dialog (also while it fades out).
class _AskText extends StatefulWidget {
  const _AskText({
    required this.title,
    required this.text,
    required this.hint,
    required this.yes,
    required this.needed,
    required this.lines,
    required this.maxLength,
    required this.initial,
    required this.kind,
  });

  final String title;
  final String text;
  final String hint;
  final String yes;
  final bool needed;
  final int lines;
  final int maxLength;
  final String initial;
  final PillKind kind;

  @override
  State<_AskText> createState() => _AskTextState();
}

class _AskTextState extends State<_AskText> {
  late final _c = TextEditingController(text: widget.initial);

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => DialogShell(
    title: widget.title,
    children: [
      if (widget.text.isNotEmpty) ...[Text(widget.text, style: TrairText.muted), const SizedBox(height: 12)],
      NoteField(
        controller: _c,
        hint: widget.hint,
        lines: widget.lines,
        maxLength: widget.maxLength,
        autofocus: true,
        fieldKey: const Key('ask-text'),
      ),
      const SizedBox(height: 14),
      ListenableBuilder(
        listenable: _c,
        builder: (_, _) => _buttons(
          context,
          widget.yes,
          widget.needed && _c.text.trim().isEmpty ? null : () => Navigator.pop(context, _c.text.trim()),
          kind: widget.kind,
        ),
      ),
    ],
  );
}

/// Found a community or join one with a code.
Future<void> showNewCommunity(BuildContext context, AppState state) {
  return showDialog<void>(
    context: context,
    barrierColor: const Color(0x9905050A),
    builder: (_) => _NewCommunity(state: state),
  );
}

class _NewCommunity extends StatefulWidget {
  const _NewCommunity({required this.state});

  final AppState state;

  @override
  State<_NewCommunity> createState() => _NewCommunityState();
}

class _NewCommunityState extends State<_NewCommunity> {
  final _name = TextEditingController();
  final _code = TextEditingController();
  bool _join = false;
  bool _busy = false;
  String _error = '';

  @override
  void dispose() {
    _name.dispose();
    _code.dispose();
    super.dispose();
  }

  Future<void> _go() async {
    final c = widget.state.communities;
    setState(() {
      _busy = true;
      _error = '';
    });
    final id = _join ? await c.join(_code.text) : await c.create(_name.text);
    if (!mounted) return;
    if (id == null) {
      setState(() {
        _busy = false;
        _error = c.message;
      });
      return;
    }
    Navigator.pop(context);
    widget.state.go(TrairScreen.community);
  }

  @override
  Widget build(BuildContext context) {
    return DialogShell(
      title: _join ? 'Community beitreten' : 'Community gründen',
      children: [
        Row(
          children: [
            Expanded(
              child: PillButton(
                label: 'Gründen',
                icon: TrairIcons.plus,
                height: 38,
                kind: _join ? PillKind.ghost : PillKind.primary,
                onPressed: () => setState(() => _join = false),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: PillButton(
                label: 'Beitreten',
                icon: TrairIcons.doorOpen,
                height: 38,
                kind: _join ? PillKind.primary : PillKind.ghost,
                onPressed: () => setState(() => _join = true),
              ),
            ),
          ],
        ),
        const SizedBox(height: 16),
        if (_join) ...[
          const Text('Einladungscode', style: TrairText.small),
          const SizedBox(height: 6),
          PillField(
            controller: _code,
            hint: 'trairc:…',
            mono: true,
            autofocus: true,
            semanticLabel: 'Einladungscode',
            onSubmitted: (_) => _go(),
          ),
        ] else ...[
          const Text('Name der Community', style: TrairText.small),
          const SizedBox(height: 6),
          PillField(
            controller: _name,
            hint: 'z. B. Abi 2026',
            autofocus: true,
            semanticLabel: 'Name der Community',
            onSubmitted: (_) => _go(),
          ),
          const SizedBox(height: 12),
          const Text(
            'Du bekommst die Rollen Admin und Moderator und die Kanäle #regeln und #allgemein. '
            'Die Kanäle sind offen: Der Server kann sie lesen, damit Moderation geht. '
            'Jede Nachricht ist vom Gerät unterschrieben, fälschen kann der Server sie nicht.',
            style: TextStyle(fontSize: 12.5, height: 1.5, color: TrairColors.textMuted),
          ),
        ],
        if (_error.isNotEmpty) ...[
          const SizedBox(height: 10),
          Text(_error, style: const TextStyle(fontSize: 13, color: TrairColors.rose)),
        ],
        const SizedBox(height: 16),
        _buttons(context, _join ? 'Beitreten' : 'Gründen', _busy ? null : _go),
      ],
    );
  }
}

/// The rules; members accept them before writing.
Future<void> showRules(BuildContext context, AppState state, CommunityInfo info) {
  return showDialog<void>(
    context: context,
    barrierColor: const Color(0x9905050A),
    builder: (ctx) => DialogShell(
      title: 'Regeln von „${info.name}“',
      width: 480,
      children: [
        Container(
          padding: const EdgeInsets.all(14),
          decoration: BoxDecoration(color: TrairColors.field, borderRadius: BorderRadius.circular(14)),
          child: Text(
            info.rules.isEmpty ? 'Diese Community hat keine Regeln.' : info.rules,
            style: const TextStyle(fontSize: 14, height: 1.55, color: TrairColors.text),
          ),
        ),
        const SizedBox(height: 16),
        if (info.rulesOk)
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [PillButton(label: 'Schließen', height: 38, onPressed: () => Navigator.pop(ctx))],
          )
        else
          _buttons(ctx, 'Akzeptieren', () async {
            Navigator.pop(ctx);
            await state.communities.acceptRules();
          }),
      ],
    ),
  );
}

/// Report someone to the Trair team (from a profile). With [chatMessages]
/// the reporter may attach the last messages of the chat with them; they
/// leave the end-to-end encryption then, and the dialog says so.
Future<bool> showTeamReport(
  BuildContext context,
  AppState state, {
  required String user,
  required String name,
  List<Map<String, Object?>> chatMessages = const [],
}) async {
  final answer = await showDialog<(String, bool)>(
    context: context,
    barrierColor: const Color(0x9905050A),
    builder: (_) => _TeamReport(name: name, messages: chatMessages.length),
  );
  if (answer == null) return false;
  final (reason, attach) = answer;
  final ok = await state.communities.reportToTeam(user, reason, evidence: attach ? chatMessages : null);
  if (context.mounted) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(
          ok ? 'Gemeldet. Das Trair-Team sieht es jetzt.' : 'Melden ging nicht: ${state.communities.message}',
        ),
      ),
    );
  }
  return ok;
}

class _TeamReport extends StatefulWidget {
  const _TeamReport({required this.name, required this.messages});

  final String name;

  /// How many chat messages could be attached.
  final int messages;

  @override
  State<_TeamReport> createState() => _TeamReportState();
}

class _TeamReportState extends State<_TeamReport> {
  final _reason = TextEditingController();
  bool _attach = false;

  @override
  void dispose() {
    _reason.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => DialogShell(
    title: '${widget.name} melden',
    children: [
      const Text(
        'Die Meldung geht an das Trair-Team, nicht an eine Community. '
        'Für Nachrichten in einer Community nimm „Melden“ an der Nachricht.',
        style: TrairText.muted,
      ),
      const SizedBox(height: 12),
      NoteField(controller: _reason, hint: 'Was ist passiert?', maxLength: 1000, autofocus: true),
      if (widget.messages > 0) ...[
        const SizedBox(height: 10),
        LabeledSwitch(
          value: _attach,
          label: 'Die letzten ${widget.messages} Nachrichten aus eurem Chat anhängen',
          onChanged: (v) => setState(() => _attach = v),
        ),
        if (_attach)
          const Padding(
            padding: EdgeInsets.only(top: 6),
            child: Text(
              'Diese Nachrichten sind dann nicht mehr Ende-zu-Ende verschlüsselt: '
              'Das Trair-Team und der Server können sie lesen.',
              style: TextStyle(fontSize: 12.5, height: 1.5, color: TrairColors.amber),
            ),
          ),
      ],
      const SizedBox(height: 14),
      ListenableBuilder(
        listenable: _reason,
        builder: (_, _) => _buttons(
          context,
          'An Trair-Team senden',
          _reason.text.trim().isEmpty ? null : () => Navigator.pop(context, (_reason.text.trim(), _attach)),
          kind: PillKind.danger,
        ),
      ),
    ],
  );
}
