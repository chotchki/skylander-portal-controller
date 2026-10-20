use super::builder::{
    ARMOR, BOOMERANG, DUMMY, GUN, HAT, KNIFE, KNIFE_UPGRADE, On, ScriptSpec, Source, Step, level,
    sample,
};
use super::*;
use crate::igz::Igz;
use crate::level::Placements;

/// The level's object index of each template, in spec order.
fn templates(igz: &Igz<'_>, spec: &ScriptSpec) -> Vec<usize> {
    let placements = Placements::read(igz).unwrap();
    spec.templates
        .iter()
        .map(|t| placements.actor(t.name).unwrap().index)
        .collect()
}

fn step(spec: &ScriptSpec) -> (Vec<usize>, Fresh, Result<Vec<Hung>, Unsettled>) {
    let bytes = level(spec);
    let igz = Igz::parse(&bytes).unwrap();
    let placements = Placements::read(&igz).unwrap();
    let hero = placements.actor("026_Synthetic").unwrap();
    let fresh = fresh(&igz, hero).unwrap();
    let hung = hung(&fresh, |t| match t {
        Template::Local(p) => placement_model(&igz, *p).unwrap().is_some(),
        Template::Global(_) => false,
    });
    (templates(&igz, spec), fresh, hung)
}

/// Each hung model as (template index in the spec, parent, slot).
fn shown(t: &[usize], hung: &[Hung]) -> Vec<(usize, Option<usize>, Option<usize>)> {
    hung.iter()
        .map(|h| {
            let Template::Local(p) = h.template else {
                panic!("a global template");
            };
            (t.iter().position(|&o| o == p).unwrap(), h.parent, h.slot)
        })
        .collect()
}

#[test]
fn a_fresh_figure_takes_its_attachments_and_leaves_an_upgrades_off() {
    let (t, fresh, hung) = step(&sample());
    // Two knives (the Def script's `knife(1)`, not the upgrade's), the gun
    // with the spear on its root, the two boomerangs; no armour (upgrade
    // 3), no upgraded knife (upgrade 2), no hat (the switch's other arm),
    // the dummy drawing nothing.
    assert_eq!(
        shown(&t, &hung.unwrap()),
        [
            (KNIFE, None, Some(1)),
            (KNIFE, None, Some(0)),
            (GUN, None, Some(2)),
            (super::builder::SPEAR, Some(2), Some(0)),
            (BOOMERANG, None, Some(0)),
            (BOOMERANG, None, Some(1)),
        ]
    );
    for gone in [KNIFE_UPGRADE, ARMOR, HAT] {
        assert!(
            !fresh
                .clones
                .iter()
                .any(|c| c.template == Some(Template::Local(t[gone]))),
            "template {gone} cloned"
        );
    }
    // The dummy hangs on the hero itself; the record is no clone.
    let dummy: Vec<&Clone> = fresh
        .clones
        .iter()
        .filter(|c| c.template == Some(Template::Local(t[DUMMY])))
        .collect();
    assert_eq!(dummy.len(), 1);
    assert_eq!(
        dummy[0].node,
        Some(Node {
            owner: Owner::Hero,
            slot: None
        })
    );
    assert_eq!(fresh.clones.len(), 7);
    assert!(fresh.clones.iter().all(|c| !c.unsure), "{:?}", fresh.unsure);
}

#[test]
fn a_model_under_a_check_it_cant_read_leaves_the_set_unsettled() {
    let mut spec = sample();
    spec.startup.push(Step::Unreadable(vec![Step::Clone {
        from: Source::Template(HAT),
        on: On::Hero(2),
        keep: None,
    }]));
    let (_, fresh, hung) = step(&spec);
    assert!(fresh.clones.last().unwrap().unsure);
    assert!(!fresh.unsure.is_empty());
    assert!(matches!(hung, Err(Unsettled::Unsure { .. })), "{hung:?}");

    // A dummy there is fine: it draws nothing either way.
    let mut spec = sample();
    spec.startup.push(Step::Unreadable(vec![Step::Clone {
        from: Source::Template(DUMMY),
        on: On::Hero(2),
        keep: None,
    }]));
    let (_, _, hung) = step(&spec);
    assert_eq!(hung.unwrap().len(), 6);
}

#[test]
fn a_clone_of_a_reference_nothing_wrote_is_unsure() {
    let mut spec = sample();
    spec.startup.push(Step::Clone {
        from: Source::Ref("026_never_set"),
        on: On::Hero(0),
        keep: None,
    });
    let (_, fresh, hung) = step(&spec);
    let last = fresh.clones.last().unwrap();
    assert_eq!((last.template, last.unsure), (None, true));
    assert!(matches!(hung, Err(Unsettled::Unsure { .. })));
}

#[test]
fn a_model_hanging_on_one_that_draws_nothing_is_refused() {
    let mut spec = sample();
    spec.startup.push(Step::Clone {
        from: Source::Template(DUMMY),
        on: On::Hero(0),
        keep: Some("dummy"),
    });
    spec.startup.push(Step::Clone {
        from: Source::Template(HAT),
        on: On::Clone("dummy", 0),
        keep: None,
    });
    let (_, _, hung) = step(&spec);
    assert!(matches!(hung, Err(Unsettled::Parent { .. })), "{hung:?}");
}

#[test]
fn flow_end_ends_the_start_and_a_global_template_is_kept() {
    let mut spec = sample();
    spec.startup.insert(
        3,
        Step::Clone {
            from: Source::Global(0x77),
            on: On::Hero(0),
            keep: None,
        },
    );
    spec.startup.insert(4, Step::End);
    let (t, fresh, hung) = step(&spec);
    // The knife on the right hand, then the global template; nothing after
    // the end.
    assert_eq!(fresh.clones.len(), 2);
    let Some(Template::Global(id)) = fresh.clones[1].template else {
        panic!("{:?}", fresh.clones[1]);
    };
    assert_eq!(id.object, 0x77);
    assert_eq!(shown(&t, &hung.unwrap()), [(KNIFE, None, Some(1))]);
}

#[test]
fn a_switch_on_a_variable_reads_what_was_written() {
    let mut spec = sample();
    // Written 1, the switch takes the hat over the boomerang.
    let at = spec
        .startup
        .iter()
        .position(|s| matches!(s, Step::Switch(..)))
        .unwrap();
    spec.startup.insert(at, Step::SetInt("boomerang kind", 1));
    let (t, _, hung) = step(&spec);
    let hung = shown(&t, &hung.unwrap());
    assert_eq!(&hung[4..], [(HAT, None, Some(0)), (HAT, None, Some(1))]);
}

#[test]
fn a_models_attach_points_read_in_list_order() {
    let spec = sample();
    let bytes = level(&spec);
    let igz = Igz::parse(&bytes).unwrap();
    let placements = Placements::read(&igz).unwrap();
    let hero = placements.actor("026_Synthetic").unwrap();
    let points = attach_points(&igz, hero.info.model.unwrap()).unwrap();
    let names: Vec<_> = points.iter().map(|p| (p.bone_name, p.bone_index)).collect();
    assert_eq!(
        names,
        [
            (Some("J_attach_L"), 14),
            (Some("J_attach_R"), 20),
            (Some("J_chest"), 3)
        ]
    );
    assert_eq!(points[2].transform.position, [0.0, 0.1, 0.0]);
    assert_eq!(points[2].transform.rotation, [0.0, 90.0, 0.0]);
    let gun = placements.actor(spec.templates[GUN].name).unwrap();
    let root = attach_points(&igz, gun.info.model.unwrap()).unwrap();
    assert_eq!(root[0].bone_name, Some(BoneAttachInfo::ROOT));
    assert_eq!(root[0].bone_index, -1);
    // A model without a list has no points.
    let knife = placements.actor(spec.templates[KNIFE].name).unwrap();
    assert!(
        attach_points(&igz, knife.info.model.unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_hero_without_a_script_makes_nothing_and_ops_out_of_their_list_are_refused() {
    let spec = sample();
    let bytes = level(&spec);
    let igz = Igz::parse(&bytes).unwrap();
    let placements = Placements::read(&igz).unwrap();
    let knife = placements.actor(spec.templates[KNIFE].name).unwrap();
    assert!(fresh(&igz, knife).unwrap().clones.is_empty());

    // The hero's STARTUP claiming more ops than the list holds.
    let hero = placements.actor("026_Synthetic").unwrap();
    let script = hero.info.placement.script.unwrap();
    let ops = OpCodeList::read(
        &igz,
        ScriptInfo::read(&igz, script).unwrap().op_list.unwrap(),
    )
    .unwrap()
    .ops;
    let startup = igz.objects()[ops[1]].at;
    let mut bad = bytes.clone();
    let field = startup.offset_by(0x1A).unwrap();
    let offset = (igz.sections()[1 + field.section()].offset + field.offset()) as usize;
    bad[offset..offset + 2].copy_from_slice(&0x7FFFu16.to_be_bytes());
    let igz = Igz::parse(&bad).unwrap();
    let placements = Placements::read(&igz).unwrap();
    let hero = placements.actor("026_Synthetic").unwrap();
    let e = fresh(&igz, hero).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::OutOfRange { .. }), "{e}");
}
