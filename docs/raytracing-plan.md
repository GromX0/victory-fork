# Ray tracing on the RX 9070: plan

Status, 2026-11-25, branch `agent/rt`. This plan covers how to make the title's
ray-traced shadows, indirect light and reflections correct on an RX 9070
(RDNA4, DXR 1.1, native 64-lane waves) at a cost we can accept. Each part gives
what to do, why, the evidence for it, the effort and the risks. The first step
is in this commit: a trace of the builder's entry points (`VICTORY_BVH_TRACE`).

## Recommendation in one paragraph

**Keep the title's own traversal programs and make the structures they walk
correct. Use the builder's entry points, which we can now see, to do it.**
Start with the title's own builder (path A). First trace it (step 1, done).
Then hold its stores inside the memory each call was given (step 2). Then
check the structures it builds on the host (step 3). If the title's 53 builder
programs cannot be made correct in bounded time, swap in our own builder
behind the same entry points (path H1). H1 writes the console format, so the
traversal programs still do not change. Host hardware ray tracing for the
traversal (path B) is not the first path. Its build half is easy: the title
passes standard D3D12 descriptions. Its traversal half means rewriting 716
node-test sites in 64 programs, with alpha tests inside the loops. It is kept
for one narrow case: shadow-only loops (B-shadow) once cost is measured.
Until then, rays miss by default (`rt_miss`). That is a labelled
approximation: rooms are lit and bounce light is flat.

## What the title does (evidence)

### The builder is a C++ class with standard D3D12 inputs

`victory inspect --exports --imports <game_dir>\XboxBvhBuilder.dll` lists 12
exports. They are methods of `class XboxBvhBuilder`. Its imports are only the
CRT, `D3D12SerializeVersionedRootSignature` and `XMemVirtualQuery`. The
title (`<title>.exe`) imports 7 of the 12:

| export (undecorated) | RVA | what the DLL's code does |
|---|---|---|
| `XboxBvhBuilder(ID3D12Device11*, bool)` | 0x21330 | makes its inner builders; when the flag is set it skips one (0x2136f) |
| `~XboxBvhBuilder()` | 0x21590 | |
| `BuildRaytracingAccelerationStructureBatched2X(ID3D12GraphicsCommandList6*, UINT, const D3D12XBOX_..._BATCHED_DESC2*)` | 0x216a0 | descriptions 80 bytes apart: a standard 48-byte `D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC`, the post-build request count at byte 48 and their array at byte 56 (16 bytes each); the destination must be 256-byte aligned; after the build, when a global flag is set, a UAV barrier on the list (vtable offset 0xD0) |
| `CopyRaytracingAccelerationStructure(list, dest, source, mode)` | 0x21980 | acts only on mode 1 (compact) |
| `GetRaytracingAccelerationStructurePrebuildInfo(inputs, D3D12XBOX_XDXR_STREAM_DESC*, info*)` | 0x21880 | 24 bytes of standard inputs, 16 of an Xbox stream description |
| `RehydrateRaytracingAccelerationStructureBatched2X(list, UINT, const DESC*, const UINT64*)` | 0x219d0 | standard build descriptions; the check at 0x21a00 reads geometry descriptions 56 bytes apart and compares a hash of them with the serialized source's dword at +8 |
| `Set64BitMortonCodes(bool)` | 0x21bd0 | a global flag |

The non-batched `BuildRaytracingAccelerationStructure` (0x21610) copies its
48-byte description into a batched one and calls the batched entry. That
pins the layout. The geometry descriptions are the public 56-byte
`D3D12_RAYTRACING_GEOMETRY_DESC`. Disassembly: `dumpbin /disasm
/range:0x180021330,0x180021c40` on the DLL. The notes are kept outside the
repository, under `D:\victory-scratch\raytracing\`.

So **every build arrives with its geometry and its memory in a public
format**: vertex and index buffer addresses, formats, counts, transforms,
instance description address, destination, scratch, source, and the
post-build requests. This changes how feasible paths B and H1 are. The fork
notes (`docs/fork-ray-tracing.md`) say "the layer would have to know the
geometry behind structures the title never describes to it". That is not so:
the title describes all of it, to the DLL.

### The static world ships prebuilt and is rehydrated

The packs hold prebuilt structures (`G:\pack\core_bvh4\core_bvh4.rpf`,
`story000_bvh4.rpf` and others, opened in run
`20261125-111938-bvh-levels` at log lines 439 and 4745). The title asks the
DLL to *rehydrate* them, and rehydration runs as builder GPU work (fork PR
#646 fixed its thread counts). JordanFromIT's BVH home run HB4 (commit
e6fe0510) found that 141 of 729 bottom levels that a top level pointed to
were "in title memory alone": built ready by the title's models.

### The traversal is the console compiler's lowering of RayQuery

`victory shader-info --disasm <game_dir>\shaders.rpf` decodes 26,503 containers.
None carries DXIL bitcode: "with LLVM bitcode 0". So the HLSL RayQuery
source is gone, and only GFX10 code is left. **64 programs** use the node
test, with **716 sites** in all (638 `image_bvh64_intersect_ray`, 78
`image_bvh_intersect_ray`). The families are `rtdirectionalshadows`
(sun shadows, up to 72 sites and 51 texture samples in one program:
`CS_RTShadowTrace32HQ_Tiled`), `shadowmask`, `indirect_light_probes`,
`indirect_diffuse_lighting` (screen probes, deferred gather, PPR
reflections), `sr_tracing` (reflections, VBuffer), `deferred_tiled` (laser
light shadow, closest depth) and `GpuParticleCollision`. The census per
program is kept outside the repository: `D:\victory-scratch\raytracing\bvh-census.txt`.

The smallest one, `indirect_light_probes:CS_IrradianceCache_ShadowTrace`
(539 instructions), shows the shape of every loop:

- a state machine in one VGPR, with values 0, 3, 4 and 8, and a cap of 0xffff
  iterations;
- a short stack: three entries in registers, sorted with
  `v_min3/med3/max3_u32`, and spill and pop through LDS
  (`ds_write_b32`, `ds_wrxchg_rtn_b32`);
- the structure's header read through the descriptor's base, at bytes 48, 52,
  56 and 0x74 (`global_load_dword`);
- instance nodes (kind 6) read by hand at node+48 (16 bytes: the
  bottom-level address in 256-byte units, mask and flags) and
  node+64..+108 (the world-to-object rows). The ray is transformed in code;
- on exit, a committed hit in two registers (the hit node and t) that the
  rest of the program turns into its output (here 0.5 or 0, the
  visibility).

The header and instance layouts agree with JordanFromIT's reading of the
public Xbox traversal header (`XboxBvh4Header`, `XboxBvh4Instance`, as
reproduced in Gaijin's DagorEngine `XDXRStandaloneTraverseShortStackCPU.hlsli`;
see `crates/victory-gpu/src/rtwindow.rs` on
`watch-JordanFromIT-CGBJ-VIctory/wip/bvhhome`). In that layout the elements
offset is at 0x34, the parents offset at 0x38, an instance is 128 bytes, and
its bottom level is at +60 in 256-byte units.

### What runs today

- The node test is emulated in DXIL
  (`crates/victory-shader/src/emit/intersect.rs:81-256`). The nodes are read
  through title memory by address (`Lower::memory`,
  `crates/victory-shader/src/emit/lower.rs:2854`), with one directory lookup
  per node, or two for a 128-byte box node. It is tested bit for bit
  against the Rust reference `crates/victory-shader/src/bvh.rs` on the card.
- With the builder off (the default), every node test answers "no child"
  (`rt_miss`, `intersect.rs:67`, `:123`). Shadow rays see nothing in the way,
  and bounce rays reach the sky.
- The builder runs only with `VICTORY_BVH_BUILDER=1`, and all or none
  (`crates/victory-gpu/src/pipeline.rs:200-313`). On this card all 53
  pipelines are made ("the ray-tracing structure builder runs ... pipelines=53",
  run `20261125-111938-bvh-levels`, `victory.err.log` line 9084).

## Path A: the title's builder and the software traversal

### Why its output corrupts title memory

The crash (run `20261125-111938-bvh-levels`, `crashes-7952987A.log`) has
several threads faulting at once on pointers loaded from the title's heap.
The registers hold these patterns:

- `0x7f8000007f800000` (+inf, +inf)
- `0xff8000007f800000` (+inf, -inf)
- `0xff800000ff800000` (-inf, -inf)

These are the three 8-byte words of an **empty bounding box**
{+inf, +inf, +inf, -inf, -inf, -inf}. A builder writes this as the start
value of a min/max reduction over bounds, often as whole arrays, one per node
or primitive. So arrays of builder scratch landed on the title's heap
objects. It is not stale data copied back, and it is not a finished
structure written over its neighbours: the builder **stored at wrong
addresses**. These ways for that to happen are known, each with evidence:

1. **Lost accesses once the copies run out.** Title memory by address uses
   at most 1,023 numbered copies (`crates/victory-gpu/src/memory.rs:59`).
   chapter-ii measured on the same AMD card: the builder's first world
   frame numbers 1,013 copies, and from the second frame on "hundreds of
   pages a frame have no copy (their reads give 0, their stores are lost)".
   With `VICTORY_BVH_QUARANTINE=1` the title runs on (chapter-ii README at
   d0f69012, lines 123-134). A stage that reads 0 for a count or an offset
   computes its store addresses from it.
2. **Pointers taken out of memory that were never made ready.** Only what a
   dispatch names in root views and root constants gets a copy beforehand
   (`memory.rs:33-38`, the shortcut on InitHeaders' address table and
   LoadInstances' records). Anything else is a lost access for that frame.
3. **Two copies of one title page.** A page of a title buffer with its own
   copy is also given the enclosing window's copy number (`memory.rs:160-178`,
   the shortcut). Root views bind the buffer's copy, and raw addresses bind
   the directory's. A stage that writes through one and is read through the
   other reads stale bytes.
4. **Copy-back carries wrong stores into the title.** Every builder dispatch
   declares every numbered copy written (`memory.rs:301-308`). The structures
   lie between the title's own objects (`pipeline.rs:203-207`, run 98). So the
   4 MiB windows compared after the submission hold title heap objects. The
   merge then writes each byte the GPU changed (`shadow.rs:2485-2551`, "Only
   the GPU's bytes") into title memory, wrong addresses included.
5. **In place, non-contiguous pool pages** (chapter-ii d76b632e: "root views
   on pool pages that do not follow one another end at the first jump").
   It was mended in d0f69012 by routing root views through the directory page
   by page. Still, "the title still dies within seconds", with "100 to 500
   pages a frame touched at addresses that are not the title's memory at
   all", and "Next: name the program for each such address" (chapter-ii
   README at d0f69012, lines 146-152).

**Does in-place pool memory fix it?** It is necessary but not sufficient. It
removes (1) and (3) for pool memory: no 1,023 limit, one place per page. It
removes the cost of (4): Xeltax's RT8 on an RTX 4080 spent 58-65 ms copying
back with the builder on, and chapter-ii's builder frame fell from 600-2,000
ms to about 270. It does nothing about stores at wrong addresses, whatever
their cause: (2), lost accesses outside the pool, a builder program that does
not translate exactly, or wrong dispatch sizes or indirect arguments. In
place, such a store reaches the title's objects **at once and without the
merge's filter**. So the in-place pool makes a store guard (step 2) more
urgent, not less.

### Traversal cost

On the console the node test is one instruction. Here it is about 100-150
DXIL operations: four slab tests and a sorting network, or a watertight
triangle test. It also takes 16-28 dwords of raw loads and one directory load
per node, two for a 128-byte box node. Xeltax's numbers on an RTX 4080 with
copies (`watch-Xeltax-VIcky/rt:VICKY-RT.md`): GPU time 185 ms per frame with
the builder off, and 282-306 ms at RT8 with it on, builder and traversal
together. That is 361 ms against 502-583 ms per frame overall. The RX 9070
has not been measured, and nothing separates the builder from the traversal
yet. The 64 programs run every frame whatever the builder does, so their
emulation cost is paid today, with `rt_miss` making every loop exit at once.
With real structures, the loops run their full depth. Expect the
traversal-heavy passes to cost several times their console time. Step 1's
counts and a per-pass timing (step 3) put a number on it.

**Effort:** step 2 takes 3-5 days, and step 3 2-3 days. Making all 53 programs
correct is open-ended: 1-3 weeks with `VICTORY_BVH_SKIP` (fork PR #579) and
the checker. **Risks:** a program that does not translate exactly
(wave64 semantics, float atomics, `DispatchX` flags), which fails silently;
the title reading results back, such as compacted sizes; and frame cost.

## Path B: host hardware ray tracing (DXR 1.1)

### B-build: HLE the entry points and build D3D12 structures

This is feasible. The 7 imports take public descriptions (see above). A host
module would:

- for each bottom level, read the geometry descriptions and build a D3D12
  BLAS from host copies (or in-place views) of the vertex and index buffers;
- for each top level, copy the instance descriptions, rewrite each
  instance's console bottom-level address into the host BLAS's address in a
  small compute pass (the descriptions are GPU memory the title may write on
  the GPU), and build a D3D12 TLAS;
- answer the size queries with the host's prebuild sizes, or with the
  console's, kept as the title expects;
- write the post-build results (compacted size and the others) where the
  title reads them back;
- copy or compact on the host.

**Open question that step 1 answers:** do rehydrations carry real geometry
(vertex buffer addresses and counts), or only enough to match the blob's
hash? If they carry none, the static world's triangles must be decoded from
the serialized console blobs, whose triangle nodes hold their vertices.
That is doable at load time, but it means knowing the blob format. Effort:
1-2 weeks. Risk: low for correctness, because all writes are host-side; there
is a memory cost for host copies of geometry.

### B-trace: translate the traversal loops to RayQuery

This is feasible only in a narrow form. What would have to happen:

- recognise each of the 716 node-test sites as part of one compiler-made loop
  (the shape is regular, see above);
- find its inputs at loop entry: origin, direction, t range, flags, mask and
  the top level's descriptor;
- find every use of its outputs after the loop, and rewrite them.

The hard parts:

- **Hit attributes are not loop outputs.** The program after the loop takes
  the hit node pointer and loads instance and triangle fields from node
  memory itself. Every such load would have to become a `RayQuery`
  intrinsic (`CommittedInstanceIndex`, `CommittedPrimitiveIndex` and so
  on), which means dataflow analysis across the whole program.
- **Non-opaque geometry is handled inside the loop.** The programs that
  alpha-test sample textures between node tests (`CS_RTShadowTrace32HQ_Tiled`:
  72 sites, 51 samples; `CS_GatherAlphaVBuffer`). That code would have to be
  cut out and moved into a `Proceed()` candidate loop.
- **One format for everything.** If any traversal program still walks the
  console format, the console-format structures must still exist. A partial
  move to RayQuery needs both a host BLAS/TLAS and the console structures.
- **Fragility.** A patch to the title recompiles all 64 programs.

Assessment: a general translator is about two months of work and fragile.
**B-shadow** is a narrower target: programs whose loop result is only
"hit or not" and that have no samples inside the loop
(`CS_IrradianceCache_ShadowTrace`, `CS_RaytraceShadows_*`,
`CS_LaserLightShadowRaytracing`, `shadowmask:CS_TraceShadowRays_Fullres`).
That is about 2-3 weeks on top of B-build, worth it only if step 3's timing
shows those passes dominate. RDNA4's node-test hardware cannot be reached
from DXIL: D3D12 exposes only whole-ray traversal. Whether AMD's driver
extensions offer anything closer has not been checked.

## Path H1: our builder in the console format (hybrid)

Behind the same entry points as B-build, write **console-format** structures
instead of D3D12 ones, into exactly `[dest, dest + size)`. The traversal
programs and the node-test emulation stay as they are.

- **Format:** the AMD node formats (64-byte box nodes with half or float
  bounds, triangle nodes, instance nodes) are already pinned by
  `crates/victory-shader/src/bvh.rs` and its bit-for-bit card tests. The Xbox
  header and instance record come from the public layout above, and the
  disassembled traversal checks them (header +0x34, +0x38; instance +48..+108).
- **Builds:** static and rehydrated bottom levels are built once on the CPU,
  or copied from the blob if rehydration is a copy plus relocation (step 1's
  sizes tell). Per-frame bottom levels (skinned characters, vehicles) are
  refitted or rebuilt by a small compute program of ours (LBVH). The top
  level is built every frame on the GPU from the instance descriptions.
- **Why it is attractive:** our stores are bounded by construction, so this
  corruption cannot happen. It runs at host speed with no title memory
  machinery for the builder. It can be tested offline: build, walk with the
  `bvh.rs` reference, and compare with brute-force ray/triangle tests over
  the same input.
- **Risks:** every field the traversal and the hit shading read must match.
  That includes primitive and geometry index encodings, instance flags and
  masks, and the parent pointers used by the short stack's restart. Some of
  these can only be read from the 64 programs.

**Effort:** 2-4 weeks. This is the fallback if A stalls, and the natural
place for the CPU-side checker of step 3 to grow into.

## Path C: stand-ins now (approximations, labelled as such)

| stand-in | where | what it gives | what is wrong |
|---|---|---|---|
| rays miss (`rt_miss`, default) | `intersect.rs:67` (after Xeltax 3fb7f967) | lit rooms; sun shadows only where the shadow maps give them | no ray-traced occlusion; bounce rays see the sky, so interiors' indirect light is flat |
| RT stand-in (JordanFromIT 12731908) | fork | clears the outputs of RT dispatches that were dropped to "unoccluded" | moot here: those programs translate and run |
| GI stand-in (JordanFromIT 645f6032) | fork | a constant indirect level written into what the tiled lighting reads | constant ambient: dark corners stay as dark as everything else is bright |

None of these can give directional bounce or contact occlusion without
geometry. The cheapest real improvement is H1's static part. It means a top
level over the static world's own prebuilt bottom levels, with dynamic
objects left out. It also needs rehydration to run correctly, or to be done
by us. That is why step 1 counts rehydrations and their geometry.

## Phases

1. **Observe** (steps 1 to 3 below). There is no change to the picture.
   Corruption becomes impossible while the guard is on.
2. **A, guarded:** builder on, with the in-place pool (main session) plus the
   guard plus the checker. Find each wrong store's program from the guard's
   log, and fix translation or host input until the checker is clean and the
   dark corner is lit. Measure the cost per pass.
3. **If A stalls or costs too much: H1**, at the same entry points. Do the
   static world first (rehydrations and the top level), then the dynamic
   bottom levels.
4. **Only if the shadow passes dominate the cost: B-shadow** over B-build, for
   the shadow-only loops.

## The first three steps

### Step 1 (this commit): trace the builder's entry points

`crates/victory-hle/src/modules/bvhbuilder.rs` with `VICTORY_BVH_TRACE=1`
binds the title's 7 imports of the DLL to wrappers. Each wrapper decodes the
call and then calls the DLL's own function with the same arguments
(`apps/victory/src/run.rs`, `Traced::new(&with_dlls)` for the exe's binding).
With the switch unset, nothing changes.

It logs:

- the first 32 calls of each entry point in full (`=<n>` or `=all` for more):
  destination, source, scratch and their sizes, flags, counts and the first
  four geometry descriptions;
- every 10 s, a summary: bottom and top builds and updates, triangles,
  boxes, instances, the largest of each, rehydrations with their geometry and
  triangles, compactions, post-build requests by type, vertex and index
  formats, and how many destinations a size query sized;
- the structures known so far by destination address.

The size queries are matched to builds by their inputs, ignoring the update
flag. That gives each structure's extent, which step 2 needs. Unit tests
cover the layouts, the signature, the resolver and a wrapped query and build
reaching stand-ins for the DLL.

**Result** (run `20261125-130040-merged-keys-64fd2470b1`, builder off,
about 3 minutes in the cell):

- **Volume.** Per 10 s there are 36-155 batched build calls. They hold
  440-1,899 bottom-level builds (100-1,256 of them updates) and 25-158
  top-level builds of up to 3,933 instances. That is 2-17 million triangles,
  plus 9-74 rehydration calls with 89-1,105 structures. Size queries come
  at about one per build. There are no compactions, although compacted-size
  results are asked for (189-974 per 10 s).
- **Memory.** By the end there were 5 top levels and 6,784 bottom levels by
  destination. Destinations lie in plain title memory at 0xd5...-0xd7...,
  the same region as the heap objects of the crash. The title packs them
  back to back at exactly the queried sizes, and scratch in the 64 MiB
  title buffer at 0x100b620000 the same way. Top levels match their queries
  exactly.
- **Sizes.** Bottom levels with R16_UINT indices match their queries.
  Those with the title's index format 53 never do exactly, though the
  answers fit the packing. So the guard takes sizes by a looser key (level,
  flags, count, primitives).
- **Rehydrations** relocate in place: destination and source are the same,
  with no scratch. Their per-call key is the destination minus
  0x6_0000_0000. Their size comes only from the structure's header, which
  the title loaded into title memory.
- **Formats.** Vertex formats are R32G32B32_FLOAT and R32G32_FLOAT (stride 8,
  some strides with bit 15 set). Rehydrations carry geometry with real
  vertex and index addresses. So host builds of the static world from the
  same inputs (B-build or H1) are possible.

### Step 2: hold the builder's stores inside what each call was given

**Done, as a copy-back guard (`VICTORY_BVH_GUARD=1`,
`crates/victory-gpu/src/bvhguard.rs`).**

- Each wrapper opens a scope with the call's write ranges while the DLL
  runs. The writes are the destination, scratch and post-build results,
  plus buffers the DLL makes for itself. The reads are geometry, instances
  and the source.
- Sizes come from an exact query, then a loose one, then an earlier build at
  the same destination, then the header in title memory (rehydrations),
  then the smallest answer with enough primitives. Each build's source is
  counted in the trace's `sized_by`.
- A builder dispatch declares written, for the copy-back, only the parts of
  its root views and of its copies of title memory by address that lie
  inside its call's ranges. Other programs that read title memory by
  address and store through no address (the traversal programs) no longer
  declare those copies written.
- Root views outside every range are logged by program and page.

What it does not do yet: see single stores, or stop them with title memory
in place. That needs the check in the translated stores below. It changes
the translator, which re-keys the whole shader cache, and it needs card
test support for root UAVs, so it comes as its own commit.

The plan for that per-store check:

- The wrappers already know each call's ranges: destination plus size,
  scratch plus size or update size, post-build destinations, and the
  compaction destination. Hand them to `victory-gpu` as a **builder scope**
  on the command list: begin it when the wrapper is entered and end it when
  the DLL returns. Every dispatch the DLL records in between carries the
  scope.
- Builder programs (those with a spilled table) get a small per-dispatch
  range table, bound like the memory directory. Their stores through raw
  addresses (`Lower::memory`, `memory_flat`) and through root views check
  the store address against it. A store outside every range is dropped, and
  its 64 KiB page and the program's hash go into a fault ring read once a
  frame. The log then names the program and the address (chapter-ii's
  "next").
- With copies (not in place), the copy-back of builder submissions is also
  cut to those ranges (`shadow.rs`, `copy_back_with`).
- Turn it on with `VICTORY_BVH_GUARD=1`, implied by `VICTORY_BVH_BUILDER=1`
  once it is tested.
- Tests: the lowering's range check against `TEST_MEMORY` (as in
  `intersect_tests.rs`), host scope bookkeeping, and the copy-back cut.

### Step 3: check the structures on the host and time the passes

**Done (`VICTORY_BVH_CHECK=1`, `crates/victory-gpu/src/bvhcheck.rs`).**

- **Checks.** A thread of the HLE reads, once a second, the structures the
  wrappers know whose last build is at least a second old. Top levels go
  first, then the oldest, within 64 MiB a second (`=<n>` sets n MiB). Each
  structure is read from title memory, where the guard's copy-back or the
  in-place pool puts it, and walked from the root at header+0x30.
- **What is counted:**
  - bad headers and roots;
  - pointers outside the structure;
  - nodes reached twice;
  - NaN and inverted boxes, and leaves of the wrong level;
  - triangles and instance transforms that are not finite;
  - instances whose bottom level no call named (dangling);
  - depth.

  Every 10 s one line per class: top levels, bottom levels built, bottom
  levels rehydrated.
- **The control group.** Rehydrated bottom levels come ready built in the
  title's data. If they walk clean and built ones do not, the builder is
  wrong. If both fail, the checker's reading of the format is.
- **Timing.** Each dispatch of a builder program, and of a program with a
  node test, sits between two GPU timestamps in its host list. They are read
  when the list is reset and summed by program, and a line every 10 s gives
  builder and traversal milliseconds and dispatches a frame and the slowest
  8 programs. Dispatches overlap on the card, so the sums are a pass's
  time; single programs are approximate.
- **Shortcut.** A structure being copied back while it is read can show
  faults it does not have. Top levels are rebuilt every frame, so a few of
  theirs may be torn reads. A fault in every top level, line after line,
  is not.

The original plan for this step:

- Once a frame, for each top and bottom level the trace knows, walk the
  console format (header, box, triangle and instance nodes per `bvh.rs` and
  the Xbox layout). Count:
  - children outside `[dest, dest + size)`;
  - NaN or inverted boxes;
  - non-finite vertices;
  - depth;
  - instances pointing to no known bottom level.
- Log counts only. This is the correctness signal for A and the test oracle
  for H1.
- Time the 64 traversal programs and the builder's dispatches separately in
  the frame line, so that cost is known per pass on the RX 9070.

### The first builder-on run with the guard (`20261125-192940`)

**What happened.** The builder ran (53 pipelines). About 1.5 s later the
title faulted. The fault was in a function at title RVA 0x55ceb20 that
reads `[rcx + rdx*8]`, with `rcx = 0x5f6951797f800000`. That address is
not canonical: Windows reports it as a read of 0xffffffffffffffff. So the
title read float data as a pointer (+inf in its low dword), as in the
crash without the guard. Nothing read all-ones back.

**Where the pointer was.** It is field +0x18 of an object at 0x6ea4ff300
(caller at RVA 0x37c9f). Its twin `+0x6_0000_0000`, 0xcea4ff318, lay
0xe00 bytes below the header of a rehydrated structure at 0xcea500100,
whose key was 0x6ea500100. That is in the 1 MiB block before the
structure's own.

**Why the guard let it through.** The guard's range for that rehydration
was the header's size plus 28,096 bytes before it, because the header's
dword at 0x38 was read as a negative offset to parent pointers. It is
not one, or not in this memory. So the guard let a store into another
object's memory reach the title.

**Fixes (this commit).**
- Rehydration and update ranges start at the header
  (`bvhguard::header_size`).
- The second cause: `DispatchX` with flag 0x20 (`USE_THREAD_DIMENSIONS`,
  counts in threads), which the builder uses, was dispatched as that many
  groups. That is up to 64 times the threads, and the extra ones store past
  their arrays. The host now dispatches the groups that hold the threads.
  This is the host half of fork PR #646. Its translator half, masking the
  extra lanes of a last partial group, is still to port. It re-keys the
  shader cache.

## The first in-game test for the main session

Build `agent/rt`, or cherry-pick its commits. Then run once with the current
defaults (builder off, rays miss) plus the trace:

```
pwsh tools\fork-run.ps1 -GameExe '<game_dir>\<title>.exe' -OutDir 'D:\victory-runs' -Label bvh-trace -Seconds 300 -SetEnv @{ VICTORY_BVH_TRACE = '1' }
```

This is the switch set of `20261125-112150-levels-rtmiss` (levels on demand and
`rt_miss` are default since df9ac61a; keep `VICTORY_SKIP_PIPELINES` as that
run had it) **plus `VICTORY_BVH_TRACE=1` and nothing else**. The title calls
the DLL whether or not its GPU work runs, so this test is safe. Expected in
`victory.err.log`:

- "the ray-tracing structure builder's entry points are traced";
- detailed lines "a structure", "a size query", "a copy";
- every 10 s, a summary.

Read these from it:

- builds per 10 s by level, triangles and instances, and the largest;
- whether rehydrations carry geometry (vertex addresses and counts), which
  decides between B-build and H1 for the static world;
- the share of destinations a size query sized (step 2 needs most of them);
- compactions and post-build request types: does the title read compacted
  sizes back?

Do **not** run `VICTORY_BVH_BUILDER=1` again before step 2's guard exists,
except with `VICTORY_BVH_QUARANTINE=1`, which keeps copies and the title
alive.
