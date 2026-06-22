<#
.SYNOPSIS
    Local CI matrix entry point for Windows.

.DESCRIPTION
    Runs the full local quality gate: cargo fmt --check, cargo clippy,
    cargo test --workspace --exclude loom-gui. Test target directory is
    redirected to F:\pj\loom-test\target by default so the host project
    cache is not polluted; override via $env:LOOM_MATRIX_TARGET_DIR.

    Exit codes:
        0  all stages green
        1  argument / environment error
        2  fmt failed
        3  clippy failed
        4  test failed

.PARAMETER SkipFmt
    Skip cargo fmt --check.

.PARAMETER SkipClippy
    Skip cargo clippy.

.PARAMETER SkipTest
    Skip cargo test.

.PARAMETER Quick
    Equivalent to -SkipClippy (clippy is the slowest stage). Use during
    rapid iteration; pre-push hook should NOT pass -Quick.

.PARAMETER StrictClippy
    Treat clippy warnings as errors (-D warnings). Default is informational
    (clippy runs without -D warnings) because the current baseline has
    pre-existing style lints that the loom-platform iteration will fix.
    Once BE/QA confirms the baseline is clean, flip this to default-on.

.EXAMPLE
    .\scripts\ci\run-matrix.ps1
    Run all three stages; clippy is informational (non-blocking).

.EXAMPLE
    .\scripts\ci\run-matrix.ps1 -StrictClippy
    Promote clippy warnings to hard errors (target end-state).

.EXAMPLE
    .\scripts\ci\run-matrix.ps1 -Quick
    Skip clippy for faster local feedback.

.NOTES
    Part of the loom-platform Windows-compatibility iteration.
    Pair script: scripts/ci/run-matrix.sh (Bash for macOS/Linux).
    See docs/ci/local-matrix.md for the design rationale.
#>

[CmdletBinding()]
param(
    [switch]$SkipFmt,
    [switch]$SkipClippy,
    [switch]$SkipTest,
    [switch]$Quick,
    [switch]$StrictClippy
)

$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $RepoRoot

if (-not $env:LOOM_MATRIX_TARGET_DIR) {
    $env:LOOM_MATRIX_TARGET_DIR = 'F:\pj\loom-test\target'
}
$env:CARGO_TARGET_DIR = $env:LOOM_MATRIX_TARGET_DIR

if ($Quick) { $SkipClippy = $true }

function Write-Stage($name) {
    Write-Host ''
    Write-Host "==> $name" -ForegroundColor Cyan
}

function Invoke-Stage($name, $code, $exitCode) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $code
    $rc = $LASTEXITCODE
    $sw.Stop()
    if ($rc -ne 0) {
        Write-Host ("[FAIL] {0} ({1:N1}s, exit {2})" -f $name, $sw.Elapsed.TotalSeconds, $rc) -ForegroundColor Red
        exit $exitCode
    }
    Write-Host ("[OK]   {0} ({1:N1}s)" -f $name, $sw.Elapsed.TotalSeconds) -ForegroundColor Green
}

Write-Host '=== loom local CI matrix (Windows) ===' -ForegroundColor Magenta
Write-Host "    repo:           $RepoRoot"
Write-Host "    target dir:     $env:CARGO_TARGET_DIR"
Write-Host "    rustc:          $((rustc --version) 2>&1)"
Write-Host "    cargo:          $((cargo --version) 2>&1)"
Write-Host ''

$total = [System.Diagnostics.Stopwatch]::StartNew()

if (-not $SkipFmt) {
    Write-Stage 'cargo fmt --check'
    Invoke-Stage 'fmt' { cargo fmt --all -- --check } 2
} else {
    Write-Host 'SKIP fmt' -ForegroundColor Yellow
}

if (-not $SkipClippy) {
    # P0-Lint PAL guardrail — runs across the FULL workspace (including loom-gui)
    # with only `clippy::disallowed_methods` denied. Catches any new
    # `std::process::Command::new` / `tokio::process::Command::new` regression
    # in gui/ without forcing the broader baseline rewrite that is still
    # excluded for loom-gui (see Iter#3 §8.9 Known Limitation).
    Write-Stage 'cargo clippy --workspace --all-targets -- -D clippy::disallowed_methods  [PAL guardrail]'
    Invoke-Stage 'clippy-pal' { cargo clippy --workspace --all-targets -- -D clippy::disallowed_methods } 3

    if ($StrictClippy) {
        Write-Stage 'cargo clippy --workspace --exclude loom-gui --all-targets -- -D warnings  [strict]'
        Invoke-Stage 'clippy' { cargo clippy --workspace --exclude loom-gui --all-targets -- -D warnings } 3
    } else {
        Write-Stage 'cargo clippy --workspace --exclude loom-gui --all-targets  [informational]'
        Invoke-Stage 'clippy' { cargo clippy --workspace --exclude loom-gui --all-targets } 3
    }
} else {
    Write-Host 'SKIP clippy' -ForegroundColor Yellow
}

if (-not $SkipTest) {
    Write-Stage 'cargo test --workspace --exclude loom-gui --no-fail-fast'
    Invoke-Stage 'test' { cargo test --workspace --exclude loom-gui --no-fail-fast } 4
} else {
    Write-Host 'SKIP test' -ForegroundColor Yellow
}

$total.Stop()
Write-Host ''
Write-Host ("=== matrix OK ({0:N1}s total) ===" -f $total.Elapsed.TotalSeconds) -ForegroundColor Green
exit 0
