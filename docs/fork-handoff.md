# Fork handoff (thomb2123/victory-fork)

Working notes so that the next session does not repeat finished work.
Update after every meaningful result. Claims are marked **verified** only
when the run that shows them is named.

## Goal

The title running correctly, stably and responsively on the owner's
Windows PC: first a steady 30 fps in representative gameplay, then 60
where possible. Expected hardware (owner's statement, **not yet
detected**): Ryzen 7 9800X3D, Radeon RX 9070 16 GB, 32 GB RAM, Windows.

Milestones (from execution only):

| | Milestone | State on the owner's PC |
|-|-----------|--------------------------|
| A | Builds, passes applicable checks | cross-checked from Linux only (see below); not yet built or tested on Windows |
| B | Launches, reaches menus | not run |
| C | World loads, renders, takes input | not run |
| D | Progresses past the first scene; graphics, controls, audio, checkpoint reload usable | not run |
| E | Sustained session, stable, paced | not run |

Others' reports, not reproduced here: fxceless (RTX 4070, i7-14700KF)
reaches C at 3-5 fps with shadowed areas black; upstream (RX 6700 XT)
reaches the world at 1-2 s a frame, surfaces unlit.

## Repositories and branches

- `origin` = thomb2123/victory-fork. `main` and
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

## Next

1. Push `dev/integrate-clean` (needs the owner's go-ahead), then the first run
   above.
2. From that evidence: if the fork line hangs or misrenders on AMD,
   bisect the fork's GPU changes against upstream on the same scene; if
   it renders, profile the frame line's dominant cost.
3. Bring `translate --cache --pipelines` (warm-up) into the fork line, or
   move the fork's work onto upstream, decided by step 2's result.
