//! The guard on the ray-tracing structure builder's stores
//! (`VICTORY_BVH_GUARD=1`, `docs/raytracing-plan.md` step 2; added
//! 2026-11-25).
//!
//! Each call the title makes into `XboxBvhBuilder.dll` names the memory its
//! GPU work may write: the destination structures, their scratch, the
//! post-build results, a compacting copy's destination. It also names what
//! the work only reads: geometry, instances, the source structure. The HLE's
//! wrappers (`victory_hle::modules::bvhbuilder`) open a **scope** with those
//! ranges while the DLL runs ([`enter`], [`leave`]). The DLL records its
//! dispatches on the calling thread in the meantime, so each builder
//! dispatch finds its call's scope here ([`dispatch_scope`]). Buffers made
//! on that thread while a scope is open are the DLL's own, and it may write
//! them in every later call ([`created`]).
//!
//! What the guard does with the scope, with host copies of title memory
//! (`crate::shadow`):
//!
//! - A builder dispatch declares written, for the copy-back, only what lies
//!   inside its scope's write ranges: its root views, and the copies of
//!   title memory by address (`crate::memory::ready`), which it otherwise
//!   declares whole. A store elsewhere stays in the host copy and never
//!   reaches title memory. The run that crashed (`20261125-111938`) had
//!   arrays of empty bounding boxes in the title's heap objects.
//! - A dispatch of another program that reads title memory by address but
//!   stores through no address (the traversal programs) no longer declares
//!   those copies written. Otherwise a builder store copied in between would
//!   go back to the title under that program's declaration.
//! - Root views a builder dispatch binds outside its scope's write ranges
//!   are counted and logged by program and page, at most [`MOST_NAMED`]
//!   lines in a run.
//!
//! shortcut: stores are not seen one by one. What is cut is the
//! *declaration*, so the log names root views outside their call's ranges
//! and bytes left out, not single stores. With title memory in place
//! (`crate::inplace`), a store reaches the title at once, and nothing here
//! stops it. A check in the translated stores themselves is the next step.
//!
//! shortcut: a program other than the builder that does store through
//! addresses still declares its copies whole. A builder store that lands in
//! one of them within the same submission goes back under that
//! declaration.
//!
//! shortcut: a write range whose size the call does not tell (no size query
//! for the same inputs came first) takes the title buffer that holds its
//! address, from the address to the buffer's end. Failing that it takes
//! [`unsized_bytes`] (`VICTORY_BVH_GUARD_UNSIZED_KB`, default 0: nothing).
//! Such ranges are counted, so the trace's numbers can tune it.

#![cfg_attr(not(windows), allow(dead_code))]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// `VICTORY_BVH_GUARD=1`.
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_BVH_GUARD").is_some_and(|v| v == "1"))
}

/// Bytes a write range of unknown size gets when no title buffer holds its
/// address (`VICTORY_BVH_GUARD_UNSIZED_KB`, default 0).
fn unsized_bytes() -> u64 {
    static BYTES: OnceLock<u64> = OnceLock::new();
    *BYTES.get_or_init(|| {
        std::env::var("VICTORY_BVH_GUARD_UNSIZED_KB")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map_or(0, |kb| kb << 10)
    })
}

/// Half-open address ranges, sorted, none touching another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Ranges(Vec<(u64, u64)>);

impl Ranges {
    pub(crate) fn new(mut ranges: Vec<(u64, u64)>) -> Ranges {
        ranges.retain(|&(from, to)| from < to);
        ranges.sort_unstable();
        let mut out: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
        for (from, to) in ranges {
            match out.last_mut() {
                Some(last) if from <= last.1 => last.1 = last.1.max(to),
                _ => out.push((from, to)),
            }
        }
        Ranges(out)
    }

    /// The parts of `[from, to)` inside the ranges, in order.
    pub(crate) fn clip(&self, from: u64, to: u64) -> Vec<(u64, u64)> {
        // The first range that ends after `from`.
        let first = self.0.partition_point(|&(_, end)| end <= from);
        self.0[first..]
            .iter()
            .take_while(|&&(start, _)| start < to)
            .map(|&(start, end)| (start.max(from), end.min(to)))
            .filter(|&(a, b)| a < b)
            .collect()
    }

    pub(crate) fn contains(&self, at: u64) -> bool {
        let i = self.0.partition_point(|&(_, end)| end <= at);
        self.0.get(i).is_some_and(|&(start, _)| start <= at)
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.0.iter().map(|&(a, b)| b - a).sum()
    }

    fn union(&self, other: &Ranges) -> Ranges {
        Ranges::new(self.0.iter().chain(&other.0).copied().collect())
    }
}

/// A range a call names: its start, and its size if the call tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Named {
    pub at: u64,
    pub bytes: Option<u64>,
}

/// One call into the builder.
#[derive(Debug)]
pub(crate) struct Scope {
    pub(crate) id: u64,
    pub(crate) what: &'static str,
    /// What its dispatches may write: the ranges it names and the DLL's
    /// own buffers.
    pub(crate) writes: Ranges,
    /// What they only read.
    pub(crate) reads: Ranges,
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Scope>>> = const { RefCell::new(None) };
}

/// The DLL's own buffers ([`created`]).
static OWNED: Mutex<Ranges> = Mutex::new(Ranges(Vec::new()));

/// The range `named` stands for: its size if told; else from its address
/// to the end of the title buffer that holds it; else [`unsized_bytes`].
/// The second value says whether its size was not told.
fn extent(named: Named, buffer: impl Fn(u64) -> Option<(u64, u64)>) -> ((u64, u64), bool) {
    if let Some(n) = named.bytes {
        return ((named.at, named.at.saturating_add(n)), false);
    }
    let end = match buffer(named.at) {
        Some((start, len)) if start <= named.at => start.saturating_add(len),
        _ => named.at.saturating_add(unsized_bytes()),
    };
    ((named.at, end), true)
}

fn buffer_extent(at: u64) -> Option<(u64, u64)> {
    #[cfg(windows)]
    return crate::shadow::extent(at);
    #[cfg(not(windows))]
    {
        let _ = at;
        None
    }
}

/// Fills `out` from title memory at `at`; false if a page of it is not
/// readable (the kernel checks, nothing faults).
pub fn read_title(at: u64, out: &mut [u8]) -> bool {
    #[cfg(windows)]
    return at != 0 && crate::host::read_into(at as *const u8, out);
    #[cfg(not(windows))]
    {
        let _ = (at, out);
        false
    }
}

/// The bytes a structure whose header is `bytes` spans around its header
/// (`XboxBvh4Header`, the layout `docs/raytracing-plan.md` cites: flags
/// with the format version in the low 16 bits, the 64-byte units after the
/// header at byte 4, the parent pointers' offset at byte 0x38, negative
/// when they lie before the header): (bytes before the header, bytes from
/// it). None if `bytes` do not hold a plausible header.
pub fn header_extent(bytes: &[u8; 64]) -> Option<(u64, u64)> {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    /// No single structure is larger.
    const MOST: u64 = 1 << 30;
    let (flags, units, parents) = (word(0), word(4), word(0x38) as i32);
    let size = 64 * (u64::from(units) + 1);
    let below = u64::from(parents.min(0).unsigned_abs());
    ((1..=6).contains(&(flags & 0xffff)) && size <= MOST && below <= MOST).then_some((below, size))
}

/// A call into the builder begins on this thread: until [`leave`], the
/// dispatches recorded here may write `writes` (and the DLL's own buffers)
/// and read `reads`. Null addresses are left out.
pub fn enter(what: &'static str, writes: &[Named], reads: &[Named]) {
    if !on() {
        return;
    }
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let mut unsized_ = 0;
    let mut ranges = |named: &[Named], count: bool| {
        Ranges::new(
            named
                .iter()
                .filter(|n| n.at != 0)
                .map(|&n| {
                    let (range, unknown) = extent(n, buffer_extent);
                    if count && unknown {
                        unsized_ += 1;
                    }
                    range
                })
                .collect(),
        )
    };
    let named_writes = ranges(writes, true);
    let reads = ranges(reads, false);
    let owned = OWNED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let scope = Scope {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        what,
        writes: named_writes.union(&owned),
        reads,
    };
    STATS[S_SCOPES].fetch_add(1, Ordering::Relaxed);
    STATS[S_UNSIZED].fetch_add(unsized_, Ordering::Relaxed);
    STATS[S_ALLOWED].fetch_add(named_writes.bytes(), Ordering::Relaxed);
    CURRENT.with(|c| *c.borrow_mut() = Some(Arc::new(scope)));
}

/// The call that [`enter`] began has returned.
pub fn leave() {
    CURRENT.with(|c| *c.borrow_mut() = None);
}

/// A title buffer was made at `start` (`len` bytes): while a scope is open
/// on this thread, it is the DLL's own, and later scopes may write it.
pub(crate) fn created(start: u64, len: u64) {
    if !on() || CURRENT.with(|c| c.borrow().is_none()) {
        return;
    }
    let mut owned = OWNED.lock().unwrap_or_else(|e| e.into_inner());
    *owned = owned.union(&Ranges::new(vec![(start, start.saturating_add(len))]));
    STATS[S_OWNED].fetch_add(len, Ordering::Relaxed);
    // Visible to the scope that is open too.
    CURRENT.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(scope) = c.as_ref() {
            *c = Some(Arc::new(Scope {
                id: scope.id,
                what: scope.what,
                writes: scope.writes.union(&owned),
                reads: scope.reads.clone(),
            }));
        }
    });
}

/// No ranges: what a builder dispatch outside every call may write.
static NOTHING: Ranges = Ranges(Vec::new());

/// For a builder dispatch being bound, with the guard on: what it may
/// write. A dispatch outside every call into the builder may write
/// nothing, and that is counted and logged once.
pub(crate) fn dispatch_scope() -> Option<Arc<Scope>> {
    STATS[S_DISPATCHES].fetch_add(1, Ordering::Relaxed);
    let scope = CURRENT.with(|c| c.borrow().clone());
    if scope.is_none() {
        STATS[S_UNSCOPED].fetch_add(1, Ordering::Relaxed);
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            tracing::warn!(
                "a ray-tracing structure builder dispatch outside every call into the builder: its \
                 stores are kept out of title memory (VICTORY_BVH_GUARD)"
            )
        });
    }
    line();
    scope
}

/// The write ranges of `scope`, or none.
pub(crate) fn writes(scope: Option<&Scope>) -> &Ranges {
    scope.map_or(&NOTHING, |s| &s.writes)
}

const S_SCOPES: usize = 0;
const S_UNSIZED: usize = 1;
const S_ALLOWED: usize = 2;
const S_OWNED: usize = 3;
const S_DISPATCHES: usize = 4;
const S_UNSCOPED: usize = 5;
/// Bytes of copies of title memory by address declared written, and left
/// out, by builder dispatches.
const S_MEMORY_KEPT: usize = 6;
const S_MEMORY_CUT: usize = 7;
/// The same for root views.
const S_ROOT_KEPT: usize = 8;
const S_ROOT_CUT: usize = 9;
/// Root views outside their scope's write ranges: in its reads, or in
/// neither.
const S_ROOT_INPUT: usize = 10;
const S_ROOT_OUTSIDE: usize = 11;
/// Dispatches of other programs whose copies of title memory by address
/// were not declared written (they store through no address).
const S_READERS: usize = 12;

static STATS: [AtomicU64; 13] = [const { AtomicU64::new(0) }; 13];

/// Copies of title memory by address that a builder dispatch would have
/// declared written whole: `kept` of their `whole` bytes were declared.
pub(crate) fn memory_cut(whole: u64, kept: u64) {
    STATS[S_MEMORY_KEPT].fetch_add(kept, Ordering::Relaxed);
    STATS[S_MEMORY_CUT].fetch_add(whole.saturating_sub(kept), Ordering::Relaxed);
}

/// A dispatch of a program that stores through no address did not declare
/// its copies of title memory by address written.
pub(crate) fn reader_left_out() {
    STATS[S_READERS].fetch_add(1, Ordering::Relaxed);
}

/// A builder dispatch's root view at `at` covers `whole` bytes of its copy,
/// of which `kept` are declared written. If its address is outside the
/// scope's write ranges, it is noted under `program`.
pub(crate) fn root_view(scope: Option<&Scope>, program: &str, at: u64, whole: u64, kept: u64) {
    STATS[S_ROOT_KEPT].fetch_add(kept, Ordering::Relaxed);
    STATS[S_ROOT_CUT].fetch_add(whole.saturating_sub(kept), Ordering::Relaxed);
    if writes(scope).contains(at) {
        return;
    }
    let input = scope.is_some_and(|s| s.reads.contains(at));
    STATS[if input { S_ROOT_INPUT } else { S_ROOT_OUTSIDE }].fetch_add(1, Ordering::Relaxed);
    if !input {
        outside(program, at, scope.map_or("no call", |s| s.what));
    }
}

/// Most lines naming a program and page.
const MOST_NAMED: usize = 64;

/// Root views outside their call's ranges, by program and 64 KiB page:
/// how many.
static OUTSIDE: Mutex<Option<HashMap<(String, u64), u64>>> = Mutex::new(None);

fn outside(program: &str, at: u64, what: &'static str) {
    let mut map = OUTSIDE.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    let key = (program.to_owned(), at >> 16);
    let fresh = !map.contains_key(&key);
    if fresh && map.len() >= MOST_NAMED * 16 {
        return;
    }
    *map.entry(key).or_default() += 1;
    if fresh && map.len() <= MOST_NAMED {
        tracing::warn!(
            program,
            page = format_args!("{:#x}", at & !0xffff),
            address = format_args!("{at:#x}"),
            call = what,
            "a ray-tracing structure builder dispatch binds a root view outside its call's \
             ranges: what it stores there is kept out of title memory (VICTORY_BVH_GUARD)"
        );
    }
}

/// The summary line, every 10 seconds.
fn line() {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let due = last.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(10));
    if !due {
        return;
    }
    let first = last.is_none();
    *last = Some(now);
    if first {
        return;
    }
    let s = |i: usize| STATS[i].load(Ordering::Relaxed);
    let mib = |i: usize| s(i) >> 20;
    let busiest = {
        let map = OUTSIDE.lock().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<(String, u64)> = map
            .iter()
            .flatten()
            .map(|((p, page), n)| (format!("{p} {:#x}", page << 16), *n))
            .collect();
        v.sort_unstable_by_key(|e| std::cmp::Reverse(e.1));
        v.truncate(4);
        v
    };
    tracing::info!(
        calls = s(S_SCOPES),
        ranges_of_unknown_size = s(S_UNSIZED),
        named_mib = mib(S_ALLOWED),
        own_buffers_mib = mib(S_OWNED),
        builder_dispatches = s(S_DISPATCHES),
        outside_every_call = s(S_UNSCOPED),
        memory_by_address_kept_mib = mib(S_MEMORY_KEPT),
        memory_by_address_cut_mib = mib(S_MEMORY_CUT),
        root_views_kept_mib = mib(S_ROOT_KEPT),
        root_views_cut_mib = mib(S_ROOT_CUT),
        root_views_on_inputs = s(S_ROOT_INPUT),
        root_views_outside = s(S_ROOT_OUTSIDE),
        readers_left_out = s(S_READERS),
        busiest = ?busiest,
        "the ray-tracing structure builder's guard since the start (VICTORY_BVH_GUARD): what its \
         dispatches declared written for the copy-back, kept to their calls' ranges, and what was \
         left out"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_tells_its_extent() {
        let mut h = [0u8; 64];
        h[0..4].copy_from_slice(&(0x8000_0004u32).to_le_bytes());
        h[4..8].copy_from_slice(&63u32.to_le_bytes());
        h[0x38..0x3c].copy_from_slice(&(-256i32).to_le_bytes());
        assert_eq!(header_extent(&h), Some((256, 4096)));
        h[0x38..0x3c].copy_from_slice(&128i32.to_le_bytes());
        assert_eq!(header_extent(&h), Some((0, 4096)));
        h[0..4].copy_from_slice(&9u32.to_le_bytes());
        assert_eq!(header_extent(&h), None);
        assert!(!read_title(0, &mut [0u8; 4]));
    }

    #[test]
    fn ranges_join_and_clip() {
        let r = Ranges::new(vec![(30, 40), (10, 20), (20, 25), (0, 0), (50, 45)]);
        assert_eq!(r.0, vec![(10, 25), (30, 40)]);
        assert_eq!(r.clip(0, 100), vec![(10, 25), (30, 40)]);
        assert_eq!(r.clip(15, 35), vec![(15, 25), (30, 35)]);
        assert_eq!(r.clip(25, 30), vec![]);
        assert_eq!(r.clip(39, 41), vec![(39, 40)]);
        assert!(r.contains(10) && r.contains(24) && !r.contains(25) && !r.contains(9));
        assert!(!r.contains(40) && r.contains(39));
        assert_eq!(r.bytes(), 25);
        assert_eq!(NOTHING.clip(0, u64::MAX), vec![]);
    }

    #[test]
    fn a_range_without_a_size_takes_its_buffer_or_nothing() {
        let buffer = |at: u64| (0x1000..0x3000).contains(&at).then_some((0x1000, 0x2000));
        let sized = Named {
            at: 0x1100,
            bytes: Some(0x40),
        };
        assert_eq!(extent(sized, buffer), ((0x1100, 0x1140), false));
        let in_buffer = Named {
            at: 0x1100,
            bytes: None,
        };
        assert_eq!(extent(in_buffer, buffer), ((0x1100, 0x3000), true));
        let nowhere = Named {
            at: 0x9000,
            bytes: None,
        };
        // `VICTORY_BVH_GUARD_UNSIZED_KB` is unset in tests.
        assert_eq!(extent(nowhere, buffer), ((0x9000, 0x9000), true));
    }

    /// The guard's rule for one declaration, as `crate::memory` and the
    /// root views apply it: a stray store's bytes are not declared, so
    /// they are never copied back, and an allowed store's are.
    #[test]
    fn a_stray_store_is_left_out_and_an_allowed_one_kept() {
        // A call that names a 4 KiB structure and 1 KiB of scratch.
        let scope = Scope {
            id: 1,
            what: "a build",
            writes: Ranges::new(vec![
                (0x10_0000_1000, 0x10_0000_2000),
                (0x10_0010_0000, 0x10_0010_0400),
            ]),
            reads: Ranges::new(vec![(0x10_0200_0000, 0x10_0201_0000)]),
        };
        // A copy of a 4 MiB window that holds the structure and the title's
        // own objects after it.
        let window = (0x10_0000_0000u64, 0x10_0040_0000u64);
        let declared = writes(Some(&scope)).clip(window.0, window.1);
        assert_eq!(
            declared,
            vec![
                (0x10_0000_1000, 0x10_0000_2000),
                (0x10_0010_0000, 0x10_0010_0400)
            ]
        );
        let in_declared = |at: u64| declared.iter().any(|&(a, b)| a <= at && at < b);
        // The builder's node store: declared, so copied back.
        assert!(in_declared(0x10_0000_1800));
        // An empty box stored over a title object 1 MiB further on: not
        // declared, so it stays in the host copy.
        assert!(!in_declared(0x10_0010_0000 - 0x8_0000));
        // Outside every call: nothing is declared.
        assert_eq!(writes(None).clip(window.0, window.1), vec![]);
    }

    #[test]
    fn root_views_are_sorted_into_kept_inputs_and_outside() {
        let scope = Scope {
            id: 2,
            what: "a build",
            writes: Ranges::new(vec![(0x2000, 0x3000)]),
            reads: Ranges::new(vec![(0x8000, 0x9000)]),
        };
        let before = |i: usize| STATS[i].load(Ordering::Relaxed);
        let (input, outside) = (before(S_ROOT_INPUT), before(S_ROOT_OUTSIDE));
        root_view(Some(&scope), "test", 0x2100, 0x100, 0x100);
        root_view(Some(&scope), "test", 0x8100, 0x100, 0);
        root_view(Some(&scope), "test", 0x5_0000, 0x100, 0);
        assert!(before(S_ROOT_INPUT) > input);
        assert!(before(S_ROOT_OUTSIDE) > outside);
        let map = OUTSIDE.lock().unwrap();
        assert!(map.as_ref().unwrap().contains_key(&("test".to_owned(), 5)));
    }
}
