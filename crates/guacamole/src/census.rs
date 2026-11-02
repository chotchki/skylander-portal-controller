//! The pipelines the TV draws Giants 01.00 with, as numbers (openspec AE.25,
//! AG.43): every key the roster's figures draw on a card (AE.5's census of
//! the character models, with what each carries fresh since AE.23) and
//! every key chapter 1 draws (AG.19's) with its props (AG.25's) and what its
//! scripts spawn (AH.24's), so a first
//! start can compile them before any extraction, when nothing of the disc
//! is read yet.
//!
//! Which of the code's keys a part draws is the disc's: a key is a material
//! key ([`MaterialKey`]: the masks as drawn, the render states) on a mesh
//! shape ([`MeshShape`]), and the masks, states and vertex layouts the
//! character shader allows run to tens of thousands of pipelines, where the
//! 01.00 disc's figures draw 57 on a card (33 on skinned meshes, 24 on the
//! static parts they carry) and its chapter 1 another 67, its props 124
//! more (AG.60's start scripts stand and spawn more of them: 32 before;
//! AG.239's route clones 10 more; Norticus' turban 2, AH.138; every
//! placement's whose clones draw by AS.63's rule 56 more), what its
//! scripts spawn 7 more beyond
//! those (parts fading in among them, AH.67; 3 before AH.39 read
//! `global.bld`'s own models; 13 before AS.63's rule took 6 into its
//! props'), since AG.23 the level's, its props' and its
//! spawns' parts that leave the shadow to the scene receiving the level's
//! (chapter 1 55 before, its props 61), their glowing parts' level glow
//! masks 37 (22 before AH.39, 29 before AS.63; a mask reads no shadow) and the heroes' keys
//! in the level 81 (every character's as the hero past the cards': 56
//! colour keys and 25 glow masks, AH.65's point lights on; Eruptor's 2
//! alone until the arrival lane, 2026-10-31), the first fight's creatures'
//! 21 more past the heroes' and the cards' (AH.25: the Chompies' model and Norticus',
//! skinned in the hero's scene; AG.150's tortoises and cannon top; AH.139's
//! fading Chompy blending its opaque parts: 10 past Eruptor's; 2 before
//! AS.63 drew every skinned template's clones) and their glow mask, and the
//! particles of what its scripts spawn 1 more and its glow mask (AH.87).
//! So the tables
//! hold the census as numbers (no game bytes: a mask, a blend, a count of
//! texture-coordinate sets), and `tests/giants_census.rs` (`#[ignore]`d, on
//! `GUAC_GIANTS_DUMP`) holds them equal to what the extractor's readers and
//! the TV's own key derivation give off the dump, so a change to either
//! shows there. A key outside them still compiles when a shelf or a level
//! lands (AE.14's warm), as it did before.

use guac_ir::model::{
    self, Blend, Filter, Glow, Mask, Params, RenderStates, Sampler, TextureRole, TextureSlot, Wrap,
};

use crate::character::key::{KeyError, MaterialKey, Pass, ShaderKey, StateKey};
use crate::character::warm::{MeshShape, WarmKey};

/// A drawn key as the tables keep it: the masks as drawn, the render
/// states, and the mesh's texture-coordinate sets, tangents and vertex
/// colours. Whether it's skinned is the table's: [`TURNTABLE`]'s are,
/// [`TURNTABLE_STATIC`]'s and [`LEVEL`]'s aren't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Row {
    pub pixel: u32,
    pub vertex: u32,
    pub blend: u8,
    pub depth_write: bool,
    pub two_sided: bool,
    pub alpha_test: bool,
    pub uv_sets: u8,
    pub tangents: bool,
    pub colours: bool,
}

/// [`Row::blend`]'s codes, in [`Blend`]'s order.
const BLENDS: [Blend; 4] = [
    Blend::Opaque,
    Blend::Alpha,
    Blend::Additive,
    Blend::Multiply,
];

const O: u8 = 0;
const A: u8 = 1;
const ADD: u8 = 2;
const MUL: u8 = 3;

#[allow(clippy::too_many_arguments)]
const fn row(
    pixel: u32,
    vertex: u32,
    blend: u8,
    depth_write: bool,
    two_sided: bool,
    alpha_test: bool,
    uv_sets: u8,
    tangents: bool,
    colours: bool,
) -> Row {
    Row {
        pixel,
        vertex,
        blend,
        depth_write,
        two_sided,
        alpha_test,
        uv_sets,
        tangents,
        colours,
    }
}

impl Row {
    /// The row's key, on a skinned mesh or a static one.
    pub fn key(&self, skinned: bool) -> Result<WarmKey, KeyError> {
        Ok(WarmKey {
            material: MaterialKey {
                shader: ShaderKey::new(self.pixel, self.vertex)?,
                states: StateKey {
                    blend: BLENDS[usize::from(self.blend)],
                    depth_write: self.depth_write,
                    two_sided: self.two_sided,
                    alpha_test: self.alpha_test,
                },
                pass: Pass::Colour,
            },
            mesh: MeshShape {
                uv_sets: self.uv_sets,
                tangents: self.tangents,
                colours: self.colours,
                skinned,
            },
        })
    }

    /// `key` as a row (its pass and whether it's skinned left out).
    pub fn of(key: &WarmKey) -> Self {
        let s = &key.material.states;
        Self {
            pixel: key.material.shader.pixel(),
            vertex: key.material.shader.vertex(),
            blend: BLENDS.iter().position(|b| *b == s.blend).unwrap_or(0) as u8,
            depth_write: s.depth_write,
            two_sided: s.two_sided,
            alpha_test: s.alpha_test,
            uv_sets: key.mesh.uv_sets,
            tangents: key.mesh.tangents,
            colours: key.mesh.colours,
        }
    }

    /// The row as the tables write it.
    pub fn source(&self) -> String {
        let blend = ["O", "A", "ADD", "MUL"][usize::from(self.blend)];
        format!(
            "row({:#06x}, {:#06x}, {blend}, {}, {}, {}, {}, {}, {}),",
            self.pixel,
            self.vertex,
            self.depth_write,
            self.two_sided,
            self.alpha_test,
            self.uv_sets,
            self.tangents,
            self.colours
        )
    }
}

/// Every key the 01.00 roster's figures draw on a card on a skinned mesh
/// (the turntable's scene: fog on, nothing else), each once: AE.5's census
/// of the 67 character models as the warm keys it, with the skinned parts
/// they carry fresh (AE.23).
pub const TURNTABLE: &[Row] = &[
    row(0x0400, 0x0400, O, true, false, false, 1, true, false),
    row(0x0404, 0x0404, O, true, false, false, 1, true, false),
    row(0x0404, 0x0404, ADD, false, false, false, 1, true, false),
    row(0x0404, 0x1404, O, true, false, false, 1, true, true),
    row(0x0404, 0x1404, A, false, false, false, 1, true, true),
    row(0x0404, 0x1404, A, false, true, false, 1, true, true),
    row(0x0418, 0x0418, O, true, false, false, 1, true, false),
    row(0x041c, 0x041c, O, true, false, false, 1, true, false),
    row(0x041c, 0x041c, O, true, false, true, 1, true, false),
    row(0x041c, 0x141c, O, true, false, false, 1, true, true),
    row(0x0420, 0x0420, O, true, false, false, 1, true, false),
    row(0x0424, 0x0424, O, true, false, false, 1, true, false),
    row(0x0424, 0x0424, O, true, false, true, 1, true, false),
    row(0x0424, 0x0424, O, true, true, true, 1, true, false),
    row(0x0424, 0x0424, A, true, true, true, 1, true, false),
    row(0x0428, 0x0428, O, true, false, false, 1, true, false),
    row(0x042c, 0x042c, O, true, false, false, 1, true, false),
    row(0x042c, 0x142c, O, true, false, false, 1, true, true),
    row(0x0438, 0x0438, O, true, false, false, 1, true, false),
    row(0x0438, 0x0438, O, true, true, false, 1, true, false),
    row(0x0438, 0x0438, O, true, true, true, 1, true, false),
    row(0x0438, 0x1438, O, true, false, false, 1, true, true),
    row(0x043c, 0x043c, O, true, false, false, 1, true, false),
    row(0x043c, 0x043c, O, true, false, true, 1, true, false),
    row(0x043c, 0x043c, O, true, true, false, 1, true, false),
    row(0x043c, 0x043c, O, true, true, true, 1, true, false),
    row(0x043c, 0x043c, A, false, true, false, 1, true, false),
    row(0x043c, 0x043c, A, true, false, false, 1, true, false),
    row(0x043c, 0x043c, A, true, false, true, 1, true, false),
    row(0x043c, 0x043c, A, true, true, false, 1, true, false),
    row(0x043c, 0x043c, A, true, true, true, 1, true, false),
    row(0x043c, 0x143c, O, true, false, false, 1, true, true),
    row(0x043c, 0x143c, A, true, false, false, 1, true, true),
];

/// Every key chapter 1 (`level_213_tutotial`) draws at AF's fresh state in
/// the level's scene (fog on but on the sky, which takes none of its
/// start's fog (AG.41); its three point lights' slots (AH.65) unless a
/// material owns its point lights off; the level's shadow (AG.23)
/// unless a material owns it off, as the sky's do), each once: AG.19's 55
/// before AG.23, the sky's 7 unfogged since AG.41 and writing no depth since
/// AG.79.
pub const LEVEL: &[Row] = &[
    row(0x0000, 0x0000, O, false, false, false, 1, true, false),
    row(0x0000, 0x1000, O, false, false, false, 1, true, true),
    row(0x0000, 0x1000, A, false, false, false, 1, true, true),
    row(0x0000, 0x1000, A, false, false, true, 1, true, true),
    row(0x0001, 0x1001, A, false, true, false, 2, true, true),
    row(0x0001, 0x1001, A, false, true, true, 2, true, true),
    row(0x0001, 0x1001, ADD, false, true, false, 2, true, true),
    row(0x3c00, 0x2400, MUL, true, true, false, 1, true, false),
    row(0x3c00, 0x3400, O, true, true, false, 1, true, true),
    row(0x3c00, 0x3400, A, false, true, false, 1, true, true),
    row(0x3c00, 0x3400, A, true, true, false, 1, true, true),
    row(0x3c01, 0x3401, A, true, true, false, 2, true, true),
    row(0x3c01, 0x3401, ADD, false, false, false, 2, true, true),
    row(0x3c02, 0x3402, O, true, true, false, 2, true, true),
    row(0x3c02, 0x3402, O, true, true, true, 2, true, true),
    row(0x3c02, 0x3402, A, false, false, false, 2, true, true),
    row(0x3c02, 0x3402, A, true, true, false, 2, true, true),
    row(0x3c04, 0x3404, O, true, true, false, 1, true, true),
    row(0x3c04, 0x3404, A, true, true, false, 1, true, true),
    row(0x3c18, 0x3418, O, true, false, false, 1, true, true),
    row(0x3c18, 0x3418, O, true, false, true, 1, true, true),
    row(0x3c18, 0x3418, O, true, true, false, 1, true, true),
    row(0x3c18, 0x3418, A, false, false, false, 1, true, true),
    row(0x3c18, 0x3418, A, false, true, false, 1, true, true),
    row(0x3c18, 0x3418, A, true, false, false, 1, true, true),
    row(0x3c18, 0x3418, A, true, false, true, 1, true, true),
    row(0x3c18, 0x3418, A, true, true, false, 1, true, true),
    row(0x3c18, 0x3418, A, true, true, true, 1, true, true),
    row(0x3c19, 0x2419, O, true, true, false, 2, true, false),
    row(0x3c19, 0x3419, O, true, false, false, 2, true, true),
    row(0x3c19, 0x3419, O, true, true, false, 2, true, true),
    row(0x3c1a, 0x341a, O, true, false, false, 2, true, true),
    row(0x3c1a, 0x341a, O, true, true, false, 2, true, true),
    row(0x3c1a, 0x341a, A, false, true, false, 2, true, true),
    row(0x3c1a, 0x341a, A, true, false, false, 2, true, true),
    row(0x3c1a, 0x341a, A, true, true, false, 2, true, true),
    row(0x3c1b, 0x341b, O, true, false, false, 3, true, true),
    row(0x3c1b, 0x341b, O, true, true, false, 3, true, true),
    row(0x3c1c, 0x341c, O, true, false, false, 1, true, true),
    row(0x3c1c, 0x341c, O, true, true, false, 1, true, true),
    row(0x3c1d, 0x341d, O, true, true, false, 2, true, true),
    row(0x3c1e, 0x341e, O, true, true, false, 2, true, true),
    row(0x3c1f, 0x341f, O, true, true, false, 3, true, true),
    row(0x3c38, 0x3438, O, true, true, false, 1, true, true),
    row(0x3c3a, 0x343a, O, true, true, false, 2, true, true),
    row(0x3c3a, 0x343a, A, true, true, false, 2, true, true),
    row(0x3c3b, 0x343b, O, true, true, false, 3, true, true),
    row(0x3c3e, 0x343e, O, true, true, false, 2, true, true),
    row(0x3c3e, 0x343e, A, true, true, false, 2, true, true),
    row(0x3c9c, 0x349c, O, true, false, true, 2, true, true),
    row(0x3d00, 0x3500, O, true, true, false, 1, true, true),
    row(0x3d00, 0x3500, A, true, true, false, 1, true, true),
    row(0x3d01, 0x3501, ADD, false, false, false, 2, true, true),
    row(0x3d02, 0x3502, O, true, true, false, 2, true, true),
    row(0x3d02, 0x3502, A, false, true, false, 2, true, true),
    row(0x3d08, 0x3508, O, true, true, false, 1, true, true),
    row(0x3d0a, 0x350a, O, true, true, false, 2, true, true),
    row(0x3d18, 0x2518, O, true, true, false, 1, true, false),
    row(0x3d18, 0x3518, O, true, true, false, 1, true, true),
    row(0x3d18, 0x3518, A, true, true, false, 1, true, true),
    row(0x3d19, 0x3519, O, true, true, false, 2, true, true),
    row(0x3d1a, 0x351a, O, true, true, false, 2, true, true),
    row(0x3d1a, 0x351a, A, true, false, false, 2, true, true),
    row(0x3d1b, 0x351b, O, true, true, false, 3, true, true),
    row(0x3d1c, 0x351c, O, true, true, false, 1, true, true),
    row(0x3d1e, 0x351e, O, true, false, false, 2, true, true),
    row(0x3d1e, 0x351e, O, true, true, false, 2, true, true),
];

/// Every key chapter 1's props draw (AG.25) that [`LEVEL`] doesn't hold:
/// the models of the placements that can stand there (a template or a
/// creature draws nothing; one without a range stands by its actor since
/// AG.60) and of the placements whose clones draw (AS.63's rule,
/// `props::clones_draw`, opaque ones also blending as a clone fades;
/// Norticus' turban's two since AH.138), in the level's scene at AF's fresh
/// state (the level's shadow on since AG.23), each once.
pub const LEVEL_PROPS: &[Row] = &[
    row(0x0534, 0x1534, O, true, false, false, 1, true, true),
    row(0x0534, 0x1534, A, true, false, false, 1, true, true),
    row(0x3d00, 0x2500, O, true, false, false, 1, true, false),
    row(0x3d00, 0x2500, O, true, false, true, 1, true, false),
    row(0x3d00, 0x2500, A, false, false, false, 1, true, false),
    row(0x3d00, 0x2500, A, false, true, false, 1, true, false),
    row(0x3d00, 0x2500, A, false, true, true, 1, true, false),
    row(0x3d00, 0x2500, A, true, false, false, 1, true, false),
    row(0x3d00, 0x2500, A, true, false, true, 1, true, false),
    row(0x3d00, 0x2500, A, true, true, false, 1, true, false),
    row(0x3d00, 0x2500, ADD, false, true, false, 1, true, false),
    row(0x3d00, 0x3500, O, true, false, false, 1, true, true),
    row(0x3d00, 0x3500, A, false, false, false, 1, true, true),
    row(0x3d00, 0x3500, A, false, true, false, 1, true, true),
    row(0x3d00, 0x3500, A, true, false, false, 1, true, true),
    row(0x3d00, 0x3500, ADD, false, false, false, 1, true, true),
    row(0x3d00, 0x3500, ADD, false, true, false, 1, true, true),
    row(0x3d01, 0x2501, O, true, false, false, 2, true, false),
    row(0x3d01, 0x2501, A, true, false, false, 2, true, false),
    row(0x3d01, 0x3501, O, true, false, false, 2, true, true),
    row(0x3d01, 0x3501, A, false, false, false, 2, true, true),
    row(0x3d01, 0x3501, A, false, true, false, 2, true, true),
    row(0x3d01, 0x3501, A, true, false, false, 2, true, true),
    row(0x3d01, 0x3501, ADD, false, true, false, 2, true, true),
    row(0x3d02, 0x3502, A, true, true, false, 2, true, true),
    row(0x3d04, 0x2504, O, true, true, false, 1, true, false),
    row(0x3d04, 0x2504, A, true, true, false, 1, true, false),
    row(0x3d04, 0x3504, O, true, false, false, 1, true, true),
    row(0x3d04, 0x3504, O, true, true, false, 1, true, true),
    row(0x3d04, 0x3504, A, false, false, false, 1, true, true),
    row(0x3d04, 0x3504, A, true, false, false, 1, true, true),
    row(0x3d04, 0x3504, A, true, true, false, 1, true, true),
    row(0x3d05, 0x3505, A, false, false, false, 2, true, true),
    row(0x3d05, 0x3505, A, false, true, false, 2, true, true),
    row(0x3d05, 0x3505, ADD, false, true, false, 2, true, true),
    row(0x3d06, 0x3506, O, true, false, false, 2, true, true),
    row(0x3d06, 0x3506, A, true, false, false, 2, true, true),
    row(0x3d0a, 0x350a, A, true, true, false, 2, true, true),
    row(0x3d14, 0x3514, O, true, false, false, 1, true, true),
    row(0x3d14, 0x3514, A, true, false, false, 1, true, true),
    row(0x3d18, 0x3518, O, true, false, false, 1, true, true),
    row(0x3d18, 0x3518, A, true, false, false, 1, true, true),
    row(0x3d19, 0x2519, O, true, false, false, 2, true, false),
    row(0x3d19, 0x2519, A, true, false, false, 2, true, false),
    row(0x3d1a, 0x351a, A, true, true, false, 2, true, true),
    row(0x3d1c, 0x351c, O, true, false, false, 1, true, true),
    row(0x3d1c, 0x351c, A, true, false, false, 1, true, true),
    row(0x3d1c, 0x351c, A, true, true, false, 1, true, true),
    row(0x3d1d, 0x251d, O, true, true, false, 2, true, false),
    row(0x3d1d, 0x251d, A, true, true, false, 2, true, false),
    row(0x3d1e, 0x251e, O, true, true, false, 2, true, false),
    row(0x3d1e, 0x251e, A, true, true, false, 2, true, false),
    row(0x3d1e, 0x351e, A, true, true, false, 2, true, true),
    row(0x3d1f, 0x351f, O, true, true, false, 3, true, true),
    row(0x3d1f, 0x351f, A, true, true, false, 3, true, true),
    row(0x3d20, 0x2520, O, true, false, false, 1, true, false),
    row(0x3d20, 0x2520, A, true, false, false, 1, true, false),
    row(0x3d20, 0x3520, O, true, false, false, 1, true, true),
    row(0x3d20, 0x3520, O, true, true, false, 1, true, true),
    row(0x3d20, 0x3520, A, true, false, false, 1, true, true),
    row(0x3d20, 0x3520, A, true, true, false, 1, true, true),
    row(0x3d21, 0x3521, O, true, false, false, 2, true, true),
    row(0x3d21, 0x3521, O, true, true, false, 2, true, true),
    row(0x3d21, 0x3521, A, false, false, false, 2, true, true),
    row(0x3d21, 0x3521, A, true, false, false, 2, true, true),
    row(0x3d21, 0x3521, A, true, true, false, 2, true, true),
    row(0x3d22, 0x3522, O, true, false, false, 2, true, true),
    row(0x3d22, 0x3522, A, true, false, false, 2, true, true),
    row(0x3d23, 0x2523, O, true, false, false, 3, true, false),
    row(0x3d23, 0x2523, A, true, false, false, 3, true, false),
    row(0x3d24, 0x2524, O, true, false, false, 1, true, false),
    row(0x3d24, 0x2524, O, true, true, false, 1, true, false),
    row(0x3d24, 0x2524, A, true, false, false, 1, true, false),
    row(0x3d24, 0x2524, A, true, true, false, 1, true, false),
    row(0x3d24, 0x3524, O, true, false, false, 1, true, true),
    row(0x3d24, 0x3524, O, true, true, false, 1, true, true),
    row(0x3d24, 0x3524, A, true, false, false, 1, true, true),
    row(0x3d24, 0x3524, A, true, true, false, 1, true, true),
    row(0x3d24, 0x3524, A, true, true, true, 1, true, true),
    row(0x3d25, 0x2525, O, true, false, false, 2, true, false),
    row(0x3d25, 0x2525, A, false, false, false, 2, true, false),
    row(0x3d25, 0x2525, A, true, false, false, 2, true, false),
    row(0x3d25, 0x3525, O, true, false, false, 2, true, true),
    row(0x3d25, 0x3525, A, true, false, false, 2, true, true),
    row(0x3d26, 0x3526, O, true, false, false, 2, true, true),
    row(0x3d26, 0x3526, A, true, false, false, 2, true, true),
    row(0x3d28, 0x3528, O, true, false, false, 1, true, true),
    row(0x3d28, 0x3528, O, true, true, false, 1, true, true),
    row(0x3d28, 0x3528, A, false, true, false, 1, true, true),
    row(0x3d28, 0x3528, A, true, false, false, 1, true, true),
    row(0x3d28, 0x3528, A, true, true, false, 1, true, true),
    row(0x3d29, 0x3529, O, true, false, false, 2, true, true),
    row(0x3d29, 0x3529, A, true, false, false, 2, true, true),
    row(0x3d2c, 0x252c, O, true, false, false, 1, true, false),
    row(0x3d2c, 0x252c, A, true, false, false, 1, true, false),
    row(0x3d2c, 0x352c, O, true, false, false, 1, true, true),
    row(0x3d2c, 0x352c, A, true, false, false, 1, true, true),
    row(0x3d2e, 0x352e, O, true, false, false, 2, true, true),
    row(0x3d2e, 0x352e, A, true, false, false, 2, true, true),
    row(0x3d34, 0x3534, O, true, false, false, 1, true, true),
    row(0x3d34, 0x3534, O, true, true, false, 1, true, true),
    row(0x3d34, 0x3534, A, true, false, false, 1, true, true),
    row(0x3d34, 0x3534, A, true, true, false, 1, true, true),
    row(0x3d38, 0x2538, O, true, false, false, 1, true, false),
    row(0x3d38, 0x2538, O, true, true, true, 1, true, false),
    row(0x3d38, 0x2538, A, true, false, false, 1, true, false),
    row(0x3d38, 0x2538, A, true, true, true, 1, true, false),
    row(0x3d38, 0x3538, O, true, false, false, 1, true, true),
    row(0x3d38, 0x3538, A, false, false, false, 1, true, true),
    row(0x3d38, 0x3538, A, true, false, false, 1, true, true),
    row(0x3d39, 0x3539, O, true, false, false, 2, true, true),
    row(0x3d39, 0x3539, A, true, false, false, 2, true, true),
    row(0x3d3a, 0x353a, O, true, false, false, 2, true, true),
    row(0x3d3a, 0x353a, A, true, false, false, 2, true, true),
    row(0x3d3c, 0x253c, O, true, false, false, 1, true, false),
    row(0x3d3c, 0x253c, O, true, true, false, 1, true, false),
    row(0x3d3c, 0x253c, A, true, false, false, 1, true, false),
    row(0x3d3c, 0x253c, A, true, true, false, 1, true, false),
    row(0x3d3c, 0x353c, O, true, false, false, 1, true, true),
    row(0x3d3c, 0x353c, O, true, true, false, 1, true, true),
    row(0x3d3c, 0x353c, A, true, false, false, 1, true, true),
    row(0x3d3c, 0x353c, A, true, true, false, 1, true, true),
    row(0x3d3d, 0x353d, O, true, false, false, 2, true, true),
    row(0x3d3d, 0x353d, A, true, false, false, 2, true, true),
];

/// Every key what the fight's scripts spawn draws (AH.24) that [`LEVEL`]
/// and [`LEVEL_PROPS`] don't hold: the models the extract's spawn part
/// holds for `global.bld` and each fighter's archive (AH.22; Eruptor's ball,
/// its splash and his eruption's blobs among them, and since AH.39
/// `global.bld`'s own: the level-up's, the death stars', the portal
/// entry's, the prompts'), in the level's scene at AF's fresh state (the
/// level's shadow on since AG.23), and each opaque one blending as a clone
/// fading in draws it (AH.67, `props::faded_keys`), each once.
pub const SPAWNS: &[Row] = &[
    row(0x3d00, 0x2500, ADD, false, false, false, 1, true, false),
    row(0x3d00, 0x2500, ADD, true, false, false, 1, true, false),
    row(0x3d00, 0x3500, ADD, false, false, true, 1, true, true),
    row(0x3d04, 0x2504, O, true, false, false, 1, true, false),
    row(0x3d04, 0x2504, A, true, false, false, 1, true, false),
    row(0x3d20, 0x3520, ADD, true, false, true, 1, true, true),
    row(0x3d21, 0x3521, ADD, true, true, true, 2, true, true),
];

/// Every glowing part's level glow mask chapter 1, its props and what its
/// scripts spawn draw (openspec AH.65, [`crate::character::Pass::LevelGlow`]),
/// each once, as the colour key's row: the pass is the table's. Every
/// glowing segment's, not only the first material's on a key: 5 of AH.65's
/// 19 share their key with a part drawn first that doesn't glow (the
/// waterfall's foam with its glimmer, an emerald with a watermelon's rind,
/// the ball's splash with the eruption's lava among them); AG.60's props
/// add one, AH.39's `global.bld` models seven, AS.63's clones' rule eight.
pub const LEVEL_GLOW: &[Row] = &[
    row(0x0000, 0x1000, A, false, false, false, 1, true, true),
    row(0x0000, 0x1000, A, false, false, true, 1, true, true),
    row(0x0001, 0x1001, A, false, true, false, 2, true, true),
    row(0x0001, 0x1001, A, false, true, true, 2, true, true),
    row(0x0001, 0x1001, ADD, false, true, false, 2, true, true),
    row(0x3c00, 0x2400, O, true, false, false, 1, true, false),
    row(0x3c00, 0x2400, A, false, false, false, 1, true, false),
    row(0x3c00, 0x2400, A, false, true, false, 1, true, false),
    row(0x3c00, 0x2400, A, true, false, false, 1, true, false),
    row(0x3c00, 0x2400, ADD, false, false, false, 1, true, false),
    row(0x3c00, 0x2400, ADD, false, true, false, 1, true, false),
    row(0x3c00, 0x3400, O, true, false, false, 1, true, true),
    row(0x3c00, 0x3400, A, false, false, false, 1, true, true),
    row(0x3c00, 0x3400, A, false, true, false, 1, true, true),
    row(0x3c00, 0x3400, A, true, true, false, 1, true, true),
    row(0x3c00, 0x3400, ADD, false, false, false, 1, true, true),
    row(0x3c00, 0x3400, ADD, false, false, true, 1, true, true),
    row(0x3c00, 0x3400, ADD, false, true, false, 1, true, true),
    row(0x3c01, 0x3401, A, false, false, false, 2, true, true),
    row(0x3c01, 0x3401, ADD, false, false, false, 2, true, true),
    row(0x3c01, 0x3401, ADD, false, true, false, 2, true, true),
    row(0x3c04, 0x3404, O, true, true, false, 1, true, true),
    row(0x3c04, 0x3404, A, false, false, false, 1, true, true),
    row(0x3c04, 0x3404, A, true, true, false, 1, true, true),
    row(0x3c05, 0x3405, A, false, true, false, 2, true, true),
    row(0x3c05, 0x3405, ADD, false, true, false, 2, true, true),
    row(0x3c1d, 0x241d, O, true, true, false, 2, true, false),
    row(0x3c20, 0x3420, ADD, true, false, true, 1, true, true),
    row(0x3c21, 0x3421, ADD, true, true, true, 2, true, true),
    row(0x3c24, 0x2424, O, true, false, false, 1, true, false),
    row(0x3c24, 0x3424, O, true, false, false, 1, true, true),
    row(0x3c24, 0x3424, A, true, true, false, 1, true, true),
    row(0x3c25, 0x2425, A, false, false, false, 2, true, false),
    row(0x3c39, 0x3439, O, true, false, false, 2, true, true),
    row(0x3c3c, 0x243c, O, true, false, false, 1, true, false),
    row(0x3c3c, 0x343c, O, true, false, false, 1, true, true),
    row(0x3c3c, 0x343c, A, true, false, false, 1, true, true),
];

/// Every key a character draws as the hero in the level on a skinned mesh
/// (openspec AH.65: the level's scene, its three point lights on) that the
/// cards' table doesn't hold, each once: every one of the roster's, since
/// any comes in as a replacement (AG.27; the arrival lane's Mac run of
/// 2026-10-31 compiled Tree Rex's three in play, under his arrival).
pub const HEROES: &[Row] = &[
    row(0x3c00, 0x2400, O, true, false, false, 1, true, false),
    row(0x3c04, 0x2404, O, true, false, false, 1, true, false),
    row(0x3c04, 0x2404, ADD, false, false, false, 1, true, false),
    row(0x3c04, 0x3404, O, true, false, false, 1, true, true),
    row(0x3c04, 0x3404, A, false, false, false, 1, true, true),
    row(0x3c04, 0x3404, A, false, true, false, 1, true, true),
    row(0x3c18, 0x2418, O, true, false, false, 1, true, false),
    row(0x3c1c, 0x241c, O, true, false, false, 1, true, false),
    row(0x3c1c, 0x241c, O, true, false, true, 1, true, false),
    row(0x3c1c, 0x341c, O, true, false, false, 1, true, true),
    row(0x3c20, 0x2420, O, true, false, false, 1, true, false),
    row(0x3c24, 0x2424, O, true, false, false, 1, true, false),
    row(0x3c24, 0x2424, O, true, false, true, 1, true, false),
    row(0x3c24, 0x2424, O, true, true, true, 1, true, false),
    row(0x3c24, 0x2424, A, true, true, true, 1, true, false),
    row(0x3c28, 0x2428, O, true, false, false, 1, true, false),
    row(0x3c2c, 0x242c, O, true, false, false, 1, true, false),
    row(0x3c2c, 0x342c, O, true, false, false, 1, true, true),
    row(0x3c38, 0x2438, O, true, false, false, 1, true, false),
    row(0x3c38, 0x2438, O, true, true, false, 1, true, false),
    row(0x3c38, 0x2438, O, true, true, true, 1, true, false),
    row(0x3c38, 0x3438, O, true, false, false, 1, true, true),
    row(0x3c3c, 0x243c, O, true, false, false, 1, true, false),
    row(0x3c3c, 0x243c, O, true, false, true, 1, true, false),
    row(0x3c3c, 0x243c, O, true, true, false, 1, true, false),
    row(0x3c3c, 0x243c, O, true, true, true, 1, true, false),
    row(0x3c3c, 0x243c, A, false, true, false, 1, true, false),
    row(0x3c3c, 0x243c, A, true, false, false, 1, true, false),
    row(0x3c3c, 0x243c, A, true, false, true, 1, true, false),
    row(0x3c3c, 0x243c, A, true, true, false, 1, true, false),
    row(0x3c3c, 0x243c, A, true, true, true, 1, true, false),
    row(0x3c3c, 0x343c, O, true, false, false, 1, true, true),
    row(0x3c3c, 0x343c, A, true, false, false, 1, true, true),
];

/// Every key a character draws as the hero in the level on a static mesh
/// (what he carries) that the level's and the cards' tables don't hold.
pub const HEROES_STATIC: &[Row] = &[
    row(0x3c00, 0x2400, O, true, false, false, 1, true, false),
    row(0x3c00, 0x2400, A, false, true, false, 1, true, false),
    row(0x3c04, 0x2404, O, true, false, false, 1, true, false),
    row(0x3c04, 0x2404, A, false, false, false, 1, true, false),
    row(0x3c04, 0x2404, A, true, false, false, 1, true, false),
    row(0x3c04, 0x3404, O, true, false, false, 1, true, true),
    row(0x3c04, 0x3404, A, false, false, false, 1, true, true),
    row(0x3c04, 0x3404, A, true, false, false, 1, true, true),
    row(0x3c08, 0x2408, O, true, false, false, 1, true, false),
    row(0x3c0c, 0x240c, O, true, false, false, 1, true, false),
    row(0x3c0c, 0x240c, O, true, true, false, 1, true, false),
    row(0x3c1c, 0x241c, O, true, false, false, 1, true, false),
    row(0x3c20, 0x2420, O, true, false, false, 1, true, false),
    row(0x3c24, 0x2424, O, true, false, false, 1, true, false),
    row(0x3c24, 0x2424, A, true, true, false, 1, true, false),
    row(0x3c24, 0x2424, ADD, true, false, false, 1, true, false),
    row(0x3c24, 0x3424, A, true, false, false, 1, true, true),
    row(0x3c2c, 0x242c, O, true, false, false, 1, true, false),
    row(0x3c38, 0x2438, O, true, false, false, 1, true, false),
    row(0x3c3c, 0x243c, O, true, false, false, 1, true, false),
    row(0x3c3c, 0x243c, O, true, true, false, 1, true, false),
    row(0x3c3c, 0x343c, O, true, false, false, 1, true, true),
    row(0x3c3c, 0x343c, A, true, false, false, 1, true, true),
];

/// His glowing parts' level glow masks on skinned meshes, as [`LEVEL_GLOW`]'s.
pub const HEROES_GLOW: &[Row] = &[
    row(0x0404, 0x0404, O, true, false, false, 1, true, false),
    row(0x041c, 0x041c, O, true, false, false, 1, true, false),
    row(0x043c, 0x043c, O, true, false, false, 1, true, false),
    row(0x3c00, 0x2400, O, true, false, false, 1, true, false),
    row(0x3c04, 0x2404, O, true, false, false, 1, true, false),
    row(0x3c04, 0x3404, A, false, false, false, 1, true, true),
    row(0x3c04, 0x3404, A, false, true, false, 1, true, true),
    row(0x3c1c, 0x241c, O, true, false, false, 1, true, false),
    row(0x3c1c, 0x241c, O, true, false, true, 1, true, false),
    row(0x3c20, 0x2420, O, true, false, false, 1, true, false),
    row(0x3c24, 0x2424, O, true, false, false, 1, true, false),
    row(0x3c2c, 0x242c, O, true, false, false, 1, true, false),
    row(0x3c2c, 0x342c, O, true, false, false, 1, true, true),
    row(0x3c3c, 0x243c, O, true, false, false, 1, true, false),
    row(0x3c3c, 0x243c, A, false, true, false, 1, true, false),
    row(0x3c3c, 0x343c, O, true, false, false, 1, true, true),
    row(0x3c3c, 0x343c, A, true, false, false, 1, true, true),
];

/// His glowing parts' level glow masks on static meshes past
/// [`LEVEL_GLOW`]'s.
pub const HEROES_GLOW_STATIC: &[Row] = &[
    row(0x041c, 0x041c, O, true, false, false, 1, true, false),
    row(0x3c04, 0x2404, O, true, false, false, 1, true, false),
    row(0x3c04, 0x2404, A, false, false, false, 1, true, false),
    row(0x3c04, 0x2404, A, true, false, false, 1, true, false),
    row(0x3c04, 0x3404, O, true, false, false, 1, true, true),
    row(0x3c04, 0x3404, A, true, false, false, 1, true, true),
    row(0x3c08, 0x2408, O, true, false, false, 1, true, false),
    row(0x3c24, 0x3424, A, true, false, false, 1, true, true),
];

/// Every key the first fight's creatures draw in the level on a skinned
/// mesh that [`HEROES`] and [`TURNTABLE`] don't hold (openspec AH.25: the four Chompies'
/// and Norticus' models; AG.150 and AS.63: every skinned template's whose
/// clones draw, the push blocks' tortoises' and the cannon top's among
/// them, `crate::creature_models`, in the walking hero's scene),
/// and each opaque one blending as a fading creature draws it (AH.139),
/// each once.
pub const CREATURES: &[Row] = &[
    row(0x3c00, 0x2400, O, true, true, true, 1, true, false),
    row(0x3c00, 0x2400, A, false, true, true, 1, true, false),
    row(0x3c00, 0x2400, A, true, false, false, 1, true, false),
    row(0x3c00, 0x2400, A, true, true, true, 1, true, false),
    row(0x3c00, 0x2400, ADD, false, false, false, 1, true, false),
    row(0x3c04, 0x2404, A, true, false, false, 1, true, false),
    row(0x3c18, 0x3418, O, true, true, false, 1, true, true),
    row(0x3c18, 0x3418, A, true, true, false, 1, true, true),
    row(0x3c1c, 0x241c, O, true, true, false, 1, true, false),
    row(0x3c1c, 0x241c, A, true, true, false, 1, true, false),
    row(0x3c1e, 0x341e, O, true, false, false, 2, true, true),
    row(0x3c1e, 0x341e, A, true, false, false, 2, true, true),
    row(0x3c20, 0x3420, O, true, true, false, 1, true, true),
    row(0x3c20, 0x3420, A, true, true, false, 1, true, true),
    row(0x3c23, 0x3423, ADD, true, false, false, 3, true, true),
    row(0x3c24, 0x2424, A, true, false, false, 1, true, false),
    row(0x3c2c, 0x242c, A, true, false, false, 1, true, false),
    row(0x3c38, 0x2438, A, true, false, false, 1, true, false),
    row(0x3c38, 0x3438, A, true, false, false, 1, true, true),
    row(0x3c3c, 0x343c, O, true, true, false, 1, true, true),
    row(0x3c3c, 0x343c, A, true, true, false, 1, true, true),
];

/// Their glowing parts' level glow masks past [`HEROES_GLOW`]'s.
pub const CREATURES_GLOW: &[Row] = &[row(0x3c23, 0x3423, ADD, true, false, false, 3, true, true)];

/// Every key the particles of what the fight's scripts spawn draw (openspec
/// AH.87) that [`LEVEL`], [`LEVEL_PROPS`] and [`SPAWNS`] don't hold: the
/// extract's particle part's generators (`global.bld`'s and each fighter's
/// emitters), each on its material (unlit, its fog and point lights owned
/// off, writing no depth, one-sided quads wound to the camera) in the
/// level's scene, each once. A blending generator's key is the sky's
/// (`LEVEL`'s), an adding one's this.
pub const PARTICLES: &[Row] = &[row(0x0000, 0x1000, ADD, false, false, false, 1, true, true)];

/// Every key chapter 1's grass draws (openspec AG.24): tfbgrass's shells on
/// their stand-in (stand-in ledger: `grass-warp`), in the level's scene,
/// in the grass's pass
/// ([`Pass::Grass`]), each once.
pub const GRASS: &[Row] = &[row(0x3d00, 0x3500, O, true, true, true, 1, false, true)];

/// Every key a warp draws (openspec AG.24): chapter 1's water's and the
/// level-up's ring's (`global.bld`'s), tfbwarp's in the warp's pass
/// ([`Pass::Warp`]), each once; stand-in ledger: `grass-warp`.
pub const WARP: &[Row] = &[
    row(0x0000, 0x0000, O, false, false, false, 1, false, false),
    row(0x0000, 0x0000, O, false, false, false, 1, false, true),
    row(0x0000, 0x0000, O, false, true, false, 1, false, false),
    row(0x0000, 0x0000, O, false, true, false, 1, false, true),
];

/// Their glowing generators' level glow masks (a bloom on the disc) past
/// [`LEVEL_GLOW`]'s.
pub const PARTICLES_GLOW: &[Row] = &[row(0x0000, 0x1000, ADD, false, false, false, 1, true, true)];

/// Every key the 01.00 roster's figures draw on a card on a static mesh
/// (the turntable's scene), each once: the unskinned parts they carry
/// fresh (AE.23), each drawn under its bone or its parent part.
pub const TURNTABLE_STATIC: &[Row] = &[
    row(0x0400, 0x0400, O, true, false, false, 1, true, false),
    row(0x0400, 0x0400, A, false, true, false, 1, true, false),
    row(0x0404, 0x0404, O, true, false, false, 1, true, false),
    row(0x0404, 0x0404, A, false, false, false, 1, true, false),
    row(0x0404, 0x0404, A, true, false, false, 1, true, false),
    row(0x0404, 0x1404, O, true, false, false, 1, true, true),
    row(0x0404, 0x1404, A, false, false, false, 1, true, true),
    row(0x0404, 0x1404, A, true, false, false, 1, true, true),
    row(0x0404, 0x1404, A, true, true, false, 1, true, true),
    row(0x0408, 0x0408, O, true, false, false, 1, true, false),
    row(0x040c, 0x040c, O, true, false, false, 1, true, false),
    row(0x040c, 0x040c, O, true, true, false, 1, true, false),
    row(0x041c, 0x041c, O, true, false, false, 1, true, false),
    row(0x0420, 0x0420, O, true, false, false, 1, true, false),
    row(0x0424, 0x0424, O, true, false, false, 1, true, false),
    row(0x0424, 0x0424, A, true, true, false, 1, true, false),
    row(0x0424, 0x0424, ADD, true, false, false, 1, true, false),
    row(0x0424, 0x1424, A, true, false, false, 1, true, true),
    row(0x042c, 0x042c, O, true, false, false, 1, true, false),
    row(0x0438, 0x0438, O, true, false, false, 1, true, false),
    row(0x043c, 0x043c, O, true, false, false, 1, true, false),
    row(0x043c, 0x043c, O, true, true, false, 1, true, false),
    row(0x043c, 0x143c, O, true, false, false, 1, true, true),
    row(0x043c, 0x143c, A, true, false, false, 1, true, true),
];

/// Every key a card draws: [`TURNTABLE`]'s on skinned meshes, then
/// [`TURNTABLE_STATIC`]'s on static ones.
pub fn turntable_keys() -> Vec<WarmKey> {
    let mut out = keys(TURNTABLE, true);
    out.extend(keys(TURNTABLE_STATIC, false));
    out
}

/// [`LEVEL`]'s keys, then [`LEVEL_PROPS`]' and [`SPAWNS`]': a level's
/// segments, its props' and what its scripts spawn are static; then their
/// glowing parts' level glow masks ([`LEVEL_GLOW`]), the heroes' keys in
/// the level ([`HEROES`], [`HEROES_STATIC`]) and theirs ([`HEROES_GLOW`],
/// [`HEROES_GLOW_STATIC`]), openspec AH.65; then the first fight's
/// creatures' ([`CREATURES`], [`CREATURES_GLOW`]), AH.25; then the
/// particles' ([`PARTICLES`], [`PARTICLES_GLOW`]), AH.87; then the grass's
/// and the warps' ([`GRASS`], [`WARP`]), AG.24.
pub fn level_keys() -> Vec<WarmKey> {
    let mut out = keys(LEVEL, false);
    out.extend(keys(LEVEL_PROPS, false));
    out.extend(keys(SPAWNS, false));
    out.extend(glow_keys(LEVEL_GLOW, false));
    out.extend(keys(HEROES, true));
    out.extend(keys(HEROES_STATIC, false));
    out.extend(glow_keys(HEROES_GLOW, true));
    out.extend(glow_keys(HEROES_GLOW_STATIC, false));
    out.extend(keys(CREATURES, true));
    out.extend(glow_keys(CREATURES_GLOW, true));
    out.extend(keys(PARTICLES, false));
    out.extend(glow_keys(PARTICLES_GLOW, false));
    out.extend(pass_keys(GRASS, Pass::Grass));
    out.extend(pass_keys(WARP, Pass::Warp));
    out
}

/// `rows`' keys on static meshes in `pass` (AG.24's grass and warps).
fn pass_keys(rows: &[Row], pass: Pass) -> Vec<WarmKey> {
    keys(rows, false)
        .into_iter()
        .map(|mut k| {
            k.material.pass = pass;
            k
        })
        .collect()
}

fn keys(rows: &[Row], skinned: bool) -> Vec<WarmKey> {
    rows.iter()
        // A row that isn't a key is a typo `cargo test` catches.
        .filter_map(|r| r.key(skinned).ok())
        .collect()
}

/// `rows`' keys in a level's glow pass.
fn glow_keys(rows: &[Row], skinned: bool) -> Vec<WarmKey> {
    keys(rows, skinned)
        .into_iter()
        .map(|mut k| {
            k.material.pass = Pass::LevelGlow;
            k
        })
        .collect()
}

/// Bits the scene could turn on that a material owns once its ignore mask
/// holds them: every one of the 14, so the masks are drawn as given in any
/// scene.
const OWN_EVERYTHING: u32 = (1 << 14) - 1;

/// An IR material that draws `key`'s material key in any scene, through
/// [`crate::character::CharacterMaterial::from_ir`] as a model's or a
/// level's does: the masks owned whole, every texture role filled, every
/// layer's values set, a glow amount (so its glow pass draws too) and the
/// key's states. What the warm draws a census key with.
pub fn material_for(key: &WarmKey) -> model::Material {
    let s = &key.material.states;
    let mask = |base| Mask {
        base,
        ignore: OWN_EVERYTHING,
    };
    let slot = |role| TextureSlot {
        role,
        texture: "census".into(),
        image: "census".into(),
        sampler: Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: None,
        },
    };
    model::Material {
        name: format!(
            "census {:#06x}/{:#06x}",
            key.material.shader.pixel(),
            key.material.shader.vertex()
        ),
        pixel: mask(key.material.shader.pixel()),
        vertex: mask(key.material.shader.vertex()),
        textures: [
            TextureRole::Layer1ColorMap,
            TextureRole::Layer1BaseNormalMap,
            TextureRole::Layer1BaseSpecMap,
            TextureRole::Layer2ColorMap,
            TextureRole::Layer2BaseNormalMap,
            TextureRole::Layer2BaseSpecMap,
            TextureRole::Layer3ColorMap,
            TextureRole::Layer4ColorMap,
        ]
        .into_iter()
        .map(slot)
        .collect(),
        params: Params {
            fixed_alpha: [-1.0, -1.0, 1.0, 1.0],
            layer1_extra: [1.0, 1.0, 1.0, 0.25],
            layer2_extra: Some([1.0, 1.0, 1.0, 0.25]),
            layer2_blend: Some([1.0, 0.0, 0.0, 0.0]),
            layer3_blend: Some([0.0, 0.0, 1.0, 0.0]),
            layer4_blend: [0.0, 1.0, 0.0, 0.0],
            layer4_values: [0.0, 1.0, 0.0, 0.0],
            material_tint: [128, 128, 128, 255],
            glow: Glow::Amount(255),
        },
        states: RenderStates {
            blend: s.blend,
            depth_write: s.depth_write,
            two_sided: s.two_sided,
            alpha_test: s.alpha_test.then_some(0.5),
        },
        scrolls: Vec::new(),
        tracks: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::CharacterMaterial;
    use crate::character::mesh::{segment_mesh, static_mesh};
    use bevy::asset::Handle;
    use guac_ir::shading::{PointLight, Scene};
    use std::collections::HashSet;

    /// Every scene a material could be drawn in: the masks are owned whole,
    /// so none of them moves the key.
    fn scenes() -> Vec<Scene> {
        let mut out = Vec::new();
        for fog in [false, true] {
            for shadows in [false, true] {
                for lights in 0..=3 {
                    out.push(Scene {
                        fog,
                        shadows,
                        point_lights: vec![
                            PointLight {
                                position: [0.0, 0.0, 0.0],
                                colour: [1.0; 3],
                                linear: 1.0,
                                quadratic: 0.0,
                            };
                            lights
                        ],
                        ..Scene::new()
                    });
                }
            }
        }
        out
    }

    #[test]
    fn every_row_is_a_key_and_none_twice() {
        let mut every = HashSet::new();
        for (rows, skinned, n) in [
            (TURNTABLE, true, 33),
            (TURNTABLE_STATIC, false, 24),
            (LEVEL, false, 67),
            (LEVEL_PROPS, false, 124),
            (SPAWNS, false, 7),
            (HEROES, true, 33),
            (HEROES_STATIC, false, 23),
            (CREATURES, true, 21),
            (PARTICLES, false, 1),
        ] {
            assert_eq!(rows.len(), n);
            let keys: Vec<WarmKey> = rows
                .iter()
                .map(|r| r.key(skinned).unwrap_or_else(|e| panic!("{r:?}: {e}")))
                .collect();
            let unique: HashSet<WarmKey> = keys.iter().copied().collect();
            assert_eq!(unique.len(), n, "a row twice");
            for (r, k) in rows.iter().zip(&keys) {
                assert_eq!(Row::of(k), *r);
                assert_eq!(k.material.pass, Pass::Colour);
            }
            every.extend(unique);
        }
        // The level glow masks' (AH.65): rows of the colour keys whose
        // parts glow, keyed in the level's glow pass.
        for (rows, skinned, n) in [
            (LEVEL_GLOW, false, 37),
            (HEROES_GLOW, true, 17),
            (HEROES_GLOW_STATIC, false, 8),
            (CREATURES_GLOW, true, 1),
            (PARTICLES_GLOW, false, 1),
        ] {
            assert_eq!(rows.len(), n);
            let keys = glow_keys(rows, skinned);
            let unique: HashSet<WarmKey> = keys.iter().copied().collect();
            assert_eq!(unique.len(), n, "a row twice");
            for (r, k) in rows.iter().zip(&keys) {
                assert_eq!(Row::of(k), *r);
                assert_eq!(k.material.pass, Pass::LevelGlow);
            }
            every.extend(unique);
        }
        // The grass's and the warps' (AG.24), in their passes.
        for (rows, pass, n) in [(GRASS, Pass::Grass, 1), (WARP, Pass::Warp, 4)] {
            assert_eq!(rows.len(), n);
            let keys = pass_keys(rows, pass);
            let unique: HashSet<WarmKey> = keys.iter().copied().collect();
            assert_eq!(unique.len(), n, "a row twice");
            for (r, k) in rows.iter().zip(&keys) {
                assert_eq!(Row::of(k), *r);
                assert_eq!(k.material.pass, pass);
                assert!(!k.mesh.skinned);
            }
            every.extend(unique);
        }
        // No key in two tables: the warm draws a key once, a card's with
        // its glow pass and a level's without.
        assert_eq!(
            every.len(),
            33 + 24 + 67 + 124 + 7 + 33 + 23 + 37 + 17 + 8 + 21 + 1 + 1 + 1 + 1 + 4
        );
        assert_eq!(
            turntable_keys().len(),
            TURNTABLE.len() + TURNTABLE_STATIC.len()
        );
        assert_eq!(
            level_keys().len(),
            LEVEL.len()
                + LEVEL_PROPS.len()
                + SPAWNS.len()
                + LEVEL_GLOW.len()
                + HEROES.len()
                + HEROES_STATIC.len()
                + HEROES_GLOW.len()
                + HEROES_GLOW_STATIC.len()
                + CREATURES.len()
                + CREATURES_GLOW.len()
                + PARTICLES.len()
                + PARTICLES_GLOW.len()
                + GRASS.len()
                + WARP.len()
        );
    }

    /// The tables as the scenes they were counted in draw them: the cards'
    /// keys (a body's and what it carries) carry the turntable's fog and
    /// nothing else, the level's its fog (but the sky's, which own it off
    /// and their point lights with it) and its three point lights' slots
    /// (AH.65), the hero's in the level too; the level's shadow (AG.23)
    /// only on the colour keys of the level, its props and its spawns, never
    /// on a mask, a hero's, a creature's or a particle's.
    #[test]
    fn each_table_is_its_scenes() {
        let cards = turntable_keys();
        for k in &cards {
            let s = k.material.shader.switches();
            assert!(s.fog && !s.shadows && s.point_lights == 0, "{k:?}");
            assert_eq!(
                (k.mesh.uv_sets, k.mesh.tangents),
                (1, true),
                "a character's or a carried part's segment: one set and a tangent"
            );
        }
        let static_parts = cards.iter().filter(|k| !k.mesh.skinned).count();
        assert_eq!(static_parts, TURNTABLE_STATIC.len());
        // A level material that owns its point lights draws none; the
        // particles own both off (AH.87).
        let mut sky = 0;
        let particles = keys(PARTICLES, false);
        let receiving: HashSet<WarmKey> = keys(LEVEL, false)
            .into_iter()
            .chain(keys(LEVEL_PROPS, false))
            .chain(keys(SPAWNS, false))
            .chain(pass_keys(GRASS, Pass::Grass))
            .collect();
        let mut shadowed = 0;
        for k in level_keys() {
            let s = k.material.shader.switches();
            assert!(matches!(s.point_lights, 0 | 3), "{k:?}");
            if s.shadows {
                assert!(receiving.contains(&k), "only a level part receives: {k:?}");
                shadowed += 1;
            }
            if !s.fog {
                assert_eq!(
                    s.point_lights, 0,
                    "an unfogged key is the sky's or a particle's: {k:?}"
                );
                if k.material.pass == Pass::Colour && !particles.contains(&k) {
                    sky += 1;
                }
            }
        }
        assert_eq!(sky, 7);
        assert!(shadowed > 0, "the level's ground receives the shadow");
        for k in particles {
            let s = k.material.shader.switches();
            assert!(!s.fog && s.point_lights == 0 && s.vertex_colour, "{k:?}");
            assert!(!s.directional, "unlit: {k:?}");
        }
    }

    /// The warm draws a census key through the constructor the turntable and
    /// the level build their materials with: in every scene it builds the
    /// key the row names, its glow pass too, on a triangle of the row's shape.
    #[test]
    fn every_census_key_builds_through_the_turntables_constructor() {
        let scenes = scenes();
        for key in turntable_keys().into_iter().chain(level_keys()) {
            let ir = material_for(&key);
            for scene in &scenes {
                let m = CharacterMaterial::from_ir(&ir, scene, |_| Handle::default())
                    .unwrap_or_else(|e| panic!("{key:?}: {e}"));
                let m = match key.material.pass {
                    Pass::LevelGlow => m.level_glow().expect("its level glow draws"),
                    // AG.24: the warm draws them in their pass.
                    pass @ (Pass::Grass | Pass::Warp) => CharacterMaterial {
                        key: MaterialKey { pass, ..m.key },
                        ..m
                    },
                    _ => m,
                };
                assert_eq!(m.key, key.material, "{scene:?}");
                let glow = m.glow_mask().expect("its glow pass draws");
                assert_eq!(glow.key.pass, Pass::GlowMask);
            }
            let triangle = key.mesh.triangle(0);
            assert_eq!(MeshShape::of(&triangle), key.mesh);
            let mesh = if key.mesh.skinned {
                segment_mesh(&triangle)
            } else {
                static_mesh(&triangle)
            };
            mesh.unwrap_or_else(|e| panic!("{key:?}: {e}"));
        }
    }

    #[test]
    fn a_row_reads_back_as_written() {
        let r = row(0x41C, 0x41C, O, true, false, true, 1, true, false);
        assert_eq!(
            r.source(),
            "row(0x041c, 0x041c, O, true, false, true, 1, true, false),"
        );
        let k = r.key(true).unwrap();
        assert_eq!(k.material.states.blend, Blend::Opaque);
        assert!(k.material.states.alpha_test);
        for (code, blend) in [
            (A, Blend::Alpha),
            (ADD, Blend::Additive),
            (MUL, Blend::Multiply),
        ] {
            let k = Row { blend: code, ..r }.key(false).unwrap();
            assert_eq!(k.material.states.blend, blend);
            assert!(!k.mesh.skinned);
        }
        // Bit 6 isn't a key the shader draws.
        assert!(
            row(0x40, 0x40, O, true, false, false, 1, true, false)
                .key(true)
                .is_err()
        );
    }
}
