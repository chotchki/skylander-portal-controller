//! The completion screen's fireworks at three stars (openspec AS.621): what
//! `UI - Objectives Menu - Start Fireworks` and `Update Fireworks` do,
//! read off the objectives screen's scripts and run here as they run them,
//! one step a 60th of a second of the screen's time ([`Show`]).
//!
//! Start makes one rocket and sets the timeout 8 s on. Each step, over the
//! rockets from the last made down: before its trail's life is out (1 s)
//! the trail moves by the rocket's velocity and the velocity's y grows 0.1
//! (screen units a step, y down: a rocket leaves upward at 2 to 7 and
//! slows, sideways at 2 to 5 either way); once it's out, the burst is made
//! where the trail is, the trail taken off; 0.9 s after that the burst is
//! taken off and the rocket is gone. Then, while the timeout isn't reached,
//! `random(10)` is rolled (0 to 10, both ends: one in eleven) and a 1 makes
//! a rocket while under 20 are up; past it, the hold ends once no rocket is
//! left, so the pads come back between 8 and 9.9 s after it began. Every
//! roll is the session's generator's, as the original's are: five a rocket
//! (its trail's life, its two speeds, which way across, its trail's
//! colour), one each step of the hold, one a burst (its own colour), the
//! colours a member of the scripts' `Fireworks` set each.
//!
//! What a rocket draws is the screen's two emitter templates' generators
//! (`UI_win_fireworks_trail`, `UI_win_fireworks_explosion`), read off the
//! screen's entry in the screen-scripts part with guac-extract's particle
//! reader and stepped by [`guacamole::particles`]' rules on the particles'
//! own random numbers, as a level's emitters are. They're laid in Bevy's UI
//! as images over the panel (every template of the screen sits at the same
//! depth, so they draw in the order made: after the panel), each the
//! generator's texture tinted by its keyed colour and the rocket's.
//! Stand-ins (the design doc's ledger, `fireworks`): a template's scale
//! (20) read as 20 screen units a metre, the screen container's origin as
//! the screen's middle, a sprite taken off taking its particles with it,
//! and an added generator drawn by an alpha its texture's brightness gives
//! (Bevy's UI blends by alpha only).

use std::path::Path;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use guac_extract::intros::IrFormat;
use guac_formats::igz::Igz;
use guac_formats::texture::{Format, Texture};
use guac_ir::interface::Content;
use guac_ir::particle::{Blend, Emitter, Kind};
use guac_script::{Generator, Value};
use guacamole::particles::{self, Cosmetic, Frame, GeneratorState};
use guacamole::scripts::ScriptRuntime;
use serde::Serialize;
use tracing::{info, warn};

use crate::completion::{CompletionOpen, TabCycle};
use crate::hero_scripts::ScriptSource;

/// Over the completion screen (930), under the portal's alerts (940).
pub(crate) const FIREWORKS_Z: i32 = crate::completion::COMPLETION_Z + 1;

/// The screen's archive, whose entry holds the two emitters.
const SCREEN: &str = "misc/ui_objectivesscreen.bld";
/// The templates a rocket's trail and its burst are made from.
const TRAIL_TEMPLATE: &str = "UI_win_fireworks_trail";
const BURST_TEMPLATE: &str = "UI_win_fireworks_explosion";
/// The scripts' set of the fireworks' colours.
const COLOURS: &str = "Fireworks";

/// At most this many rockets up at once.
pub(crate) const MOST_UP: usize = 20;
/// Each step's roll, `random(10)`; a 1 makes a rocket.
pub(crate) const ODDS: u32 = 10;
/// One step of the screen's update.
pub(crate) const STEP: f32 = 1.0 / 60.0;
/// The hold's timeout past its start.
pub(crate) const TIMEOUT: f32 = 8.0;
/// A trail's life: 1 plus `random(0.0)`.
pub(crate) const TRAIL_LIFE: f32 = 1.0;
/// A burst's life past its trail's.
pub(crate) const BURST_LIFE: f32 = 0.9;
/// What a step adds to a rocket's y velocity.
pub(crate) const FALL: f32 = 0.1;
/// Screen units a metre of a UI emitter: the templates' scale, 20 (stand-in).
pub(crate) const UNITS_A_METRE: f32 = 20.0;
/// Steps a frame catches up at most, so a long stall doesn't run away.
const MOST_STEPS: u32 = 30;
/// A template's colour that tints nothing.
const NEUTRAL: [u8; 4] = [128, 128, 128, 255];
/// Where the TV's own particles start, apart from the level's.
const PARTICLE_SEED: u64 = 0x4153_2E36_3231;

/// `random(n)` of a 32-bit draw, guac-script's rule: 0 to `n`, both ends.
pub(crate) fn roll(n: u32, number: u32) -> u32 {
    (number & 0x7fff_ffff) % (n + 1)
}

/// One lit template: where it is (screen units off the middle, y down),
/// its colour (four codes, 128 neutral) and its generators' state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lit {
    pub(crate) at: Vec2,
    pub(crate) tint: [u8; 4],
    pub(crate) generators: Vec<GeneratorState>,
}

impl Lit {
    fn new(at: Vec2, tint: [u8; 4], emitter: Option<&Emitter>) -> Self {
        Self {
            at,
            tint,
            generators: vec![GeneratorState::default(); emitter.map_or(0, |e| e.generators.len())],
        }
    }
}

/// One rocket of the scripts' particle set.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rocket {
    /// Its trail's place, screen units off the middle, y down.
    pub(crate) at: Vec2,
    /// Screen units a step.
    pub(crate) velocity: Vec2,
    /// The screen's time its trail's life is out.
    pub(crate) trail_ends: f32,
    pub(crate) trail: Option<Lit>,
    pub(crate) burst: Option<Lit>,
}

/// The two emitters a rocket draws.
#[derive(Debug, Clone)]
pub(crate) struct Emitters {
    pub(crate) trail: Arc<Emitter>,
    pub(crate) burst: Arc<Emitter>,
}

/// The fireworks' run: the screen's time since they started, the rockets
/// up and what's been made.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Show {
    pub(crate) clock: f32,
    pub(crate) rockets: Vec<Rocket>,
    /// Rockets made, the first included.
    pub(crate) made: u32,
    pub(crate) bursts: u32,
    /// The most up at once.
    pub(crate) most_up: usize,
    /// Steps run.
    pub(crate) steps: u32,
    /// Draws taken off the generator.
    pub(crate) draws: u32,
}

/// What the scripts' rolls draw on, counted.
struct Draws<'a> {
    draw: &'a mut dyn FnMut() -> u32,
    taken: u32,
}

impl Draws<'_> {
    fn next(&mut self) -> u32 {
        self.taken += 1;
        (self.draw)()
    }

    /// A random member of `colours`, as `setvariant.random` picks one:
    /// a draw modulo its count; an empty set draws nothing and is neutral.
    fn colour(&mut self, colours: &[[u8; 4]]) -> [u8; 4] {
        if colours.is_empty() {
            return NEUTRAL;
        }
        colours[self.next() as usize % colours.len()]
    }
}

impl Show {
    /// `Start Fireworks`: the first rocket, made at once.
    pub(crate) fn start(
        colours: &[[u8; 4]],
        emitters: Option<&Emitters>,
        draw: &mut dyn FnMut() -> u32,
    ) -> Self {
        let mut show = Self {
            clock: 0.0,
            rockets: Vec::new(),
            made: 0,
            bursts: 0,
            most_up: 0,
            steps: 0,
            draws: 0,
        };
        let mut draws = Draws { draw, taken: 0 };
        show.make(colours, emitters, &mut draws);
        show.draws = draws.taken;
        show
    }

    /// `Create Fireworks Rocket`.
    fn make(&mut self, colours: &[[u8; 4]], emitters: Option<&Emitters>, draws: &mut Draws<'_>) {
        // The trail's life, 1 + random(0.0): a draw that adds nothing.
        draws.next();
        let mut x = 2.0 + roll(3, draws.next()) as f32;
        if roll(1, draws.next()) == 0 {
            x = -x;
        }
        let y = -2.0 - roll(5, draws.next()) as f32;
        let tint = draws.colour(colours);
        self.rockets.push(Rocket {
            at: Vec2::ZERO,
            velocity: Vec2::new(x, y),
            trail_ends: self.clock + TRAIL_LIFE,
            trail: Some(Lit::new(
                Vec2::ZERO,
                tint,
                emitters.map(|e| e.trail.as_ref()),
            )),
            burst: None,
        });
        self.made += 1;
        self.most_up = self.most_up.max(self.rockets.len());
    }

    /// One step of `Update Fireworks`, then the particles' step.
    pub(crate) fn step(
        &mut self,
        colours: &[[u8; 4]],
        emitters: Option<&Emitters>,
        draw: &mut dyn FnMut() -> u32,
        rng: &mut Cosmetic,
    ) {
        self.clock += STEP;
        self.steps += 1;
        let now = self.clock;
        let mut draws = Draws { draw, taken: 0 };
        for i in (0..self.rockets.len()).rev() {
            let r = &mut self.rockets[i];
            if now < r.trail_ends {
                r.at += r.velocity;
                r.velocity.y += FALL;
                if let Some(t) = &mut r.trail {
                    t.at = r.at;
                }
            } else if now < r.trail_ends + BURST_LIFE {
                if r.burst.is_none() {
                    let tint = draws.colour(colours);
                    r.burst = Some(Lit::new(r.at, tint, emitters.map(|e| e.burst.as_ref())));
                    r.trail = None;
                    self.bursts += 1;
                }
            } else {
                self.rockets.remove(i);
            }
        }
        if now < TIMEOUT {
            let n = roll(ODDS, draws.next());
            if self.rockets.len() < MOST_UP && n == 1 {
                self.make(colours, emitters, &mut draws);
            }
        }
        self.draws += draws.taken;
        if let Some(e) = emitters {
            for r in &mut self.rockets {
                for (lit, emitter) in [(&mut r.trail, &e.trail), (&mut r.burst, &e.burst)] {
                    let Some(lit) = lit else { continue };
                    let frame = frame_at(lit.at);
                    for (state, g) in lit.generators.iter_mut().zip(&emitter.generators) {
                        state.step(g, frame, true, STEP, rng);
                    }
                }
            }
        }
    }

    /// The hold's over: the timeout reached and no rocket left
    /// (`End Animation Mode`).
    pub(crate) fn over(&self) -> bool {
        self.clock >= TIMEOUT && self.rockets.is_empty()
    }
}

/// A lit template's frame for the particles: screen units, y up.
fn frame_at(at: Vec2) -> Frame {
    Frame {
        at: Vec3::new(at.x, -at.y, 0.0),
        turn: Quat::IDENTITY,
        scale: UNITS_A_METRE,
    }
}

/// One particle as the screen draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Quad {
    /// 0 the trail's, 1 the burst's.
    pub(crate) emitter: usize,
    pub(crate) generator: usize,
    /// Its middle, screen units off the screen's middle, y down.
    pub(crate) at: Vec2,
    /// Its side, screen units.
    pub(crate) side: f32,
    /// Radians, clockwise on the screen.
    pub(crate) turn: f32,
    /// What its texture is multiplied by: red, green, blue, alpha.
    pub(crate) colour: [f32; 4],
}

/// Every particle `show` draws this step.
pub(crate) fn quads(show: &Show, emitters: &Emitters) -> Vec<Quad> {
    let mut out = Vec::new();
    for r in &show.rockets {
        for (k, lit, emitter) in [
            (0, &r.trail, &emitters.trail),
            (1, &r.burst, &emitters.burst),
        ] {
            let Some(lit) = lit else { continue };
            let frame = frame_at(lit.at);
            for (generator, (state, g)) in
                lit.generators.iter().zip(&emitter.generators).enumerate()
            {
                let Kind::Sprite(s) = &g.kind else { continue };
                let at = frame.of(g);
                let mid = particles::midpoint(s.midpoint_percent);
                for p in &state.particles {
                    let t = (p.age / p.life).min(1.0);
                    let place = if s.inherits_translation {
                        at.at + p.at
                    } else {
                        p.at
                    };
                    let size = particles::keyed_size(&s.size, t, mid, p.draws[1]);
                    let c = particles::keyed_colour(&s.colour, t, mid, p.draws[0]);
                    let tint = lit.tint.map(f32::from);
                    out.push(Quad {
                        emitter: k,
                        generator,
                        at: Vec2::new(place.x, -place.y),
                        side: size[0].max(0.0) * at.scale,
                        turn: -p.rotation,
                        colour: [
                            c[0] / 127.0 * tint[0] / 128.0,
                            c[1] / 127.0 * tint[1] / 128.0,
                            c[2] / 127.0 * tint[2] / 128.0,
                            (c[3] / 255.0 * tint[3] / 255.0).clamp(0.0, 1.0),
                        ],
                    });
                }
            }
        }
    }
    out
}

/// The `Fireworks` set's colours off the run's VM, each four codes (the
/// VM keeps a colour red high); none when it isn't there.
fn colours_of(runtime: &ScriptRuntime) -> Vec<[u8; 4]> {
    let program = runtime.vm.program();
    let Some(g) = program.global_named(COLOURS) else {
        return Vec::new();
    };
    let Some((_, guac_script::Global::Set { members, .. })) = program.globals.get(g as usize)
    else {
        return Vec::new();
    };
    let members = members.clone();
    members
        .iter()
        .filter_map(|m| match m {
            guac_script::Elem::Global(m) => runtime.vm.global_peek(*m).or_else(|| {
                // Not read yet: what the file holds.
                match program.globals.get(*m as usize) {
                    Some((_, guac_script::Global::Number(v))) => Some(*v),
                    _ => None,
                }
            }),
            _ => None,
        })
        .filter_map(|v| match v {
            Value::Color(c) => Some(c.to_be_bytes()),
            _ => None,
        })
        .collect()
}

/// The screen's emitters with an image for each generator's texture, or
/// why there's none.
#[derive(Clone)]
pub(crate) struct Art {
    pub(crate) emitters: Emitters,
    /// Per emitter (trail, burst), per generator.
    pub(crate) images: [Vec<Option<Handle<Image>>>; 2],
}

/// The fireworks' state on the TV.
#[derive(Resource)]
pub(crate) struct Fireworks {
    pub(crate) show: Option<Show>,
    pub(crate) art: Option<Result<Art, String>>,
    colours: Vec<[u8; 4]>,
    /// Seconds of the screen's time owed to the next step.
    owed: f32,
    rng: Cosmetic,
    /// The draws when the run has no generator of its own (a test's app).
    own: Generator,
    /// The pool of images laid each frame, and how many it laid.
    root: Option<Entity>,
    pool: Vec<Entity>,
    pub(crate) drawn: usize,
    pub(crate) most_drawn: usize,
}

impl Default for Fireworks {
    fn default() -> Self {
        Self {
            show: None,
            art: None,
            colours: Vec::new(),
            owed: 0.0,
            rng: Cosmetic::new(PARTICLE_SEED),
            own: Generator::seeded(5489),
            root: None,
            pool: Vec::new(),
            drawn: 0,
            most_drawn: 0,
        }
    }
}

/// The fireworks' part of `/api/_test/tv`'s `completion`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FireworksShot {
    /// The hold's time so far.
    pub secs: f32,
    pub up: usize,
    pub made: u32,
    pub bursts: u32,
    pub most_up: usize,
    pub steps: u32,
    pub draws: u32,
    /// Particles laid this frame, and the most in a frame.
    pub drawn: usize,
    pub most_drawn: usize,
    /// `disc`, or `missing: <why>` (nothing drawn, the rockets still run).
    pub art: String,
    /// The `Fireworks` set's colours read.
    pub colours: usize,
}

impl Fireworks {
    pub(crate) fn shot(&self) -> Option<FireworksShot> {
        let show = self.show.as_ref()?;
        Some(FireworksShot {
            secs: show.clock,
            up: show.rockets.len(),
            made: show.made,
            bursts: show.bursts,
            most_up: show.most_up,
            steps: show.steps,
            draws: show.draws,
            drawn: self.drawn,
            most_drawn: self.most_drawn,
            art: match &self.art {
                Some(Ok(_)) => "disc".into(),
                Some(Err(why)) => format!("missing: {why}"),
                None => String::new(),
            },
            colours: self.colours.len(),
        })
    }

    /// The fireworks begin (the tally's last tab out at three stars).
    pub(crate) fn begin(
        &mut self,
        runtime: Option<&mut ScriptRuntime>,
        source: Option<&ScriptSource>,
        images: &mut Assets<Image>,
    ) {
        if self.art.is_none() {
            let art = match source.and_then(ScriptSource::extract) {
                Some((dir, ir)) => load_art(dir, *ir, images),
                None => Err("the run's extract isn't known".into()),
            };
            if let Err(why) = &art {
                warn!(reason = %why, "the fireworks draw nothing");
            }
            self.art = Some(art);
        }
        let emitters = self.emitters();
        self.colours = runtime.as_deref().map(colours_of).unwrap_or_default();
        self.owed = 0.0;
        self.most_drawn = 0;
        let own = &mut self.own;
        let show = match runtime {
            Some(r) => Show::start(&self.colours, emitters.as_ref(), &mut || {
                r.generator.next_u32()
            }),
            None => Show::start(&self.colours, emitters.as_ref(), &mut || own.next_u32()),
        };
        info!(colours = self.colours.len(), "the fireworks begin");
        self.show = Some(show);
    }

    fn emitters(&self) -> Option<Emitters> {
        match &self.art {
            Some(Ok(a)) => Some(a.emitters.clone()),
            _ => None,
        }
    }

    /// `secs` of the screen's time: whole steps run, the rest owed.
    pub(crate) fn advance(&mut self, secs: f32, runtime: Option<&mut ScriptRuntime>) {
        let emitters = self.emitters();
        let Some(show) = &mut self.show else {
            return;
        };
        self.owed += secs;
        let mut steps = 0;
        let own = &mut self.own;
        let mut runtime = runtime;
        while self.owed >= STEP && steps < MOST_STEPS && !show.over() {
            self.owed -= STEP;
            steps += 1;
            match runtime.as_deref_mut() {
                Some(r) => show.step(
                    &self.colours,
                    emitters.as_ref(),
                    &mut || r.generator.next_u32(),
                    &mut self.rng,
                ),
                None => show.step(
                    &self.colours,
                    emitters.as_ref(),
                    &mut || own.next_u32(),
                    &mut self.rng,
                ),
            }
        }
        if steps == MOST_STEPS {
            self.owed = 0.0;
        }
    }

    /// Ends the show: nothing left of it to draw.
    pub(crate) fn end(&mut self) {
        if self.show.take().is_some() {
            info!("the fireworks are over");
        }
        self.owed = 0.0;
    }
}

/// The two templates' emitters off the screen's entry at `dir`, their
/// textures decoded for Bevy's UI.
pub(crate) fn load_art(
    dir: &Path,
    ir: IrFormat,
    images: &mut Assets<Image>,
) -> Result<Art, String> {
    let record = guacamole::interface_host::read_record(
        dir,
        ir,
        &guac_extract::interface::path(ir, SCREEN),
    )?;
    let named = |template: &str| -> Result<String, String> {
        let t = record
            .templates
            .iter()
            .find(|t| t.name == template)
            .ok_or_else(|| format!("{SCREEN} holds no {template}"))?;
        match &t.content {
            Content::Emitter(Some(name)) => Ok(name.clone()),
            _ => Err(format!("{template} holds no emitter")),
        }
    };
    let (trail, burst) = (named(TRAIL_TEMPLATE)?, named(BURST_TEMPLATE)?);
    let rel = guac_extract::screens::path(SCREEN);
    let bytes = std::fs::read(dir.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
    let igz = Igz::parse(&bytes).map_err(|e| format!("{rel}: {e}"))?;
    let identity = record.identity.clone();
    let read = guac_extract::particles::emitters(&igz, SCREEN, identity);
    let find = |name: &str| -> Result<Arc<Emitter>, String> {
        read.ir
            .emitters
            .iter()
            .find(|e| e.name == name)
            .cloned()
            .map(Arc::new)
            .ok_or_else(|| {
                let why = read
                    .ir
                    .refused
                    .iter()
                    .find(|(n, _)| n == name)
                    .map_or("not in its entry".to_owned(), |(_, w)| w.clone());
                format!("{name}: {why}")
            })
    };
    let emitters = Emitters {
        trail: find(&trail)?,
        burst: find(&burst)?,
    };
    let mut image = |g: &guac_ir::particle::Generator| -> Option<Handle<Image>> {
        let t = g.texture.as_ref()?;
        let ktx2 = read
            .textures
            .iter()
            .find(|(p, _)| *p == t.texture)
            .map(|(_, b)| b.clone())
            .or_else(|| std::fs::read(dir.join(&t.texture)).ok())?;
        let (w, h, mut rgba) = rgba_of_ktx2(&ktx2)?;
        if g.blend == Blend::Add {
            added_as_alpha(&mut rgba);
        }
        Some(images.add(Image::new(
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        )))
    };
    let images = [
        emitters.trail.generators.iter().map(&mut image).collect(),
        emitters.burst.generators.iter().map(&mut image).collect(),
    ];
    Ok(Art { emitters, images })
}

/// A KTX2 the extract wrote, its first level decoded to RGBA8 rows top
/// row first: its width, height and texels; `None` for one that doesn't
/// read.
pub(crate) fn rgba_of_ktx2(ktx2: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(ktx2.get(at..at + 4)?.try_into().ok()?))
    };
    let long = |at: usize| -> Option<u64> {
        Some(u64::from_le_bytes(ktx2.get(at..at + 8)?.try_into().ok()?))
    };
    if ktx2.get(..12)? != b"\xABKTX 20\xBB\r\n\x1A\n" {
        return None;
    }
    let vk = word(12)?;
    let (w, h) = (word(20)?, word(24)?);
    // No supercompression: the extract never writes one.
    if word(44)? != 0 {
        return None;
    }
    let format = [
        Format::Bc1RgbaUnorm,
        Format::Bc3Unorm,
        Format::R8Unorm,
        Format::Rgba8Unorm,
    ]
    .into_iter()
    .find(|f| f.vk_format() == vk)?;
    let (offset, length) = (long(80)? as usize, long(88)? as usize);
    let level = ktx2.get(offset..offset.checked_add(length)?)?.to_vec();
    let texture = Texture {
        format,
        width: w,
        height: h,
        levels: vec![level],
    };
    Some((w, h, texture.rgba8_top_down(0).ok()?))
}

/// An added texture as one Bevy's UI can blend: its brightest channel,
/// times its alpha, as the alpha, the colour divided by it, so drawn by
/// alpha it adds about what it would added (and dims what's under it by
/// the same).
pub(crate) fn added_as_alpha(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = f32::from(px[3]) / 255.0;
        let rgb = [px[0], px[1], px[2]].map(|c| f32::from(c) / 255.0 * a);
        let m = rgb.iter().copied().fold(0.0f32, f32::max);
        if m <= 0.0 {
            px.copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for (c, v) in px.iter_mut().zip(rgb) {
            *c = (v / m * 255.0).round() as u8;
        }
        px[3] = (m * 255.0).round() as u8;
    }
}

/// Runs the fireworks while the tally holds for them, on the screen's
/// time, and ends them with the screen.
pub(crate) fn step_fireworks(
    (open, cycle, time): (Res<CompletionOpen>, Res<TabCycle>, Res<Time<Real>>),
    world: Option<Res<guacamole::scripts::ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
    runtime: Option<ResMut<ScriptRuntime>>,
    mut images: ResMut<Assets<Image>>,
    mut fireworks: ResMut<Fireworks>,
) {
    let mut runtime = runtime;
    // Gone with the screen: its continue, or the level leaving.
    let at_screen = world.as_deref().is_some_and(crate::completion::is_open);
    if !cycle.fireworks || !at_screen {
        fireworks.end();
        return;
    }
    if !open.0 {
        return;
    }
    if fireworks.show.is_none() {
        fireworks.begin(runtime.as_deref_mut(), source.as_deref(), &mut images);
        return;
    }
    fireworks.advance(time.delta_secs(), runtime.as_deref_mut());
}

/// Lays the fireworks' particles as images over the screen, a pool of
/// them reused frame to frame; hidden while the screen steps aside, none
/// once the show's over.
pub(crate) fn draw_fireworks(
    mut commands: Commands,
    open: Res<CompletionOpen>,
    mut fireworks: ResMut<Fireworks>,
    mut pieces: Query<(&mut ImageNode, &mut UiTransform, &mut Visibility)>,
    mut roots: Query<&mut Visibility, Without<ImageNode>>,
) {
    let fw = &mut *fireworks;
    let quads = match (&fw.show, &fw.art) {
        (Some(show), Some(Ok(art))) => quads(show, &art.emitters),
        _ => Vec::new(),
    };
    let Some(Ok(art)) = &fw.art else {
        fw.drawn = 0;
        return;
    };
    if fw.show.is_none() {
        if let Some(root) = fw.root.take() {
            commands.entity(root).despawn();
        }
        fw.pool.clear();
        fw.drawn = 0;
        return;
    }
    let root = *fw.root.get_or_insert_with(|| {
        commands
            .spawn((
                Name::new("fireworks"),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                bevy::ui::FocusPolicy::Pass,
                GlobalZIndex(FIREWORKS_Z),
            ))
            .id()
    });
    if let Ok(mut v) = roots.get_mut(root) {
        v.set_if_neq(if open.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let mut laid = 0;
    for q in &quads {
        let Some(Some(image)) = art.images[q.emitter].get(q.generator) else {
            continue;
        };
        let transform = UiTransform {
            translation: Val2::px(
                q.at.x * crate::completion::UNIT,
                q.at.y * crate::completion::UNIT,
            ),
            scale: Vec2::splat(q.side * crate::completion::UNIT / BASE),
            rotation: Rot2::radians(q.turn),
        };
        let colour = Color::srgba(q.colour[0], q.colour[1], q.colour[2], q.colour[3]);
        if let Some(&e) = fw.pool.get(laid)
            && let Ok((mut node, mut t, mut v)) = pieces.get_mut(e)
        {
            if node.image != *image {
                node.image = image.clone();
            }
            node.color = colour;
            t.set_if_neq(transform);
            v.set_if_neq(Visibility::Inherited);
        } else if laid >= fw.pool.len() {
            let e = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(50),
                        top: percent(50),
                        width: px(BASE),
                        height: px(BASE),
                        margin: UiRect {
                            left: px(-BASE / 2.0),
                            top: px(-BASE / 2.0),
                            ..default()
                        },
                        ..default()
                    },
                    ImageNode {
                        image: image.clone(),
                        color: colour,
                        ..default()
                    },
                    transform,
                    Visibility::Inherited,
                    ChildOf(root),
                ))
                .id();
            fw.pool.push(e);
        }
        laid += 1;
    }
    for &e in fw.pool.iter().skip(laid) {
        if let Ok((_, _, mut v)) = pieces.get_mut(e) {
            v.set_if_neq(Visibility::Hidden);
        }
    }
    fw.drawn = laid;
    fw.most_drawn = fw.most_drawn.max(laid);
}

/// A pooled image's side before its scale, pixels.
const BASE: f32 = 64.0;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use guac_ir::model::{Filter, Sampler, Transform as Offset, Wrap};
    use guac_ir::particle::{Direction, Generator as Gen, Keyed, Sprite, Texture as Tex, Volume};

    /// A sprite generator made up for a test: `rate` a second, `most`
    /// alive, each living `life`, leaving at `speed` m/s.
    fn sprite(rate: f32, most: u16, life: f32, speed: f32, blend: Blend) -> Gen {
        fn keyed<T: Copy>(start: T, end: T, zero: T) -> Keyed<T> {
            Keyed {
                start,
                start_random: zero,
                mid: start,
                mid_random: zero,
                end,
                end_random: zero,
            }
        }
        Gen {
            blend,
            overlay_layer: 5,
            bloom: 0.0,
            offset: Offset {
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            start_delay: 0.0,
            texture: Some(Tex {
                texture: "textures/t.ktx2".into(),
                image: "t.png".into(),
                sampler: Sampler {
                    wrap_u: Wrap::Clamp,
                    wrap_v: Wrap::Clamp,
                    mag: Filter::Linear,
                    min: Filter::Linear,
                    mip: None,
                },
            }),
            kind: Kind::Sprite(Sprite {
                spawn_rate: rate,
                max_alive: most,
                lifespan: life,
                lifespan_random: 0.0,
                max_distance: 30.0,
                midpoint_percent: 50.0,
                next_gen_scale: 1.0,
                volume: Some(Volume::Sphere),
                volume_size: [1.0; 3],
                uniform: false,
                inherits_translation: false,
                direction: Some(Direction::Outward),
                speed,
                speed_random: 0.0,
                spread: 0.0,
                drag: [0.0; 3],
                gravity: [0.0; 3],
                colour: keyed([127, 127, 127, 255], [127, 127, 127, 0], [0; 4]),
                size: keyed([0.5; 3], [0.5; 3], [0.0; 3]),
                rotation: 0.0,
                rotation_random: 0.0,
                spin: keyed(0.0, 0.0, 0.0),
                random_spin_direction: false,
                view_alignment: 0,
            }),
        }
    }

    pub(crate) fn test_emitters() -> Emitters {
        Emitters {
            trail: Arc::new(Emitter {
                name: "trail".into(),
                object: 1,
                generators: vec![sprite(20.0, 10, 0.3, 2.0, Blend::Add)],
                unread: Vec::new(),
            }),
            burst: Arc::new(Emitter {
                name: "burst".into(),
                object: 2,
                generators: vec![sprite(9999.0, 30, 1.5, 8.0, Blend::Blend)],
                unread: Vec::new(),
            }),
        }
    }

    const COLOURS3: [[u8; 4]; 3] = [
        [46, 138, 36, 255],
        [100, 201, 238, 255],
        [225, 46, 193, 255],
    ];

    /// Runs a show to its end on `draw`, checking each step with `each`.
    fn run(draw: &mut dyn FnMut() -> u32, mut each: impl FnMut(&Show)) -> Show {
        let e = test_emitters();
        let mut rng = Cosmetic::new(1);
        let mut show = Show::start(&COLOURS3, Some(&e), draw);
        while !show.over() {
            assert!(show.steps < 2000, "the hold never ends");
            show.step(&COLOURS3, Some(&e), draw, &mut rng);
            each(&show);
        }
        show
    }

    /// A draw of `n` rolls `random(10)` to 1 (and every other roll to 1
    /// or past: `random(1)` reads 1, `random(3)` 1, `random(5)` 1).
    const ALWAYS: u32 = 1;

    /// openspec AS.621: never more than 20 up, even with every roll a 1;
    /// the 20 reached and new ones made as old ones go.
    #[test]
    fn never_more_than_twenty_are_up() {
        let mut most = 0;
        let show = run(&mut || ALWAYS, |s| {
            assert!(s.rockets.len() <= MOST_UP, "{} up", s.rockets.len());
            most = most.max(s.rockets.len());
        });
        assert_eq!(most, MOST_UP);
        assert!(show.made > MOST_UP as u32, "{} made", show.made);
    }

    /// The session's generator, seeded: one roll each step of the 8 s
    /// (480), five a rocket, one a burst, every rocket bursting; about one
    /// step in eleven makes a rocket.
    #[test]
    fn a_rocket_comes_on_about_one_step_in_eleven_on_the_sessions_rolls() {
        let mut g = Generator::seeded(5489);
        let mut draws = 0u32;
        let mut rolled = 0u32;
        let show = run(
            &mut || {
                draws += 1;
                g.next_u32()
            },
            |s| rolled += u32::from(s.clock < TIMEOUT),
        );
        // The 8 s's steps, 479 or 480 as the clock's sum rounds.
        assert!((479..=480).contains(&rolled), "{rolled}");
        assert_eq!(show.bursts, show.made, "every rocket bursts");
        assert_eq!(show.draws, draws);
        assert_eq!(draws, rolled + 5 * show.made + show.bursts);
        // The made beyond the first, against the 480 rolls' one in eleven.
        let rate = (show.made - 1) as f32 / rolled as f32;
        assert!((0.06..0.125).contains(&rate), "{} made: {rate}", show.made);
        // The same seed, the same show.
        let mut g2 = Generator::seeded(5489);
        let again = run(&mut || g2.next_u32(), |_| {});
        assert_eq!(again.made, show.made);
        assert_eq!(again.clock, show.clock);
    }

    /// A rocket climbs and slows, bursts where its trail is after 1 s, the
    /// burst lit 0.9 s; no rocket is made past 8 s, the hold over once the
    /// last is gone (8 to 9.9 s), and a show ended draws nothing.
    #[test]
    fn a_rocket_climbs_bursts_where_it_is_and_the_hold_ends_with_the_last() {
        let e = test_emitters();
        let mut rng = Cosmetic::new(1);
        // The first rocket's rolls, then nothing but 0s: no more rockets.
        let mut first = [0u32, 2, 1, 3, 0].into_iter();
        let mut show = Show::start(&COLOURS3, Some(&e), &mut || first.next().unwrap_or(0));
        let r = &show.rockets[0];
        // 2 + random(3) of 2 across, kept by random(1) of 1, 2 + 3 up.
        assert_eq!(r.velocity, Vec2::new(4.0, -5.0));
        assert_eq!(r.trail.as_ref().unwrap().tint, COLOURS3[0]);
        let mut top = 0.0f32;
        let mut burst_at = None;
        let mut trail_drew = false;
        while !show.over() {
            show.step(&COLOURS3, Some(&e), &mut || 0, &mut rng);
            if let Some(r) = show.rockets.first() {
                top = top.min(r.at.y);
                if r.trail.is_some() && !quads(&show, &e).is_empty() {
                    trail_drew = true;
                }
                if let (None, Some(b)) = (burst_at, &r.burst) {
                    burst_at = Some((show.clock, b.at, r.at));
                    assert!(!quads(&show, &e).is_empty(), "the burst draws at once");
                }
            }
        }
        assert!(trail_drew, "the trail draws particles");
        let (when, at, trail) = burst_at.expect("a burst");
        assert!((when - TRAIL_LIFE).abs() < 1.5 * STEP, "burst at {when}");
        assert_eq!(at, trail, "the burst at the trail's place");
        // 60 steps up from -5 a step, slowing 0.1 a step: about 123 units.
        assert!((-130.0..-115.0).contains(&top), "top {top}");
        assert_eq!(show.made, 1);
        assert!(
            (TIMEOUT..TIMEOUT + 2.0 * STEP).contains(&show.clock),
            "over at {}",
            show.clock
        );
        assert!(quads(&show, &e).is_empty(), "nothing left drawn");
    }

    /// The hold runs past 8 s while a rocket's still up, and ends with it.
    #[test]
    fn the_hold_waits_on_the_last_rocket() {
        let mut late = Vec::new();
        let show = run(&mut || ALWAYS, |s| {
            if s.clock > TIMEOUT {
                late.push(s.rockets.len());
            }
        });
        assert!(!late.is_empty() && late[0] > 0, "rockets up past 8 s");
        assert!(
            show.clock > TIMEOUT + 1.5
                && show.clock < TIMEOUT + TRAIL_LIFE + BURST_LIFE + 2.0 * STEP,
            "over at {}",
            show.clock
        );
    }

    /// A key from the environment, else the workspace's `.env.dev`.
    fn env_or_dotenv(key: &str) -> Option<String> {
        if let Ok(value) = std::env::var(key) {
            return Some(value);
        }
        let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
        let text = std::fs::read_to_string(dotenv).ok()?;
        text.lines().find_map(|line| {
            let (k, v) = line.trim().split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
        })
    }

    /// openspec AS.621 on the disc's extract (read only): the screen's two
    /// templates name emitters its entry holds, the trail's two generators
    /// and the burst's five each with an image; the scripts' rule over
    /// them, on a seeded generator, draws trails and bursts, at most 20 up.
    ///
    /// `cargo test -p guacamole --bin guacamole the_screens_fireworks -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the Giants extract at GUAC_EXTRACT_DIR"]
    fn the_screens_fireworks_load_off_the_extract_and_draw() {
        let root = std::path::PathBuf::from(
            env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"),
        );
        let dir = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("BLUS30968-01.00-"))
                    && p.join("manifest.json").is_file()
            })
            .expect("a Giants extract under GUAC_EXTRACT_DIR");
        let ir = if dir
            .join(guac_extract::interface::path(IrFormat::Ron, SCREEN))
            .is_file()
        {
            IrFormat::Ron
        } else {
            IrFormat::Postcard
        };
        let mut images = Assets::<Image>::default();
        let started = std::time::Instant::now();
        let art = load_art(&dir, ir, &mut images).unwrap_or_else(|e| panic!("{e}"));
        println!("loaded in {:?}", started.elapsed());
        assert_eq!(art.emitters.trail.generators.len(), 2);
        assert_eq!(art.emitters.burst.generators.len(), 5);
        for (k, list) in art.images.iter().enumerate() {
            assert!(list.iter().all(Option::is_some), "emitter {k}: {list:?}");
        }
        let mut g = Generator::seeded(5489);
        let mut rng = Cosmetic::new(PARTICLE_SEED);
        let colours = [
            [46, 138, 36, 255],
            [100, 201, 238, 255],
            [225, 46, 193, 255],
        ];
        let mut show = Show::start(&colours, Some(&art.emitters), &mut || g.next_u32());
        let (mut trails, mut bursts, mut most) = (0usize, 0usize, 0usize);
        while !show.over() {
            show.step(
                &colours,
                Some(&art.emitters),
                &mut || g.next_u32(),
                &mut rng,
            );
            assert!(show.rockets.len() <= MOST_UP);
            let q = quads(&show, &art.emitters);
            trails += q.iter().filter(|q| q.emitter == 0).count();
            bursts += q.iter().filter(|q| q.emitter == 1).count();
            most = most.max(q.len());
        }
        println!(
            "{} rockets, {} bursts, {} steps; quads: {trails} trail, {bursts} burst, {most} most a step",
            show.made, show.bursts, show.steps
        );
        assert!(trails > 0 && bursts > 0, "trails {trails}, bursts {bursts}");
        assert_eq!(show.made, show.bursts);
    }

    #[test]
    fn a_roll_is_zero_to_n_both_ends() {
        assert_eq!(roll(10, 1), 1);
        assert_eq!(roll(10, 10), 10);
        assert_eq!(roll(10, 11), 0);
        assert_eq!(roll(10, 0x8000_0001), 1);
    }

    /// An added texel keeps what it adds: its colour times its alpha comes
    /// back off the new colour times the new alpha.
    #[test]
    fn an_added_texel_drawn_by_alpha_adds_what_it_did() {
        let mut px = vec![200, 100, 50, 255, 0, 0, 0, 255, 255, 255, 255, 128];
        added_as_alpha(&mut px);
        assert_eq!(&px[0..4], &[255, 128, 64, 200]);
        assert_eq!(&px[4..8], &[0, 0, 0, 0], "black adds nothing");
        assert_eq!(px[11], 128);
    }

    /// A KTX2 as the extract writes one reads back texel for texel.
    #[test]
    fn a_written_ktx2_reads_back() {
        let t = Texture {
            format: Format::Rgba8Unorm,
            width: 2,
            height: 2,
            levels: vec![vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]],
        };
        let (w, h, rgba) = rgba_of_ktx2(&t.to_ktx2()).expect("reads");
        assert_eq!((w, h), (2, 2));
        // Top row first: the stored rows reversed.
        assert_eq!(
            rgba,
            [9, 10, 11, 12, 13, 14, 15, 16, 1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert!(rgba_of_ktx2(b"not a ktx2").is_none());
    }

    /// A particle's colour: its keyed colour over 127 times the rocket's
    /// over 128, alpha over 255 each.
    #[test]
    fn a_quad_is_tinted_by_its_rocket() {
        let e = test_emitters();
        let mut rng = Cosmetic::new(1);
        let mut rolls = [0u32, 0, 1, 0, 1].into_iter();
        let mut show = Show::start(&COLOURS3, Some(&e), &mut || rolls.next().unwrap_or(0));
        for _ in 0..10 {
            show.step(&COLOURS3, Some(&e), &mut || 0, &mut rng);
        }
        let q = quads(&show, &e);
        assert!(!q.is_empty());
        let tint = COLOURS3[1];
        for q in &q {
            assert_eq!(q.emitter, 0);
            assert!(
                (q.colour[1] - f32::from(tint[1]) / 128.0).abs() < 0.02,
                "{q:?}"
            );
            assert!((q.side - 0.5 * UNITS_A_METRE).abs() < 1e-3, "{q:?}");
        }
    }
}
