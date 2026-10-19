use super::builder::{CharacterSpec, archive, character, inverse_joint, sample, seeds};
use super::*;
use crate::havok::builder::clip;
use crate::iga::Archive;

fn parsed(spec: &CharacterSpec) -> &'static Igz<'static> {
    let bytes: &'static [u8] = Vec::leak(character(spec));
    Box::leak(Box::new(Igz::parse(bytes).unwrap()))
}

#[test]
fn the_portal_sprites_model_is_picked_over_an_unskinned_attachment() {
    let igz = parsed(&sample());
    let p = portal_model(igz).unwrap();
    assert_eq!(p.sprite_name, Some("009_Portal_Toy"));
    assert_eq!(p.name(), Some("Eruptor.mdl"));
    assert_eq!(p.hero.meshes.len(), 2);
    assert_eq!(p.attached.len(), 1);
    let (sprite, model) = p.attached[0];
    let s = SpriteInfo::read(igz, sprite).unwrap();
    assert_eq!(s.placement.name, Some("009_ReposeSpikes"));
    assert!(
        Hero::read(igz, model).is_err(),
        "an attachment without a body"
    );
}

#[test]
fn a_skinned_attachment_loses_to_the_portal_toy_sprite_and_a_tie_is_refused() {
    // Tree Rex's woodpecker: a second model with a skeleton.
    let mut spec = sample();
    spec.attachments = vec![("112_Woodpecker", true)];
    let p = portal_model(parsed(&spec)).unwrap();
    assert_eq!(p.sprite_name, Some("009_Portal_Toy"));
    assert_eq!(p.attached.len(), 1);
    // Neither sprite says Portal_Toy: refused, not guessed.
    spec.portal_sprite = "009_Toy";
    let e = portal_model(parsed(&spec)).unwrap_err();
    assert_eq!(
        e.kind,
        ErrorKind::CountMismatch {
            found: 0,
            expected: 1
        }
    );
    // The name matches in any case.
    spec.portal_sprite = "009_PORTAL_TOY_Base";
    assert!(portal_model(parsed(&spec)).is_ok());
}

#[test]
fn an_archive_without_a_drawn_skeleton_has_no_portal_model() {
    let none = crate::igz::layouts::builder::bare_hero(|_, _, _, _| Vec::new());
    let e = portal_model(&Igz::parse(&none).unwrap()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Missing);
    // Only the attachment's sprite, which draws no skeleton.
    let mut s = sample();
    s.portal_sprite = "009_ReposeHorn";
    s.attachments.clear();
    assert!(
        portal_model(parsed(&s)).is_ok(),
        "a lone skinned model needs no name"
    );
}

#[test]
fn joints_map_by_name_to_the_bones_and_their_inverse_joints() {
    let igz = parsed(&sample());
    let p = portal_model(igz).unwrap();
    let sk = skeleton(igz, &p.hero).unwrap();
    assert_eq!(sk.bones.len(), 3);
    let map = JointMap::new(&sk, &p.hero.bones).unwrap();
    // Alchemy's unnamed root has no joint; the rest follow it.
    assert_eq!(map.joint_of_bone, [None, Some(0), Some(1), Some(2)]);
    assert_eq!(map.bone_of_joint, [1, 2, 3]);
    let inverse = map
        .inverse_joints(&p.hero.bones, &p.hero.skeleton.inverse_joints)
        .unwrap();
    assert_eq!(inverse, (0..3).map(inverse_joint).collect::<Vec<_>>());
    // A joint without a bone of its name, or with two, is refused.
    let mut renamed = sk.clone();
    renamed.bones[1].name = Some("J_tail");
    assert!(JointMap::new(&renamed, &p.hero.bones).is_err());
    let mut twice = p.hero.bones.clone();
    twice.push(twice[1]);
    let e = JointMap::new(&sk, &twice).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Duplicate);
    // A bone whose blend matrix isn't there.
    assert!(map.inverse_joints(&p.hero.bones, &[]).is_err());
}

#[test]
fn the_idle_is_slot_0_and_poses_the_skeleton() {
    let igz = parsed(&sample());
    let p = portal_model(igz).unwrap();
    let sk = skeleton(igz, &p.hero).unwrap();
    let idle = idle(igz, &p.hero, &sk).unwrap();
    assert_eq!(idle.file, Some("Eruptor_Idle.ma"));
    assert_eq!(idle.clip.animation.tracks.len(), 3);
    assert_eq!(idle.clip, {
        let c = clip();
        let bytes = crate::havok::builder::payload(&crate::havok::builder::PayloadSpec {
            clip: Some(c),
            ..Default::default()
        })
        .bytes;
        let parsed = crate::havok::Container::parse(Vec::leak(bytes)).unwrap();
        parsed.clip.unwrap()
    });
}

#[test]
fn an_idle_that_isnt_a_full_normal_clip_is_refused() {
    let refused = |spec: &CharacterSpec| {
        let igz = parsed(spec);
        let p = portal_model(igz).unwrap();
        let sk = skeleton(igz, &p.hero).unwrap();
        idle(igz, &p.hero, &sk).unwrap_err()
    };
    // No slot 0.
    let mut s = sample();
    s.idle = None;
    assert_eq!(refused(&s).kind, ErrorKind::Missing);
    // Additive or partial by the wrapper's flags.
    for flag in [1 << 4, 1 << 5] {
        let mut s = sample();
        s.idle.as_mut().unwrap().1 |= flag;
        assert_eq!(refused(&s).field, "_properties");
    }
    // An additive binding.
    let mut s = sample();
    s.idle.as_mut().unwrap().0.blend_hint = 1;
    assert_eq!(refused(&s).field, "blendHint");
    // A track bound past the skeleton.
    let mut s = sample();
    s.idle.as_mut().unwrap().0.track_to_bone = vec![0, 1, 7];
    assert_eq!(refused(&s).field, "transformTrackToBoneIndices");
}

#[test]
fn the_archive_carries_the_level_and_the_seeds_parse() {
    let level = character(&sample());
    let bytes = archive(&level);
    let a = Archive::parse(&bytes).unwrap();
    let entry = a.find("level.bld").unwrap();
    assert_eq!(a.read(entry).unwrap().as_ref(), level.as_slice());
    for seed in seeds() {
        Igz::parse(&seed).unwrap();
    }
}
