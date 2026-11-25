<div align="center">

# victory-fork

**An AMD-first integration of [VIctory](https://github.com/Marvisak/VIctory):
an Xbox Series title running natively on Windows, from files you supply.**

[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6)
![Tested on: Radeon RX 9070](https://img.shields.io/badge/tested%20on-Radeon%20RX%209070-ED1C24)
![Status: not playable yet](https://img.shields.io/badge/status-not%20playable%20yet-orange)

</div>

> [!IMPORTANT]
> **This repository contains no game files, code or assets.** You need your
> own copy of the game. Not affiliated with or endorsed by the game's
> developer or publisher.

> [!NOTE]
> **Is it NVIDIA-only? No.** Most community testing so far was on NVIDIA,
> but this fork runs on AMD: on a Radeon RX 9070 it loads into the world
> and plays its opening cutscenes at **about 6 fps**. Slow and rough, on
> every vendor, for now.

> [!WARNING]
> **Use Series X mode (the default) on AMD for now.** With
> `--console series-s` this build lost the GPU after about three minutes on
> the RX 9070. Series X ran without a device loss.

Work happens on **`dev/integrate-clean`**; `main` follows it.

## Results on an RX 9070 (2026-11-25)

Measured on the target PC: **Radeon RX 9070 16 GB** (driver 32.0.32029.2010),
**Ryzen 7 9800X3D**, 32 GB RAM, Windows 11. Release builds of
`dev/integrate-clean`, default settings, recorded runs (`tools/fork-run.ps1`).

| Milestone | Result |
|---|---|
| Builds, tests pass | ✅ 624 tests pass, **including the GPU tests on the RX 9070**. The first run on RDNA4 found a real descriptor allocator bug, now fixed. One fxceless test depends on test order (also on fxceless's own tree). |
| Start screen | ✅ About 1.5 min after launch; the keyboard works (Enter = A) |
| Loads into the world | ✅ Loading, title card, opening credits, then indoor and rooftop cutscenes with lit, textured characters. **72 s** to the world (was 178 s) |
| Stable session | ✅ Series X: 7 to 10 minutes without a crash in the last runs · ⚠️ Series S: GPU lost after about 3 minutes |
| Playable | ❌ Not yet |

**Speed in the first cutscenes** (median of the last 3 minutes of a run,
the build's own frame line):

| Build | fps | ms/frame | of which on the GPU |
|---|---|---|---|
| Before the latest merge (fxceless 310c6eb + picks) | 4.9 | 202 | 75 ms |
| **Now** (fxceless f85101b + KLBF Lab's AMD fixes) | **6.3** (peaks 6.5) | **158** | 76 ms |

Menus and loading screens run at the game's own 30 fps. A scene whose
shaders are still being built drops to 0.5-2 fps until they are made.

The GPU is not the bottleneck yet: about half of each frame is copying
memory to and from the card and preparing work for it.

### What changed in the latest merge

- **fxceless's newest work:** descriptor records the game copies as raw bytes
  are mirrored (the first rooms render right), a much cheaper descriptor sync
  (70-220 ms a frame down to a few), and a shader cache keyed on the
  translator, so a rebuild no longer throws all translations away.
  **Every shader that failed to translate on AMD before now does.**
- **KLBF Lab's AMD fixes** (tested on an RX 9070 XT): 32-lane console shaders
  run per half of AMD's 64-lane waves (fixes purple blocks), reductions per
  half, a NaN guard on colour outputs, texture ranges kept through loading.

### Known issues on AMD

- **Series S mode loses the GPU** (see the warning above). Being looked at.
- **The picture is still off:** glowing eyes, an over-exposed image and some
  magenta patches.
- **Memory.** The game commits about **27 GB**. Close heavy background apps
  before a long run, or give Windows a larger page file, or it stops with
  "memory allocation failed".
- **The driver refuses one mesh pipeline**; its draws are missing.
- **Pipelines are built again on each launch**, so loads stutter every time.

## Why this fork

Dozens of forks now carry real fixes, each tested on one machine, many
conflicting with one another, some trading rendering work for speed. This
fork starts from [fxceless/victory-fork](https://github.com/fxceless/victory-fork)
(the furthest-along renderer) and does the integration work for AMD:

- **Every fork with work of its own was reviewed commit by commit** (17 so
  far). Only fixes for a concrete failure were taken: device loss, driver
  compiler crashes, missing geometry or textures, wrong shader results,
  real speed costs. Changes that skip or fake rendering work were left
  out; each decision and its reason is in
  [docs/fork-handoff.md](docs/fork-handoff.md).
- **Fixes are made to work together, not just stacked.** Combining them
  turned up problems none of the sources had on its own.
- **AMD settings are checked against this code, not copied** from reports
  made on other branches.

<details>
<summary><b>Problems found while combining fixes</b></summary>

- The mesh shader index mask still included the triangle's edge-flag bit,
  and this branch also culls triangles whose indices are out of range:
  together, any triangle exported with an edge flag set was dropped.
  Masked to the 9-bit index.
- A 64-bit compare fix for AMD (dword-wise compares) would have read
  negative constants as large positive ones next to upstream's
  sign-extension rule. Corrected and covered by a test.
- The AMD skip list for compute programs that crash the driver's compiler
  only worked in diagnostics builds, and even there a skipped program was
  still compiled ahead of time. Now honoured in every build.
- Helper lanes in pixel-shader waterfall loops could spin to the loop
  limit (seconds per draw, then a GPU hang). The upstream fix did not
  apply to this branch's lowering; ported by hand, with a test.
- A dynamic-descriptor fix taken from upstream was cut short by a scan
  limit on a full-size heap, so a free run around the search position was
  still missed and tables were not made. Found by the first GPU test run on
  the RX 9070; fixed, with a test that fails without it.
- fxceless's latest commits and two of the AMD picks failed the CI lint
  gate; fixed, and a diagnostic one of them dropped was restored.

</details>

<details>
<summary><b>AMD settings: what was checked</b></summary>

The relay thread that one RDNA4 report blames for device loss is *on by
default* in this line (it was off where that report was made). The "AMD
shader option" that report recommends turns off three optimisation passes,
not the one it names. Its texture-queue switch names do not exist here. The
AMD run profile (`-Profile amd`) uses only switches whose meaning was
verified in this branch; A/B runs with and without it on the RX 9070 are
next.

</details>

## What is in it

About 60 fixes from upstream and other forks, plus this fork's own work.
By area:

| Area | Fixes |
|---|---|
| **Stability on AMD** | stale descriptor slots mirrored before each submission (device loss 5-90 s into the world on an RX 9070 XT); shader code that crashed AMD's compiler (optimiser handle reuse, buffer size queries); 64-bit compares AMD's driver got wrong; a staging-buffer race under the relay; R9G9B9E5 targets that lost the device |
| **What is drawn** | mesh triangles with edge flags; textures uploaded before the characters' bake (black skin and clothes); DirectStorage page lists filled late (models and textures left unwritten); a dropped deferred material; samplers the device supports; several shader instruction and float fixes |
| **Load times and stutter** | the pipeline library is saved safely and often (a corrupt or lost library meant minutes of rebuilding); bounded texture re-uploads |
| **Frame time** | faster card-side compares, fewer barriers between indirect draws, a late frame flips at once |

Source commits and authors for each are in
[docs/fork-handoff.md](docs/fork-handoff.md); every change keeps its author
in the history.

## Running it

**You need:** Windows 10/11, a CPU with AVX2, a Direct3D 12 GPU with shader
model 6.6 and mesh shaders, Rust, the Visual Studio Build Tools (C++ and a
Windows SDK), and **about 27 GB of free memory commit** for the game.

```powershell
git clone https://github.com/GromX0/victory-fork
cd victory-fork
git switch dev/integrate-clean
copy victory.example.toml victory.toml        # your paths, outside the repository
cargo build --release -p victory
```

| To | Run |
|---|---|
| **Play** | `powershell -ExecutionPolicy Bypass -File scripts\play.ps1 -Game "<game folder>\<the game's exe>"` |
| **Record a run** | `powershell -ExecutionPolicy Bypass -File tools\fork-run.ps1 -GameExe "<...>" -Seconds 600` |
| **Other console mode** | the same with `-Console series-s` (renders less; on AMD it currently loses the GPU, see above) |

The `-Profile amd` settings (relay off) ran at the same speed as the default on
the RX 9070 and drew a worse picture, so the default is the one to use.

`fork-run.ps1` works in the Windows PowerShell that comes with Windows. It
records the machine and the GPU the game actually used, keeps a copy of the
build next to its logs and frame times, and clears leftover test switches so
the run is normal play. `-AmdSkips` and `-Diag` are there for crash hunts.

At the start screen press **Enter** (the pad's A) to continue; the full
keyboard layout is in the upstream section below.

## Reported by the forks this builds on

Their machines, their numbers:

| Fork | GPU | Reported |
|---|---|---|
| fxceless/victory-fork | RTX 4070 | prologue renders with HUD and input; about 4.5-5 fps with defaults, 6-7 with its new frame-rate switches; shadowed areas black (ray-traced lighting does not run yet) |
| PastiiJay/klbf-lab-beta (AMD branch) | RX 9070 XT (RDNA4) | first cell with walls and characters; 4.8-5.2 fps in Series X mode, about 7.3 fps in Series S mode with AMD FSR, 74 minutes without a device loss (on its own build) |
| ezbyDev/victory-amd | RX 9060 XT (RDNA4) | 3D scenes at 1-3 fps, no device loss with its AMD settings |
| ka1qux/victory-ka1qux | RX 7800 XT | 25-minute session, 12 runs without a crash, 2-3.5 fps |

## Following other forks

`tools/fork-watch.sh` fetches every fork in `tools/fork-watch.txt` and lists
only the commits that neither upstream, fxceless nor this branch has, marking
what is new since the last check.

## Next

1. Series S mode on AMD: find why the GPU is lost (leads: fills that no
   longer wait for work in flight, and the game's FSR 4 passes).
2. The picture: glowing eyes, over-exposure, magenta patches.
3. Keep pipelines between launches, so a second start does not rebuild them.
4. More of KLBF Lab's AMD work (walls, characters and faces, pipeline
   pre-build), and the refused mesh pipeline.
5. Then the measured bottleneck (memory copies, not the GPU), and light in
   shadowed areas.

## Credits

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors, the base of everything here. GPL-3.0-or-later, as is this
  fork.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**, and
  the authors of every fix taken from ezbyDev/victory-amd,
  ka1qux/victory-ka1qux, Slaktarn700/xs-proton, triippywhale/victory-fork,
  DesertGamer/VIctory-V2, TaynanGT/victory-windows, pablogonz12/Scarlight,
  Zettrox/ztrx, PastiiJay/klbf-lab-beta (KLBF Lab's AMD fixes) and upstream
  pull requests; their names stay on their commits.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only), which fxceless
  used as a reference; fixes inspired by it were written anew.

Everything below this line is upstream VIctory's own README, unchanged; its
status section describes upstream, not this fork.

---

# VIctory

Runs the title (Xbox Series X|S) natively on Windows, from files
the user supplies.

The title's x86-64 code runs unchanged inside our process; VIctory supplies
the platform underneath it and converts the title's data, ahead of time where
it can and on first use where it must (shaders). See
[docs/architecture.md](docs/architecture.md).

## Status (2026-11-23)

**Not playable yet.** The title boots, runs its menus at its own 30 fps and
loads the world, where rendering fails.

| Stage | State |
|-------|-------|
| Boot, first-boot screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video with sound | works |
| Start screen, profile and saves | works |
| Loading into the world (about 50 s) | works |
| The world (first mission's intro) | reached and runs with indirect draws; its title card renders, the scene after it is still dark; 0.5-1.8 s per frame |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `VICTORY_D3D12_WARP=1`, which renders on the software adapter.

What each part does today and what it still needs:

| Subsystem | Today | Needs |
|-----------|-------|-------|
| Loader | Runs the title's executable and its DLLs in our process; crash reports name the faulting code and are kept in `crashes.log` | – |
| Runtime services | One signed-in offline user, licences, packages, saves that survive a kill at any step, text entry dialog | Speech is silent; the dialog and relaunch are untested live |
| Network | Offline by design: the title cannot reach the network | – |
| Files and streaming | Archives and streamed data read, zlib and BCPack decoded in parallel | Loading is limited by writes into title memory |
| Input | XInput pads, keyboard, keyboard stand-in pad | Trigger rumble |
| Audio | Menu, video and loading sound; XMA voices and Opus streams decode | Reverb is silent; Opus results unverified against the title |
| GPU: menus | Draws, compute and presentation on host D3D12; only changed title memory is copied | Frame pacing drifts on one screen in release builds |
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes, indirect execution (arguments clamped), pipelines made by worker threads and kept on disk | Most of the scene is still missing (untranslated shaders drop draws); frames take seconds; real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`).

The title also needs several GiB of available Windows commit at startup.
Check **Committed** under **Task Manager → Performance → Memory**. If
`crashes.log` reports Windows error 1455, close other applications or increase
the Windows paging file size, then retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo test --workspace                   # needs no game data
cargo run -p victory -- config           # check your paths
```

Run the title:

```powershell
cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\victory\data`), derived
data under the cache folder.

Input: XInput pads. With none connected the keyboard is a pad while the
title's window has focus:

| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

## Commands

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
```

Logging: `VICTORY_LOG=victory_loader=trace,info` (a release build logs
info and above).

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `VICTORY_PIPELINE_WAIT_MS=24` | reduce the per-frame wait for background pipeline creation from the default of 500 ms. A draw can be omitted until its pipeline is ready |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `VICTORY_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
