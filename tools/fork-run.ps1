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
  pwsh tools\fork-run.ps1 -GameExe 'D:\games\title\title.exe' -NoBuild -Env @{ VICTORY_RELAY = '0' }
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
    [hashtable]$Env = @{},
    # Short name for this run, used in the folder name.
    [string]$Label = 'run'
)
$ErrorActionPreference = 'Stop'
$repo = (git rev-parse --show-toplevel).Trim()
$outFull = [System.IO.Path]::GetFullPath($OutDir)
if ($outFull.StartsWith([System.IO.Path]::GetFullPath($repo), [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "OutDir $outFull is inside the repository; choose a folder outside it (docs/asset-policy.md)"
}
if (-not (Test-Path -LiteralPath $GameExe)) { throw "no file at $GameExe" }

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
    rustc        = (rustc -V)
    gpu          = $gpu
    cpu          = $cpu
    os           = $os
    disks        = $disk
    game_exe     = $GameExe
    seconds      = $Seconds
    env          = $Env
    victory_env  = (Get-ChildItem env: | Where-Object Name -like 'VICTORY_*' | ForEach-Object { "$($_.Name)=$($_.Value)" })
}
$facts | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $run 'run.json')
Write-Host "run folder: $run"
Write-Host ("GPU: " + (($gpu | ForEach-Object { "$($_.Name) driver $($_.DriverVersion)" }) -join '; '))

# --- Build (release: the only profile performance is reported from).
if (-not $NoBuild) {
    cargo build --release -p victory --locked
    if ($LASTEXITCODE -ne 0) { throw "release build failed" }
}
if ($Tests) {
    cargo test --workspace --locked *> (Join-Path $run 'tests.log')
    Write-Host "tests exit code $LASTEXITCODE (log: tests.log)"
}
$exe = Join-Path $repo 'target\release\victory.exe'
# Keep this exact build next to its results, so a good one survives later experiments.
$kept = Join-Path $outFull "builds\$short"
New-Item -ItemType Directory -Force -Path $kept | Out-Null
Copy-Item -Force $exe $kept
(Get-FileHash -Algorithm SHA256 $exe).Hash | Set-Content (Join-Path $run 'victory.exe.sha256')

& $exe config *> (Join-Path $run 'config.txt')

# --- The run.
# Switches left over in this shell that press keys or save frames would make
# the run unlike normal play: clear them unless this run asks for them with
# -Env (the same list as scripts\play.ps1, from Zettrox/ztrx).
$cleared = @()
foreach ($v in 'VICTORY_KEYS', 'VICTORY_CAPTURE', 'VICTORY_CAPTURE_FRAMES', 'VICTORY_DUMP_TARGETS') {
    if (-not $Env.ContainsKey($v) -and (Test-Path "env:$v")) { Remove-Item "env:$v"; $cleared += $v }
}
if ($cleared) { Write-Host "cleared for this run: $($cleared -join ', ')" }
$facts['cleared_env'] = $cleared
foreach ($k in $Env.Keys) { Set-Item -Path "env:$k" -Value $Env[$k] }
if (-not $env:VICTORY_LOG) { $env:VICTORY_LOG = 'info' }
$log = Join-Path $run 'victory.log'
$err = Join-Path $run 'victory.err.log'
$p = Start-Process -FilePath (Join-Path $kept 'victory.exe') -ArgumentList @('run', "`"$GameExe`"", '--execute') `
    -RedirectStandardOutput $log -RedirectStandardError $err -PassThru -NoNewWindow
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
$data = if ($env:VICTORY_DATA_DIR) { $env:VICTORY_DATA_DIR } else { Join-Path $env:LOCALAPPDATA 'victory\data' }
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
