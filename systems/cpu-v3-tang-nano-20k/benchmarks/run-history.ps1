# Runs the frozen benchmark suite across every stageN-bench tag plus HEAD and
# merges the per-ref CSVs into one cross-Stage CSV. Generated artifacts stay
# under target/bench-history/ and are never committed.
param(
    [Parameter()]
    [string]$OutDir = "",

    # HEAD runs are for standalone checks; full history runs stages only
    [Parameter()]
    [switch]$IncludeCurrent
)

$ErrorActionPreference = "Continue"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
if ($OutDir -eq "") { $OutDir = Join-Path $repoRoot "target/bench-history" }
$images = Join-Path $OutDir "images"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# 1. build the shared images with the current compiler
Push-Location $repoRoot
try {
    & (Join-Path $repoRoot "scripts/run-cargo.ps1") `
        -Subcommand run `
        -Label "bench images" `
        -CargoArgs @("-p", "cpu-v3-tang-nano-20k", "--bin", "cpu-v3-bench-images", "--", "--out", $images)
    if ($LASTEXITCODE -ne 0) { throw "image build failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

# 2. run every ref
$refs = @()
0..12 | ForEach-Object { $refs += @{ Ref = "stage$_-bench"; Label = "stage$_" } }
if ($IncludeCurrent) { $refs += @{ Ref = "HEAD"; Label = "current" } }

$failed = @()
foreach ($entry in $refs) {
    Write-Host "== $($entry.Label) ($($entry.Ref))"
    & (Join-Path $PSScriptRoot "run-commit.ps1") `
        -Ref $entry.Ref `
        -Label $entry.Label `
        -Images $images `
        -OutDir $OutDir
    if ($LASTEXITCODE -ne 0) { $failed += $entry.Label }
}

# 3. merge per-ref CSVs (header once; export-results.ps1 pads missing columns)
$combined = Join-Path $OutDir "combined.csv"
$headerWritten = $false
$rows = @()
foreach ($entry in $refs) {
    $file = Join-Path $OutDir "$($entry.Label).csv"
    if (-not (Test-Path $file)) { continue }
    $lines = Get-Content -LiteralPath $file
    if (-not $headerWritten) {
        $rows += $lines[0]
        $headerWritten = $true
    }
    $rows += $lines | Select-Object -Skip 1
}
Set-Content -LiteralPath $combined -Value $rows
Write-Host "wrote $combined"

if ($failed.Count -gt 0) {
    throw "failed refs: $($failed -join ', ')"
}
