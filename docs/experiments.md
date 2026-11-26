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
