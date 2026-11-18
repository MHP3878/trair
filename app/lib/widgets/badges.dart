// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../backend/backend.dart';
import '../theme.dart';
import 'common.dart';
import 'trair_icons.dart';

/// Colours and symbol of one kind of badge.
class _Look {
  const _Look(this.title, this.icon, this.light, this.dark, this.ink);

  final String title;

  /// Null: the Trair logo.
  final IconData? icon;
  final Color light;
  final Color dark;
  final Color ink;
}

const _looks = {
  BadgeKind.team: _Look('Trair-Team', null, Color(0xFFB8A6FF), Color(0xFF5A3BE6), Colors.white),
  BadgeKind.founder: _Look('Gründer', TrairIcons.crown, Color(0xFFFFE38F), Color(0xFFFF9A1F), Color(0xFF3B2100)),
  BadgeKind.admin: _Look('Admin', TrairIcons.shieldCheck, Color(0xFFFFA3B0), Color(0xFFE23D5A), Colors.white),
  BadgeKind.mod: _Look('Moderation', TrairIcons.gavel, Color(0xFF94F5CF), Color(0xFF17B386), Color(0xFF02281B)),
};

/// Short name of a badge, for example "Admin".
String badgeTitle(ProfileBadge b) => _looks[b.kind]!.title;

/// What a badge means, for the tooltip and screen readers.
String badgeMeaning(ProfileBadge b) {
  final c = b.community ?? '';
  final text = switch (b.kind) {
    BadgeKind.team => 'Gehört zum Trair-Team. Das Team fragt dich nie nach deinem Passwort.',
    BadgeKind.founder => 'Hat „$c“ gegründet.',
    BadgeKind.admin => 'Admin in „$c“.',
    BadgeKind.mod => 'Moderiert „$c“.',
  };
  return b.communityWarning ? '$text Der Name „$c“ wirkt offiziell, gehört aber nicht zu Trair.' : text;
}

/// One badge, like on Discord: a small glowing tile with a symbol. On
/// hover a shine runs across it; the tooltip says what it means and,
/// for community roles, where it comes from.
class TrairBadge extends StatefulWidget {
  const TrairBadge({super.key, required this.badge, this.size = 22});

  final ProfileBadge badge;
  final double size;

  @override
  State<TrairBadge> createState() => _TrairBadgeState();
}

class _TrairBadgeState extends State<TrairBadge> with SingleTickerProviderStateMixin {
  late final AnimationController _shine = AnimationController(vsync: this, duration: const Duration(milliseconds: 700));

  @override
  void dispose() {
    _shine.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final b = widget.badge;
    final look = _looks[b.kind]!;
    final s = widget.size;
    final radius = BorderRadius.circular(s * 0.3);
    final tile = b.kind == BadgeKind.team
        ? TrairLogo(size: s)
        : DecoratedBox(
            decoration: BoxDecoration(
              borderRadius: radius,
              gradient: LinearGradient(
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
                colors: [look.light, look.dark],
              ),
              border: Border.all(color: const Color(0x47FFFFFF)),
            ),
            child: SizedBox.square(
              dimension: s,
              child: Icon(look.icon, size: s * 0.6, color: look.ink),
            ),
          );
    return Tooltip(
      richMessage: TextSpan(
        children: [
          TextSpan(
            text: look.title,
            style: TextStyle(fontWeight: FontWeight.w700, color: look.light),
          ),
          TextSpan(text: '\n${badgeMeaning(b)}'),
        ],
      ),
      waitDuration: const Duration(milliseconds: 150),
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
      constraints: const BoxConstraints(maxWidth: 260),
      decoration: BoxDecoration(
        color: const Color(0xF2181824),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: TrairColors.border),
      ),
      textStyle: const TextStyle(fontSize: 12.5, height: 1.45, color: TrairColors.textSoft),
      child: MouseRegion(
        onEnter: (_) => _shine.forward(from: 0),
        child: Container(
          decoration: BoxDecoration(
            borderRadius: radius,
            boxShadow: [BoxShadow(color: look.dark.withAlpha(0x73), blurRadius: s * 0.5, spreadRadius: -s * 0.05)],
          ),
          child: Stack(
            clipBehavior: Clip.none,
            children: [
              ClipRRect(
                borderRadius: radius,
                child: Stack(
                  children: [
                    tile,
                    Positioned.fill(
                      child: AnimatedBuilder(
                        animation: _shine,
                        builder: (context, _) => _shine.isAnimating ? _Shine(progress: _shine.value) : const SizedBox(),
                      ),
                    ),
                  ],
                ),
              ),
              if (b.communityWarning)
                Positioned(
                  right: -2,
                  top: -2,
                  child: Container(
                    width: s * 0.36,
                    height: s * 0.36,
                    decoration: BoxDecoration(
                      color: TrairColors.amber,
                      shape: BoxShape.circle,
                      border: Border.all(color: TrairColors.background, width: 1.5),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// A diagonal light stripe that runs across the badge once.
class _Shine extends StatelessWidget {
  const _Shine({required this.progress});

  final double progress;

  @override
  Widget build(BuildContext context) {
    final x = -1.6 + progress * 3.2;
    return IgnorePointer(
      child: DecoratedBox(
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment(x - 0.6, -1),
            end: Alignment(x + 0.6, 1),
            colors: const [Color(0x00FFFFFF), Color(0x8CFFFFFF), Color(0x00FFFFFF)],
            stops: const [0.3, 0.5, 0.7],
          ),
        ),
      ),
    );
  }
}

/// Several badges in a row.
class BadgeRow extends StatelessWidget {
  const BadgeRow({super.key, required this.badges, this.size = 18, this.spacing = 5});

  final List<ProfileBadge> badges;
  final double size;
  final double spacing;

  @override
  Widget build(BuildContext context) {
    if (badges.isEmpty) return const SizedBox.shrink();
    return Semantics(
      label: 'Abzeichen: ${badges.map(badgeTitle).join(', ')}',
      child: Wrap(
        spacing: spacing,
        runSpacing: spacing,
        children: [for (final b in badges) TrairBadge(badge: b, size: size)],
      ),
    );
  }
}
