<#
.SYNOPSIS
  First-time setup of the VIctory fork on the user's Windows PC: tools, checkout, config, build.
  Written in a cloud session without Windows: NOT yet run. Its first run is also its test.

.DESCRIPTION
  powershell -ExecutionPolicy Bypass -File setup-pc.ps1 -Root C:\dev -GameDir 'D:\path\to\game'

  1. Reports what is installed: winget, Git, rustup, MSVC build tools (vswhere), GPU driver.
  2. Installs what is missing with winget (each install asks for approval; -NoInstall only reports).
  3. Clones https://github.com/thomb2123/victory-fork into <Root>\victory-fork (remote `upstream` =
     fxceless/victory-fork), with the `notes` branch as a worktree in <Root>\victory-notes.
  4. Writes victory.toml with game_dir = -GameDir. The game folder is only read; it must be outside the checkout.
  5. Installs the toolchain pinned in rust-toolchain.toml, then builds release and diag on -Branch.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Root,
    [string]$GameDir,
    [string]$Branch = 'integration',
    [switch]$NoInstall
)
$ErrorActionPreference = 'Stop'
function Has([string]$cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
function Section([string]$n) { Write-Host "`n=== $n ===" -ForegroundColor Cyan }
function Refresh-Path { $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + "$env:USERPROFILE\.cargo\bin" }

Section 'what is installed'
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$msvc = if (Test-Path $vswhere) { & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath } else { $null }
$state = [ordered]@{
    winget = Has winget
    git    = Has git
    rustup = (Has rustup) -or (Test-Path "$env:USERPROFILE\.cargo\bin\rustup.exe")
    msvc   = [bool]$msvc
}
$state.GetEnumerator() | ForEach-Object { '{0,-7} {1}' -f $_.Key, $(if ($_.Value) { 'yes' } else { 'MISSING' }) }
Get-CimInstance Win32_VideoController | ForEach-Object { "gpu     $($_.Name) driver $($_.DriverVersion)" }

if (-not $NoInstall) {
    if (-not $state.winget -and (-not $state.git -or -not $state.rustup -or -not $state.msvc)) { throw 'winget is missing: install "App Installer" from the Microsoft Store, then rerun' }
    if (-not $state.git) { Section 'install Git'; winget install --id Git.Git -e --source winget --accept-package-agreements --accept-source-agreements }
    if (-not $state.rustup) { Section 'install rustup'; winget install --id Rustlang.Rustup -e --source winget --accept-package-agreements --accept-source-agreements }
    if (-not $state.msvc) {
        Section 'install Visual Studio Build Tools (C++ workload; several GB)'
        winget install --id Microsoft.VisualStudio.2022.BuildTools -e --source winget --accept-package-agreements --accept-source-agreements `
            --override '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
    }
    Refresh-Path
}

Section 'checkout'
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$code = Join-Path $Root 'victory-fork'
$notes = Join-Path $Root 'victory-notes'
if (-not (Test-Path (Join-Path $code '.git'))) {
    git clone https://github.com/thomb2123/victory-fork $code
    git -C $code remote add upstream https://github.com/fxceless/victory-fork
}
git -C $code fetch origin
git -C $code fetch upstream main
git -C $code checkout $Branch
if (-not (Test-Path $notes)) { git -C $code worktree add $notes notes }
"code  $code @ $((git -C $code rev-parse --short HEAD).Trim()) ($Branch)"
"notes $notes"

if ($GameDir) {
    Section 'config'
    $game = (Resolve-Path $GameDir).Path
    if ($game.StartsWith((Resolve-Path $code).Path, [StringComparison]::OrdinalIgnoreCase)) { throw 'the game folder must be outside the checkout' }
    "[paths]`ngame_dir = '$game'" | Set-Content -Encoding utf8 (Join-Path $code 'victory.toml')
    "victory.toml: game_dir = $game (only read)"
}

Section 'toolchain and builds'
Push-Location $code
try {
    rustup show active-toolchain   # installs the version pinned in rust-toolchain.toml
    cargo build --release -p victory --locked
    if ($LASTEXITCODE) { throw 'release build failed' }
    cargo build --profile diag -p victory --features diagnostics --locked
    if ($LASTEXITCODE) { throw 'diag build failed' }
    if ($GameDir) { .\target\release\victory.exe config }
} finally { Pop-Location }
Write-Host "`nSetup done. Next: $notes\tools\baseline.ps1 (see PROGRESS.md, 'How to run the baseline')." -ForegroundColor Green
