<#
.SYNOPSIS
  One recorded run of this fork: build, note the machine and the build, keep
  a copy of the executable, run the title, and summarise the frame lines.

.DESCRIPTION
  Answers "what did this exact build do on this machine" without retyping
  the same steps. Everything it writes goes under -OutDir (default
  %LOCALAPPDATA%\victory\runs), never into the repository, and it never
  touches the game folder: it only passes the title's path to victory.exe.

  Untested as written: it was prepared on a Linux machine without
  PowerShell. Report the first error it prints and it will be fixed.

.EXAMPLE
  # Build, run 5 minutes, kill, summarise:
  pwsh tools\fork-run.ps1 -GameExe 'D:\games\title\title.exe' -Seconds 300

.EXAMPLE
  # Same build again, no rebuild, with extra switches:
  pwsh tools\fork-run.ps1 -GameExe 'D:\games\title\title.exe' -NoBuild -SetEnv @{ VICTORY_RELAY = '0' }
#>
param(
    [Parameter(Mandatory = $true)][string]$GameExe,
    [string]$OutDir = (Join-Path $env:LOCALAPPDATA 'victory\runs'),
    # Seconds before the run is stopped (0: until the window is closed).
    [int]$Seconds = 0,
    [switch]$NoBuild,
    # Also run `cargo test --workspace` (the GPU shader tests need this machine).
    [switch]$Tests,
    # Extra environment variables for this run only, e.g. @{ VICTORY_LATE_WAIT = '3' }.
    [hashtable]$SetEnv = @{},
    # Short name for this run, used in the folder name.
    [string]$Label = 'run',
    # 'amd': the stability settings ezbyDev/victory-amd reports for an RX 9060 XT
    # (RDNA4), limited to switches that mean the same in this branch (checked in
    # the code): relay off (on by default here since fxceless c96b4f2, and said to
    # remove the device on AMD), submissions sent ahead (LATE_WAIT=3), clean-up
    # passes 4, 16 and 32 off (SHADER_OPT=11, victory-amd's tested value; 59 would
    # turn off pass 4 alone), pipeline workers at normal priority.
    [ValidateSet('default', 'amd')][string]$Profile = 'default',
    # With -Profile amd: also skip the five compute programs victory-amd reports
    # crashing, page-faulting or hanging AMD's shader compiler. Removes their
    # draws and dispatches: use only once a run has shown that failure.
    [switch]$AmdSkips,
    # The diagnostics build (target\diag, --features diagnostics): needed for
    # VICTORY_SHADER_LOOP_LIMIT, captures and scripted keys. Not for timing.
    [switch]$Diag,
    # The console the title is told it runs on. 'series-s' renders at the
    # smaller console's resolution: KLBF Lab measured ~7.3 fps there against
    # ~5 in Series X mode on an RX 9070 XT.
    [ValidateSet('series-x', 'series-s')][string]$Console = 'series-x',
    # With -Diag: save presented frames (every power of two, plus -CaptureFrames)
    # as PNGs in this folder, outside the repository. -Targets also saves every
    # render target of those frames. (VICTORY_CAPTURE set in the shell is
    # cleared like the other test switches; this is the way to ask for it.)
    [string]$Capture = '',
    [string]$CaptureFrames = '',
    [switch]$Targets
)
$ErrorActionPreference = 'Stop'
$repo = (git -C $PSScriptRoot rev-parse --show-toplevel).Trim()
# cargo works on the current folder: build and test this checkout wherever the script is called from.
Set-Location -LiteralPath $repo
$outFull = [System.IO.Path]::GetFullPath($OutDir)
if ($outFull.StartsWith([System.IO.Path]::GetFullPath($repo), [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "OutDir $outFull is inside the repository; choose a folder outside it (docs/asset-policy.md)"
}
if (-not (Test-Path -LiteralPath $GameExe)) { throw "no file at $GameExe" }
if ($Profile -eq 'amd') {
    $amd = @{ VICTORY_RELAY = '0'; VICTORY_LATE_WAIT = '3'; VICTORY_SHADER_OPT = '11'; VICTORY_PIPELINE_PRIORITY = 'normal' }
    if ($Diag) { $amd['VICTORY_SHADER_LOOP_LIMIT'] = '1024' }
    if ($AmdSkips) { $amd['VICTORY_SKIP_PIPELINES'] = 'c1eee35dbc50a9ef,f16876929fa64f9f,eeed82b877b44b5f,02384a60fda6f2d0,39f0e9aeb156a61c' }
    foreach ($k in $amd.Keys) { if (-not $SetEnv.ContainsKey($k)) { $SetEnv[$k] = $amd[$k] } }
}
if ($Capture) {
    if (-not $Diag) { throw "-Capture needs -Diag: only the diagnostics build saves frames" }
    $capFull = [System.IO.Path]::GetFullPath($Capture)
    if ($capFull.StartsWith([System.IO.Path]::GetFullPath($repo), [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Capture folder $capFull is inside the repository; frames hold game images (docs/asset-policy.md)"
    }
    New-Item -ItemType Directory -Force -Path $capFull | Out-Null
    $SetEnv['VICTORY_CAPTURE'] = $capFull
    if ($CaptureFrames) { $SetEnv['VICTORY_CAPTURE_FRAMES'] = $CaptureFrames }
    if ($Targets) { $SetEnv['VICTORY_DUMP_TARGETS'] = '1' }
}

$commit = (git -C $repo rev-parse HEAD).Trim()
$short = $commit.Substring(0, 10)
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$run = Join-Path $outFull "$stamp-$Label-$short"
New-Item -ItemType Directory -Force -Path $run | Out-Null

# --- The machine and the build, as detected now (not as remembered).
$gpu = Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion, DriverDate, AdapterRAM
$cpu = Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors
$os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, TotalVisibleMemorySize, FreePhysicalMemory
$disk = Get-PSDrive -PSProvider FileSystem | Select-Object Name, @{n = 'FreeGB'; e = { [math]::Round($_.Free / 1GB, 1) } }
$dirty = (git -C $repo status --porcelain) -join "`n"
$facts = [ordered]@{
    commit       = $commit
    branch       = (git -C $repo rev-parse --abbrev-ref HEAD).Trim()
    dirty        = $dirty
    # Not needed with -NoBuild, so a missing rustc must not stop the run.
    rustc        = $(if (Get-Command rustc -ErrorAction SilentlyContinue) { rustc -V } else { 'not on PATH' })
    gpu          = $gpu
    cpu          = $cpu
    os           = $os
    disks        = $disk
    game_exe     = $GameExe
    seconds      = $Seconds
    env          = $SetEnv
    profile      = $Profile
    console      = $Console
    build        = $(if ($Diag) { 'diag' } else { 'release' })
    victory_env  = (Get-ChildItem env: | Where-Object Name -like 'VICTORY_*' | ForEach-Object { "$($_.Name)=$($_.Value)" })
}
$facts | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $run 'run.json')
Write-Host "run folder: $run"
Write-Host ("GPU: " + (($gpu | ForEach-Object { "$($_.Name) driver $($_.DriverVersion)" }) -join '; '))

# --- Build (release: the only profile performance is reported from).
if (-not $NoBuild) {
    if ($Diag) { cargo build --profile diag -p victory --features diagnostics --locked }
    else { cargo build --release -p victory --locked }
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
}
# Runs a native command with stdout and stderr in one file. Windows PowerShell
# 5.1 turns each redirected stderr line of a native command into an error
# record, which stops the script under $ErrorActionPreference = 'Stop' (cargo
# writes its progress to stderr), so the redirect is left to cmd.exe.
function Invoke-Logged([string]$Command, [string]$LogFile) {
    cmd /c "$Command > `"$LogFile`" 2>&1"
    return $LASTEXITCODE
}
if ($Tests) {
    $code = Invoke-Logged 'cargo test --workspace --locked' (Join-Path $run 'tests.log')
    Write-Host "tests exit code $code (log: tests.log)"
}
$exe = Join-Path $repo $(if ($Diag) { 'target\diag\victory.exe' } else { 'target\release\victory.exe' })
# Keep this exact build next to its results, so a good one survives later experiments.
$kept = Join-Path $outFull ("builds\$short" + $(if ($Diag) { '-diag' } else { '' }))
New-Item -ItemType Directory -Force -Path $kept | Out-Null
Copy-Item -Force $exe $kept
(Get-FileHash -Algorithm SHA256 $exe).Hash | Set-Content (Join-Path $run 'victory.exe.sha256')

$null = Invoke-Logged "`"$exe`" config" (Join-Path $run 'config.txt')

# --- The run.
# Switches left over in this shell that press keys or save frames would make
# the run unlike normal play: clear them unless this run asks for them with
# -SetEnv (the same list as scripts\play.ps1, from Zettrox/ztrx).
$cleared = @()
foreach ($v in 'VICTORY_KEYS', 'VICTORY_CAPTURE', 'VICTORY_CAPTURE_FRAMES', 'VICTORY_DUMP_TARGETS') {
    if (-not $SetEnv.ContainsKey($v) -and (Test-Path "env:$v")) { Remove-Item "env:$v"; $cleared += $v }
}
if ($cleared) { Write-Host "cleared for this run: $($cleared -join ', ')" }
$facts['cleared_env'] = $cleared
foreach ($k in $SetEnv.Keys) { Set-Item -Path "env:$k" -Value $SetEnv[$k] }
if (-not $env:VICTORY_LOG) { $env:VICTORY_LOG = 'info' }
$log = Join-Path $run 'victory.log'
$err = Join-Path $run 'victory.err.log'
$runArgs = @('run', "`"$GameExe`"", '--execute', '--console', $Console)
$p = Start-Process -FilePath (Join-Path $kept 'victory.exe') -ArgumentList $runArgs `
    -RedirectStandardOutput $log -RedirectStandardError $err -PassThru -NoNewWindow
# Without the handle taken now, ExitCode reads empty once the process has exited.
$null = $p.Handle
$started = Get-Date
if ($Seconds -gt 0) {
    if (-not $p.WaitForExit($Seconds * 1000)) { Stop-Process -Id $p.Id -Force; Write-Host "stopped after $Seconds s" }
} else {
    $p.WaitForExit()
}
$facts['exit_code'] = $p.ExitCode
$facts['ran_seconds'] = [int]((Get-Date) - $started).TotalSeconds
$facts | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $run 'run.json')

# --- crashes.log from the data folder (default location; VICTORY_DATA_DIR overrides).
# The folder victory itself reports (`victory config`: environment, then
# victory.toml, then the default), so a data_dir set in victory.toml is seen.
$data = Select-String -Path (Join-Path $run 'config.txt') -Pattern '^data_dir\s*:\s*(.+)$' -ErrorAction SilentlyContinue |
    Select-Object -First 1 | ForEach-Object { $_.Matches[0].Groups[1].Value.Trim() }
if (-not $data) { $data = if ($env:VICTORY_DATA_DIR) { $env:VICTORY_DATA_DIR } else { Join-Path $env:LOCALAPPDATA 'victory\data' } }
Get-ChildItem -Recurse -Filter crashes.log -Path $data -ErrorAction SilentlyContinue |
    Where-Object LastWriteTime -gt $started | ForEach-Object { Copy-Item $_.FullName (Join-Path $run "crashes-$($_.Directory.Name).log") }

# --- Summary: the periodic frame line (fps, ms a frame, CPU/GPU split) and errors.
$all = @(Get-Content $log, $err -ErrorAction SilentlyContinue)
$frames = $all | Select-String -Pattern 'fps' | Select-Object -Last 20
$bad = $all | Select-String -Pattern 'ERROR|panicked|DEVICE_|removed|hung' | Select-Object -First 20
$frames | ForEach-Object { $_.Line } | Set-Content (Join-Path $run 'frames.txt')
Write-Host "`n--- last frame lines"; $frames | ForEach-Object { $_.Line }
Write-Host "`n--- first errors"; $bad | ForEach-Object { $_.Line }
Write-Host "`nexit code $($p.ExitCode); everything is in $run"
Write-Host "Logs can name paths on this machine: read them before attaching them anywhere public."
