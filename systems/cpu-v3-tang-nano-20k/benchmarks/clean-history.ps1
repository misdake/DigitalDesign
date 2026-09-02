# Removes every artifact produced by benchmarks/run-history.ps1: the merged
# and per-ref CSVs, per-ref trace directories, the hex images, and any
# leftover worktrees (including git-locked ones from interrupted runs).
param(
    [Parameter()]
    [string]$OutDir = ""
)

$ErrorActionPreference = "Continue"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
if ($OutDir -eq "") { $OutDir = Join-Path $repoRoot "target/bench-history" }

# remove leftover worktrees first (git tracks them even after deletion)
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
Get-ChildItem -LiteralPath $OutDir -Directory -Filter "wt-*" -ErrorAction SilentlyContinue |
    ForEach-Object {
        & git -C $repoRoot worktree remove -f -f $_.FullName 2>$null | Out-Null
        if (Test-Path $_.FullName) { Remove-Item -Recurse -Force $_.FullName }
    }
& git -C $repoRoot worktree prune
$ErrorActionPreference = $prevEap

if (Test-Path $OutDir) {
    Remove-Item -Recurse -Force $OutDir
    Write-Host "removed $OutDir"
} else {
    Write-Host "nothing to clean ($OutDir)"
}
