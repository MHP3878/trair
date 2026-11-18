// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

import 'package:material_ui/material_ui.dart';

import '../app_state.dart';
import '../backend/backend.dart';
import '../theme.dart';
import 'badges.dart';
import 'common.dart';
import 'trair_icons.dart';

/// Who can see the own profile (setting in Privatsphäre).
const profileVisibilityChoices = {
  'alle': 'Alle',
  'gemeinsam': 'Gemeinsame Chats',
  'kontakte': 'Nur Kontakte',
};

const _trustNames = ['Neu', 'Dabei', 'Vertraut', 'Verlässlich'];

/// What a profile card shows.
class ProfileView {
  const ProfileView({
    required this.name,
    required this.color,
    this.glowStrength = 35,
    this.about = '',
    this.trust = 0,
    this.badges = const [],
    this.nameWarning = false,
    this.isSelf = false,
    this.demoBadges = false,
    this.visibility,
  });

  /// Someone else, or the user if [Person.isSelf].
  factory ProfileView.of(Person p, AppState state) {
    if (p.isSelf) return ProfileView.self(state);
    return ProfileView(
      name: p.name,
      color: p.color,
      about: p.about,
      trust: p.trust,
      badges: p.badges,
      nameWarning: p.nameWarning,
      demoBadges: state.backend.isDemo,
    );
  }

  /// The user's own profile, as others will see it.
  factory ProfileView.self(AppState state) => ProfileView(
    name: state.userName.trim().isEmpty ? 'Dein Name' : state.userName.trim(),
    color: state.glow,
    glowStrength: state.glowStrength,
    about: state.about,
    trust: 2,
    badges: state.backend.ownBadges(),
    isSelf: true,
    demoBadges: state.backend.isDemo,
    visibility: profileVisibilityChoices[state.profileVisibility],
  );

  final String name;
  final Color color;
  final double glowStrength;
  final String about;
  final int trust;
  final List<ProfileBadge> badges;
  final bool nameWarning;
  final bool isSelf;
  final bool demoBadges;

  /// Only for the own profile: who can see it.
  final String? visibility;
}

/// Open the profile card of [view] above everything else.
Future<void> showProfileCard(BuildContext context, ProfileView view) {
  return showDialog<void>(
    context: context,
    barrierColor: const Color(0x9905050A),
    barrierLabel: 'Profil schließen',
    builder: (_) => Center(child: ProfilePopup(view: view)),
  );
}

/// The profile card, like a Discord profile: banner, glowing name, badges.
class ProfilePopup extends StatelessWidget {
  const ProfilePopup({super.key, required this.view});

  final ProfileView view;

  @override
  Widget build(BuildContext context) {
    final v = view;
    final initial = v.name.isEmpty ? '?' : v.name.characters.first.toUpperCase();
    return Material(
      type: MaterialType.transparency,
      child: Container(
        width: 360,
        decoration: BoxDecoration(
          color: const Color(0xF5141420),
          borderRadius: BorderRadius.circular(26),
          border: Border.all(color: TrairColors.border),
          boxShadow: const [
            BoxShadow(color: Color(0x66000000), blurRadius: 48, spreadRadius: -8, offset: Offset(0, 12)),
          ],
        ),
        child: ClipRRect(
          borderRadius: BorderRadius.circular(26),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SizedBox(
                height: 150,
                child: Stack(
                  children: [
                    Container(
                      height: 104,
                      decoration: BoxDecoration(
                        gradient: LinearGradient(
                          begin: Alignment.topLeft,
                          end: Alignment.bottomRight,
                          colors: [v.color.withAlpha(0xB3), v.color.withAlpha(0x33)],
                        ),
                      ),
                    ),
                    Positioned(
                      right: 10,
                      top: 10,
                      child: RoundIconButton(
                        icon: TrairIcons.x,
                        tooltip: 'Schließen',
                        size: 32,
                        color: const Color(0x59000000),
                        iconColor: Colors.white,
                        onPressed: () => Navigator.of(context).pop(),
                      ),
                    ),
                    Positioned(
                      left: 18,
                      top: 60,
                      child: Container(
                        width: 86,
                        height: 86,
                        padding: const EdgeInsets.all(6),
                        decoration: const BoxDecoration(color: Color(0xFF141420), shape: BoxShape.circle),
                        child: Container(
                          alignment: Alignment.center,
                          decoration: BoxDecoration(
                            color: v.color,
                            shape: BoxShape.circle,
                            boxShadow: [BoxShadow(color: v.color.withAlpha(0x66), blurRadius: 18)],
                          ),
                          child: Text(
                            initial,
                            style: const TextStyle(fontSize: 30, fontWeight: FontWeight.w700, color: Color(0xFF0B0B12)),
                          ),
                        ),
                      ),
                    ),
                    if (v.badges.isNotEmpty)
                      Positioned(
                        right: 14,
                        top: 114,
                        child: Container(
                          padding: const EdgeInsets.all(6),
                          decoration: BoxDecoration(
                            color: const Color(0x14FFFFFF),
                            borderRadius: BorderRadius.circular(12),
                            border: Border.all(color: TrairColors.borderSoft),
                          ),
                          child: BadgeRow(badges: v.badges, size: 26, spacing: 7),
                        ),
                      ),
                  ],
                ),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 4, 20, 20),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Row(
                      children: [
                        Flexible(
                          child: Text(
                            v.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              fontSize: 22,
                              fontWeight: FontWeight.w700,
                              letterSpacing: -0.4,
                              color: v.color,
                              shadows: glowShadows(v.color, strength: v.glowStrength),
                            ),
                          ),
                        ),
                        if (v.isSelf) ...[
                          const SizedBox(width: 8),
                          const Text('Du', style: TrairText.small),
                        ],
                      ],
                    ),
                    if (v.nameWarning) ...[const SizedBox(height: 10), const _NameWarning()],
                    const SizedBox(height: 14),
                    const Divider(height: 1, color: TrairColors.borderSoft),
                    const SizedBox(height: 14),
                    const Text('Über mich', style: TrairText.label),
                    const SizedBox(height: 4),
                    Text(
                      v.about.trim().isEmpty ? 'Noch nichts geschrieben.' : v.about.trim(),
                      style: v.about.trim().isEmpty ? TrairText.muted : TrairText.body.copyWith(fontSize: 14),
                    ),
                    const SizedBox(height: 14),
                    const Text('Trust', style: TrairText.label),
                    const SizedBox(height: 6),
                    Row(
                      children: [
                        for (var i = 1; i <= 3; i++)
                          Container(
                            width: 22,
                            height: 6,
                            margin: const EdgeInsets.only(right: 4),
                            decoration: BoxDecoration(
                              color: i <= v.trust ? TrairColors.mint : const Color(0x1FFFFFFF),
                              borderRadius: BorderRadius.circular(999),
                            ),
                          ),
                        const SizedBox(width: 6),
                        Text('Stufe ${v.trust} · ${_trustNames[v.trust.clamp(0, 3)]}', style: TrairText.small),
                      ],
                    ),
                    if (v.badges.isNotEmpty && v.demoBadges) ...[
                      const SizedBox(height: 12),
                      const Text(
                        'Abzeichen sind hier noch Demo. Echte kommen unterschrieben mit dem Server.',
                        style: TrairText.small,
                      ),
                    ],
                    const SizedBox(height: 14),
                    Row(
                      children: [
                        const Icon(TrairIcons.lock, size: 13, color: TrairColors.textMuted),
                        const SizedBox(width: 6),
                        Expanded(
                          child: Text(
                            v.visibility == null ? 'Verschlüsselt geteilt' : 'Sichtbar für: ${v.visibility}',
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
    );
  }
}

class _NameWarning extends StatelessWidget {
  const _NameWarning();

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.fromLTRB(10, 8, 10, 8),
      decoration: BoxDecoration(
        color: const Color(0x1FFFB547),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: const Color(0x4DFFB547)),
      ),
      child: const Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(TrairIcons.triangleAlert, size: 15, color: TrairColors.amber),
          SizedBox(width: 8),
          Expanded(
            child: Text(
              'Vorsicht: Der Name wirkt offiziell, aber diese Person gehört nicht zum Trair-Team. '
              'Gib niemandem dein Passwort.',
              style: TextStyle(fontSize: 12.5, height: 1.45, color: TrairColors.amberText),
            ),
          ),
        ],
      ),
    );
  }
}
