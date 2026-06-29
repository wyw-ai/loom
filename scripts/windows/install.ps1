# Loom Windows Service installer
# Requires Administrator privileges
# Usage: .\install.ps1 [-Service server|daemon|all] [-Bind 127.0.0.1:7878]

param(
    [ValidateSet("server", "daemon", "all")]
    [string]$Service = "all",
    [string]$Bind = "127.0.0.1:7878"
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
$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$ConfigDir  = Join-Path (Split-Path -Parent $ScriptDir) "config"
$WinSW      = Join-Path $ScriptDir "WinSW-x64.exe"

if (-not (Test-Path $WinSW)) {
    Write-Error "WinSW-x64.exe not found at $WinSW. Ensure you are running this script from the release package bin/ directory."
    exit 1
}

# ── 3. Install server service ─────────────────────────────────────────
if ($Service -eq "server" -or $Service -eq "all") {
    $ServerXml = Join-Path $ConfigDir "loom-server.xml"
    if (-not (Test-Path $ServerXml)) {
        Write-Error "loom-server.xml not found at $ServerXml"
        exit 1
    }

    # Resolve %BASE% placeholder and bind address
    $ServerConfig = (Get-Content $ServerXml -Raw) -replace "%BASE%", $ConfigDir
    $ServerConfig = $ServerConfig -replace "127.0.0.1:7878", $Bind
    $TempXml = Join-Path $env:TEMP "loom-server.xml"
    $ServerConfig | Set-Content $TempXml -Encoding UTF8

    Write-Host "Installing Loom Server service..."
    # Idempotent: WinSW `install` fails if the service already exists, so on a
    # re-run (config change, upgrade, partial prior install) stop+uninstall the
    # existing instance first. Native-exe non-zero exits don't trip
    # $ErrorActionPreference, so failures here just fall through to install.
    if (Get-Service -Name LoomServer -ErrorAction SilentlyContinue) {
        Write-Host "LoomServer already installed; removing old instance for a clean reinstall..."
        & $WinSW stop   $TempXml 2>$null
        & $WinSW uninstall $TempXml 2>$null
    }
    & $WinSW install $TempXml
    Write-Host "Setting Loom Server to Automatic (delayed start)..."
    & $WinSW set $TempXml --startmode Automatic
    Write-Host "Starting Loom Server..."
    & $WinSW start $TempXml

    Remove-Item $TempXml -Force -ErrorAction SilentlyContinue
    Write-Host "Loom Server service installed and started."
}

# ── 4. Install daemon service ─────────────────────────────────────────
if ($Service -eq "daemon" -or $Service -eq "all") {
    $DaemonXml = Join-Path $ConfigDir "loom-daemon.xml"
    if (-not (Test-Path $DaemonXml)) {
        Write-Error "loom-daemon.xml not found at $DaemonXml"
        exit 1
    }

    $DaemonConfig = (Get-Content $DaemonXml -Raw) -replace "%BASE%", $ConfigDir
    $TempXml = Join-Path $env:TEMP "loom-daemon.xml"
    $DaemonConfig | Set-Content $TempXml -Encoding UTF8

    Write-Host "Installing Loom Daemon service..."
    # Idempotent: see the server block above.
    if (Get-Service -Name LoomDaemon -ErrorAction SilentlyContinue) {
        Write-Host "LoomDaemon already installed; removing old instance for a clean reinstall..."
        & $WinSW stop   $TempXml 2>$null
        & $WinSW uninstall $TempXml 2>$null
    }
    & $WinSW install $TempXml
    Write-Host "Starting Loom Daemon..."
    & $WinSW start $TempXml

    Remove-Item $TempXml -Force -ErrorAction SilentlyContinue
    Write-Host "Loom Daemon service installed and started."
}

Write-Host ""
Write-Host "Installation complete."
Write-Host "  Manage services:  services.msc"
Write-Host "  Status:           sc query LoomServer; sc query LoomDaemon"
Write-Host "  Stop:             sc stop LoomServer; sc stop LoomDaemon"
Write-Host "  Start:            sc start LoomServer; sc start LoomDaemon"
Write-Host "  Logs:             %LOCALAPPDATA%\loom\logs\"
Write-Host "  Uninstall:        .\uninstall.ps1"
