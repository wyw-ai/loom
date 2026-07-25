#Requires -Version 5.1
<#
.SYNOPSIS
  Loom runtime installer for Windows.

.DESCRIPTION
  Downloads the latest Loom runtime package (loom, loom-server, loom-daemon)
  from GitHub Releases, verifies its SHA-256 checksum against the release
  SHA256SUMS file, installs the binaries, and adds the install directory to
  the user PATH.

  One-liner:
    iwr -useb https://raw.githubusercontent.com/wyw-ai/loom/main/scripts/install.ps1 | iex

  This is the user-level installer. For Windows Service deployment
  (auto-start, crash recovery), see scripts/windows/install.ps1 instead.

.PARAMETER Version
  Release tag to install, for example v0.1.0. Defaults to the latest release.

.PARAMETER BinDir
  Directory to install the binaries into.
  Defaults to %LOCALAPPDATA%\Programs\Loom\bin.

.EXAMPLE
  .\install.ps1
  .\install.ps1 -Version v0.1.0 -BinDir C:\tools\loom
#>
param(
  [string]$Version = "",
  [string]$BinDir = (Join-Path $env:LOCALAPPDATA "Programs\Loom\bin")
)

$ErrorActionPreference = "Stop"
$Repo = "wyw-ai/loom"
$Target = "x86_64-pc-windows-msvc"

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Resolve-Tag {
  if ($Version) { return $Version }
  $uri = "https://api.github.com/repos/$Repo/releases/latest"
  try {
    $release = Invoke-RestMethod -Uri $uri -Headers @{ "User-Agent" = "loom-install" }
  }
  catch {
    throw "failed to resolve the latest release of $Repo (no release published yet?): $_"
  }
  if (-not $release.tag_name) { throw "no published release found for $Repo" }
  return [string]$release.tag_name
}

function Get-Sha256FromSums {
  param([string]$SumsFile, [string]$AssetName)
  foreach ($line in Get-Content $SumsFile) {
    # sha256sum/shasum output format: "<hash>  <filename>"
    if ($line -match "^([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($AssetName))$") {
      return $Matches[1].ToLower()
    }
  }
  throw "no checksum entry for $AssetName in SHA256SUMS"
}

$tag = Resolve-Tag
$ver = $tag -replace '^v', ''
$asset = "loom-runtime-$ver-$Target.zip"
$baseUrl = "https://github.com/$Repo/releases/download/$tag"

Write-Host "Loom $tag ($Target)"
Write-Host "asset:  $asset"
Write-Host "bin dir: $BinDir"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("loom-install-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
  $zipPath = Join-Path $tmp $asset
  $sumsPath = Join-Path $tmp "SHA256SUMS"

  Write-Host "downloading $baseUrl/$asset"
  Invoke-WebRequest -Uri "$baseUrl/$asset" -OutFile $zipPath
  Invoke-WebRequest -Uri "$baseUrl/SHA256SUMS" -OutFile $sumsPath

  $expected = Get-Sha256FromSums -SumsFile $sumsPath -AssetName $asset
  $actual = (Get-FileHash -Algorithm SHA256 $zipPath).Hash.ToLower()
  if ($actual -ne $expected) {
    throw "checksum mismatch for ${asset}: expected $expected, got $actual"
  }
  Write-Host "checksum verified"

  $extractDir = Join-Path $tmp "extract"
  Expand-Archive -Path $zipPath -DestinationPath $extractDir

  $packageRoot = Get-ChildItem -Path $extractDir -Directory -Filter "loom-runtime-*" | Select-Object -First 1
  if (-not $packageRoot) { throw "runtime package did not extract correctly" }

  New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
  $binaries = Get-ChildItem -Path (Join-Path $packageRoot.FullName "bin") -Filter "loom*.exe"
  if (-not $binaries) { throw "no loom binaries found in $asset" }
  foreach ($exe in $binaries) {
    Copy-Item $exe.FullName (Join-Path $BinDir $exe.Name) -Force
    Write-Host "installed $($exe.Name) -> $BinDir"
  }

  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (($userPath -split ';') -notcontains $BinDir) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$BinDir", "User")
    Write-Host "added $BinDir to the user PATH"
  }
  if (($env:Path -split ';') -notcontains $BinDir) {
    $env:Path = "$env:Path;$BinDir"
  }

  Write-Host ""
  Write-Host "Done. Next steps (open a NEW terminal to pick up PATH):"
  Write-Host "  1. loom-server --bind 127.0.0.1:7878"
  Write-Host "  2. loom who"
  Write-Host "     loom channel create --title general"
  Write-Host "     loom chat"
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
