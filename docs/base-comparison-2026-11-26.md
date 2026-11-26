# Base comparison, 2026-11-26 (RX 9070 16 GB, Ryzen 7 9800X3D, 32 GB)

Driver 32.0.32029.2010, Windows 11, page file C: 32-64 GB (commit ceiling
~95 GB; Windows restarted 26 Nov 17:16 before these runs). Game
`D:\GTA6\game_scarlett_master.exe`, Series X profile (4K output drawn at
1440p), no FSR, no frame generation.

## Method

- Each build in its own checkout, own shader/pipeline cache and its own
  copy of the same save data (`D:\victory-data\{data,belobug,avatar}`).
- Same harness (`tools/fork-run.ps1` at de15fee7): every inherited
  `VICTORY_*` cleared, switches only from the env file, effective
  switches recorded in `run.json` (`effective_env`).
- Scene: new game from the start screen, the opening cutscene and the
  cell (`-Keys start`: Enter every 5 s from 20 s to 150 s), 420 s.
- Warm caches: one unmeasured warm-up run per build before the measured
  one. Nothing else running (no builds, no agents' game runs).
- Frame figures are the layer's 5-s averaged frame lines, not per-frame
  samples: "p90" is of those averages, not a per-frame percentile.
  Shots follow the game's clock, so builds of different speed show
  different shots at a given frame number; compare by draw-count band.

## Builds and switch files

| Label | Tree | Commit | Build | Exe SHA-256 (16) | Env file |
| --- | --- | --- | --- | --- | --- |
| ours | GromX0 int/harvest-test | de15fee7 | release | 92A16BE9D550F441 | `envs/ours-base.env` (SKIP_PIPELINES=39f0e9aeb156a61c only) |
| belobug | Belobug/VIctory-Belobug-AMD rx9070xt (= AvatarManz 0a43566 + 1 commit) | 81ebb7bd | diag | B1CAD0AE80776326 | `envs/belobug-play.env` (its play.ps1 defaults; SKIP as ours, its own: none) |
| combined | AvatarManz 2de99801 + Belobug 81ebb7bd cherry-picked, magenta filter left out (branch base/avatar-rdna4) | b7fb278a | diag | 11A7DFC4F2E84DCF | `envs/base-play.env` (AvatarManz play.ps1 defaults + SKIP as ours) |
| avatar | avatarmanz/VIctory-7900XTX main | 2de99801 | diag | - | `envs/avatar-play.env` |

Env files are in `D:\victory-runs\envs\`; kept exes in
`D:\victory-runs\builds\<commit>[-diag]\`. The play.ps1 defaults are
TEXTURE_NO_WAIT, RELAY=2, WIDE_BACK, CARD_REHASH_RUNS, JOIN_COPIES,
INDIRECT_WINDOW, RELAY_COPY, CARD_STEPS=2, INDIRECT_BATCH=2,
MAP_THREADS, TITLE_PRIORITIES=0, CARD_AWAKE, SHOW_AFTER,
**AMBIENT=0.8**, D3D12_DRED=light, THIN, JOIN_PARTS=8, UPLOAD_AHEAD.

## Results (measured runs)

| | ours (ours-measure) | belobug (bb-measure2) | combined (base-measure) |
| --- | --- | --- | --- |
| Ran / stop | 438 s, time limit | 441 s, time limit | 440 s, time limit |
| Device loss | none | none | none (but one in its warm-up, below) |
| Median of world lines (3,000+ draws) | 148.2 ms (6.7 fps) | 47.5 ms (21.1 fps) | 46.1 ms (21.7 fps) |
| p90 of those lines | 198.4 ms | 97.0 ms | 70.5 ms |
| 3,000-6,000 draws, median | 145.2 ms (28 lines) | 47.8 ms (44) | 46.3 ms (30) |
| 6,000+ draws, median | 158.1 ms (24) | 62.0 ms (11) | 48.5 ms (18) |
| Parts (median): prep / GPU wait / copy back | 79.3 / 48.2 / 41.5 | 40.0 / 0.3 / 20.5 | 38.7 / 0.8 / 22.8 |
| Commit peak | 51.0 GB | 60.6 GB | 62.2 GB |
| Draws dropped a frame (world median) | 456 (skip list) | 368 + 71 indirect | 381 + 67 indirect |
| Pipelines left out | 1 (39f0e9aeb156a61c) | 1 + 35 in `warm-bad.txt` (AMD compiler crashed on them) | 1 + 37 |
| Picture-changing defaults | rays forced to miss | rays miss (builder off) + **AMBIENT=0.8 replacement ambient light**, KEEP_SHOWN/SHOW_AFTER (new textures appear later) | same as belobug |

The ~21 fps figure is a sampled 5-s-average result in this scene, not
sustained equivalent-quality performance: the faster builds add a
replacement ambient light, leave 35-37 more pipelines out, and show new
textures later.

Other runs:
- avatar (2de99801 as published): crashed in `amdxc64.dll+0x861b9f`
  (AMD shader compiler, access violation) at 71 s and 40 s, both
  warm-ups. Unusable on this card without a compiler crash guard.
- belobug warm-ups: first stopped by our guard (commit grew 38.2 GB >
  38, limit since raised to 70); second ran 438 s, 21 compiler crashes
  caught.
- combined warm-up (base-warmup, b7fb278a): **device removed** at
  18:19:41, ~3.5 min in, "an internal issue", empty breadcrumbs, 34
  compiler crashes caught; the process ran on to the time limit.
  Unexplained: under investigation before the combined build is
  promoted.
- ours, builder on (m-rt2, bf937768): not a freeze. Ran out of commit at
  the old 62.2 GB ceiling about 102 s in: uncapped builder page faults
  each became a 4 MiB copy window (1,023 copies within ~3 frames).

## Visual comparison (so far)

Contact sheets of the presented frames:
- ours: `D:\victory-scratch\captures\sheet-ours.png` (diag capture `rainbow`, 3ad44676)
- belobug: `D:\victory-scratch\captures\sheet-belobug.png` (diag capture `bb-look`)

Same shot, Lucia at the cell window: ours shows a bright **magenta panel
on the wall** right of the window (the known AMD purple defect); belobug
shows ordinary posters there. Belobug's interiors are brighter (its
ambient light). Faces, hair and walls comparable. Belobug progressed
further in the same wall time (corridor, mess hall, "Go with the prison
guard"); ours stayed at the window. No Xbox reference frame is available
for any of these shots. **Not yet done:** frame-for-frame pairs of the
same shot for all three builds, and the combined build's capture.

## Unresolved

1. Combined build's device loss in its warm-up (cause unknown).
2. 35-37 pipelines the AMD compiler crashes on are left out in the
   AvatarManz/Belobug line: which draws they are and what they render.
3. AMBIENT=0.8 is a replacement for traced indirect light; picture
   equivalence with the console is not established for any build.
4. Ray tracing: ours dies of commit with the builder on; the combined
   build's builder-on run (base-rt) is in progress.
5. Purple/magenta: ours traced to the foliage pixel shaders (first wrong
   target: 2560x1440 RGBA16F written by their indirect draws). The
   belobug commit's magenta filter (left out of combined) hides it.
6. Glowing eyes (ours, frame 1721 of `rainbow`).

## Update: ray tracing on, matched-shot captures, decision

Combined build with the game's own BVH builder on (`envs/base-rt.env` =
base-play + VICTORY_BVH_BUILDER=1; run base-rt): 438 s, time limit, no
device loss, no compiler crashes (warm), commit peak 67.8 GB. Median of
world lines 86.2 ms (11.6 fps), p90 282.6 ms (frames with up to 186 ms
on the card). Draws dropped a frame: 1 (the ~380 of the builder-off runs
are mostly the builder's own passes, skipped on purpose).

Captures (diagnostics, presented frames, every 8th world frame):
`base-look` (builder off) and `base-rt-look` (builder on), sheets
`sheet-base.png`, `sheet-base-rt.png`; pair `pair-window-ours-belobug.png`.

- Wall papers beside the cell window: purple in ours AND in the combined
  build with the builder off (magenta filter left out); white in
  belobug. So belobug's clean wall is its magenta filter at work, not a
  fix. Its own comment: "Magenta ... is what the probes' memory held,
  not light".
- Hypothesis (not yet proven): the purple is the lighting reading probe
  memory that the title's ray tracing fills; with the builder off it
  holds whatever was there. With the builder on, no purple was seen in
  this run's thumbnails, but the shots differ (it progressed into the
  corridor, mess hall and cell block); the papers are not in view.
- Builder on, frame 1463: sun shafts and yard lighting plausible, but
  the cell's walls are missing (the window frame stands in the open
  yard), the defect KLBF lists ("sky where the wall should be").

## Base decision (2026-11-26)

New working base: **base/avatar-rdna4** (AvatarManz 2de99801 + Belobug
81ebb7bd's RX 9070 compiler guard and VRAM changes, magenta filter left
out). Reasons, all measured here: 2.5-3x ours by draw band with the
builder off; the only build where the title's ray tracing runs on this
card (11.6 fps median, faster than ours with rays forced to miss); the
picture matches belobug's except the purple it masks. Conditions before
calling it stable: the device loss in its warm-up (DRIVER_INTERNAL_ERROR
3.5 min in, after 34 compiler crashes caught in-process) must be
explained or not recur in repeated runs.

Kept as they are: belobug 81ebb7bd (`D:\dev\wt-belobug`, exe
`builds\81ebb7bdc9-diag`) as the comparison point; GromX0
int/harvest-test (ours, de15fee7 + notes) as the fallback. Our own
fixes are ported to the new base one at a time, each measured.

## Reference material

- `D:\victory-scratch\reference\powerpyx-prologue-1-ps5.jpg` (not in the
  repository): a 1280x720 screenshot from PowerPyx's prologue guide
  (https://www.powerpyx.com/gta-6-prologue-walkthrough/), Lucia's cell at
  the start of the prologue. Its prompts are PlayStation's ("Hold L2"),
  so it is the PS5 version, not Series X: qualitative evidence of colour
  and lighting character only, not a pixel oracle. Shows the photos and
  papers on the cell walls in ordinary colours (white, yellow), a warm,
  dim, low-sun interior.
- Comparison `D:\victory-scratch\captures\pair-cell-reference.png`:
  reference | combined b7fb278a ray tracing on (base-rt-look 1559) |
  belobug 81ebb7bd (bb-look 2040). Camera angles differ (the reference
  faces the window wall, the builds the door wall), so only gross
  differences count: the builds' cell is lit with sun patches and reads
  cooler and higher in contrast than the reference's warm interior
  (unconfirmed: the walls differ). The purple papers of ours and of the
  builder-off combined build are confirmed wrong by the reference.
- No Series X reference of the same build (930.048.020) is available yet.
