# Provisional base: pinned manifest (Stage 0)

Pinned 2026-11-26. The working base is conditional on Stage 1 of the
console-parity roadmap (`Victory_Console_Parity_Roadmap_2026-11-26.md`).

## Binary

| Item | Value |
| --- | --- |
| Branch | `base/avatar-rdna4` (local; D:\dev\wt-base) |
| Code commit | `b7fb278a910618b456fcb4ca0cf94b4c23dc3d15` (docs-only commits after it do not change the binary) |
| Upstream pins | avatarmanz/VIctory-7900XTX `2de99801`; Belobug/VIctory-Belobug-AMD rx9070xt `81ebb7bdc9b7ca6ec2f3169973f50be77c7c803c` (cherry-picked, magenta filter in ambient.rs left out) |
| Build | `cargo build --locked --profile diag -p victory --features diagnostics`, toolchain 1.98.1 (rust-toolchain.toml) |
| Executable | `target\diag\victory.exe`, SHA-256 `11A7DFC4F2E84DCF9AABB4FE94FA52B9947BD2DF7FBD564135802223C974E716`; kept copy `D:\victory-runs\builds\b7fb278a91-diag\` |
| Working tree | clean apart from git-ignored `victory.toml`; `tools\fork-run.ps1` (the harness of GromX0 int/harvest-test de15fee7) committed later, docs and tools only |
| Game | `<game_dir>\<title>.exe`, SHA-256 prefix `E95020B2FA4A721D`, title 7952987A, version 930.048.020-vb_sub_story |

## Configuration

- `victory.toml`: game_dir `<game_dir>`, cache_dir `D:\victory-data\avatar\cache`
  (shared with the avatar checkout: same translator), data_dir
  `D:\victory-data\avatar\data` (a copy of GromX0's save data made 26 Nov).
- Switch files in `D:\victory-runs\envs\`: `base-play.env` (AvatarManz
  play.ps1 defaults, not -Safe, + VICTORY_SKIP_PIPELINES=39f0e9aeb156a61c),
  `base-rt.env` (+ VICTORY_BVH_BUILDER=1). The effective switches of each
  run are in its `run.json` (`effective_env`).
- Harness: `tools\fork-run.ps1 -NoBuild -Diag -Console series-x -Keys start
  -EnvFile <file> -CommitGrowLimitGB 70 -Seconds 420`.
- Cache state: warm = after one unmeasured warm-up run on this cache;
  `pipelines\warm-bad.txt` lists the pipelines the AMD compiler crashed on
  (37 when pinned), which later runs leave out. Cold = a cache with the
  translated shaders but no `pipelines` folder (D:\victory-data\combined-cold).

## Machine

Windows 11 Pro 10.0.26200; AMD Ryzen 7 9800X3D (8 cores / 16 threads);
31.1 GB visible RAM; AMD Radeon RX 9070 16 GB, driver 32.0.32029.2010
(2026-11-18); BIOS AMI 4.03; display 2560x1440 at 240 Hz; page file C:
32-64 GB (set 26 Nov; Windows restarted 26 Nov 17:16). Console profile
Series X (4K output drawn at 1440p), no FSR, no frame generation; the
window presents at 1920x1080.

## Baseline (warm, Series X, opening and cell route, 420 s)

| Run | Builder | Median of 5-s frame lines | p90 of lines | Draws dropped/frame | Pipelines left out |
| --- | --- | --- | --- | --- | --- |
| base-measure | off | 46.1 ms | 70.5 ms | 381 + 67 indirect | 1 skipped + warm-bad list |
| base-rt | on | 86.2 ms | 282.6 ms | 1 | 1 skipped + warm-bad list |

Figures are periodic 5-s averages from the log, not per-frame samples.
Commit figures in run.json are the system's commit (GlobalMemoryStatusEx),
not the process's.
