<#
.SYNOPSIS
  QSL Agent installer for Windows (PowerShell 5.1+ / 7+) - Quantum Secular Labs
  https://www.quantumsecularlabs.com/qsl-agent

.DESCRIPTION
  Installs qsl-agent.exe plus a qslagent.exe copy so either command works from
  any terminal. Runs elevated -> C:\Program Files\QSL Agent (system-wide, all
  users); otherwise -> %LOCALAPPDATA%\Programs\QSL Agent (current user).
  The install directory is added to PATH.

  irm https://raw.githubusercontent.com/0x0806/qsl_agent/main/install.ps1 | iex

  With options:
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/0x0806/qsl_agent/main/install.ps1))) -Version v0.6.0

  Options: -Version <tag>  -Prefix <dir>  -User  -FromSource  -Uninstall
  Help: Get-Help .\install.ps1 -Full
#>
[CmdletBinding()]
param(
  [string]$Version = $(if ($env:QSL_VERSION) { $env:QSL_VERSION } else { 'latest' }),
  [string]$Prefix  = $env:QSL_PREFIX,
  [switch]$User,
  [switch]$FromSource,
  [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.ServicePointManager]::SecurityProtocol } catch {}

$Repo  = '0x0806/qsl_agent'
$Bin   = 'qsl-agent'
$Alias = 'qslagent'
$BaseUrl = $env:QSL_BASE_URL

function Info($m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Ok($m)   { Write-Host " ok $m" -ForegroundColor Green }
function Warn($m) { Write-Host "warn $m" -ForegroundColor Yellow }
function Die($m)  { Write-Host "error $m" -ForegroundColor Red; throw $m }

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
           ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if ($Prefix)                    { $Dest = $Prefix; $Scope = 'User' }
elseif ($isAdmin -and -not $User) { $Dest = Join-Path $env:ProgramFiles 'QSL Agent'; $Scope = 'Machine' }
else                            { $Dest = Join-Path $env:LOCALAPPDATA 'Programs\QSL Agent'; $Scope = 'User' }

function Get-PathEntries($scope) {
  $p = [Environment]::GetEnvironmentVariable('Path', $scope)
  if ($p) { $p -split ';' | Where-Object { $_ } } else { @() }
}

# ---- uninstall ---------------------------------------------------------------
if ($Uninstall) {
  foreach ($d in @($Dest, (Join-Path $env:ProgramFiles 'QSL Agent'), (Join-Path $env:LOCALAPPDATA 'Programs\QSL Agent'))) {
    if (Test-Path $d) {
      try { Remove-Item -Recurse -Force $d; Ok "removed $d" } catch { Warn "could not remove $d (re-run elevated?)" }
    }
    foreach ($s in 'User', 'Machine') {
      $entries = Get-PathEntries $s
      if ($entries -contains $d) {
        try { [Environment]::SetEnvironmentVariable('Path', (($entries | Where-Object { $_ -ne $d }) -join ';'), $s) } catch {}
      }
    }
  }
  Write-Host 'Config (%APPDATA%\qsl-agent) and your Obsidian vault were left in place.'
  return
}

# ---- platform ----------------------------------------------------------------
$arch = $env:PROCESSOR_ARCHITEW6432; if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
switch ($arch) {
  'AMD64' { $Target = 'x86_64-pc-windows-msvc' }
  'ARM64' { $Target = 'x86_64-pc-windows-msvc'; Warn 'ARM64 detected; using the x64 build (runs under emulation).' }
  default { Die "unsupported architecture: $arch" }
}

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("qslagent-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Tmp | Out-Null

try {
  $exe = Join-Path $Tmp "$Bin.exe"

  function Install-FromSource {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Die 'no prebuilt binary available and cargo not found. Install Rust from https://rustup.rs and re-run with -FromSource.' }
    if (-not (Get-Command git -ErrorAction SilentlyContinue))   { Die 'git is required to build from source' }
    Info 'Building from source (this takes a few minutes)'
    $src = Join-Path $Tmp 'src'
    $gitArgs = @('clone', '--depth', '1')
    if ($Version -ne 'latest') { $gitArgs += @('--branch', $Version) }
    $gitArgs += @("https://github.com/$Repo.git", $src)
    & git @gitArgs 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Die 'git clone failed' }
    Push-Location $src
    try { & cargo build --release --locked; if ($LASTEXITCODE -ne 0) { Die 'cargo build failed' } } finally { Pop-Location }
    Copy-Item (Join-Path $src "target\release\$Bin.exe") $exe
  }

  function Install-FromRelease {
    $asset = "$Bin-$Target.zip"
    if ($BaseUrl) { $url = $BaseUrl }
    elseif ($Version -eq 'latest') { $url = "https://github.com/$Repo/releases/latest/download" }
    else { $url = "https://github.com/$Repo/releases/download/$Version" }
    Info "Downloading $asset ($Version)"
    $zip = Join-Path $Tmp $asset
    try { Invoke-WebRequest -UseBasicParsing -Uri "$url/$asset" -OutFile $zip } catch { return $false }
    $sumFile = "$zip.sha256"
    $haveSum = $true
    try { Invoke-WebRequest -UseBasicParsing -Uri "$url/$asset.sha256" -OutFile $sumFile } catch { $haveSum = $false }
    if ($haveSum) {
      $want = ((Get-Content $sumFile -Raw).Trim() -split '\s+')[0].ToLower()
      $got = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
      if ($want -ne $got) { Die "checksum mismatch for $asset (expected $want, got $got)" }
      Ok 'checksum verified'
    } else { Warn 'no checksum published for this release; skipping verification' }
    Expand-Archive -Path $zip -DestinationPath $Tmp -Force
    return (Test-Path $exe)
  }

  if ($FromSource) { Install-FromSource }
  elseif (-not (Install-FromRelease)) { Warn "no prebuilt binary for $Target; falling back to a source build"; Install-FromSource }

  # ---- install ---------------------------------------------------------------
  New-Item -ItemType Directory -Path $Dest -Force | Out-Null
  $target1 = Join-Path $Dest "$Bin.exe"
  $target2 = Join-Path $Dest "$Alias.exe"
  Copy-Item $exe $target1 -Force
  Copy-Item $exe $target2 -Force

  $ver = & $target1 --version 2>$null
  if (-not $ver) { Die "installed binary failed to run: $target1" }
  Ok "installed $ver"
  Ok $target1
  Ok "$target2 (same program)"

  # ---- PATH ------------------------------------------------------------------
  if ((Get-PathEntries $Scope) -notcontains $Dest) {
    $new = ((Get-PathEntries $Scope) + $Dest) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $new, $Scope)
    Ok "added $Dest to the $Scope PATH"
  }
  $env:Path = "$env:Path;$Dest"

  Write-Host ''
  Write-Host 'Open a NEW terminal, then:'
  Write-Host '  qslagent --health-check'
  Write-Host '  qslagent'
  Write-Host '  qslagent --help   (or /help inside the REPL)'
  Write-Host 'Website: https://www.quantumsecularlabs.com/qsl-agent'
  Write-Host ''
  Write-Host "Uninstall: & ([scriptblock]::Create((irm https://raw.githubusercontent.com/$Repo/main/install.ps1))) -Uninstall"
}
finally {
  Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
