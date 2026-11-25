# victory-fork (thomb2123): an AMD-focused integration fork

A work-in-progress fork of [VIctory](https://github.com/Marvisak/VIctory), a
compatibility layer that runs an Xbox Series title natively on Windows from
files the user supplies. It builds on
[fxceless/victory-fork](https://github.com/fxceless/victory-fork) and gathers
fixes from upstream and from other forks, with one aim: the game's world
rendered correctly, stable and responsive on a **Windows PC with an AMD
Radeon RX 9070 (RDNA4)**.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's
developer or publisher.

The work is on the branch **`dev/integrate-clean`**.

## Status (2026-11-25)

**Not run on the target PC yet.** Everything below is either checked at
build level or reported by the forks the fixes came from.

| | |
|---|---|
| Builds (Windows target), both CI lint passes, test binaries link | checked (cross-built from Linux) |
| Tests that run without Windows (shader translator, formats, ...) | pass: 187 translator tests and the rest |
| GPU tests, the game itself | **not run yet**: they need the Windows machine |

What the fork's base reached on other machines (their reports, not ours):

| Fork | GPU | Reported |
|---|---|---|
| fxceless/victory-fork | RTX 4070 | prologue renders with HUD and input at 3-5 fps; shadowed areas black (ray-traced lighting does not run yet) |
| ezbyDev/victory-amd | RX 9060 XT (RDNA4) | 3D scenes at 1-3 fps, no device loss with its AMD settings |
| ka1qux/victory-ka1qux | RX 7800 XT | 25-minute play session, 12 runs without a crash, about 2-3.5 fps |

## What this fork adds on top of fxceless

**Taken from upstream VIctory** (about 30 fixes): sampler filtering the
device supports, mesh triangle indices without their edge-flag bit (such
triangles were dropped), helper lanes in waterfall loops (draws could spin
for seconds and hang the GPU), several shader instructions and float
fixes, the host device's removal reason, GPU-hang settings logged.

**Taken from other forks, each tested by its author on AMD unless noted:**

- **victory-amd** (ezbyDev): the pipeline library is saved one save at a
  time, by a thread of its own and on crash and exit (a corrupt or lost
  library cost minutes of rebuilding); `VICTORY_SKIP_PIPELINES` works in
  release builds.
- **ka1qux and the RDNA3 forks:** textures uploaded before the characters'
  bake (black skin and clothes), DirectStorage page lists filled late
  (models and textures left unwritten), an AMD shader-compiler crash
  guard, bounded texture re-uploads.
- **xs-proton** (slaktarn, RX 9070 XT): descriptor slots the title rewrites
  are mirrored before each submission (device loss 5-90 s into the world).
- **triippywhale line:** the shader clean-up passes no longer produce code
  that crashes AMD's compiler.
- **VIctory-V2** (alexdeliu): 64-bit integer compares that AMD's driver
  got wrong on an RX 9070.
- **TaynanGT and upstream pull requests** (NVIDIA): a staging-buffer race
  under the relay, faster card-side compares, fewer barriers between
  indirect draws.
- Smaller fixes from Scarlight, VIctory-V2 and others.

Changes that skip or fake rendering work to gain speed were left out. The
full list, with the source commit of every change and the reasons for
what was not taken, is in [docs/fork-handoff.md](docs/fork-handoff.md).
Every change keeps its author.

## Running it

Requirements as upstream (below): Windows 10/11, a CPU with AVX2, a
Direct3D 12 GPU with shader model 6.6 and mesh shaders, Rust.

```powershell
git clone https://github.com/thomb2123/victory-fork
cd victory-fork
git switch dev/integrate-clean
copy victory.example.toml victory.toml        # set your paths, outside the repository
```

- Play: `powershell -ExecutionPolicy Bypass -File scripts\play.ps1 -Game "<game folder>\<the game's exe>"`
- A recorded run (build, machine details, logs and frame times kept
  outside the repository): `pwsh tools\fork-run.ps1 -GameExe "<...>" -Seconds 600`
- On AMD, try `-Profile amd` as well: the stability settings victory-amd
  reports for RDNA4. Whether they are needed on an RX 9070 is not known
  yet.

The first launch of each build is slow while shaders are translated and
cached.

## Tools for following other forks

`tools/fork-watch.sh` lists, for every fork in `tools/fork-watch.txt`, the
commits that neither upstream, fxceless nor this branch has.

## Credits

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors: everything this is built on. GPL-3.0-or-later, as is this
  fork.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**
  and the forks named above, whose authors keep their names on their
  commits here.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was used by
  fxceless as a reference; fixes inspired by it were written anew.
- This fork's integration work was done with
  [Claude Code](https://claude.com/claude-code).

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
