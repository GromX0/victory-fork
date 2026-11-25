# GTA VI on PC through the VIctory fork: progress record

**Objective (kept across sessions):** the user's own copy of GTA VI, rendering correctly and running at playable
performance on the user's PC, through [fxceless/victory-fork](https://github.com/fxceless/victory-fork). We work on the
compatibility layer only. Obtaining or redistributing game files, and bypassing protection, are out of scope.

The fork is a Windows-only Rust project and a fork of [VIctory](https://github.com/Marvisak/VIctory). The title's
x86-64 code runs unchanged in-process, `victory-hle` supplies the console's platform services, `victory-gpu` maps
D3D12.X onto host D3D12, and `victory-shader` recompiles the console's RDNA2 GPU code to DXIL when the title creates a
pipeline.

**Where things live (since 2026-11-25):** everything is in `thomb2123/victory-fork`. Code goes on its branches
(below). These notes, the scripts (`tools/`) and draft posts (`posts/`) are on its **`notes`** branch, which shares
no history with the code. The earlier `thomb2123/Gh` repository is retired; the user asked for it to be deleted.
Get the notes with `git fetch origin notes && git worktree add ../victory-notes notes`.

This file is the record across sessions. Update it at the end of every session with the commit, commands, results,
hypotheses and next steps.

## Working rules (from the user, 2026-11-25)

- Game files stay outside Git and are never modified. No game data in this repository or the fork, including
  screenshots, shader dumps and logs that quote game strings (the fork's `docs/asset-policy.md`). Logs stay on the PC;
  only summaries come here.
- The fork author's results are a **hypothesis to reproduce on the user's machine**, not facts.
- No success-returning stubs, no invented console or hardware behaviour, and a dropped draw never counts as a
  correctness fix. Mark assumptions as assumptions.
- One change at a time: measure a baseline, change one thing, rerun **the same** test with the same switches, and
  record both numbers.

## Evidence levels (never conflate these)

| Level | Meaning | Verified by us | Claimed by the fork author (one PC: i7-14700KF, RTX 4070, Win 11) |
|---|---|---|---|
| L0 Build | Workspace compiles, clippy clean | ✅ cross-build in the cloud (Session 2) | ✅ |
| L1 Unit tests | Tests with no game data | ⚠️ partial: everything runnable without a GPU passes; 23 GPU tests need a real card | ✅ (CI on windows-latest) |
| L2 Executes | The title's code runs: boot, menus | ❌ not yet on the user's PC | ✅ menus at 30 fps |
| L3 Displays | Frames presented: menus, cutscene, world | ❌ | ✅ cutscene; world renders |
| L4 Renders correctly | The world without missing passes | ❌ | ❌ everything in shadow is black (ray-traced light not running); a room's interior sometimes missing |
| L5 Playable | Input, save load, stable, playable frame time | ❌ | ❌ 2–4 fps (240–450 ms/frame) in the world; input, pause menu and save load reported working |

Target for L5, **our assumption until the user says otherwise**: the title's own 30 fps (33.3 ms/frame) in the first
gameplay scene, with no hang or device loss over a 10-minute session.

## Current state

- Fork `main` @ `c96b4f2` (2026-11-25), as reported by its author (RTX 4070). The depth bounds test now works, so
  the first room is lit by its lamp. The speed switches are now **on by default**: relay stage 2, wide copy-back,
  card buffers 1 MiB to 1,024 MB, 256 lists in flight. About 230 ms/frame where it was about 400. Textures are
  uploaded before the decoration bake, and there's one host copy per placed texture. Known: the first room's walls
  are sometimes missing, and shaded areas are still too dark (no ray-tracing structures).
  **Its plain build fails CI's clippy gate** (`placed_filler_twins` never used without the diagnostics feature).
  Our branch `fix-clippy-placed-filler` fixes it.
- User's PC (stated by the user, 2026-11-25; to be confirmed by the baseline's `hardware.txt`): **AMD Ryzen 7
  9800X3D** (Zen 5), **AMD Radeon RX 9070** (RDNA 4) and **32 GB RAM**. OS, driver version, page file and the game
  copy: unknown. Others report ~25 GB process memory in the world (#424) and 24.4 GB resident with the builder on (#543),
  so 32 GB is enough but close: keep the page file system-managed. Nothing
  has been reproduced there yet.
- **This is not the fork author's hardware** (Intel + NVIDIA), so their results are a weaker hypothesis for us.
  What follows from the code (inferences, checked against the source, not yet run):
  - CPU: the console is Zen 2. `crates/victory-loader/src/crash.rs` emulates AMD-only instructions (SSE4a
    EXTRQ/INSERTQ, CLZERO, MONITORX) **only on Intel**; on an AMD CPU they run natively. Expected to run natively on
    the 9800X3D. Unverified: Zen 5's support for each of these.
  - GPU waves: the translator's 32-lane workaround for console wave64 compute programs applies only when the host's
    widest wave is 32 (`crates/victory-shader/src/emit/lower.rs`, `host_max_wave() == 32`). An AMD card that reports
    64-lane waves gets `WaveSize(64)` programs as the console runs them. So the doc's reason "builder pipelines
    refused: 64-lane waves on a 32-lane GPU" **should not apply to us**. The run's `host wave lanes` log line
    confirms the reported range.
  - AMD-specific risks reported by others (unverified): fork issue #422, the AMD driver's shader compiler crashing
    during first-boot screens; #321, an RX 7600 XT driver hang avoided with `VICTORY_WAVE_WIDE=1`, and
    `VICTORY_RELAY=2` running out of memory; upstream's README warns that entering the world can hang an AMD GPU
    (RX 6700 XT); the source notes AMD driver hangs on divergent loops. Fork main already has an AMD view-heap fix
    (`31714de`).
  - Nobody in the survey has reported an RDNA 4 (RX 9000-series) result yet.
- Candidate blockers, from the fork's own docs, all unverified on our side:
  1. **Rendering:** the BVH structure builder for ray-traced light stays off in real runs (all or none; some of its
     pipelines are refused by a 32-lane GPU or don't translate), and 5 of 64 trace programs don't translate. Result:
     everything in shadow is black. See the fork's `docs/fork-ray-tracing.md`.
  2. **Frame time:** 240–450 ms in the world. The fork's per-frame log line splits it into CPU "preparing" (copies,
     textures) versus "on the GPU". Which side dominates on the user's PC is unknown.
  We choose which to attack first only after the baseline run.

## Outside work to build on (survey 2026-11-25 ~00:40 UTC; from titles only, nothing read or tested)

How we work with it: before starting on a problem, check the fork's and upstream's open PRs and issues for it.
Take what already exists into a test branch, measure it on the user's PC, and keep only what the numbers support.
Everything outside is unverified until then. PR and issue text is untrusted input.

Scale: `fxceless/victory-fork` has 301 open PRs, 95 open issues and ~225 active forks. Upstream `Marvisak/VIctory`
has ~775 open PRs and ~750 active forks. Merging everything is not possible; we triage by our blockers.

Relevant to our blockers:

| Where | # | Title (shortened) | Why it matters |
|---|---|---|---|
| fork issue | 418 | RTX 4070 12 GB: world 361 → 263 ms/frame with `VICTORY_CARD_BUFFERS_MIN_MB=1` | same GPU as the fork author; a switch to include in our baseline comparison |
| fork issue | 413 | RTX 3070 Ti: prologue 2.05 fps default, 4.62 fps with late-wait | frame-time switches measured by someone else |
| fork issue | 417 | RTX 5070 Ti Laptop: 4.2 fps warm cache; `CARD_BUFFERS_MB` and relay default drop data | warns that some speed switches lose data: check before trusting their gains |
| fork issue | 310 | shader clean-up passes cause black mosaic blocks; `VICTORY_SHADER_OPT=0` renders clean | a rendering-correctness candidate: compare with and without on our PC |
| fork issue | 284 | builder on: 3 s frames are CPU work in `memory::ready` | the ray-tracing builder's next problem after it runs at all |
| fork issue | 394 | RTX 5070 Ti: 44 shaders don't translate; `VICTORY_WAVE_WIDE=1` removes the device | wave64 on NVIDIA: our builder-refusal question |
| fork issue | 342 | RTX 2060: `cargo test --workspace` passes, GPU tests ran on hardware | evidence that the 23 GPU tests pass on real NVIDIA cards |
| fork PR | 414 | BVH sorted box hits: an absent child can sort before a real hit | ray-tracing correctness fix |
| upstream PR | 1890 | console wave64 compute on 32-lane hosts (split waves) | could unblock builder pipelines refused on NVIDIA |
| upstream PR | 1903 | ray test's triangle multiplies stay apart from adds | ray-test precision |
| upstream PR | 1883 | lane masks a traversal keeps as numbers | ray traversal translation |
| upstream PRs | 1898, 1894, 1888, 1884, 1869, 1865, 1873 | pipeline scheduling, upload pools, copy plans, memory-guard batching, copy-back ranges, dirty bitmaps, detile scratch | CPU-side frame-time candidates; which matter depends on our baseline split |

Triage of batch 2026-11-25 00:43 (a background agent read the a803c87 diff and the PR/issue pages; not tested by us):

| Where | # | What | For us |
|---|---|---|---|
| fork main | a803c87 | textures picked by index are re-checked after upload (4/12/40/120/400 frames) and re-uploaded if changed. Fixes the main character's face rendering black and hair pale in ~3 of 4 launches. `VICTORY_HEAP_RECHECK=0` turns it off | correctness fix, on main. Measure the frame-time cost with and without it |
| fork PR | 426 | DirectStorage read refused (`0x800706f8`) into guarded memory copied through a buffer. Fixed a cold-shader-cache crash at world load on an RTX 3070 Ti (3/3 crashed before, 3/3 reached the world after) | take it if our first runs hit that error or breakpoint `0x80000003` |
| fork issue | 424 | RTX 3090: 60 min stable, 278 ms mean (3.6 fps); 170–570 draws per frame dropped, mostly the ray-tracing builder; ~25 GB process memory, +4.5 GB at a scene change | slow frames aren't NVIDIA-only. **RAM matters: need the user's RAM size** |
| fork PR | 423 | Series S compositor; adds `VICTORY_HEAP_TEXTURES_MS/MB` budgets | later, as a frame-time switch |
| fork issue | 427 | RTX 4060: 6 shaders don't translate, 7 pipelines refused (`0x80070057`) | same pattern as the builder question |
| fork PR | 414 | BVH box test: an absent child (pointer `0xFFFFFFFF`) whose bounds pass the slab test no longer gets a finite sort key | **Reviewed by us, not taken.** It applies cleanly on `a803c87` and the shader tests pass (172; 16 ray-test). But AMD's own open-source emulation of this instruction, Mesa RADV `intersect_ray_amd_software_box` (`src/amd/vulkan/nir/radv_nir_rt_common.c`, Mesa `e175081`, 2026-11-24), turns a child off only by a NaN minimum x and never checks the pointer, which is what the fork does now. The PR would move the fork away from AMD's own implementation without evidence of console behaviour. Hardware behaviour is still unestablished |

Triage of batch 2026-11-25 00:53 (agent read PRs #436–#446 as commits and test merges, #431 and upstream #1915 pages; the rest by title):

| Where | # | What | For us |
|---|---|---|---|
| upstream PR | 1915 | lowers 32/64-bit global atomics: cmpswap, inc, dec, csub, `_x2` | the fork's raw-address path (`lower.rs` ~2883) refuses exactly these today. **Candidate for the 3 untranslated builder programs**, if the PC run names one of them; needs porting to that path, not a cherry-pick |
| fork issue | 431 | RTX 2060 on `a803c87`: frame time doesn't change from 4K to 480p; mostly waiting (fetches 225–450 ms, submitting thread 140–340 ms, remapped title memory 75–500 ms). `VICTORY_RELAY=2 VICTORY_PIPELINE_WAIT_MS=24 VICTORY_CARD_BUFFERS_MIN_MB=1 VICTORY_CARD_BUFFERS_MB=1024` cut compares 430 → 35 MB/frame, best frame 172 ms | not resolution-bound, so not raw GPU power. **Our second baseline run uses this switch set**. Mind #417's report that some of these drop data |
| fork PR | 437 | lite device-removed report in every build: last op finished, first unfinished with its pipeline, last 64 ops | for AMD hangs; in `integration`. Cost unmeasured |
| fork PRs | 440–446 | translator miscompile fixes (dropped side effects, read-modify-write as overwrite, wrong skips, inactive-lane scalar conditions, `_g16` address decode, mixed descriptors) | in `integration`. Synthetic tests only, and some make the translator refuse more programs: recount builder (49/52) and trace (59/64) translation after the PC run |
| fork PR | 438 | textures up to 8 frames stale (intro flicker) fixed; tested on WARP only | in `integration` |
| upstream PR | 1917 | opt-in AMD frame generation in host upscaling | later only: it doesn't lower real frame time |

Triage of batch 2026-11-25 01:46 (53 fork items, 41 upstream PRs; agent read about 15, the rest by title):

| Where | # | What | For us |
|---|---|---|---|
| fork PR | 156 (in bundle #487) | `slot_address` lowering, not wave-width-specific. On 32-lane hosts with the bundle, 4 of 5 shadow traces and 52/52 builder programs translate. The 3 builder failures #497 addresses are 32-lane-specific | **in `integration`.** On the 9070's 64-lane waves the builder may already translate in full: the PC run's translate counts settle it |
| fork PR | 452 | buffer `getDimensions` heap handles marked non-uniform, against an `amdxc64.dll` 0xc0000005 driver crash. An RX 7800 XT reviewer couldn't reproduce it and suspects RDNA 4 (an RX 9070 XT is cited) | **in `integration`**: likely our fork #422 risk |
| upstream PR | 1936 | refuse translated shaders the DXIL validator rejects. On an RX 9060 XT (RDNA 4), runs went from 42–90 s to 5 min 40 s before failing | fallback if `amdxc64` still crashes with #452 |
| fork PR | 464 | BVH: reject partial-axis NaN and inverted boxes (min ≤ max) | **declined, as #414**: RADV checks only min-x NaN, and fmin/fmax orders inverted boxes; the fork already matches it |
| fork PR | 467 | time split in the fps line | changes the same format string as our fix. Expect a conflict; no measured frame-time change |
| fork PRs | 465, 470 | NVIDIA measurements: remap-lock and texture waits; ~300 pipelines stuck waiting; a pipeline-cache double-create race in #470's comments | switches to try: `VICTORY_COPY_COMPARE=1`, `VICTORY_TEXTURE_NO_WAIT=1` |
| fork PR | 497 | builder runs on 32-lane GPUs, but the title crashes in ~15 s | NVIDIA path; not ours |
| upstream PR | 1923 | async submissions on a host fence timeline: 16–33 ms/frame saved on an RTX 5090; its AMD repeated-recording test fails, and it conflicts with the fork's relay | hold |
| fork PR | 450 | report of a fork `myokex/victory-amd` (adds upstream #175, scoped title-memory locks); reached the world on an RTX 4080 | not tested on AMD; watch |

### RDNA 4: what others have found (batch 2026-11-25 02:17; an agent read fork #505, #543, #422, #558 and upstream #1972)

This replaces guesswork about the user's card, but it is still other people's results, not ours.

1. **Driver shader-compiler crash.** Fork #505 (RX 9070, Adrenalin 26.11.2) crashes in `amdxc64.dll+0x861b9f`
   before the world. #422 ties it to the translator's clean-up pass 4 (REUSE). **`VICTORY_SHADER_OPT=59` (every
   pass except 4) clears it**, confirmed in #543 on an RX 9070 XT at `c96b4f2`. Two programs (`c1eee35dbc50a9ef`,
   `f16876929fa64f9f`) still crash with every pass off; upstream #1972 adds tooling to skip pipelines the driver
   crashes on. **Every run on the user's PC starts with `VICTORY_SHADER_OPT=59`.** #452 (in `integration`) hasn't
   been tried against this crash.
2. **The ray-tracing builder runs on RDNA 4.** #543: with 64-lane waves every builder pipeline gets a host pipeline
   (`VICTORY_BVH_BUILDER=1`, `SHADER_OPT=59`, `LOOP_LIMIT=256`, `RELAY=2`). The frame looked lit, but refresh of
   title memory grew to 86,959 MB/frame (3,752 ms/frame, 0 fps). With the builder off, one run lost the device
   (page fault at `0x2a0b000`). **On RDNA 4 the shadow blocker is now the cost of keeping title memory in sync,
   not translation.**
3. **What the 86 GB is: correction.** We read #543 directly. The author calls it "86 GB per frame of *texture*
   refresh" ("every builder write invalidates host copies, and the refresh path re-uploads the world"). That is
   texture uploads, not the `memory.rs` copies. Our first explanation (every numbered copy refreshed once per host
   list, in `memory::ready()`) doesn't account for it, and is **withdrawn as the explanation**. The per-list refresh
   in `ready()` is real, but nobody has measured its share. Open hypothesis: builder writes reach title memory
   through copy-back, and write watching then marks textures that share those pages as changed, so whole textures
   are re-uploaded although their texels are the same. The diagnostics build's `copies per frame:` line separates
   the cases (`textures X MB watched and changed, Y MB hashed, Z MB of them changed`); `baseline.ps1` now extracts
   that line.
4. **Levers, cheapest first:**
   a. `VICTORY_INPLACE=1`: title memory opened as a graphics heap, so builder writes need no copy-back. Per
      `Shadow::gpu_writes` the card's writes aren't seen by write watching, so watched invalidation should vanish
      too. Experimental, first stage only, untested on AMD (#580 used it on an NVIDIA laptop to fill in more of the
      first room).
   b. Fewer lists in flight (`VICTORY_LATE_WAIT_LISTS`), to test any per-list cost.
   c. Code: which fix depends on what the split shows. A shader-written dirty-page mask (`memory.rs`'s own
      suggestion) bounds the copy-back. A finer-grained texture invalidation would address watched false
      positives. **Paused until measured**: designed, not started.

Upstream #1972 (find and skip pipelines the driver's compiler crashes on) **doesn't merge into the fork**: it is
based on upstream VIctory and conflicts in ~45 files. A port is only worth it if the two programs that still crash
with `SHADER_OPT=59` stop our run.

First-run order on the PC (on `integration`): warm the shader cache with **the same switches as the run**, since
`SHADER_OPT` is part of the cache key: `$env:VICTORY_SHADER_OPT='59'; target\release\victory.exe translate
--cache <game dir>`. Then the baseline run.

PC run plan for the shadows (after the plain baseline; all with `VICTORY_SHADER_OPT=59`, on `integration`):
`VICTORY_BVH_BUILDER=1 VICTORY_BVH_QUARANTINE=1` (reproduce #543 and get our own numbers), then the same plus
`VICTORY_INPLACE=1`.

### Read cursor: only what is newer than this is read next time

| Source | Last seen |
|---|---|
| `fxceless/victory-fork` main | commit `c96b4f2` (read and triaged 2026-11-25 ~01:50 UTC) |
| `fxceless/victory-fork` PRs / issues | #561 |
| `Marvisak/VIctory` open PRs | #1981 |
| Forks | surveyed 2026-11-25 ~00:40 UTC (most-recently-updated list) |

### Watching between passes

`tools/watch.sh STATE_DIR 600` runs in the background while a session is open. It costs no tokens: every
10 minutes it checks both mains (`git ls-remote`) and new PR/issue numbers (GitHub search). When something is new it
exits and wakes the session, which hands the batch to a background triage agent and restarts the watcher. Only
items the agent rates important for us interrupt the user. Its state lives in the session scratchpad. After a
container reset, seed it from the cursor above. The 3-hourly scheduled task covers the time no session is open.

### The continuous loop (each pass, whether started by the user or by the schedule)

1. Read only what is new since the cursor: `git fetch` both mains, plus PRs and issues numbered above it. Add the
   relevant items to the table above. Advance the cursor.
2. Our open patches: rebase onto fork main and drop any that landed upstream. Then build, run CI's clippy gates, and
   run the tests that can run here.
3. The current blocker. If the user's PC is attached: run the experiment and measure. If not: do only work that unit
   tests can check here (translator, ray-test maths), and mark it unverified on a GPU.
4. Commit and push our branch and these notes. A pull request to `fxceless` is shown to the user first.

## Plan for the first session at the PC (agreed 2026-11-25)

The user will make a separate folder for the code checkout and tell us where the game is. We work in both: we build
and run in the checkout, and **only read** the game folder. The game folder stays outside every repository, and
nothing from it is copied into Git, the cloud session or posts.

Posting: when the PC is connected, try posting from there with the user's own GitHub login (`gh auth login`
done by the user once). Re-read each thread first and drop anything already answered. Drafts: `posts/`.

## Waiting on the user (at the PC)

1. The paths: the victory-fork checkout, and the game directory (the folder with the title's executable).
2. Run the baseline script (below) and send back `summary.txt`. It records CPU, GPU and driver, RAM, page file, OS,
   disk and tool versions (Rust, git, Windows SDK). If tools are missing, install them first: `rustup` (the repo pins
   1.98.1), Git, and Visual Studio Build Tools with the "Desktop development with C++" workload.
3. What you see in the window: a short description, and screenshots shared in chat **only if you choose**. They
   never go into Git.

## How to run the baseline (Windows, PowerShell)

```powershell
cd <victory-fork checkout>
git clone https://github.com/thomb2123/victory-fork; cd victory-fork; git fetch origin notes   # notes checkout: git worktree add ..\victory-notes origin/notes
git checkout builder-diagnostics              # run 1; then mine/integration for the A/B
copy victory.example.toml victory.toml             # set game_dir (outside the repo)
# RX 9070: always pass VICTORY_SHADER_OPT=59 (driver compiler crash, fork #505/#422); add it to every -Switches below too
powershell -ExecutionPolicy Bypass -File <notes checkout>\tools\baseline.ps1 -GameExe '<game dir>\<the title's exe>' -Switches @{ VICTORY_SHADER_OPT = '59' }
# the speed switches #431 used are now the defaults (c96b4f2); compare with the old defaults, and with #465/#470's:
powershell -ExecutionPolicy Bypass -File <notes checkout>\tools\baseline.ps1 -SkipTests -SkipBuild -GameExe '...' -Switches @{ VICTORY_RELAY = '0'; VICTORY_WIDE_BACK = '0'; VICTORY_CARD_BUFFERS_MIN_MB = '16'; VICTORY_CARD_BUFFERS_MB = '512'; VICTORY_LATE_WAIT_LISTS = '128' }
powershell -ExecutionPolicy Bypass -File <notes checkout>\tools\baseline.ps1 -SkipTests -SkipBuild -GameExe '...' -Switches @{ VICTORY_TEXTURE_NO_WAIT = '1' }
# the ray-tracing builder question (diagnostics build; send the "structure builder" lines of summary.txt):
powershell -ExecutionPolicy Bypass -File <notes checkout>\tools\baseline.ps1 -SkipTests -SkipBuild -GameExe '...' -Switches @{ VICTORY_BVH_BUILDER = '1'; VICTORY_BVH_QUARANTINE = '1' }
```

The script writes to `%LOCALAPPDATA%\victory-baseline\<time>\`, never into a repository. It has **not been run yet**:
it was written in a cloud session with no Windows and no PowerShell, so its first run on the PC is also its test.
Report any error it prints.

What the summary gives us: `fps, ms/frame (X preparing ...; Y on the GPU ...)` lines with draws and dropped draws per
frame, warnings and errors grouped by kind, ray-tracing builder lines, new `crashes.log` entries, test results, peak
process memory, and NVIDIA GPU load (from `nvidia-smi`, if present).

## Session log

### Our fork: `thomb2123/victory-fork` (created by the user 2026-11-25)

| Branch | On | Commits | State |
|---|---|---|---|
| `fix-clippy-placed-filler` | `c96b4f2` (fork main) | `48e3435` `placed_filler_twins` compiled only with the diagnostics feature, like its one caller | fixes fork main's red clippy gate; both gates ✅. **Candidate for a PR to `fxceless`** (needs the user's OK) |
| `builder-diagnostics` | `fix-clippy-placed-filler` | `15db4e0` builder stages without a host pipeline named in the log (diagnostics builds); `725851a` the per-frame log line no longer carries runs of 18 spaces | both clippy gates ✅; Windows/Wine 477 passed, the same 28 environmental failures; Linux 217/0. **Not run on a GPU** |
| `integration` | `builder-diagnostics` | merges of fork PRs #426, #437, #438, #440–#446 (see batch 2), **#452** (buffer `getDimensions` heap handles non-uniform: the AMD `amdxc64.dll` crash, suspected RDNA 4) **#156** (`slot_address` lowering: trace programs that read their structure's address from a heap view at a computed index; no wave-width gate) **#558** (barrier-free all-lane forms retried on native wave64 hosts; tested by its author on an RX 9070) and **#544** (shader cache keyed by the translator's sources and switches, so it survives rebuilds; `victory translate --cache <game dir>` warms it, about 7 min for 25,648 shaders on an RTX 5070 Ti) | merged without conflicts; both clippy gates ✅; Windows/Wine 497 passed, the same 28 environmental failures; Linux 226/0. **Not run on a GPU.** For A/B against `builder-diagnostics` |

### Session 2 — 2026-11-25 (cloud; no game, no GPU)

Environment: cloud container, 4 cores, 15 GB RAM, no GPU. Rust 1.98.1 (pinned) with the MSVC target, clang/lld 18 as
`clang-cl`/`lld-link`, xwin 0.10.0 (MSVC CRT + Windows SDK 10.0.26100; running xwin accepts Microsoft's licence for
those files), Ubuntu Wine 9.0. Setup: `tools/winenv.sh` and "Reproduce" below.

Results at fork `main` `31714de` (the first pass at `2119b0e` gave the same picture):

| Check | Result |
|---|---|
| Linux-native tests of the portable crates (core, formats, shader, pipeline, game, xtask) | ✅ 217 passed, 0 failed. The other crates are `cfg(windows)` |
| Windows cross-build, whole workspace, all targets | ✅ |
| CI's clippy gates (`-D warnings`, with and without `--all-features`) | ✅ both clean |
| Release `victory.exe` under Wine: `--help`, `inspect`, `config` | ✅ |
| Windows tests under Wine (`cargo test --workspace`) | ⚠️ 476 passed, 28 failed, 8 ignored. **None traced to the fork's code**; see below |

The 28 failures, by cause (all environmental; recheck on the PC):

| Tests | Cause |
|---|---|
| 23 in `victory-shader` (`valu_tests`, `intersect_tests`, `waterfall_tests`) | Need a D3D12 device with shader model 6.6. Wine's D3D12 here has none ("no SM 6.6 device available"). These include the ray/node intersection tests behind the shadow work: **must pass on the PC** |
| `xtask` `repository_passes_policy` | Spawns `git`; there is no Windows `git.exe` inside Wine. Passes in the Linux run |
| `victory-loader` `load_probe::loads_binds_and_calls_probe` | Spawns `cargo` (a Linux binary) from the Windows process |
| `victory-hle` `display::sdr_and_display_deferrals` | `0x80070578` ERROR_INVALID_WINDOW_HANDLE: no real window/display |
| `victory-hle` `net::only_loopback_is_reachable` | Wine's ws2_32 lacks `WSAConnectByList` |
| `victory-gpu` `host::sparse_copies_preserve_islands_and_observe_remaps` | Probably Wine: its `ReadProcessMemory` on its own process still reads a page made `PAGE_NOACCESS` (the test got the old bytes where Windows refuses the read). Hypothesis; settled by the PC run |

Changes:
- the builder-diagnostics patch (now branch `builder-diagnostics`) (+11 lines,
  `crates/victory-gpu/src/pipeline.rs`). With `VICTORY_BVH_BUILDER=1`, when the builder stays off, a diagnostics build
  now logs each builder stage with no host pipeline and why: the shader hashes that don't translate, or the pipeline
  the host refused. Before, it logged only the count. Clippy is clean with and without `--all-features`. Not yet run
  on a GPU. Not pushed to the fork: this session has read-only access to it.
- `tools/baseline.ps1`: the baseline and measurement script (not yet run, see above).

Findings:
- Ubuntu's Wine 9.0 crashes at startup (`free(): invalid pointer`) while `TMPDIR` is set; it calls `free()` on that
  variable's value (backtrace in `ntdll.so` `__wine_main`). `unset TMPDIR` fixes it (`tools/winenv.sh` does this).
- `victory translate` reads two variables for the same setting: `VICTORY_HOST_WAVE` (`apps/victory/src/main.rs`) and
  `VICTORY_HOST_MAX_WAVE` (`apps/victory/src/translate.rs`). Harmless, not changed.
- Blocked here by this session's permission rules: adding WineHQ's apt repository, and downloading PowerShell to
  syntax-check the script.

## Next steps

1. **User at the PC:** connect Remote Control to an empty folder (e.g. `C:\dev`); we run `tools/setup-pc.ps1 -Root C:\dev -GameDir <game folder>` (not yet run anywhere), then the paths, then the baseline script's `summary.txt` (default switches), and screenshots only
   if you choose.
2. Us: reproduce, or contradict, the author's L2–L5 claims on your machine and fill in the evidence table.
3. Us: pick the largest measured problem. For frame time that means "preparing" versus "on the GPU" and which part of
   it; for rendering, the black shadow areas or the missing room. Trace it through `victory-gpu` / `victory-shader`,
   fix it, and rerun the same baseline to measure the effect.
4. Every session: check the fork's `main` for new commits before working, and rebase our patches onto it.

## Reproduce the cloud environment

```sh
rustup toolchain install 1.98.1 --profile minimal -c rustfmt -c clippy -t x86_64-pc-windows-msvc
cargo install xwin --locked
xwin --accept-license --sdk-version 10.0.26100 splat --use-winsysroot-style --preserve-ms-arch-notation --output ~/.local/share/xwin-sdk
mkdir -p ~/.local/xbin; for t in lld-link llvm-lib llvm-rc llvm-mt; do ln -sf /usr/lib/llvm-18/bin/$t ~/.local/xbin/$t; done
ln -sf /usr/lib/llvm-18/bin/clang ~/.local/xbin/clang-cl
apt-get install -y wine64 xvfb xauth
git clone https://github.com/fxceless/victory-fork && cd victory-fork
. <notes checkout>/tools/winenv.sh
cargo build --workspace --all-targets --locked --target x86_64-pc-windows-msvc
CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=wine xvfb-run -a cargo test --workspace --locked --no-fail-fast --target x86_64-pc-windows-msvc
```
