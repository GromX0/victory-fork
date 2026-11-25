//! Checks of the ray-tracing structures the title's builder wrote, and the
//! time rays take on the card (`VICTORY_BVH_CHECK`, `docs/raytracing-plan.md`
//! step 3; added 2026-11-25).
//!
//! **The structures.** [`check`] reads a structure from title memory (where
//! the builder's stores land after the copy-back, or at once in place) and
//! [`walk`] follows its nodes from the root. A wrong pointer can only mislead
//! the count, not this process: every read is checked by the kernel, and the
//! walk stays inside the bytes read. The layout, as `docs/raytracing-plan.md`
//! cites it:
//!
//! - the header (`XboxBvh4Header`): flags at 0 (format version in the low 16
//!   bits, bit 31 for a bottom level), 64-byte units after the header at 4,
//!   the root node's pointer at 0x30. The traversal programs load it from
//!   there (`CS_IrradianceCache_ShadowTrace`, at 0x13c-0x17c);
//! - a node pointer: the node's offset from the structure's start in 8-byte
//!   units, its kind in the low 3 bits. The traversal adds the structure's
//!   address over 8 to each pointer a box test returns;
//! - nodes as `victory_shader::bvh` reads them: kinds 0-3 are triangles,
//!   4 and 5 are boxes with 16-bit and 32-bit bounds, 6 an instance, 7 a
//!   procedural primitive. Box children are dwords 0-3 (`NONE` for none);
//! - an instance node: the bottom level's address in 256-byte units at +60,
//!   its root pointer in the low 30 bits of +56, and the world-to-object
//!   rows at +64..+112.
//!
//! Counted ([`Report`]): bad headers and roots, pointers outside the
//! structure, nodes reached twice, empty children, boxes with NaN bounds
//! (a child the box test never enters), inverted boxes, leaves of the wrong
//! level, triangles and transforms that are not finite, instances whose
//! bottom level no call to the builder named (dangling), and depth.
//!
//! **The time.** With the switch on, each dispatch of a builder program or
//! of a program that walks structures (`image_bvh*_intersect_ray`) is put
//! between two GPU timestamps in its host list ([`begin`], [`end`]). They
//! are read when the list is reset (its work done: [`harvest`]) and summed
//! by program. A summary line every 10 seconds gives milliseconds a frame.
//! shortcut: dispatches overlap on the card, so a program's time is when
//! its dispatch ran, not its own cost alone. The sums are close to a pass's
//! time; single programs are approximate.

#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use victory_shader::bvh;

/// `VICTORY_BVH_CHECK`: 0 (unset) off; `1` on, with structures read at
/// 64 MiB a second at most; `<n>` (2 or more): at most `n` MiB a second.
pub fn budget() -> u64 {
    static BUDGET: OnceLock<u64> = OnceLock::new();
    *BUDGET.get_or_init(|| parse_budget(std::env::var("VICTORY_BVH_CHECK").ok().as_deref()))
}

fn parse_budget(v: Option<&str>) -> u64 {
    match v.map(str::trim) {
        None | Some("" | "0") => 0,
        Some("1") => 64 << 20,
        Some(n) => n.parse::<u64>().map_or(64 << 20, |mb| mb << 20),
    }
}

pub fn on() -> bool {
    budget() > 0
}

/// What the walks of structures found. Sums over the structures checked.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub structures: u64,
    /// No readable memory at the address, or a structure larger than is read.
    pub unreadable: u64,
    pub too_big: u64,
    /// The header is not plausible, or says the other level.
    pub bad_header: u64,
    pub wrong_level: u64,
    /// The header spans more than the call into the builder named.
    pub oversize: u64,
    pub bad_root: u64,
    pub nodes: u64,
    pub box_nodes: u64,
    pub triangle_nodes: u64,
    pub instance_nodes: u64,
    pub procedural_nodes: u64,
    /// Pointers to outside the structure, or into its header.
    pub bad_pointers: u64,
    /// Nodes reached a second time (a tree reaches each once).
    pub revisits: u64,
    pub empty_children: u64,
    pub nan_boxes: u64,
    pub inverted_boxes: u64,
    /// A triangle under a top level, or an instance under a bottom level.
    pub wrong_leaves: u64,
    pub nonfinite_triangles: u64,
    pub nonfinite_transforms: u64,
    pub null_instances: u64,
    pub dangling_instances: u64,
    /// An instance's root pointer past its bottom level's end.
    pub bad_instance_roots: u64,
    /// Walks stopped at the depth or node limit.
    pub cut_short: u64,
    pub max_depth: u64,
    /// Over all leaves reached: depth summed, and the leaves.
    pub depth_sum: u64,
    pub leaves: u64,
}

impl Report {
    pub fn add(&mut self, o: &Report) {
        let max_depth = self.max_depth.max(o.max_depth);
        macro_rules! sum {
            ($($f:ident),*) => { $( self.$f += o.$f; )* };
        }
        sum!(
            structures,
            unreadable,
            too_big,
            bad_header,
            wrong_level,
            oversize,
            bad_root,
            nodes,
            box_nodes,
            triangle_nodes,
            instance_nodes,
            procedural_nodes,
            bad_pointers,
            revisits,
            empty_children,
            nan_boxes,
            inverted_boxes,
            wrong_leaves,
            nonfinite_triangles,
            nonfinite_transforms,
            null_instances,
            dangling_instances,
            bad_instance_roots,
            cut_short,
            depth_sum,
            leaves
        );
        self.max_depth = max_depth;
    }

    /// Structures with something wrong in them, as far as counted.
    pub fn faults(&self) -> u64 {
        self.bad_header
            + self.wrong_level
            + self.oversize
            + self.bad_root
            + self.bad_pointers
            + self.revisits
            + self.inverted_boxes
            + self.wrong_leaves
            + self.nonfinite_triangles
            + self.nonfinite_transforms
            + self.dangling_instances
            + self.bad_instance_roots
    }
}

/// A plausible header: flags, 64-byte units after it, the root pointer.
fn header(bytes: &[u8]) -> Option<(u32, u32, u32)> {
    let word = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let (flags, units, root) = (word(0)?, word(4)?, word(0x30)?);
    ((1..=6).contains(&(flags & 0xffff)) && u64::from(units) < MOST_BYTES / 64)
        .then_some((flags, units, root))
}

/// Bit 31 of the header's flags: a bottom level.
const BOTTOM_LEVEL: u32 = 0x8000_0000;
/// Most bytes of one structure read.
pub const MOST_BYTES: u64 = 64 << 20;
/// Deepest node followed.
const MOST_DEPTH: u64 = 96;
/// The low 30 bits of an instance's root pointer that mean none.
const NO_ROOT: u32 = 0x3fff_ffff;

/// The node's first 28 dwords (zeros past the bytes).
fn words(bytes: &[u8], at: usize) -> [u32; 28] {
    std::array::from_fn(|k| {
        let b = at + 4 * k;
        bytes
            .get(b..b + 4)
            .map_or(0, |w| u32::from_le_bytes(w.try_into().unwrap()))
    })
}

/// Walks the structure whose bytes from its header on are `bytes`, a top
/// level if `top`, into `r`. `bottom` answers for a bottom-level address an
/// instance names: `None` if no call to the builder named it, else its size
/// if known.
pub fn walk(bytes: &[u8], top: bool, bottom: &dyn Fn(u64) -> Option<Option<u64>>, r: &mut Report) {
    r.structures += 1;
    let Some((flags, units, root)) = header(bytes) else {
        r.bad_header += 1;
        return;
    };
    if (flags & BOTTOM_LEVEL != 0) == top {
        r.wrong_level += 1;
    }
    let size = (64 * (units as usize + 1)).min(bytes.len());
    let mut seen = vec![false; size / 64];
    let mut stack = vec![(root, 1u64)];
    let most_nodes = size / 64 + 1;
    let mut first = true;
    while let Some((pointer, depth)) = stack.pop() {
        let kind = pointer & 7;
        let at = (pointer as usize >> 3) * 64;
        let len = if matches!(kind, 5 | 6) { 128 } else { 64 };
        if at < 64 || at + len > size {
            if first {
                r.bad_root += 1;
            } else {
                r.bad_pointers += 1;
            }
            first = false;
            continue;
        }
        first = false;
        if seen[at / 64] {
            r.revisits += 1;
            continue;
        }
        seen[at / 64] = true;
        r.nodes += 1;
        r.max_depth = r.max_depth.max(depth);
        if depth > MOST_DEPTH || r.nodes as usize > most_nodes * 4 {
            r.cut_short += 1;
            break;
        }
        let w = words(bytes, at);
        match kind {
            4 | 5 => {
                r.box_nodes += 1;
                let boxes = if kind == 5 {
                    bvh::boxes32(&w)
                } else {
                    bvh::boxes16(&w)
                };
                for (child, b) in bvh::children(&w).into_iter().zip(boxes) {
                    if child == bvh::NONE {
                        r.empty_children += 1;
                        continue;
                    }
                    if b.iter().any(|f| f.is_nan()) {
                        r.nan_boxes += 1;
                        continue;
                    }
                    if b[0] > b[3] || b[1] > b[4] || b[2] > b[5] {
                        r.inverted_boxes += 1;
                    }
                    let leaf = child & 7;
                    if (top && leaf < 4) || (!top && leaf == 6) {
                        r.wrong_leaves += 1;
                    }
                    stack.push((child, depth + 1));
                }
            }
            0..=3 => {
                r.triangle_nodes += 1;
                r.leaves += 1;
                r.depth_sum += depth;
                let v = bvh::triangle_vertices(&w, kind);
                if v.iter().flatten().any(|f| !f.is_finite()) {
                    r.nonfinite_triangles += 1;
                }
            }
            6 => {
                r.instance_nodes += 1;
                r.leaves += 1;
                r.depth_sum += depth;
                // +56, +60, then the rows from +64 (dwords 16-27).
                let (root, units) = (w[14] & NO_ROOT, w[15]);
                if w[16..28].iter().any(|&x| !f32::from_bits(x).is_finite()) {
                    r.nonfinite_transforms += 1;
                }
                let address = u64::from(units) << 8;
                if address == 0 {
                    r.null_instances += 1;
                    continue;
                }
                match bottom(address) {
                    None => r.dangling_instances += 1,
                    Some(Some(bytes))
                        if root != NO_ROOT && (u64::from(root) >> 3) * 64 + 64 > bytes =>
                    {
                        r.bad_instance_roots += 1;
                    }
                    Some(_) => {}
                }
            }
            _ => {
                r.procedural_nodes += 1;
                r.leaves += 1;
                r.depth_sum += depth;
            }
        }
    }
}

/// Reads the structure at title address `base` and walks it ([`walk`]).
/// `extent` is what the call into the builder named for it (bytes before
/// and from `base`), if known.
pub fn check(
    base: u64,
    top: bool,
    extent: Option<(u64, u64)>,
    bottom: &dyn Fn(u64) -> Option<Option<u64>>,
) -> (Report, u64) {
    let mut r = Report::default();
    let mut head = [0u8; 64];
    if !crate::bvhguard::read_title(base, &mut head) {
        r.structures += 1;
        r.unreadable += 1;
        return (r, 64);
    }
    let Some((_, units, _)) = header(&head) else {
        r.structures += 1;
        r.bad_header += 1;
        return (r, 64);
    };
    let size = 64 * (u64::from(units) + 1);
    if size > MOST_BYTES {
        r.structures += 1;
        r.too_big += 1;
        return (r, 64);
    }
    if extent.is_some_and(|(_, from)| size > from) {
        r.oversize += 1;
    }
    let mut bytes = vec![0u8; size as usize];
    if !crate::bvhguard::read_title(base, &mut bytes) {
        r.structures += 1;
        r.unreadable += 1;
        return (r, size);
    }
    walk(&bytes, top, bottom, &mut r);
    (r, size)
}

/// Logs one class of structures: since the start and since the line before.
pub fn log(class: &str, builder: &str, all: &Report, window: &Report) {
    let mean = |r: &Report| r.depth_sum as f64 / r.leaves.max(1) as f64;
    tracing::info!(
        class,
        builder,
        structures = all.structures,
        with_faults = all.faults(),
        unreadable = all.unreadable,
        too_big = all.too_big,
        bad_header = all.bad_header,
        wrong_level = all.wrong_level,
        oversize = all.oversize,
        bad_root = all.bad_root,
        nodes = all.nodes,
        boxes = all.box_nodes,
        triangles = all.triangle_nodes,
        instances = all.instance_nodes,
        procedural = all.procedural_nodes,
        bad_pointers = all.bad_pointers,
        revisits = all.revisits,
        empty_children = all.empty_children,
        nan_boxes = all.nan_boxes,
        inverted_boxes = all.inverted_boxes,
        wrong_leaves = all.wrong_leaves,
        nonfinite_triangles = all.nonfinite_triangles,
        nonfinite_transforms = all.nonfinite_transforms,
        null_instances = all.null_instances,
        dangling_instances = all.dangling_instances,
        bad_instance_roots = all.bad_instance_roots,
        cut_short = all.cut_short,
        max_depth = all.max_depth,
        mean_leaf_depth = format_args!("{:.1}", mean(all)),
        checked_since_line_before = window.structures,
        faults_since_line_before = window.faults(),
        "ray-tracing structures checked in title memory since the start (VICTORY_BVH_CHECK)"
    );
}

/// What a timed dispatch runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Builder = 0,
    Traversal = 1,
}

/// Timestamps a host list takes: two per timed dispatch.
#[cfg(windows)]
const STAMPS: u32 = 1024;

/// A host list's timestamps ([`begin`]): the heap and its read-back
/// buffer once made, and per pair what it timed.
#[cfg(windows)]
#[derive(Default)]
pub(crate) struct Stamps {
    heap: Option<(
        windows::Win32::Graphics::Direct3D12::ID3D12QueryHeap,
        windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    )>,
    timed: Vec<(Class, String)>,
    resolved: bool,
}

/// Before a dispatch: its first timestamp, if it is timed.
#[cfg(windows)]
pub(crate) fn begin(list: &crate::host::HostList, class: Class, program: &str) -> Option<u32> {
    use windows::Win32::Graphics::Direct3D12::*;
    if !on() {
        return None;
    }
    let mut s = list.ray_stamps();
    if s.heap.is_none() {
        let device = &crate::host::host()?.device;
        let mut heap: Option<ID3D12QueryHeap> = None;
        let mut back: Option<ID3D12Resource> = None;
        let readback = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: u64::from(STAMPS) * 8,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        // SAFETY: plain object creation on our device.
        unsafe {
            device
                .CreateQueryHeap(
                    &D3D12_QUERY_HEAP_DESC {
                        Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
                        Count: STAMPS,
                        NodeMask: 0,
                    },
                    &mut heap,
                )
                .ok()?;
            device
                .CreateCommittedResource(
                    &readback,
                    D3D12_HEAP_FLAG_NONE,
                    &desc,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    None,
                    &mut back,
                )
                .ok()?;
        }
        s.heap = Some((heap?, back?));
    }
    let n = s.timed.len() as u32;
    if 2 * n + 2 > STAMPS {
        return None;
    }
    let (heap, _) = s.heap.as_ref()?;
    // SAFETY: an open list and our query heap.
    unsafe { list.list.EndQuery(heap, D3D12_QUERY_TYPE_TIMESTAMP, 2 * n) };
    s.timed.push((class, program.to_owned()));
    Some(n)
}

/// After the dispatch [`begin`] timed.
#[cfg(windows)]
pub(crate) fn end(list: &crate::host::HostList, n: Option<u32>) {
    use windows::Win32::Graphics::Direct3D12::D3D12_QUERY_TYPE_TIMESTAMP;
    let Some(n) = n else { return };
    let s = list.ray_stamps();
    if let Some((heap, _)) = &s.heap {
        // SAFETY: an open list and our query heap.
        unsafe {
            list.list
                .EndQuery(heap, D3D12_QUERY_TYPE_TIMESTAMP, 2 * n + 1)
        };
    }
}

/// When the list is closed: its timestamps go to the read-back buffer.
#[cfg(windows)]
pub(crate) fn resolve(list: &crate::host::HostList) {
    use windows::Win32::Graphics::Direct3D12::D3D12_QUERY_TYPE_TIMESTAMP;
    let mut s = list.ray_stamps();
    let count = 2 * s.timed.len() as u32;
    if count == 0 {
        return;
    }
    if let Some((heap, back)) = &s.heap {
        // SAFETY: an open list, our heap and read-back buffer.
        unsafe {
            list.list
                .ResolveQueryData(heap, D3D12_QUERY_TYPE_TIMESTAMP, 0, count, back, 0)
        };
        s.resolved = true;
    }
}

/// Time by program: class, dispatches, ticks.
static TIMES: Mutex<Option<HashMap<String, (Class, u64, u64)>>> = Mutex::new(None);

/// When the list is reset (its work has run): what it timed is summed.
#[cfg(windows)]
pub(crate) fn harvest(list: &crate::host::HostList) {
    use windows::Win32::Graphics::Direct3D12::D3D12_RANGE;
    let mut s = list.ray_stamps();
    let timed = std::mem::take(&mut s.timed);
    let resolved = std::mem::take(&mut s.resolved);
    if timed.is_empty() || !resolved {
        return;
    }
    let Some((_, back)) = &s.heap else { return };
    let count = 2 * timed.len();
    let mut data = std::ptr::null_mut();
    let range = D3D12_RANGE {
        Begin: 0,
        End: count * 8,
    };
    // SAFETY: our read-back buffer, resolved by the list that has run.
    let stamps: Vec<u64> = unsafe {
        if back.Map(0, Some(&range), Some(&mut data)).is_err() || data.is_null() {
            return;
        }
        let v = std::slice::from_raw_parts(data.cast::<u64>(), count).to_vec();
        back.Unmap(0, None);
        v
    };
    let frequency = crate::host::timestamp_frequency();
    let mut times = TIMES.lock().unwrap_or_else(|e| e.into_inner());
    let times = times.get_or_insert_with(HashMap::new);
    for (i, (class, program)) in timed.into_iter().enumerate() {
        let ticks = stamps[2 * i + 1].saturating_sub(stamps[2 * i]);
        // A list closed and never run reads what the buffer held.
        if ticks > frequency {
            continue;
        }
        let e = times.entry(program).or_insert((class, 0, 0));
        e.1 += 1;
        e.2 += ticks;
    }
    drop(s);
    time_line(frequency, times);
}

/// The time line, every 10 seconds: per frame since the line before.
#[cfg(windows)]
fn time_line(frequency: u64, times: &mut HashMap<String, (Class, u64, u64)>) {
    static LAST: Mutex<Option<(std::time::Instant, u64)>> = Mutex::new(None);
    static LINES: AtomicU64 = AtomicU64::new(0);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let frame = crate::frame::current();
    let now = std::time::Instant::now();
    let Some((then, first)) = *last else {
        *last = Some((now, frame));
        times.clear();
        return;
    };
    if now.duration_since(then) < std::time::Duration::from_secs(10) {
        return;
    }
    *last = Some((now, frame));
    let frames = frame.saturating_sub(first).max(1) as f64;
    let ms = |ticks: u64| ticks as f64 * 1e3 / frequency.max(1) as f64 / frames;
    let mut sums = [(0u64, 0u64); 2];
    for (class, n, ticks) in times.values() {
        sums[*class as usize].0 += n;
        sums[*class as usize].1 += ticks;
    }
    let mut slowest: Vec<(&String, &(Class, u64, u64))> = times.iter().collect();
    slowest.sort_unstable_by_key(|e| std::cmp::Reverse(e.1.2));
    let slowest: Vec<String> = slowest
        .iter()
        .take(8)
        .map(|(p, (c, n, t))| format!("{c:?} {p}: {:.2} ms in {:.1}", ms(*t), *n as f64 / frames))
        .collect();
    LINES.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        frames = frames as u64,
        builder_ms_per_frame = format_args!("{:.2}", ms(sums[0].1)),
        builder_dispatches_per_frame = format_args!("{:.1}", sums[0].0 as f64 / frames),
        traversal_ms_per_frame = format_args!("{:.2}", ms(sums[1].1)),
        traversal_dispatches_per_frame = format_args!("{:.1}", sums[1].0 as f64 / frames),
        slowest = ?slowest,
        "ray-tracing dispatches on the card since the line before (VICTORY_BVH_CHECK; per frame; \
         a dispatch's time is from the timestamp before it to the one after)"
    );
    times.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn putf(b: &mut [u8], at: usize, v: f32) {
        put(b, at, v.to_bits());
    }

    /// A pointer to the node at byte `at` of kind `kind`.
    fn pointer(at: usize, kind: u32) -> u32 {
        (at as u32 / 64) << 3 | kind
    }

    /// A bottom level of `units` 64-byte units after the header: the root
    /// a 32-bit box node at 64 whose first child is a triangle node at 192
    /// and whose second is `second`.
    fn bottom_level(units: u32, second: u32) -> Vec<u8> {
        let mut b = vec![0u8; 64 * (units as usize + 1)];
        put(&mut b, 0, BOTTOM_LEVEL | 1);
        put(&mut b, 4, units);
        put(&mut b, 0x30, pointer(64, 5));
        // The box node: children, then per child min xyz, max xyz.
        put(&mut b, 64, pointer(192, 0));
        put(&mut b, 68, second);
        put(&mut b, 72, bvh::NONE);
        put(&mut b, 76, bvh::NONE);
        for child in 0..4 {
            for k in 0..6 {
                putf(
                    &mut b,
                    64 + 16 + 4 * (6 * child + k),
                    if k < 3 { 0.0 } else { 1.0 },
                );
            }
        }
        // The triangle node: five vertices, all finite; id 0.
        for k in 0..15 {
            putf(&mut b, 192 + 4 * k, 0.25 * k as f32);
        }
        b
    }

    const KNOWN: fn(u64) -> Option<Option<u64>> = |a| (a == 0x10_0000).then_some(Some(4096));

    #[test]
    fn a_well_formed_bottom_level_walks_clean() {
        let mut r = Report::default();
        walk(&bottom_level(3, bvh::NONE), false, &KNOWN, &mut r);
        assert_eq!(
            (r.structures, r.nodes, r.box_nodes, r.triangle_nodes),
            (1, 2, 1, 1)
        );
        assert_eq!(
            (r.empty_children, r.max_depth, r.leaves, r.depth_sum),
            (3, 2, 1, 2)
        );
        assert_eq!(r.faults(), 0, "{r:?}");
    }

    #[test]
    fn broken_nodes_are_counted() {
        // A second child past the end.
        let mut r = Report::default();
        walk(&bottom_level(3, pointer(4096, 0)), false, &KNOWN, &mut r);
        assert_eq!(r.bad_pointers, 1);
        // Back to the root: reached twice.
        let mut r = Report::default();
        walk(&bottom_level(3, pointer(64, 5)), false, &KNOWN, &mut r);
        assert_eq!(r.revisits, 1);
        // An instance under a bottom level, NaN and inverted bounds.
        let mut b = bottom_level(3, pointer(128, 6));
        let mut r = Report::default();
        walk(&b, false, &KNOWN, &mut r);
        assert_eq!(r.wrong_leaves, 1);
        putf(&mut b, 64 + 16, f32::NAN);
        // The second child's maximum x below its minimum.
        putf(&mut b, 64 + 16 + 4 * (6 + 3), -5.0);
        let mut r = Report::default();
        walk(&b, false, &KNOWN, &mut r);
        assert_eq!((r.nan_boxes, r.inverted_boxes), (1, 1));
        // A triangle that is not finite.
        let mut b = bottom_level(3, bvh::NONE);
        for k in 0..15 {
            putf(&mut b, 192 + 4 * k, f32::INFINITY);
        }
        let mut r = Report::default();
        walk(&b, false, &KNOWN, &mut r);
        assert_eq!(r.nonfinite_triangles, 1);
        // No header, and a root in the header.
        let mut r = Report::default();
        walk(&[0u8; 64], false, &KNOWN, &mut r);
        assert_eq!(r.bad_header, 1);
        let mut b = bottom_level(3, bvh::NONE);
        put(&mut b, 0x30, pointer(0, 5));
        let mut r = Report::default();
        walk(&b, false, &KNOWN, &mut r);
        assert_eq!(r.bad_root, 1);
    }

    /// A top level: a box node over two instances, one naming a bottom
    /// level a call named and one naming none.
    #[test]
    fn instances_name_their_bottom_levels() {
        let mut b = vec![0u8; 64 * 6];
        put(&mut b, 0, 2);
        put(&mut b, 4, 5);
        put(&mut b, 0x30, pointer(64, 4));
        put(&mut b, 64, pointer(128, 6));
        put(&mut b, 68, pointer(256, 6));
        put(&mut b, 72, bvh::NONE);
        put(&mut b, 76, bvh::NONE);
        // 16-bit bounds all zero: a point, not inverted.
        for (at, blas) in [(128usize, 0x10_0000u64), (256, 0x20_0000)] {
            put(&mut b, at + 56, pointer(64, 5));
            put(&mut b, at + 60, (blas >> 8) as u32);
            for k in 0..12 {
                putf(&mut b, at + 64 + 4 * k, 1.0);
            }
        }
        let mut r = Report::default();
        walk(&b, true, &KNOWN, &mut r);
        assert_eq!(
            (r.instance_nodes, r.dangling_instances, r.wrong_level),
            (2, 1, 0)
        );
        assert_eq!(
            (r.bad_pointers, r.bad_instance_roots, r.wrong_leaves),
            (0, 0, 0)
        );
        // A root past its bottom level's 4 KiB.
        put(&mut b, 128 + 56, pointer(8192, 5));
        let mut r = Report::default();
        walk(&b, true, &KNOWN, &mut r);
        assert_eq!(r.bad_instance_roots, 1);
        // Read as a bottom level, its flags say otherwise.
        let mut r = Report::default();
        walk(&b, false, &KNOWN, &mut r);
        assert_eq!(r.wrong_level, 1);
    }

    #[test]
    fn the_budget_reads_on_and_megabytes() {
        assert_eq!(parse_budget(None), 0);
        assert_eq!(parse_budget(Some("0")), 0);
        assert_eq!(parse_budget(Some("1")), 64 << 20);
        assert_eq!(parse_budget(Some("16")), 16 << 20);
    }

    #[test]
    fn reports_add_up() {
        let mut a = Report {
            nodes: 3,
            max_depth: 4,
            bad_pointers: 1,
            ..Report::default()
        };
        let b = Report {
            nodes: 2,
            max_depth: 9,
            revisits: 2,
            ..Report::default()
        };
        a.add(&b);
        assert_eq!((a.nodes, a.max_depth, a.faults()), (5, 9, 3));
    }
}
