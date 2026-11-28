//! Pipelines the console runtime serialized, as the title hands them back
//! (`DeserializeGraphicsPipelineStateX`, device method 0x1c8): rebuilt as
//! console shader containers, one per stage, that the translator takes like
//! any other.
//!
//! A serialized pipeline is a register packet plus, per stage, the GFX10
//! code and a metadata blob; nothing else (no signatures, no export patch
//! variants). What the containers need comes from those (28 Nov 2026, the
//! game's 163 dumped descriptions; layout as the kit's console runtime
//! `xgs12_10.dll` reads the packet back, field meanings from AMD's GFX10
//! register reference where the runtime does not name them):
//!
//! ```text
//! packet +0x04 u32   bits 0-1: 1 tessellation, 2 a GS-stage (mesh) pipeline
//!        +0x0c u32   & 0x3f: the render-target record index
//!        +0x28 u16   register block size; +0x2a u16 render-target block size
//!        +0x2c       PsoSpi_Standard (0x2c B): +0x0c VS RSRC1, +0x10 VS RSRC2,
//!                    +0x20 PS RSRC1, +0x24 PS RSRC2
//!        +0x58       PsoSpi_GeometryShader (0x54 B) when GS: +0x08 RSRC1_GS,
//!                    +0x0c RSRC2_GS (bits 1-5 user SGPRs, 19-26 LDS in 512 B),
//!                    +0x18 VGT_GS_ONCHIP_CNTL (bits 0-10 lanes per group),
//!                    +0x1c, +0x20 the vertex and primitive maxima (both 128 in
//!                    every sample: their order is not shown), +0x30
//!                    VGT_GS_OUT_PRIM_TYPE (2 triangles)
//!        then        PsoContext_Standard (0xe0 B): +0x00 SPI_PS_INPUT_ENA (the
//!                    input VGPRs follow it as SPI_PS_INPUT_ADDR), +0x04
//!                    SPI_PS_IN_CONTROL (bits 0-5 attributes, bit 15 wave32),
//!                    +0x18 SPI_SHADER_COL_FORMAT, +0x40 PA_CL_VS_OUT_CNTL
//!        then        the render-target block (see [`Packet::render_targets`])
//! stage metadata  u32 2, u32 size - 0xc, u32 code dwords, u8 0x40, u8, u8, u8
//!                 0; 32-byte records from +0x10 as the compute form's
//!                 ([`crate::native::fspx_program`]): 0x41 a resource (+0x10 is
//!                 a structured buffer's stride), 0x42 a view, 0x43 the hash
//! ```
//!
//! The stages were compiled together: pixel attribute n is mesh parameter n
//! (checked on all 156 sampled pairs: no attribute read that is not
//! exported). A parameter the pixel stage interpolates as 16-bit values
//! carries two 16-bit rows per dword (8 halves), as packed console outputs
//! do. The pixel code exports in the target's format (`SPI_SHADER_COL_FORMAT`:
//! compressed FP16 mostly): the container says so in a `VXCF` part, which the
//! translator reads ([`crate::emit`]).

use std::collections::BTreeMap;

use crate::ShaderError::{self, Malformed, Unsupported};
use crate::dxbc::u32_at;
use crate::gfx10::{self, Fields, Operand};

/// The container part that carries a pixel stage's `SPI_SHADER_COL_FORMAT`.
pub const COLOR_FORMAT_PART: [u8; 4] = *b"VXCF";

const CONTEXT_SIZE: usize = 0xe0;
const SPI_STANDARD: usize = 0x2c;
const SPI_GS: usize = 0x58;
const SPI_GS_SIZE: usize = 0x54;

/// A serialized pipeline's register packet.
#[derive(Clone, Copy)]
pub struct Packet<'a>(&'a [u8]);

/// One render target as the packet gives it: the CB format, number type and
/// swap of AMD's registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub format: u8,
    pub number: u8,
    pub swap: u8,
}

impl<'a> Packet<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, ShaderError> {
        let p = Packet(data);
        let block = u16_at(data, 0x28)? as usize;
        let context_end = p.context_at()? + CONTEXT_SIZE;
        if 0x1c + block != context_end {
            return Err(Malformed("serialized packet: register block size disagrees"));
        }
        let rt = u16_at(data, 0x2a)? as usize;
        if data.len() < context_end + rt {
            return Err(Malformed("serialized packet: truncated"));
        }
        Ok(p)
    }

    fn config(&self) -> u32 {
        u32_at(self.0, 4).unwrap_or(0) & 3
    }

    /// A GS-stage pipeline (the console's mesh shaders and NGG geometry).
    pub fn geometry(&self) -> bool {
        self.config() & 2 != 0
    }

    pub fn tessellation(&self) -> bool {
        self.config() & 1 != 0
    }

    fn context_at(&self) -> Result<usize, ShaderError> {
        if self.tessellation() {
            return Err(Unsupported { offset: 0, what: "a tessellation pipeline" });
        }
        Ok(SPI_GS + if self.geometry() { SPI_GS_SIZE } else { 0 })
    }

    /// A dword of `PsoContext_Standard`.
    pub fn context(&self, at: usize) -> u32 {
        self.context_at().ok().and_then(|c| u32_at(self.0, c + at).ok()).unwrap_or(0)
    }

    /// A dword of the whole packet (`PsoSpi_*` fields).
    pub fn word(&self, at: usize) -> u32 {
        u32_at(self.0, at).unwrap_or(0)
    }

    /// The render-target block's record (three dwords at block + 4 * index
    /// + 0x38): 8 number types of 3 bits, 8 swaps of 2 bits (the first
    /// dword's bits 24-31, the second's 0-7), 8 formats of 5 bits (the
    /// second dword's bits 8-27, the third's 0-19), the count in the second
    /// dword's bits 28-31, samples as 1 << the third's bits 20-22.
    pub fn render_targets(&self) -> Result<(Vec<Target>, u32), ShaderError> {
        let block = 0x1c + u16_at(self.0, 0x28)? as usize;
        let at = block + 4 * (self.word(0x0c) & 0x3f) as usize + 0x38;
        let (d0, d1, d2) = (u32_at(self.0, at)?, u32_at(self.0, at + 4)?, u32_at(self.0, at + 8)?);
        let count = (d1 >> 28).min(8) as usize;
        let targets = (0..count)
            .map(|i| {
                let i32_ = i as u32;
                let swap = if i < 4 { d0 >> (24 + 2 * i32_) } else { d1 >> (2 * (i32_ - 4)) } & 3;
                let format = if i < 4 { d1 >> (8 + 5 * i32_) } else { d2 >> (5 * (i32_ - 4)) } & 31;
                Target { format: format as u8, number: (d0 >> (3 * i32_) & 7) as u8, swap: swap as u8 }
            })
            .collect();
        Ok((targets, 1 << (d2 >> 20 & 7)))
    }
}

/// The DXGI format of a render target, as the console runtime's
/// `ConvertTargetColorFormatToDxgiFormat` has it (number types: 0 UNORM,
/// 1 SNORM, 4 UINT, 5 SINT, 6 SRGB, 7 FLOAT).
pub fn dxgi_format(t: Target) -> Option<u32> {
    const UNORM: u8 = 0;
    const UINT: u8 = 4;
    const SRGB: u8 = 6;
    const FLOAT: u8 = 7;
    let n = t.number;
    Some(match (t.format, t.swap) {
        (1, 3) => 0x41,
        (1, _) => match n { 0 => 0x3d, 1 => 0x3f, 4 => 0x3e, 5 => 0x40, _ => return None },
        (2, _) => match n { 0 => 0x38, 1 => 0x3a, 4 => 0x39, 5 => 0x3b, 7 => 0x36, _ => return None },
        (3, _) => match n { 0 => 0x31, 1 => 0x33, 4 => 0x32, 5 => 0x34, _ => return None },
        (4, _) => match n { 4 => 0x2a, 5 => 0x2b, 7 => 0x29, _ => return None },
        (5, _) => match n { 0 => 0x23, 1 => 0x25, 4 => 0x24, 5 => 0x26, 7 => 0x22, _ => return None },
        (6, _) if n == FLOAT => 0x1a,
        (9, _) => match n { 0 => 0x18, 1 => 0xbd, 4 => 0x19, _ => return None },
        (10, 1) if n == UNORM => 0x57,
        (10, 1) if n == SRGB => 0x5b,
        (10, _) => match n { 0 => 0x1c, 1 => 0x1f, 4 => 0x1e, 5 => 0x20, 6 => 0x1d, _ => return None },
        (11, _) => match n { 4 => 0x11, 5 => 0x12, 7 => 0x10, _ => return None },
        (12, _) => match n { 0 => 0x0b, 1 => 0x0d, 4 => 0x0c, 5 => 0x0e, 7 => 0x0a, _ => return None },
        (14, _) => match n { 4 => 3, 5 => 4, 7 => 2, _ => return None },
        (16, 2) => 0x55,
        (17, 1) => 0x56,
        (19, 1) => 0x73,
        (20, _) => match n { 4 => 0x2f, _ => 0x2e },
        (22, 1) if n == UINT => 0x16,
        (22, _) if n == FLOAT => 0x15,
        (24, _) if n == FLOAT => 0x43,
        (30, _) if n == FLOAT => 116,
        (31, _) if n == FLOAT => 117,
        _ => return None,
    })
}

/// One stage of a serialized pipeline: its code and metadata.
#[derive(Clone, Copy)]
pub struct StageParts<'a> {
    pub code: &'a [u8],
    pub meta: &'a [u8],
}

/// What the root signature says that the stage metadata does not: every
/// parameter's size in user-data dwords (a table 1, a root view 2, constants
/// their count), and whether shaders index the heaps directly. The console
/// lays the parameters out in slot order from user-data dword 8, then the
/// resource heap's address, then the sampler heap's, as the title's own
/// containers have them (`MSNC_VS_PropFoliageSkinDeferred`: tables at 8, 9
/// and 10, a root view at 11, constants b500 at 13-14, the resource heap at
/// 15, the sampler heap at 16) and as every record of the dumped pipelines
/// agrees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootLayout {
    /// User-data dwords per parameter, by slot.
    pub sizes: Vec<u32>,
    pub resource_heap: bool,
    pub sampler_heap: bool,
}

/// The first user-data dword of root parameters.
const FIRST_PARAMETER_DWORD: u32 = 8;

impl RootLayout {
    /// The first user-data dword of the parameter in `slot`.
    fn user_data(&self, slot: u8) -> Option<u32> {
        let slot = usize::from(slot);
        (slot < self.sizes.len()).then(|| FIRST_PARAMETER_DWORD + self.sizes[..slot].iter().sum::<u32>())
    }

    /// Where the heap addresses go: after every parameter.
    fn end(&self) -> u32 {
        FIRST_PARAMETER_DWORD + self.sizes.iter().sum::<u32>()
    }
}

impl RootLayout {
    /// From a serialized root signature (`RTS0`, alone or in a DXBC
    /// container; versions 1.0 to 1.2).
    pub fn from_blob(blob: &[u8]) -> Result<Self, ShaderError> {
        let rts0 = if blob.starts_with(b"DXBC") {
            let c = crate::Container::parse(blob)?;
            c.part(crate::FourCC::RTS0).ok_or(Malformed("root signature: no RTS0 part"))?.data
        } else {
            blob
        };
        let count = u32_at(rts0, 4)? as usize;
        let at = u32_at(rts0, 8)? as usize;
        if count > 64 {
            return Err(Malformed("root signature: implausible parameter count"));
        }
        let flags = u32_at(rts0, 20)?;
        let mut sizes = Vec::with_capacity(count);
        for slot in 0..count {
            let p = at + 12 * slot;
            sizes.push(match u32_at(rts0, p)? {
                0 => 1,
                1 => u32_at(rts0, u32_at(rts0, p + 8)? as usize + 8)?,
                2..=4 => 2,
                _ => return Err(Malformed("root signature: unknown parameter type")),
            });
        }
        Ok(RootLayout {
            sizes,
            resource_heap: flags & 0x400 != 0,
            sampler_heap: flags & 0x800 != 0,
        })
    }
}

/// The containers of a serialized mesh pipeline.
#[derive(Debug, Default)]
pub struct Graphics {
    pub mesh: Vec<u8>,
    /// None: a depth-only pipeline.
    pub pixel: Option<Vec<u8>>,
}

/// What the pixel code reads of one attribute.
#[derive(Default, Clone, Copy)]
struct Read {
    f16: bool,
    f32: bool,
    flat: bool,
}

/// A signature row.
#[derive(Clone, Copy)]
struct Row {
    name: &'static str,
    index: u32,
    /// The signature's system value (0 none, 1 position, 2 clip, 3 cull,
    /// 9 front face, 64 target, 65 depth).
    system: u32,
    /// 1 uint, 2 sint, 3 float, 6 16-bit float.
    comp: u32,
    register: u32,
    mask: u8,
}

/// The containers for a serialized GS-stage (mesh) pipeline with its
/// pixel stage, if any.
pub fn mesh_pipeline(
    packet: &[u8],
    root: &RootLayout,
    gs: StageParts,
    ps: Option<StageParts>,
) -> Result<Graphics, ShaderError> {
    let p = Packet::new(packet)?;
    if !p.geometry() {
        return Err(Unsupported { offset: 0, what: "a serialized pipeline without a GS stage" });
    }
    let gs_code = group_prologue(stage_code(gs)?)?;
    let gs_code = &gs_code[..];
    let gs_insts = gfx10::decode(gs_code)?;
    if wave32_idiom(&gs_insts) {
        return Err(Unsupported { offset: 0, what: "a wave32 serialized mesh stage" });
    }
    // What the mesh stage exports.
    let mut params: BTreeMap<u8, u8> = BTreeMap::new();
    let mut distances = 0u8;
    for i in &gs_insts {
        if let Fields::Exp { target, src, .. } = i.fields(true) {
            let mask = (0..4).filter(|&k| src[k].is_some()).fold(0u8, |m, k| m | 1 << k);
            match target {
                // Clip and cull distances: channels up to the highest enabled.
                13 | 14 => distances = distances.max(4 * (target - 13) + 8 - mask.leading_zeros() as u8),
                15 => return Err(Unsupported { offset: i.offset, what: "a serialized mesh stage exporting pos3" }),
                32..=63 => *params.entry(target - 32).or_default() |= mask,
                _ => {}
            }
        }
    }
    let ps = match ps {
        Some(s) => {
            let code = stage_code(s)?;
            let insts = gfx10::decode(code)?;
            Some((s, code, insts))
        }
        None => None,
    };
    let ps_wave32 = p.context(4) >> 15 & 1 != 0;
    let mut reads: BTreeMap<u8, Read> = BTreeMap::new();
    if let Some((_, _, insts)) = &ps {
        for i in insts {
            let Some(name) = i.name() else { continue };
            if !name.starts_with("v_interp_") {
                continue;
            }
            let attr = match i.fields(!ps_wave32) {
                Fields::Vintrp { attr, .. } => attr,
                Fields::Valu(v) => v.src.first().copied().unwrap_or(Operand::Const(0)),
                _ => continue,
            };
            let Operand::Attr { index, .. } = attr else { continue };
            let r = reads.entry(index).or_default();
            if name.contains("f16") {
                r.f16 = true;
            } else if name == "v_interp_mov_f32" {
                r.flat = true;
            } else {
                r.f32 = true;
            }
        }
    }
    for (&a, r) in &reads {
        if !params.contains_key(&a) {
            return Err(Unsupported { offset: 0, what: "a pixel attribute the mesh stage does not export" });
        }
        if r.f16 && (r.f32 || r.flat) {
            return Err(Unsupported { offset: 0, what: "an attribute read both as 16-bit and 32-bit values" });
        }
        if r.f16 && r.flat {
            return Err(Unsupported { offset: 0, what: "a flat 16-bit attribute" });
        }
    }
    // Rows: the position, then each parameter (two 16-bit rows when the
    // pixel stage reads 16-bit values), then the clip distances.
    let mut vertex_rows = vec![Row { name: "SV_Position", index: 0, system: 1, comp: 3, register: 0, mask: 0xf }];
    let mut outputs = Vec::new(); // (usage, row, param, flags)
    let mut inputs = Vec::new(); // (row, attr, row2, flags)
    let mut next = 1u32;
    let mut index = 0u32;
    for &n in params.keys() {
        let r = reads.get(&n).copied().unwrap_or_default();
        let first = next;
        let rows = if r.f16 { 2 } else { 1 };
        for _ in 0..rows {
            vertex_rows.push(Row {
                name: "ATTR",
                index,
                system: 0,
                comp: if r.f16 { 6 } else { 3 },
                register: next,
                mask: 0xf,
            });
            index += 1;
            next += 1;
        }
        let half = if r.f16 { 3 } else { 0 };
        outputs.push((6u8, first as u8, n, half));
        if reads.contains_key(&n) {
            let flags = if r.f16 { 0x18 } else { u8::from(r.flat) };
            inputs.push((first as u8, n, if r.f16 { first as u8 + 1 } else { first as u8 }, flags));
        }
    }
    let clip_cntl = p.context(0x40);
    if distances > 0 {
        let clip = (clip_cntl & 0xff) as u8;
        let cull = (clip_cntl >> 8 & 0xff) as u8;
        let all = ((1u16 << distances) - 1) as u8;
        let system = if clip & all == all {
            2
        } else if cull & all == all && clip & all == 0 {
            3
        } else {
            return Err(Unsupported { offset: 0, what: "clip and cull distances mixed in one export" });
        };
        for k in 0..distances.div_ceil(4) {
            let cols = (distances - 4 * k).min(4);
            vertex_rows.push(Row {
                name: if system == 2 { "SV_ClipDistance" } else { "SV_CullDistance" },
                index: u32::from(k),
                system,
                comp: 3,
                register: next,
                mask: ((1u16 << cols) - 1) as u8,
            });
            next += 1;
        }
    }
    let gs_block = |at: usize| p.word(SPI_GS + at);
    let rsrc2 = gs_block(0x0c);
    let topology = match gs_block(0x30) {
        2 => 2,
        1 => 1,
        _ => return Err(Unsupported { offset: 0, what: "a serialized mesh stage of points" }),
    };
    let mesh = MeshInfo {
        threads: gs_block(0x18) & 0x7ff,
        max_vertices: gs_block(0x1c),
        max_primitives: gs_block(0x20),
        user_sgprs: (rsrc2 >> 1 & 0x1f | (rsrc2 >> 27 & 1) << 5) as u8,
        topology,
        lds_bytes: (rsrc2 >> 19 & 0xff) * 512,
        clip_cntl,
    };
    if mesh.threads == 0 || mesh.max_vertices == 0 || mesh.max_primitives == 0 || mesh.max_vertices > 256 || mesh.max_primitives > 256 {
        return Err(Malformed("serialized mesh stage: implausible group limits"));
    }
    let (layout, used) = records(gs.meta, u32::from(mesh.user_sgprs), root)?;
    let gs_record = Record { code: gs_code, wave32: false, vgprs: vgprs(gs_block(0x08), false), layout, used };
    let mesh_blob = fssm(Slot::Ms, &gs_record, &mesh_stage_data(&outputs, &mesh), mesh.lds_bytes);
    let mesh_container = container(&mesh_blob, None, &[], &vertex_rows, false);

    let pixel = match ps {
        None => None,
        Some((s, code, insts)) => {
            let ena = p.context(0);
            // Ancillary data and sample coverage are not given signature
            // rows here.
            if ena & (1 << 13 | 1 << 14) != 0 {
                return Err(Unsupported { offset: 0, what: "ancillary or coverage pixel inputs" });
            }
            let col_format = p.context(0x18);
            let (targets, _) = p.render_targets()?;
            let mut rows: Vec<Row> = Vec::new();
            let mut exported = 0u8;
            let mut depth = false;
            for i in &insts {
                if let Fields::Exp { target, src, .. } = i.fields(!ps_wave32) {
                    match target {
                        0..=7 => exported |= 1 << target,
                        8 => {
                            if src[1..].iter().any(Option::is_some) {
                                return Err(Unsupported { offset: i.offset, what: "a stencil or sample-mask export" });
                            }
                            depth |= src[0].is_some();
                        }
                        _ => {}
                    }
                }
            }
            for n in 0..8u8 {
                if exported >> n & 1 == 0 || col_format >> (4 * n) & 0xf == 0 {
                    continue;
                }
                let comp = match targets.get(usize::from(n)).map(|t| t.number) {
                    Some(4) => 1,
                    Some(5) => 2,
                    _ => 3,
                };
                rows.push(Row { name: "SV_Target", index: u32::from(n), system: 64, comp, register: u32::from(n), mask: 0xf });
            }
            if depth {
                rows.push(Row { name: "SV_Depth", index: 0, system: 65, comp: 3, register: u32::MAX, mask: 1 });
            }
            // The pixel inputs: the rows the mesh stage writes, the clip
            // distances left out.
            let mut in_rows: Vec<Row> = vertex_rows
                .iter()
                .filter(|r| r.system <= 1)
                .copied()
                .collect();
            if ena >> 12 & 1 != 0 {
                let register = in_rows.iter().map(|r| r.register + 1).max().unwrap_or(0);
                in_rows.push(Row { name: "SV_IsFrontFace", index: 0, system: 9, comp: 1, register, mask: 1 });
            }
            let (layout, used) = records(s.meta, user_sgprs(p.word(SPI_STANDARD + 0x24)), root)?;
            let record = Record { code, wave32: ps_wave32, vgprs: vgprs(p.word(SPI_STANDARD + 0x20), ps_wave32), layout, used };
            let blob = fssm(Slot::Ps, &record, &pixel_stage_data(&inputs, ena), 0);
            Some(container(&blob, Some(col_format), &in_rows, &rows, true))
        }
    };
    Ok(Graphics { mesh: mesh_container, pixel })
}

/// User SGPRs of an `SPI_SHADER_PGM_RSRC2_*`.
fn user_sgprs(rsrc2: u32) -> u32 {
    rsrc2 >> 1 & 0x1f | (rsrc2 >> 27 & 1) << 5
}

/// VGPRs of an `SPI_SHADER_PGM_RSRC1_*` (granules of 4, of 8 for wave32).
fn vgprs(rsrc1: u32, wave32: bool) -> u16 {
    (((rsrc1 & 0x3f) + 1) * if wave32 { 8 } else { 4 }).min(256) as u16
}

/// Whether code writes only the low half of EXEC (a wave32 program).
fn wave32_idiom(insts: &[gfx10::Instruction]) -> bool {
    insts.iter().any(|i| {
        let name = i.name().unwrap_or("");
        name.ends_with("saveexec_b32") || i.text(true).contains("exec_lo")
    })
}

/// The serialized mesh stages' prologue, rewritten for the host. Every one
/// of the game's starts so (`sX` s10, or s8 without the save):
///
/// ```text
/// s_mov_b64 exec, 1                 lane 0 only
/// s_mov_b64 s[8:9], exec            (most; restored later for more lane-0 work)
/// s_and_b32 sX, s3, 0x0f000000      wave 0?
/// s_cbranch_scc1 L
/// v_lshrrev_b32 v5, 7, v5           wave 0: the group id, to LDS
/// v_mov_b32 v0, 0
/// ds_write_b32 v0, v5 offset:8
/// s_waitcnt_depctr 0xffe3
/// s_barrier
/// s_branch E
/// L: s_barrier                      the other waves: from LDS
/// v_mov_b32 v0, 0
/// ds_read_b32 v5, v0 offset:8
/// E: (sX written before it is read)
/// ```
///
/// On the host every lane of every wave gets the group input (the
/// translator's `MeshGroup`), so each lane takes it itself: EXEC stays
/// full, and the lane-0 sections the saved pair restores later (which only
/// move uniform values through LDS) run in every lane, with the same
/// results. Becomes:
///
/// ```text
/// s_mov_b64 exec, -1
/// s_mov_b64 s[8:9], exec
/// v_readfirstlane_b32 sX, v5
/// s_lshr_b32 sX, sX, 7
/// v_mov_b32 v5, sX
/// v_mov_b32 v0, 0
/// s_branch E
/// ```
///
/// Code without the prologue is left as it is.
fn group_prologue(code: &[u8]) -> Result<Vec<u8>, ShaderError> {
    let insts = gfx10::decode(code)?;
    let text: Vec<String> = insts.iter().map(|i| i.text(true)).collect();
    let Some(k) = text.iter().position(|t| t == "s_mov_b64 exec, 1") else {
        return Ok(code.to_vec());
    };
    let saved = text.get(k + 1).is_some_and(|t| t == "s_mov_b64 s[8:9], exec");
    let at = k + 1 + usize::from(saved);
    let Some(x) = text
        .get(at)
        .and_then(|t| t.strip_prefix("s_and_b32 s")?.strip_suffix(", s3, lit(0x0f000000)")?.parse::<u8>().ok())
    else {
        return Ok(code.to_vec());
    };
    let want = [
        "s_cbranch_scc1 ",
        "v_lshrrev_b32 v5, 7, v5",
        "v_mov_b32 v0, 0",
        "ds_write_b32 v0, v5 offset:8",
        "s_waitcnt_depctr 0xffe3",
        "s_barrier",
        "s_branch ",
        "s_barrier",
        "v_mov_b32 v0, 0",
        "ds_read_b32 v5, v0 offset:8",
    ];
    let fits = want.iter().enumerate().all(|(n, w)| {
        text.get(at + 1 + n).is_some_and(|t| if w.ends_with(' ') { t.starts_with(w) } else { t == w })
    });
    if !fits || at + 1 + want.len() >= insts.len() {
        return Ok(code.to_vec());
    }
    let end = insts[at + 1 + want.len()].offset;
    // sX: written before it is read from E on.
    let sx = format!("s{x}");
    let mentions = |t: &str| t.split([' ', ',']).any(|w| w == sx);
    let first = text[at + 1 + want.len()..].iter().find(|t| mentions(t));
    if !first.is_some_and(|t| t.starts_with(&format!("s_mov_b32 {sx}, "))) {
        return Ok(code.to_vec());
    }
    let mut out = code.to_vec();
    let mut put = |offset: usize, word: u32| {
        out[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    };
    let x = u32::from(x);
    let and = &insts[at];
    let branch = &insts[at + 3];
    let jump = (end as i64 - (branch.offset as i64 + 4)) / 4;
    // s_mov_b64 exec, -1 (inline constant 0xc1 for 1's 0x81).
    put(insts[k].offset, insts[k].words[0] & !0xff | 0xc1);
    // In the 8 bytes of the s_and (it has a literal): v_readfirstlane_b32
    // sX, v5 (VOP1 2); s_lshr_b32 sX, sX, 7 (SOP2 32).
    put(and.offset, 0x7e00_0000 | x << 17 | 2 << 9 | 0x105);
    put(and.offset + 4, 0x8000_0000 | 32 << 23 | x << 16 | 0x87 << 8 | x);
    // v_mov_b32 v5, sX (VOP1 1), then the v_mov_b32 v0, 0 as it was.
    put(insts[at + 1].offset, 0x7e00_0000 | 5 << 17 | 1 << 9 | x);
    put(insts[at + 2].offset, insts[at + 3].words[0]);
    // s_branch E (SOPP 2).
    put(branch.offset, 0xbf82_0000 | (jump as u32 & 0xffff));
    // As intended, or nothing.
    let check = gfx10::decode(&out)?;
    let at_offset = |o: usize| check.iter().find(|i| i.offset == o).map(|i| i.text(true));
    let expect = [
        (insts[k].offset, "s_mov_b64 exec, -1".to_owned()),
        (and.offset, format!("v_readfirstlane_b32 {sx}, v5")),
        (and.offset + 4, format!("s_lshr_b32 {sx}, {sx}, 7")),
        (insts[at + 1].offset, format!("v_mov_b32 v5, {sx}")),
        (insts[at + 2].offset, "v_mov_b32 v0, 0".to_owned()),
    ];
    for (o, t) in expect {
        if at_offset(o).as_deref() != Some(t.as_str()) {
            return Err(Malformed("serialized mesh stage: the prologue rewrite did not decode as intended"));
        }
    }
    if !at_offset(branch.offset).is_some_and(|t| t.starts_with("s_branch")) {
        return Err(Malformed("serialized mesh stage: the prologue rewrite did not decode as intended"));
    }
    Ok(out)
}

/// The stage's code, as long as its metadata says (the blob has 0x28 bytes
/// more: the hash and padding).
fn stage_code<'a>(s: StageParts<'a>) -> Result<&'a [u8], ShaderError> {
    if u32_at(s.meta, 0)? != 2 || s.meta.len() < 0x10 {
        return Err(Malformed("serialized stage metadata: unknown tag"));
    }
    let dwords = u32_at(s.meta, 8)? as usize;
    s.code.get(..4 * dwords).ok_or(Malformed("serialized stage: code shorter than its metadata says"))
}

/// The root layout and the resources a stage's metadata records list
/// (32-byte records from +0x10; see [`crate::native::fspx_program`]).
fn records(meta: &[u8], user_sgprs: u32, root: &RootLayout) -> Result<(Vec<[u8; 20]>, Vec<[u8; 20]>), ShaderError> {
    let le = |e: &[u8], at: usize| u32::from_le_bytes(e[at..at + 4].try_into().expect("4 bytes"));
    let mut found: Vec<(u8, u8, u8, u32, u32, u32, u8, u8)> = Vec::new(); // kind, root, dwords, table, register, stride, user data, slot
    for e in meta.get(0x10..).unwrap_or(&[]).chunks_exact(32) {
        match le(e, 0) {
            0x41 => found.push((e[4], e[5], e[20], le(e, 8), le(e, 12), le(e, 16), e[21], e[22])),
            0x42 => {}
            0x43 => break,
            _ => return Err(Malformed("serialized stage metadata: unknown record")),
        }
    }
    // Where each root parameter's user data starts, by slot.
    let mut params: BTreeMap<u8, (u8, u8)> = BTreeMap::new(); // slot -> (root, user data)
    for f in &found {
        if matches!(f.1, 0 | 1 | 3) {
            params.entry(f.7).or_insert((f.1, f.6));
        }
    }
    let entry = |root: u32, sgprs: u32, base: u32, ud: u32, slot: u8| {
        let mut e = [0u8; 20];
        e[0..4].copy_from_slice(&root.to_le_bytes());
        e[8..12].copy_from_slice(&sgprs.to_le_bytes());
        e[12..16].copy_from_slice(&base.to_le_bytes());
        e[16] = ud as u8;
        e[17] = slot;
        e
    };
    let mut layout = Vec::new();
    for (&slot, &(kind, ud)) in &params {
        if root.user_data(slot) != Some(u32::from(ud)) {
            return Err(Malformed("serialized stage metadata: user data disagrees with the root signature"));
        }
        let sgprs = root.sizes[usize::from(slot)];
        let base = found.iter().find(|f| f.7 == slot).map_or(0, |f| f.4);
        layout.push(entry(u32::from(kind), sgprs, base, u32::from(ud), slot));
    }
    // The heap addresses, where the stage gets them.
    let mut at = root.end();
    for (on, kind) in [(root.resource_heap, 5), (root.sampler_heap, 6)] {
        if on {
            if at < user_sgprs {
                layout.push(entry(kind, 1, 0, at, 0));
            }
            at += 1;
        }
    }
    let used = found
        .iter()
        .map(|&(kind, root, dwords, table, register, stride, ud, slot)| {
            let mut e = [0u8; 20];
            e[0] = kind;
            e[1] = root;
            e[4..8].copy_from_slice(&table.to_le_bytes());
            e[8..12].copy_from_slice(&register.to_le_bytes());
            e[12..16].copy_from_slice(&stride.to_le_bytes());
            e[16] = dwords;
            e[17] = ud;
            e[18] = slot;
            e
        })
        .collect();
    Ok((layout, used))
}

struct MeshInfo {
    threads: u32,
    max_vertices: u32,
    max_primitives: u32,
    user_sgprs: u8,
    topology: u8,
    lds_bytes: u32,
    clip_cntl: u32,
}

struct Record<'a> {
    code: &'a [u8],
    wave32: bool,
    vgprs: u16,
    layout: Vec<[u8; 20]>,
    used: Vec<[u8; 20]>,
}

#[derive(Clone, Copy)]
enum Slot {
    Ps,
    Ms,
}

// FSSM stage data offsets (from the record's end; [`crate::native`]).
const PS_INPUTS: usize = 0x24;
const PS_INPUT_COUNT: usize = 0x124;
const PS_INPUT_ADDR: usize = 0x134;
const PS_PATCHES: usize = 0x14c;
const PATCH_BYTES: usize = 4 + 10 * 10 * 4;
const VS_OUTPUTS: usize = 0xc8;
const VS_OUTPUT_COUNT: usize = 0x188;
const MS_REGS: usize = 0x18c;
const MS_INFO: usize = 0x448;

fn put(data: &mut [u8], at: usize, v: u32) {
    data[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn pixel_stage_data(inputs: &[(u8, u8, u8, u8)], input_addr: u32) -> Vec<u8> {
    let mut d = vec![0u8; PS_PATCHES + 8 * PATCH_BYTES];
    for (k, &(row, attr, row2, flags)) in inputs.iter().enumerate().take(32) {
        let at = PS_INPUTS + 8 * k;
        d[at..at + 8].copy_from_slice(&[6, row, 0xf, attr, 0, row2, attr, flags]);
    }
    put(&mut d, PS_INPUT_COUNT, inputs.len().min(32) as u32);
    put(&mut d, PS_INPUT_ADDR, input_addr);
    for mrt in 0..8 {
        put(&mut d, PS_PATCHES + mrt * PATCH_BYTES, u32::MAX);
    }
    d
}

fn mesh_stage_data(outputs: &[(u8, u8, u8, u8)], m: &MeshInfo) -> Vec<u8> {
    let mut d = vec![0u8; MS_INFO + 0x14];
    for (k, &(usage, row, param, flags)) in outputs.iter().enumerate().take(32) {
        let at = VS_OUTPUTS + 6 * k;
        d[at..at + 6].copy_from_slice(&[usage, row, param, row, param, flags]);
    }
    put(&mut d, VS_OUTPUT_COUNT, outputs.len().min(32) as u32);
    put(&mut d, MS_REGS + 8, m.clip_cntl);
    let info = MS_INFO;
    d[info..info + 2].copy_from_slice(&(m.threads as u16).to_le_bytes());
    d[info + 2..info + 4].copy_from_slice(&(m.max_vertices as u16).to_le_bytes());
    d[info + 4..info + 6].copy_from_slice(&(m.max_primitives as u16).to_le_bytes());
    d[info + 6] = m.user_sgprs;
    d[info + 7] = m.topology;
    d
}

/// An unpacked `FSSM` blob holding one program ([`crate::native`]'s
/// layout); `lds` goes in the record (+0x4c), where mesh programs keep it.
fn fssm(slot: Slot, r: &Record, stage_data: &[u8], lds: u32) -> Vec<u8> {
    const HEADER: usize = 0x68;
    const RECORD: usize = 0x54;
    let record_at = HEADER;
    let layout_at = record_at + RECORD + stage_data.len();
    let used_at = layout_at + 20 * r.layout.len();
    let code_at = (used_at + 20 * r.used.len()).next_multiple_of(0x100);
    let mut payload = vec![0u8; code_at + r.code.len()];
    payload[0..8].copy_from_slice(&6u64.to_le_bytes());
    for at in (0x08..HEADER).step_by(8) {
        payload[at..at + 8].copy_from_slice(&0xDEAD_BEEF_DEAD_BEEFu64.to_le_bytes());
    }
    let slot_at = match slot {
        Slot::Ps => 0x30,
        Slot::Ms => 0x48,
    };
    payload[slot_at..slot_at + 8].copy_from_slice(&(record_at as u64).to_le_bytes());
    let rec = &mut payload[record_at..record_at + RECORD];
    rec[0..8].copy_from_slice(&(code_at as u64).to_le_bytes());
    rec[8..16].copy_from_slice(&(r.code.len() as u64).to_le_bytes());
    put(rec, 0x14, if r.wave32 { 1 << 5 } else { 0 });
    rec[0x28..0x30].copy_from_slice(&(layout_at as u64).to_le_bytes());
    rec[0x30..0x38].copy_from_slice(&(used_at as u64).to_le_bytes());
    put(rec, 0x38, r.layout.len() as u32);
    put(rec, 0x3c, r.used.len() as u32);
    put(rec, 0x4c, lds);
    put(rec, 0x50, u32::from(r.vgprs));
    let sd = record_at + RECORD;
    payload[sd..sd + stage_data.len()].copy_from_slice(&stage_data);
    for (k, e) in r.layout.iter().enumerate() {
        payload[layout_at + 20 * k..layout_at + 20 * k + 20].copy_from_slice(e);
    }
    for (k, e) in r.used.iter().enumerate() {
        payload[used_at + 20 * k..used_at + 20 * k + 20].copy_from_slice(e);
    }
    payload[code_at..].copy_from_slice(r.code);
    let mut out = Vec::with_capacity(0x10 + payload.len());
    out.extend(b"FSSM");
    out.extend(0x101u32.to_le_bytes());
    out.extend((payload.len() as u64).to_le_bytes());
    out.extend(payload);
    out
}

/// A console-layout signature part.
fn signature(rows: &[Row]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((rows.len() as u32).to_le_bytes());
    out.extend(8u32.to_le_bytes());
    let mut names = Vec::new();
    let mut offsets = Vec::new();
    let mut next = 8 + 32 * rows.len() as u32;
    for r in rows {
        offsets.push(next);
        next += r.name.len() as u32 + 1;
        names.push(r.name);
    }
    for (r, &offset) in rows.iter().zip(&offsets) {
        for v in [0, offset, r.index, r.system, r.comp, r.register] {
            out.extend(v.to_le_bytes());
        }
        out.extend([r.mask, r.mask, 0, 0]);
        out.extend(0u32.to_le_bytes());
    }
    for n in names {
        out.extend(n.as_bytes());
        out.push(0);
    }
    out
}

/// The container: a `DXIL` part whose `XDAT` index holds the unpacked
/// `FSSM` blob, the signatures, and the color export format.
fn container(fssm: &[u8], col_format: Option<u32>, inputs: &[Row], outputs: &[Row], pixel: bool) -> Vec<u8> {
    let mut dxil = vec![0u8; 0x18];
    dxil.extend(b"XDAT");
    dxil.extend(1u32.to_le_bytes());
    dxil.extend(0x24u32.to_le_bytes());
    let blob_at = 0x30u32;
    dxil.extend(0x0cu32.to_le_bytes());
    dxil.extend(blob_at.to_le_bytes());
    dxil.extend((fssm.len() as u32).to_le_bytes());
    dxil.extend(fssm);
    let isg1 = signature(inputs);
    let osg1 = signature(outputs);
    let psg1 = signature(&[]);
    let vxcf = col_format.map(u32::to_le_bytes);
    let mut parts: Vec<(&[u8; 4], &[u8])> = vec![(b"ISG1", &isg1), (b"OSG1", &osg1)];
    if !pixel {
        parts.push((b"PSG1", &psg1));
    }
    if let Some(v) = &vxcf {
        parts.push((&COLOR_FORMAT_PART, v));
    }
    parts.push((b"DXIL", &dxil));
    crate::emit::container_build(&parts)
}

fn u16_at(data: &[u8], at: usize) -> Result<u16, ShaderError> {
    data.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Malformed("serialized packet out of bounds"))
}
