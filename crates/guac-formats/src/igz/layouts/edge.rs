//! Edge geometry: the segment (Edge's PPU-side segment description, field
//! for field), the segment list and the attribute a technique draws.

use super::{DataList, Layout, Vector, View};
use crate::FormatError;
use crate::igz::MemoryRef;

/// `igPS3EdgeGeometrySegment`: size 0xB0, parent `igObject`. Skinned
/// segments carry `_spuVertexes1` and `_skinIndexesAndWeights`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeGeometrySegment<'a> {
    /// Edge's 16-byte SPU config record.
    pub spu_config_info: Option<MemoryRef<'a>>,
    /// The compressed index stream.
    pub indexes: Option<MemoryRef<'a>>,
    pub indexes_sizes: [u16; 2],
    pub spu_vertexes0: Option<MemoryRef<'a>>,
    pub spu_vertexes1: Option<MemoryRef<'a>>,
    /// Two groups of three, one per SPU vertex stream.
    pub spu_vertexes_sizes: [u16; 6],
    pub rsx_only_vertexes: Option<MemoryRef<'a>>,
    pub rsx_only_vertexes_size: u32,
    pub skin_matrices_byte_offsets: [u16; 2],
    /// Multiples of 48 (3x4 f32 matrices).
    pub skin_matrices_sizes: [u16; 2],
    pub skin_indexes_and_weights_sizes: [u16; 2],
    pub skin_indexes_and_weights: Option<MemoryRef<'a>>,
    pub io_buffer_size: u32,
    pub scratch_size: u32,
    pub spu_input_stream_descs0: Option<MemoryRef<'a>>,
    pub spu_input_stream_descs1: Option<MemoryRef<'a>>,
    pub spu_output_stream_desc: Option<MemoryRef<'a>>,
    pub rsx_only_stream_desc: Option<MemoryRef<'a>>,
    pub spu_input_stream_desc_sizes: [u16; 2],
    pub spu_output_stream_desc_size: u16,
    pub rsx_only_stream_desc_size: u16,
    pub num_blend_shapes: u32,
    /// u16 elements.
    pub blend_shape_sizes: Vector<'a>,
    pub blend_shape_data: Option<MemoryRef<'a>>,
    /// Raw-ref elements.
    pub blend_shapes: Vector<'a>,
}

impl EdgeGeometrySegment<'_> {
    pub const SPU_CONFIG_INFO: u32 = 0x08;
    pub const INDEXES: u32 = 0x10;
    pub const INDEXES_SIZES: u32 = 0x18;
    pub const SPU_VERTEXES0: u32 = 0x1C;
    pub const SPU_VERTEXES1: u32 = 0x24;
    pub const SPU_VERTEXES_SIZES: u32 = 0x2C;
    pub const RSX_ONLY_VERTEXES: u32 = 0x38;
    pub const RSX_ONLY_VERTEXES_SIZE: u32 = 0x40;
    pub const SKIN_MATRICES_BYTE_OFFSETS: u32 = 0x44;
    pub const SKIN_MATRICES_SIZES: u32 = 0x48;
    pub const SKIN_INDEXES_AND_WEIGHTS_SIZES: u32 = 0x4C;
    pub const SKIN_INDEXES_AND_WEIGHTS: u32 = 0x50;
    pub const IO_BUFFER_SIZE: u32 = 0x58;
    pub const SCRATCH_SIZE: u32 = 0x5C;
    pub const SPU_INPUT_STREAM_DESCS0: u32 = 0x60;
    pub const SPU_INPUT_STREAM_DESCS1: u32 = 0x68;
    pub const SPU_OUTPUT_STREAM_DESC: u32 = 0x70;
    pub const RSX_ONLY_STREAM_DESC: u32 = 0x78;
    pub const SPU_INPUT_STREAM_DESC_SIZES: u32 = 0x80;
    pub const SPU_OUTPUT_STREAM_DESC_SIZE: u32 = 0x84;
    pub const RSX_ONLY_STREAM_DESC_SIZE: u32 = 0x86;
    pub const NUM_BLEND_SHAPES: u32 = 0x88;
    pub const BLEND_SHAPE_SIZES: u32 = 0x8C;
    pub const BLEND_SHAPE_DATA: u32 = 0x98;
    pub const BLEND_SHAPES: u32 = 0xA0;

    /// (numVertexes, numIndexes) from the config record's u16s at +8 and +10.
    pub fn counts(&self) -> Option<(u16, u16)> {
        let c = self.spu_config_info?.data;
        let word = |at: usize| Some(u16::from_be_bytes([*c.get(at)?, *c.get(at + 1)?]));
        Some((word(8)?, word(10)?))
    }

    pub fn is_skinned(&self) -> bool {
        self.skin_indexes_and_weights.is_some()
    }
}

impl<'a> Layout<'a> for EdgeGeometrySegment<'a> {
    const TYPE: &'static str = "igPS3EdgeGeometrySegment";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            spu_config_info: v.memory_ref(Self::SPU_CONFIG_INFO, "_spuConfigInfo")?,
            indexes: v.memory_ref(Self::INDEXES, "_indexes")?,
            indexes_sizes: v.u16s(Self::INDEXES_SIZES, "_indexesSizes")?,
            spu_vertexes0: v.memory_ref(Self::SPU_VERTEXES0, "_spuVertexes0")?,
            spu_vertexes1: v.memory_ref(Self::SPU_VERTEXES1, "_spuVertexes1")?,
            spu_vertexes_sizes: v.u16s(Self::SPU_VERTEXES_SIZES, "_spuVertexesSizes")?,
            rsx_only_vertexes: v.memory_ref(Self::RSX_ONLY_VERTEXES, "_rsxOnlyVertexes")?,
            rsx_only_vertexes_size: v.u32(Self::RSX_ONLY_VERTEXES_SIZE, "_rsxOnlyVertexesSize")?,
            skin_matrices_byte_offsets: v
                .u16s(Self::SKIN_MATRICES_BYTE_OFFSETS, "_skinMatricesByteOffsets")?,
            skin_matrices_sizes: v.u16s(Self::SKIN_MATRICES_SIZES, "_skinMatricesSizes")?,
            skin_indexes_and_weights_sizes: v.u16s(
                Self::SKIN_INDEXES_AND_WEIGHTS_SIZES,
                "_skinIndexesAndWeightsSizes",
            )?,
            skin_indexes_and_weights: v
                .memory_ref(Self::SKIN_INDEXES_AND_WEIGHTS, "_skinIndexesAndWeights")?,
            io_buffer_size: v.u32(Self::IO_BUFFER_SIZE, "_ioBufferSize")?,
            scratch_size: v.u32(Self::SCRATCH_SIZE, "_scratchSize")?,
            spu_input_stream_descs0: v
                .memory_ref(Self::SPU_INPUT_STREAM_DESCS0, "_spuInputStreamDescs0")?,
            spu_input_stream_descs1: v
                .memory_ref(Self::SPU_INPUT_STREAM_DESCS1, "_spuInputStreamDescs1")?,
            spu_output_stream_desc: v
                .memory_ref(Self::SPU_OUTPUT_STREAM_DESC, "_spuOutputStreamDesc")?,
            rsx_only_stream_desc: v.memory_ref(Self::RSX_ONLY_STREAM_DESC, "_rsxOnlyStreamDesc")?,
            spu_input_stream_desc_sizes: v.u16s(
                Self::SPU_INPUT_STREAM_DESC_SIZES,
                "_spuInputStreamDescSizes",
            )?,
            spu_output_stream_desc_size: v.u16(
                Self::SPU_OUTPUT_STREAM_DESC_SIZE,
                "_spuOutputStreamDescSize",
            )?,
            rsx_only_stream_desc_size: v
                .u16(Self::RSX_ONLY_STREAM_DESC_SIZE, "_rsxOnlyStreamDescSize")?,
            num_blend_shapes: v.u32(Self::NUM_BLEND_SHAPES, "_numBlendShapes")?,
            blend_shape_sizes: v.vector(Self::BLEND_SHAPE_SIZES, "_blendShapeSizes")?,
            blend_shape_data: v.memory_ref(Self::BLEND_SHAPE_DATA, "_blendShapeData")?,
            blend_shapes: v.vector(Self::BLEND_SHAPES, "_blendShapes")?,
        })
    }
}

/// `igPS3EdgeGeometry`: size 0x1C, an object list of segments plus a hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeGeometry<'a> {
    pub list: DataList<'a>,
    /// Indexes of the `igPS3EdgeGeometrySegment` objects, in draw order.
    pub segments: Vec<usize>,
    pub hash_code: u32,
}

impl EdgeGeometry<'_> {
    pub const HASH_CODE: u32 = 0x18;
}

impl<'a> Layout<'a> for EdgeGeometry<'a> {
    const TYPE: &'static str = "igPS3EdgeGeometry";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let segments = v
            .elements(&list, Some("igPS3EdgeGeometrySegment"))?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self {
            list,
            segments,
            hash_code: v.u32(Self::HASH_CODE, "_hashCode")?,
        })
    }
}

/// `igAttr`'s fields: size 0x10, runtime bookkeeping in files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attr {
    pub cached_unit_id: i16,
    pub cached_attr_index: i16,
    pub read_only_copy: bool,
}

impl Attr {
    pub const CACHED_UNIT_ID: u32 = 0x08;
    pub const CACHED_ATTR_INDEX: u32 = 0x0A;
    pub const READ_ONLY_COPY: u32 = 0x0C;

    pub(crate) fn fields(v: &mut View<'_, '_>) -> Result<Self, FormatError> {
        Ok(Self {
            cached_unit_id: v.i16(Self::CACHED_UNIT_ID, "_cachedUnitID")?,
            cached_attr_index: v.i16(Self::CACHED_ATTR_INDEX, "_cachedAttrIndex")?,
            read_only_copy: v.bool(Self::READ_ONLY_COPY, "_readOnlyCopy")?,
        })
    }
}

/// `igEdgeGeometryAttr`: size 0x14, parent `igDrawableAttr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeGeometryAttr {
    pub attr: Attr,
    /// An `igPS3EdgeGeometry`.
    pub geometry: Option<usize>,
}

impl EdgeGeometryAttr {
    pub const GEOMETRY: u32 = 0x10;
}

impl<'a> Layout<'a> for EdgeGeometryAttr {
    const TYPE: &'static str = "igEdgeGeometryAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            geometry: v.object(Self::GEOMETRY, "_geometry", "igPS3EdgeGeometry")?,
        })
    }
}
