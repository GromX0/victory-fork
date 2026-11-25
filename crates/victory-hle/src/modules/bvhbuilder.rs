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

use victory_gpu::bvhguard::Named;
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
    /// Its bytes before and from its address ([`found`]).
    extent: Option<(u64, u64)>,
    builds: u64,
    rehydrated: bool,
    /// Made by a compacting copy.
    compacted: bool,
    /// When it was last built, and last checked (`VICTORY_BVH_CHECK`).
    built: Instant,
    checked: Option<Instant>,
}

/// Where a build's sizes came from ([`found`]), in the summary's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizedBy {
    /// A size query with the same inputs ([`signature`]).
    Exact = 0,
    /// A size query with the same level, flags, count and primitives
    /// ([`loose_key`]): the title's queries for geometry with index format
    /// 53 never match exactly (run `20261125-130040`), though their answers
    /// are what it packs destinations by.
    Loose,
    /// An earlier build at the same destination (an update, a rebuild in
    /// place, a rehydration).
    Earlier,
    /// The structure's header in title memory (rehydrations: the title
    /// loaded it there).
    Header,
    /// The smallest answer for the same level, flags and count with at
    /// least as many primitives ([`capacity_key`]): a top level built with
    /// fewer instances than it was asked about.
    Capacity,
    Nothing,
}

const SIZED_BY: usize = 6;

/// What a build's ranges are ([`found`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Found {
    /// Bytes before and from the destination.
    dest: Option<(u64, u64)>,
    scratch: Option<u64>,
    by: SizedBy,
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
    /// Builds by where their sizes came from ([`SizedBy`]).
    sized_by: [u64; SIZED_BY],
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
    /// Size answers by [`signature`], by [`loose_key`], and by
    /// [`capacity_key`] with the primitives asked about (the latest
    /// [`MOST_CAPACITIES`]).
    sizes: HashMap<u64, Sizes>,
    loose: HashMap<u64, Sizes>,
    capacity: HashMap<u64, Vec<(u64, Sizes)>>,
    /// Structures by destination address.
    structures: HashMap<u64, Structure>,
    /// Morton code widths asked for.
    morton: Vec<bool>,
}

impl State {
    fn new() -> State {
        State {
            window: Window::default(),
            last_line: Instant::now(),
            spelled: [0; COUNT],
            sizes: HashMap::new(),
            loose: HashMap::new(),
            capacity: HashMap::new(),
            structures: HashMap::new(),
            morton: Vec::new(),
        }
    }
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
    let state = guard.get_or_insert_with(State::new);
    let out = f(state);
    if state.last_line.elapsed() >= LINE_EVERY {
        // With only the guard on, the counts are kept and not logged.
        if level() > 0 {
            summary(state);
        }
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
        if let Some((below, from)) = s.extent {
            sized += 1;
            bytes += below + from;
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
        sized_by = ?w.sized_by,
        other_geometry = w.other_geometry,
        vertex_formats = %formats(&w.vertex_formats),
        index_formats = %formats(&w.index_formats),
        "the ray-tracing structure builder's calls since the line before (VICTORY_BVH_TRACE; calls: new, drop, \
         build, copy, size query, rehydrate, Morton width; postbuild: compacted size, tools, serialization, \
         current size, other; sized_by: exact query, loose query, earlier build, header, capacity, nothing)"
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

/// Most answers kept per [`capacity_key`].
const MOST_CAPACITIES: usize = 64;

/// A top level's instances, or a bottom level's triangles and boxes.
fn primitives(inputs: &Inputs, geometry: &[Geometry]) -> u64 {
    if inputs.top() {
        u64::from(inputs.count)
    } else {
        geometry.iter().map(|g| g.triangles() + g.aabbs()).sum()
    }
}

/// A size query's inputs as far as sizes depend on them: level, flags but
/// the update flag, count, primitives.
fn loose_key(inputs: &Inputs, primitives: u64) -> u64 {
    let mut h = Fnv::new();
    for word in [
        u64::from(inputs.kind),
        u64::from(inputs.flags & !PERFORM_UPDATE),
        u64::from(inputs.count),
        primitives,
    ] {
        h.add(word);
    }
    h.0
}

/// Level and flags, and a bottom level's geometry count (a top level's
/// count is its capacity, compared in [`found`]).
fn capacity_key(inputs: &Inputs) -> u64 {
    let mut h = Fnv::new();
    h.add(u64::from(inputs.kind));
    h.add(u64::from(inputs.flags & !PERFORM_UPDATE));
    h.add(if inputs.top() {
        0
    } else {
        u64::from(inputs.count)
    });
    h.add(0xca9a);
    h.0
}

/// Notes a size query's answer.
fn note_sizes(state: &mut State, inputs: &Inputs, geometry: &[Geometry], sizes: Sizes) {
    let key = signature(inputs, geometry);
    if state.sizes.len() < MOST_KEPT || state.sizes.contains_key(&key) {
        state.sizes.insert(key, sizes);
    }
    let prims = primitives(inputs, geometry);
    let key = loose_key(inputs, prims);
    if state.loose.len() < MOST_KEPT || state.loose.contains_key(&key) {
        state.loose.insert(key, sizes);
    }
    let key = capacity_key(inputs);
    if state.capacity.len() < MOST_KEPT || state.capacity.contains_key(&key) {
        let v = state.capacity.entry(key).or_default();
        v.retain(|&(p, _)| p != prims);
        if v.len() >= MOST_CAPACITIES {
            v.remove(0);
        }
        v.push((prims, sizes));
    }
}

/// The ranges a build or rehydration writes, from the sources
/// [`SizedBy`] lists. A new build trusts only size answers. An update or
/// a rehydration works on a structure that is there already: an earlier
/// build's extent, or its header in title memory (read with
/// `read_header`), first.
fn found(
    state: &State,
    desc: &BuildDesc,
    geometry: &[Geometry],
    rehydrate: bool,
    read_header: impl Fn(u64) -> Option<(u64, u64)>,
) -> Found {
    let inputs = &desc.inputs;
    let prims = primitives(inputs, geometry);
    let scratch_of = |s: &Sizes| {
        if inputs.update() {
            s.update_scratch
        } else {
            s.scratch
        }
    };
    let asked = state
        .sizes
        .get(&signature(inputs, geometry))
        .map(|s| (*s, SizedBy::Exact))
        .or_else(|| {
            state
                .loose
                .get(&loose_key(inputs, prims))
                .map(|s| (*s, SizedBy::Loose))
        });
    // May reach past the title's allocation (another structure's answer):
    // after an earlier build's extent.
    let capacity = || {
        let v = state.capacity.get(&capacity_key(inputs))?;
        let s = v
            .iter()
            .filter(|(p, _)| *p >= prims)
            .min_by_key(|(_, s)| s.result)?;
        Some((s.1, SizedBy::Capacity))
    };
    let query = asked.or_else(capacity);
    let scratch = query.map(|(s, _)| scratch_of(&s));
    let answered = |(s, by): (Sizes, SizedBy)| Found {
        dest: Some((0, s.result)),
        scratch: Some(scratch_of(&s)),
        by,
    };
    let earlier = state.structures.get(&desc.dest).and_then(|s| s.extent);
    if !rehydrate && !inputs.update() {
        if let Some(q) = asked {
            return answered(q);
        }
        // Built again where it was (the title rebuilds some bottom levels
        // in place every frame).
        if let Some(extent) = earlier {
            return Found {
                dest: Some(extent),
                scratch,
                by: SizedBy::Earlier,
            };
        }
        return capacity().map_or(
            Found {
                dest: None,
                scratch: None,
                by: SizedBy::Nothing,
            },
            answered,
        );
    }
    if let Some(extent) = earlier {
        return Found {
            dest: Some(extent),
            scratch,
            by: SizedBy::Earlier,
        };
    }
    if let Some(extent) = read_header(desc.dest) {
        return Found {
            dest: Some(extent),
            scratch,
            by: SizedBy::Header,
        };
    }
    query.map_or(
        Found {
            dest: None,
            scratch,
            by: SizedBy::Nothing,
        },
        answered,
    )
}

/// The structure header at title address `at`: its extent, if it holds a
/// plausible one.
fn header_at(at: u64) -> Option<(u64, u64)> {
    let mut bytes = [0u8; 64];
    victory_gpu::bvhguard::read_title(at, &mut bytes)
        .then(|| victory_gpu::bvhguard::header_extent(&bytes))
        .flatten()
}

/// Bytes of an index of the DXGI format `format` (the title's 53, which no
/// public index format is, taken as 16-bit like 57).
fn index_bytes(format: u32) -> u64 {
    match format {
        42 => 4,
        _ => 2,
    }
}

/// What a build description reads: its instances, or its geometry's
/// vertices, indices, transforms and boxes; and its source structure.
fn reads_of(state: &State, desc: &BuildDesc, geometry: &[Geometry]) -> Vec<Named> {
    let inputs = &desc.inputs;
    let mut out = Vec::new();
    if desc.source != 0 && desc.source != desc.dest {
        let extent = state.structures.get(&desc.source).and_then(|s| s.extent);
        out.push(Named {
            at: desc.source - extent.map_or(0, |e| e.0),
            bytes: extent.map(|(below, from)| below + from),
        });
    }
    if inputs.top() {
        out.push(Named {
            at: inputs.descs,
            bytes: Some(64 * u64::from(inputs.count)),
        });
        return out;
    }
    for g in geometry {
        match *g {
            Geometry::Triangles {
                transform,
                index_format,
                index_count,
                vertex_count,
                index_buffer,
                vertex_start,
                vertex_stride,
                ..
            } => {
                // The stride's high bits carry something of the Xbox's
                // (strides of 0x8008 next to 8s).
                out.push(Named {
                    at: vertex_start,
                    bytes: Some(u64::from(vertex_count) * (vertex_stride & 0x7fff)),
                });
                out.push(Named {
                    at: index_buffer,
                    bytes: Some(u64::from(index_count) * index_bytes(index_format)),
                });
                out.push(Named {
                    at: transform,
                    bytes: Some(48),
                });
            }
            Geometry::Aabbs {
                count,
                start,
                stride,
                ..
            } => out.push(Named {
                at: start,
                bytes: Some(count.saturating_mul(stride)),
            }),
            Geometry::Other { .. } => {}
        }
    }
    out
}

/// Notes one build description (a build or a rehydration), logs it in
/// full if `spelled`, and adds what its work may write and reads to
/// `writes` and `reads` (`victory_gpu::bvhguard`).
///
/// # Safety
/// `desc` is a live call's: its geometry descriptions are readable.
unsafe fn note_desc(
    state: &mut State,
    desc: &BuildDesc,
    rehydrate: bool,
    spelled: Option<String>,
    writes: &mut Vec<Named>,
    reads: &mut Vec<Named>,
) {
    let inputs = &desc.inputs;
    // SAFETY: as the caller says.
    let geometry = unsafe { geometries(inputs) };
    let triangles: u64 = geometry.iter().map(Geometry::triangles).sum();
    let aabbs: u64 = geometry.iter().map(Geometry::aabbs).sum();
    let found = found(state, desc, &geometry, rehydrate, header_at);
    writes.push(match found.dest {
        Some((below, from)) => Named {
            at: desc.dest.saturating_sub(below),
            bytes: Some(below + from),
        },
        None => Named {
            at: desc.dest,
            bytes: None,
        },
    });
    if desc.scratch != 0 {
        writes.push(Named {
            at: desc.scratch,
            bytes: found.scratch,
        });
    }
    reads.extend(reads_of(state, desc, &geometry));
    let w = &mut state.window;
    w.sized_by[found.by as usize] += 1;
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
    if state.structures.len() < MOST_KEPT || state.structures.contains_key(&desc.dest) {
        let s = state.structures.entry(desc.dest).or_insert(Structure {
            top: inputs.top(),
            extent: None,
            builds: 0,
            rehydrated: false,
            compacted: false,
            built: Instant::now(),
            checked: None,
        });
        s.top = inputs.top();
        s.builds += 1;
        s.rehydrated |= rehydrate;
        s.built = Instant::now();
        if found.dest.is_some() {
            s.extent = found.dest;
        }
    }
    if let Some(what) = spelled {
        let spelled: Vec<String> = geometry.iter().take(SPELLED).map(Geometry::spell).collect();
        tracing::info!(
            what,
            level = if inputs.top() { "top" } else { "bottom" },
            dest = format_args!("{:#x}", desc.dest),
            bytes = ?found.dest,
            sized_by = ?found.by,
            source = format_args!("{:#x}", desc.source),
            scratch = format_args!("{:#x}", desc.scratch),
            scratch_bytes = ?found.scratch,
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

/// The classes of structures checked ([`check_loop`]): top levels, bottom
/// levels the builder built, bottom levels rehydrated. Rehydrated ones come
/// ready built in the title's data, so they show whether the checker reads
/// the format right; the others show whether the builder wrote it right.
const CLASSES: [&str; 3] = [
    "top levels",
    "bottom levels built",
    "bottom levels rehydrated",
];

fn class_of(s: &Structure) -> usize {
    match (s.top, s.rehydrated) {
        (true, _) => 0,
        (false, false) => 1,
        (false, true) => 2,
    }
}

/// How long after its build a structure is left before it is read: its
/// work has run and come back to title memory by then.
const SETTLE: Duration = Duration::from_secs(1);

/// What to check now, within `budget` bytes: top levels first, then the
/// structures built longest ago, each once after each build. With the
/// bottom levels by address (bytes before and from it) for the instances.
fn pick(
    state: &State,
    budget: u64,
) -> (
    Vec<(u64, Structure)>,
    std::collections::BTreeMap<u64, (u64, u64)>,
) {
    let now = Instant::now();
    let mut due: Vec<(u64, Structure)> = state
        .structures
        .iter()
        .filter(|(_, s)| {
            now.duration_since(s.built) >= SETTLE && s.checked.is_none_or(|c| c < s.built)
        })
        .map(|(&a, s)| (a, *s))
        .collect();
    due.sort_unstable_by_key(|(_, s)| (!s.top, s.built));
    let mut bytes = 0;
    due.retain(|(_, s)| {
        let take = bytes < budget;
        bytes += s.extent.map_or(64 << 10, |(below, from)| below + from);
        take
    });
    let bottoms = state
        .structures
        .iter()
        .filter(|(_, s)| !s.top)
        .map(|(&a, s)| (a, s.extent.unwrap_or((0, 0))))
        .collect();
    (due, bottoms)
}

/// For an instance's bottom-level address: whether a call named a bottom
/// level that holds it, and the bytes left from it if known.
fn bottom_at(
    bottoms: &std::collections::BTreeMap<u64, (u64, u64)>,
    address: u64,
) -> Option<Option<u64>> {
    let (&at, &(_, from)) = bottoms.range(..=address).next_back()?;
    if address == at {
        return Some((from > 0).then_some(from));
    }
    (address < at + from).then(|| Some(at + from - address))
}

/// The thread of `VICTORY_BVH_CHECK`: once a second it checks what
/// [`pick`] gives, and every 10 seconds it logs each class.
fn check_loop() {
    let budget = victory_gpu::bvhcheck::budget();
    let builder = std::env::var("VICTORY_BVH_BUILDER").unwrap_or_default();
    let mut all: [victory_gpu::bvhcheck::Report; 3] = Default::default();
    let mut window: [victory_gpu::bvhcheck::Report; 3] = Default::default();
    let mut last_line = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let (due, bottoms) = with_state(|s| pick(s, budget));
        let mut checked = Vec::with_capacity(due.len());
        for (at, s) in &due {
            let (r, _) =
                victory_gpu::bvhcheck::check(*at, s.top, s.extent, &|a| bottom_at(&bottoms, a));
            let class = class_of(s);
            all[class].add(&r);
            window[class].add(&r);
            checked.push((*at, s.built));
        }
        let now = Instant::now();
        with_state(|state| {
            for (at, built) in checked {
                if let Some(s) = state.structures.get_mut(&at)
                    && s.built == built
                {
                    s.checked = Some(now);
                }
            }
        });
        if last_line.elapsed() >= LINE_EVERY {
            last_line = Instant::now();
            for (k, class) in CLASSES.iter().enumerate() {
                victory_gpu::bvhcheck::log(class, &builder, &all[k], &window[k]);
            }
            window = Default::default();
        }
    }
}

/// Starts [`check_loop`] once, with `VICTORY_BVH_CHECK`.
fn start_checks() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if !victory_gpu::bvhcheck::on() {
        return;
    }
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("victory-bvh-check".to_owned())
            .spawn(check_loop);
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "the ray-tracing structure checks did not start (VICTORY_BVH_CHECK)");
        }
    });
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
    // Buffers it makes meanwhile are its own (`VICTORY_BVH_GUARD`).
    victory_gpu::bvhguard::enter("the builder's constructor", &[], &[]);
    // SAFETY: the title's arguments, passed on.
    let made = unsafe { f(this, device, flag) };
    victory_gpu::bvhguard::leave();
    made
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
    let (mut writes, mut reads) = (Vec::new(), Vec::new());
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
                // The public results are 8 or 16 bytes; the Xbox's own
                // type (0x10, RVA 0x2177f) is not known.
                writes.push(Named {
                    at: u64_at(&p, 0),
                    bytes: Some(if kind <= 3 { 16 } else { 64 }),
                });
            }
            let what = spelled.then(|| format!("build {} of {count}, list {list:?}", i + 1));
            // SAFETY: a live call's description.
            unsafe { note_desc(state, &batched.desc, false, what, &mut writes, &mut reads) };
        }
    });
    let Some(f) = real(Entry::Build) else { return };
    // SAFETY: as in `new`.
    let f: BuildFn = unsafe { std::mem::transmute::<usize, BuildFn>(f) };
    victory_gpu::bvhguard::enter("a build", &writes, &reads);
    // SAFETY: as in `new`.
    unsafe { f(this, list, count, descs) };
    victory_gpu::bvhguard::leave();
}

unsafe extern "system" fn copy(
    this: *mut c_void,
    list: *mut c_void,
    dest: u64,
    source: u64,
    mode: u32,
) {
    // A compacted copy is no larger than its source.
    let extent = with_state(|state| {
        state.window.calls[Entry::Copy as usize] += 1;
        if mode == COMPACT {
            state.window.compactions += 1;
        } else {
            state.window.other_copies += 1;
        }
        let from = state.structures.get(&source).copied();
        let extent = from.and_then(|f| f.extent);
        if mode == COMPACT
            && (state.structures.len() < MOST_KEPT || state.structures.contains_key(&dest))
        {
            let s = state.structures.entry(dest).or_insert(Structure {
                top: from.is_some_and(|f| f.top),
                extent: None,
                builds: 0,
                rehydrated: false,
                compacted: true,
                built: Instant::now(),
                checked: None,
            });
            s.compacted = true;
            s.builds += 1;
            s.extent = extent;
            s.built = Instant::now();
        }
        if spell(state, Entry::Copy) {
            tracing::info!(
                dest = format_args!("{dest:#x}"),
                source = format_args!("{source:#x}"),
                mode,
                source_known = from.is_some(),
                source_extent = ?extent,
                list = ?list,
                "the ray-tracing structure builder: a copy (mode 1 compacts; VICTORY_BVH_TRACE)"
            );
        }
        extent
    });
    let Some(f) = real(Entry::Copy) else { return };
    // SAFETY: as in `new`.
    let f: CopyFn = unsafe { std::mem::transmute::<usize, CopyFn>(f) };
    let (below, bytes) = (extent.map_or(0, |e| e.0), extent.map(|(b, f)| b + f));
    victory_gpu::bvhguard::enter(
        "a copy",
        &[Named {
            at: dest.saturating_sub(below),
            bytes,
        }],
        &[Named {
            at: source.saturating_sub(below),
            bytes,
        }],
    );
    // SAFETY: as in `new`.
    unsafe { f(this, list, dest, source, mode) };
    victory_gpu::bvhguard::leave();
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
        note_sizes(state, &inputs, &geometry, sizes);
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
                geometry = %geometry.iter().take(SPELLED).map(Geometry::spell).collect::<Vec<_>>().join("; "),
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
    let (mut writes, mut reads) = (Vec::new(), Vec::new());
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
            unsafe { note_desc(state, &desc, true, what, &mut writes, &mut reads) };
        }
    });
    let Some(f) = real(Entry::Rehydrate) else {
        return 0;
    };
    // SAFETY: as in `new`.
    let f: RehydrateFn = unsafe { std::mem::transmute::<usize, RehydrateFn>(f) };
    victory_gpu::bvhguard::enter("a rehydration", &writes, &reads);
    // SAFETY: as in `new`.
    let hr = unsafe { f(this, list, count, descs, keys) };
    victory_gpu::bvhguard::leave();
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
    /// On with `VICTORY_BVH_TRACE`, `VICTORY_BVH_GUARD=1` or
    /// `VICTORY_BVH_CHECK` (which starts the checks): the guard and the checks
    /// take their ranges and structures from the wrappers.
    pub fn new(inner: &'a dyn ImportResolver) -> Traced<'a> {
        let guard = victory_gpu::bvhguard::on();
        let check = victory_gpu::bvhcheck::on();
        let on = level() > 0 || guard || check;
        if on {
            tracing::info!(
                spelled = level(),
                guard,
                check_bytes_per_second = victory_gpu::bvhcheck::budget(),
                "the ray-tracing structure builder's entry points are wrapped (VICTORY_BVH_TRACE, \
                 VICTORY_BVH_GUARD, VICTORY_BVH_CHECK)"
            );
            start_checks();
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
        assert_eq!(s.extent, Some((0, 0x4_0000)));
    }

    fn inputs(kind: u32, flags: u32, count: u32) -> Inputs {
        Inputs {
            kind,
            flags,
            count,
            layout: 0,
            descs: 0x5000,
        }
    }

    fn desc(dest: u64, inputs: Inputs) -> BuildDesc {
        BuildDesc {
            dest,
            inputs,
            source: 0,
            scratch: 0x9000,
        }
    }

    fn sizes(result: u64) -> Sizes {
        Sizes {
            result,
            scratch: result / 4,
            update_scratch: result / 16,
        }
    }

    const NO_HEADER: fn(u64) -> Option<(u64, u64)> = |_| None;

    /// The tiers seen in run `20261125-130040`: bottom levels whose query
    /// does not match exactly (index format 53) but has the same level,
    /// flags, count and triangles; top levels built with fewer instances
    /// than a query of theirs; rehydrations whose size only their header
    /// in title memory tells.
    #[test]
    fn a_builds_sizes_come_from_the_closest_source() {
        let mut state = State::new();
        // Queried with R16_UINT indices, built with format 53.
        let asked = [Geometry::parse(&triangles(390, 135))];
        let mut raw = triangles(390, 135);
        put32(&mut raw, 16, 53);
        let built = [Geometry::parse(&raw)];
        let bottom = inputs(1, 0xa, 1);
        note_sizes(&mut state, &bottom, &asked, sizes(26624));
        let exact = found(&state, &desc(0x100, bottom), &asked, false, NO_HEADER);
        assert_eq!((exact.dest, exact.by), (Some((0, 26624)), SizedBy::Exact));
        let loose = found(&state, &desc(0x100, bottom), &built, false, NO_HEADER);
        assert_eq!(
            (loose.dest, loose.scratch, loose.by),
            (Some((0, 26624)), Some(6656), SizedBy::Loose)
        );
        // A top level of 3305 instances: the smallest answer for at least
        // as many.
        let top = |count| inputs(0, 0, count);
        note_sizes(&mut state, &top(32768), &[], sizes(8_913_920));
        note_sizes(&mut state, &top(3072), &[], sizes(836_608));
        note_sizes(&mut state, &top(4096), &[], sizes(1_100_000));
        let by_capacity = found(&state, &desc(0x200, top(3305)), &[], false, NO_HEADER);
        assert_eq!(
            (by_capacity.dest, by_capacity.by),
            (Some((0, 1_100_000)), SizedBy::Capacity)
        );
        // A new build nothing was asked about.
        let none = found(
            &state,
            &desc(0x300, inputs(1, 0x4, 7)),
            &[],
            false,
            NO_HEADER,
        );
        assert_eq!(
            (none.dest, none.scratch, none.by),
            (None, None, SizedBy::Nothing)
        );
        // A rehydration in place: its header.
        let header = |at: u64| (at == 0x400).then_some((256, 4096));
        let rehydrated = found(&state, &desc(0x400, inputs(1, 0x14, 25)), &[], true, header);
        assert_eq!(
            (rehydrated.dest, rehydrated.by),
            (Some((256, 4096)), SizedBy::Header)
        );
        // An update of a structure built before: its extent, and the
        // update's scratch.
        state.structures.insert(
            0x100,
            Structure {
                top: false,
                extent: Some((0, 26624)),
                builds: 1,
                rehydrated: false,
                compacted: false,
                built: Instant::now(),
                checked: None,
            },
        );
        let update = found(
            &state,
            &desc(0x100, inputs(1, 0x2a, 1)),
            &built,
            false,
            NO_HEADER,
        );
        assert_eq!(
            (update.dest, update.scratch, update.by),
            (Some((0, 26624)), Some(1664), SizedBy::Earlier)
        );
    }

    /// What a build hands the guard: its destination and scratch as
    /// writes, its geometry as reads.
    #[test]
    fn a_build_names_its_writes_and_reads() {
        let mut state = State::new();
        let geometry = triangles(300, 120);
        let bottom = Inputs {
            descs: geometry.as_ptr() as u64,
            ..inputs(1, 0x2, 1)
        };
        let g = [Geometry::parse(&geometry)];
        note_sizes(&mut state, &bottom, &g, sizes(0x8000));
        let (mut writes, mut reads) = (Vec::new(), Vec::new());
        // SAFETY: the description's geometry is `geometry`.
        unsafe {
            note_desc(
                &mut state,
                &desc(0x10_0000, bottom),
                false,
                None,
                &mut writes,
                &mut reads,
            )
        };
        assert_eq!(
            writes,
            vec![
                Named {
                    at: 0x10_0000,
                    bytes: Some(0x8000)
                },
                Named {
                    at: 0x9000,
                    bytes: Some(0x2000)
                },
            ]
        );
        // Vertices (120 of 12 bytes), indices (300 of 2), no transform.
        assert!(reads.contains(&Named {
            at: 0x10_0000_2000,
            bytes: Some(1440)
        }));
        assert!(reads.contains(&Named {
            at: 0x10_0000_1000,
            bytes: Some(600)
        }));
    }
}

#[cfg(test)]
mod check_tests {
    use super::*;

    fn structure(top: bool, extent: Option<(u64, u64)>, age: u64) -> Structure {
        Structure {
            top,
            extent,
            builds: 1,
            rehydrated: false,
            compacted: false,
            built: Instant::now() - Duration::from_secs(age),
            checked: None,
        }
    }

    /// Settled structures only, top levels first, then the oldest, within
    /// the budget; once each after each build.
    #[test]
    fn checks_take_top_levels_first_and_keep_to_the_budget() {
        let mut state = State::new();
        state
            .structures
            .insert(0x1000, structure(false, Some((0, 4096)), 5));
        state
            .structures
            .insert(0x2000, structure(false, Some((0, 4096)), 9));
        state
            .structures
            .insert(0x3000, structure(true, Some((0, 8192)), 2));
        // Built just now: not yet.
        state
            .structures
            .insert(0x4000, structure(false, Some((0, 64)), 0));
        let (due, bottoms) = pick(&state, 9000);
        let order: Vec<u64> = due.iter().map(|(a, _)| *a).collect();
        // The top level (8 KiB) takes most of the budget; the oldest
        // bottom level goes in before the budget is passed.
        assert_eq!(order, vec![0x3000, 0x2000]);
        assert_eq!(bottoms.len(), 3);
        // Checked after its build: not again.
        let s = state.structures.get_mut(&0x2000).unwrap();
        s.checked = Some(Instant::now());
        let (due, _) = pick(&state, 1 << 30);
        let order: Vec<u64> = due.iter().map(|(a, _)| *a).collect();
        assert_eq!(order, vec![0x3000, 0x1000]);
    }

    #[test]
    fn an_instance_finds_the_bottom_level_that_holds_it() {
        let bottoms: std::collections::BTreeMap<u64, (u64, u64)> =
            [(0x1000, (0, 0x400)), (0x8000, (0, 0))]
                .into_iter()
                .collect();
        assert_eq!(bottom_at(&bottoms, 0x1000), Some(Some(0x400)));
        assert_eq!(bottom_at(&bottoms, 0x1100), Some(Some(0x300)));
        assert_eq!(bottom_at(&bottoms, 0x1400), None);
        assert_eq!(bottom_at(&bottoms, 0x800), None);
        // Known, size unknown.
        assert_eq!(bottom_at(&bottoms, 0x8000), Some(None));
        assert_eq!(bottom_at(&bottoms, 0x8100), None);
    }

    #[test]
    fn classes_tell_rehydrated_from_built() {
        let mut s = structure(false, None, 0);
        assert_eq!(class_of(&s), 1);
        s.rehydrated = true;
        assert_eq!(class_of(&s), 2);
        s.top = true;
        assert_eq!(class_of(&s), 0);
    }
}
