//! What the game has attached to the hero, read at a pause (openspec AE.22):
//! each attached placement, the model it draws and where it hangs, by bone or
//! node name, and the hero's own model.
//!
//! Found in AE.22's first held boot (Stealth Elf placed over AF.8's fresh
//! state): her two knives are game actors of their own (`026_Knife_reg1`
//! twice, on the actor list, not skinned) whose placements point back at her.
//! The layout, Giants 01.00, offsets from each object's start, names the
//! reflection registry's (AC.4):
//! - Every placement is an `AbstractPlacement`: `_activityState` +0x54,
//!   `_attachNode` +0x6C, `_attachNodeOwner` +0x70, `_attachedObjects`
//!   +0x74, `_attachXform` +0x78 (a `tfbEulerTransform`: position, then tilt,
//!   heading and roll). The hero's `_attachedObjects` is an
//!   `AbstractPlacementList` (an `igDataList`) of the placements attached to
//!   him; each of those has the hero's placement as `_attachNodeOwner`.
//! - An attach node on a bone is a `BoneAttachInfo` (0x3C bytes): `_boneName`
//!   +0x1C (Stealth Elf's `J_attach_R` and `J_attach_L`), `_boneIndex` +0x20,
//!   `_transform` +0x24. Anything else there is named by its class and
//!   `_name` (an `igNamedObject`'s +0x08), never guessed at.
//! - A `tfbActorInfo`'s `_model` (+0xDC) is the model it draws: a
//!   `tfbPhysicsModel` whose `_name` is the source path
//!   (`C:/tfb/Content/Models/Characters/StealthElfKnife.mdl`). The hero's own
//!   is the model the game draws for him (Punch Pop Fizz's question, AE.5).
//!   `_gameActor` (+0xF4) is its corpus, and a corpus's `igNode::_flags` sits
//!   at +0x1C.
//!
//! An attached placement's own attachments are read the same way, three deep.
//! As a cross-check every actor on the list whose `_attachNodeOwner` is the
//! hero but who isn't in his list is reported as unlisted. A class the reader
//! doesn't know is named from the EBOOT's RTTI (the typeinfo a word before
//! the vtable, its mangled name a word into that), else by its vtable.

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::actors::{ActorFrame, Gather, be32, c_string, expect_vtable, floats, object_ptr, one};

/// The classes this reader follows, by vtable.
pub mod vtable {
    pub const ACTOR_INFO: u32 = 0x0062_1048;
    pub const CORPUS: u32 = 0x0062_1618;
    pub const PLACEMENT_LIST: u32 = 0x0062_2240;
    pub const BONE_ATTACH_INFO: u32 = 0x0067_E708;
    pub const PHYSICS_MODEL: u32 = 0x0063_2AA0;
}

const NAME: usize = 0x08;
const ACTIVITY_STATE: usize = 0x54;
const ATTACH_NODE: usize = 0x6C;
const ATTACH_NODE_OWNER: usize = 0x70;
const ATTACHED_OBJECTS: usize = 0x74;
const ATTACH_XFORM: usize = 0x78;
/// `AbstractPlacement`'s size: what every attached object is read as first.
const PLACEMENT_LEN: usize = 0xA0;
const ACTOR_INFO_LEN: usize = 0xF8;
const ACTOR_SCALE: usize = 0xB8;
const ACTOR_MODEL: usize = 0xDC;
const ACTOR_GAME_ACTOR: usize = 0xF4;
const BONE_ATTACH_LEN: usize = 0x3C;
const BONE_NAME: usize = 0x1C;
const BONE_INDEX: usize = 0x20;
const BONE_TRANSFORM: usize = 0x24;
/// `igNamedObject`: enough for a vtable and a name.
const NAMED_LEN: usize = 0x0C;
const NODE_FLAGS: usize = 0x1C;
/// How deep attachments of attachments are followed.
pub const MAX_DEPTH: usize = 3;
/// A list longer than this is taken for garbage (Stealth Elf's holds 2;
/// AE.5's census found at most 15 attaching sprites in one archive).
pub const MAX_ATTACHED: u32 = 64;
const STRING_LEN: usize = 128;

/// Where an attachment hangs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// No attach node.
    None,
    /// A `BoneAttachInfo`: the bone by name and index in the owner's
    /// skeleton, and the offset from it (position, then tilt, heading, roll).
    Bone {
        address: u32,
        name: Option<String>,
        index: i32,
        transform: [f32; 6],
    },
    /// Another class: its name from the RTTI and its `_name`.
    Other {
        address: u32,
        class: String,
        name: Option<String>,
    },
}

/// A model an actor info draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRef {
    pub address: u32,
    pub class: String,
    /// The source path the disc names it by.
    pub name: Option<String>,
}

impl ModelRef {
    /// The name's last path component, `StealthElfKnife.mdl`.
    pub fn file(&self) -> Option<&str> {
        let name = self.name.as_deref()?;
        name.rsplit(['/', '\\']).next()
    }
}

/// One placement attached to the hero (or to another attachment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub placement: u32,
    pub class: String,
    pub name: Option<String>,
    /// `_activityState`, raw.
    pub activity_state: u32,
    pub node: Node,
    /// `_attachNodeOwner` is the placement it's listed under.
    pub owned_by_parent: bool,
    /// `_attachXform`: position, then tilt, heading, roll.
    pub attach_xform: [f32; 6],
    /// An actor info's `_model`.
    pub model: Option<ModelRef>,
    /// An actor info's `_gameActor`, the corpus.
    pub corpus: Option<u32>,
    /// That corpus is on the game's actor list this frame.
    pub listed: bool,
    /// The corpus's `igNode::_flags`.
    pub corpus_flags: Option<u32>,
    /// An actor info's `_scaleVec`, percent.
    pub scale: Option<[f32; 3]>,
    /// What's attached to it in turn.
    pub attached: Vec<Attachment>,
}

impl Attachment {
    /// `StealthElfKnife.mdl@J_attach_R`, or the placement's name where it
    /// draws no model, `?` where there's no bone.
    pub fn label(&self) -> String {
        let what = self
            .model
            .as_ref()
            .and_then(|m| m.file().map(str::to_owned))
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| format!("{:#010x}", self.placement));
        let at = match &self.node {
            Node::None => "-".to_owned(),
            Node::Bone { name, index, .. } => {
                name.clone().unwrap_or_else(|| format!("bone {index}"))
            }
            Node::Other { class, name, .. } => {
                format!("{class} {}", name.as_deref().unwrap_or("?"))
            }
        };
        format!("{what}@{at}")
    }

    /// Itself and everything under it, depth first.
    pub fn flatten(&self) -> Vec<&Attachment> {
        let mut out = vec![self];
        for a in &self.attached {
            out.extend(a.flatten());
        }
        out
    }
}

/// What a pause shows of the hero (the skinned actor a pad drives): the model
/// the game draws for him and the placements hanging off him.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroAttachments {
    pub placement: u32,
    pub name: Option<String>,
    /// The hero's own `_model`: what the game draws for him.
    pub model: Option<ModelRef>,
    /// His `_attachedObjects`, in list order.
    pub attached: Vec<Attachment>,
    /// Actors whose `_attachNodeOwner` is the hero but who aren't in his
    /// list: `name@placement`.
    pub unlisted: Vec<String>,
}

impl HeroAttachments {
    /// Every attachment, nested ones included.
    pub fn all(&self) -> Vec<&Attachment> {
        self.attached.iter().flat_map(Attachment::flatten).collect()
    }

    /// The attachments that draw a model, nested ones included.
    pub fn models(&self) -> Vec<&Attachment> {
        self.all()
            .into_iter()
            .filter(|a| a.model.is_some())
            .collect()
    }
}

/// The hero of `frame` (the skinned actor a pad drives), his model and what
/// the game has attached to him.
pub fn read(g: &mut impl Gather, frame: &ActorFrame) -> Result<HeroAttachments> {
    let hero = frame.hero()?.clone();
    let p = one(g, hero.placement, ACTOR_INFO_LEN, "the hero's placement")?;
    expect_vtable(&p, vtable::ACTOR_INFO, hero.placement, "tfbActorInfo")?;
    let model = model(g, be32(&p, ACTOR_MODEL))?;
    let listed: BTreeSet<u32> = frame.actors.iter().map(|a| a.corpus).collect();
    let mut seen = BTreeSet::from([hero.placement]);
    let attached = attached(g, hero.placement, &p, &listed, &mut seen, 0)?;

    // The cross-check: anyone else on the list who names the hero as owner.
    let others: Vec<_> = frame
        .actors
        .iter()
        .filter(|a| a.placement != hero.placement && object_ptr(a.placement))
        .collect();
    let ranges: Vec<(u32, usize)> = others
        .iter()
        .map(|a| (a.placement, ATTACH_NODE_OWNER + 4))
        .collect();
    let mut unlisted = Vec::new();
    for (a, bytes) in others.iter().zip(g.gather(&ranges)?) {
        let Some(b) = bytes else { continue };
        if be32(&b, ATTACH_NODE_OWNER) == hero.placement && !seen.contains(&a.placement) {
            unlisted.push(format!(
                "{}@{:#010x}",
                a.name.as_deref().unwrap_or("?"),
                a.placement
            ));
        }
    }
    Ok(HeroAttachments {
        placement: hero.placement,
        name: hero.name,
        model,
        attached,
        unlisted,
    })
}

/// The placements in `owner`'s `_attachedObjects` (`p` its bytes), each read.
fn attached(
    g: &mut impl Gather,
    owner: u32,
    p: &[u8],
    listed: &BTreeSet<u32>,
    seen: &mut BTreeSet<u32>,
    depth: usize,
) -> Result<Vec<Attachment>> {
    let at = be32(p, ATTACHED_OBJECTS);
    if at == 0 {
        return Ok(Vec::new());
    }
    let items = crate::actors::list(g, at, vtable::PLACEMENT_LIST, "AbstractPlacementList")?;
    if items.len() as u32 > MAX_ATTACHED {
        bail!(
            "{owner:#010x}'s attached objects count {} (over {MAX_ATTACHED})",
            items.len()
        );
    }
    let mut out = Vec::new();
    for item in items {
        if !seen.insert(item) {
            bail!("{item:#010x} is attached twice under {owner:#010x}: a cycle");
        }
        out.push(attachment(g, owner, item, listed, seen, depth)?);
    }
    Ok(out)
}

fn attachment(
    g: &mut impl Gather,
    owner: u32,
    at: u32,
    listed: &BTreeSet<u32>,
    seen: &mut BTreeSet<u32>,
    depth: usize,
) -> Result<Attachment> {
    let head = one(g, at, PLACEMENT_LEN, "an attached placement")?;
    let vt = be32(&head, 0);
    let actor = vt == vtable::ACTOR_INFO;
    let p = if actor {
        one(g, at, ACTOR_INFO_LEN, "an attached actor info")?
    } else {
        head
    };
    let (model, corpus, scale) = if actor {
        let corpus = Some(be32(&p, ACTOR_GAME_ACTOR)).filter(|&c| c != 0);
        (
            model(g, be32(&p, ACTOR_MODEL))?,
            corpus,
            Some(floats::<3>(&p, ACTOR_SCALE)),
        )
    } else {
        (None, None, None)
    };
    let corpus_flags = match corpus {
        Some(c) if object_ptr(c) => {
            let b = one(g, c, NODE_FLAGS + 4, "an attached corpus")?;
            (be32(&b, 0) == vtable::CORPUS).then(|| be32(&b, NODE_FLAGS))
        }
        _ => None,
    };
    let nested = if depth + 1 < MAX_DEPTH {
        attached(g, at, &p, listed, seen, depth + 1)?
    } else {
        Vec::new()
    };
    Ok(Attachment {
        placement: at,
        class: class_name(g, vt),
        name: string_at(g, be32(&p, NAME)),
        activity_state: be32(&p, ACTIVITY_STATE),
        node: node(g, be32(&p, ATTACH_NODE))?,
        owned_by_parent: be32(&p, ATTACH_NODE_OWNER) == owner,
        attach_xform: floats::<6>(&p, ATTACH_XFORM),
        model,
        corpus,
        listed: corpus.is_some_and(|c| listed.contains(&c)),
        corpus_flags,
        scale,
        attached: nested,
    })
}

fn node(g: &mut impl Gather, at: u32) -> Result<Node> {
    if at == 0 {
        return Ok(Node::None);
    }
    let head = one(g, at, NAMED_LEN, "an attach node")?;
    let vt = be32(&head, 0);
    if vt == vtable::BONE_ATTACH_INFO {
        let b = one(g, at, BONE_ATTACH_LEN, "a BoneAttachInfo")?;
        return Ok(Node::Bone {
            address: at,
            name: string_at(g, be32(&b, BONE_NAME)),
            index: be32(&b, BONE_INDEX) as i32,
            transform: floats::<6>(&b, BONE_TRANSFORM),
        });
    }
    Ok(Node::Other {
        address: at,
        class: class_name(g, vt),
        name: string_at(g, be32(&head, NAME)),
    })
}

fn model(g: &mut impl Gather, at: u32) -> Result<Option<ModelRef>> {
    if at == 0 {
        return Ok(None);
    }
    let head = one(g, at, NAMED_LEN, "a model")?;
    let vt = be32(&head, 0);
    Ok(Some(ModelRef {
        address: at,
        class: class_name(g, vt),
        name: string_at(g, be32(&head, NAME)),
    }))
}

/// The C string at `at`, `None` for a null or unmapped pointer.
fn string_at(g: &mut impl Gather, at: u32) -> Option<String> {
    if at == 0 {
        return None;
    }
    // A short string at the end of a mapped page: try less before giving up.
    for len in [STRING_LEN, 32, 8] {
        if let Ok(mut got) = g.gather(&[(at, len)])
            && let Some(Some(bytes)) = got.pop()
        {
            return Some(c_string(&bytes));
        }
    }
    None
}

/// A class's name: the ones this reader follows by name, any other from the
/// RTTI the EBOOT keeps beside its vtable (GCC's: the typeinfo pointer a word
/// before the vtable, the mangled name's pointer a word into the typeinfo),
/// else the vtable in hex.
pub fn class_name(g: &mut impl Gather, vt: u32) -> String {
    let known = match vt {
        vtable::ACTOR_INFO => Some("tfbActor::tfbActorInfo"),
        vtable::CORPUS => Some("tfbActor::tfbCorpus"),
        vtable::PLACEMENT_LIST => Some("tfbScript::AbstractPlacementList"),
        vtable::BONE_ATTACH_INFO => Some("tfbModel::BoneAttachInfo"),
        vtable::PHYSICS_MODEL => Some("tfbPhysicsLink::tfbPhysicsModel"),
        _ => None,
    };
    if let Some(k) = known {
        return k.to_owned();
    }
    rtti_name(g, vt).unwrap_or_else(|| format!("vtable {vt:#010x}"))
}

fn rtti_name(g: &mut impl Gather, vt: u32) -> Option<String> {
    let ti = word_at(g, vt.checked_sub(4)?)?;
    let name = word_at(g, ti.checked_add(4)?)?;
    if !object_ptr(name) {
        return None;
    }
    demangle(&string_at(g, name)?)
}

/// The word at `at`, `None` off an object address or unmapped.
fn word_at(g: &mut impl Gather, at: u32) -> Option<u32> {
    if !object_ptr(at) {
        return None;
    }
    let b = g.gather(&[(at, 4)]).ok()?.pop()??;
    Some(be32(&b, 0))
}

/// An Itanium type name: `N8tfbActor12tfbActorInfoE` is
/// `tfbActor::tfbActorInfo`, `12tfbActorInfo` is `tfbActorInfo`; anything
/// else isn't a name this reads.
pub fn demangle(mangled: &str) -> Option<String> {
    let inner = match mangled.strip_prefix('N') {
        Some(rest) => rest.strip_suffix('E')?,
        None => mangled,
    };
    let mut parts = Vec::new();
    let mut rest = inner;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        let n: usize = rest[..digits].parse().ok()?;
        let name = rest.get(digits..digits + n)?;
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        parts.push(name);
        rest = &rest[digits + n..];
    }
    (!parts.is_empty()).then(|| parts.join("::"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::fixture::{Added, STATICS, Spec, World};
    use crate::actors::read as read_frame;

    const KNIFE: &str = "C:/tfb/Content/Models/Characters/StealthElfKnife.mdl";
    /// A vtable in the fixture's statics page, for a class the reader names
    /// from its RTTI.
    const PARTICLE_VT: u32 = STATICS + 0x4_0010;

    fn model_object(w: &mut World, name: &str) -> u32 {
        let m = w.object(vtable::PHYSICS_MODEL, 0x30);
        let s = w.string(name);
        w.put32(m + 0x08, s);
        m
    }

    fn bone(w: &mut World, name: &str, index: i32) -> u32 {
        let b = w.object(vtable::BONE_ATTACH_INFO, BONE_ATTACH_LEN);
        let s = w.string(name);
        w.put32(b + BONE_NAME as u32, s);
        w.put32(b + BONE_INDEX as u32, index as u32);
        w.put_floats(b + BONE_TRANSFORM as u32, &[0.0, 0.1, 0.0, 0.0, 90.0, 0.0]);
        b
    }

    /// Stealth Elf as AE.22's held boot found her: two knives on her hands, on
    /// the actor list, owned by her placement; a VFX of a class the reader
    /// doesn't know on the right knife; and a pet that names her as owner
    /// without being in her list.
    fn stealth_elf() -> (World, Added, [Added; 2], u32) {
        let mut w = World::new();
        let hero = w.actor(&Spec {
            name: "026_StealthElf".into(),
            scale: [120.0; 3],
            skinned: true,
            pad: Some(-1),
            ..Spec::default()
        });
        let own = model_object(&mut w, "C:/tfb/Content/Models/Characters/StealthElf.mdl");
        w.put32(hero.placement + ACTOR_MODEL as u32, own);
        let knives = [("J_attach_R", 20), ("J_attach_L", 14)].map(|(b, i)| {
            let k = w.actor(&Spec {
                name: "026_Knife_reg1".into(),
                scale: [120.0; 3],
                ..Spec::default()
            });
            let m = model_object(&mut w, KNIFE);
            w.put32(k.placement + ACTOR_MODEL as u32, m);
            let node = bone(&mut w, b, i);
            w.put32(k.placement + ATTACH_NODE as u32, node);
            w.put32(k.placement + ATTACH_NODE_OWNER as u32, hero.placement);
            w.put32(k.placement + ACTIVITY_STATE as u32, 1);
            w.put32(k.corpus + NODE_FLAGS as u32, 0x0001_0000);
            k
        });
        let list = w.list(
            vtable::PLACEMENT_LIST,
            &[knives[0].placement, knives[1].placement],
            4,
        );
        w.put32(hero.placement + ATTACHED_OBJECTS as u32, list);

        // RTTI for the VFX's class: the typeinfo a word before its vtable,
        // the mangled name a word into the typeinfo.
        let ti = w.alloc(8);
        let name = w.string("N13tfbParticleFx15tfbParticleInfoE");
        w.put32(PARTICLE_VT - 4, ti);
        w.put32(ti + 4, name);
        let vfx = w.object(PARTICLE_VT, 0xE0);
        let vfx_name = w.string("026_KnifeGlint");
        w.put32(vfx + 0x08, vfx_name);
        // Its node is the knife itself, an actor info.
        w.put32(vfx + ATTACH_NODE as u32, knives[0].placement);
        w.put32(vfx + ATTACH_NODE_OWNER as u32, knives[0].placement);
        let on_knife = w.list(vtable::PLACEMENT_LIST, &[vfx], 4);
        w.put32(knives[0].placement + ATTACHED_OBJECTS as u32, on_knife);

        let pet = w.actor(&Spec {
            name: "Pet".into(),
            ..Spec::default()
        });
        w.put32(pet.placement + ATTACH_NODE_OWNER as u32, hero.placement);
        (w, hero, knives, vfx)
    }

    #[test]
    fn the_heros_attachments_read_with_their_models_and_bones() {
        let (mut w, hero, knives, vfx) = stealth_elf();
        let frame = read_frame(&mut w).unwrap();
        let a = read(&mut w, &frame).unwrap();
        assert_eq!(a.placement, hero.placement);
        assert_eq!(a.name.as_deref(), Some("026_StealthElf"));
        assert_eq!(
            a.model.as_ref().and_then(ModelRef::file),
            Some("StealthElf.mdl")
        );
        assert_eq!(a.attached.len(), 2);
        let labels: Vec<String> = a.attached.iter().map(Attachment::label).collect();
        assert_eq!(
            labels,
            [
                "StealthElfKnife.mdl@J_attach_R",
                "StealthElfKnife.mdl@J_attach_L"
            ]
        );
        let right = &a.attached[0];
        assert_eq!(right.placement, knives[0].placement);
        assert_eq!(right.class, "tfbActor::tfbActorInfo");
        assert_eq!(right.name.as_deref(), Some("026_Knife_reg1"));
        assert_eq!(right.activity_state, 1);
        assert!(right.owned_by_parent);
        assert_eq!(
            right.node,
            Node::Bone {
                address: match right.node {
                    Node::Bone { address, .. } => address,
                    _ => unreachable!(),
                },
                name: Some("J_attach_R".into()),
                index: 20,
                transform: [0.0, 0.1, 0.0, 0.0, 90.0, 0.0],
            }
        );
        assert_eq!(right.corpus, Some(knives[0].corpus));
        assert!(right.listed);
        assert_eq!(right.corpus_flags, Some(0x0001_0000));
        assert_eq!(right.scale, Some([120.0; 3]));
        assert_eq!(
            right.model.as_ref().unwrap().class,
            "tfbPhysicsLink::tfbPhysicsModel"
        );

        // The VFX on the right knife: its class off the RTTI, its node the
        // knife (another class), no model.
        let [glint] = &right.attached[..] else {
            panic!("{:?}", right.attached)
        };
        assert_eq!(glint.placement, vfx);
        assert_eq!(glint.class, "tfbParticleFx::tfbParticleInfo");
        assert_eq!(glint.name.as_deref(), Some("026_KnifeGlint"));
        assert!(glint.owned_by_parent);
        assert!(glint.model.is_none() && glint.corpus.is_none() && !glint.listed);
        assert_eq!(
            glint.node,
            Node::Other {
                address: knives[0].placement,
                class: "tfbActor::tfbActorInfo".into(),
                name: Some("026_Knife_reg1".into()),
            }
        );
        assert_eq!(
            glint.label(),
            "026_KnifeGlint@tfbActor::tfbActorInfo 026_Knife_reg1"
        );
        assert_eq!(a.all().len(), 3);
        assert_eq!(a.models().len(), 2);
        // The pet names her as owner but isn't hers: reported, not counted.
        assert_eq!(a.unlisted.len(), 1);
        assert!(a.unlisted[0].starts_with("Pet@"), "{:?}", a.unlisted);
    }

    #[test]
    fn a_hero_with_nothing_attached_reads_empty() {
        let mut w = World::new();
        w.actor(&Spec {
            name: "009_Eruptor".into(),
            skinned: true,
            pad: Some(-1),
            ..Spec::default()
        });
        let frame = read_frame(&mut w).unwrap();
        let a = read(&mut w, &frame).unwrap();
        assert!(a.attached.is_empty() && a.unlisted.is_empty() && a.model.is_none());
    }

    #[test]
    fn a_list_of_another_class_a_cycle_and_a_bad_pointer_are_refused() {
        // Each broken link refused for what it is, not for something else
        // the reader happened to trip on.
        let refused = |w: &mut World| {
            let frame = read_frame(w).unwrap();
            format!("{:#}", read(w, &frame).unwrap_err())
        };

        // The list's vtable isn't AbstractPlacementList's.
        let (mut w, hero, _, _) = stealth_elf();
        let list = w.get32(hero.placement + ATTACHED_OBJECTS as u32);
        w.put32(list, 0x0062_0000);
        let e = refused(&mut w);
        assert!(e.contains("not AbstractPlacementList's"), "{e}");

        // A knife listing the hero as its own attachment.
        let (mut w, hero, knives, _) = stealth_elf();
        let back = w.list(vtable::PLACEMENT_LIST, &[hero.placement], 4);
        w.put32(knives[1].placement + ATTACHED_OBJECTS as u32, back);
        let e = refused(&mut w);
        assert!(e.contains("cycle"), "{e}");

        // An attached object pointer that isn't an object address.
        let (mut w, hero, _, _) = stealth_elf();
        let list = w.get32(hero.placement + ATTACHED_OBJECTS as u32);
        let data = w.get32(list + 0x14);
        w.put32(data, 0x0000_0003);
        let e = refused(&mut w);
        assert!(
            e.contains("an attached placement") && e.contains("object address"),
            "{e}"
        );

        // A knife's attach node, and its model, pointing at unmapped memory:
        // refused, never read as no node or no model.
        const UNMAPPED: u32 = 0x2000_0000;
        let (mut w, _, knives, _) = stealth_elf();
        w.put32(knives[1].placement + ATTACH_NODE as u32, UNMAPPED);
        let e = refused(&mut w);
        assert!(
            e.contains("an attach node") && e.contains("isn't mapped"),
            "{e}"
        );
        let (mut w, _, knives, _) = stealth_elf();
        w.put32(knives[0].placement + ACTOR_MODEL as u32, UNMAPPED);
        let e = refused(&mut w);
        assert!(e.contains("a model") && e.contains("isn't mapped"), "{e}");

        // A list counting past its capacity.
        let (mut w, hero, _, _) = stealth_elf();
        let list = w.get32(hero.placement + ATTACHED_OBJECTS as u32);
        w.put32(list + 0x08, 5);
        let e = refused(&mut w);
        assert!(e.contains("counts 5 with capacity 4"), "{e}");
    }

    #[test]
    fn itanium_type_names_demangle_and_anything_else_doesnt() {
        assert_eq!(
            demangle("N8tfbActor12tfbActorInfoE").as_deref(),
            Some("tfbActor::tfbActorInfo")
        );
        assert_eq!(
            demangle("N9tfbScript21AbstractPlacementListE").as_deref(),
            Some("tfbScript::AbstractPlacementList")
        );
        assert_eq!(demangle("12tfbActorInfo").as_deref(), Some("tfbActorInfo"));
        assert_eq!(demangle("N8tfbActorE12x").as_deref(), None);
        assert_eq!(demangle("N9tfbActorE"), None, "a length past the end");
        assert_eq!(demangle(""), None);
        assert_eq!(demangle("hello"), None);
        // A vtable with no RTTI behind it is named by its value.
        let mut w = World::new();
        assert_eq!(class_name(&mut w, 0x0062_9990), "vtable 0x00629990");
    }
}
