# Fork handoff (GromX0/victory-fork, formerly thomb2123/victory-fork)

Working notes so that the next session does not repeat finished work.
Update after every meaningful result. Claims are marked **verified** only
when the run that shows them is named.

## Start here (a session on the owner's PC)

The owner is at the PC and wants to be hands-off: do the setup, build and
runs yourself; they will click Windows admin prompts. They allowed
ordinary development and diagnostic tools (Git, Rust, VS Build Tools and
Windows SDK, PIX, PresentMon, AMD's Radeon Developer Tool Suite); ask
before any driver, BIOS or Windows security change. Keep game files
unchanged and out of Git; logs and caches outside the repository. Commit
as `thomb2123 <thomb2123@users.noreply.github.com>`, keep other authors'
names on their commits, push to `dev/integrate-clean` (`main` follows it
by fast-forward only; never force-push).

1. Detect the real machine (GPU and driver, CPU, RAM, disk, whether
   Resizable BAR / Smart Access Memory is on) and record it under Goal.
2. Build release, run `cargo test --workspace` (first time the GPU shader
   tests run on the RX 9070; failures there are findings, report them).
3. `pwsh tools\fork-run.ps1 -GameExe <exe> -Seconds 600` twice per setting
   (cold, then warm) for: default; `-Profile amd`; `-Profile amd -SetEnv
   @{ VICTORY_RELAY = '2' }`. Record the furthest milestone and the frame
   lines in the table under Goal.
4. Fix the earliest blocker the runs show (see "Next").

Found by the fork watch after the last integration, not yet reviewed:
- Joaco9020/mangrove 5959db7a (`victory warm`: pre-translate every shader
  and pre-build compute pipelines) and 2995c9d5 (`VICTORY_POOL_NO_ASSERT`,
  "gets past the 8-minute crash": check what it disables before using it).
- triippywhale 43b65e23 (twice the dynamic descriptor slots on an
  uncapped heap: the AMD case).
- FQY55kai d7413997 / 11cdbec6 (speed switches on by default; shader fixes).
- tools/fork-watch.sh marks commits as NEW too eagerly; fix it.

## Goal

The title running correctly, stably and responsively on the owner's
Windows PC: first a steady 30 fps in representative gameplay, then 60
where possible. Hardware **detected 2026-11-25**: AMD Radeon RX 9070
16 GB (driver 32.0.32029.2010; DXGI's default adapter, as the host
device's log line shows) beside the CPU's integrated Radeon, Ryzen 7
9800X3D, 32 GB RAM (44-60 GB commit limit, system-managed page file on
C:), Windows 11 Pro 26200. Repository, runs and caches on D: (exFAT: git
needs a `safe.directory` entry there).

Milestones (from execution only):

| | Milestone | State on the owner's PC |
|-|-----------|--------------------------|
| A | Builds, passes applicable checks | **verified** 2026-11-25: release build; `cargo test --workspace` 580 pass after 8dc5898 and 59c92d4 (first run: 3 victory-gpu failures, one a real allocator bug); GPU shader tests ran on the RX 9070 |
| B | Launches, reaches menus | **verified**, run `20261125-092142-default-cold`: start screen at ~1.5 min, 30 fps; Enter (A) accepted |
| C | World loads, renders, takes input | **partly**: same run reached the first cutscene, character lit, some buildings dark or misplaced; best 5.8-6.2 fps (162-172 ms/frame, 48-64 ms on the GPU); input in the world not tried |
| D | Progresses past the first scene; graphics, controls, audio, checkpoint reload usable | not run |
| E | Sustained session, stable, paced | **no**: run `20261125-093256-default-warm` aborted at 373 s, "memory allocation of 2823609 bytes failed" (0xC0000409) after a pipeline refused with E_OUTOFMEMORY; the title commits ~24 GB and other apps held 28 GB of a 44 GB limit. Redo with memory freed |

Findings of those runs, for "Next": the "warm" run still dropped up to
6,292 draws a frame for pipelines being made (the saved library was
327 KB), so pipelines do not survive a launch; five shaders do not
translate (b22a16a178976e1b 16-bit wave sum, ceb5eb68cd99cb07 descriptor
address, 2fd01fca16993001 / 6b6db9f1a526daeb / 7d0f14ce44746d7e descriptor
read as data) and mesh pipeline f5e50eb2108c425b+ce3f0432a08f1d35 is
refused (E_INVALIDARG). `-Profile amd` runs not done yet.

Others' reports, not reproduced here: fxceless (RTX 4070, i7-14700KF)
reaches C at 3-5 fps with shadowed areas black; upstream (RX 6700 XT)
reaches the world at 1-2 s a frame, surfaces unlit.

## Repositories and branches

- `origin` = GromX0/victory-fork (moved from thomb2123/victory-fork on
  2026-11-25; the old address redirects). `main` and
  `claude/gta-vi-victory-fork-b5hg86` both at `a803c87` at session start
  (2026-11-25): identical to fxceless/victory-fork minus its two newest
  commits.
- `fxceless` = fxceless/victory-fork `main` `310c6eb` (2 commits ahead of
  us: depth bounds test #82, faster defaults, packed-texture queue).
- `upstream` = Marvisak/VIctory `main` `940d32f` (last checked
  2026-11-25 ~04:00 UTC). The fork's own work is one squashed commit on
  `ffe310e`; upstream has 319 commits (182 non-merge) since.
- Local `dev/integrate-clean` (worktree `/home/user/wt-integrate`, not
  pushed): fxceless `310c6eb` + 24 clean upstream cherry-picks + 3
  resolved picks + 1 port + 1 lint fix + ztrx's launcher + these notes. This is the build
  to try first on the owner's PC. (`dev/integrate` is an earlier copy
  whose last two commits also carried a Linux build folder by mistake;
  superseded, never pushed, same source tree.)

A full merge of upstream into the fork was tried in memory
(`git merge-tree`): **45 files, 342 conflict hunks** (lower.rs 60,
draw.rs 38, host.rs 35, ...). Both sides rewrote the same subsystems
(page tables for raw title addresses, BVH intersection, copy-back
tracking, pipeline warm-up). Not attempted blind; see "Next".

## Environment (this cloud container, not the owner's PC)

Linux x86-64, 4 cores, 15 GiB RAM, no GPU, no Windows. Toolchain 1.98.1
(rust-toolchain.toml) with the `x86_64-pc-windows-msvc` target.

Cross-checking the Windows build from Linux (reproducible):

```sh
cargo install --locked xwin
xwin --accept-license --arch x86_64 --cache-dir ~/.xwin-cache splat --output ~/.xwin
# ml64.exe -> `llvm-ml-18 -m64 "$@"` wrapper on PATH (blake3's assembly)
export CC_x86_64_pc_windows_msvc=clang-cl-18 AR_x86_64_pc_windows_msvc=llvm-lib
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=lld-link
export CFLAGS_x86_64_pc_windows_msvc="/imsvc ~/.xwin/crt/include /imsvc ~/.xwin/sdk/include/ucrt /imsvc ~/.xwin/sdk/include/um /imsvc ~/.xwin/sdk/include/shared"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-Lnative=~/.xwin/crt/lib/x86_64 -Lnative=~/.xwin/sdk/lib/um/x86_64 -Lnative=~/.xwin/sdk/lib/ucrt/x86_64"
cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked --target x86_64-pc-windows-msvc -- -D warnings
cargo test --workspace --locked --target x86_64-pc-windows-msvc --no-run
cargo build --release -p victory --locked --target x86_64-pc-windows-msvc
```

(Use absolute paths instead of `~` inside the quoted variables.)

Results, 2026-11-25:

| Revision | clippy | clippy all-features | tests link | release build |
|----------|--------|---------------------|-----------|----------------|
| a803c87 (start) | pass | pass | pass | - |
| fxceless 310c6eb | **fail** (3 lints, fixed in 832d4a0) | not run | - | - |
| dev/integrate (bb6fdc7; same source tree as dev/integrate-clean) | pass | pass | pass | pass |

Tests run natively on Linux (crates that build there), same tree:
victory-core 11, victory-formats 24 (+4 ignored), victory-pipeline 4,
victory-game 3, victory-shader 180 (+2 ignored): all pass.
victory-pe: 4 tests read their own executable as a PE image and fail on
Linux (ELF): an environment artifact, not a defect. victory-gpu, -hle,
-loader do not build on Linux (Windows APIs).

**Not run here:** every GPU-backed test (`valu_tests`, `waterfall_tests`,
`diff_tests`, `wave64_tests`, `intersect_tests` are `cfg(windows)`; the
`diff_tests` also need the console and PC HLSL compilers), and all
Windows-only crates' tests. Wine 9.0 was tried and aborts at start in
this VM (`free(): invalid pointer`); not pursued.

## Changes on dev/integrate-clean (since fxceless 310c6eb)

Clean cherry-picks of upstream (authorship kept, `-x` trailers):
sampler point-mip and CreateSamplerX fixes (e45bacd, 23fc8a5, 7dfa86f,
e6fe8ec, e34cb53; the device's AnisoFilterWithPointMip support differs
between vendors), scalar quadmask (c3a04e2, e3fe5bf), v_cvt_off_f32_i4,
normalized buffer ends, signed 64-bit literals, FLAT offset bit 11,
A8_UNORM family, TDR settings log, descriptor wraparound scan, AVX2
pre-flight, clwb/clzero/rdpru (60d2cf9, 0de9efc, 274f698, 514927d),
s_atc_probe, v_sad*, v_mul_hi_*24, rectangle attribute interpolation,
depth/stencil copy footprints.

Resolved by hand:
- c430b1d tbuffer d16 loads: kept the fork's narrow-access table, took
  upstream's d16 rule for tbuffer too.
- c0228d9 mesh primitive indices: masks to 9 bits. In this fork the
  10-bit mask included the corner's edge flag, and the fork also culls a
  primitive whose index is past the vertex count, so any primitive
  exported with an edge flag set was dropped. Layout per the RDNA
  primitive export (index 9 bits + edge flag per corner, null bit 31, as
  Mesa's ac_build_export_prim packs it). Whether the title sets edge flags
  is **unknown**; no regression test (the mesh tests need the GDK
  compilers).
- 13185f5 GetDeviceRemovedReason: both sides added a device method;
  kept both, took only the healthy-device test.
- 0601955 helper lanes in v_readfirstlane (port, by Vax upstream): a
  waterfall loop in a pixel shader could spin to the loop limit in helper
  lanes (upstream saw 0.3-1.3 s draws and DEVICE_HUNG). Regression test
  `a_pixel_first_lane_read_asks_whether_the_lane_is_a_helper` (fails
  without the change). Effect on this fork's frame time: **not measured**.

Own: 832d4a0 makes fxceless's two newest commits pass CI clippy again
(no behaviour change). Also a candidate contribution to fxceless.

Picks not taken (conflict, and judged not worth a blind resolution now):
1e923ad (already in the fork), e29391f, 691236d, 4ab20fc, 4e60ea3,
7f00a62, f47ebd8, 12eba49.

## From other forks (2026-11-25, reviewed commit by commit)

Taken, authors kept (`-x` trailers name the source commit). All
compile-checked and clippy-clean for Windows; **none run on a GPU yet**.

| Source (tested on) | Commits | What |
|---|---|---|
| ezbyDev/victory-amd (RX 9060 XT, RDNA4) | b13d0a6d, 8e8c69a6, e2706a4e | pipeline library: one save at a time (an overlap left a corrupt library and minutes of rebuilding), a save thread and saves on crash/exit paths, honest save result |
| same | 35d7748e (adapted) | `VICTORY_SKIP_PIPELINES` in release builds and before early compiles; pipeline workers/priority switches |
| ka1qux and its sources (RX 7800 XT, RDNA3) | 68190e62, 144fb081, 8492bb9e, c88c3755, dc502e1f, f47507f4, 714494dc, 99c85747, 15cc76c3, bd797b41 | early-texture heap check; textures uploaded before the decoration bake (black skin/clothes); DirectStorage page lists filled late (models/textures left unwritten); AutoSubmitDisabled; AMD compiler crash guard for buffer sizes; BC7 dictionary entry 0; textures changed outside the guard; interpolation first halves; bounded texture re-looks (7.5 GB of re-uploads claimed) |
| triippywhale line (RX 7900 XT) | 329cf50e | the fork's own clean-up passes reused annotateHandle across blocks: AMD compiler crash |
| Slaktarn700/xs-proton (RX 9070 XT, Linux) | 0f799933, a7773fff | heap slots the title rewrites in its memory mirrored before each submission, settled first under late waits: device loss 5-90 s into the world (umr wave dumps) |
| TaynanGT / upstream PRs | e3e7ab3b, b1d3e1c1, 4bc295ef, 90423b1d | staging buffer overwritten while the card copies from it under the relay (on by default here); card hash compare 32 lanes a piece; no full-wait barrier between indirect draws |
| Scarlight (RX 9070 XT, Linux) | bc5d6ab6 | indirect clamp read past a tight argument buffer |
| others | d802b4c1, 097b67b3, 6989081f, e2fa4fcd, f2fbbb29, 0b3223af | R9G9B9E5 targets as FP16 (device loss); no RTV for unrenderable formats; v_div_fixup zero/inf; APU heap use-after-free; 16-bit mix low half (deferred material dropped); a late frame flips at once |

Own follow-ups: lint fixes for the picks (95d01592), the accounting and
diagnostics lines adapted to ka1qux's texture-queue fields.

Not taken, and why:
- Pipeline skips of rendering work by default (ka1qux 5f98a38c, myokex
  4436f8f1: 348 ray-traced hit programs), Mauricio's RT_STUB / POPS_SKIP /
  INTERP_ZERO, JordanFromIT's RT/GI stand-ins: they remove or fake
  rendering work. Opt-in candidates only, after measurement.
- e8cad1e1 (heap capped at 1M): would null slot ~1,061,522, which the
  title reads; the branch's 2^21 clamp is right.
- `VICTORY_INDIRECT_CLAMP=0` (5805143d): removes the garbage guard.
  Dispatches over 65,535 groups (ka1qux 2c150419) still to be ported
  without it: the 115,200-group final gather is truncated today.
- victory-amd f2969f6f (hung its card), c4ca121e (WIP, always on),
  4bece454 (always-on lock change: review first), e1ad89d9 (wave64 retry
  on 64-lane hosts: changes every wave64 program, needs narrowing).
- chapter-ii's LogicOp clearing (wrong) and low-address allocator
  (always on), VICTORY_INPLACE_POOL (commit leak).
- myokex's texture span rewrite: competes with fxceless 310c6eb, already in.

Next candidates (conflict, need a hand port): faccfbc7 (64-bit compares
wrong on the RX 9070's scalar unit), eb5061cc/66a81a58 (integer-target
blending refused by the host), fe3a0bcf (sRGB/linear views), 0e49a91c
(46 dropped world programs), 2c150419 (dispatch parts), Scarlight's
streamed-mip set, Taynan's CPU set, ebc2c918 and the ray-tracing
prerequisites (none lights anything yet).

## Upstream and fork watch

| Checked | Source | Revision | Note |
|---------|--------|----------|------|
| 2026-11-25 | Marvisak/VIctory main | 940d32f | 182 non-merge commits since ffe310e; README now: world drawn, unlit, 1-2 s/frame on RX 6700 XT; "some compute programs need 64-lane waves" (the RX 9070 has them, the RTX 4070 does not) |
| 2026-11-25 | fxceless/victory-fork main | 310c6eb | depth bounds (#82), defaults ~230 ms/frame on RTX 4070 (claim) |
| 2026-11-25 | 17 forks in tools/fork-watch.txt | see tools/fork-watch.sh | reviewed; results in "From other forks" |
| 2026-11-25 | Zettrox/ztrx main | 90b529f | no code of its own: fxceless's history to 2119b0e plus a README and `scripts/play.ps1` (taken, 02afc8d). Claim: on an RTX 3050 the prologue plays from the cell to the boat chase at 1-3 fps, 1 h 30 min without a crash, at 2119b0e's code. So 2119b0e is the known-stable fallback to bisect against if a newer build crashes |

Second check, 2026-11-25 ~10:30 UTC, from the owner's PC (`tools/fork-watch.sh`
output and the GitHub fork listings are in `D:\victory-runs`, outside the
repository):

| Source | Revision | Note |
|--------|----------|------|
| Marvisak/VIctory main | 940d32f | nothing new |
| fxceless/victory-fork main | f85101b | 4 commits (8dd30ab, a26859f, 9376fd6, f85101b): **merged**, see "Merged on dev/merge-fx-klbf" |
| PastiiJay/klbf-lab-beta `klbf-amd` | 74315e76 (Lab 35) | RX 9070 XT, driver 32.0.32029.2010 (the owner's): Series X ~4.8-5.2 fps (as ours), Series S + AMD FSR ~7.3 fps, 74 min; `39f0e9aeb156a61c` never leaves the AMD compiler and grows memory ~240 MB/s (fits our 24 GB commit and run 2's out-of-memory stop). Its own AMD commits **picked**; its Lab 28/31/33 squashes (myokex walls, upstream #334 and #1271 ports, `translate --cache --pipelines` pre-build) not yet: 70 conflict hunks as one, mostly the same fixes taken twice |
| justr4mzi/VIctory | 424d9f88 | RX 6700 XT on fxceless 9376fd6: AMD pipeline skips (myokex, ZSlayer), reuse pass off on AMD (ztrx#61, amdxc64 crash), a remap under an uploaded texture uploads it again (#574) |
| triippywhale/victory-fork | (116 since the first check) | shader fixes: early depth for pixel programs, array LOD without slice, whole-wave `Sym::Own` lanes; textures refused for memory retried |
| Skillter/victory-fork-plus | | "stop rewriting the pipeline library for a few hundred kilobytes", older library generations dropped: may be why our second launches rebuilt thousands of pipelines |
| Joaco9020/mangrove | | `victory warm` (all shaders and compute pipelines ahead), `VICTORY_POOL_NO_ASSERT` (8-minute crash: check what it disables) |
| Xeltax/VIcky, dkdknight, Trexies, TaynanGT, FQY55kai, JordanFromIT | | research and feature branches; not reviewed yet |

Notable upstream work not yet in the fork: `translate --cache` shader
warm-up and `--pipelines` pre-build from the shader pack (9c58a1c,
c47b529: affects first-run stutter); page-marked copy-backs (cd94d35,
940d32f: overlaps the fork's card hashing); FidelityFX paths and the BVH
intersection (0b35b7f: overlaps the fork's ray-tracing work).

GitHub API access in this session covers only thomb2123/victory-fork (no
issues, no PRs). Upstream and fxceless are read through git only, so
their PR and issue discussions have not been read.

## First run on the owner's PC (needs the game folder)

Questions the first runs answer, smallest first:

1. Does `dev/integrate-clean` build and pass `cargo test --workspace` on the
   RX 9070? This is the first run of the GPU shader tests, including the
   picked fixes, on RDNA4.
2. Which build gets further on AMD: `dev/integrate-clean` (fork line, tested on
   NVIDIA) or `upstream/main` (tested on AMD)? Same scene, same settings,
   separate cache folders.
3. If the newest build crashes or hangs early: the same run at 2119b0e
   (Zettrox's 1 h 30 min stable report on NVIDIA), to tell a regression in
   the 36 newer commits from an AMD-specific failure.
4. Furthest milestone (B/C) and the release build's periodic frame line
   (`fps, ms/frame (preparing ..., waiting ...)`) after the shader cache
   is warm (second launch), not the first.

```powershell
git fetch origin; git switch dev/integrate-clean   # once it is pushed
copy victory.example.toml victory.toml        # game_dir, cache_dir, data_dir outside the repo
pwsh tools\fork-run.ps1 -GameExe '<game dir>\<title exe>' -Tests -Seconds 600 -Label integrate-cold
pwsh tools\fork-run.ps1 -GameExe '<game dir>\<title exe>' -NoBuild -Seconds 600 -Label integrate-warm
```

For plain play without measurement: `powershell -ExecutionPolicy Bypass -File
scripts\play.ps1 -Game '<game dir>\<title exe>'` (Zettrox's launcher).

AMD settings to A/B (claims, none shown on an RX 9070 yet): `-Profile amd`
sets relay off, `LATE_WAIT=3`, `SHADER_OPT=11` and normal-priority pipeline
workers (ezbyDev/victory-amd, RX 9060 XT RDNA4: the relay removed the
device there). The RDNA3 forks (victory-chapter-ii on an RX 7900 XTX,
4.1-4.8 fps) ran with the relay on without device loss, so run both:
`-Profile amd` and `-Profile amd -SetEnv @{ VICTORY_RELAY = '2' }`.
`-AmdSkips` adds victory-amd's five skipped compute programs (removes
their work; only after a run shows the compiler crash or hang).
`-Diag` is needed for `VICTORY_SHADER_LOOP_LIMIT`. The shader cache key
hashes the executable, so every rebuild starts cold: compare warm runs
of one build.

`tools/fork-run.ps1` clears leftover scripted-press and capture switches
(as `play.ps1` does) unless `-SetEnv` asks for them, records commit, GPU and driver, CPU, OS, disk and
`VICTORY_*` variables, keeps a copy of the executable under
`%LOCALAPPDATA%\victory\runs\builds\<commit>`, and writes the logs there.
It was written without PowerShell to test it on: expect a first fix.

## Merged on dev/merge-fx-klbf (2026-11-25, on the owner's PC)

- `7321a1d5` merges fxceless f85101b. Hand resolutions are in its message;
  the one with a design choice: fxceless's `sync_heaps` replaces our
  `resync_title_writes`, but still settles flights in the air before it
  changes a slot (now through a callback, inside `Host::submit`'s lock).
- KLBF's own AMD commits, authors kept, `-x` trailers: 3f84c4ef (wave32
  pixel and vertex programs per 32-lane half on 64-lane hosts:
  `VICTORY_PS_HALVES=0` off), 32f35992 (the pixel wave size is in the
  translation fingerprint), 77688fae and 69249586 (per-half reductions;
  denormal flush only on request, `VICTORY_DENORM`), 18d0518b (NaN guard
  on colour exports and typed stores, `VICTORY_NAN_GUARD=0` off; applied in
  `emit/lower/export.rs`), e07a9c4a (Lab 35: remapped texture ranges kept
  through loading). Skipped 5f8cc12a: our ka1qux queue switches
  (`VICTORY_TEXTURE_STAGE_MB/MS`, with catch-up) already do it.
- `emit/lower.rs` hit the asset policy's 512 KiB per-file limit with both
  lines' shader work: `export` and `interp` moved, unchanged, to
  `emit/lower/`.
- Checks on the RX 9070: release build, workspace clippy `-D warnings`,
  policy; `cargo test --workspace` 624 pass. One fails:
  `warp_budget_saturates_without_wraparound_and_preserves_draw_fields`, an
  order dependence of fxceless's own test (fails alone, passes in the full
  suite, on fxceless f85101b too).
- `tools/fork-run.ps1 -Console series-s` for KLBF's faster mode.

Runs of this branch (5fc4460452), `VICTORY_SKIP_PIPELINES=39f0e9aeb156a61c`,
default profile, first launches (translator changed: cache cold):

| Run | Console | Result | World at | Steady (median, last 3 min) |
|-----|---------|--------|----------|------------------------------|
| `20261125-104343-merged-seriess` | Series S | **device removed at 171 s** (0x887A0006, no breadcrumbs), black window before it; compute pipelines of the title's FSR 4 passes were being made (3.4 s), and `fill_ahead` had just caught a fill that read bytes a flight then changed | - | - |
| `20261125-104735-merged-seriesx` | Series X | 420 s, no error, no device loss | 72 s (178 before) | **6.3 fps, 158 ms/frame** (4.9 fps, 202 ms before): preparing 59, GPU 76, copying back 79 ms; 27.2 GB committed (24 before) |

Seen in the Series X run (owner's screen): characters lit and textured
indoors, but glowing eyes, an over-exposed image and a magenta patch.

| `20261125-110017-fx-switches` | Series X, `VICTORY_ASYNC_SUBMIT=1 VICTORY_QUEUE2=1 VICTORY_TEXTURE_NO_WAIT=1 VICTORY_INDIRECT_WINDOW=1` (fxceless's frame-rate switches) | 420 s, no error, no device loss | 127 s | **7.4 fps, 135 ms/frame** (peaks 8.0): preparing 64, GPU **50** (76 without), copying back **30** (79 without); ~4,470 draws (a later scene: the save moves on) |

Seen in that run (a bar, later in the story): a character's face drawn as a
dark stretched slab ("web triangles", also in KLBF's open list), magenta on
faces and a bottle, dark areas black (no ray-traced light: the structure
builder is off unless `VICTORY_BVH_BUILDER=1`). Owner's screenshot of the
cell, same build: lit through the window, purple papers, magenta blocks.

Ray tracing, state of the forks: Xeltax/VIcky `rt` (VICKY-RT.md) runs the
builder on an RTX 4080 at 502-583 ms a frame against 361 off, and the dark
corner of the cell is not lit yet. Research, not a switch to turn on.

Lighting and texture runs on the RX 9070 (same scene, the cell; screenshots
taken during the runs):

| Run | Switches | Result |
|-----|----------|--------|
| `20261125-111141-rt-miss` | `VICTORY_RT_MISS=1` (429e3e5a, after Xeltax 3fb7f967) | sun patches and cast shadows on the walls; ceiling, corners and floor still near black; purple papers, a magenta block, a pink rope. 6.5-8.0 fps at 5,100-7,500 draws, no error |
| `20261125-111938-bvh-levels` | `VICTORY_BVH_BUILDER=1` + levels on demand | the builder's 53 pipelines made, then the **title crashed** 4 s later (98 s in): several threads read garbage pointers at once, `0x7f8000007f800000` (two float infinities) in registers. The builder's writes corrupt title memory. Do not turn it on |
| `20261125-112150-levels-rtmiss` | `VICTORY_LEVELS_ON_DEMAND=1` + `VICTORY_RT_MISS=1` | **the cell lit** (ceiling lights, walls, door, floor), purple blocks and pink rope gone, **gameplay reached** ("Approach a cellmate", then "Go with the prison guard"); 480 s, no error; 5.0-6.8 fps at 6,800-7,500 draws; the card ~55% busy |

Both now on by default (df9ac61a). Still wrong in that run: the brightest
lines of some ceiling lights (and white papers) turn purple, as if green
is lost in very bright colours (look at how R9G9B9E5/R11G11B10 targets and
the FP16 stand-in for R9G9B9E5 carry bright values); the exposure overshoots
for the first seconds after a load (the title's auto exposure catching up at
0.3 fps); bounce light indoors is approximate (rays miss).

### 2026-11-25 night (RX 9070 16 GB, Ryzen 7 9800X3D, 32 GB)

| Run | What changed | Result |
| --- | --- | --- |
| `faces-textures` | upstream #334 buffer-state tracking (17b3e5e8) + `VICTORY_TEXTURE_STAGE_MB=1024`, `VICTORY_TEXTURE_STAGE_MS=1000` | **faces render** (the woman in the cell has a face), 7.0 fps at ~5,900 draws, 480 s, no error |
| `nanguard-off` | NaN guard off | purple unchanged, so the guard is not the cause; device lost at 239 s |
| `pool-inplace` | `VICTORY_INPLACE_POOL=1` (6b29560a) | works, but slower: 4.0-4.9 fps; 438 s, no error; 7.9 GB of commit still held after exit (agents were building at the same time, so the number is not clean) |
| `seriess-v2` | `-Console series-s` | stuck on a black start screen |
| `seriess-v3` | `-Console series-s` | out of memory at 168 s (0xC0000409, system commit peaked at 62.2 GB) |

Findings:

- Faces and characters were the implicit buffer promotion on AMD: a buffer
  written by a shader and read by the next draw in the same list kept stale
  data. `BufferStates` (port of upstream #334) gives each host copy an
  explicit read or write state per command; `VICTORY_BUFFER_STATES=0` turns
  it off.
- Resizable BAR is on: the adapter reports `GPUUploadHeapSupported`
  (6143d2bb logs it at start), so title memory could live in GPU upload heaps
  that the processor writes straight into, instead of host copies.
- In place probe: `OpenExistingHeapFromFileMapping` takes private and
  committed (`SEC_COMMIT`) sections and refuses the reserved pool sections;
  the title has ~10.5 GB of pool views. `VICTORY_INPLACE_POOL=1` commits the
  pool as ≤1 GiB sections and places buffers in them. It runs, but slower
  than the copies (4.0-4.9 against 7.0 fps), so it stays off by default; the
  next try is GPU upload heaps for the hot ranges only.
- Commit, not RAM, is what runs out: with the game closed the system's
  commit sat at 38.5 of 47.8 GB, ~17 GB of it held outside any process (by
  the driver, released only by a restart) and SignalRGB growing by 9-12 GB.
  `fork-run.ps1` now records the commit before, at peak and after, stops the
  game when it grows by more than `-CommitGrowLimitGB` (38) or the page
  file's drive drops below `-PageDriveMinGB` (6), and warns when more than
  2 GB is still held after exit (c7daebbf).
- Series S is black because the title draws through FidelityFX FSR 4
  (`amd_fidelityfx_loader_xbox.dll`) and `DeserializeComputePipelineStateX`
  is not implemented; this tree has no FidelityFX path. Upstream 0b35b7f
  runs FidelityFX on the host (`VICTORY_FIDELITYFX_DIR`), which needs AMD's
  `amd_fidelityfx_upscaler_dx12.dll` from the FidelityFX SDK (not
  downloaded; needs the owner's approval).
- Device losses: DRED reports a page fault at a wild GPU address while
  "textures picked by index made ready"; intermittent (239 s, never in the
  480 s runs with #334).
- Purple only on AMD: the brightest colours (ceiling lights, white paper,
  eyes) lose green. The NaN guard is ruled out.
- Still refused: the mesh pipeline `f5e50eb2108c425b+ce3f0432a08f1d35`
  (`E_INVALIDARG`).

Work in progress on branches (worktrees under `D:\dev\wt-*`): `agent/mesh`
(the refused mesh pipeline), `agent/pipelines` (pipeline library kept
between launches and a warm-up), `agent/purple` (the purple colours: shader
analysis and a capture reader), `agent/rt` (`docs/raytracing-plan.md`: rays
that hit without the title's BVH builder corrupting title memory),
`agent/harvest` (fixes from other forks), `agent/fsr` (upstream FidelityFX
on the host).

## Next

1. Purple in the brightest colours: captures of a frame with a ceiling light
   in a diagnostics build (`VICTORY_CAPTURE`, `VICTORY_DUMP_TARGETS=1`) and
   the pass where green drops out: `tools\purple-pass.ps1 -Folder <capture>`
   names it (README, Debugging). Checked on the RX 9070 (agent/purple, GPU
   tests): R9G9B9E5 is a real target format there (render, blend, typed UAV;
   NVIDIA gets the RGBA16F stand-in) and keeps every channel of bright colours
   (`targets_tests`); the host's f16 conversion rounds toward zero with 65504
   for any overflow, where the console gives infinity from 65520 on, the same
   in both halves of packed results (`VICTORY_F16_NEAREST=1` now covers
   `v_cvt_f16_f32`, `v_pk_*` and `v_fma_mix*` too); and a console wave64
   `ds_bpermute_b32` run as one 64-lane wave read across the wave's halves,
   AMD only (fixed; `VICTORY_BPERMUTE_HALVES=0` for the old reading).
2. Frame rate: the card is ~55% busy; the CPU side (copies 40-55 ms,
   preparing 70-90 ms a frame) is the limit.
3. Series S device loss: try `VICTORY_FILL_WAIT=1` (fxceless's fills that
   no longer wait for flights, new and NVIDIA-only so far), then the FSR 4
   pipelines (the title's `FFX_FSR4_*` passes; KLBF runs Series S with FSR
   on an RX 9070 XT without device loss, on its own stack).
4. Pipelines that survive a launch (Skillter's pipeline-library fixes,
   upstream `translate --cache --pipelines`, mangrove's `victory warm`).
5. KLBF's Lab 31/33 work as separate ports: myokex's wall fixes, upstream
   #334 (buffer-state tracking: characters and faces), #1271 (`glc` loads
   coherent on RDNA).
6. Then the frame line's dominant cost: copying to and from the card, not
   the GPU.
