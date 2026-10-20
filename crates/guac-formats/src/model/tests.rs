use super::builder::{
    ActorSpec, CharacterSpec, SlotSpec, archive, character, eruptor, inverse_joint, sample, seeds,
};
use super::*;
use crate::havok::builder::clip;
use crate::iga::Archive;
use crate::igz::layouts::Shape;

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
    // Two joints naming one bone (cargo fuzz, AE.0's hour) are refused too.
    let mut doubled = sk.clone();
    doubled.bones[2].name = doubled.bones[1].name;
    let e = JointMap::new(&doubled, &p.hero.bones).unwrap_err();
    assert_eq!((e.field, e.kind), ("hkaBone name", ErrorKind::Duplicate));
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

/// Each locomotion slot of `spec`'s archive, read.
fn slots(spec: &CharacterSpec) -> Vec<(usize, Result<Option<SlotClip<'static>>, FormatError>)> {
    let igz = parsed(spec);
    let p = portal_model(igz).unwrap();
    let sk = skeleton(igz, &p.hero).unwrap();
    [WALK_SLOT, JOG_SLOT, RUN_SLOT, FALL_SLOT, LANDING_SLOT, 200]
        .into_iter()
        .map(|slot| (slot, slot_clip(igz, &p.hero, &sk, slot)))
        .collect()
}

#[test]
fn the_locomotion_clips_are_the_sets_fixed_slots() {
    let read = slots(&sample());
    /// File, label and duration.
    type Seen<'a> = Option<(&'a str, Option<&'a str>, f32)>;
    let got: Vec<(usize, Seen)> = read
        .iter()
        .map(|(slot, c)| {
            let c = c.as_ref().unwrap().as_ref();
            (
                *slot,
                c.map(|c| (c.file.unwrap(), c.label, c.clip.animation.duration)),
            )
        })
        .collect();
    // Each clip the builder put in its slot, by its own duration (8 frames
    // at its frame time); the jog's slot empty and slot 200 past the set.
    assert_eq!(
        got,
        [
            (
                1,
                Some(("Eruptor_Walk.ma", Some("3.658 m/sec"), 8.0 / 15.0))
            ),
            (2, None),
            (3, Some(("Eruptor_Run.ma", Some("6.401 m/sec"), 8.0 / 30.0))),
            (57, Some(("Eruptor_Falling.ma", None, 8.0 / 60.0))),
            (61, Some(("Eruptor_Landing.ma", None, 8.0 / 20.0))),
            (200, None),
        ]
    );
    let walk = read[0].1.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(walk.slot, WALK_SLOT);
    assert_eq!(walk.clip.animation.tracks.len(), 3);
}

#[test]
fn a_locomotion_slot_that_isnt_a_full_normal_clip_is_refused() {
    let refused = |bend: &dyn Fn(&mut SlotSpec)| {
        let mut s = sample();
        let run = s.slots.iter_mut().find(|x| x.slot == RUN_SLOT).unwrap();
        bend(run);
        let (_, r) = slots(&s).swap_remove(2);
        r.unwrap_err()
    };
    // An entry wrapping no clip: an animation network reads so.
    let e = refused(&|r| r.clip = None);
    assert_eq!((e.field, e.kind), ("_tfbAnimation", ErrorKind::Missing));
    for flag in [1 << 4, 1 << 5] {
        assert_eq!(refused(&|r| r.properties |= flag).field, "_properties");
    }
    let e = refused(&|r| r.clip.as_mut().unwrap().blend_hint = 1);
    assert_eq!(e.field, "blendHint");
    let e = refused(&|r| r.clip.as_mut().unwrap().track_to_bone = vec![0, 1, 7]);
    assert_eq!(e.field, "transformTrackToBoneIndices");
}

/// The sample with a palette of every kind past its locomotion: a partial
/// clip on one joint (slot 20, Eruptor's throw), an additive one (slot
/// 12, a lean) and an entry with no clip (slot 7, a network).
fn palette_sample() -> CharacterSpec {
    let mut s = sample();
    let mut partial = clip();
    partial.tracks.truncate(1);
    partial.track_to_bone = vec![2];
    s.slots.push(SlotSpec {
        clip: Some(partial),
        properties: 0x620,
        label: Some("throw"),
        ..SlotSpec::new(20, 1.0 / 30.0, "Eruptor_LavaLob.ma")
    });
    let mut additive = clip();
    additive.blend_hint = 1;
    s.slots.push(SlotSpec {
        clip: Some(additive),
        properties: 0x610,
        ..SlotSpec::new(12, 1.0 / 30.0, "Eruptor_LeanLeft.ma")
    });
    s.slots.push(SlotSpec {
        clip: None,
        ..SlotSpec::new(7, 1.0 / 30.0, "Eruptor_Network.ma")
    });
    s
}

/// Every slot of `spec`'s set as the palette reads it, past its end too.
fn palette(spec: &CharacterSpec) -> Vec<Result<Option<PaletteSlot<'static>>, FormatError>> {
    let igz = parsed(spec);
    let p = portal_model(igz).unwrap();
    let sk = skeleton(igz, &p.hero).unwrap();
    (0..=p.hero.clip_slots.len())
        .map(|slot| palette_slot(igz, &p.hero, &sk, slot))
        .collect()
}

#[test]
fn the_palette_reads_every_slot_whatever_it_holds() {
    let read = palette(&palette_sample());
    // Slots 0-61 and one past the end.
    assert_eq!(read.len(), 63);
    /// Slot, file, (additive, partial), the clip's tracks and their joints.
    type Seen = (usize, Option<&'static str>, (bool, bool), Vec<u16>);
    let mut clips: Vec<Seen> = Vec::new();
    let mut networks = Vec::new();
    for r in &read {
        let Some(s) = r.as_ref().unwrap() else {
            continue;
        };
        match &s.clip {
            Some(c) => clips.push((
                s.slot,
                c.file,
                (c.flags.additive_blend(), c.flags.partial_animation()),
                c.clip.binding.track_to_bone.clone(),
            )),
            None => networks.push((s.slot, s.name)),
        }
    }
    assert_eq!(
        clips,
        [
            (0, Some("Eruptor_Idle.ma"), (false, false), vec![0, 1, 2]),
            (1, Some("Eruptor_Walk.ma"), (false, false), vec![0, 1, 2]),
            (3, Some("Eruptor_Run.ma"), (false, false), vec![0, 1, 2]),
            (
                12,
                Some("Eruptor_LeanLeft.ma"),
                (true, false),
                vec![0, 1, 2]
            ),
            (20, Some("Eruptor_LavaLob.ma"), (false, true), vec![2]),
            (
                57,
                Some("Eruptor_Falling.ma"),
                (false, false),
                vec![0, 1, 2]
            ),
            (
                61,
                Some("Eruptor_Landing.ma"),
                (false, false),
                vec![0, 1, 2]
            ),
        ]
    );
    assert_eq!(networks, [(7, Some("?"))]);
    let throw = read[20].as_ref().unwrap().as_ref().unwrap();
    assert_eq!((throw.label, throw.slot), (Some("throw"), 20));
    // The locomotion reader still refuses the throw and the lean, and the
    // palette keeps its clips exactly as the locomotion reads them.
    let igz = parsed(&palette_sample());
    let p = portal_model(igz).unwrap();
    let sk = skeleton(igz, &p.hero).unwrap();
    for slot in [12, 20] {
        assert_eq!(
            slot_clip(igz, &p.hero, &sk, slot).unwrap_err().field,
            "_properties"
        );
    }
    let walk = slot_clip(igz, &p.hero, &sk, WALK_SLOT).unwrap().unwrap();
    let read_walk = read[1].as_ref().unwrap().as_ref().unwrap();
    assert_eq!(read_walk.clip.as_ref().unwrap().clip, walk.clip);
}

#[test]
fn a_palette_slot_that_cant_play_is_refused_by_name() {
    let refused = |slot: usize, bend: &dyn Fn(&mut SlotSpec)| {
        let mut s = palette_sample();
        bend(s.slots.iter_mut().find(|x| x.slot == slot).unwrap());
        palette(&s).swap_remove(slot).unwrap_err()
    };
    // The wrapper says additive and the binding normal, or the other way.
    let e = refused(12, &|x| x.clip.as_mut().unwrap().blend_hint = 0);
    assert_eq!(
        (e.field, e.kind),
        ("blendHint", ErrorKind::Unsupported { value: 0 })
    );
    let e = refused(20, &|x| x.clip.as_mut().unwrap().blend_hint = 1);
    assert_eq!(
        (e.field, e.kind),
        ("blendHint", ErrorKind::Unsupported { value: 1 })
    );
    // A track bound past the skeleton, partial, additive or whole.
    for slot in [20, 12, RUN_SLOT] {
        let e = refused(slot, &|x| {
            let c = x.clip.as_mut().unwrap();
            *c.track_to_bone.last_mut().unwrap() = 7;
        });
        assert_eq!(e.field, "transformTrackToBoneIndices", "slot {slot}");
    }
}

/// The hero `spec`'s archive plays, read as `009_Eruptor`'s.
fn hero(spec: &CharacterSpec) -> Result<Actor<'static>, FormatError> {
    let igz = parsed(spec);
    hero_actor(igz, &portal_model(igz).unwrap(), "009_Eruptor")
}

#[test]
fn the_hero_is_the_solid_actor_drawing_the_portal_model() {
    let a = hero(&sample()).unwrap();
    // Not the clone beside him, whose solid neither blocks nor meets the
    // level, nor the blob, which draws nothing.
    assert_eq!(a.name(), Some("009_Eruptor"));
    let c = a.collision.unwrap();
    assert_eq!((c.properties, c.shape()), (0x1B, Shape::Sphere));
    assert_eq!(
        (c.width, c.height, c.length, c.weight, c.rebound),
        (4.0, 6.0, 3.0, 10.0, 25.0)
    );
    assert_eq!(c.transform.position, [0.0, 0.5, 0.25]);
    assert_eq!(c.transform.rotation, [0.0, 90.0, 0.0]);
    let m = a.parameters.unwrap();
    assert_eq!(
        (m.parameters.ground_max_speed, m.parameters.ground_turn_rate),
        (18.0, 360.0)
    );
    assert!(m.parameters.tilts_with_ground);
}

#[test]
fn without_one_drawing_the_portal_model_the_hero_goes_by_the_load_file() {
    // Punch Pop Fizz's archive: her actor draws another model.
    let mut s = sample();
    for a in &mut s.actors {
        a.draws_portal = false;
    }
    assert_eq!(hero(&s).unwrap().name(), Some("009_Eruptor"));
    // By name, any case; but a name that isn't solid doesn't count.
    s.actors[1].name = "009_ERUPTOR";
    assert_eq!(hero(&s).unwrap().name(), Some("009_ERUPTOR"));
    s.actors[1].collision = 0x13;
    let e = hero(&s).unwrap_err();
    assert_eq!(
        e.kind,
        ErrorKind::CountMismatch {
            found: 0,
            expected: 1
        }
    );
}

#[test]
fn no_hero_or_two_are_refused() {
    let mut s = sample();
    s.actors.retain(|a| a.name != "009_Eruptor");
    let e = hero(&s).unwrap_err();
    assert_eq!(e.field, "the hero's tfbActorInfo");
    assert_eq!(
        e.kind,
        ErrorKind::CountMismatch {
            found: 0,
            expected: 1
        }
    );
    // Two solid actors drawing the portal model: refused, not guessed.
    let mut s = sample();
    s.actors.push(ActorSpec {
        name: "009_EruptorTwin",
        ..eruptor()
    });
    assert_eq!(
        hero(&s).unwrap_err().kind,
        ErrorKind::CountMismatch {
            found: 2,
            expected: 1
        }
    );
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
