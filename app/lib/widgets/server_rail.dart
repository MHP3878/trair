// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../theme.dart';
import 'common.dart';
import 'trair_icons.dart';

/// The rail on the far left, like Discord's server list: direct messages,
/// the communities, a button for more, and the settings at the bottom.
class ServerRail extends StatelessWidget {
  const ServerRail({super.key});

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final community = state.backend.community();
    return Container(
      width: 72,
      color: TrairColors.rail,
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: Column(
        children: [
          RailItem(
            tooltip: 'Direktnachrichten',
            selected: state.screen == TrairScreen.chats,
            onTap: () => state.go(TrairScreen.chats),
            builder: (_) => const TrairLogo(size: 48),
          ),
          Container(
            margin: const EdgeInsets.symmetric(vertical: 10),
            width: 32,
            height: 2,
            decoration: BoxDecoration(color: TrairColors.border, borderRadius: BorderRadius.circular(1)),
          ),
          RailItem(
            tooltip: community.name,
            selected: state.screen == TrairScreen.community,
            onTap: () => state.go(TrairScreen.community),
            builder: (on) => _Disc(
              color: on ? TrairColors.accent : TrairColors.glassLite,
              child: Text(
                community.initials,
                style: TextStyle(
                  fontSize: 15,
                  fontWeight: FontWeight.w600,
                  color: on ? Colors.white : TrairColors.accentPale,
                ),
              ),
            ),
          ),
          RailItem(
            tooltip: 'Community gründen oder beitreten',
            selected: false,
            onTap: () => ScaffoldMessenger.of(context).showSnackBar(
              const SnackBar(
                content: Text(
                  'Eigene Communities kommen mit den echten Gruppen. „Trair Beta“ ist bis dahin eine Demo.',
                ),
              ),
            ),
            builder: (on) => _Disc(
              color: on ? TrairColors.mint : TrairColors.glassLite,
              child: Icon(TrairIcons.plus, size: 22, color: on ? TrairColors.amberDark : TrairColors.mint),
            ),
          ),
          const Spacer(),
          RailItem(
            tooltip: 'Einstellungen',
            selected: state.screen == TrairScreen.profile,
            onTap: () => state.go(TrairScreen.profile),
            builder: (on) => _Disc(
              color: on ? TrairColors.accent : TrairColors.glassLite,
              child: Icon(TrairIcons.userCog, size: 20, color: on ? Colors.white : TrairColors.textSoft),
            ),
          ),
        ],
      ),
    );
  }
}

class _Disc extends StatelessWidget {
  const _Disc({required this.color, required this.child});

  final Color color;
  final Widget child;

  @override
  Widget build(BuildContext context) => ColoredBox(
    color: color,
    child: Center(child: child),
  );
}

/// One entry of the rail: round, a rounded square when chosen or
/// pointed at, with the little white bar on the left like Discord.
class RailItem extends StatefulWidget {
  const RailItem({
    super.key,
    required this.tooltip,
    required this.selected,
    required this.onTap,
    required this.builder,
  });

  final String tooltip;
  final bool selected;
  final VoidCallback onTap;

  /// The 48 × 48 content; the argument says whether it is highlighted.
  final Widget Function(bool highlighted) builder;

  @override
  State<RailItem> createState() => _RailItemState();
}

class _RailItemState extends State<RailItem> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final on = widget.selected || _hover;
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: SizedBox(
        width: 72,
        height: 48,
        child: Stack(
          alignment: Alignment.center,
          children: [
            Positioned(
              left: 0,
              child: AnimatedContainer(
                duration: const Duration(milliseconds: 180),
                width: 4,
                height: widget.selected ? 40 : (_hover ? 20 : 0),
                decoration: const BoxDecoration(
                  color: Colors.white,
                  borderRadius: BorderRadius.horizontal(right: Radius.circular(4)),
                ),
              ),
            ),
            Tooltip(
              message: widget.tooltip,
              preferBelow: false,
              child: Semantics(
                button: true,
                selected: widget.selected,
                label: widget.tooltip,
                child: MouseRegion(
                  cursor: SystemMouseCursors.click,
                  onEnter: (_) => setState(() => _hover = true),
                  onExit: (_) => setState(() => _hover = false),
                  child: GestureDetector(
                    onTap: widget.onTap,
                    child: TweenAnimationBuilder<double>(
                      tween: Tween(end: on ? 16 : 24),
                      duration: const Duration(milliseconds: 180),
                      builder: (context, r, child) => ClipRRect(
                        borderRadius: BorderRadius.circular(r),
                        child: SizedBox(width: 48, height: 48, child: widget.builder(on)),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
