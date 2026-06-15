# Loom Windows Service uninstaller
# Requires Administrator privileges
# Usage: .\uninstall.ps1 [-Service server|daemon|all]

param(
    [ValidateSet("server", "daemon", "all")]
    [string]$Service = "all"
)

$ErrorActionPreference = "Stop"

# ── 1. Admin privilege check ──────────────────────────────────────────
if (-NOT ([Security.Principal.WindowsPrincipal]
          [Security.Principal.WindowsIdentity]::GetCurrent()
          ).IsInRole([Security.Principal.WindowsBuiltInRole] "Administrator")) {
    Write-Error "This script requires Administrator privileges. Right-click PowerShell and select 'Run as Administrator'."
    exit 1
}

# ── 2. Locate files ───────────────────────────────────────────────────
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ConfigDir = Join-Path (Split-Path -Parent $ScriptDir) "config"
$WinSW     = Join-Path $ScriptDir "WinSW-x64.exe"

if (-not (Test-Path $WinSW)) {
    Write-Error "WinSW-x64.exe not found at $WinSW"
    exit 1
}

# ── 3. Uninstall server service ───────────────────────────────────────
if ($Service -eq "server" -or $Service -eq "all") {
    $ServerXml = Join-Path $ConfigDir "loom-server.xml"
    if (Test-Path $ServerXml) {
        Write-Host "Stopping Loom Server..."
        & $WinSW stop $ServerXml *>$null
        Write-Host "Uninstalling Loom Server..."
        & $WinSW uninstall $ServerXml
        Write-Host "Loom Server service removed."
    } else {
        Write-Host "Loom Server service not found (loom-server.xml missing)."
    }
}

# ── 4. Uninstall daemon service ───────────────────────────────────────
if ($Service -eq "daemon" -or $Service -eq "all") {
    $DaemonXml = Join-Path $ConfigDir "loom-daemon.xml"
    if (Test-Path $DaemonXml) {
        Write-Host "Stopping Loom Daemon..."
        & $WinSW stop $DaemonXml *>$null
        Write-Host "Uninstalling Loom Daemon..."
        & $WinSW uninstall $DaemonXml
        Write-Host "Loom Daemon service removed."
    } else {
        Write-Host "Loom Daemon service not found (loom-daemon.xml missing)."
    }
}

Write-Host ""
Write-Host "Uninstallation complete. Log files remain at %LOCALAPPDATA%\loom\logs\."
