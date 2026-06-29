<#
  rebuild.ps1 — one-command rebuild for evo_sim.

  Run it from ANYWHERE; it locates its own folder (the repo root) and steps into
  sim\, the root, and tools\ for you. It also prints each directory it moves into,
  so the structure stays visible rather than hidden.

  Usage examples (from any prompt):
    .\rebuild.ps1                # web build + copy + harness build + harness CHECK
    .\rebuild.ps1 -Trace         # same, but web wasm built with --features trace
    .\rebuild.ps1 -Serve         # full rebuild, then start the web server
    .\rebuild.ps1 -WebOnly       # web build + copy only (fast loop, no harness)
    .\rebuild.ps1 -Capture       # re-baseline the golden (deliberate; see note)
    .\rebuild.ps1 -Trace -Serve  # traced build, then serve

  If PowerShell blocks the script, run it once as:
    powershell -ExecutionPolicy Bypass -File .\rebuild.ps1
#>
param(
    [switch]$Trace,      # build web wasm with --features trace
    [switch]$Serve,      # start python server.py at the end (blocks)
    [switch]$WebOnly,    # skip the harness build + check
    [switch]$Capture,    # re-baseline the golden INSTEAD of checking it
    [int]$Seed  = 137,
    [int]$Ticks = 5000,
    [int]$Every = 500
)

# --- locate the repo root (this script's own folder) ------------------------
$Root  = $PSScriptRoot
$Sim   = Join-Path $Root 'sim'
$Web   = Join-Path $Root 'web'
$Tools = Join-Path $Root 'tools'

foreach ($d in @($Sim, $Web)) {
    if (-not (Test-Path $d)) {
        Write-Host "ERROR: expected folder not found: $d" -ForegroundColor Red
        Write-Host "Put rebuild.ps1 in the repo root (the folder that contains sim\ and web\)." -ForegroundColor Red
        exit 1
    }
}

# --- helper: run a command in a directory, abort on non-zero exit -----------
function Invoke-Step {
    param([string]$Dir, [string]$Label, [scriptblock]$Action)
    Write-Host ""
    Write-Host "==> $Label" -ForegroundColor Cyan
    Write-Host "    (in $Dir)" -ForegroundColor DarkGray
    Push-Location $Dir
    try {
        & $Action
        if ($LASTEXITCODE -ne 0) {
            Write-Host "FAILED at: $Label (exit $LASTEXITCODE)" -ForegroundColor Red
            Pop-Location
            exit $LASTEXITCODE
        }
    } finally {
        if ((Get-Location).Path -eq (Resolve-Path $Dir).Path) { Pop-Location }
    }
}

$goldenRel = "golden/seed${Seed}_${Ticks}.txt"

# --- 1. build the web wasm (from sim\) --------------------------------------
if ($Trace) {
    Invoke-Step $Sim "Building web wasm (TRACED)" { wasm-pack build --target web --release -- --features trace }
} else {
    Invoke-Step $Sim "Building web wasm" { wasm-pack build --target web --release }
}

# --- 2. copy pkg into web\ (from root) --------------------------------------
Invoke-Step $Root "Copying sim\pkg -> web\pkg" { xcopy sim\pkg web\pkg /E /I /Y }

# --- 3. + 4. harness build + check (unless -WebOnly) ------------------------
if (-not $WebOnly) {
    Invoke-Step $Sim "Building harness wasm (nodejs target)" {
        wasm-pack build --target nodejs --release --out-dir ..\tools\pkg-node
    }

    if (-not (Test-Path $Tools)) {
        Write-Host "WARNING: tools\ not found — skipping harness check." -ForegroundColor Yellow
    } elseif ($Capture) {
        Write-Host ""
        Write-Host "NOTE: -Capture OVERWRITES the golden baseline." -ForegroundColor Yellow
        Write-Host "Only do this when you have INTENTIONALLY changed dynamics." -ForegroundColor Yellow
        Invoke-Step $Tools "Re-baselining golden ($goldenRel)" {
            node verify.cjs capture $Seed $Ticks --every $Every > $goldenRel
        }
        Write-Host "New baseline written. Re-run without -Capture to confirm it MATCHes twice." -ForegroundColor Green
    } else {
        Invoke-Step $Tools "Harness check vs $goldenRel" {
            node verify.cjs check $Seed $Ticks $goldenRel --every $Every
        }
    }
}

# --- 5. serve (optional, last — it blocks) ----------------------------------
if ($Serve) {
    $serverPy = $null
    foreach ($cand in @((Join-Path $Web 'server.py'), (Join-Path $Root 'server.py'))) {
        if (Test-Path $cand) { $serverPy = $cand; break }
    }
    if (-not $serverPy) {
        $found = Get-ChildItem -Path $Root -Recurse -Filter server.py -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $serverPy = $found.FullName }
    }
    if (-not $serverPy) {
        Write-Host "Could not find server.py — start it yourself from the folder that has index.html." -ForegroundColor Yellow
    } else {
        $serveDir = Split-Path $serverPy -Parent
        Write-Host ""
        Write-Host "==> Serving (Ctrl+C to stop)" -ForegroundColor Cyan
        Write-Host "    (in $serveDir)  ->  open localhost:9090, hard-reload Ctrl+Shift+R" -ForegroundColor DarkGray
        Push-Location $serveDir
        python server.py
        Pop-Location
    }
}

Write-Host ""
Write-Host "Done." -ForegroundColor Green
