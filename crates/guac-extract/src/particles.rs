//! The emitters the fight's scripts spawn (openspec AH.87): every
//! `tfbParticleInfo` in each spawn archive (`global.bld` and each
//! fighter's, [`scripts::spawn_archives`]) with its generators, through
//! guac-formats' [`particle`] reader, into
//! `particles/<stem>.<ron|postcard>`, each generator's texture into the
//! store. An extract-wide part of its own ([`PART`]), so an extract made
//! before it resumes for it alone.
//!
//! A level's own emitters (openspec AS.17) are a part of that level's
//! ([`level_emitters`], [`crate::levels::PARTICLES`]): every emitter its
//! file places, read as these are, each with its placement, read with the
//! level's other parts (on a first run for chapter 1, on demand for the
//! rest), so an extract made before them gains them on that level's next
//! load.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{BlendMode, SpawnVolume, VelocityDirection};
use guac_formats::particle::{self, Generator};
use guac_ir::DumpIdentity;
use guac_ir::particle::{
    Blend, Direction, Emitter, Emitters, Keyed, Kind, Ribbon, Sprite, Texture, Volume,
};

use crate::intros::IrFormat;
use crate::levels::Read;
use crate::models::{self, Textures};
use crate::stages::{Event, Files, count_kept, put};
use crate::{ExtractError, IdentityDir, Result, scripts};

/// The extract-wide part ([`crate::PARTS`]).
pub const PART: &str = "particles";
/// The folder each archive's file sits in.
pub const DIR: &str = "particles";

/// `particles/<stem>.<ron|postcard>`.
pub fn path(ir: IrFormat, archive: &str) -> String {
    let ext = match ir {
        IrFormat::Ron => "ron",
        IrFormat::Postcard => "postcard",
    };
    format!("{DIR}/{}.{ext}", scripts::stem(archive))
}

/// The archives the part reads: the spawn models'.
pub fn archives() -> Vec<String> {
    scripts::spawn_archives()
}

/// Each emitter `igz` places, its generators and their textures read; one
/// that doesn't read, or whose texture doesn't, is listed with why and the
/// rest land.
pub fn emitters(igz: &Igz<'_>, archive: &str, identity: DumpIdentity) -> Read<Emitters> {
    let mut textures = Textures::default();
    let mut out = Emitters {
        identity,
        archive: archive.to_owned(),
        emitters: Vec::new(),
        refused: Vec::new(),
    };
    for (index, read) in particle::emitters(igz) {
        let mut own = textures.clone();
        let e = read
            .map_err(|e| e.to_string())
            .and_then(|e| emitter(igz, &e, &mut own));
        match e {
            Ok(e) => {
                textures = own;
                out.emitters.push(e);
            }
            Err(why) => out.refused.push((name_of(igz, index), why)),
        }
    }
    Read {
        ir: out,
        textures: textures.files,
    }
}

/// Each emitter a level's `igz` places (openspec AS.17), its generators
/// and their textures read as a spawn archive's are ([`emitters`]), with
/// where it stands. Refused by name with the rest landing: one that doesn't
/// read, whose texture doesn't, or that rides something (a parent, an
/// attach node or its owner, or an object it moves with), which the TV
/// draws nowhere yet; none of the disc's 15,383 does.
pub fn level_emitters(
    igz: &Igz<'_>,
    level: &str,
    identity: DumpIdentity,
) -> Read<guac_ir::particle::LevelEmitters> {
    use guac_formats::igz::layouts::ObjectRef;
    let mut textures = Textures::default();
    let mut out = guac_ir::particle::LevelEmitters {
        identity,
        level: level.to_owned(),
        emitters: Vec::new(),
        refused: Vec::new(),
    };
    for (index, read) in particle::emitters(igz) {
        let mut own = textures.clone();
        let placed = read.map_err(|e| e.to_string()).and_then(|e| {
            let p = &e.info.placement;
            let at = &p.placement;
            let rides = [
                ("a parent", at.parent),
                ("an attach node", at.attach_node),
                ("an attach node's owner", at.attach_node_owner),
                ("an object it moves with", at.move_object),
            ]
            .into_iter()
            .find(|(_, r)| *r != ObjectRef::Null);
            if let Some((what, _)) = rides {
                return Err(format!("it rides {what}, which isn't drawn yet"));
            }
            Ok(guac_ir::particle::Placed {
                emitter: emitter(igz, &e, &mut own)?,
                position: at.init_matrix.position,
                rotation: at.init_matrix.rotation,
                scale: p.scale,
                activity_state: at.activity_state,
                activation_range: p.activation_range,
                activation_range_buffer: p.activation_range_buffer,
            })
        });
        match placed {
            Ok(p) => {
                textures = own;
                out.emitters.push(p);
            }
            Err(why) => out.refused.push((name_of(igz, index), why)),
        }
    }
    Read {
        ir: out,
        textures: textures.files,
    }
}

fn name_of(igz: &Igz<'_>, index: usize) -> String {
    use guac_formats::igz::layouts::{Layout, ParticleInfo};
    ParticleInfo::read(igz, index)
        .ok()
        .and_then(|p| p.placement.placement.name)
        .map_or_else(|| format!("#{index}"), str::to_owned)
}

/// One emitter as the IR keeps it, its textures into `textures`.
pub fn emitter(
    igz: &Igz<'_>,
    e: &particle::Emitter<'_>,
    textures: &mut Textures,
) -> std::result::Result<Emitter, String> {
    let mut generators = Vec::with_capacity(e.generators.len());
    for g in &e.generators {
        // An image of no whole blocks (the level-up's arrow, 109 x 128) is
        // stored decoded ([`Textures::path`], openspec AH.244).
        let texture = match g.texture() {
            Some(t) => Some(Texture {
                texture: textures.path(igz, t.image).map_err(|e| e.to_string())?,
                image: t.path.unwrap_or_default().to_owned(),
                sampler: models::sampler(&t.attr)?,
            }),
            None => None,
        };
        let vfx = match g {
            Generator::Sprite(s) => &s.info.vfx,
            Generator::Ribbon(r) => &r.info.vfx,
        };
        let blend = match vfx.blend_mode() {
            Some(BlendMode::Knockout) => Blend::Knockout,
            Some(BlendMode::Blend) => Blend::Blend,
            Some(BlendMode::Add) => Blend::Add,
            None => {
                return Err(format!(
                    "generator #{}: blend {}",
                    g.index(),
                    vfx.blend_code()
                ));
            }
        };
        let kind = match g {
            Generator::Sprite(s) => Kind::Sprite(sprite(s)),
            Generator::Ribbon(r) => Kind::Ribbon(ribbon(r)),
        };
        generators.push(guac_ir::particle::Generator {
            blend,
            overlay_layer: vfx.overlay_layer() as u8,
            bloom: vfx.bloom_intensity,
            offset: models::euler_transform(&vfx.init_matrix),
            start_delay: vfx.start_delay,
            texture,
            kind,
        });
    }
    Ok(Emitter {
        name: e.name.to_owned(),
        object: e.index as u32,
        generators,
        unread: e.unread.iter().map(|(_, t)| (*t).to_owned()).collect(),
    })
}

fn sprite(s: &particle::Sprite<'_>) -> Sprite {
    let (g, m, a) = (&s.generation, &s.movement, &s.appearance);
    Sprite {
        spawn_rate: g.spawn_rate,
        max_alive: g.max_alive,
        lifespan: g.lifespan,
        lifespan_random: g.lifespan_randomness,
        max_distance: g.max_distance,
        midpoint_percent: g.midpoint_percent,
        next_gen_scale: g.next_gen_scale_factor,
        volume: SpawnVolume::of(m.spawn_volume_code()).map(|v| match v {
            SpawnVolume::Box => Volume::Box,
            SpawnVolume::Sphere => Volume::Sphere,
            SpawnVolume::Cylinder => Volume::Cylinder,
        }),
        volume_size: m.spawn_volume,
        uniform: m.uniform_distribution(),
        inherits_translation: m.inherits_translation(),
        direction: VelocityDirection::of(m.velocity_code()).map(|d| match d {
            VelocityDirection::Orientation => Direction::Orientation,
            VelocityDirection::Outward => Direction::Outward,
            VelocityDirection::Inward => Direction::Inward,
        }),
        speed: m.initial_speed,
        speed_random: m.initial_speed_randomness,
        spread: m.initial_direction_spread,
        drag: [m.start_drag, m.mid_drag, m.end_drag],
        gravity: [m.start_gravity, m.mid_gravity, m.end_gravity],
        colour: Keyed {
            start: a.start_color,
            start_random: a.start_color_randomness,
            mid: a.mid_color,
            mid_random: a.mid_color_randomness,
            end: a.end_color,
            end_random: a.end_color_randomness,
        },
        size: Keyed {
            start: a.start_size,
            start_random: a.start_size_randomness,
            mid: a.mid_size,
            mid_random: a.mid_size_randomness,
            end: a.end_size,
            end_random: a.end_size_randomness,
        },
        rotation: a.initial_rotation_angle,
        rotation_random: a.initial_rotation_angle_randomness,
        spin: Keyed {
            start: a.start_rotation_speed,
            start_random: a.start_rotation_speed_randomness,
            mid: a.mid_rotation_speed,
            mid_random: a.mid_rotation_speed_randomness,
            end: a.end_rotation_speed,
            end_random: a.end_rotation_speed_randomness,
        },
        random_spin_direction: a.random_rotation_direction(),
        view_alignment: a.view_alignment() as u8,
    }
}

fn ribbon(r: &particle::Ribbon<'_>) -> Ribbon {
    let i = &r.info;
    Ribbon {
        taper: i.segment_properties as u8 & 3,
        segments: i.segment_count() as u16,
        scroll_uv_percent: i.scroll_uv_percent,
        scale_uv_percent: i.scale_uv_percent,
        width: i.segment_width,
        head_colour: i.tint_color,
        tail_colour: i.end_color,
        granularity: i.segment_granularity,
    }
}

/// What the part did this run.
#[derive(Debug, Clone, Default)]
pub struct ParticleStage {
    /// Whether the part was read this run, or already in and kept.
    pub read: bool,
    pub kept: bool,
    /// Emitters read this run, and those refused (`archive: name`, why).
    pub emitters: usize,
    pub refused: Vec<(String, String)>,
    /// An archive that didn't read, stopping the part, with why.
    pub failed: Option<(String, String)>,
    /// The IR and the textures.
    pub files: Files,
    pub took: Duration,
}

/// The part, unless the extract already holds it
/// ([`IdentityDir::part_holds`]): each spawn archive's emitters into
/// `particles/<stem>.<ron|postcard>` and their textures into the store,
/// recorded once every archive's is in. An archive that won't read stops
/// the part and is reported ([`ParticleStage::failed`]); a file that won't
/// write stops the run. Its progress goes out as [`Event::Particles`]: none
/// of the archives done, then each as it reads (all at once when the part
/// is kept), openspec AT.26.
pub fn particle_stage(
    dir: &mut IdentityDir,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<ParticleStage> {
    let started = Instant::now();
    let usrdir = PathBuf::from(&dir.manifest().source_dir).join("PS3_GAME/USRDIR");
    let identity = dir.manifest().identity.clone();
    let mut stage = ParticleStage::default();
    let archives = archives();
    let total = archives.len();
    send(Event::Particles { done: 0, total });
    let irs: Vec<String> = archives.iter().map(|a| path(ir, a)).collect();
    if dir.part_holds(PART, &irs)? {
        let recorded = dir.manifest().parts[PART].clone();
        count_kept(dir, &recorded, &mut stage.files);
        stage.kept = true;
        stage.took = started.elapsed();
        send(Event::Particles { done: total, total });
        return Ok(stage);
    }
    let mut read = Vec::with_capacity(archives.len());
    for (i, (archive, rel)) in archives.iter().zip(&irs).enumerate() {
        let one = scripts::entry(&usrdir, archive).and_then(|bytes| {
            let igz = Igz::parse(&bytes).map_err(|e| e.to_string())?;
            let r = emitters(&igz, archive, identity.clone());
            let bytes = ir.encode(&r.ir, dir, rel).map_err(|e| e.to_string())?;
            Ok((r, bytes))
        });
        match one {
            Ok(r) => read.push(r),
            Err(reason) => {
                let e = ExtractError::ScriptsUnreadable {
                    part: PART.to_owned(),
                    archive: archive.clone(),
                    reason,
                };
                stage.failed = Some((archive.clone(), e.to_string()));
                stage.took = started.elapsed();
                return Ok(stage);
            }
        }
        send(Event::Particles { done: i + 1, total });
    }
    let mut files = Vec::new();
    for ((r, bytes), rel) in read.into_iter().zip(&irs) {
        for (t, ktx2) in &r.textures {
            if !files.contains(t) {
                put(dir, t, ktx2, &mut stage.files)?;
                files.push(t.clone());
            }
        }
        put(dir, rel, &bytes, &mut stage.files)?;
        files.push(rel.clone());
        stage.emitters += r.ir.emitters.len();
        stage.refused.extend(
            r.ir.refused
                .iter()
                .map(|(n, why)| (format!("{}: {n}", r.ir.archive), why.clone())),
        );
    }
    dir.record_part(PART, files)?;
    stage.read = true;
    stage.took = started.elapsed();
    Ok(stage)
}

#[cfg(test)]
mod tests;
