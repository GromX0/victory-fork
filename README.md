<div align="center">

# victory-fork

**An Xbox Series X title on a Radeon RX 9070, aiming to look and play the way
it does on the console.**

A fork of [VIctory](https://github.com/Marvisak/VIctory), the compatibility
layer that runs an Xbox Series title natively on Windows from files you supply.

[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6)
![Tested on: Radeon RX 9070](https://img.shields.io/badge/tested%20on-Radeon%20RX%209070-ED1C24)
![Status: about 21 fps sampled, not 30 yet](https://img.shields.io/badge/status-~21%20fps%20sampled%2C%20not%2030%20yet-orange)

</div>

> [!IMPORTANT]
> **This repository contains no game files, code or assets.** You need your
> own copy of the game. It is not affiliated with or endorsed by the game's
> developer or publisher.

## The goal

The Series X version, on a PC, as it is on the console:

- **The same picture**: the Series X profile (4K output drawn at 1440p), the
  title's own ray-traced lighting, full-detail textures, no colours or dark
  areas the console does not have.
- **A sustained 30 fps** at those settings, natively. Frame generation,
  skipped rendering work or hidden defects do not count.
- **Responsive controls and reliable saves**, in 30-minute sessions and longer.

## Where it stands (26 November 2026)

My test PC: Ryzen 7 9800X3D, 32 GB RAM, Radeon RX 9070 16 GB (driver
32.0.32029.2010), Windows 11, page file 32-64 GB. Series X profile, no FSR,
no frame generation. The scene is the opening cutscene and the first
interior, 420-second runs with warm caches, all measured with the same
harness (`tools/fork-run.ps1`).

| Build | Median frame | p90 |
|---|---|---|
| **This build, ray tracing off, all compute work run** | **65.1 ms (15.4 fps)** | 272.9 ms |
| The pinned base before the shader fix (37 compute pipelines left out) | 46.1 ms (21.7 fps) | 70.5 ms |
| The pinned base, ray tracing on | 86.2 ms (11.6 fps) | 282.6 ms |
| Belobug's RX 9070 fork (comparison point, 35 left out) | 47.5 ms (21.1 fps) | 97.0 ms |
| My previous line (`int/harvest-test`) | 148.2 ms (6.7 fps) | 198.4 ms |

These are sampled 5-second averages from one scene. The earlier 21 fps
figures were partly bought by leaving out 37 compute pipelines the AMD
shader compiler crashed on. This build fixes that crash and runs them, which
costs about 45 ms a frame in the heaviest scenes. Still **not
console-equivalent**: it uses a replacement ambient light
(`VICTORY_AMBIENT=0.8`) and shows newly streamed textures later.

**First open-world session** (43 minutes, played with a controller from the
start screen through the prologue into the open world; 0 device losses, 0
crashes):

| Draws per frame | Median frame | p90 |
|---|---|---|
| 3-6k (prologue interiors) | 56.5 ms (17.7 fps) | 121 ms |
| 6-9k | 86.0 ms (11.6 fps) | 189 ms |
| 9k+ (open world) | 177.5 ms (5.6 fps) | 285 ms |

Against Series X footage, and in the open world:

- Foliage renders **purple/magenta** almost everywhere outdoors. Papers and
  posters indoors too, with ray tracing off and on.
- Some frames wash out white-blue and smear (temporal accumulation or
  exposure).
- Nights and interiors are far too dark where the console shows bounced
  light.
- Memory: the process reached 52 GB and the system's commit 82 GB.

## What I'm working on

Stage by stage, each with an evidence gate before the next:

| Stage | Status |
|---|---|
| 0. Pin the baseline | Done: commits, hashes, switches and machine in [docs/base-manifest.md](docs/base-manifest.md). Still to do: more routes and per-frame timing. |
| 1. Startup and shader-compiler stability | **Mostly done.** 6 of 6 cold and warm starts and a 43-minute session without a device loss. The AMD compiler crash on 37 compute shaders is fixed at its cause: one clean-up pass in the shader translator reused descriptor handles across blocks; offline 35 of 35 now compile, and a cold start has 0 crashes (32 before). Faces missing after a cold start are fixed by compiling pipelines when the game creates them (`VICTORY_EARLY_ALL=1`). **Open:** the title crashed at start-up (frame 0) in 2 of 5 boots of the fixed build; being counted against the previous build. |
| 2. Memory | Commit peaked at 82 GB in the open world (process 52 GB); 32 GB of RAM needs a large page file. Next stage. |
| 3. Picture correctness | Purple foliage and papers, smeared washed-out frames, missing bounced light: first inventory from the open world in [docs/experiments.md](docs/experiments.md). |
| 4. CPU frame time | Prologue: CPU-bound (submission preparation ~39-45 ms, copy-back ~20 ms). Open world at 9k+ draws: 177 ms a frame, most of it not yet accounted for. |
| 5. GPU and ray tracing | Ray tracing works on the RX 9070 but costs about 40 ms a frame. |
| 6. Frame pacing | After 4 and 5. |
| 7. Acceptance | 30-minute sessions on three routes at 30 fps, matching the Series X picture. |

The evidence for each step, including what didn't work:

- [docs/experiments.md](docs/experiments.md): every experiment and its result.
- [docs/base-comparison-2026-11-26.md](docs/base-comparison-2026-11-26.md):
  the builds compared, the captures and the Series X reference.

## Build and run

You need Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with shader
model 6.6 and mesh shaders, Rust (the toolchain is pinned), the MSVC build
tools, and your own dump of the title.

```powershell
git clone https://github.com/GromX0/victory-fork
cd victory-fork
copy victory.example.toml victory.toml      # then set your paths, outside this folder
cargo build --locked --profile diag -p victory --features diagnostics
scripts\play.ps1 -Exe <path of your copy's executable>
```

- `-RayTracing` turns the title's ray-traced lighting on (`VICTORY_BVH_BUILDER=1`).
- `-Safe` leaves out the newest, faster paths: try it first when something
  is wrong.
- With 32 GB of RAM, set a page file of 32-64 GB.

Before the first run, do these once (both can be stopped and resumed):

```powershell
target\diag\victory.exe shaders <the shader archives>   # translates every shader, minutes
target\diag\victory.exe warm --used                     # pipelines earlier runs drew with
```

To measure a run the way the table above was measured, use
`tools\fork-run.ps1 -GameExe <exe> -NoBuild -Diag -Seconds 420 -Keys start
-EnvFile <switches.env>`. It clears every other `VICTORY_*` variable and
records, for each run, the switches in effect, the commit peak and the frame
times.

## What's in this build

- **Renderer patches** (`VICTORY_PORT_*`): the title's constants heap,
  descriptor tables and constant buffers are read where the title writes
  them, instead of being copied every frame.
- **Submission on threads of its own**: list calls are recorded on the
  title's threads and translated by eight worker threads; submissions are
  prepared in parallel and joined.
- **Copy-back beside the write guards**: what shaders wrote returns to title
  memory by the dwords that changed, on its own thread.
- **Textures on their own copy queue**, shown only once uploaded.
- **RX 9070 support**: a guard that survives the AMD shader compiler's
  crashes and leaves those pipelines out, VRAM eviction, and a keep-shown
  guard.
- **Measuring and comparison tools**: the run harness with env files, the
  stability screen, and side-by-side captures against Series X footage.

Every `VICTORY_...` switch is described in a comment where it is read. The
upstream write-up of the renderer work and its RX 7900 XTX measurements is
in [docs/README-avatarmanz.md](docs/README-avatarmanz.md).

## Credits

This fork builds on other people's work, and the history keeps every
author:

- [VIctory](https://github.com/Marvisak/VIctory) by Marvisak and contributors:
  the loader, runtime, Direct3D 12 back end and shader recompiler.
- [avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX):
  the renderer patches and threaded submission this build is based on.
- [Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD):
  the RX 9070 compiler guard and VRAM changes.
- [fxceless/victory-fork](https://github.com/fxceless/victory-fork),
  [DexNom/VIctory](https://github.com/DexNom/VIctory) and the others named in
  [docs/README-avatarmanz.md](docs/README-avatarmanz.md).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE). The one file it does
not cover is named in [docs/asset-policy.md](docs/asset-policy.md).
