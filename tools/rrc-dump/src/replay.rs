//! Walks the command list, tracking the NV4097 registers that locate a draw's inputs.
//! At each `SET_BEGIN_END(0)` it hands the caller a [`Draw`] with the bytes read out of sparse memory.

use serde::Serialize;

use crate::capture::Capture;
use crate::memory::SparseMemory;
use crate::nv4097 as m;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawKind {
    Arrays,
    Indexed,
    /// No `DRAW_ARRAYS`/`DRAW_INDEX_ARRAY` range: immediate-mode vertices, not dumped.
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DrawRange {
    pub first: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct VertexArray {
    pub slot: u32,
    /// RSX offset of vertex 0 (`BASE_OFFSET` + the array offset, masked to 28 bits).
    pub rsx_offset: u32,
    pub location: u32,
    pub type_raw: u32,
    pub type_name: &'static str,
    pub components: u32,
    pub stride: u32,
    pub frequency: u32,
    pub element_size: u32,
    /// The dumped bytes start at this vertex: `rsx_offset + first_vertex * stride`.
    pub first_vertex: u32,
    pub vertex_count: u32,
    /// Bytes the draw reads that no captured block covered. Array draws count each range
    /// separately, so the gap between disjoint ranges (zero-filled in `bytes`) isn't counted.
    /// RPCS3's capture truncates `index + base` to u16 for u16 draws, so when a base index
    /// pushes them past 0xFFFF it records a wrapped window: missing bytes there are a capture
    /// gap, not a decode error.
    pub missing_bytes: usize,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexArray {
    pub rsx_offset: u32,
    pub location: u32,
    pub type_name: &'static str,
    pub type_size: u32,
    pub base_index: u32,
    pub restart_index: Option<u32>,
    /// The dumped bytes start at this index slot: `rsx_offset + first * type_size`.
    pub first: u32,
    pub span: u32,
    pub index_count: u32,
    /// Smallest and largest vertex referenced after the base index, restart skipped.
    pub min_vertex: Option<u32>,
    pub max_vertex: Option<u32>,
    /// Bytes of the draw's index ranges no captured block covered (counted per range).
    pub missing_bytes: usize,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextureUnit {
    pub unit: u32,
    pub offset: u32,
    pub location: u32,
    pub format_raw: u32,
    pub format_name: &'static str,
    /// The format's raw `LN` bit.
    pub linear: bool,
    /// Texels are Morton-ordered: `LN` clear, except for DXT* and B8R8_G8R8/R8B8_R8G8, which
    /// RPCS3 never swizzles (for them `LN` clear means packed rows, see `nv4097::never_swizzled`).
    pub swizzled: bool,
    pub unnormalized: bool,
    pub dimension: u32,
    pub cubemap: bool,
    pub border: bool,
    pub mipmaps: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub pitch: u32,
    pub remap: u32,
    /// A block was recorded at this texture's address. RPCS3 copies every enabled texture with
    /// a non-empty layout out of guest memory at each draw's end, sized by its own layout (all
    /// mips, all faces), so a recorded texture is whole by construction (though possibly stale:
    /// guest memory, not GPU-resident data). False means RPCS3 recorded nothing.
    pub captured: bool,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Constants {
    /// Inclusive vec4 ranges whose four words are all known.
    pub known_ranges: Vec<(usize, usize)>,
    pub partially_known: usize,
    pub changed_mid_draw: bool,
    #[serde(skip)]
    pub values: Vec<[u32; 4]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Draw {
    pub draw: usize,
    /// Position of the closing `SET_BEGIN_END(0)` in the command list.
    pub command_index: usize,
    pub primitive: u32,
    pub primitive_name: &'static str,
    pub kind: DrawKind,
    pub ranges: Vec<DrawRange>,
    pub vertex_count: u32,
    pub vertex_data_base_offset: u32,
    pub vertex_attrib_input_mask: u32,
    pub vertex_arrays: Vec<VertexArray>,
    pub index_array: Option<IndexArray>,
    pub textures: Vec<TextureUnit>,
    pub constants: Constants,
    /// `SET_VERTEX_DATA_BASE_OFFSET`, `SET_VERTEX_DATA_BASE_INDEX` or an array offset was written
    /// between two ranges. RPCS3 draws each range with the binding it was issued under, but both
    /// its capture and this dump use the final binding for every range, so the earlier ranges'
    /// true bytes aren't in the capture.
    pub vertex_bindings_changed_mid_draw: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ReplayStats {
    pub commands: usize,
    pub draws: usize,
    /// Memory-state ids with no matching block or data entry.
    pub unresolved_blocks: usize,
    /// Zero-count commands outside any batch. Every capture opens with exactly one: the
    /// capture-start NOP, stored as the bare register index (RPCS3's replay writes its header
    /// word and no argument). More than one is an anomaly.
    pub orphan_commands: usize,
    pub stray_ends: usize,
    pub starting_constants: bool,
}

struct OpenDraw {
    primitive: u32,
    ranges: Vec<DrawRange>,
    indexed: Option<bool>,
    constants_changed: bool,
    bindings_changed: bool,
}

struct Tracker {
    regs: Vec<u32>,
    constants: Vec<[u32; 4]>,
    known: Vec<[bool; 4]>,
    mem: SparseMemory,
    open: Option<OpenDraw>,
}

impl Tracker {
    fn reg(&self, r: u32) -> u32 {
        self.regs.get(r as usize).copied().unwrap_or(0)
    }

    fn write_constants(&mut self, first_reg: u32, values: &[u32]) {
        let index0 = first_reg - m::SET_TRANSFORM_CONSTANT;
        let load = self.reg(m::SET_TRANSFORM_CONSTANT_LOAD) as usize;
        let n = values.len().min((m::UPLOAD_WINDOW - index0) as usize);
        for (k, &v) in values[..n].iter().enumerate() {
            let idx = index0 as usize + k;
            let slot = load + idx / 4;
            if slot < m::TRANSFORM_CONSTANTS {
                self.constants[slot][idx % 4] = v;
                self.known[slot][idx % 4] = true;
            }
        }
        if let Some(open) = &mut self.open
            && !open.ranges.is_empty()
        {
            open.constants_changed = true;
        }
    }

    /// `ranges` is set for array draws, whose missing bytes are counted per range.
    fn vertex_arrays(
        &self,
        span: Option<(u32, u32)>,
        ranges: Option<&[DrawRange]>,
    ) -> Vec<VertexArray> {
        let mask = self.reg(m::SET_VERTEX_ATTRIB_INPUT_MASK);
        let base = self.reg(m::SET_VERTEX_DATA_BASE_OFFSET);
        let mut out = Vec::new();
        for slot in 0..m::VERTEX_ARRAYS {
            let format = self.reg(m::SET_VERTEX_DATA_ARRAY_FORMAT + slot);
            let components = (format >> 4) & 0xf;
            if mask & (1 << slot) == 0 || components == 0 {
                continue;
            }
            let type_raw = format & 0x7;
            let stride = (format >> 8) & 0xff;
            let offset_reg = self.reg(m::SET_VERTEX_DATA_ARRAY_OFFSET + slot);
            let rsx_offset = base.wrapping_add(offset_reg & 0x7fff_ffff) & 0x0fff_ffff;
            let location = offset_reg >> 31;
            let element_size = m::vertex_element_size(type_raw, components);
            let (first_vertex, vertex_count) = span.unwrap_or((0, 0));
            let read_len =
                |count: u32| (count - 1) as usize * stride as usize + element_size as usize;
            let at = |vertex: u32| u64::from(rsx_offset) + u64::from(vertex) * u64::from(stride);
            let (bytes, missing_bytes) = if vertex_count == 0 || element_size == 0 {
                (Vec::new(), 0)
            } else {
                let (bytes, span_missing) =
                    self.mem
                        .read(location, at(first_vertex), read_len(vertex_count));
                let missing = match ranges {
                    Some(ranges) => ranges
                        .iter()
                        .filter(|r| r.count > 0)
                        .map(|r| self.mem.read(location, at(r.first), read_len(r.count)).1)
                        .sum(),
                    None => span_missing,
                };
                (bytes, missing)
            };
            out.push(VertexArray {
                slot,
                rsx_offset,
                location,
                type_raw,
                type_name: m::vertex_type(type_raw).0,
                components,
                stride,
                frequency: format >> 16,
                element_size,
                first_vertex,
                vertex_count,
                missing_bytes,
                bytes,
            });
        }
        out
    }

    fn index_array(&self, ranges: &[DrawRange]) -> IndexArray {
        let rsx_offset = self.reg(m::SET_INDEX_ARRAY_ADDRESS) & m::INDEX_ARRAY_OFFSET_MASK;
        let dma = self.reg(m::SET_INDEX_ARRAY_DMA);
        let location = dma & 0xf;
        let is_u16 = (dma >> 4) & 0xf == 1;
        let type_size: u32 = if is_u16 { 2 } else { 4 };
        let base_index = self.reg(m::SET_VERTEX_DATA_BASE_INDEX);
        let restart_value = self.reg(m::SET_RESTART_INDEX);
        let restart_index = (self.reg(m::SET_RESTART_INDEX_ENABLE) != 0
            && !(is_u16 && restart_value > 0xffff))
            .then_some(restart_value);

        let first = ranges.iter().map(|r| r.first).min().unwrap_or(0);
        let end = ranges.iter().map(|r| r.first + r.count).max().unwrap_or(0);
        let span = end - first;
        let start = u64::from(rsx_offset) + u64::from(first) * u64::from(type_size);
        let (bytes, _) = self
            .mem
            .read(location, start, span as usize * type_size as usize);
        let missing_bytes = ranges
            .iter()
            .map(|r| {
                let at = u64::from(rsx_offset) + u64::from(r.first) * u64::from(type_size);
                self.mem
                    .read(location, at, r.count as usize * type_size as usize)
                    .1
            })
            .sum();

        let mut min_vertex: Option<u32> = None;
        let mut max_vertex: Option<u32> = None;
        for r in ranges {
            for slot in r.first..r.first + r.count {
                let at = ((slot - first) * type_size) as usize;
                let raw = if is_u16 {
                    u32::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]))
                } else {
                    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
                };
                if restart_index == Some(raw) {
                    continue;
                }
                let v = raw.wrapping_add(base_index) & 0x000f_ffff;
                min_vertex = Some(min_vertex.map_or(v, |m| m.min(v)));
                max_vertex = Some(max_vertex.map_or(v, |m| m.max(v)));
            }
        }

        IndexArray {
            rsx_offset,
            location,
            type_name: if is_u16 { "u16" } else { "u32" },
            type_size,
            base_index,
            restart_index,
            first,
            span,
            index_count: ranges.iter().map(|r| r.count).sum(),
            min_vertex,
            max_vertex,
            missing_bytes,
            bytes,
        }
    }

    fn textures(&self) -> Vec<TextureUnit> {
        let mut out = Vec::new();
        for unit in 0..m::FRAGMENT_TEXTURE_UNITS {
            let r = |base: u32| self.reg(base + unit * m::TEXTURE_UNIT_STRIDE);
            if r(m::SET_TEXTURE_CONTROL0) >> 31 == 0 {
                continue;
            }
            let format = r(m::SET_TEXTURE_FORMAT);
            let offset = r(m::SET_TEXTURE_OFFSET) & 0x7fff_ffff;
            let location = (format & 0x3).wrapping_sub(1);
            let format_raw = (format >> 8) & 0xff;
            let dimension = (format >> 4) & 0xf;
            let rect = r(m::SET_TEXTURE_IMAGE_RECT);
            let control3 = self.reg(m::SET_TEXTURE_CONTROL3 + unit);
            let block_len = self.mem.block_len_at(location, offset);
            let bytes = block_len.map_or_else(Vec::new, |len| {
                self.mem.read(location, u64::from(offset), len).0
            });
            let base = format_raw & !(m::TEXTURE_LN | m::TEXTURE_UN);
            let linear = format_raw & m::TEXTURE_LN != 0;
            out.push(TextureUnit {
                unit,
                offset,
                location,
                format_raw,
                format_name: m::texture_format_name(base),
                linear,
                swizzled: !linear && !m::never_swizzled(base),
                unnormalized: format_raw & m::TEXTURE_UN != 0,
                dimension,
                cubemap: (format >> 2) & 1 == 1,
                border: (format >> 3) & 1 == 1,
                mipmaps: format >> 16,
                width: rect >> 16,
                height: if dimension == 1 { 1 } else { rect & 0xffff },
                depth: if dimension == 3 { control3 >> 20 } else { 1 },
                pitch: control3 & 0xf_ffff,
                remap: r(m::SET_TEXTURE_CONTROL1),
                captured: block_len.is_some(),
                bytes,
            });
        }
        out
    }

    fn constants(&self, changed_mid_draw: bool) -> Constants {
        let mut known_ranges: Vec<(usize, usize)> = Vec::new();
        let mut partially_known = 0;
        for (i, k) in self.known.iter().enumerate() {
            if k.iter().all(|&b| b) {
                match known_ranges.last_mut() {
                    Some((_, end)) if *end + 1 == i => *end = i,
                    _ => known_ranges.push((i, i)),
                }
            } else if k.iter().any(|&b| b) {
                partially_known += 1;
            }
        }
        Constants {
            known_ranges,
            partially_known,
            changed_mid_draw,
            values: self.constants.clone(),
        }
    }

    fn close(&self, open: OpenDraw, draw: usize, command_index: usize) -> Draw {
        let kind = match open.indexed {
            Some(true) => DrawKind::Indexed,
            Some(false) => DrawKind::Arrays,
            None => DrawKind::Immediate,
        };
        let (index_array, span) = match kind {
            DrawKind::Indexed => {
                let ia = self.index_array(&open.ranges);
                let span = ia
                    .min_vertex
                    .zip(ia.max_vertex)
                    .map(|(lo, hi)| (lo, hi - lo + 1));
                (Some(ia), span)
            }
            DrawKind::Arrays => {
                let lo = open.ranges.iter().map(|r| r.first).min().unwrap_or(0);
                let hi = open
                    .ranges
                    .iter()
                    .map(|r| r.first + r.count)
                    .max()
                    .unwrap_or(0);
                (None, Some((lo, hi - lo)))
            }
            DrawKind::Immediate => (None, None),
        };
        let vertex_count = match kind {
            DrawKind::Indexed => span.map_or(0, |s| s.1),
            _ => open.ranges.iter().map(|r| r.count).sum(),
        };
        let vertex_arrays = self.vertex_arrays(
            span,
            (kind == DrawKind::Arrays).then_some(open.ranges.as_slice()),
        );
        Draw {
            draw,
            command_index,
            primitive: open.primitive,
            primitive_name: m::primitive_name(open.primitive),
            kind,
            ranges: open.ranges,
            vertex_count,
            vertex_data_base_offset: self.reg(m::SET_VERTEX_DATA_BASE_OFFSET),
            vertex_attrib_input_mask: self.reg(m::SET_VERTEX_ATTRIB_INPUT_MASK),
            vertex_arrays,
            index_array,
            textures: self.textures(),
            constants: self.constants(open.constants_changed),
            vertex_bindings_changed_mid_draw: open.bindings_changed,
        }
    }
}

/// Replays `capture`, calling `on_draw` at each draw's end with its inputs read out.
pub fn replay<E>(
    capture: &Capture,
    mut on_draw: impl FnMut(Draw) -> Result<(), E>,
) -> Result<ReplayStats, E> {
    let starting = capture.reg_state.transform_constants.as_ref();
    let mut t = Tracker {
        regs: capture.reg_state.registers.clone(),
        constants: match starting {
            Some(c) => c[..m::TRANSFORM_CONSTANTS].to_vec(),
            None => vec![[0; 4]; m::TRANSFORM_CONSTANTS],
        },
        known: vec![[starting.is_some(); 4]; m::TRANSFORM_CONSTANTS],
        mem: SparseMemory::default(),
        open: None,
    };
    let mut stats = ReplayStats {
        commands: capture.commands.len(),
        starting_constants: starting.is_some(),
        ..ReplayStats::default()
    };

    let cmds = &capture.commands;
    let mut i = 0;
    while i < cmds.len() {
        let header = cmds[i].header;
        let count = ((header >> m::HEADER_COUNT_SHIFT) & m::HEADER_COUNT_MASK) as usize;
        let end = (i + count.max(1)).min(cmds.len());
        for cmd in &cmds[i..end] {
            for id in &cmd.memory_state {
                match capture
                    .blocks
                    .get(id)
                    .and_then(|b| capture.data.get(&b.data_id).map(|d| (b, d)))
                {
                    Some((b, d)) => t.mem.write(m::normalize_location(b.location), b.offset, d),
                    None => stats.unresolved_blocks += 1,
                }
            }
        }
        if count == 0 {
            stats.orphan_commands += 1;
            i += 1;
            continue;
        }

        let reg = (header & m::HEADER_METHOD_MASK) >> 2;
        let values: Vec<u32> = cmds[i..end].iter().map(|c| c.value).collect();
        let arrays =
            m::SET_VERTEX_DATA_ARRAY_OFFSET..m::SET_VERTEX_DATA_ARRAY_OFFSET + m::VERTEX_ARRAYS;
        if let Some(open) = &mut t.open
            && !open.ranges.is_empty()
            && (reg == m::SET_VERTEX_DATA_BASE_OFFSET
                || reg == m::SET_VERTEX_DATA_BASE_INDEX
                || arrays.contains(&reg))
        {
            open.bindings_changed = true;
        }
        if let Some(slot) = t.regs.get_mut(reg as usize) {
            *slot = values[0];
        }
        let window = m::SET_TRANSFORM_CONSTANT..m::SET_TRANSFORM_CONSTANT + m::UPLOAD_WINDOW;
        if window.contains(&reg) {
            t.write_constants(reg, &values);
        }
        // Only the three ranged methods batch, and RPCS3's handlers consume their tails without
        // touching the register array, so a batch's later words never land in registers.
        if count == 1 && !window.contains(&reg) {
            let value = values[0];
            match reg {
                // RPCS3 looks only at the low byte: 0x100 ends a draw like 0 does.
                m::SET_BEGIN_END if value & 0xff != 0 => {
                    t.open = Some(OpenDraw {
                        primitive: value & 0xff,
                        ranges: Vec::new(),
                        indexed: None,
                        constants_changed: false,
                        bindings_changed: false,
                    });
                }
                m::SET_BEGIN_END => match t.open.take() {
                    Some(open) => {
                        let draw = t.close(open, stats.draws, i);
                        stats.draws += 1;
                        on_draw(draw)?;
                    }
                    None => stats.stray_ends += 1,
                },
                m::DRAW_ARRAYS | m::DRAW_INDEX_ARRAY => {
                    if let Some(open) = &mut t.open {
                        open.indexed = Some(reg == m::DRAW_INDEX_ARRAY);
                        open.ranges.push(DrawRange {
                            first: value & 0x00ff_ffff,
                            count: (value >> 24) + 1,
                        });
                    }
                }
                _ => {}
            }
        }
        i = end;
    }
    Ok(stats)
}
