// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'dart:ui' show ImageFilter;

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../theme.dart';
import 'trair_icons.dart';

/// Dark background with three soft, still color fields. The glass panels
/// need something behind them, otherwise the frosted effect is invisible.
class Backdrop extends StatelessWidget {
  const Backdrop({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Stack(
      children: [
        const Positioned.fill(child: ColoredBox(color: TrairColors.background)),
        Positioned(left: -160, top: -220, child: _blob(760, 560, const Color(0x4D5B47D6))),
        Positioned(right: -160, bottom: -220, child: _blob(680, 520, const Color(0x402C4FB8))),
        Align(alignment: const Alignment(0.15, -0.1), child: _blob(460, 360, const Color(0x403B2F8F))),
        // Transparent Material above the background colour, so ink and
        // hover effects of InkWells on the background stay visible.
        Positioned.fill(
          child: Material(type: MaterialType.transparency, child: child),
        ),
      ],
    );
  }

  Widget _blob(double w, double h, Color c) {
    return IgnorePointer(
      child: Container(
        width: w,
        height: h,
        decoration: BoxDecoration(gradient: RadialGradient(colors: [c, c.withAlpha(0)])),
      ),
    );
  }
}

/// A frosted glass surface.
class GlassPanel extends StatelessWidget {
  const GlassPanel({
    super.key,
    required this.child,
    this.radius = 28,
    this.padding = EdgeInsets.zero,
    this.lite = false,
    this.color,
    this.borderColor,
  });

  final Widget child;
  final double radius;
  final EdgeInsetsGeometry padding;
  final bool lite;
  final Color? color;
  final Color? borderColor;

  @override
  Widget build(BuildContext context) {
    final r = BorderRadius.circular(radius);
    final blur = lite ? 18.0 : 26.0;
    return ClipRRect(
      borderRadius: r,
      child: BackdropFilter(
        filter: ImageFilter.blur(sigmaX: blur, sigmaY: blur),
        child: Container(
          padding: padding,
          decoration: BoxDecoration(
            color: color ?? (lite ? TrairColors.glassLite : TrairColors.glass),
            borderRadius: r,
            border: Border.all(color: borderColor ?? TrairColors.border),
          ),
          // Same reason as in Backdrop: ink must paint above the glass.
          child: Material(type: MaterialType.transparency, child: child),
        ),
      ),
    );
  }
}

/// The Trair logo "Zylinder": a white lock cylinder with a T-shaped keyway
/// on a violet tile. Same drawing as assets/branding/trair-app-icon.svg.
class TrairLogo extends StatelessWidget {
  const TrairLogo({super.key, this.size = 32});

  final double size;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: 'Trair',
      image: true,
      child: DecoratedBox(
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(size * 58 / 256),
          boxShadow: const [BoxShadow(color: Color(0x59000000), blurRadius: 8, offset: Offset(0, 2))],
        ),
        child: CustomPaint(size: Size.square(size), painter: const _LogoPainter()),
      ),
    );
  }
}

class _LogoPainter extends CustomPainter {
  const _LogoPainter();

  @override
  void paint(Canvas canvas, Size size) {
    canvas.scale(size.width / 256);
    final tile = RRect.fromLTRBR(0, 0, 256, 256, const Radius.circular(58));
    canvas.drawRRect(
      tile,
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [Color(0xFF8A6CFF), Color(0xFF5A3BE6)],
        ).createShader(const Rect.fromLTWH(0, 0, 256, 256)),
    );
    canvas.drawRRect(
      tile.deflate(1),
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = const Color(0x29FFFFFF),
    );
    final keyway = Path.combine(
      PathOperation.union,
      Path()..addRRect(RRect.fromLTRBR(82, 92, 174, 118, const Radius.circular(13))),
      Path()..addRRect(RRect.fromLTRBR(115, 92, 141, 180, const Radius.circular(13))),
    );
    final disc = Path()..addOval(Rect.fromCircle(center: const Offset(128, 128), radius: 88));
    canvas.drawPath(Path.combine(PathOperation.difference, disc, keyway), Paint()..color = Colors.white);
    canvas.drawCircle(
      const Offset(128, 128),
      74,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 3
        ..color = const Color(0x2E5A3BE6),
    );
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

/// Round avatar with initials on a tinted background.
class Avatar extends StatelessWidget {
  const Avatar({super.key, required this.initials, required this.color, this.size = 40, this.square = false});

  final String initials;
  final Color color;
  final double size;
  final bool square;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: size,
      height: size,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: color.withAlpha(0x24),
        borderRadius: BorderRadius.circular(square ? size * 0.32 : size / 2),
      ),
      child: Text(
        initials,
        style: TextStyle(color: color, fontWeight: FontWeight.w600, fontSize: size * 0.36),
      ),
    );
  }
}

enum PillKind { primary, ghost, amber, danger, dashed }

/// Rounded button. `onPressed == null` shows it disabled.
class PillButton extends StatelessWidget {
  const PillButton({
    super.key,
    required this.label,
    required this.onPressed,
    this.kind = PillKind.primary,
    this.icon,
    this.height = 44,
    this.expand = false,
  });

  final String label;
  final VoidCallback? onPressed;
  final PillKind kind;
  final IconData? icon;
  final double height;
  final bool expand;

  @override
  Widget build(BuildContext context) {
    final (bg, fg, border) = switch (kind) {
      PillKind.primary => (TrairColors.accent, Colors.white, Colors.transparent),
      PillKind.ghost => (const Color(0x0DFFFFFF), TrairColors.textSoft, Colors.transparent),
      PillKind.amber => (TrairColors.amber, TrairColors.amberDark, Colors.transparent),
      PillKind.danger => (Colors.transparent, TrairColors.rose, const Color(0x4DFF8A98)),
      PillKind.dashed => (Colors.transparent, TrairColors.text, const Color(0x29FFFFFF)),
    };
    final enabled = onPressed != null;
    final text = Text(
      label,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(color: fg, fontWeight: FontWeight.w600, fontSize: 14),
    );
    final content = Row(
      mainAxisSize: expand ? MainAxisSize.max : MainAxisSize.min,
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        if (icon != null) ...[Icon(icon, size: 17, color: fg), const SizedBox(width: 8)],
        // Only an expanded button has a bounded width, so only then may the
        // label shrink; elsewhere the button sizes itself to the label.
        if (expand) Flexible(child: text) else text,
      ],
    );
    return Opacity(
      opacity: enabled ? 1 : 0.38,
      child: Material(
        color: bg,
        shape: StadiumBorder(side: BorderSide(color: border)),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onPressed,
          hoverColor: const Color(0x14FFFFFF),
          child: Container(
            height: height,
            padding: const EdgeInsets.symmetric(horizontal: 20),
            alignment: Alignment.center,
            child: content,
          ),
        ),
      ),
    );
  }
}

/// Round icon button.
class RoundIconButton extends StatelessWidget {
  const RoundIconButton({
    super.key,
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.size = 36,
    this.filled = true,
    this.color,
    this.iconColor,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final double size;
  final bool filled;
  final Color? color;
  final Color? iconColor;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: Opacity(
        opacity: onPressed == null ? 0.38 : 1,
        child: Material(
          color: color ?? (filled ? const Color(0x0DFFFFFF) : Colors.transparent),
          shape: const CircleBorder(),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: onPressed,
            child: SizedBox(
              width: size,
              height: size,
              child: Icon(icon, size: size * 0.48, color: iconColor ?? TrairColors.textSoft, semanticLabel: tooltip),
            ),
          ),
        ),
      ),
    );
  }
}

/// Pill shaped text field.
class PillField extends StatelessWidget {
  const PillField({
    super.key,
    required this.controller,
    required this.hint,
    this.leading,
    this.trailing,
    this.obscure = false,
    this.enabled = true,
    this.onChanged,
    this.onSubmitted,
    this.height = 48,
    this.mono = false,
    this.semanticLabel,
    this.focusNode,
    this.autofocus = false,
  });

  final TextEditingController controller;
  final FocusNode? focusNode;
  final bool autofocus;
  final String hint;
  final Widget? leading;
  final Widget? trailing;
  final bool obscure;
  final bool enabled;
  final ValueChanged<String>? onChanged;
  final ValueChanged<String>? onSubmitted;
  final double height;
  final bool mono;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: height,
      padding: EdgeInsets.only(left: leading == null ? 20 : 14, right: trailing == null ? 20 : 5),
      decoration: BoxDecoration(
        color: TrairColors.field,
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: TrairColors.border),
      ),
      child: Row(
        children: [
          if (leading != null) ...[leading!, const SizedBox(width: 10)],
          Expanded(
            child: Semantics(
              label: semanticLabel,
              textField: true,
              child: TextField(
                controller: controller,
                focusNode: focusNode,
                autofocus: autofocus,
                obscureText: obscure,
                enabled: enabled,
                onChanged: onChanged,
                onSubmitted: onSubmitted,
                style: mono
                    ? TrairText.mono.copyWith(letterSpacing: 1.2, fontSize: 15)
                    : const TextStyle(fontSize: 15, color: TrairColors.text),
                decoration: InputDecoration.collapsed(
                  hintText: hint,
                  hintStyle: const TextStyle(color: TrairColors.placeholder, fontSize: 15),
                ),
              ),
            ),
          ),
          if (trailing != null) ...[const SizedBox(width: 6), trailing!],
        ],
      ),
    );
  }
}

/// The security capsule: dot, short status, optional chevron.
class SecurityCapsule extends StatelessWidget {
  const SecurityCapsule({super.key, required this.text, required this.tone, this.onTap, this.open = false});

  final String text;
  final CapsuleTone tone;
  final VoidCallback? onTap;
  final bool open;

  @override
  Widget build(BuildContext context) {
    final (dot, fg, border) = switch (tone) {
      CapsuleTone.safe => (TrairColors.mint, TrairColors.mintText, const Color(0x385EE6B0)),
      CapsuleTone.warn => (TrairColors.amber, const Color(0xFFFFD08A), const Color(0x66FFB547)),
      CapsuleTone.locked => (TrairColors.accentLight, TrairColors.accentPale, const Color(0x33A18DFF)),
    };
    return Material(
      color: TrairColors.field,
      shape: StadiumBorder(side: BorderSide(color: border)),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 14, 8),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(color: dot, shape: BoxShape.circle),
              ),
              const SizedBox(width: 8),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 260),
                child: Text(
                  text,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: fg, fontSize: 12.5, fontWeight: FontWeight.w500),
                ),
              ),
              if (onTap != null) ...[
                const SizedBox(width: 6),
                AnimatedRotation(
                  turns: open ? 0.5 : 0,
                  duration: const Duration(milliseconds: 220),
                  child: Icon(TrairIcons.chevronDown, size: 16, color: fg),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

enum CapsuleTone { safe, warn, locked }

/// Pill switch.
class PillSwitch extends StatelessWidget {
  const PillSwitch({super.key, required this.value, required this.onChanged, required this.label});

  final bool value;
  final ValueChanged<bool> onChanged;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: label,
      toggled: value,
      button: true,
      child: GestureDetector(
        onTap: () => onChanged(!value),
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 220),
          width: 50,
          height: 30,
          padding: const EdgeInsets.all(3),
          decoration: BoxDecoration(
            color: value ? TrairColors.accent : const Color(0x1FFFFFFF),
            borderRadius: BorderRadius.circular(999),
          ),
          child: AnimatedAlign(
            duration: const Duration(milliseconds: 260),
            curve: Curves.easeOutCubic,
            alignment: value ? Alignment.centerRight : Alignment.centerLeft,
            child: Container(
              width: 24,
              height: 24,
              decoration: const BoxDecoration(
                color: Colors.white,
                shape: BoxShape.circle,
                boxShadow: [BoxShadow(color: Color(0x59000000), blurRadius: 6, offset: Offset(0, 2))],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Logo, product name and an optional action on the right.
class SidebarHeader extends StatelessWidget {
  const SidebarHeader({super.key, this.trailing});

  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        const TrairLogo(size: 32),
        const SizedBox(width: 10),
        const Text('Trair', style: TextStyle(fontSize: 17, fontWeight: FontWeight.w600, letterSpacing: -0.3)),
        const Spacer(),
        if (trailing != null) trailing!,
      ],
    );
  }
}

/// Small pill that goes back to the chats.
class BackToChats extends StatelessWidget {
  const BackToChats({super.key});

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    return Material(
      color: const Color(0x0DFFFFFF),
      shape: const StadiumBorder(),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: () => state.go(TrairScreen.chats),
        child: const Padding(
          padding: EdgeInsets.fromLTRB(8, 7, 12, 7),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(TrairIcons.chevronLeft, size: 18, color: TrairColors.textSoft),
              Text('Chats', style: TextStyle(fontSize: 13, color: TrairColors.textSoft)),
            ],
          ),
        ),
      ),
    );
  }
}

/// Profile card at the bottom of every sidebar.
class ProfileCard extends StatelessWidget {
  const ProfileCard({super.key, this.subtitle});

  final String? subtitle;

  @override
  Widget build(BuildContext context) {
    final state = AppScope.of(context);
    final initial = state.userName.isEmpty ? '?' : state.userName.characters.first.toUpperCase();
    return GlassPanel(
      lite: true,
      radius: 22,
      padding: const EdgeInsets.fromLTRB(10, 8, 8, 8),
      child: Row(
        children: [
          Expanded(
            child: InkWell(
              borderRadius: BorderRadius.circular(16),
              onTap: () => state.go(TrairScreen.profile),
              child: Padding(
                padding: const EdgeInsets.all(2),
                child: Row(
                  children: [
                    Container(
                      width: 38,
                      height: 38,
                      alignment: Alignment.center,
                      decoration: BoxDecoration(color: state.glow, shape: BoxShape.circle),
                      child: Text(
                        initial,
                        style: const TextStyle(color: Color(0xFF0B0B12), fontWeight: FontWeight.w700),
                      ),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            state.userName,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              color: state.glow,
                              fontWeight: FontWeight.w600,
                              fontSize: 14,
                              shadows: glowShadows(state.glow, strength: state.glowStrength * 0.6),
                            ),
                          ),
                          const SizedBox(height: 2),
                          Row(
                            children: [
                              Container(
                                width: 6,
                                height: 6,
                                decoration: BoxDecoration(
                                  color: state.strict ? TrairColors.mint : TrairColors.amber,
                                  shape: BoxShape.circle,
                                ),
                              ),
                              const SizedBox(width: 6),
                              Flexible(
                                child: Text(
                                  subtitle ?? (state.strict ? 'Strenger Modus an' : 'Strenger Modus aus'),
                                  overflow: TextOverflow.ellipsis,
                                  style: TrairText.small,
                                ),
                              ),
                            ],
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
          RoundIconButton(icon: TrairIcons.lock, tooltip: 'Trair sperren', onPressed: state.lock),
        ],
      ),
    );
  }
}

/// Strip at the top of the window while demo data is used. It sits
/// outside the screens, so no screen can hide it.
class DemoBar extends StatelessWidget {
  const DemoBar({super.key});

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: 'Demo-Chats, nicht verschlüsselt. Konto und Sperre sind echt.',
      container: true,
      child: const Material(
        color: Color(0xFF15120C),
        child: DecoratedBox(
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: Color(0x40FFB547))),
          ),
          child: SizedBox(
            height: 30,
            child: Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Icon(TrairIcons.flaskConical, size: 14, color: TrairColors.amber),
                SizedBox(width: 8),
                Text(
                  'Demo-Chats – nicht verschlüsselt',
                  style: TextStyle(fontSize: 12, color: TrairColors.amber, fontWeight: FontWeight.w600),
                ),
                Flexible(
                  child: Text(
                    ' · Echte Chats stehen unter „Echt verschlüsselt“, Communities sind noch ausgedacht',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontSize: 12, color: TrairColors.amberText),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Small label above a field.
class FieldLabel extends StatelessWidget {
  const FieldLabel(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(left: 18, bottom: 8),
      child: Text(text, style: TrairText.label),
    );
  }
}

/// Glowing name preview.
class GlowName extends StatelessWidget {
  const GlowName({super.key, required this.name, required this.color, this.strength = 35, this.size = 40});

  final String name;
  final Color color;
  final double strength;
  final double size;

  @override
  Widget build(BuildContext context) {
    return Text(
      name,
      textAlign: TextAlign.center,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(
        fontSize: size,
        fontWeight: FontWeight.w600,
        letterSpacing: -0.8,
        color: color,
        shadows: glowShadows(color, strength: strength),
      ),
    );
  }
}

/// Row of round color swatches.
class GlowSwatches extends StatelessWidget {
  const GlowSwatches({super.key, required this.selected, required this.onPick});

  final Color selected;
  final ValueChanged<Color> onPick;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(6),
      decoration: BoxDecoration(
        color: TrairColors.field,
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: TrairColors.borderSoft),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final e in TrairColors.glowChoices.entries)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Semantics(
                label: 'Leuchtfarbe ${e.key}',
                selected: e.value == selected,
                button: true,
                child: GestureDetector(
                  onTap: () => onPick(e.value),
                  child: AnimatedContainer(
                    duration: const Duration(milliseconds: 200),
                    width: 38,
                    height: 38,
                    decoration: BoxDecoration(
                      color: e.value,
                      shape: BoxShape.circle,
                      border: Border.all(
                        color: e.value == selected ? Colors.white : const Color(0x1FFFFFFF),
                        width: e.value == selected ? 2.5 : 1,
                      ),
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// Card inside a glass panel.
class SoftCard extends StatelessWidget {
  const SoftCard({super.key, required this.child, this.padding = const EdgeInsets.all(22), this.borderColor});

  final Widget child;
  final EdgeInsetsGeometry padding;
  final Color? borderColor;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: padding,
      decoration: BoxDecoration(
        color: TrairColors.card,
        borderRadius: BorderRadius.circular(24),
        border: Border.all(color: borderColor ?? TrairColors.borderSoft),
      ),
      child: child,
    );
  }
}

/// The layout is made for at least [minWidth] x [minHeight]. A smaller
/// window shows the same layout scaled down instead of cutting it off.
class MinSizeScaler extends StatelessWidget {
  const MinSizeScaler({super.key, required this.child, this.minWidth = 1100, this.minHeight = 660});

  final Widget child;
  final double minWidth;
  final double minHeight;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, c) {
        final w = c.maxWidth;
        final h = c.maxHeight;
        if (!w.isFinite || !h.isFinite || w <= 0 || h <= 0) return const SizedBox.shrink();
        var scale = 1.0;
        if (w / minWidth < scale) scale = w / minWidth;
        if (h / minHeight < scale) scale = h / minHeight;
        // Always the same widget structure, even at scale 1, so crossing
        // the minimum size does not rebuild the app and lose its state.
        return FittedBox(
          fit: BoxFit.fill,
          child: SizedBox(width: w / scale, height: h / scale, child: child),
        );
      },
    );
  }
}
