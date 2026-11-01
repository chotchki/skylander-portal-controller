//! The frame's layout (the crate docs, Layout; openspec AT.7): the
//! updater's walk over `active` and each sprite's place, scale, size,
//! priority, depth and colour on screen.

use crate::model::{Model, World};
use crate::sprite::{SpriteId, kind, props};
use crate::text::{GlyphQuad, TextOut};

/// A sprite's place this frame: what the draw list draws it from and what
/// its children hang from.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// Its top-left on screen, once its own anchor is off: x, depth, y, in
    /// frame pixels (a parentless sprite's units are frame pixels).
    pub point: [f32; 3],
    /// Its computed scale: x, depth, y.
    pub scale: [f32; 3],
    /// Its computed width and height, in its own units.
    pub size: [f32; 2],
    pub priority: f32,
    pub depth: f32,
    /// Its colour, the alpha times its parent's over 255.
    pub color: [u8; 4],
    pub replace: [u8; 4],
    /// Drawn this frame: visible, and its parent drawn.
    pub drawn: bool,
    /// Dirty this frame: its own bit or its parent's.
    pub dirty: bool,
    /// A text's glyphs (font pixels from its top-left) and its layout.
    pub text: Option<(Vec<GlyphQuad>, TextOut)>,
}

impl Placed {
    /// Its rectangle on screen: left, top, right, bottom in frame pixels.
    pub fn rect(&self) -> [f32; 4] {
        let [x, _, y] = self.point;
        [
            x,
            y,
            x + self.size[0] * self.scale[0],
            y + self.size[1] * self.scale[2],
        ]
    }
}

/// A frame's layout: each sprite visited this frame, by its id, and the
/// order the walk visited them in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    placed: Vec<Option<Placed>>,
    pub walk: Vec<SpriteId>,
}

impl Frame {
    /// The sprite's layout this frame, if it was laid out (drawn or dirty).
    pub fn get(&self, id: SpriteId) -> Option<&Placed> {
        self.placed.get(id.0 as usize).and_then(Option::as_ref)
    }
}

/// An anchor's column and row (0-8 from the top-left, row by row).
fn anchor(value: u32) -> (f32, f32) {
    ((value % 3) as f32, (value / 3) as f32)
}

/// Whether a kind keeps its own anchors off (a model or particles).
fn unanchored(k: u32) -> bool {
    k == kind::MODEL || k == kind::PARTICLES
}

impl Model {
    /// One frame of the sprite updater (0xBCB0C): `active` walked in order,
    /// a sprite whose parent isn't visited yet moved to the end, each laid
    /// out when drawn or dirty (0xBBD04, 0xBA240). A sprite not *sized*
    /// stores its computed size; a drawn one's *dirty* clears; the drawn
    /// non-containers join the frame's sprite list (the draw order's), the
    /// rest leave it.
    pub fn lay_out(&mut self, world: &dyn World) -> Frame {
        let n = self.len();
        let mut frame = Frame {
            placed: vec![None; n],
            walk: Vec::new(),
        };
        let mut visited = vec![false; n];
        let mut drawn = vec![false; n];
        let mut dirty = vec![false; n];
        let mut i = 0;
        // The model's guard, not the original's: a sprite whose parent never
        // comes up stops the walk once every one left has been put back.
        let mut put_back = 0;
        while i < self.active.len() {
            let id = self.active[i];
            if let Some(p) = self.sprite(id).parent
                && !visited[p.0 as usize]
            {
                let moved = self.active.remove(i);
                self.active.push(moved);
                put_back += 1;
                if put_back > self.active.len() - i {
                    break;
                }
                continue;
            }
            put_back = 0;
            i += 1;
            visited[id.0 as usize] = true;
            frame.walk.push(id);
            let s = self.sprite(id);
            let parent = s.parent;
            let shown =
                s.properties & props::VISIBLE != 0 && parent.is_none_or(|p| drawn[p.0 as usize]);
            let changed =
                s.properties & props::DIRTY != 0 || parent.is_some_and(|p| dirty[p.0 as usize]);
            drawn[id.0 as usize] = shown;
            dirty[id.0 as usize] = changed;
            if !shown {
                self.sprite_mut(id).properties &= !props::DIRTY;
            }
            if shown || changed {
                let placed = self.place(id, parent.and_then(|p| frame.get(p)), world);
                let s = self.sprite_mut(id);
                if !s.sized() {
                    s.size[0] = placed.size[0];
                    s.size[1] = placed.size[1];
                }
                // Its current matrix's place and its kind's scale (0xBA240).
                s.matrix.position = placed.point;
                s.computed_scale = Some(placed.scale);
                frame.placed[id.0 as usize] = Some(Placed {
                    drawn: shown,
                    dirty: changed,
                    ..placed
                });
            }
            let draws = shown && self.sprite(id).kind() != kind::CONTAINER;
            if draws {
                self.sprite_mut(id).properties &= !props::DIRTY;
                if !self.drawing.contains(&id) {
                    self.drawing.push(id);
                }
            } else {
                if self.sprite(id).kind() == kind::CONTAINER {
                    self.sprite_mut(id).properties &= !props::DIRTY;
                }
                self.drawing.retain(|&d| d != id);
            }
        }
        // Whatever left `active` (removed, asleep) leaves the list too.
        let active = &self.active;
        self.drawing.retain(|d| active.contains(d));
        frame
    }

    /// The root a sprite with no parent sprite hangs on (the crate docs,
    /// Layout): a container the frame's size at 100 %, its place (0, 0) at
    /// depth 10,000 (20,000 × ½: the one sprite with no parent at all).
    fn root(&self) -> Placed {
        Placed {
            point: [0.0, 20_000.0 * 0.5, 0.0],
            scale: [1.0; 3],
            size: self.frame,
            priority: 0.0,
            depth: 0.0,
            color: [0x80, 0x80, 0x80, 0xFF],
            replace: [0x80, 0x80, 0x80, 0],
            drawn: true,
            dirty: false,
            text: None,
        }
    }

    /// One sprite's layout (0xBA240) under its parent's, or the root's.
    fn place(&self, id: SpriteId, parent: Option<&Placed>, world: &dyn World) -> Placed {
        let root;
        let p = match parent {
            Some(p) => p,
            None => {
                root = self.root();
                &root
            }
        };
        let s = self.sprite(id);
        let k = s.kind();
        let z = if matches!(k, kind::MODEL | kind::CONTAINER | kind::PARTICLES) {
            s.scale[2] * 0.01
        } else {
            1.0
        };
        let own = [s.scale[0] * 0.01, z, s.scale[1] * 0.01];
        let mut point = [
            f32::from(s.location[0]) * 0.0625,
            s.z_depth * 25.0,
            f32::from(s.location[1]) * 0.0625,
        ];
        let scale = [
            own[0] * p.scale[0],
            own[1] * p.scale[1],
            own[2] * p.scale[2],
        ];
        let (priority, depth) = (s.priority + p.priority, s.z_depth + p.depth);
        let alpha = f32::from(p.color[3]);
        let mut color = s.color;
        color[3] = (f32::from(s.color[3]) * (alpha / 255.0)) as u8;
        let text = self.glyph_layout(id, world);
        let size = match &text {
            Some((_, out)) => out.size,
            None => {
                let [w, h, _] = self.size_of(id, world);
                [w, h]
            }
        };
        for (v, by) in point.iter_mut().zip(p.scale) {
            *v *= by;
        }
        // The root is a container.
        let parent_kind = s.parent.map_or(kind::CONTAINER, |q| self.sprite(q).kind());
        if !unanchored(parent_kind) {
            let (col, row) = anchor(props::ALIGN_TO_PARENT.get(s.properties));
            point[0] += col * 0.5 * (p.size[0] * p.scale[0]);
            point[2] += row * 0.5 * (p.size[1] * p.scale[2]);
        }
        for (v, at) in point.iter_mut().zip(p.point) {
            *v += at;
        }
        if !unanchored(k) {
            let (col, row) = anchor(props::ALIGNMENT.get(s.properties));
            point[0] -= col * 0.5 * (size[0] * scale[0]);
            point[2] -= row * 0.5 * (size[1] * scale[2]);
        }
        Placed {
            point,
            scale,
            size,
            priority,
            depth,
            color,
            replace: s.replace,
            drawn: false,
            dirty: false,
            text,
        }
    }
}
