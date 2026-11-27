# Experiment log

One line of evidence per experiment: build, switches, scene, hypothesis,
result, decision. Scene unless stated: opening cutscene and cell, Series X,
420 s, warm cache, `tools/fork-run.ps1 -Keys start`. Run folders are under
`D:\victory-runs`.

| Date | Build | Change / switches | Hypothesis | Result | Decision |
| --- | --- | --- | --- | --- | --- |
| 11-25 | ours 021227d3 | baseline (m-base) | - | 133.4 ms median, 436 s, no device loss | baseline |
| 11-25 | ours 021227d3 | `VICTORY_HEAP_SYNC_CARD=1` (m-heapsync) | heap-point settles wait for every flight's landing; waiting for the card alone saves 9-12 ms | 128.4 ms (-5 ms); settles 8.5→3.8/frame but 5-6 card-only waits of 18-20 ms appeared | keep, off by default |
| 11-25 | ours 021227d3 | `VICTORY_STALE_OVERLAP=1` (m-stale) | STALE settles need only the declaring flights: 4-5 ms | 133.3 ms, no gain: narrow waits cost 13.3-13.6 ms each (declaring flights at the back) | rejected |
| 11-25 | ours | staging buffers let go after upload (53c47a3a), then after 250 ms/2 frames, shared zero buffer (168560a4) | 14 GB write-combined memory at the peak is texture staging | private peak unchanged (24.6-25.9 GB); our live staging was ≤1.8 GB; list uploads 0 | hypothesis rejected; changes kept (harmless) |
| 11-25 | test | 400 × 30 MB upload buffers made and released (ignored test in host.rs) | the driver hoards released upload memory | peak 256 MB, reused | rejected |
| 11-25 | ours | null slot for buffers picked at a texture's slot (01f194bf) | DRED fault 0x7000_02ae_b000 = texture descriptor read as buffer | no device loss in 4 runs of 7 min (losses at 45-110 s before) | keep |
| 11-25 | ours | keep display on (d42c67cc) | boot int3 at +0x4dcbc9d = audio command queue full with no output device | all boots work again | keep |
| 11-25 | ours bf937768 | builder on + guard from header + DispatchX thread counts (cbae5e7a) | 83-s crash = builder stores outside ranges / over-dispatch | no crash; died of commit at 62.2 GB ~102 s (uncapped fault windows) | partial; next: cap builder faults |
| 11-26 | avatar 2de99801 | its play.ps1 defaults | stronger base | AMD compiler crash at 40-71 s, twice | not usable as published |
| 11-26 | belobug 81ebb7bd | its play.ps1 defaults | stronger base | 47.5 ms median (3.0x ours by band), 441 s, 35 pipelines left out, ambient light added | comparison point, preserved |
| 11-26 | combined b7fb278a | AvatarManz play.ps1 defaults | newer renderer ports + RX 9070 guard beat belobug | 46.1 ms median; 6k+ draws 48.5 vs 62.0 ms; device loss in its warm-up | candidate; device loss under investigation |
| 11-26 | combined b7fb278a | + VICTORY_BVH_BUILDER=1 (base-rt) | AvatarManz's RT path runs on an RX 9070 | 438 s, no loss, 86.2 ms median, p90 282.6 ms | RT works; cost to be cut |
| 11-26 | combined b7fb278a | captures builder off / on | belobug's clean wall is a fix or a mask | purple returns without the magenta filter; builder on: real light, cell walls missing in one shot | mask; probe-memory hypothesis open |
| 11-26 | all builds vs Series X | YouTube Series X walkthrough frames (480p) against the captures | purple = probe memory the title's RT fills | purple papers also with the builder on (base-rt-look 1503); every build lacks the warm bounced light in shadowed areas | probe hypothesis falsified; new one (Stage 3): indirect diffuse light wrong (purple = garbage, black = nothing) |

## Stage 1: startup, pipeline compilation and GPU execution

```text
Stage / blocker: 1, device loss in the combined build's cold warm-up (base-warmup: DRIVER_INTERNAL_ERROR at 18:19:41, ~3.5 min, empty breadcrumbs)
Baseline commit + executable hash: b7fb278a, 11A7DFC4F2E84DCF
Scene, route, settings, cache state: opening and cell, base-play.env, COLD pipeline cache (D:\victory-data\combined-cold: translated shaders, no pipelines folder)
Observed evidence: the only combined run with a device loss had 34 in-process compiler crashes; four runs with 0-3 had none
Hypothesis and falsification test: parked crashed compiler workers leave the driver unsafe -> a cold run reproducing ~30+ in-process crashes should lose the device; none = not supported
One change: none (cache state only)
Before / after measurements: base-coldrepro: 32 compiler crashes caught, 0 device losses, ran to the time limit
Rendering and gameplay checks: n/a
Memory / stability effects: none recorded
Keep, revert or investigate: hypothesis not supported by one trial (a loss is not reproduced by crashes alone); the loss's cause is open
Next single action: inventory the base-warmup failure (compiler crash hashes, the DRED/state at the loss, what ran in the seconds before) and repeat cold starts towards the 3 cold + 3 warm gate
```

Inventory of the base-warmup loss (log only): the first sign was at
18:19:40.50, "host pipeline refused ... device suspended" for compute
a0ce4bef0ae97c3f (one of the 37). At the loss, nothing of ours ran on the
card; the submitting thread had been preparing a submission for 1.3 ms.
The pipeline library listed 3 live makes (0.0-0.9 s old) and 37 makes "running"
21-405 s: the crashed compiler workers the guard parks, which never return.
The last crash was 18 s before (efdb8133aabc37fe). The device went while the
compiler was working with ~37 crashed threads parked in it. That fits the
guard leaving driver state unsafe, but cold runs with 32 parked crashes
(base-coldrepro, stab-cold1, stab-cold2) did not lose the device, so it is
at most a rare trigger. Removing the crashes at their source would settle it.

```text
Stage / blocker: 1, the AMD compiler (amdxc64.dll+0x861b9f) crashes on 37 compute pipelines of the combined build; our line's translation of the same shaders compiles 35 of them
Baseline commit + executable hash: b7fb278a, 11A7DFC4F2E84DCF; ours de15fee7 (release)
Scene, route, settings, cache state: offline, no GPU. `victory translate <35 title containers> --list --validate`, containers saved by our 07cf595f run (2 of the 37 were never saved: 0ac6717164281c2d, e932a28baa58153e). Host settings as a run's log gives them: VICTORY_HOST_WAVE=64, VICTORY_HOST_RDNA=1, VICTORY_VIEW_LIMIT=2097151
Observed evidence: both builds translate and validate all 35, but none of the 35 DXIL outputs are identical. f018c205b5199d39 (disassembled): combined has FAbs 2866 / FMin 1994 / FMax 330, ours 267 / 181 / 328. The extra ops are the combined build's AMD-only float forms (VICTORY_RDNA_FLOAT, all bits on by default on RDNA): its legacy multiply is `min(|a|,|b|) == 0 ? 0 : a*b` (rdna::LEGACY). With VICTORY_RDNA_FLOAT=0, those counts equal ours (267 / 181 / 328). The DXIL is still not identical: for f018, RawBufferLoad 1087 vs 1155, cbufferLoadLegacy 414 vs 426 and UMin 362 vs 442 (the combined build loads and clamps less). All 35 still differ in their op mix with the forms off
Hypothesis and falsification test: the RDNA float forms make DXIL the AMD compiler crashes on -> `translate --pipelines` of these containers crashes with the default bits and compiles all of them with VICTORY_RDNA_FLOAT=0. Crashes with 0 as well = rejected
One change: none yet (a runtime switch, no code)
Before / after measurements: pending: --pipelines needs the GPU, run after the stability screen
Rendering and gameplay checks: n/a yet
Memory / stability effects: n/a yet
Keep, revert or investigate: investigate
Next single action: offline --pipelines on the 35 containers, default bits vs VICTORY_RDNA_FLOAT=0
```

Files: `D:\victory-scratch\dxil-diff\` (containers in `crashers-cdxil`, DXIL in `combined`,
`combined-rdna0`, `ours`, disassembly `off-*-f018.ll`).

```text
Stage / blocker: 1, gate part 1: 3 cold + 3 warm starts without the known failure (the device loss)
Baseline commit + executable hash: b7fb278a code, exe 11A7DFC4F2E84DCF in all six runs (later commits are docs only)
Scene, route, settings, cache state: opening and cell, 420 s, base-play.env + VICTORY_D3D12_DRED=1 (base-stab.env); cold = D:\victory-data\combined-cold with its pipelines folder removed (translated shaders kept), warm = the cache the cold run just filled
Observed evidence (stage1-screen.ps1, D:\victory-runs\stage1-screen.txt):
  cold1 438 s, 32 compiler crashes caught, 0 device losses, commit peak 68.8 GB
  warm1 437 s,  1, 0, 63.7 GB
  cold2 439 s, 32, 0, 68.7 GB
  warm2 438 s,  1, 0, 62.0 GB
  cold3 437 s, 32, 0, 69.3 GB
  warm3 438 s,  3, 0, 65.6 GB
Hypothesis and falsification test: the base-warmup loss recurs in ordinary cold or warm starts -> a loss in any of six
One change: none
Before / after measurements: 0 losses in 6 (with base-coldrepro: 0 in 7 since the one loss)
Rendering and gameplay checks: NEW DEFECT, seen by the user in cold3 (19:29, gameplay in the cell block): faces, necks and arms of every character invisible (wall shows through); hair, eyes, mouths, clothes drawn. Not in warm3 at the same scene (faces seen 19:36-19:39). No graphics pipeline refused, no shader failed to translate; 0 pipelines being made at the time. Cold3 dropped 599 draws/frame "being made" during the load (19:27:31), warm runs at most 42
Memory / stability effects: commit peak 62-69 GB (cold runs ~69)
Keep, revert or investigate: gate part 1 passed for the device loss (still unexplained, rare); part 2, a 30-minute session, not run yet. The cold-start face defect blocks calling cold starts good
Next single action: cold start with VICTORY_PIPELINE_WAIT_MS=20000 (draws wait for their pipeline instead of being dropped after 500 ms a frame): faces present = a one-time dispatch or draw lost while its pipeline compiled
```

```text
Stage / blocker: 1 (and 3), faces, necks and arms invisible after a cold start; fine after a warm one
Baseline commit + executable hash: b7fb278a, 11A7DFC4F2E84DCF
Scene, route, settings, cache state: as stab-cold3, + VICTORY_PIPELINE_WAIT_MS=20000 (cold-wait.ps1, run coldwait)
Hypothesis and falsification test: a draw or one-time dispatch is dropped while its pipeline compiles and its result never comes -> with a 20 s budget per frame faces render; missing = not supported
One change: VICTORY_PIPELINE_WAIT_MS 500 -> 20000
Before / after measurements: 436 s, 32 compiler crashes, 0 losses, commit peak 67.1 GB. The budget took effect ("wait budget per frame millis=20000") but did not cover the load: 1269.5 draws/frame dropped "being made" at 19:42:46, then 0.1-5/frame
Rendering and gameplay checks: faces still missing (19:46, intake cutscene: eyes and teeth float, no skin), including on a character loaded after the compiles had finished (0 being made)
Keep, revert or investigate: not supported, though not a clean falsification (drops at the load remained). A character loaded later, also faceless, argues against a one-time loss at the load. Remaining difference between cold and warm: in cold runs the compiler crashes 32 times inside the process (threads parked); in warm runs those pipelines are skipped without being tried
Next single action: cold start keeping warm-bad.txt (all else in the pipelines folder removed): cold compiles without compiler crashes. Faces present = the in-process crashes cause it
```

| Date | Build | Change / switches | Hypothesis | Result | Decision |
| --- | --- | --- | --- | --- | --- |
| 11-26 | b7fb278a | cold, warm-bad.txt (37) kept (coldkeepbad) | the in-process compiler crashes cause the missing faces | 438 s, **0** compiler crashes, 0 losses; faces still missing (19:55, floating teeth) | rejected |
| 11-26 | 9dfbe7c6 | bake draws wait for their pipeline (VICTORY_BAKE_WAIT, on), cold + bad list (coldbake) | the skin bake (PS_BakeDecorations_*) is dropped while its pipeline is made | the 4 bake pipelines were ready in 1-3 ms (made=true): bake draws were not being dropped; faces still missing (20:02). The bake's texture pass looks as in warm3 | rejected for the bake itself; change kept (harmless) |
| 11-26 | 9dfbe7c6 | cold + bad list + VICTORY_EARLY_ALL=1 + VICTORY_PIPELINE_WAIT_MS=20000 (coldearly, 300 s) | one-time load work is dropped while its pipeline compiles: start every pipeline at creation | **faces, necks and arms render** in the cell block (20:07, same shot as cold3's faceless one). Still 427 draws/frame dropped "being made" at the load. 319 s, 30 new compiler crashes (bad list 37 -> 67: pipelines the title created, compiled here though maybe never used), 0 losses, commit peak 72 GB; 2.4-3.8 fps while compiling | supported: cold-start faces need pipelines started at creation. Cost: 30 more pipelines on the crash list, a slow first load. Next: EARLY_ALL alone |
| 11-26 | b7fb278a, offline | `translate --pipelines` of f018c205b5199d39, run settings (wave 64, RDNA host, view limit 2097151), VICTORY_RDNA_FLOAT default vs 0 | the AMD float forms crash the compiler | both crash in amdxc64.dll+0x861b9f (as in runs); our de15fee7 translation of the same container compiles (pipeline created, 3 s) | float-forms hypothesis rejected |
| 11-26 | b7fb278a, offline | same, VICTORY_SHADER_OPT bisection (default 63 = passes 1, 2, 4, 8, 16, 32) | one clean-up pass makes DXIL the compiler crashes on | 0: compiles; 59 (pass 4 "one instruction for one value" off): **compiles**; 62, 61, 55, 47, 31 (any other one off): crash. Pass 4 reuses a descriptor handle across loads where ours makes a clamped handle per load (80 fewer UMin + CreateHandleFromHeap in f018) | pass 4 triggers the crash for f018; all 35 under test |
| 11-26 | b7fb278a, offline (compiler-repro.ps1) | all 35 saved containers, VICTORY_SHADER_OPT default vs 59 | REUSE causes the crashes on all of them | default: 0 compile, **35 crash**; 59: **35 compile**, 0 crash | supported for all 35 |
| 11-26 | 1465c1f2, offline | REUSE gives a handle-making call (createHandleFromHeap, annotateHandle) the earlier one of its own block only; TRANSLATOR_VERSION 2; all passes on | handles reused across blocks are what crashes the compiler | **35 compile, 0 crash**; REUSE's 11 unit tests pass | fix; in-game check next (fresh cache: warm-bad keys are the title's shaders, not our translation, so the 37 stay skipped in old caches) |
| 11-26 | 1465c1f2 | `cargo test -p victory-shader --lib` | the change breaks nothing | 301 passed, 0 failed, 4 ignored. `size_tests::the_record_count_of_a_structured_buffer_read_by_index` crashes the test process in amdxc64.dll+0x861b9f **with VICTORY_SHADER_OPT=59 and =0 too** | not caused by the change: a second trigger of the same compiler fault, in that test's shader. Title shaders not seen yet may hit it |
| 11-26 | 1465c1f2 | fresh cache D:\victory-data\handlefix (no translations, no pipelines, no bad list), base-stab.env (hf-cold) | with the fix a cold start has no compiler crashes | 438 s, **0 compiler crashes** (32 in every earlier cold run), 0 device losses, commit peak 63.1 GB | Stage 1 compiler stability: fixed for the shaders seen in this route |
| 11-26 | 1465c1f2 | same cache warm, base-play.env (hf-measure) | frame time with the 37 pipelines run | the title crashed 34 s in: unhandled 0xc0000005 at victory-image+0x2215c35 (write 0x0), at boot. First at this address; earlier builds crashed at boot at other addresses (+0x27c195d 5x, +0x4dcbc9d 4x, ...) | not attributed; repeat (hf-measure2) |
| 11-26 | 1465c1f2 | repeat, same cache warm, base-play.env (hf-measure2) | as above | 442 s, 0 compiler crashes, 0 losses, commit peak 64.8 GB, no boot crash. Left out now: only the builder's stages (off) and 39f0e9aeb156a61c; **the 37 compute pipelines run**. Median of world lines 65.1 ms (15.4 fps), p90 272.9 ms (base-measure: 46.1 / 70.5). By draw band: 3-6k 42.7 ms (base 46.0), **6k+ 92.8 ms (base 47.3)** | the baseline's speed was partly the 37 pipelines left out: +45 ms in the heaviest scenes, spikes. Honest figure for Stage 4/5; their cost to be profiled there. The boot crash of hf-measure did not recur (1 in 2) |
| 11-26 | 1465c1f2 (25ad7ce1 exe copy, same binary) | session1: user-played, base-play.env + VICTORY_EARLY_ALL=1, handlefix cache | Stage 1 gate part 2 | title crashed at boot again, **same address** victory-image+0x2215c35 (write 0x0). The layer's crash report: "Game State: System Init, Frame: 0"; the exception follows the D3D12 device's creation by ~0.1 s (log order), in a title thread (stack: +0x2215c35 <- +0x2215eca <- KERNEL32 thread start); rbx, rbp, r12-r15 = 0. None of the 37 newly run pipelines had bound (they first bind ~46 s in) | open Stage 1 item: a start-up race, 2 boots in 4 of this build, none in dozens of boots of earlier builds. Not yet attributed. Next: short boots of this and the previous build, counted |
| 11-26 | 1465c1f2 (25ad7ce1 copy) | session2: user-played with a controller from the start screen into the open world, base-play.env + VICTORY_EARLY_ALL=1, handlefix cache, 45 min limit | Stage 1 gate part 2: 30 minutes without the known failure | **2625 s (43.75 min, stopped by the user), 0 device losses, 0 title crashes**, 4 compiler crashes (all at 20:45:34-45, start-up: EARLY_ALL compiles every created pipeline; new shaders, caught), commit peak 82.3 GB (process 52 GB private at the end; 0.6-3.5 GB left to commit at times while the page file grew to 45 GB+). By draw band: 3-6k 56.5 ms (17.7 fps, p90 121), 6-9k 86.0 ms (11.6 fps, p90 189), 9k+ 177.5 ms (5.6 fps, p90 285; prep 58.4 + copy back 20.2: most of the frame is elsewhere). 52 screenshots: `D:\victory-scratch\session2-shots`, sheet `captures\session2-sheet.png` | gate part 2 passed for the device loss. Not passed as a whole: the boot crash (2 of 5 boots of this build) is a start-up failure. The user found it unplayable in the open world (below) |

```text
Stage / blocker: 1, the title crashes at start-up (victory-image+0x2215c35, frame 0)
Baseline commit + executable hash: 1465c1f2 (fixed translator) vs 9dfbe7c6 (before it)
Scene, route, settings, cache state: boot-count.ps1: 6 boots each, alternating, 45 s, base-play.env, handlefix cache
Observed evidence: 1465c1f2 1 crash in 6 (3 in 11 with the earlier runs), 9dfbe7c6 0 in 6 (0 in 9). Same time from launch
  to device creation (0.16-0.33 s) and same device set-up (~0.32 s) in crashed and clean boots; one clean boot of
  9dfbe7c6 took 6.8 s to set up the device. Minidump of the crash (cdb, WinDbg 'winget install Microsoft.WinDbg'):
  - the faulting instruction is `mov dword ptr [0],0`: a deliberate crash by the title's WATCHDOG thread, which
    sleeps 1 s a round, checks heartbeat timestamps against a limit, and counts down 30 rounds without progress;
  - nothing was logged for ~30 s before it (exception printed right after "crash tracing on"; process exit 32 s in;
    breakpad report at +31 s): the start-up STALLED right after the D3D12 device creation began;
  - thread 43 at the crash: our host device creation (D3D12Core!D3D12CoreGetLayeredDeviceSize) inside AMD's shader
    compiler initialisation (amdxc64!GetSettingsBlobsAll -> OpenShimInterface -> boost serialisation of its settings);
    every other thread of ours was idle.
  AMD's own compiled-shader cache %LOCALAPPDATA%\AMD\DxcCache: 9,265 MB in 124 files (grown by today's compiles).
Hypothesis and falsification test: not tested yet. Candidates: AMD's cache size or state makes the compiler's
  initialisation take >30 s at times; the fixed build only correlates through cache state (3/11 vs 0/9, not significant)
One change: none
Keep, revert or investigate: diagnosed as a driver-side stall during device creation that the title's watchdog turns
  into a crash; not our translator's output. Mitigation that holds whatever the cause: create the host D3D12 device
  before the title starts, so a driver stall delays the start instead of tripping the title's watchdog
Next single action: that mitigation, then count boots again
```

| Date | Build | Change / switches | Hypothesis | Result | Decision |
| --- | --- | --- | --- | --- | --- |
| 11-26 | 1465c1f2 + host device made before the title starts (VICTORY_HOST_EARLY, on; exe F35295901371ED9A) | boot-count.ps1, 10 boots, same cache and switches | a driver stall in device creation no longer reaches the title's watchdog | **10 of 10 boots clean** (before: 1 in 6 this way, 3 in 11 in all); device made in 290-391 ms before the title's first code; boots reach the logo frames as before. No boot stalled this time, so the change has not met an actual stall yet (0 in 10 by chance at the old rate: ~6%) | keep (on by default); watch for long "device made before the title starts" times |

## Stage 2: memory (first accounting, from existing counters)

The layer's `copystat` lines already give: process private commit and working set, the PC's free
memory and commit left; host copies of title memory (alive, MB; of them windows outside buffers);
snapshot buffers; texture staging; host textures alive; cumulative upload, shared and card buffers
made; host copies never let go (render targets and written resources). session2 (43 min, open
world), every ~5 min:

| Time | Private | Host copies | Host textures | Never let go | Shared buffers made (cum.) | Private - host copies |
| --- | --- | --- | --- | --- | --- | --- |
| 20:50 | 41.0 GB | 4.7 GB | 6.5 GB | 3.1 GB | 18,018 / 8.6 GB | 36.3 GB |
| 21:01 | 47.6 | 5.1 | 6.3 | 3.4 | 56,286 / 28.8 GB | 42.4 |
| 21:12 | 48.1 | 5.4 | 7.3 | 3.6 | 96,814 / 39.0 GB | 42.7 |
| 21:28 | 55.1 | 6.1 | 6.5 | 3.7 | 126,759 / 57.6 GB | 49.0 |

- Private commit grows ~0.4 GB a minute for the whole session (not bounded); the accounted
  categories stay flat (host copies 4.7-7.2 GB, never-let-go 3.1-3.7 GB). **~37-43 GB of private
  commit is in no category the layer reports** (residual, not forced into a total). Shared
  buffers alive are not reported (only made).
- Same route, same point, with and without VICTORY_EARLY_ALL=1 (coldkeepbad vs coldearlyonly):
  private 31.8 vs 37.4 GB at 120 s, 32.1 (flat) vs 40.1 GB (growing) at 280 s. **Compiling every
  pipeline at creation costs ~8 GB by 5 min and keeps growing** as the title creates pipelines:
  the likely driver of session2's climb (EARLY_ALL was on). The cold-start face fix and memory
  are in tension: to be resolved here (EARLY_ALL for the first, cold run only, or bounded).
- Shared buffers are the host copies of title memory; their live figure is in the line (host
  copies alive, 5-7 GB), so they retire. Title memory is committed per physical page once and
  kept while the title owns it (xmem `commit`/`unmap`), as on the console: bounded by its asks
  (~63 GB asked, mostly reservations, all at boot).
- **Graphics memory over budget, spilling into system memory** (`memory of textures` line):
  the card 14.6-15.4 GB in use against a budget of 13.5-13.7 GB from 20:50 on, and **10.4-12.9 GB
  of the PC's memory shared with the card in use** (1.7 GB at the start). Textures are let go and
  made again all the time: 685,185 let go (71 GB), 294,797 made again (32.6 GB), 28,309 passes
  that made none for lack of room. GPU residency churn, which the Stage 2 gate names; it also
  costs frame time. The spill counts against the PC's commit.
- **Host command lists**: 5,737 made by the host (14 ever reset), and "private bytes grew 12,183 MB
  while lists were made" (10,591 MB at 21:07): lists and their allocators pile up.
- Kept shader containers 189 MB and translations 790 MB: small.
- Next: the VRAM spill and the lists first (largest, demonstrated); then EARLY_ALL first-run only.

Correction and attribution (same session, more lines read):

- The pipelines: EARLY_ALL started 84,252 and made 72,223 by 21:04. By whole runs it adds ~4.5 GB
  on the opening route (40.6 vs 36.0 GB private at the end, coldearlyonly vs coldkeepbad); the
  per-pipeline figure from session2's first half mixed in the open world's streaming.
- The host lists are flat (2,170 -> 2,230 alive); "private bytes grew 12 GB while lists were
  made" counts what other threads allocated meanwhile.
- **Our own heap** (the counting allocator) grew 2.5 -> 9.2 GB over the session, and **the view
  table "distinct views kept" 9,619 entries 119 MB -> 12,256,904 entries 5,721 MB, 0 free**: the
  shapes of placed views filled their 4,194,304 numbers at ~21:10, after which every view of
  placed memory became a table entry kept for the run (12.7 million by 21:29).
- Shape census (e7d6152f, opening route, hf-measure-census, 897,671 shapes after 7 min, ~150,000
  a minute): of the 29 words of a shape, **the buffer view's FirstElement (view word 3) has 49,668
  distinct values in 69,052 sampled**; then view word 9 (4,084), resource width (3,962), resource
  words 1 and 13 (3,259, 2,792). The title makes buffer views at ever other element offsets.
  Folding the offset into the address is not neutral: a placed buffer view's host copy is found
  by its base address and width (`buffer_extent`), so each offset would become a buffer of its own.
- Fix (6c88940d): table entries of views of placed memory unused for 900 frames are let go under
  a new generation (a record still held names nothing, never the next view there), 8 looked at
  per new entry. A/B on the opening route with the shapes capped at 100,000 to make them overflow.

| Date | Build | Change / switches | Hypothesis | Result | Decision |
| --- | --- | --- | --- | --- | --- |
| 11-26 | 6c88940d | recyc-off (VICTORY_VIEW_RECYCLE_FRAMES=0) vs recyc-on (900), both VICTORY_VIEW_SHAPES_MAX=100000, opening route, 420 s | letting go of unused placed entries bounds the table without changing what draws read | table 4.73 M entries (0 free) vs **1.81 M** (484 k free, 4.35 M let go); our heap 4,318 vs **2,795 MB**; private 36.5 vs 34.6 GB; 0 losses, 0 crashes both. But records looked up after their entry was let go: 40.0 M vs **46.4 M** (+16%): the title comes back to views unused for 900 frames, and those resolve to null views | memory part works; **off by default** (174ff6f4) until the picture is shown unchanged (captures, longer age, or a scheme that keeps entries a record may still name) |
| 11-26 | 12a52593 | VICTORY_EARLY_ALL unset = automatic: all pipelines started at creation only on a cache without an early list. autocold (combined-cold, pipelines folder removed but the bad list kept, base-stab.env) then autowarm (same cache, base-play.env), 300 s each | first runs keep their faces, later runs drop the memory cost | autocold: `all=true first_run=true` (early list 0), 74,312 made of 84,249 started, faces present (22:41); private 36.1 GB at 202 s. autowarm: `all=false first_run=false` (early list 1,978), 8 made, 2,000 started early; private **31.1 GB** at 204 s, 33.1 at 283 s (cold: 36.3). 0 losses, 0 crashes both | keep: first run 3-5 GB more, every later run lean |

**Route note (26 Nov, 22:37 on):** the save folder `D:\victory-data\avatar\data` now holds the user's
open-world autosave from session2, so `-Keys start` (Enter at the start screen) continues it: runs
from autocold on start in the open world (at home), not at the prologue. Runs before and after are
not comparable. Saves are now frozen in `D:\victory-data\snapshots\{prologue,openworld}` and
`ab-run.ps1 -Save <name>` restores one into `D:\victory-data\scripted\data` before each run (the
user's own save folder untouched). victory.toml's cache_dir is `D:\victory-data\handlefix\cache`
(translator 2, no stale bad list) since the handle fix.

| Date | Build | Change / switches | Hypothesis | Result | Decision |
| --- | --- | --- | --- | --- | --- |
| 11-26 | 5a308dc1 | ow-idle1: save 'openworld', 900 s, idle at home | memory is bounded when nothing new happens | private 31.5 GB (2 min) -> 34.1 GB (14 min), slowing; host copies flat 4.4 GB; **our heap 1.0 -> 3.2 GB; 2,019,719 shapes of placed views** (still ~19,000 a minute); 0 losses, 0 crashes | not bounded: the view shapes |
| 11-26 | 2832b5ed | view descriptions cleaned where the title creates them: SRV word 3 (padding before the 8-byte-aligned union), union words past the dimension's members, resource words 1 and 13 (padding); the census's most varied words were exactly these (D3D12 layouts) | stack bytes in padding make alike views distinct | ow-idle2, same save and length: **shapes 55,048 (35x fewer; word 3 and word 9 now 1 value each)**, our heap 2,632 MB (3,182), private 33.2 GB (34.1) at the end; 0 losses, 0 crashes. Frame lines by draw band: 3-6k 51.1 ms (50.8), 9k+ 122.9 (102.9) but 81 lines against 57 in 9k+ scenes (the run went into an outdoor cutscene): not comparable. Tests: 385 passed (3 new on the layouts) | keep. Picture A/B with VICTORY_VIEW_CLEAN=0 (2fbb45b6) and timed screenshots running |
| 11-26 | 2832b5ed | papers-check: save 'prologue', cell window | the clean-up also clears the purple (foliage looked green in ow-idle2) | **papers by the cell window still purple** (23:34) | the purple has at least one other cause; the foliage observation is unconfirmed (different places and times) until the A/B |
| 11-26 | 2fbb45b6 | (first try of the A/B, clean-off) | - | device made before the title in **38,657 ms**: a driver stall longer than the title's 30 s watchdog; the title started late and ran (the run was stopped for a capture bug) | the start-up fix met a real stall and held |
| 11-27 | 2fbb45b6 | clean-on vs clean-off (VICTORY_VIEW_CLEAN=0), save 'openworld', 720 s each, a screenshot every 15 s (`ab-shots.ps1`; D:\victory-scratch\shots) | the clean-up changes the picture (for better or worse) | both 740 s, no losses or crashes; foliage green in both (trees, palms, flowers); no defect in one that the other lacks. The game's flow diverged (traffic, route), so scenes, not frames, compare | no picture change seen: the clean-up is a memory fix. The green foliage of ow-idle2 is not its doing |
| 11-27 | 2fbb45b6 | overflow-off: clean-off + VICTORY_VIEW_SHAPES_MAX=50000 (shapes full before 108 s; 3.5 M placed table entries by 233 s) | session2's purple foliage began when its shapes filled (21:10): the entry path draws placed views wrong | after the overflow the foliage stayed green (flame tree, palms, bushes; 122-275 s). Purple on window glass (0122) as on the prologue's papers, overflow or not | rejected. (Read in passing: the copy pass defers placed views it knows by their record, CONSTANT/PLACED, to the draws, but makes table-entry ones at once; not shown to matter) |
| 11-27 | 2fbb45b6 | earlyall-on: VICTORY_EARLY_ALL=1 (as in session2), save 'openworld', 600 s, shots every 15 s | session2's purple foliage comes with starting every pipeline at creation | **purple foliage back**: the bush beside the flame tree magenta (244-305 s), the trees through the car's windows purple-tinted (336-549 s); same save and scene green without it. Card **over budget from 240 s** (12.7 of 11.7 GB; 12.9 of 11.5 at 481 s) and texture passes that made none for lack of room **306 -> 3,536 -> 7,624 -> 11,770** (clean-on: 344 -> 586, flat, with eviction churn of 220 k let go / 92 k made again; session2: 28,309). Textures held back for lack of room wait until host copies are let go (`TextureQueue::hold`/`room`), and with the card full of what is not let go they never come | **supported: purple foliage = textures never made for lack of room on the card.** Compiling every pipeline fills it (shader code lives on the card); since 12a52593 that happens only on a cache's first run. Next (Stage 2): free room for needed textures under pressure (least recently used host copies) instead of waiting |

### rec1: user-played open world, screen recorded (27 Nov, cfc63afa build, normal switches)

`D:\victory-scratch\recordings\rec1-play.mkv` (347 s, both monitors; the game is the left
2560x1440), frames every 4 s in `rec1-frames`, full-size defect frames in `recordings\defects`.
The layer's log starts with the recording (00:27:03). 0 device losses; private 33.0 GB at the end.

| Video time | Defect | Card at that time | Textures refused for lack of room | Stage |
| --- | --- | --- | --- | --- |
| 110 s | purple/magenta stripes on a glass door (as the prologue's papers) | 10.9 of 11.5 GB | 0-6 | 3 (rendering) |
| 168 s | whole screen washed out white and smeared sideways, ghosted copies ("melting") | 11.3-11.8 of 11.4-12.0 GB | 517-1,305 | 3 (temporal blending; to be shown) |
| 292-296 s | distant trees neon green; two gantry signs blank grey | **12.1 of 11.6 GB (over since ~244 s)** | **2,307** | 2 (textures never made) |

The card's budget fell from 15.4 GB at the start to ~11.6 GB in the world. Pop-in (the user's
report) fits textures waiting for room.

The card at the end of rec1 (the system's count: budget 11,417 MB, in use 12,163 MB; the PC's memory
shared with the card 10,557 MB of 14,056): host copies of textures the title fills 2,883 MB (18,037),
of textures the host renders to or writes 2,926 MB (765), of buffers in the card's memory 925 MB (57);
**~5.4 GB not attributed** by any line (pipelines, pools, tiles, twins, the driver's own). The
sweep (`resource::let_go_idle`) lets a texture's host copy go only when no list and **no descriptor
slot** holds it; slots of the title's heaps hold thousands. The texture pass makes no new host
texture past 90% of the budget (`resource::room`), and the slot waits: blank signs, pop-in. Windows
itself moves allocations to the PC's memory past the budget, which keeps the picture (at some cost).
Test: `VICTORY_TEXTURE_BUDGET_MB` replaces the 90% gate by a limit on the textures' own bytes.

| Date | Build | Change / switches | Hypothesis | Result | Decision |
| --- | --- | --- | --- | --- | --- |
| 11-27 | cfc63afa | texbudget6g: VICTORY_TEXTURE_BUDGET_MB=6000, save 'openworld', 600 s, shots every 15 s; against clean-on (same route and build, 90% gate) | the refusals (blank signs, late textures) are the 90% gate on a moving budget | **refusals 0 for the run** (clean-on: 344 -> 586); host textures ~3.1 GB alive (2.5); card 12.2 of 11.6 GB, the PC's memory shared with the card 11.3 GB (10.3); private 33.7 GB (33.2); 0 losses, 0 crashes; no blank textures seen in the shots; the smear frame at 336 s as before. Frame lines: 6-9k 55.8 ms (48.9), 9k+ 67.1 (66.2), scene mix different | refusals are skipped work and go; default made from a share of the card (21244e58), A/B in one build next |
| 11-27 | 21244e58 | share40 (the new default, VICTORY_TEXTURE_SHARE=40), save 'openworld', 600 s; baseline clean-on (the 90% gate alone; the second run with SHARE=0 cancelled as the same code path) | a share of the card removes the refusals at an acceptable cost | **refusals 0** (586); host textures 2.8-3.6 GB alive (2.3-2.7); the PC's memory shared with the card 10.0-11.4 GB (10.3); 0 losses, 0 crashes. Frame lines: 6-9k **59.2 ms** (48.9), 9k+ 63.4 (66.2); with texbudget6g's 55.8, the medium scenes cost ~7-10 ms more. Upload queue: mean due 598 (865), mean oldest 108 frames (65) | **keep**: no texture the title asked for is left out; the medium-scene cost is a Stage 4 target (where it goes: the system moving card memory, or more uploads) |

Pop-in has a second cause, the upload queue (rec1's `textures picked by index, the queue` line):
slots due for upload 200-1,000 typically, 3,805-7,068 at peaks, the oldest written 8 frames
(~0.5 s) ago typically, 72-459 frames (5-30 s) at peaks; a further 4,000-23,000 wait for the title
to map their memory (its own streaming, as on the console). The layer notices a texture the title
filled and uploads it within a per-frame budget; on the console the card reads it at once. Next,
after the room fix: measure that lag and cut it (what is drawn first, budget per frame). Next: free room for needed textures under pressure
(Stage 2), then the purple glass/papers and the smear at their first wrong pass (Stage 3).

### rec2: user-played open world, screen recorded (27 Nov, b8a88c42, play-popin.env)

Pop-in reported by the user is mostly not textures arriving late: a nearby tree vanishes or ghosts
when the player walks a few steps away (236-266 s), which points at the title's LOD switch / dither
fade together with the temporal pass (Stage 3), not at memory. The upload-budget settings of
play-popin.env (512 MB, 24 ms) are not adopted as defaults (cost unmeasured; Stage 6). Also seen:
a dark/red frame at 186 s, the purple glass/papers, the smear.

### Stage 2 gate check (27 Nov)

```text
Stage / blocker:          Stage 2 gate: bounded working set, retirement, paging, picture undamaged
Baseline commit + exe:    b8a88c42 (base/avatar-rdna4 = main), victory.exe SHA-256 8229C308...BE90F6
Scene, route, settings:   save 'openworld' (snapshot, at home), scripted, 300 s per run, base-play.env,
                          warm cache D:\victory-data\handlefix (early list present: EARLY_ALL auto off).
                          s2-on = defaults; s2-off = VICTORY_VIEW_CLEAN=0, VICTORY_TEXTURE_SHARE=0.
                          Both: VICTORY_CAPTURE_WORLD (+2,+6,+30,+80,+150,+300,+600,+1200 frames
                          after the first 3D frame), typeperf paging counters every second
                          (D:\victory-runs\stage2-close.ps1, stage2-compare.ps1). Route approximate:
                          the harness only presses Start. CORRECTION (user, 27 Nov): the user was
                          moving the character around during these and the s3-* test runs, so they
                          are not idle runs: unscripted, different movement per run. That explains
                          the camera/pose differences at equal frames and the "heavy phase" starting
                          at different times (s2-on never reached it). Frames +2/+6 precede input.
Observed evidence:        private commit 28.9 -> 31.7 -> 31.9 -> 32.1 GB at minutes 1-4 (on), 29.7 ->
                          32.5 (off); clean-on (12 min, same save) flat at 33.0-33.2 GB from minute 3;
                          rec2 (played) 30.5 -> 32.4 GB over minutes 2-4. Host copies 4.4 GB and host
                          textures 5.7 GB flat after minute 2. Texture refusals: on 0, off 3,698.
                          Paging (on, after 120 s): pages read from disk median 163/s, mean 1,073/s
                          (4.2 MB/s), p95 5,538/s, max 38,079/s; 37 of ~200 s over 1,000/s; free RAM
                          1.1-1.5 GB; working set 4.1-21.7 GB (Windows trims it). Off: median 147/s,
                          mean 763/s, free RAM 0.5-0.9 GB.
Hypothesis / falsification: the Stage 2 changes bound memory without damaging the picture; paging
                          does not drive frame time. Falsified by: private memory still climbing after
                          the load, a defect in s2-on absent in s2-off, or frame time following paging.
One change:               none (measurement of the current defaults against the changes switched off)
Before / after:           paging against 5-s frame-time lines (on, steady part): the 3 windows over
                          1,000 pages/s 64.4 ms/frame, the other 24 59.8 ms; lowest-paging windows
                          56-65 ms. Frame similarity (SSIM on/off) 0.70-0.89: driven by the scene's
                          divergence, not by a rendering difference (pairs inspected).
Rendering checks:         +6 frames: same house and garden; with the changes OFF a palm's lower fronds
                          are magenta (a texture never made: the 3,698 refusals); ON it is green. +300:
                          the purple window blinds are in BOTH runs (Stage 3 defect, not memory). No
                          defect in s2-on that s2-off lacks.
Memory / stability:       0 device losses, 0 crashes in both runs.
Decision:                 Stage 2 gate PASSED on this route, with limits: user-moved, unscripted play
                          (not a repeated identical traversal), 5-12 minute runs; equal-frame
                          comparisons hold for +2/+6 only; the card still overflows ~10 GB into the PC's
                          memory and the PC runs at ~1 GB free, so paging bursts cost ~5 ms where they
                          occur (Stage 6 hitch work, not a Stage 2 blocker on this evidence).
Next single action:       Stage 3, step 1: the inventory of skipped pipelines, dropped draws
                          ("dropped ~360 without a host pipeline and ~75 indirect" per frame in rec2),
                          placeholders and filters, then the purple blinds/papers traced to their
                          first wrong pass with VICTORY_CAPTURE_FRAMES + VICTORY_DUMP_TARGETS.
```

## Stage 3: inventory of omitted and substituted work (27 Nov, step 1)

Source sweep of crates/victory-gpu and victory-shader (every drop, placeholder, stand-in, clamp and
switch, with its default), counted on s2-on (b8a88c42, base-play.env, save 'openworld', 300 s, steady
part). Only what is active under the play switches is listed; the rest (switches off, 0 in the run)
is in the sweep, not here. "Per frame" = the meter's per-frame averages.

| # | Omission / substitute | Switch (play setting) | Frequency (s2-on) | Affected pass / consequence (known) | Next repair |
| --- | --- | --- | --- | --- | --- |
| 1 | Ray-tracing structure builder: every dispatch dropped | VICTORY_BVH_BUILDER off | **329 dispatches/frame** | the title's rays find nothing: ray-traced light, reflections and shadows absent (why item 2 exists) | the DispatchX thread-count flag (audit finding 1) in an isolated branch, small dispatch tests, then the builder on |
| 2 | Indirect-light stand-in after CS_ScreenProbes_TemporalResolve: light raised to an estimate, the title's own bounded to 4x it | VICTORY_AMBIENT=0.8 | every frame (Radiance 1280x720, Resolve 2560x1440) | replacement lighting, not the title's: dark interiors / blown-out windows / colour of indirect light unverified | goes when 1 works; until then a labelled compatibility mode |
| 3 | Compute pipelines never compiled: the bad list `warm-bad.txt` (42 keys) was copied from the cache made **before** the compiler fix (1465c1f2); 35 of those shaders compile offline since | VICTORY_WARM_BAD on | **~74 dispatches/frame, 21 shaders** ("refused") | unknown compute passes skipped every frame | one run with VICTORY_WARM_BAD=0: do they compile, crash, change the picture |
| 4 | One indirect signature dropped (two arguments: changes root arguments) | none (shortcut) | **~73 executions/frame** | unknown | identify the signature's pass from a frame's ops list; implement root-argument signatures |
| 5 | Shader 39f0e9aeb156a61c skipped by name | VICTORY_SKIP_PIPELINES in base-play.env (from the AMD skip list) | ~1 draw/frame | reason not recorded | re-test without the skip |
| 6 | Grey 1x1 "neutral" texture shown until a new texture's upload | VICTORY_SHOW_AFTER=1 | 114,762 slots over the run, 0.64 frames on average, at most 3 | brief grey surfaces when textures change (a pop-in form) | Stage 6 (upload order), keep the device-loss fix |
| 7 | A rewritten slot keeps showing its old texture until the new one is made | VICTORY_KEEP_SHOWN 1 | 79,786 slots | old texture for a frame or more | Stage 6 |
| 8 | Queries not run: occlusion says visible, statistics 0 | none | every query | nothing culled by queries (cost, not picture) | Stage 4/5 if measured |
| 9 | Translation approximations: custom border colour = transparent black; some non-pixel samples at level 0 without anisotropy; cube arrays as cubes; quad mask true | none | no counter | possible blur at grazing angles, edge colours | only where a traced defect points |
| 10 | HDR refused, SDR presentation | none | always | tone/brightness vs the console's HDR output | compare against an SDR console reference |
| 11 | Silent no-ops without a counter: resolve/clear/copy when a side has no host texture; indirect DispatchMeshX | none | **unmeasured** | unknown | add counters before judging |

Not active in this run (0 or off; s2-on never reached the heavy phase, see item 12): stand-in formats, depth bounds, topology rewrite, root-view drops,
dynamic-slot exhaustion, levels without memory, unmapped by-address reads, indirect clamps (0
records emptied), pipeline-wait drops ("0 being made"), the loop limit (only with fork-run -Profile
amd -Diag). No magenta/purple filter exists in this base (docs/base-comparison-2026-11-26.md).

```text
Stage / blocker:          Stage 3, inventory item 3 (stale compiler-crash list)
Baseline commit + exe:    b8a88c42 / a1efef7a (diag 8229C308...), then f656af43 (diag)
Scene, route, settings:   save 'openworld', 240-300 s, base-play.env, warm cache handlefix
Observed evidence:        warm-bad.txt (42 keys) came from the cache made before the REUSE fix;
                          21 compute shaders refused, ~74 dispatches/frame; offline 35/35 compile.
Hypothesis / falsification: they compile with the fixed translator; falsified by compiler crashes.
One change:               s3-badlist: VICTORY_WARM_BAD=0. Then the code fix (f656af43): each line of
                          warm-bad.txt carries known::fingerprint() (translator + settings + driver);
                          only lines of the fingerprint in use count, so a translator fix or a new
                          driver retries them; `victory warm` reads and writes the same form.
Before / after:           s3-badlist: 20 of 21 shaders made; "refused" dispatches ~74/frame -> 0; the
                          indirect drops of the steady scene 73/frame -> 0 (they were the same
                          pipelines run by ExecuteIndirect: items 3 and 4 were one). One shader,
                          compute f16876929fa64f9f (key 0629334f...), still crashes AMD's compiler
                          (amdxc64 +0x861b9f, caught by the guard). s3-fp (defaults, f656af43): log
                          "listed=43 counted=0", the same one crash, re-listed with fingerprint
                          e9cc66c4ece2c90a; next runs skip it alone.
Rendering checks:         world +6/+300/+1200 against s2-on (SSIM 0.70/0.91/0.92, scene drift): same
                          scene, no new defect; the purple blinds unchanged (not these passes).
Memory / stability:       0 device losses; commit peak 63.0-63.3 GB (64.2 before); frame time of the
                          steady scene 59-61 ms (s3-badlist), 49-53 ms (s3-fp): not claimed as a gain
                          (one run each, scene drift).
Decision:                 keep (f656af43). Remaining: f16876929fa64f9f (a Stage 1 compiler repro case).
Next single action:       inventory item 12 (below): "indirect kind N: prepare failed".
```

Item 12, found while checking this: in a heavier phase of the same save (2-4 min after the world
appears, ~11,000 draws/frame instead of ~5,800; most likely where the user moved the character, see
the correction in the Stage 2 record), ExecuteIndirect calls are dropped with "indirect kind
0/1/2/10: prepare failed", **500-1,650 executions/frame**, frames 70-250 ms. Present in earlier runs
with the old crash list too (s2-off, share40, rec2, clean-on: 4-50 log lines each), so not caused by
this change; s2-on never reached the phase. `draw.rs` ~4143.

```text
Stage / blocker:          Stage 3, item 12: "indirect kind N: prepare failed" in the heavy phase
Baseline commit + exe:    066d4c08 (diag: the slot counts and the free count at the first refusals)
Scene, route, settings:   save 'openworld', 330 s, base-play.env (s3-slots)
Observed evidence:        "prepare failed" is State::prepare/bind returning false; the cause under it
                          is "no free dynamic descriptor slots: a table is not made" (descriptor/
                          host.rs), within 3 s of the heavy phase in every run that reached it
                          (clean-on, share40, s2-off, rec2, s3-badlist, s3-fp; s2-on never did).
                          Direct draws refused the same way are dropped too.
Measured:                 host view heap 2^22: title slots 1,245,184, dynamic 851,967 (the rest of
                          the lower 2^21; raw twins of every slot, dynamic ones included, fill the
                          upper half). At the first refusals free = 5,500-22,000 of 851,967: 97-99%
                          held (exhaustion, H1; fragmentation H2 secondary: 22,000 free without a run
                          of 254). Demand: 15,000 tables/frame steady, 24,000-30,500 in the heavy
                          phase, tables of 26-254 slots; refused 50-967 tables/frame; the slot search
                          grows from 6 to 450 ms per line as the heap fills (the 70-250 ms frames).
                          Slots go back when a list's submission has run (State::tables_done):
                          ~18 submissions wait in the async line (under half a frame), so the hold
                          time is normal; the demand per frame is near the capacity.
Hypothesis:               the heavy phase drops draws because per-draw tables need more dynamic slots
                          than the heap has room for below the raw twins.
Options (not yet chosen): A) heap 2^23 with the raw twins at 2^22: dynamic up to ~2.9 M (the cap
                          VICTORY_DYNAMIC_SLOTS / 4x2^18 raised with it); every shader translated
                          again (the limit is in the cache key), +128 MB of descriptors on a card
                          already over budget; needs the driver to take 2^23 views (probe).
                          B) fewer slots per draw: copy only the part of a table its shaders read, or
                          share identical tables across lists: also a Stage 4 cost cut, more risk.
                          C) a faster free-slot search (a free list instead of a linear scan): cuts
                          the stalls, not the refusals.
Next single action:       pending the choice of A or B.
```

```text
Stage / blocker:          Stage 3, item 12: option A chosen by the user (bigger heap)
Baseline commit + exe:    0fd737b3 (diag)
Scene, route, settings:   save 'openworld', 300 s, base-play.env, user moving into the busy area;
                          first run after TRANSLATOR_VERSION 3 (every shader translated again)
Probe:                    the RX 9070 takes shader-visible view heaps of 2^21 to 2^24 views
                          (ignored test view_heap_sizes_the_host_takes).
One change:               raw twins at 2^22 when the host takes 2^23 views (VICTORY_BIG_VIEW_HEAP=0:
                          2^21 as before); SLOT_STRIDES_COUNT 2^22; dynamic slots up to 2^21
                          (VICTORY_DYNAMIC_SLOTS default 8x2^18); TRANSLATOR_VERSION 3.
Before / after:           heap 4,194,304 views + twins (2^23); dynamic 851,967 -> 2,097,152. Busy
                          area 23,600-28,100 tables/frame: refused 50-967 -> **0**; slot search
                          6-450 ms -> 3-10 ms per line; "prepare failed" lines 28-33 -> 0; indirect
                          drops in the busy scene 157-778/frame -> 0 once compiling ended (02:21:38 on;
                          the remaining ~280-470 drops/frame are the builder's ~330 and pipelines
                          still being made). (The 30,500 tables/frame of s3-fp's peak not reached.)
Rendering / stability:    0 device losses; the one known compiler crash (f16876929fa64f9f) again,
                          caught, re-listed under the new fingerprint. Commit peak 67 GB (63 before;
                          this run compiled 2,000 pipelines; descriptors +128 MB on the card).
Frame time:               not comparable: ~300 ms frames for 3 minutes of compiling, then 59-153 ms in
                          the busy area; a warm run in the same area is needed for that.
Decision:                 keep (0fd737b3).
Unrelated, noticed:       victory-shader's GPU test the_record_count_of_a_structured_buffer_read_by_index
                          ends its process (0xC0000005) with or without this change; the other 301
                          pass. Earlier today the whole suite passed: a driver-side crash to look at
                          with the Stage 1 compiler cases.
Next single action:       a warm run in the busy area (user moving) for frame time and the picture.
```

s3-heap-busy (0fd737b3, second run after the translator change, user moving into the busy area,
360 s): peak 30,203 tables/frame, **0 refused**, search 6.2 ms at the peak; 0 "prepare failed", 0
indirect drops, 0 compiler crashes (the known one now listed under the new fingerprint), 0 device
losses, commit peak 66.4 GB. Not yet warm: pipelines "being made" fell from 277 to 0 over the first
4 minutes (the first run compiled only the places it reached), with 167-327 ms frames among them.
After that (last 45 s, 7,000-9,000 draws/frame) 57-110 ms, mean ~81 ms; s3-fp's busy windows (before
the fix, 10,800-11,700 draws, dropping work) 71-94 ms, mean 77 ms. **Frame time: inconclusive**
(compiling tail, different movement, different draw counts). No regression is shown, none excluded.
Pictures not captured (no capture switches in these runs). Item 12 is fixed for correctness; its frame
time belongs with Stage 4/6 measurements on a fully warm cache.
Next single action (Stage 3): the purple blinds/glass traced to their first wrong pass (save
'openworld', VICTORY_CAPTURE_FRAMES + VICTORY_DUMP_TARGETS at the frame where they show, user idle).

### Stage 3: what the user's F9 captures show (s3-purple, 47391920, 27 Nov)

New tool (47391920): with VICTORY_CAPTURE set, F9 saves the next frame (picture, ops list, and with
VICTORY_DUMP_TARGETS what it renders to). 12 frames caught while playing (5001 ... 13035). Six kinds of
defect, by the user's report the biggest first:

| # | Defect | Frames | Known |
| --- | --- | --- | --- |
| 1 | Objects missing up close or stuck half-faded (dither holes), visible again from further away or another angle | 5001, 5059 (same palm half-faded 4 s apart); 6707 (a tree over the boat missing) vs 6931 (there) | the holes are already in the depth buffer (frame 5001 target 06): drawn faded from the first pass, not TAA; the fade does not finish |
| 2 | Purple/magenta foliage, in shade and on frond undersides | 6931, 13035; pinkish fronds 11338-11882 | refusals are 0, so not the old texture cause |
| 3 | Shadows near black (no bounced light) | 6707, 6931, 11273 | ray-traced light absent (builder off, item 1), stand-in only |
| 4 | Blue rectangles on shaded ground | 6707, 6931 (under the stilt house) | not traced |
| 5 | Purple glass/blinds | s2/s3 captures | not traced |
| 6 | Smear/ghosting | rec1, rec2 | not traced |

2-5 are all lighting/reflection results (wrong only in shade, on glass, at angles): the leading common
cause is item 1 of the inventory (the structure builder off, rays find nothing). Not counted: the bright
pink shrubs of 12084/12139 (flowering shrubs).

```text
Stage / blocker:          Stage 3, defect 1 (objects missing / stuck half-faded)
Baseline commit + exe:    47391920 (diag)
Scene, route, settings:   save 'openworld', user playing through the same yard and stairs;
                          play-safe.env = base-play.env without its 13 speed shortcuts (THIN,
                          CARD_STEPS, JOIN_PARTS, UPLOAD_AHEAD, RELAY=0, RELAY_COPY, WIDE_BACK=0,
                          JOIN_COPIES, CARD_REHASH_RUNS, MAP_THREADS=1, INDIRECT_WINDOW,
                          INDIRECT_BATCH=0, TEXTURE_NO_WAIT), those on by default set off
Hypothesis / falsification: a speed shortcut delays when GPU-written data is seen, so the fade state
                          never advances; falsified if the defect is unchanged with all of them off.
Result (user):            "exactly the same" (run stopped early by the user, s3-safe).
Decision:                 rejected: none of the 13 shortcuts causes defect 1.
Next single action:       one run with GPU-to-CPU read-backs off (VICTORY_READ_BACK=0), diagnostic
                          only: if the fades change sharply, the game's visibility decisions come
                          from read-back data and that path is traced next; if not, a per-pixel
                          "which draw wrote this" tool for a captured frame.
```

Defect 1, three more causes rejected (27 Nov, a444f002 diag, user playing the same yard and stairs,
save 'openworld'; each verdict the user's):

| Run | One change | What it tested | Check that it acted | User | Decision |
| --- | --- | --- | --- | --- | --- |
| s3-readback | VICTORY_READ_BACK=0 | texture-to-buffer read-backs carry the visibility data | - | still pops | rejected |
| s3-picked | VICTORY_PICKED_WRITES=2 | writes through heap-picked views (never landed, refreshed over) | log: 1 buffer of 51 found written, 24 MB/frame declared and landed | still pops | rejected |
| s3-stream | VICTORY_DSTORAGE_LOG | high-detail models arriving late or never (the object shows only from further away) | 138,389 requests, 0 failed; after the load (50,350): median 4 ms, p90 95 ms, p99 793 ms, max 860 ms; the title sees a status 9.7 ms after it is written | still pops | rejected (a 0.9 s worst case cannot keep an object away until the player walks off) |

Code survey (read only) on the same question: no fence is seen complete before a declared copy-back
lands; queries can only say "visible" (occlusion 2^20 samples, binary 1); writes through undeclared
views, texture-to-texture copies and render targets never reach title memory.

So: four ways data travels between GPU, CPU and disk are cleared. Leading hypothesis: the title's own
GPU-side visibility (instance culling against the previous frame's depth, with a per-instance
"was visible" history that drives the fades) gets one step wrong on the host and keeps visible objects
culled or half-faded. Falsification test: in F9 frames, the counts the card actually draws per
ExecuteIndirect (count buffers and instance counts read back after the frame): a missing object's
draws present with 0 instances where the same view with the object shows them > 0.
Next single action: build that counter (diagnostics only), then one play run with two F9 presses at
the same spot (object missing / object there).

Defect 1 continued (27 Nov, 03:10-03:40):

| Run | Build / change | Result | Decision |
| --- | --- | --- | --- |
| s3-counts | 16a907e6: F9 frames also write `frame-N-counts.txt` (each ExecuteIndirect's arguments as run, read back) | works (242-582 executions a frame); the user's pair 3093 (stairs palm invisible) / 3407 (there) differ in view too much (582 vs 447 executions) to single out the palm's draws | tool kept; needs near-identical views |
| (log) | indirect budget rule (VICTORY_INDIRECT_REST, AvatarManz findings #4) | 0.00 records a frame emptied for budget in s3-counts, s3-picked, rec2, s3-heap-busy | rejected |
| s3-plain | VICTORY_SHADER_OPT=0 + VICTORY_RDNA_FLOAT=0 | AMD's compiler crashed on 5 pipelines (graphics among them); white screen | invalid, stopped; the clean-up passes keep the compiler alive |
| bb-popin2 | Belobug rx9070xt 81ebb7bd, its play settings | still pops | not our merge |
| av-ports2 | AvatarManz 014d52de (newest: its 26 Nov pop-in fix, ports 1 and 2 improved), its README's tested settings: patches 1, 2, 5, 6, 7 on; after its 10-minute compile (0 pipelines being made) | **still pops** on this PC | AvatarManz reports the trees and palms fixed on an RX 7900 XTX / 64 GB; here, RX 9070 (RDNA4) / 32 GB, driver 32.0.32029.2010, it is not |

Host adapter checked: the layer uses the RX 9070 (not the CPU's integrated graphics). Conclusion so far:
the defect follows this PC, not a build or a missing patch. Leading suspects: the RDNA4 card or its
driver (AMD's compiler already treats RDNA4 differently: the REUSE crashes, Belobug's #324), the
translator's RDNA-specific forms (VICTORY_RDNA_FLOAT alone, passes kept), wave-size choices on this
host; then the 32 GB of RAM (the PC runs at ~1 GB free, Stage 2).
Next single action: VICTORY_RDNA_FLOAT=0 alone (a warm-up run unattended, then a verdict run).

Defect 1, 27 Nov 03:40-04:10:

| Run | Change | Result | Decision |
| --- | --- | --- | --- |
| s3-rdnaf-warm | VICTORY_RDNA_FLOAT=0 (passes kept; judged once nearby pipelines were made) | still pops | rejected |
| (code) | the loop cap | it is 2^20 iterations (lower.rs LOOP_LIMIT), not 128/256 as the source survey said | rejected |
| (code/logs) | indirect DispatchMeshX dropped | the warning never fired in any run; indirect mesh work runs (29,000-41,000 groups a frame) | rejected |
| (web) | a newer AMD driver | this PC's 32.0.32029.2010 (18 Nov) is newer than the newest release notes found (26.9.1, Sept) | not tried |
| rec3 | user's own save, user walking; a capture (picture, ops, counts) every 20 frames, screen recorded | see below | - |
| rec4 | VICTORY_GPU_LOAD_MS=25 (the title's four GPU timestamps a frame say 25 ms instead of 0) | still pops; presence flips of mesh+pixel draw types 0.196 per type per capture step (rec3: 0.147; routes differ) | rejected |

**The user's own description:** an object is drawn far away, invisible in a middle band of distance,
and drawn again close up; many objects, not only trees.

**rec3, the stairs palm, captures 20 frames apart:** 3100 ghost, 3120-3160 solid, 3180-3200 ghost. Four
mesh pipelines (6cc4dd2e/278a8c57, d9c3bcbf/d20f3c14, f0aee31b/5dff3af2, f2b41c69/e6bc7adb, 60-235
groups a frame) are **absent** from the frames the palm is a ghost in (not run with zero work: no
execution of them at all), while those frames have more mesh executions in all (373 vs 206): the
title draws the same scenery another way in those frames and back again a second later. The title's
own level-of-detail / fade state flips; no drop of ours is involved (ops: only the builder and the
skipped 39f0e9ae dropped).

Leading hypothesis: what the title decides that state from is wrong here. Candidate not yet tested:
the merge of a submission's GPU writes into title memory keeps the **title's** byte where both changed it
while the GPU ran (shadow.rs ~1918, settle path; the relay merges likewise for bytes the title wrote
after sending them). A CPU-clears / GPU-sets visibility flag loses the GPU's set that way, and our GPU
runs a frame or more behind the CPU, so such conflicts are more common than on the console. Falsification:
a diagnostic switch giving the GPU's byte on conflicts (risk: the crash the rule was made for,
PalmVIDev e0b70476 / VIctory f39c6c2) changes the flips (rec-style captures) and the user's verdict;
unchanged, rejected.
Next single action: that switch, then one recorded walk.

rec5 (b3bdf46b, VICTORY_GPU_WINS=1 on: log confirms; save 'openworld' copy, user walking, 179 s): **same
pop-in** (user). Rejected: the conflict rule of the merges. (Its captures were asked from frame 12,000
on; the run reached ~2,500: none saved. The user's verdict decides it.)

The user's quickest check: the palm beside the stairs, a few seconds' walk from the save's start, is
missing every time. That is a fixed, repeatable failure: the next step is an automatic test (save
'openworld', scripted keys to the stairs, VICTORY_KEYS holds, a capture at fixed frames, palm present or
not in the depth target) so that the remaining causes are tested without the user playing.

s3-1080 (b3bdf46b, VICTORY_OUTPUT=1920x1080, save 'openworld'): the frame rate rose from ~15-17 to
~21 fps (46-49 ms at 4,300-7,300 draws); the stairs palm still missing, same pop-in (user).
AvatarManz's note that the title paces its streaming by its own frames made frame rate a candidate
lever; a 40% faster frame changed nothing visible: rejected as the main cause (30 fps not reachable
to test fully).

### The automatic stairs-palm test (27 Nov, 94ca52bd + 7932dc69)

- `VICTORY_INPUT_RECORD/REPLAY=<file>` (diagnostics): the first pad as the title gets it, recorded and
  played back from the world's first frame (`victory_gpu::world_started`), so a walk is the same in
  every run whatever loading takes. `VICTORY_CAPTURE_AT_MS`: captures (as F9) at times after that frame.
- The user's walk, `D:\victory-runs\routes\palm.tsv` (save 'openworld'): still at the start 0-20 s
  (the palm beside the stairs invisible: the middle band), closer 21-30 s, still 31-35 s (visible), back
  36-38 s, still 39.5 s on (invisible).
- `D:\victory-runs\palm-test.ps1 -Label <x> [-Extra ...]`: replay, captures with targets and counts at
  15, 33, 44 s, in about 2.5 minutes, nobody playing.

palm-base1 (7932dc69, base-play.env): **reproduced**: 15 s palm missing, 33 s drawn, 44 s missing.
`mesh f0aee31b12ffcb28 / pixel 5dff3af2d8f8a06e` runs 7 times close up and not at all in either start
frame (one of rec3's four foliage types that vanish together); no draw type common to the three frames
drops to zero only at the start (the medium detail is not culled to nothing on the GPU): at the
start position the title issues no draw for the palm: its CPU-side decision.

**CORRECTION (27 Nov, later): not established.** Ops and counts are attributed to "the frame being
presented + 1" when recorded, and recording comes in bursts (the title's threads, our THIN workers): five
consecutive frames of blinds1 held 1,028 / 19,591 / 120 / 1 / 14,252 draw operations, against 5,000-11,000
draws a frame in the log. A single captured frame number holds anything from none to several frames'
work, so a draw missing from one says nothing. The same applies to rec3's single-frame captures (300-840
draw operations each) and to every "absent" conclusion drawn from them. From now on: a span of consecutive
frames (VICTORY_CAPTURE_SPAN), its total checked against the log's draws a frame, before any presence or
absence is read from it.

Next single action: record, per ExecuteIndirect of a captured frame, the title's return address (the
code that issued it); take the one issuing f0aee31b close up; read that code (and what it reads) at the
start position for the branch that leaves the palm out.

### Overnight, unattended (27 Nov 04:30 on; the user asleep, no one playing)

| Run / step | What | Result |
| --- | --- | --- |
| palm-callers (186a10d9) | callers by RtlCaptureStackBackTrace | every indirect execution "from +0x2dda7<+0xc9c5c"; both land mid-instruction (cdb -z on the exe, read only): the unwinder loses its way at the stubs between the title and us. Useless |
| palm-callers2 (5dea60c4) | callers by a stack scan for return addresses after calls | none found (the module name test failed); fixed in f7778c76 (the range from the loader), not yet run |
| palm-seriess | --console series-s | the same: palm missing at the start, drawn close, missing back; far islands' trees strongly magenta on Series S | only that the Series S detail tables do not remove the defect; the title's detail-level logic itself is NOT ruled out (corrected 27 Nov) |
| palm-rt | VICTORY_BVH_BUILDER=1 (ray tracing on) | **the builder runs: 53 pipelines, no crash, no device loss for 2+ minutes, ~6 fps** (before the DispatchX handling it overwrote memory); the purple blinds stay purple with it; the walk did not replay (the opening shots last longer at 6 fps) |
| palm-face | the walk again, capture at 19.5 s (start, after the user's camera turn) | the walk went elsewhere (the character still leaning at 19.5 s, at the door at 33 s): the camera's direction after the opening varies between runs and the recorded stick inputs are relative to it. **The recorded walk is not a reliable test**; palm-base1's clean result was in part luck |

Code checks: texture views keep the title's component mapping (`srv_desc`); the frame's 49,586 view
records and 33,917 placed views all use the default mapping (0x1688) except buffer views: no swizzle
cause for the purple blinds. (This concerns colour only; it says nothing about an absent draw.)

**Scope note (user, 27 Nov):** a cause counts as ruled out only where the test actually exercised it.
Lighting and the purple blinds are parked; the disappearing objects are the only active investigation.

Purple blinds traced (blinds1, VICTORY_CAPTURE_SPAN=3, 455 targets of frame 1557): the geometry pass's
targets (50 material code, 51 grey, 52 normals, 54 motion) hold no purple colour; the first purple is in
the **lit HDR scene colour** (2560x1440 RGBA16F, 0x158d90cfcb0), which a compute chain (2da4beb3 ->
3d002bca/f753697c) downsamples into a half-size blur pyramid (target 55, 1280x720 R11G11B10). With the
builder on (ray-traced light, stand-in off) the blinds stay purple. So the purple is made by the lighting
of the blinds (or a translucency/behind-glass pass), not by their stored colour, not by a channel
mapping, not by the stand-in. Lead: the lighting takes a material class from the stencil plane (the
frame's stencil values: 0, 7, 64, 72, 8, 16, 65, 3, ...); the console's D16S8 views (118-120) are
read as host 45-47.

New tools: VICTORY_CAPTURE_SPAN (consecutive frames per capture), VICTORY_INPUT_REPLAY_OFFSET_MS,
`D:\victory-runs\palm-strip.sh`, `region-colour.sh`.

**A repeatable view: the opening shots.** After the save loads, the camera flies a fixed path round the
house before the player has control, whatever the input: the same shots in every run. With timed
captures a second apart (2de7bf32: captures kept in a list; one slot lost them while loading at two
frames a second) and `D:\victory-runs\match-shots.sh` (SSIM pairs of two runs' shots), runs can be
compared shot for shot.

fly-off2 / fly-rt2 (2de7bf32, base-play.env, ray tracing off / on, D:\victory-scratch\capture\fly-pairs.png):
matched shots (SSIM 0.77-0.87). With the builder on the lighting is the title's own: deeper shading on
the house front, under the deck and in the trees, richer foliage; off, the stand-in is flatter and
lighter in the shade. No purple in these shots either way. Cost: ~6 fps against ~15. The ray-traced
picture is the target (the Series X runs with ray tracing); its frame time is Stage 5's.

Stencil planes read back are now saved as images too (b70e3c94: grey = value), for the material class
of the blinds against their neighbours (not yet run).

Defects seen but not explained by any omission yet: the purple glass/blinds/papers (reproducible:
save 'openworld', world frame +300, both s2 runs), the smear/ghosting (rec1, rec2), vanishing trees.

### Stage 3 defect inventory from session2 (open world, first look)

1. Purple/magenta foliage almost everywhere, day and night (the papers' defect, far
   worse outdoors); some foliage green instead (211845).
2. Washed-out, smeared frames (212520-212722): the view goes white-blue and blurred
   with streaks; looks like temporal accumulation (anti-aliasing or motion blur) and/or
   exposure gone wrong. Probably most of "unplayable".
3. Very dark nights and interiors with windows blown out to white (212349, 212419):
   bounced light missing, as in the prison.
4. A black frame (212218), probably a load.
| 11-26 | 9dfbe7c6 | cold + bad list (67) + VICTORY_EARLY_ALL=1 alone (coldearlyonly, 300 s) | the 20 s wait is not needed | faces render (20:13: the guard's face and hands, the main character's ear and neck) | **fix for cold-start faces: VICTORY_EARLY_ALL=1**; the wait budget is not part of it |

rt-soak2 (8ebc2b11-era build, VICTORY_BVH_BUILDER=1, save 'openworld', idle, stopped at 9.0 min at the
user's request): **0 errors, 0 device losses**; after loading 4.6-15.6 fps, median 9.8; private memory
33.9 -> 36.8 GB over the run (still rising at the end: to be watched when ray tracing is taken up again).
Ray tracing parked; the disappearing objects are the active work.

### Disappearing objects: validated tools and verified observations (27 Nov, morning)

Tools, each checked before use:
- **Capture completeness.** VICTORY_CAPTURE_SPAN=6 holds 6.6-8.9 frames' worth of draw operations (the
  log's draws a frame as reference); single-frame captures hold 0-3, so absence is read from spans only.
- **Callers (b2bffa15).** The stack scan finds nothing with the play switches (the title's draw calls
  are recorded later on our worker threads: the scan ran 3.5 KB below the top of a fresh stack, 0 title
  values). With all speed shortcuts off (play-safe.env) and VICTORY_THIN=0 it finds title return
  addresses; checked in cdb on game_scarlett_master.exe (read only): each follows a call instruction and
  they chain (+0x9786ab calls +0x24962e0 ... +0x2496802 calls +0x248ca90, which holds +0x248cbc1, the
  vtable call into the layer). Validated.
- **Positioning by hand.** VICTORY_INPUT_LIVE (a file the pad is read from) and VICTORY_CAPTURE_TRIGGER
  (8ebc2b11); `D:\victory-runs\drive.sh stick|snap`.
- **Arguments as written vs as run (5924cff0).** Counts lines give title memory at the arguments when
  recorded and at the frame's present, beside what the card ran.
- A hardware write breakpoint (cdb attached) on 0xd9991fc60 caught no write in ~10 frames (the debugger
  slowed the game to 0.2 fps; the game hung after detach): inconclusive, not used.

Verified (session palm3, save 'openworld', play-safe.env, complete spans):
1. The stairs palm (fan palm at the foot of the stairs) is drawn in the opening shot (far), drawn from
   the deck in one span (p5) and missing from nearly the same viewpoint in another (q4) and after a
   step (p6): it flips at a fixed place.
2. Missing: pipeline `mesh fd08d25a / pixel 2af15279` (and `mesh 18bbb989`, same pixel shader) not issued
   (p6) or issued with 0 groups (q4); visible: 84 executions, 652 groups (p5); far: 44, 1,438.
3. Its arguments are not the title's CPU writes: title memory at them reads 0 while the card ran 22, 140
   ...; a writable buffer view (address 57,063,513,120, 32 KiB) covers them, bound by four compute
   passes, `3b471c12`, `9f017d90`, `764ca63d`, `87bf9e3f`, which run in both states (9f017d90 3,006
   groups visible and missing; 87bf9e3f 12,024 both).
4. compute `e8c96235` (6 dispatches) ran 6,578 groups visible and 0 missing; its arguments (0xd9991fc60)
   are in the title's per-frame CPU data (beside a camera constant buffer) and title memory holds real
   counts there (72-82 in the opening shot).

**Not yet established: the first difference that accounts for the disappearance.** The palm's draw
arguments are produced on the GPU by culling/argument passes that run in both states; what they read
that differs between the states is the next question.
Next single action: in a visible and a missing span at the same place, dump the 32 KiB argument buffer
as the card has it after the four passes and the inputs they read (their tables in the ops), and find
the first input that differs for the palm's records.

### Disappearing objects: the argument buffer checked, and a correction (27 Nov, session palm4)

Tools (d4ba6422, 4fcbfbe8): `VICTORY_DUMP=<addr>,<len>` with `VICTORY_DUMP_AFTER=<hashes>` copies a title
buffer's host copy out after each named pass and before a list's first draw reading it (frame-N-dump.bin,
indexed in frame-N-dumps.txt, a DUMP line in the ops); `VICTORY_SKIP_ARGS=<file>` drops indirect executions
whose arguments lie in a listed range, or (one-word lines) draws of a named pipeline, reread every 200 ms.
Both worked in palm4 (133-200 dumps a frame; drops seen in the log and in the spans' ops).

Submission structure of the 32 KiB buffer (57,063,513,120), from complete spans: it is not one object's
arguments. It is shared by ~80 lists a frame, one per culling view (main view, shadow cascades, others);
each list clears it (`9146291b`, 16x1x1, whose only writable view is the whole buffer), fills it with a
pass pair (`3b471c12`/`9f017d90` in the main view, `764ca63d`/`87bf9e3f` or `fc5e66f8`/`0452fc2d` in
others), clears/finalises again, fills again (two phases), then draws. A 32-byte slot is a bucket of that
view's fill (many mesh pipelines read the same slots in different lists), and slot numbers shift as the
view changes. Our clamp's budget emptied no record: 0.00 records a frame for the whole palm3 run.

**Correction.** In the middle state the stairs palm is drawn, as a ghost: absent from the grill deck (b13),
a faint dithered outline from the stairs (b16), about half opaque two steps lower (b17, c0), a smooth
temporal-dither translucency (TAA resolving a per-pixel dither). It fades in with distance over several
metres. Its draw remains; the earlier "not issued / 0 groups" link to `fd08d25a`/`2af15279` was a
correlation, and a causal test says it is not the palm:
- c1/c2: dropping the slots the `fd08d25a`/`2af15279` draws read (0xd494034e0-0xd49403700): ghost unchanged.
- c3/c4: dropping every indirect execution reading the whole 32 KiB buffer: the character, deck, railings,
  dock and house disappear, **the ghost palm stays**. The palm is not drawn from this buffer.
So items 2-3 of the morning's "verified" list describe a bucket that changes with the view, not the palm;
the per-pass comparison of that buffer does not bear on the palm and was not pursued further.

Also found: after ~30 s without input the game's idle camera takes over (the "opening shots" seen while
waiting were it); drive.sh now nudges the camera before each capture.

Next: with the camera held on the ghost, drop pipelines by name (bisection over the main view's draw
pipelines) until the ghost goes; then that draw's fade input, ghost against solid, and whether another
LOD is meant to cover the range where this one is faded.

### Disappearing objects: what hides the palm (27 Nov, session palm5, 4fcbfbe8)

Method: camera held on the stairs palm in its ghost state (drive.sh nudges the camera before each capture
so the idle camera never starts), then pipelines dropped by name through `VICTORY_SKIP_ARGS` and the
picture compared: 220 pipelines drawing to the 2560x1440 view, halved eight times, each finding checked
alone (D:\victory-scratch\capture\palm5: e*-j* crops, k0/k1 full frames). Same place, same view, same
moment for every pair: no camera difference to account for.

Each alone:
- dropping `mesh 89a62261 / pixel 6539386a` (G-buffer pass, 3 targets, main depth): **palm gone**;
- dropping `vertex 25ebc920 / pixel 1cd9bc63` (screen-space tiles drawing into scene colour, depth test
  EQUAL against a separate depth buffer, no depth write: deferred shading for one material class):
  **palm gone**;
- dropping `vertex 24abbd85 / pixel 2de553a0` (screen-space water composite): water gone and **the palm
  drawn whole** where it was a ghost (k0 against k1);
- the other seven of the last sets: ghost unchanged.

So in the middle state the palm is drawn, G-buffer and shading both, and the water composite paints over
it. The composite's pixel shader reads a water-depth texture at the pixel, discards where it is 0 (no
water) or a G-buffer flag says so, and otherwise writes water colour and that depth (SV_Depth) with the
depth test set to ALWAYS and stencil 0x17. It does not compare against the scene's depth itself; what keeps
water off nearer objects must be in its inputs: the water surface (`vertex 17076288 / pixel 7933d968`,
drawn into its own 2560x1440 depth buffer, depth test GREATER, with no clear or copy into it seen in its
list) and that shader's own per-pixel test (it reads two textures at the pixel and works in coordinates
(pixel+0.5)x2, i.e. built for a half-resolution pass). Why that is wrong: the palm stands on land in front
of the water; nothing on the console version covers it with water (and in our own picture the dock posts
and the character in front of the same water are not covered). Which input lets water through behind the
palm (a depth source without the palm or with it at half resolution, the order of the lists, or the
half-resolution mapping) is not yet established.

Not the explanation, each tested: the 32 KiB argument buffer (palm4 c3), our clamp's budget (0 refusals),
a missing draw (the palm's draws run), another LOD meant to cover the range (the same G-buffer pipeline
draws the palm in the ghost state; it is hidden, not replaced).

Next single action: in the ghost state, save the water pass's targets and depth (0x...7a640 at the
frame's end, VICTORY_DUMP_TARGETS) and the two textures its surface shader reads, and see at the palm's
pixels whether water depth is present there and what the scene depth it tested against holds.

Session palm6 (ef62ab0b): is it only the palm? Pictures with the water composite (`24abbd85`) on and off,
camera unchanged (D:\victory-scratch\capture\palm6\pair-*.png), from the deck, the stairs and the garden
facing the water, and towards the house and the yard. Facing the water the palm's ghost returns whole
with the composite off again (w3); the composite paints over whatever stands in front of water. Facing
land or sky, no object came back with it off in these views (v2, v3, w3's left half) - but none of those
views was checked to contain a missing object, so this neither shows nor rules out that inland objects
vanish the same way. Not established: whether objects with land behind them disappear by this pass or by
something else; needs one such object, held in view, through the same bisection.

Session inl1 (447df7ed) and rec2 again: objects without water behind them. From the deck facing inland
(street, blue house, trees against sky) no object was found that stays missing; a big tree showed a
half-done crossfade (two versions dithered over each other) right after the camera turned, and was whole
a few seconds later with the camera still (n1/n2 against t-on/s1/s2). In the user's rec2 (17 fps,
base-play) the two tall palms by the house go see-through against open sky while the player walks
(238-242 s) and are solid again once he stands (246-264 s), while the stairs palm over the water stays a
ghost. So a fade does show against sky, but in what was seen it finishes; the object that stays missing
in all captures so far is one in front of water. An inland object that stays missing has not been
found by searching; one named by the user is needed for the same bisection.

### Disappearing objects: what the "ghost" is (27 Nov, sessions hz1, fm1, tr1, tw1, tw2)

Ruled out by test (each exercised, picture compared at a held view):
- Occlusion pyramid builders (`2dc60219`, `42683eeb`, `82849f7a`, the passes that write the 1024x512 R32
  depth pyramid the culling passes read) dropped: the tree by the deck dissolved the same (hz1 h2b/h3).
- `v_fmac_f32` accumulator modifiers (upstream #3158, Vice-Tay 8fe3ccd), now in as `VICTORY_FMAC_MODS=1`
  (off by default; counted either way): 0 uses of them in 4,157 shaders translated in the world (fm1).
- RenderDoc 1.47 (in-app API, `VICTORY_RENDERDOC`): hooks the device, but the capture fails with
  E_OUTOFMEMORY creating its initial-state copies (41,679 resources): not usable at this memory size.

Found (tr1, pipeline bisection by name at a held view, the dissolving tree left of the deck):
- The tree is two draws of two detail levels: `mesh adc5c689 / pixel 64ea0f34` and `mesh 89a62261 /
  pixel 6539386a` (the same G-buffer pipeline the stairs palm was drawn by). Dropping both removes the tree;
  dropping either leaves the other's dithered part.
- The two pixel shaders are the same program (two table offsets differ). Their last discard is a
  screen-space dither: interleaved gradient noise (0.06711056, 0.00583715, 52.9829; frame offset 5.588 x
  (counter & 63)) against a threshold `saturate(alpha x G) x (flag ? F(1-t) : 1-F(1-t))`, with `t` the
  object's fade value (times a near-camera fade), `F` and `G` per-frame constants (1.0, 2.0). A crossfade
  between detail levels by dither; the ghost is that crossfade being shown, not a missing draw.
- `t` comes from the mesh shader's `COLOR3` = bits 16-23 of a per-instance word / 255, read from a small
  raw buffer (`s1+56`, 12-16 words) in the title's per-frame constant ring (0x36..., written by the
  title's CPU each frame). The dither's frame counter (per-frame buffer offset 1520) advances 0..7 every
  frame; the near-camera fade constants settle at 0.15 / 20.0.

So the fade amount of each object is decided by the title's CPU code and handed to the GPU per frame; the
translated shaders apply it as written. What is not established: which instance word is the tree's (the
low 16 bits are not a unique id), and therefore whether its fade byte is held part-way, oscillates, or is
right and something else differs. Next: find the tree's draw and instance (bisection over its draws), read
its fade byte over a minute with the camera still; if it is held or flips, trace what the title's LOD code
reads to decide it (frame time, and GPU results it reads back: visibility/occlusion).

Skipped and refused compute passes against the dissolving tree (27 Nov, ns1, 1468bfd9):
- `39f0e9aeb156a61c` (left out by name since the AMD skip list; reason never recorded) run instead
  (`envs/play-safe-noskip.env`): first bound at frame 1311, no crash in ~5 minutes, and the tree left of
  the deck dissolved the same (ns1 n1 against tw1 w0). Not the cause for that tree.
- `f16876929fa64f9f` (the one pipeline AMD's compiler still crashes on, never made): in tw2 the tree was
  already dissolving at frames 1583-1588 and the pipeline was first bound at frame 2408. Not the cause for
  that tree.
- The other 36 compute pipelines the compiler once crashed on run (hf-measure2, 0 compiler crashes).
Neither result covers every object; each is one tree at one place.

### Disappearing objects: the fade is the title's, by distance (27 Nov, sessions tw3-tw5)

Tools added: `VICTORY_SKIP_ARGS` lines `<pipeline> <lo> <hi>` (drop a pipeline's executions by number within
a list; ops lines show `ord N`); `VICTORY_OPS_WORDS=1` (small raw buffers' words in the ops lines, read
again at the list's submission: `AT-SUBMIT`, and at the presents after it, one log line a frame);
`VICTORY_OPS_AT=<byte offsets>`.

Measured:
1. **The translation of the dither is exact.** The console code of pixel shader 64ea0f34 (disassembled with
   `victory shader-info --disasm` from its .cdxil) does what our DXIL does, instruction for instruction:
   noise = fract(52.98 x fract(0.0671 x py + 0.00583 x px)) after adding 5.588 x ((v14<<4 | 9) + counter & 63);
   threshold = clamp(alpha x G) x (flag ? F(1-t) : 1-F(1-t)); `v_cmpx_gt_f32` noise > threshold kills.
2. **The GPU gets the title's data.** Of ~50,000 small instance lists a frame read again at the first
   present after their submission, 0 had changed (tw5): the title writes them before submitting, our
   copy at submission is what it wrote. (Between recording and submission ~5% change: the title fills
   them after recording the draw; the copy is made at submission, so that is fine.)
3. **Submissions are waited for** (no relay, no late flights in play-safe: `Host::execute` runs and waits),
   so the title never reads GPU results that have not landed: stale read-back is not the cause here.
4. **The fade value is set by distance.** `t` = bits 16-23 of each instance word / 255, written by the
   title's CPU into a per-frame list. With the camera still, part-way values (0x44, 0x79, 0xc4 ...) stay put
   for seconds; after a step back of about a metre every one of them moved a little the same way
   (0x79->0x7e, 0xc4->0xc8, 0x62->0x67, 0x71->0x73, 0xb2->0xb7): a band in space, about 2% per metre, so
   tens of metres wide, in which an object is part-faded.
5. In that band the title's own shader keeps a dithered part of each detail level, with the same noise for
   both; so an object there is see-through by design for as long as it stays in the band.

So the disappearing/ghosting of the palm and the tree is the title's distance crossfade between detail
levels, applied exactly as the title asks, over a band far wider (or in another place) than the console's
must be. What sets the band (the LOD distances and their fade range) is not established; it is computed by
the title's CPU code. Candidates for an input that differs from the console: a quality/LOD scale the title
derives from its frame times (we run 3-20 fps; a 40% faster frame was tried in s3-1080 without a visible
change), the output/render size it is given (`VICTORY_OUTPUT`/`VICTORY_RENDER_SCALE` patch it), or the
console model. Next: the title code that writes the instance word (draw callers +0x4e315bb, +0x32472,
up from +0x555e97c) and the distances it reads.

### Disappearing objects: time, writers, water depth (27 Nov, sessions ws1-ws2, fs1, pz1, wd1-wd2, st1)

- **Not time-driven.** Camera held still for two minutes at normal speed (st1, four spans 35 s apart):
  the part-way fade values stay put (0x9a, 0xa8, 0x69, 0xcc, 0x2a over all four); and with 600 ms slept
  at every present (fs1, `VICTORY_PRESENT_SLEEP`, 0.3 fps) they did not jump to done either. The fade is
  not a timed crossfade that our low frame rate stretches out.
- **Writers.** Watching the pages of the instance lists (`VICTORY_WATCH_PIPES`/`VICTORY_WATCH_SLOTS`)
  caught no write inside the lists themselves (they are not in guarded memory); writes beside them come
  from +0x50ebc64 (an allocator's counters) and +0x557a54e (a prefix-sum loop). The code that sets the
  fade byte is not found this way.
- **The palm against the water depth** (wd2, frame 1897 targets): in the water pass's depth (f39) the
  palm is a dither pattern, about half its pixels; scene depth (f19) has it faint. So the palm is drawn
  part-faded, and water shows through the holes. The water pass is not wrong on its own: it fills what the
  dither leaves.
- Our depth-plane shortcut (`twin_use`, "used before the pass") hits one texture, 2560x1440 format 44
  (the deferred-shading depth copy), read only: not the water depth.

What it adds up to: the palm and the tree are drawn as one detail level part-faded, and where the other
level would cover the holes it is not drawn there (for the palm the near level, `fd08d25a`, contributed
nothing in the ghost state: palm4 c1/c2; its arguments come from the GPU culling passes). Whether the
near level is culled on the GPU or never asked for by the title's CPU is the open question; it is the
next thing to settle (its clusters in the culling output in the ghost state against the solid one).

Sessions nl1, hz2 (27 Nov): the test meant to tell "near level culled on the GPU" from "never asked for".
- From the stairs the palm was a ghost (nl1 g0), and a step along the rail it was gone altogether (c4,
  span from 1973). The solid close-up state was not reached: the character stays in the rail-leaning
  animation and the stick does not take him down the stairs from there.
- Occlusion pyramid builders dropped from the start (hz2, as hz1): from about the same place on the stairs
  the palm is not there at all (hz2 g0 against nl1 g0; the positions differ by a step). Dropping them
  mid-session (nl1 m_hz) freezes the last pyramid rather than removing it, and the camera sway with the
  palm behind the character made those pictures useless.
**Result: inconclusive.** Without the pyramid the palm did not come back, which points away from the
occlusion test, but the pyramid's content when never built is unknown (zero might mean "nothing
occluded" or "everything", depending on the depth convention), and the positions differ; neither cause is
established.

### Disappearing objects: the user's close-up, the arguments' writer, stale memory (27 Nov, user2, dw1-dw2, qw0, lb1, rc1-rc2)

- **user2 (the user at the controls, F9):** solid close-up at the foot of the stairs, and gone from the
  deck above (frame 1997, the garden table in view, no palm, no trunk). Close up, dropping `89a62261`
  alone removes the fronds (the trunk is another pipeline); bisection by draw number: ord 13 is the
  fronds (70 groups; ords 13-15 ran 70, 12, 10). The ghost on the stairs is drawn by the same pipeline
  and the same triple (dw2: ords 17-19 = 70, 12, 10), so it is one mesh, faded. In the invisible state no
  70/12/10 triple ran; there are triples of 0 groups.
- **These draws' sizes are GPU-written** (title memory 0 at the frame's present, the card ran 70) and they
  do reach title memory one present later (lb1, probes read at every present: title = GPU at +1 for
  89a62261). Not a missing copy-back.
- **The writer:** dumps after every pass of the early compute lists (`VICTORY_DUMP_FILE`, ranges and
  passes set at runtime): the value is already there before `91b10a73`, which is a clear (writes zeros),
  and is cleared on the other half of the per-frame ring; the pass that fills it was not isolated among
  ~100 compute pipelines on the async queue.
- **Queue order:** `VICTORY_QUEUE_WAIT=0` (every submission runs as made): palm still gone from the
  stairs (qw0). No forced runs at presents in any captured frame.
- **Stale memory:** `VICTORY_REFRESH_CHECK=1` (rc1) found 7 blocks of the 174 MB heap, 0x2cc90000-
  0x2cd10000, stale three refreshes in a row. New `VICTORY_REFRESH_CHECK=2` (481c4f3b, copies such blocks
  again): 250-440 blocks (16-29 MB) re-copied every 5 s, and the palm on the stairs is still a ghost
  (rc2 g0, c4). Not the cause.

Not established: what makes the title fade the palm out in the middle band, or which GPU pass gives its
draw 0 groups there. Ruled out by test today: shader translation of the dither, instance data reaching the
GPU, frame rate/time, our clamp's budget, the water pass (separate), skipped/refused compute pipelines,
occlusion pyramid, queue waiting, copy-back of the draw sizes, stale heap blocks (as sampled).

### Disappearing objects: the culling pass's detail levels (27 Nov, 11:20-13:30, sz3, w32, nb1, l0, l1, rc3, l1b, fb1, nx, l1fb, l1fb2, dm, rt, l0b, fb2, def, off)

The pass that decides what is drawn is `compute 55679a0547649d0b` (wave64, 6,095 IR instructions; its
code read from `shader-info --disasm`). Per instance it computes the camera distance, picks a **detail
level** `v10` (0-3) against the batch's thresholds, a crossfade factor near each threshold, and a
"stand-in" band factor (per-archetype distances); then it appends the instance to the chosen level's
lists in the 174 MB heap: an instance counter per draw slot, a fade word per instance
(`[31:24]` = 255*distance fade, `[23:16]` = fade, `[15:0]` = instance) and 16-byte group records, or, for
a slot whose record has no mesh word, a group count added to a per-slot DispatchMesh argument
(`heap[s94 + slot*16]` = `[N, 1, 1, 0]`, read by `indirect kind 10` draws of `fe660f2d`, `903776ba`, ...).
Three places write a fade word: the instance's own level (fade = band * crossfade * distance), the band's
stand-in lists, and the next level's lists (the crossfade's other half). The ghost palm's fade word in
sz3 was `ff2d0000` (0x2d/255 = 18%).

New tools: `VICTORY_CODE_PATCH=<file>` (diagnostics: lines `<hash> <offset> <dword>...` replace dwords
of a shader's machine code before translation; `shader-info --disasm` takes the same file to list the
patched code), `VICTORY_WAVE32=<hash>` (that shader translated for 32-lane host waves),
`VICTORY_REFRESH_CHECK=3` (every byte compared, not samples), title-memory dumps beside the GPU's
(`frame-N-title-<seq>.bin`), `D:\victory-scratch\tools\lodslots.js` / `reqs.js` (the pass's tables walked
in a heap dump).

| Run | Change | Stairs palm (g0 / c4) | Decision |
| --- | --- | --- | --- |
| w32 | `VICTORY_WAVE32=55679a05` (two host waves of 32, exact "allocation idiom" form) | ghost / ghost | the card's wave64 code: rejected |
| nb1 | stand-in band off (level 999 and band factor removed) | ghost / ghost | the band: rejected |
| l0 | every instance at level 0, no crossfade | **solid / solid**, the trees before the house back, fps unchanged | level 0 draws everything |
| l1 | every instance at level 1 (its last where it has fewer), no crossfade | solid / **gone**, trees before the house gone | level 1 comes out invisible for many objects |
| rc3 | `VICTORY_REFRESH_CHECK=3`: 650-1,050 blocks (42-67 MB) re-copied every 5 s | ghost at c4 | stale title memory (where no shader writes): rejected |
| fb1 | no crossfade; level-1 "no sub-draws" taken to the level-0 path | solid / solid | (see nx) |
| nx | no crossfade only (control) | solid / solid | **the ghost is the crossfade into an invisible level 1** |
| l1fb | l1 + "no sub-draws" to level 0 | solid / gone; some trees back | one of two cases |
| l1fb2 | l1fb + "slot without a mesh record" to level 0 | **solid / solid**, trees back | **both cases found** |
| dm | full heap dumps (GPU's copy and the title's) after the pass | - | records the same on both sides (0 of 495 empty slots filled on the title's side): not stale uploads; per-slot DispatchMesh arguments `[N,1,1,0]` on the GPU, `[0,1,1,0]` cleared by the title |
| rt | `VICTORY_BVH_BUILDER=1` (the structure builder on) | gone at c4 | rejected |
| fb2 | no crossfade + both level-1 cases to level 0 | **start / g0 / c4 all solid** | kept |
| def | fb2 built in (`VICTORY_LOD_NEAR`, default on), no diagnostic patch | **start / g0 / c4 all solid** | the fix |

**Result:** the objects that vanish are those the title puts at detail level 1 (the middle distance band;
the dithered ghost is the crossfade from level 0 into it). Level 1 comes out invisible two ways: the
batch has no level-1 sub-draws, or the level-1 slot has no mesh record and is drawn by a per-slot
DispatchMesh that shows nothing. Taking those level-1 instances to level 0 brings the objects back.
**Not yet known:** why level 1 is empty here (the title's streaming of those meshes, or what the per-slot
dispatch path needs). Device methods 0x1c8/0x218 (16,384 + 2,048 pipelines the title goes without at load)
remain a candidate for the empty level.

The fix (`crates/victory-gpu/src/pipeline.rs`, `VICTORY_LOD_NEAR`): unset/`1` = no crossfade and the two
level-1 cases drawn at level 0; `all` = every drawn object at level 0; `0` = the title's code. Applied
only where the four dwords are the expected ones (else logged and left).

### Pop-in across the board: user-played tests (27 Nov, 19:00-20:25, 6f4db9ed + diagnostics)

The user played their own save (base-play.env unless noted) and judged each build; every patch's dwords
were checked with `shader-info --disasm` and its load seen in the log.

| Run | Change | User | Decision |
| --- | --- | --- | --- |
| usertest | 6f4db9ed default (`VICTORY_LOD_NEAR`), play-safe.env | stairs palm fixed; other objects still pop | (play-safe.env ~5 fps: the 13 speed shortcuts are off; base-play.env 15-30 fps) |
| userfast | `VICTORY_LOD_NEAR=all` | palm no longer ghosts but switches on/off | - |
| usernofade | all + distance fade gate off in `55679a05` (v5 = 1) | still pops | the tree pass's distance fade: rejected |
| userfamily | `patch-family.txt`: the 30 `InstanceCulling:CS_{Tree,Prop,Grass,Simple,Slod}_BatchCull*` shaders (one source, c47eb2b0) found by pattern; 24 matched: every object at level 0, no crossfade | pop-in across the board unchanged | level choice in the GPU culling family: not the general cause |
| usernocpu | `42683eeb` (`CS_InitDepthPyramidWithCpuOcclusion3x3`): the CPU occluder samples (128x72 R32 the title's CPU draws, uploaded by the layer) left out of the pyramid (9 x `v_max_f32` -> `v_mov_b32`) + default tree fix | **stairs palm fully solid, no ghost, no pop**; others still pop | CPU occluders reach the GPU stale: part of the palm's defect |
| usercombo, usercombo2 | family + CPU occluders off (+ distance fade off in 23 shaders, `patch-family-nofade.txt`) | the palm by the house fixed too; everything else still ghosts or pops | - |
| userback | `VICTORY_TEXTURE_BACK` + 1024x512f41 (depth pyramid) and 9 small GPU-written texture kinds | no change | texture copy-back: rejected |
| userw32 | `VICTORY_WAVE32` for all 77 InstanceCulling / meshlet_culling / depth_pyramid shaders (27 loaded, 0 failed) | no change ("far visible, middle gone, close there", every object) | wave64 on RDNA4 in the culling: rejected |
| userstats2 | new `VICTORY_MESH_STATS=all` (diagnostics: pipeline-statistics-1 + occlusion query around every indirect mesh draw, totals per mesh/pixel shader logged) | - | the per-slot mesh draws of empty detail levels (`903776ba`, `fe660f2d`) **do land pixels**: 72% / 85% of pixel-shader runs pass (tree leaves `adc5c689`: 16%) |

Streaming (s3-stream) and read-backs (s3-readback) had been rejected at 03:10 and were not the question.
The user's description of every object: far detail level shown, middle gone, close shown.
**Corrected:** the per-slot path is not "drawing nothing"; its draws reach the screen. Not yet known where
(wrong place/transform, wrong target, or overdrawn). Next: in an F9 frame, the per-slot draws' output
(target and position) against the same object's level-0 draw.

Later the same evening (20:40-21:10):
- **The palm's exact fix ported** (`patch-port.txt`, generated by D:\victory-scratch\scan\port.js): crossfade
  off and the two level-1 branches redirected to the level-0 block, found by structure in 15 BatchCull
  shaders (Tree, Prop, Grass, Simple, Slod and their ValidateInst variants; the tree lines byte-identical to
  `LOD_FALLBACK_PATCH`; the Prop shader tests level 0 with `v_cmp_eq_u32 s[2:3]` + `s_and_saveexec`, its
  level-0 block returns to the else at 0x2740 as the tree's does). With the CPU occluders off, user on
  their street: **still popping**. The palm's fix does not carry to the other objects.
- Device methods 0x1c8 (16,384+ calls) and 0x218 (2,048+): the description's first word is **one of our
  root signatures** (vtable compared), then (address, size) pairs: 0x1c and 0x14 bytes (linked by a hash
  word), 0x180-0x400 bytes in a separate region (0xd494....), 0x118-0x1d8 bytes of metadata, two
  addresses and a 64-bit key. None of the pieces is a DXBC container. Whole pipeline creations the
  title goes without; what they draw (object materials, or ray-tracing per-material programs) not known.
- **Driver**: AMD Adrenalin 26.11.2 (32.0.32029.2010) replaced by 26.7.1 WHQL (32.0.31035.1003, 24 Jul 2026;
  AMD-signed installer, factory reset), run userdrv2671 (base-play.env, tree fix + CPU occluders off),
  user on their street: **still pops**. The driver version is ruled out (AvatarManz's driver is not recorded
  anywhere). Device methods 0x1c8/0x218 are unimplemented in AvatarManz's build too, which reportedly does not
  pop there: not the cause on its own. The title asks XMemGetWorkingSetStatistics only on Series S.

### Pop-in across the board: what removes the objects (27 Nov, 21:15-22:50)

New tool `VICTORY_PIXEL_WHO=<file>` (diagnostics, capture.rs `who`): boxes `x y w h` of the render targets;
in an F9/triggered span, after every draw the boxes of its first colour target are copied out and
compared with the previous copy **in the same host list** (lists are recorded on several threads; across
lists recording order is not the card's order): `frame-N-who.txt` lists each draw that changed pixels.
Also `VICTORY_DUMP_ONCE=1` (each pass's dump once per F9 span) and the culling heap moves between loads
(0x33d30000 test save; 0x28dc0000, 0x34840000, 0x9830000 on the user's save): read its address from the
s0+688 view (182452224 bytes) of `55679a05`/`17af15c4` in the ops list.

User's save, captures who4 (frames 37460 visible view, 43647 ghost bin, 46690 vanished fence; pictures
in D:\victory-scratch\capture\who4):
- White lattice where it is drawn: two draws, `mesh 89771a8c / pixel 0fbc5c91` and `mesh 7f7b42a2 / pixel
  b5640b10`, ~550 of 1,024 box pixels each, complementary: a dithered crossfade of two detail levels,
  whole between them.
- The same lattice panel where it is gone (in front of the pink bush): only `7f7b42a2` writes, ~100 of
  1,024; `89771a8c` writes nothing there. Its arguments (`0xf0b32e4`, 16 groups) equal what the title's
  CPU wrote (`title_recorded 16`): these draws are sized by the CPU, not by the culling pass.
- The grey bin ghosting close up: drawn by a direct mesh dispatch `mesh c9dee8dc / pixel a2a7199b`
  (~435-500 of 1,024), not by its list draw `9e3584ab / b8df1385` (1,024 from further off). The direct
  draws carry a per-draw constant (s2+0 .w) of 0.2-0.6.
- The pixel shaders' last discard (`b5640b10` at 0xd4, `0fbc5c91` 0xd4, `64ea0f34` / `6539386a` 0xcdc,
  `b8df1385` 0xd4): interleaved gradient noise against a per-object dissolve (`attr7.x`, or 1 - it for
  the other half of a crossfade); `v_cmpx_gt_f32` picks the pixels to kill, `s_andn2` removes them.
- Test nodither2: that compare replaced by `v_cmpx_f_f32` (kill none) at 1,968 sites in 1,860 pixel
  shaders (every one with the noise constant 0x4253ee82 followed by that compare): **the fence and bush no
  longer dip** (user, own save). Water and other effects use the same noise and broke (water's top layer
  drawn everywhere): the patch must be limited to object shaders. (A first try with `v_cmpx_tru_f32`
  killed every pixel: objects gone.)

So the objects that vanish or ghost are drawn, and dissolved by their dissolve value. Not yet known: why
those values are wrong here (what the title's CPU or the mesh shaders compute them from).

Built in (27 Nov, 23:35): `crates/victory-gpu/src/builtin.rs` + `patches/builtin.txt` (1,301 `dissolve` sites,
9 `occluders` sites), applied through the shader cache (the change is part of the entry's key), each only
where the title's dword matches. `VICTORY_DISSOLVE=0` / `VICTORY_CPU_OCCLUDERS=1` turn them off. Run nohiz
logged both groups loaded and no mismatch.

Objects that pop fully in and out (not fading) remain. Each rejected with the dissolve fix on, user on
their street (pop2-pop5, nohiz):
| Run | Change | Result |
| --- | --- | --- |
| pop2 | distance fade gate off in 23 culling shaders | purple bush / fan palm still gone at middle distance |
| pop3 | stand-in band off (the 5 tree shaders, the only ones with level 999) | still gone |
| pop4 | the culling visibility flag forced on in the 7 main BatchCull shaders | **bush and palm back**; textures broken (everything drawn); other objects still pop |
| pop5 | depth pyramid level 0 written as far (both 3x3 init builders) | still pops |
| nohiz | the HiZ comparison itself (`v_cmp_ge_f32` after the pyramid samples) passed in 18 instance-culling shaders (29 sites) | still pops |
So for the tree-shader objects the flag that hides them is not the occlusion test; what is left in it are
the per-view plane tests and the per-instance flags combined with them (55679a05 0x2700-0x27a4). Other
objects that pop are not covered by that flag at all. Not done: which part of the flag, and what removes the rest.
