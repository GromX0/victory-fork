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
