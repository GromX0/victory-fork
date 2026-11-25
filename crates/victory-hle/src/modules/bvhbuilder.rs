//! The title's ray-tracing structure builder, `XboxBvhBuilder.dll`, seen
//! from outside (added 2026-11-25, `docs/raytracing-plan.md` step 1).
//!
//! The DLL ships with the title and runs as its code. It is a C++ class
//! whose methods take the standard D3D12 ray-tracing descriptions
//! (`D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC` and its inputs,
//! geometry descriptions and post-build requests) plus two Xbox ones, and
//! record the builder's compute work into the title's command list. The
//! title imports seven of its twelve exports ([`ENTRIES`]).
//!
//! With `VICTORY_BVH_TRACE` the title's imports of those seven are bound
//! to wrappers here ([`Traced`]): each reads what the call asks for, notes
//! it, and calls the DLL's own function with the same arguments. Nothing
//! the title or the DLL sees changes. What it is for:
//!
//! - which memory each build may write (its destination and scratch, with
//!   their sizes when a size query for the same inputs came first), the
//!   ranges a guard on the builder's stores needs;
//! - how much geometry the title builds, in which formats, how often, and
//!   how much of the world arrives ready built (rehydrated): what a host
//!   build of the same inputs would take.
//!
//! `VICTORY_BVH_TRACE=1` logs the first 32 calls of each entry point in
//! full and a summary line every 10 seconds; `=<n>` the first `n` calls;
//! `=all` every call. Unset or `0`: the imports bind straight to the DLL,
//! as before.
//!
//! Layouts. The public D3D12 ones, confirmed in the DLL's own code
//! (release of 2026-11-25, image base 0x180000000): the non-batched build
//! (RVA 0x21610) copies a 48-byte description into a batched one and
//! calls the batched build; the batched build (0x216a0) walks its
//! descriptions 80 bytes apart and their post-build requests (count at
//! byte 48, array at byte 56) 16 bytes apart; the size query (0x21880)
//! copies 24 bytes of inputs and 16 of its Xbox stream description; the
//! rehydration check (0x21a00) reads the inputs' count, layout and array
//! at bytes 16, 20 and 24 of a build description and its geometry
//! descriptions 56 bytes apart (or through pointers).
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use victory_loader::{ImportResolver, Provenance, Resolution};
use victory_pe::ImportSymbol;

/// The DLL, as the title's import table names it (compared ignoring case).
pub const DLL: &str = "xboxbvhbuilder.dll";

/// The entry points the title imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    /// `XboxBvhBuilder(ID3D12Device11*, bool)`.
    New = 0,
    /// `~XboxBvhBuilder()`.
    Drop,
    /// `BuildRaytracingAccelerationStructureBatched2X(list, count, descs)`.
    Build,
    /// `CopyRaytracingAccelerationStructure(list, dest, source, mode)`.
    Copy,
    /// `GetRaytracingAccelerationStructurePrebuildInfo(inputs, stream, info)`.
    Prebuild,
    /// `RehydrateRaytracingAccelerationStructureBatched2X(list, count, descs, keys)`.
    Rehydrate,
    /// `Set64BitMortonCodes(bool)`.
    Morton,
}

const COUNT: usize = 7;

/// The title's imports of the DLL by their decorated names, as
/// `victory inspect --imports` lists them for the title.
const ENTRIES: [(Entry, &str); COUNT] = [
    (
        Entry::New,
        "??0XboxBvhBuilder@@QEAA@PEAUID3D12Device11@@_N@Z",
    ),
    (Entry::Drop, "??1XboxBvhBuilder@@QEAA@XZ"),
    (
        Entry::Build,
        "?BuildRaytracingAccelerationStructureBatched2X@XboxBvhBuilder@@QEAAXPEAUID3D12GraphicsCommandList6@@IPEBUD3D12XBOX_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_BATCHED_DESC2@@@Z",
    ),
    (
        Entry::Copy,
        "?CopyRaytracingAccelerationStructure@XboxBvhBuilder@@QEAAXPEAUID3D12GraphicsCommandList4@@_K1W4D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE@@@Z",
    ),
    (
        Entry::Prebuild,
        "?GetRaytracingAccelerationStructurePrebuildInfo@XboxBvhBuilder@@QEAAXPEBUD3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS@@PEBUD3D12XBOX_XDXR_STREAM_DESC@@PEAUD3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO@@@Z",
    ),
    (
        Entry::Rehydrate,
        "?RehydrateRaytracingAccelerationStructureBatched2X@XboxBvhBuilder@@QEAAJPEAUID3D12GraphicsCommandList8@@IPEBUD3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC@@PEB_K@Z",
    ),
    (
        Entry::Morton,
        "?Set64BitMortonCodes@XboxBvhBuilder@@QEAAX_N@Z",
    ),
];

fn entry_of(name: &str) -> Option<Entry> {
    ENTRIES.iter().find(|(_, n)| *n == name).map(|&(e, _)| e)
}

/// Bytes of the layouts read here.
const BUILD_DESC: usize = 48;
const BATCHED_DESC: usize = 80;
const INPUTS: usize = 24;
const GEOMETRY_DESC: usize = 56;
const POSTBUILD_DESC: usize = 16;
const PREBUILD_INFO: usize = 24;
const STREAM_DESC: usize = 16;

/// `D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE`.
const TOP_LEVEL: u32 = 0;
/// `D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PERFORM_UPDATE`.
const PERFORM_UPDATE: u32 = 0x20;
/// `D3D12_ELEMENTS_LAYOUT_ARRAY_OF_POINTERS` (else an array).
const ARRAY_OF_POINTERS: u32 = 1;
/// `D3D12_RAYTRACING_GEOMETRY_TYPE`.
const TRIANGLES: u32 = 0;
const PROCEDURAL_AABBS: u32 = 1;
/// `D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE_COMPACT`: the one
/// mode the DLL's copy acts on (RVA 0x21980 returns for any other).
const COMPACT: u32 = 1;
/// Most geometry descriptions of one build looked at (a garbage count
/// must not walk off into memory).
const MOST_GEOMETRIES: u32 = 1 << 16;
/// Most post-build requests of one build looked at.
const MOST_POSTBUILD: u32 = 64;
/// Most structures and size answers kept.
const MOST_KEPT: usize = 1 << 16;
/// Geometry descriptions spelled out in a detailed line.
const SPELLED: usize = 4;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// `D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Inputs {
    /// 0 top level (instances), 1 bottom level (geometry).
    kind: u32,
    flags: u32,
    /// Instances or geometry descriptions.
    count: u32,
    layout: u32,
    /// A top level's instance descriptions (a GPU address, not read here)
    /// or a bottom level's geometry descriptions (a CPU pointer, or a
    /// pointer to pointers).
    descs: u64,
}

impl Inputs {
    fn parse(b: &[u8]) -> Inputs {
        Inputs {
            kind: u32_at(b, 0),
            flags: u32_at(b, 4),
            count: u32_at(b, 8),
            layout: u32_at(b, 12),
            descs: u64_at(b, 16),
        }
    }

    fn top(&self) -> bool {
        self.kind == TOP_LEVEL
    }

    fn update(&self) -> bool {
        self.flags & PERFORM_UPDATE != 0
    }
}

/// `D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuildDesc {
    dest: u64,
    inputs: Inputs,
    /// The structure updated from (an update), or the serialized one
    /// rehydrated from.
    source: u64,
    scratch: u64,
}

impl BuildDesc {
    fn parse(b: &[u8]) -> BuildDesc {
        BuildDesc {
            dest: u64_at(b, 0),
            inputs: Inputs::parse(&b[8..8 + INPUTS]),
            source: u64_at(b, 32),
            scratch: u64_at(b, 40),
        }
    }
}

/// `D3D12XBOX_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_BATCHED_DESC2` as far
/// as the DLL's code shows it: a build description, then its post-build
/// requests. Bytes 52-55 and 64-79 are not known (the non-batched build
/// leaves them zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BatchedDesc {
    desc: BuildDesc,
    postbuild_count: u32,
    postbuild: u64,
}

impl BatchedDesc {
    fn parse(b: &[u8]) -> BatchedDesc {
        BatchedDesc {
            desc: BuildDesc::parse(&b[..BUILD_DESC]),
            postbuild_count: u32_at(b, 48),
            postbuild: u64_at(b, 56),
        }
    }
}

/// `D3D12_RAYTRACING_GEOMETRY_DESC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Geometry {
    Triangles {
        flags: u32,
        transform: u64,
        index_format: u32,
        vertex_format: u32,
        index_count: u32,
        vertex_count: u32,
        index_buffer: u64,
        vertex_start: u64,
        vertex_stride: u64,
    },
    Aabbs {
        flags: u32,
        count: u64,
        start: u64,
        stride: u64,
    },
    /// A type the public headers do not have.
    Other { kind: u32, flags: u32 },
}

impl Geometry {
    fn parse(b: &[u8]) -> Geometry {
        let (kind, flags) = (u32_at(b, 0), u32_at(b, 4));
        match kind {
            TRIANGLES => Geometry::Triangles {
                flags,
                transform: u64_at(b, 8),
                index_format: u32_at(b, 16),
                vertex_format: u32_at(b, 20),
                index_count: u32_at(b, 24),
                vertex_count: u32_at(b, 28),
                index_buffer: u64_at(b, 32),
                vertex_start: u64_at(b, 40),
                vertex_stride: u64_at(b, 48),
            },
            PROCEDURAL_AABBS => Geometry::Aabbs {
                flags,
                count: u64_at(b, 8),
                start: u64_at(b, 16),
                stride: u64_at(b, 24),
            },
            _ => Geometry::Other { kind, flags },
        }
    }

    /// Triangles it holds: by its indices, or its vertices without.
    fn triangles(&self) -> u64 {
        match *self {
            Geometry::Triangles {
                index_format: 0,
                vertex_count,
                ..
            } => u64::from(vertex_count / 3),
            Geometry::Triangles { index_count, .. } => u64::from(index_count / 3),
            _ => 0,
        }
    }

    fn aabbs(&self) -> u64 {
        match *self {
            Geometry::Aabbs { count, .. } => count,
            _ => 0,
        }
    }

    /// What a size query must have been asked with for the same sizes
    /// (`D3D12` asks a build to use the query's inputs).
    fn hash_into(&self, h: &mut Fnv) {
        match *self {
            Geometry::Triangles {
                flags,
                transform,
                index_format,
                vertex_format,
                index_count,
                vertex_count,
                ..
            } => {
                h.add(u64::from(TRIANGLES));
                h.add(u64::from(flags));
                h.add(u64::from(transform != 0));
                h.add(u64::from(index_format));
                h.add(u64::from(vertex_format));
                h.add(u64::from(index_count));
                h.add(u64::from(vertex_count));
            }
            Geometry::Aabbs { flags, count, .. } => {
                h.add(u64::from(PROCEDURAL_AABBS));
                h.add(u64::from(flags));
                h.add(count);
            }
            Geometry::Other { kind, flags } => {
                h.add(u64::from(kind));
                h.add(u64::from(flags));
            }
        }
    }

    fn spell(&self) -> String {
        match *self {
            Geometry::Triangles {
                flags,
                transform,
                index_format,
                vertex_format,
                index_count,
                vertex_count,
                index_buffer,
                vertex_start,
                vertex_stride,
            } => format!(
                "triangles {} (vertices {vertex_count} {} stride {vertex_stride} at {vertex_start:#x}; \
                 indices {index_count} {} at {index_buffer:#x}; transform {transform:#x}; flags {flags:#x})",
                self.triangles(),
                format_name(vertex_format),
                format_name(index_format),
            ),
            Geometry::Aabbs {
                flags,
                count,
                start,
                stride,
            } => format!("boxes {count} stride {stride} at {start:#x} (flags {flags:#x})"),
            Geometry::Other { kind, flags } => format!("type {kind} (flags {flags:#x})"),
        }
    }
}

/// The DXGI formats a geometry description is likely to name.
fn format_name(format: u32) -> String {
    match format {
        0 => "none".into(),
        6 => "R32G32B32_FLOAT".into(),
        10 => "R16G16B16A16_FLOAT".into(),
        13 => "R16G16B16A16_SNORM".into(),
        16 => "R32G32_FLOAT".into(),
        34 => "R16G16_FLOAT".into(),
        37 => "R16G16_SNORM".into(),
        42 => "R32_UINT".into(),
        57 => "R16_UINT".into(),
        f => format!("format {f}"),
    }
}

/// FNV-1a over 64-bit words.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn add(&mut self, word: u64) {
        for byte in word.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// What a build of `inputs` over `geometries` asks the size query: the
/// same inputs but for the update flag.
fn signature(inputs: &Inputs, geometries: &[Geometry]) -> u64 {
    let mut h = Fnv::new();
    h.add(u64::from(inputs.kind));
    h.add(u64::from(inputs.flags & !PERFORM_UPDATE));
    h.add(u64::from(inputs.count));
    for g in geometries {
        g.hash_into(&mut h);
    }
    h.0
}

/// `D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Sizes {
    result: u64,
    scratch: u64,
    update_scratch: u64,
}

impl Sizes {
    fn parse(b: &[u8]) -> Sizes {
        Sizes {
            result: u64_at(b, 0),
            scratch: u64_at(b, 8),
            update_scratch: u64_at(b, 16),
        }
    }
}

/// `n` bytes at `p`, if `p` is not null.
///
/// # Safety
/// `p` is null or holds `N` readable bytes (the title passes the DLL
/// pointers it reads at once).
unsafe fn read<const N: usize>(p: *const u8) -> Option<[u8; N]> {
    // SAFETY: as the caller says.
    (!p.is_null()).then(|| unsafe { p.cast::<[u8; N]>().read_unaligned() })
}

/// The geometry descriptions of bottom-level inputs, at most
/// [`MOST_GEOMETRIES`]; empty for a top level.
///
/// # Safety
/// The inputs are a live call's: their array (or array of pointers) holds
/// `count` descriptions.
unsafe fn geometries(inputs: &Inputs) -> Vec<Geometry> {
    if inputs.top() || inputs.descs == 0 {
        return Vec::new();
    }
    let n = inputs.count.min(MOST_GEOMETRIES) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let at = if inputs.layout == ARRAY_OF_POINTERS {
            // SAFETY: the array holds `count` pointers.
            let p = unsafe { read::<8>((inputs.descs as *const u8).add(8 * i)) };
            p.map_or(std::ptr::null(), |p| u64::from_le_bytes(p) as *const u8)
        } else {
            (inputs.descs as *const u8).wrapping_add(GEOMETRY_DESC * i)
        };
        // SAFETY: each description is 56 bytes; a null pointer is skipped.
        if let Some(b) = unsafe { read::<GEOMETRY_DESC>(at) } {
            out.push(Geometry::parse(&b));
        }
    }
    out
}

/// What was built at a destination address.
#[derive(Debug, Clone, Copy)]
struct Structure {
    top: bool,
    /// Its size, from a size query with the same inputs.
    bytes: Option<u64>,
    builds: u64,
    rehydrated: bool,
    /// Made by a compacting copy.
    compacted: bool,
}

/// Counts over one summary line's time.
#[derive(Debug, Default)]
struct Window {
    calls: [u64; COUNT],
    bottom: u64,
    bottom_updates: u64,
    top: u64,
    top_updates: u64,
    geometries: u64,
    triangles: u64,
    aabbs: u64,
    instances: u64,
    rehydrated: u64,
    rehydrated_geometries: u64,
    rehydrated_triangles: u64,
    compactions: u64,
    other_copies: u64,
    /// Post-build requests by type (compacted size, tools, serialization,
    /// current size, anything else).
    postbuild: [u64; 5],
    /// Builds whose destination size a size query told, and those it did
    /// not.
    sized: u64,
    unsized_: u64,
    /// The largest bottom level (triangles) and top level (instances).
    largest_bottom: u64,
    largest_top: u64,
    /// Vertex and index formats of triangle geometry, by geometry.
    vertex_formats: BTreeMap<u32, u64>,
    index_formats: BTreeMap<u32, u64>,
    /// Geometry that is neither triangles nor boxes.
    other_geometry: u64,
}

struct State {
    window: Window,
    last_line: Instant,
    /// Calls logged in full, per entry point.
    spelled: [u64; COUNT],
    /// Size answers by [`signature`].
    sizes: HashMap<u64, Sizes>,
    /// Structures by destination address.
    structures: HashMap<u64, Structure>,
    /// Morton code widths asked for.
    morton: Vec<bool>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// How often the summary line is written.
const LINE_EVERY: Duration = Duration::from_secs(10);

/// `VICTORY_BVH_TRACE`: 0 off, else how many calls of each entry point
/// are logged in full.
fn level() -> u64 {
    static LEVEL: OnceLock<u64> = OnceLock::new();
    *LEVEL.get_or_init(|| parse_level(std::env::var("VICTORY_BVH_TRACE").ok().as_deref()))
}

fn parse_level(v: Option<&str>) -> u64 {
    match v.map(str::trim) {
        None | Some("" | "0") => 0,
        Some("all") => u64::MAX,
        Some("1") => 32,
        Some(n) => n.parse().unwrap_or(32),
    }
}

/// Runs `f` on the state, then writes the summary line if it is due.
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.get_or_insert_with(|| State {
        window: Window::default(),
        last_line: Instant::now(),
        spelled: [0; COUNT],
        sizes: HashMap::new(),
        structures: HashMap::new(),
        morton: Vec::new(),
    });
    let out = f(state);
    if state.last_line.elapsed() >= LINE_EVERY {
        summary(state);
        state.last_line = Instant::now();
        state.window = Window::default();
    }
    out
}

/// Whether this call of `entry` is logged in full.
fn spell(state: &mut State, entry: Entry) -> bool {
    let n = &mut state.spelled[entry as usize];
    *n += 1;
    *n <= level()
}

fn summary(state: &State) {
    let w = &state.window;
    let seconds = state.last_line.elapsed().as_secs_f64().max(1e-3);
    let (mut tops, mut bottoms, mut sized, mut bytes, mut rehydrated, mut compacted) =
        (0, 0, 0, 0u64, 0, 0);
    for s in state.structures.values() {
        if s.top {
            tops += 1;
        } else {
            bottoms += 1;
        }
        if let Some(b) = s.bytes {
            sized += 1;
            bytes += b;
        }
        rehydrated += u32::from(s.rehydrated);
        compacted += u32::from(s.compacted);
    }
    let formats = |m: &BTreeMap<u32, u64>| {
        m.iter()
            .map(|(&f, &n)| format!("{} {n}", format_name(f)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    tracing::info!(
        seconds = format_args!("{seconds:.1}"),
        calls = ?w.calls,
        bottom_builds = w.bottom,
        bottom_updates = w.bottom_updates,
        top_builds = w.top,
        top_updates = w.top_updates,
        geometries = w.geometries,
        triangles = w.triangles,
        boxes = w.aabbs,
        instances = w.instances,
        largest_bottom_triangles = w.largest_bottom,
        largest_top_instances = w.largest_top,
        rehydrated = w.rehydrated,
        rehydrated_geometries = w.rehydrated_geometries,
        rehydrated_triangles = w.rehydrated_triangles,
        compactions = w.compactions,
        other_copies = w.other_copies,
        postbuild = ?w.postbuild,
        destinations_sized = w.sized,
        destinations_unsized = w.unsized_,
        other_geometry = w.other_geometry,
        vertex_formats = %formats(&w.vertex_formats),
        index_formats = %formats(&w.index_formats),
        "the ray-tracing structure builder's calls since the line before (VICTORY_BVH_TRACE; calls: new, drop, \
         build, copy, size query, rehydrate, Morton width; postbuild: compacted size, tools, serialization, \
         current size, other)"
    );
    tracing::info!(
        top_levels = tops,
        bottom_levels = bottoms,
        with_a_known_size = sized,
        known_mib = bytes >> 20,
        rehydrated,
        compacted,
        size_answers = state.sizes.len(),
        morton_widths = ?state.morton,
        "the ray-tracing structures built so far, by destination address (VICTORY_BVH_TRACE)"
    );
}

/// Notes one build description (a build or a rehydration) and logs it in
/// full if `spelled`.
///
/// # Safety
/// `desc` is a live call's: its geometry descriptions are readable.
unsafe fn note_desc(state: &mut State, desc: &BuildDesc, rehydrate: bool, spelled: Option<String>) {
    let inputs = &desc.inputs;
    // SAFETY: as the caller says.
    let geometry = unsafe { geometries(inputs) };
    let triangles: u64 = geometry.iter().map(Geometry::triangles).sum();
    let aabbs: u64 = geometry.iter().map(Geometry::aabbs).sum();
    let w = &mut state.window;
    if rehydrate {
        w.rehydrated += 1;
        w.rehydrated_geometries += geometry.len() as u64;
        w.rehydrated_triangles += triangles;
    } else if inputs.top() {
        w.top += 1;
        w.top_updates += u64::from(inputs.update());
        w.instances += u64::from(inputs.count);
        w.largest_top = w.largest_top.max(u64::from(inputs.count));
    } else {
        w.bottom += 1;
        w.bottom_updates += u64::from(inputs.update());
        w.geometries += geometry.len() as u64;
        w.triangles += triangles;
        w.aabbs += aabbs;
        w.largest_bottom = w.largest_bottom.max(triangles);
    }
    for g in &geometry {
        match *g {
            Geometry::Triangles {
                vertex_format,
                index_format,
                ..
            } => {
                *w.vertex_formats.entry(vertex_format).or_default() += 1;
                *w.index_formats.entry(index_format).or_default() += 1;
            }
            Geometry::Other { .. } => w.other_geometry += 1,
            Geometry::Aabbs { .. } => {}
        }
    }
    let sizes = state.sizes.get(&signature(inputs, &geometry)).copied();
    if sizes.is_some() {
        w.sized += 1;
    } else {
        w.unsized_ += 1;
    }
    if state.structures.len() < MOST_KEPT || state.structures.contains_key(&desc.dest) {
        let s = state.structures.entry(desc.dest).or_insert(Structure {
            top: inputs.top(),
            bytes: None,
            builds: 0,
            rehydrated: false,
            compacted: false,
        });
        s.top = inputs.top();
        s.builds += 1;
        s.rehydrated |= rehydrate;
        if let Some(sizes) = sizes {
            s.bytes = Some(sizes.result);
        }
    }
    if let Some(what) = spelled {
        let spelled: Vec<String> = geometry.iter().take(SPELLED).map(Geometry::spell).collect();
        tracing::info!(
            what,
            level = if inputs.top() { "top" } else { "bottom" },
            dest = format_args!("{:#x}", desc.dest),
            bytes = ?sizes.map(|s| s.result),
            source = format_args!("{:#x}", desc.source),
            scratch = format_args!("{:#x}", desc.scratch),
            scratch_bytes = ?sizes.map(|s| if inputs.update() { s.update_scratch } else { s.scratch }),
            flags = format_args!("{:#x}", inputs.flags),
            count = inputs.count,
            layout = inputs.layout,
            descs = format_args!("{:#x}", inputs.descs),
            triangles,
            boxes = aabbs,
            geometry = %spelled.join("; "),
            "the ray-tracing structure builder: a structure (VICTORY_BVH_TRACE)"
        );
    }
}

/// The DLL's own entry points, by [`Entry`] (0 until bound).
static REAL: [AtomicUsize; COUNT] = [const { AtomicUsize::new(0) }; COUNT];

fn real(entry: Entry) -> Option<usize> {
    let a = REAL[entry as usize].load(Ordering::Acquire);
    (a != 0).then_some(a)
}

type NewFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u8) -> *mut c_void;
type DropFn = unsafe extern "system" fn(*mut c_void);
type BuildFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *const u8);
type CopyFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u64, u64, u32);
type PrebuildFn = unsafe extern "system" fn(*mut c_void, *const u8, *const u8, *mut u8);
type RehydrateFn =
    unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *const u8, *const u64) -> i32;
type MortonFn = unsafe extern "system" fn(*mut c_void, u8);

/// The constructor: `this`, the device, and a flag (when set, the DLL
/// skips making one of its two inner builders: RVA 0x2136f). Returns
/// `this`, as MSVC constructors do.
unsafe extern "system" fn new(this: *mut c_void, device: *mut c_void, flag: u8) -> *mut c_void {
    tracing::info!(
        this = ?this,
        device = ?device,
        flag,
        "the ray-tracing structure builder is made (VICTORY_BVH_TRACE)"
    );
    with_state(|s| s.window.calls[Entry::New as usize] += 1);
    let Some(f) = real(Entry::New) else {
        return this;
    };
    // SAFETY: the DLL's own constructor, bound for this import.
    let f: NewFn = unsafe { std::mem::transmute::<usize, NewFn>(f) };
    // SAFETY: the title's arguments, passed on.
    unsafe { f(this, device, flag) }
}

unsafe extern "system" fn release(this: *mut c_void) {
    tracing::info!(this = ?this, "the ray-tracing structure builder is let go (VICTORY_BVH_TRACE)");
    with_state(|s| s.window.calls[Entry::Drop as usize] += 1);
    let Some(f) = real(Entry::Drop) else { return };
    // SAFETY: as in `new`.
    let f: DropFn = unsafe { std::mem::transmute::<usize, DropFn>(f) };
    // SAFETY: as in `new`.
    unsafe { f(this) }
}

/// `count` batched descriptions at `descs`, 80 bytes apart.
unsafe extern "system" fn build(
    this: *mut c_void,
    list: *mut c_void,
    count: u32,
    descs: *const u8,
) {
    with_state(|state| {
        state.window.calls[Entry::Build as usize] += 1;
        let spelled = spell(state, Entry::Build);
        for i in 0..count.min(MOST_GEOMETRIES) as usize {
            // SAFETY: the title passes `count` descriptions.
            let Some(b) = (unsafe { read::<BATCHED_DESC>(descs.wrapping_add(BATCHED_DESC * i)) })
            else {
                break;
            };
            let batched = BatchedDesc::parse(&b);
            for k in 0..batched.postbuild_count.min(MOST_POSTBUILD) as usize {
                let at = (batched.postbuild as *const u8).wrapping_add(POSTBUILD_DESC * k);
                // SAFETY: the description's requests, as the DLL walks them.
                let Some(p) = (unsafe { read::<POSTBUILD_DESC>(at) }) else {
                    break;
                };
                let kind = u32_at(&p, 8) as usize;
                state.window.postbuild[kind.min(4)] += 1;
            }
            let what = spelled.then(|| format!("build {} of {count}, list {list:?}", i + 1));
            // SAFETY: a live call's description.
            unsafe { note_desc(state, &batched.desc, false, what) };
        }
    });
    let Some(f) = real(Entry::Build) else { return };
    // SAFETY: as in `new`.
    let f: BuildFn = unsafe { std::mem::transmute::<usize, BuildFn>(f) };
    // SAFETY: as in `new`.
    unsafe { f(this, list, count, descs) }
}

unsafe extern "system" fn copy(
    this: *mut c_void,
    list: *mut c_void,
    dest: u64,
    source: u64,
    mode: u32,
) {
    with_state(|state| {
        state.window.calls[Entry::Copy as usize] += 1;
        if mode == COMPACT {
            state.window.compactions += 1;
        } else {
            state.window.other_copies += 1;
        }
        let from = state.structures.get(&source).copied();
        if mode == COMPACT
            && (state.structures.len() < MOST_KEPT || state.structures.contains_key(&dest))
        {
            let s = state.structures.entry(dest).or_insert(Structure {
                top: from.is_some_and(|f| f.top),
                bytes: None,
                builds: 0,
                rehydrated: false,
                compacted: true,
            });
            s.compacted = true;
            s.builds += 1;
        }
        if spell(state, Entry::Copy) {
            tracing::info!(
                dest = format_args!("{dest:#x}"),
                source = format_args!("{source:#x}"),
                mode,
                source_known = from.is_some(),
                source_bytes = ?from.and_then(|f| f.bytes),
                list = ?list,
                "the ray-tracing structure builder: a copy (mode 1 compacts; VICTORY_BVH_TRACE)"
            );
        }
    });
    let Some(f) = real(Entry::Copy) else { return };
    // SAFETY: as in `new`.
    let f: CopyFn = unsafe { std::mem::transmute::<usize, CopyFn>(f) };
    // SAFETY: as in `new`.
    unsafe { f(this, list, dest, source, mode) }
}

/// The size query: asked first, the answer noted after.
unsafe extern "system" fn prebuild(
    this: *mut c_void,
    inputs: *const u8,
    stream: *const u8,
    info: *mut u8,
) {
    if let Some(f) = real(Entry::Prebuild) {
        // SAFETY: as in `new`.
        let f: PrebuildFn = unsafe { std::mem::transmute::<usize, PrebuildFn>(f) };
        // SAFETY: as in `new`.
        unsafe { f(this, inputs, stream, info) }
    }
    // SAFETY: the title passes its inputs and room for the answer (the DLL
    // fails on a null answer: RVA 0x21899).
    let (Some(i), Some(out)) = (unsafe { read::<INPUTS>(inputs) }, unsafe {
        read::<PREBUILD_INFO>(info)
    }) else {
        return;
    };
    let inputs = Inputs::parse(&i);
    let sizes = Sizes::parse(&out);
    // SAFETY: the stream description is 16 bytes, or null.
    let stream = unsafe { read::<STREAM_DESC>(stream) };
    // SAFETY: a live call's inputs.
    let geometry = unsafe { geometries(&inputs) };
    with_state(|state| {
        state.window.calls[Entry::Prebuild as usize] += 1;
        let key = signature(&inputs, &geometry);
        if state.sizes.len() < MOST_KEPT || state.sizes.contains_key(&key) {
            state.sizes.insert(key, sizes);
        }
        if spell(state, Entry::Prebuild) {
            tracing::info!(
                level = if inputs.top() { "top" } else { "bottom" },
                flags = format_args!("{:#x}", inputs.flags),
                count = inputs.count,
                triangles = geometry.iter().map(Geometry::triangles).sum::<u64>(),
                result_bytes = sizes.result,
                scratch_bytes = sizes.scratch,
                update_scratch_bytes = sizes.update_scratch,
                stream = ?stream.map(|s| (format!("{:#x}", u64_at(&s, 0)), format!("{:#x}", u64_at(&s, 8)))),
                "the ray-tracing structure builder: a size query (VICTORY_BVH_TRACE)"
            );
        }
    });
}

/// `count` build descriptions at `descs`, 48 bytes apart, and `keys`, one
/// 64-bit value per description as far as known.
unsafe extern "system" fn rehydrate(
    this: *mut c_void,
    list: *mut c_void,
    count: u32,
    descs: *const u8,
    keys: *const u64,
) -> i32 {
    with_state(|state| {
        state.window.calls[Entry::Rehydrate as usize] += 1;
        let spelled = spell(state, Entry::Rehydrate);
        for i in 0..count.min(MOST_GEOMETRIES) as usize {
            // SAFETY: the title passes `count` descriptions.
            let Some(b) = (unsafe { read::<BUILD_DESC>(descs.wrapping_add(BUILD_DESC * i)) })
            else {
                break;
            };
            let desc = BuildDesc::parse(&b);
            let what = spelled.then(|| {
                // SAFETY: as many keys as descriptions, or none.
                let key = (!keys.is_null()).then(|| unsafe { keys.add(i).read_unaligned() });
                format!(
                    "rehydration {} of {count}, key {key:x?}, list {list:?}",
                    i + 1
                )
            });
            // SAFETY: a live call's description.
            unsafe { note_desc(state, &desc, true, what) };
        }
    });
    let Some(f) = real(Entry::Rehydrate) else {
        return 0;
    };
    // SAFETY: as in `new`.
    let f: RehydrateFn = unsafe { std::mem::transmute::<usize, RehydrateFn>(f) };
    // SAFETY: as in `new`.
    let hr = unsafe { f(this, list, count, descs, keys) };
    if hr < 0 {
        tracing::warn!(
            hr = format_args!("{hr:#x}"),
            count,
            "the ray-tracing structure builder refused a rehydration (VICTORY_BVH_TRACE)"
        );
    }
    hr
}

unsafe extern "system" fn morton(this: *mut c_void, wide: u8) {
    with_state(|state| {
        state.window.calls[Entry::Morton as usize] += 1;
        if !state.morton.contains(&(wide != 0)) {
            state.morton.push(wide != 0);
        }
    });
    let Some(f) = real(Entry::Morton) else { return };
    // SAFETY: as in `new`.
    let f: MortonFn = unsafe { std::mem::transmute::<usize, MortonFn>(f) };
    // SAFETY: as in `new`.
    unsafe { f(this, wide) }
}

fn wrapper(entry: Entry) -> usize {
    match entry {
        Entry::New => new as *const () as usize,
        Entry::Drop => release as *const () as usize,
        Entry::Build => build as *const () as usize,
        Entry::Copy => copy as *const () as usize,
        Entry::Prebuild => prebuild as *const () as usize,
        Entry::Rehydrate => rehydrate as *const () as usize,
        Entry::Morton => morton as *const () as usize,
    }
}

/// The wrapper for the DLL's export `name` at `real`, which it will call;
/// `None` for an export not wrapped here.
fn interpose(name: &str, real: usize) -> Option<usize> {
    let entry = entry_of(name)?;
    REAL[entry as usize].store(real, Ordering::Release);
    Some(wrapper(entry))
}

/// Resolves through `inner`; with `VICTORY_BVH_TRACE`, what it finds for
/// the title's imports of the structure builder is the wrapper that logs
/// the call and calls it (see the module).
pub struct Traced<'a> {
    inner: &'a dyn ImportResolver,
    on: bool,
}

impl<'a> Traced<'a> {
    pub fn new(inner: &'a dyn ImportResolver) -> Traced<'a> {
        let on = level() > 0;
        if on {
            tracing::info!(
                spelled = level(),
                "the ray-tracing structure builder's entry points are traced (VICTORY_BVH_TRACE)"
            );
        }
        Traced { inner, on }
    }
}

impl ImportResolver for Traced<'_> {
    fn resolve(&self, dll: &str, symbol: &ImportSymbol) -> Resolution {
        let found = self.inner.resolve(dll, symbol);
        if !self.on || !dll.eq_ignore_ascii_case(DLL) {
            return found;
        }
        match (found, symbol) {
            (
                Resolution::Found {
                    address,
                    provenance: Provenance::Title,
                },
                ImportSymbol::Name { name, .. },
            ) => match interpose(name, address) {
                Some(address) => Resolution::Found {
                    address,
                    provenance: Provenance::Title,
                },
                None => found,
            },
            _ => found,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put64(b: &mut [u8], at: usize, v: u64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// A triangle geometry description: `indices` 16-bit indices over
    /// `vertices` float3 vertices.
    fn triangles(indices: u32, vertices: u32) -> [u8; GEOMETRY_DESC] {
        let mut g = [0u8; GEOMETRY_DESC];
        put32(&mut g, 0, TRIANGLES);
        put32(&mut g, 4, 1);
        put32(&mut g, 16, 57);
        put32(&mut g, 20, 6);
        put32(&mut g, 24, indices);
        put32(&mut g, 28, vertices);
        put64(&mut g, 32, 0x10_0000_1000);
        put64(&mut g, 40, 0x10_0000_2000);
        put64(&mut g, 48, 12);
        g
    }

    fn build_desc(dest: u64, kind: u32, flags: u32, count: u32, descs: u64) -> [u8; BUILD_DESC] {
        let mut d = [0u8; BUILD_DESC];
        put64(&mut d, 0, dest);
        put32(&mut d, 8, kind);
        put32(&mut d, 12, flags);
        put32(&mut d, 16, count);
        put32(&mut d, 20, 0);
        put64(&mut d, 24, descs);
        put64(&mut d, 32, 0x77);
        put64(&mut d, 40, 0x10_0040_0000);
        d
    }

    #[test]
    fn a_build_description_reads_the_d3d12_layout() {
        let d = BuildDesc::parse(&build_desc(0x10_0000_0100, 1, 0x6, 3, 0x1234));
        assert_eq!(d.dest, 0x10_0000_0100);
        assert_eq!(
            d.inputs,
            Inputs {
                kind: 1,
                flags: 6,
                count: 3,
                layout: 0,
                descs: 0x1234
            }
        );
        assert_eq!((d.source, d.scratch), (0x77, 0x10_0040_0000));
        assert!(!d.inputs.top() && !d.inputs.update());
    }

    #[test]
    fn a_batched_description_has_its_requests_at_48_and_56() {
        let mut b = [0u8; BATCHED_DESC];
        b[..BUILD_DESC].copy_from_slice(&build_desc(0x100, 0, 0x20, 9, 0x5000));
        put32(&mut b, 48, 2);
        put64(&mut b, 56, 0xabcd);
        let batched = BatchedDesc::parse(&b);
        assert_eq!(batched.desc.dest, 0x100);
        assert!(batched.desc.inputs.top() && batched.desc.inputs.update());
        assert_eq!((batched.postbuild_count, batched.postbuild), (2, 0xabcd));
    }

    #[test]
    fn geometry_descriptions_read_triangles_and_boxes() {
        let t = Geometry::parse(&triangles(300, 120));
        assert_eq!(t.triangles(), 100);
        let Geometry::Triangles {
            vertex_format,
            index_format,
            vertex_stride,
            vertex_start,
            ..
        } = t
        else {
            panic!("{t:?}")
        };
        assert_eq!((vertex_format, index_format, vertex_stride), (6, 57, 12));
        assert_eq!(vertex_start, 0x10_0000_2000);
        // Without indices: by vertices.
        let mut raw = triangles(0, 99);
        put32(&mut raw, 16, 0);
        assert_eq!(Geometry::parse(&raw).triangles(), 33);
        let mut b = [0u8; GEOMETRY_DESC];
        put32(&mut b, 0, PROCEDURAL_AABBS);
        put64(&mut b, 8, 7);
        put64(&mut b, 16, 0x9000);
        put64(&mut b, 24, 24);
        let boxes = Geometry::parse(&b);
        assert_eq!((boxes.aabbs(), boxes.triangles()), (7, 0));
        put32(&mut b, 0, 5);
        assert!(matches!(
            Geometry::parse(&b),
            Geometry::Other { kind: 5, .. }
        ));
    }

    #[test]
    fn the_size_signature_ignores_only_the_update_flag() {
        let g = [Geometry::parse(&triangles(30, 12))];
        let inputs = Inputs {
            kind: 1,
            flags: 0x2,
            count: 1,
            layout: 0,
            descs: 1,
        };
        let update = Inputs {
            flags: 0x22,
            descs: 2,
            ..inputs
        };
        assert_eq!(signature(&inputs, &g), signature(&update, &g));
        let other = Inputs {
            flags: 0x4,
            ..inputs
        };
        assert_ne!(signature(&inputs, &g), signature(&other, &g));
        let more = [Geometry::parse(&triangles(33, 12))];
        assert_ne!(signature(&inputs, &g), signature(&inputs, &more));
    }

    #[test]
    fn the_trace_level_reads_counts_and_all() {
        assert_eq!(parse_level(None), 0);
        assert_eq!(parse_level(Some("0")), 0);
        assert_eq!(parse_level(Some("1")), 32);
        assert_eq!(parse_level(Some("5")), 5);
        assert_eq!(parse_level(Some("all")), u64::MAX);
        assert_eq!(parse_level(Some("yes")), 32);
    }

    #[test]
    fn only_the_builders_exports_have_wrappers() {
        let mut seen = Vec::new();
        for (entry, name) in ENTRIES {
            assert_eq!(entry_of(name), Some(entry));
            let w = wrapper(entry);
            assert!(!seen.contains(&w), "{name}");
            seen.push(w);
        }
        // Exported by the DLL, not imported by the title: left alone.
        assert_eq!(
            entry_of(
                "?CpuValidateResultX@XboxBvhBuilder@@QEAAXPEAUID3D12GraphicsCommandList4@@PEBUD3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC@@@Z"
            ),
            None
        );
        assert_eq!(interpose("memcpy", 1), None);
    }

    struct Fake;

    impl ImportResolver for Fake {
        fn resolve(&self, dll: &str, _: &ImportSymbol) -> Resolution {
            Resolution::Found {
                address: 0x1234,
                provenance: if dll.eq_ignore_ascii_case(DLL) {
                    Provenance::Title
                } else {
                    Provenance::Hle
                },
            }
        }
    }

    #[test]
    fn the_traced_resolver_changes_only_the_builders_imports() {
        let name = |n: &str| ImportSymbol::Name {
            hint: 0,
            name: n.to_owned(),
        };
        let off = Traced {
            inner: &Fake,
            on: false,
        };
        let on = Traced {
            inner: &Fake,
            on: true,
        };
        let found = |address, provenance| Resolution::Found {
            address,
            provenance,
        };
        let morton = name(ENTRIES[Entry::Morton as usize].1);
        assert_eq!(
            off.resolve("XboxBvhBuilder.dll", &morton),
            found(0x1234, Provenance::Title)
        );
        assert_eq!(
            on.resolve("XboxBvhBuilder.dll", &morton),
            found(wrapper(Entry::Morton), Provenance::Title)
        );
        assert_eq!(
            on.resolve("kernel32.dll", &morton),
            found(0x1234, Provenance::Hle)
        );
        assert_eq!(
            on.resolve("XboxBvhBuilder.dll", &name("Other")),
            found(0x1234, Provenance::Title)
        );
    }

    /// What the stand-ins for the DLL were called with.
    static CALLED: Mutex<Vec<(usize, u64, u64)>> = Mutex::new(Vec::new());

    unsafe extern "system" fn fake_build(
        this: *mut c_void,
        list: *mut c_void,
        count: u32,
        descs: *const u8,
    ) {
        CALLED.lock().unwrap().push((
            this as usize ^ list as usize,
            u64::from(count),
            descs as u64,
        ));
    }

    unsafe extern "system" fn fake_prebuild(
        _: *mut c_void,
        _: *const u8,
        _: *const u8,
        info: *mut u8,
    ) {
        let mut out = [0u8; PREBUILD_INFO];
        put64(&mut out, 0, 0x4_0000);
        put64(&mut out, 8, 0x1_0000);
        put64(&mut out, 16, 0x800);
        // SAFETY: the test passes room for the answer.
        unsafe { info.copy_from_nonoverlapping(out.as_ptr(), PREBUILD_INFO) };
    }

    /// A size query, then a build of the same inputs, through the
    /// wrappers: the DLL's functions get the title's arguments, and the
    /// build's destination gets the size the query answered.
    #[test]
    fn wrapped_calls_reach_the_dll_and_a_build_gets_its_queried_size() {
        REAL[Entry::Build as usize].store(fake_build as *const () as usize, Ordering::Release);
        REAL[Entry::Prebuild as usize]
            .store(fake_prebuild as *const () as usize, Ordering::Release);
        let geometry: Vec<u8> = [triangles(300, 120), triangles(6, 4)].concat();
        let dest = 0x10_0123_4500;
        let desc = build_desc(dest, 1, 0x2, 2, geometry.as_ptr() as u64);
        let mut info = [0u8; PREBUILD_INFO];
        // SAFETY: the inputs are the description's bytes 8-31; no stream.
        unsafe {
            prebuild(
                std::ptr::null_mut(),
                desc[8..].as_ptr(),
                std::ptr::null(),
                info.as_mut_ptr(),
            );
        }
        assert_eq!(Sizes::parse(&info).result, 0x4_0000);
        let mut batched = [0u8; BATCHED_DESC];
        batched[..BUILD_DESC].copy_from_slice(&desc);
        let (this, list) = (0x1000 as *mut c_void, 0x30 as *mut c_void);
        // SAFETY: one batched description whose geometry is `geometry`.
        unsafe { build(this, list, 1, batched.as_ptr()) };
        assert!(
            CALLED
                .lock()
                .unwrap()
                .contains(&(0x1030, 1, batched.as_ptr() as u64))
        );
        let s = with_state(|s| s.structures.get(&dest).copied()).unwrap();
        assert!(!s.top && s.builds >= 1);
        assert_eq!(s.bytes, Some(0x4_0000));
    }
}
