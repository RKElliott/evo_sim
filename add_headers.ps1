<#
  add_headers.ps1 — prepend the Apache/copyright header to each source file.

  Idempotent: files already carrying the notice are skipped, so it's safe to
  re-run (e.g. after adding new source files). Preserves your line endings and
  writes UTF-8 without a BOM.

  Skips by design: docs (.md), Cargo.toml/.lock, the golden master, and this
  script / rebuild.ps1 (their comment-based-help block must stay first).

  Run from the repo root:
      .\add_headers.ps1
  If PowerShell blocks it:
      powershell -ExecutionPolicy Bypass -File .\add_headers.ps1

  Review with `git diff` before committing.
#>

$root = $PSScriptRoot
if (-not $root) { $root = (Get-Location).Path }

$headerLines = @(
  'evo_sim - Copyright (c) 2026 Lens and Mix, LLC',
  'Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.',
  'More information: https://rkeithelliott.com'
)
$mark = 'Lens and Mix, LLC'
$nl   = "`r`n"

function Make-Block($style) {
  switch ($style) {
    '//'   { return (($headerLines | ForEach-Object { "// $_" }) -join $nl) + $nl }
    '#'    { return (($headerLines | ForEach-Object { "# $_" })  -join $nl) + $nl }
    'html' { return "<!-- $($headerLines[0])$nl     $($headerLines[1])$nl     $($headerLines[2]) -->$nl" }
  }
}

# Build the target list: (fullpath, comment style, after-first-line regex or $null)
$targets = @()
foreach ($dir in @('sim\src', 'utils\src')) {
  $p = Join-Path $root $dir
  if (Test-Path $p) {
    Get-ChildItem -Path $p -Filter *.rs | ForEach-Object { $targets += ,@($_.FullName, '//', $null) }
  }
}
$targets += ,@((Join-Path $root 'web\worker.js'),         '//',   $null)
$targets += ,@((Join-Path $root 'tools\verify.cjs'),      '//',   '^#!')
$targets += ,@((Join-Path $root 'web\server.py'),         '#',    '^#!')
$targets += ,@((Join-Path $root 'web\index.html'),        'html', '^<!DOCTYPE')
$targets += ,@((Join-Path $root 'web\architecture.html'), 'html', '^<!DOCTYPE')

$utf8 = New-Object System.Text.UTF8Encoding($false)
$changed = 0; $skipped = 0

foreach ($t in $targets) {
  $path = $t[0]; $style = $t[1]; $after = $t[2]
  if (-not (Test-Path $path)) { Write-Host "  (missing, skipped) $path" -ForegroundColor DarkGray; continue }

  $text = [System.IO.File]::ReadAllText($path)
  $head = $text.Substring(0, [Math]::Min(500, $text.Length))
  if ($head.Contains($mark)) { $skipped++; continue }

  $hdr = Make-Block $style

  if ($after) {
    $idx = $text.IndexOf("`n")
    if ($idx -ge 0) {
      $firstLine = $text.Substring(0, $idx).TrimEnd("`r")
      $rest      = $text.Substring($idx + 1)
      if ($firstLine -match $after) {
        $text = $firstLine + $nl + $hdr + $rest
      } else {
        $text = $hdr + $text
      }
    } else {
      $text = $hdr + $text
    }
  } else {
    $text = $hdr + $text
  }

  [System.IO.File]::WriteAllText($path, $text, $utf8)
  Write-Host "  headered $($path.Substring($root.Length).TrimStart('\'))" -ForegroundColor Cyan
  $changed++
}

Write-Host ""
Write-Host "Done. $changed headered, $skipped already had the notice (skipped)." -ForegroundColor Green
Write-Host "Review with: git diff   then commit and push." -ForegroundColor DarkGray
