<#
.SYNOPSIS
  One reproducible baseline of the VIctory fork on this PC: hardware and tools, the commit, the tests, the builds,
  and one logged run of the game. Everything is written OUTSIDE the repository (asset policy); a short summary.txt
  is what you send back.

.DESCRIPTION
  Run from a checkout of fxceless/victory-fork in PowerShell 5.1 or 7:

    powershell -ExecutionPolicy Bypass -File path\to\baseline.ps1 -GameExe 'D:\victory-data\game\<the title's exe>'

  Steps (each can be skipped):
    1. hardware.txt   CPU, GPU and driver, memory, page file, OS, disk, tool versions
    2. tests.txt      cargo test --workspace --locked              (-SkipTests)
    3. builds         cargo build --release, and the diagnostics build (target\diag\victory.exe)
    4. translate.txt  victory translate --pipelines over the game   (only with -Translate; long)
    5. run.log        target\diag\victory.exe run <exe> --execute, stderr logged; you play, then close the game
                      (-SkipRun). Meanwhile process CPU/memory and, on NVIDIA, GPU load are sampled every 5 s.
    6. summary.txt    extracted counters only: fps and ms/frame lines, warnings grouped by kind, builder lines,
                      new crashes.log entries, test results. Read it before sending: it can contain file paths.

  Never send run.log, captures or anything from the game folder; they stay on your PC.

.PARAMETER Switches
  Extra VICTORY_* switches for the run, e.g. @{ VICTORY_RELAY = '2' }. The same switches must be used when a run is
  repeated to measure a change.
#>
[CmdletBinding()]
param(
    [string]$GameExe,
    [string]$Repo = (Get-Location).Path,
    [string]$OutDir = (Join-Path $env:LOCALAPPDATA ("victory-baseline\" + (Get-Date -Format 'yyyyMMdd-HHmmss'))),
    [hashtable]$Switches = @{},
    [string]$Log = 'info',
    [switch]$SkipTests,
    [switch]$SkipBuild,
    [switch]$SkipRun,
    [switch]$Translate,
    [switch]$Capture,
    [int]$SampleSeconds = 5
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3

function Section([string]$name) { Write-Host "`n=== $name ===" -ForegroundColor Cyan }
$Esc = [string][char]27
function Strip-Ansi([string]$s) { return ($s -replace "$Esc\[[0-9;]*[A-Za-z]", '') }
function Try-Run([scriptblock]$block) { try { & $block 2>&1 | Out-String } catch { "unavailable: $($_.Exception.Message)" } }

$Repo = (Resolve-Path $Repo).Path
if (-not (Test-Path (Join-Path $Repo '.victory-root'))) { throw "$Repo is not a VIctory checkout (no .victory-root)" }
$OutFull = [System.IO.Path]::GetFullPath($OutDir)
if ($OutFull.StartsWith($Repo, [System.StringComparison]::OrdinalIgnoreCase)) { throw "OutDir must be outside the repository" }
New-Item -ItemType Directory -Force -Path $OutFull | Out-Null
$summary = New-Object System.Collections.Generic.List[string]
function S([string]$line) { $summary.Add($line); Write-Host $line }
Write-Host "Output: $OutFull"

# ---------------------------------------------------------------- 1. hardware and tools
Section 'hardware and tools'
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$cs = Get-CimInstance Win32_ComputerSystem
$os = Get-CimInstance Win32_OperatingSystem
$gpus = Get-CimInstance Win32_VideoController
$pf = Get-CimInstance Win32_PageFileUsage -ErrorAction SilentlyContinue
$drive = Get-PSDrive -Name ($Repo.Substring(0, 1))
$nvsmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
$hw = @(
    "cpu        $($cpu.Name.Trim()) ($($cpu.NumberOfCores) cores, $($cpu.NumberOfLogicalProcessors) threads)"
    "memory     $([math]::Round($cs.TotalPhysicalMemory / 1GB, 1)) GB"
    "page file  " + $(if ($pf) { ($pf | ForEach-Object { "$($_.Name) $($_.AllocatedBaseSize) MB" }) -join ', ' } else { 'system managed / unknown' })
    "os         $($os.Caption) $($os.Version) build $($os.BuildNumber)"
    "disk       $($drive.Name): $([math]::Round($drive.Free / 1GB)) GB free"
)
foreach ($g in $gpus) { $hw += "gpu        $($g.Name) driver $($g.DriverVersion)" }
if ($nvsmi) { $hw += 'nvidia-smi ' + ((& nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader) -join '; ') }
$hw += 'rustc      ' + (Try-Run { rustc --version }).Trim()
$hw += 'cargo      ' + (Try-Run { cargo --version }).Trim()
$hw += 'git        ' + (Try-Run { git --version }).Trim()
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
$hw += 'win sdk    ' + $(if ($sdk) { "$($sdk.Name)" + $(if (Test-Path "$($sdk.FullName)\x64\dxil.dll") { ' (dxil.dll present)' } else { ' (no dxil.dll)' }) } else { 'none found (translate --validate needs one)' })
$hw | Set-Content (Join-Path $OutFull 'hardware.txt')
$hw | ForEach-Object { S $_ }

Push-Location $Repo
try {
    $commit = (git rev-parse HEAD).Trim()
    $dirty = (git status --porcelain) | Where-Object { $_ }
    S "commit     $commit$(if ($dirty) { ' (with local changes: ' + (($dirty | ForEach-Object { $_.Substring(3) }) -join ', ') + ')' })"
    $switchText = if ($Switches.Count) { ($Switches.GetEnumerator() | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ' ' } else { '(defaults)' }
    S "run env    $switchText"

    # ------------------------------------------------------------ 2. tests
    if (-not $SkipTests) {
        Section 'tests (cargo test --workspace --locked)'
        $testOut = Join-Path $OutFull 'tests.txt'
        $sw = [Diagnostics.Stopwatch]::StartNew()
        cmd /c "cargo test --workspace --locked --no-fail-fast > `"$testOut`" 2>&1"
        $rc = $LASTEXITCODE
        $lines = Get-Content $testOut | ForEach-Object { Strip-Ansi $_ }
        $pass = 0; $fail = 0; $ign = 0
        foreach ($l in $lines) {
            if ($l -match '^test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored') { $pass += [int]$Matches[1]; $fail += [int]$Matches[2]; $ign += [int]$Matches[3] }
        }
        S "tests      $pass passed, $fail failed, $ign ignored (exit $rc, $([int]$sw.Elapsed.TotalSeconds) s)"
        $failed = $lines | Where-Object { $_ -match '^test (\S+) \.\.\. FAILED' } | ForEach-Object { ($_ -split ' ')[1] }
        foreach ($f in $failed) { S "  FAILED   $f" }
        $skips = $lines | Where-Object { $_ -match 'skip' -and $_ -match 'card|device|GPU|SM 6' } | Select-Object -Unique -First 5
        foreach ($k in $skips) { S "  note     $k" }
    }

    # ------------------------------------------------------------ 3. builds
    if (-not $SkipBuild) {
        Section 'builds'
        cargo build --release -p victory --locked
        if ($LASTEXITCODE) { throw 'release build failed' }
        cargo build --profile diag -p victory --features diagnostics --locked
        if ($LASTEXITCODE) { throw 'diagnostics build failed' }
        S 'builds     release and diag: ok'
    }
    $victory = Join-Path $Repo 'target\diag\victory.exe'

    # ------------------------------------------------------------ 4. offline translation (optional)
    if ($Translate) {
        Section 'victory translate --pipelines (all shaders found under the configured game dir)'
        $tr = Join-Path $OutFull 'translate.txt'
        cmd /c "`"$victory`" translate --pipelines > `"$tr`" 2>&1"
        $tl = Get-Content $tr | ForEach-Object { Strip-Ansi $_ }
        S 'translate  (closing summary)'
        $start = -1
        for ($i = 0; $i -lt $tl.Count; $i++) { if ($tl[$i] -match '^programs\s') { $start = $i; break } }
        if ($start -ge 0) { $tl[$start..($tl.Count - 1)] | Select-Object -First 60 | ForEach-Object { S "  $_" } } else { $tl | Select-Object -Last 40 | ForEach-Object { S "  $_" } }
    }

    # ------------------------------------------------------------ 5. the run
    if (-not $SkipRun) {
        if (-not $GameExe) { throw 'give -GameExe (or -SkipRun)' }
        if (-not (Test-Path $GameExe)) { throw "no such file: $GameExe" }
        $exeFull = (Resolve-Path $GameExe).Path
        if ($exeFull.StartsWith($Repo, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'the game must be outside the repository' }
        Section 'run'
        # crashes.log: remember where each one ends, to report only this run's entries
        $crashLogs = @{}
        $roots = @((Join-Path $env:LOCALAPPDATA 'victory'), $env:VICTORY_DATA_DIR, $env:VICTORY_CACHE_DIR) | Where-Object { $_ -and (Test-Path $_) }
        foreach ($r in $roots) { Get-ChildItem $r -Recurse -Filter crashes.log -ErrorAction SilentlyContinue | ForEach-Object { $crashLogs[$_.FullName] = $_.Length } }

        $runLog = Join-Path $OutFull 'run.log'
        $saved = @{}
        $runEnv = @{ VICTORY_LOG = $Log; NO_COLOR = '1' }
        foreach ($k in $Switches.Keys) { $runEnv[$k] = $Switches[$k] }
        if ($Capture) { $runEnv['VICTORY_CAPTURE'] = (Join-Path $OutFull 'frames') }
        foreach ($k in $runEnv.Keys) { $saved[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, [string]$runEnv[$k]) }
        Write-Host 'The game starts now. Go to the scene under test, stay there about a minute, then close the game window.' -ForegroundColor Yellow
        $gpuSamples = $null
        $gpuCsv = $null
        if ($nvsmi) {
            $gpuCsv = Join-Path $OutFull 'gpu.csv'
            $gpuSamples = Start-Process nvidia-smi -ArgumentList "--query-gpu=timestamp,utilization.gpu,memory.used,clocks.gr --format=csv -l $SampleSeconds -f `"$gpuCsv`"" -WindowStyle Hidden -PassThru
        }
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $p = Start-Process -FilePath $victory -ArgumentList @('run', "`"$exeFull`"", '--execute') -RedirectStandardError $runLog -RedirectStandardOutput (Join-Path $OutFull 'run.stdout.txt') -NoNewWindow -PassThru
        $null = $p.Handle   # keeps the exit code readable after exit
        $samples = New-Object System.Collections.Generic.List[string]
        $samples.Add('seconds,cpu_seconds,working_set_mb,private_mb,threads')
        while (-not $p.HasExited) {
            Start-Sleep -Seconds $SampleSeconds
            try { $p.Refresh(); $samples.Add(('{0},{1:F1},{2},{3},{4}' -f [int]$sw.Elapsed.TotalSeconds, $p.TotalProcessorTime.TotalSeconds, [int]($p.WorkingSet64 / 1MB), [int]($p.PrivateMemorySize64 / 1MB), $p.Threads.Count)) } catch { }
        }
        $p.WaitForExit()
        if ($gpuSamples -and -not $gpuSamples.HasExited) { Stop-Process -Id $gpuSamples.Id -ErrorAction SilentlyContinue }
        $samples | Set-Content (Join-Path $OutFull 'process.csv')
        foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
        S "run        exit code $($p.ExitCode) after $([int]$sw.Elapsed.TotalSeconds) s"
        if ($samples.Count -gt 1) {
            $peak = ($samples | Select-Object -Skip 1 | ForEach-Object { [int](($_ -split ',')[3]) } | Measure-Object -Maximum).Maximum
            S "  peak private memory $peak MB"
        }
        if ($gpuSamples -and (Test-Path $gpuCsv)) {
            $util = Get-Content $gpuCsv | Select-Object -Skip 1 | ForEach-Object { $f = $_ -split ','; if ($f.Count -gt 1 -and $f[1] -match '\d') { [int]($f[1] -replace '[^0-9]', '') } }
            if ($util) { $m = $util | Measure-Object -Average -Maximum; S ("  GPU load: average {0:F0} %, max {1} % ({2} samples)" -f $m.Average, $m.Maximum, $m.Count) }
        }

        # ---------------------------------------------------------- 6. summary from the log
        $log = Get-Content $runLog | ForEach-Object { Strip-Ansi $_ }
        foreach ($pat in @('host wave lanes', 'display mode reported', 'frame interval', 'host pipeline wait budget')) {
            $hit = $log | Where-Object { $_ -match $pat } | Select-Object -First 1
            if ($hit) { S "  $($hit -replace '^\S+\s+', '')" }
        }
        # the fps lines: fps, ms/frame, preparing (CPU), on the GPU, draws, dropped
        $rx = '([\d.]+) fps, ([\d.]+) ms/frame \(([\d.]+) preparing.*?; ([\d.]+) on the GPU.*?\), (\d+) draws/frame, dropped (\d+) without a host pipeline \((\d+) being made\)'
        $rows = foreach ($l in $log) { if ($l -match $rx) { [pscustomobject]@{ fps = [double]$Matches[1]; ms = [double]$Matches[2]; cpu = [double]$Matches[3]; gpu = [double]$Matches[4]; draws = [int]$Matches[5]; dropped = [int]$Matches[6]; making = [int]$Matches[7] } } }
        if ($rows) {
            $rows | Export-Csv (Join-Path $OutFull 'frames.csv') -NoTypeInformation
            S "  frame lines: $(@($rows).Count) (about one per 5 s); the last 12 (the scene you ended in):"
            S '      fps   ms/frame  preparing  on GPU   draws  dropped  being made'
            foreach ($r in ($rows | Select-Object -Last 12)) { S ('    {0,5:F1} {1,9:F1} {2,9:F1} {3,8:F1} {4,7} {5,8} {6,10}' -f $r.fps, $r.ms, $r.cpu, $r.gpu, $r.draws, $r.dropped, $r.making) }
            $tail = @($rows | Select-Object -Last 12 | ForEach-Object { $_.ms } | Sort-Object)
            S ('    median of those: {0:F1} ms/frame' -f $tail[[int][math]::Floor($tail.Count / 2)])
        } else {
            S '  no fps lines in the log (the title presented no frames, or the line format changed)'
        }
        # diagnostics build: what the texture refresh is made of (watched writes vs hashed changes), last 6 lines
        $tex = $log | Where-Object { $_ -match 'copies per frame:' } | Select-Object -Last 6
        if ($tex) {
            S '  copies per frame, texture part (last 6 lines: watched and changed / hashed / of them changed):'
            foreach ($t in $tex) { if ($t -match 'textures ([\d.]+) MB watched and\s+changed, ([\d.]+) MB hashed, ([\d.]+) MB of them changed') { S ('    watched+changed {0} MB, hashed {1} MB, changed {2} MB' -f $Matches[1], $Matches[2], $Matches[3]) } }
        }
        $builder = $log | Where-Object { $_ -match 'structure builder' } | Select-Object -First 40
        foreach ($b in $builder) { S "  $($b -replace '^\S+\s+', '')" }
        # warnings and errors grouped by message, numbers and hex blanked so repeats collapse
        foreach ($level in @('ERROR', 'WARN')) {
            $groups = $log | Where-Object { $_ -match "^\S*\s*$level\s" } |
                ForEach-Object { ($_ -replace '^\S+\s+\w+\s+', '') -replace '0x[0-9a-fA-F]+', '0x_' -replace '\b\d+(\.\d+)?\b', '#' } |
                Group-Object | Sort-Object Count -Descending | Select-Object -First 25
            if ($groups) {
                S "  $level messages (count, message with numbers blanked):"
                foreach ($g in $groups) { $t = $g.Name; if ($t.Length -gt 220) { $t = $t.Substring(0, 220) + '...' }; S ('    {0,6}  {1}' -f $g.Count, $t) }
            }
        }
        foreach ($r in $roots) {
            Get-ChildItem $r -Recurse -Filter crashes.log -ErrorAction SilentlyContinue | ForEach-Object {
                $before = if ($crashLogs.ContainsKey($_.FullName)) { $crashLogs[$_.FullName] } else { 0 }
                if ($_.Length -gt $before) {
                    $fs = [System.IO.File]::OpenRead($_.FullName); $fs.Seek($before, 'Begin') | Out-Null
                    $new = (New-Object System.IO.StreamReader($fs)).ReadToEnd(); $fs.Close()
                    S "  crashes.log, new this run:"
                    ($new -split "`r?`n" | Select-Object -First 60) | ForEach-Object { S "    $_" }
                }
            }
        }
    }
} finally {
    Pop-Location
}

$summary | Set-Content (Join-Path $OutFull 'summary.txt')
Write-Host "`nDone. Send back: $(Join-Path $OutFull 'summary.txt') (read it first). Keep run.log and the rest on this PC." -ForegroundColor Green
