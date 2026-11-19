// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 The Trair contributors

//! Badges: only real ones are shown. Every test plays someone who wants a
//! badge they should not have.

use trair_core::badges::{
    Badge, BadgeKind, Community, MAX_BADGES, MAX_LIFETIME, MAX_TEAM_LIFETIME, TEAM_KEYS,
    claims_authority, name_needs_warning, verify, verify_all,
};
use trair_core::{Account, VerifiedBadge};

const NOW: u64 = 1_800_000_000;
const DAY: u64 = 24 * 3600;

fn account(name: &str) -> Account {
    Account::generate(name).expect("account")
}

fn hex_key(a: &Account) -> String {
    hex::encode(a.public_key())
}

struct Setup {
    founder: Account,
    admin: Account,
    moderator: Account,
    community: Community,
}

fn setup() -> Setup {
    let founder = account("gruenderin");
    let community = Community::create("Trainingsgruppe", &founder).unwrap();
    Setup {
        founder,
        admin: account("admin_anna"),
        moderator: account("mod_tom"),
        community,
    }
}

fn issue(
    issuer: &Account,
    kind: BadgeKind,
    holder: &Account,
    community: Option<&Community>,
    via: Option<Badge>,
) -> Badge {
    Badge::issue(
        issuer,
        kind,
        holder.public_key(),
        community.cloned(),
        NOW,
        7 * DAY,
        via,
    )
    .expect("issue")
}

#[test]
fn a01_founder_badge_belongs_to_the_founder_only() {
    let s = setup();
    let badge = issue(
        &s.founder,
        BadgeKind::Founder,
        &s.founder,
        Some(&s.community),
        None,
    );
    let v = verify(&badge, s.founder.public_key(), NOW + 60, TEAM_KEYS).expect("valid");
    assert_eq!(v.kind, BadgeKind::Founder);
    assert_eq!(v.community.as_deref(), Some("Trainingsgruppe"));
    assert_eq!(v.community_id.as_deref(), Some(s.community.id.as_str()));
    // Shown on another profile: refused.
    assert!(verify(&badge, s.admin.public_key(), NOW + 60, TEAM_KEYS).is_err());
    // Someone else claims to have founded it.
    let fake = issue(
        &s.admin,
        BadgeKind::Founder,
        &s.admin,
        Some(&s.community),
        None,
    );
    assert!(verify(&fake, s.admin.public_key(), NOW, TEAM_KEYS).is_err());
    // There is one founder: they cannot hand the founder badge to a friend.
    let gift = issue(
        &s.founder,
        BadgeKind::Founder,
        &s.admin,
        Some(&s.community),
        None,
    );
    assert!(verify(&gift, s.admin.public_key(), NOW, TEAM_KEYS).is_err());
}

#[test]
fn a02_admin_badge_needs_the_founder() {
    let s = setup();
    let real = issue(
        &s.founder,
        BadgeKind::Admin,
        &s.admin,
        Some(&s.community),
        None,
    );
    assert!(verify(&real, s.admin.public_key(), NOW, TEAM_KEYS).is_ok());
    // Signed by the admin themselves, or by a moderator.
    let own = issue(
        &s.admin,
        BadgeKind::Admin,
        &s.admin,
        Some(&s.community),
        None,
    );
    assert!(verify(&own, s.admin.public_key(), NOW, TEAM_KEYS).is_err());
    let by_mod = issue(
        &s.moderator,
        BadgeKind::Admin,
        &s.admin,
        Some(&s.community),
        None,
    );
    assert!(verify(&by_mod, s.admin.public_key(), NOW, TEAM_KEYS).is_err());
}

#[test]
fn a03_a_copied_community_with_another_founder_is_refused() {
    let s = setup();
    // The attacker keeps id, name and nonce but puts in their own key.
    let mut copy = s.community.clone();
    copy.founder = hex_key(&s.admin);
    let fake = issue(&s.admin, BadgeKind::Founder, &s.admin, Some(&copy), None);
    assert!(verify(&fake, s.admin.public_key(), NOW, TEAM_KEYS).is_err());
    let fake_admin = issue(&s.admin, BadgeKind::Admin, &s.moderator, Some(&copy), None);
    assert!(verify(&fake_admin, s.moderator.public_key(), NOW, TEAM_KEYS).is_err());
}

#[test]
fn a04_moderators_by_founder_or_by_an_admin_with_proof() {
    let s = setup();
    let by_founder = issue(
        &s.founder,
        BadgeKind::Mod,
        &s.moderator,
        Some(&s.community),
        None,
    );
    assert!(verify(&by_founder, s.moderator.public_key(), NOW, TEAM_KEYS).is_ok());

    let admin_badge = issue(
        &s.founder,
        BadgeKind::Admin,
        &s.admin,
        Some(&s.community),
        None,
    );
    let by_admin = issue(
        &s.admin,
        BadgeKind::Mod,
        &s.moderator,
        Some(&s.community),
        Some(admin_badge.clone()),
    );
    assert!(verify(&by_admin, s.moderator.public_key(), NOW, TEAM_KEYS).is_ok());

    // An admin without proof, or a founder who attaches a needless proof.
    let no_proof = issue(
        &s.admin,
        BadgeKind::Mod,
        &s.moderator,
        Some(&s.community),
        None,
    );
    assert!(verify(&no_proof, s.moderator.public_key(), NOW, TEAM_KEYS).is_err());
    let founder_with_proof = issue(
        &s.founder,
        BadgeKind::Mod,
        &s.moderator,
        Some(&s.community),
        Some(admin_badge.clone()),
    );
    assert!(
        verify(
            &founder_with_proof,
            s.moderator.public_key(),
            NOW,
            TEAM_KEYS
        )
        .is_err()
    );
}

#[test]
fn a05_moderator_proofs_cannot_be_stretched() {
    let s = setup();
    let other_founder = account("anderer");
    let other = Community::create("Andere Gruppe", &other_founder).unwrap();
    let mod_badge = issue(
        &s.founder,
        BadgeKind::Mod,
        &s.moderator,
        Some(&s.community),
        None,
    );
    let newcomer = account("neuling");

    // A moderator appoints moderators: refused.
    let chain = issue(
        &s.moderator,
        BadgeKind::Mod,
        &newcomer,
        Some(&s.community),
        Some(mod_badge),
    );
    assert!(verify(&chain, newcomer.public_key(), NOW, TEAM_KEYS).is_err());
    // Admin of another community.
    let foreign_admin = issue(
        &other_founder,
        BadgeKind::Admin,
        &s.admin,
        Some(&other),
        None,
    );
    let wrong_place = issue(
        &s.admin,
        BadgeKind::Mod,
        &newcomer,
        Some(&s.community),
        Some(foreign_admin),
    );
    assert!(verify(&wrong_place, newcomer.public_key(), NOW, TEAM_KEYS).is_err());
    // Someone attaches a real admin badge that belongs to someone else.
    let anna_admin = issue(
        &s.founder,
        BadgeKind::Admin,
        &s.admin,
        Some(&s.community),
        None,
    );
    let borrowed = issue(
        &newcomer,
        BadgeKind::Mod,
        &newcomer,
        Some(&s.community),
        Some(anna_admin),
    );
    assert!(verify(&borrowed, newcomer.public_key(), NOW, TEAM_KEYS).is_err());
    // The admin's own badge has run out.
    let old_admin = Badge::issue(
        &s.founder,
        BadgeKind::Admin,
        s.admin.public_key(),
        Some(s.community.clone()),
        NOW - 10 * DAY,
        2 * DAY,
        None,
    )
    .unwrap();
    let late = issue(
        &s.admin,
        BadgeKind::Mod,
        &newcomer,
        Some(&s.community),
        Some(old_admin),
    );
    assert!(verify(&late, newcomer.public_key(), NOW, TEAM_KEYS).is_err());
}

#[test]
fn a06_team_badge_only_with_a_pinned_team_key() {
    let team = account("trair-team");
    let julian = account("julian");
    let badge = issue(&team, BadgeKind::Team, &julian, None, None);
    let team_key = hex_key(&team);
    let pinned = [team_key.as_str()];
    assert_eq!(
        verify(&badge, julian.public_key(), NOW, &pinned)
            .unwrap()
            .kind,
        BadgeKind::Team
    );
    // The app ships without any team key yet: nobody has the badge.
    assert!(TEAM_KEYS.is_empty());
    assert!(verify(&badge, julian.public_key(), NOW, TEAM_KEYS).is_err());
    // Self-made team badge, and a team badge with a community attached.
    let own = issue(&julian, BadgeKind::Team, &julian, None, None);
    assert!(verify(&own, julian.public_key(), NOW, &pinned).is_err());
    let founder = account("gruender");
    let c = Community::create("Trair Fans", &founder).unwrap();
    let odd = issue(&team, BadgeKind::Team, &julian, Some(&c), None);
    assert!(verify(&odd, julian.public_key(), NOW, &pinned).is_err());
}

#[test]
fn a07_every_change_breaks_the_signature() {
    let s = setup();
    let badge = issue(
        &s.founder,
        BadgeKind::Mod,
        &s.moderator,
        Some(&s.community),
        None,
    );
    let key = s.moderator.public_key();
    assert!(verify(&badge, key, NOW, TEAM_KEYS).is_ok());

    let mut kind = badge.clone();
    kind.claim.kind = BadgeKind::Admin;
    let mut name = badge.clone();
    name.claim.community.as_mut().unwrap().name = "Trair Offiziell".into();
    let mut longer = badge.clone();
    longer.claim.expires += DAY;
    let mut holder = badge.clone();
    holder.claim.holder = hex_key(&s.admin);
    let mut signature = badge.clone();
    let other = issue(
        &s.founder,
        BadgeKind::Mod,
        &s.admin,
        Some(&s.community),
        None,
    );
    signature.signature = other.signature;
    for (what, b) in [
        ("kind", kind),
        ("name", name),
        ("expires", longer),
        ("signature", signature),
    ] {
        assert!(verify(&b, key, NOW, TEAM_KEYS).is_err(), "{what}");
    }
    assert!(
        verify(&holder, s.admin.public_key(), NOW, TEAM_KEYS).is_err(),
        "holder"
    );
}

#[test]
fn a08_badges_run_out_and_cannot_come_from_the_future() {
    let s = setup();
    let badge = issue(
        &s.founder,
        BadgeKind::Admin,
        &s.admin,
        Some(&s.community),
        None,
    );
    let key = s.admin.public_key();
    assert!(verify(&badge, key, NOW + 7 * DAY - 1, TEAM_KEYS).is_ok());
    assert!(
        verify(&badge, key, NOW + 7 * DAY, TEAM_KEYS).is_err(),
        "expired"
    );
    assert!(
        verify(&badge, key, NOW - 4 * 60, TEAM_KEYS).is_ok(),
        "small clock difference"
    );
    assert!(
        verify(&badge, key, NOW - 3600, TEAM_KEYS).is_err(),
        "issued in the future"
    );
    assert!(
        Badge::issue(
            &s.founder,
            BadgeKind::Admin,
            key,
            Some(s.community.clone()),
            NOW,
            MAX_LIFETIME + 1,
            None
        )
        .is_err()
    );
    assert!(
        Badge::issue(
            &s.founder,
            BadgeKind::Admin,
            key,
            Some(s.community.clone()),
            NOW,
            0,
            None
        )
        .is_err()
    );
}

#[test]
fn a09_profile_shows_each_badge_once_in_order() {
    let s = setup();
    let team = account("trair-team");
    let team_key = hex_key(&team);
    let pinned = [team_key.as_str()];
    let other_founder = account("anderer");
    let other = Community::create("Andere Gruppe", &other_founder).unwrap();
    let f = &s.founder;
    let badges = vec![
        issue(f, BadgeKind::Mod, f, Some(&s.community), None),
        issue(f, BadgeKind::Admin, f, Some(&s.community), None),
        issue(&other_founder, BadgeKind::Mod, f, Some(&other), None),
        issue(f, BadgeKind::Founder, f, Some(&s.community), None),
        issue(&team, BadgeKind::Team, f, None, None),
        issue(&team, BadgeKind::Team, f, None, None),
        issue(f, BadgeKind::Team, f, None, None),
    ];
    let (shown, refused) = verify_all(&badges, f.public_key(), NOW, &pinned);
    let kinds: Vec<BadgeKind> = shown.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        vec![BadgeKind::Team, BadgeKind::Founder, BadgeKind::Mod]
    );
    assert_eq!(shown[2].community.as_deref(), Some("Andere Gruppe"));
    assert_eq!(refused, 1, "the self-made team badge");

    let many = vec![badges[1].clone(); MAX_BADGES + 3];
    let (shown, refused) = verify_all(&many, f.public_key(), NOW, &pinned);
    assert_eq!(shown.len(), 1);
    assert_eq!(refused, 3, "more than {MAX_BADGES} are refused");
}

#[test]
fn a10_names_that_pretend_get_a_warning() {
    let s = setup();
    let admin = verify(
        &issue(
            &s.founder,
            BadgeKind::Admin,
            &s.admin,
            Some(&s.community),
            None,
        ),
        s.admin.public_key(),
        NOW,
        TEAM_KEYS,
    )
    .unwrap();
    let team = VerifiedBadge {
        kind: BadgeKind::Team,
        community: None,
        community_id: None,
        community_warning: false,
        expires: NOW + DAY,
    };
    for name in [
        "Trair Team",
        "TR41R Support",
        "tr\u{430}ir",
        "T.r.a.i.r",
        "Admin Lena",
        "[MOD] Tom",
        "Staff",
        "Gründer",
    ] {
        assert!(name_needs_warning(name, &[]), "{name}");
    }
    for name in ["Lena", "Modellbau Max", "Tom Moderne", "Straße"] {
        assert!(!name_needs_warning(name, &[]), "{name}");
    }
    assert!(!name_needs_warning(
        "Admin Lena",
        std::slice::from_ref(&admin)
    ));
    assert!(
        name_needs_warning("Trair Support", std::slice::from_ref(&admin)),
        "admin is not the Trair team"
    );
    assert!(!name_needs_warning("Julian · Trair", &[team]));

    // Community names that look official are marked.
    assert!(claims_authority("Trair Offiziell"));
    assert!(!claims_authority("Trainingsgruppe"));
    let c = Community::create("Trair Support", &s.founder).unwrap();
    let v = verify(
        &issue(&s.founder, BadgeKind::Founder, &s.founder, Some(&c), None),
        s.founder.public_key(),
        NOW,
        TEAM_KEYS,
    )
    .unwrap();
    assert!(v.community_warning);
}

#[test]
fn a11_strange_input_is_refused_not_crashed() {
    let s = setup();
    let admin_badge = issue(
        &s.founder,
        BadgeKind::Admin,
        &s.admin,
        Some(&s.community),
        None,
    );
    let mut json = serde_json::to_value(&admin_badge).unwrap();
    json["extra"] = serde_json::json!(1);
    assert!(
        serde_json::from_value::<Badge>(json).is_err(),
        "unknown fields"
    );
    // Proofs inside proofs inside proofs.
    let mut deep = admin_badge.clone();
    for _ in 0..3 {
        deep = Badge {
            via: Some(Box::new(deep.clone())),
            ..admin_badge.clone()
        };
    }
    assert!(verify(&deep, s.admin.public_key(), NOW, TEAM_KEYS).is_err());
    let mut nested = String::new();
    for _ in 0..1000 {
        nested.push_str(r#"{"claim":{},"signature":"","via":"#);
    }
    assert!(
        serde_json::from_str::<Badge>(&nested).is_err(),
        "deep nesting"
    );
    for bad in ["", "\u{7}x", " vorne", &"x".repeat(41)] {
        assert!(Community::create(bad, &s.founder).is_err(), "{bad:?}");
    }
    // Round trip keeps a badge valid.
    let text = serde_json::to_string(&admin_badge).unwrap();
    let back: Badge = serde_json::from_str(&text).unwrap();
    assert!(verify(&back, s.admin.public_key(), NOW, TEAM_KEYS).is_ok());
}

#[test]
fn a12_team_badges_last_a_year_roles_only_two_weeks() {
    let team = account("trair-team");
    let julian = account("julian");
    let team_key = hex_key(&team);
    let pinned = [team_key.as_str()];
    let year = Badge::issue(
        &team,
        BadgeKind::Team,
        julian.public_key(),
        None,
        NOW,
        365 * DAY,
        None,
    )
    .unwrap();
    assert!(verify(&year, julian.public_key(), NOW + 364 * DAY, &pinned).is_ok());
    assert!(
        verify(&year, julian.public_key(), NOW + 365 * DAY, &pinned).is_err(),
        "runs out"
    );
    assert!(
        Badge::issue(
            &team,
            BadgeKind::Team,
            julian.public_key(),
            None,
            NOW,
            MAX_TEAM_LIFETIME + 1,
            None
        )
        .is_err()
    );
    // A founder cannot hand out a year-long admin badge.
    let s = setup();
    let long = Badge::issue(
        &s.founder,
        BadgeKind::Admin,
        s.admin.public_key(),
        Some(s.community.clone()),
        NOW,
        15 * DAY,
        None,
    );
    assert!(long.is_err());
}
