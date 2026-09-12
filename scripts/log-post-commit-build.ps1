# Run this only right after a commit: rebuild the full system and append one PnR row
# to the project ledger (.agent/logs/projects/<project>/resources.csv).
#
# Why: that CSV records "the complete build of a committed tree". Copying the numbers by
# hand is slow, error-prone, and easy to forget. This script does three things:
#   1. preconditions: the worktree must be clean (only committed builds are recorded), and
#      after the rebuild the PnR report must be newer than HEAD;
#   2. rebuild the full system through scripts/run-cargo.ps1 (includes the Gowin audits);
#   3. parse the PnR report and append one row: date/commit/resources/one-line change.
#
# Do not run it casually: it is only meaningful for a freshly committed complete build, and
# WIP or intermediate commits must not be recorded.
#
# Usage:
#   scripts/log-post-commit-build.ps1 -Change "cache valid-array write port" `
#       [-Promotion "ledger:Cache valid-array write port"] [-State dirty] [-Project <name>]
# Inspect what it would write first:
#   scripts/log-post-commit-build.ps1 -Change "..." -SkipBuild -DryRun

param(
    [Parameter(Mandatory = $true)][string]$Change,
    # Empty means "resolve the active project from .agent/README.md's project table".
    [string]$Project = "",
    [string]$Promotion = "exploratory",
    [string]$State = "clean",
    [switch]$SkipBuild,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

if (-not $Project) {
    # The active project is the one whose project.md says `status: active`.
    $projectsRoot = Join-Path $repoRoot ".agent/logs/projects"
    $active = @()
    if (Test-Path $projectsRoot) {
        $active = @(Get-ChildItem -Path $projectsRoot -Directory | Where-Object {
                $meta = Join-Path $_.FullName "project.md"
                (Test-Path $meta) -and (Select-String -Path $meta -Pattern '^status:\s*active' -Quiet)
            })
    }
    if ($active.Count -ne 1) {
        throw "Cannot resolve the active project: $($active.Count) found with 'status: active' under .agent/logs/projects. Pass -Project <name>."
    }
    $Project = $active[0].Name
}

function Quote-CsvField([string]$value) {
    if ($null -eq $value) { return "" }
    if ($value -match '[",\r\n]') { return '"' + ($value -replace '"', '""') + '"' }
    return $value
}

function Get-ReportField([string]$text, [string]$pattern, [string]$name) {
    $match = [regex]::Match($text, $pattern)
    if (-not $match.Success) { throw "PnR report has no $name (pattern: $pattern)" }
    return $match.Groups[1].Value
}

Push-Location $repoRoot
try {
    # --- 1. Preconditions ------------------------------------------------------
    $dirty = @(& git status --porcelain)
    if ((@($dirty) -join "").Trim().Length -ne 0) {
        if ($DryRun) {
            Write-Warning "Worktree is not clean; previewing anyway because -DryRun writes nothing."
        } else {
            throw "Worktree is not clean; only committed builds are recorded. Commit first, or use -SkipBuild -DryRun to preview."
        }
    }
    $commit = (& git rev-parse --short HEAD).Trim()
    # The freshness check below is about when the commit landed, so it uses the committer date.
    # The ledger row's date follows the journal: the author date (it survives a rewrite) bucketed
    # by the same 06:00 day boundary, so a row matches the daily entry it belongs to.
    $commitTime = [datetime]::Parse((& git show -s --format=%cI HEAD).Trim())
    $commitDay = [datetime]::Parse((& git show -s --format=%aI HEAD).Trim()).AddHours(-6).ToString("yyyy-MM-dd")

    $csv = Join-Path $repoRoot ".agent/logs/projects/$Project/resources.csv"
    if (-not (Test-Path $csv)) { throw "Project ledger not found: $csv" }
    $existing = Import-Csv $csv
    if ($existing | Where-Object { $_.commit -eq $commit }) {
        if ($DryRun) {
            Write-Warning "$commit is already recorded in $csv; previewing anyway."
        } else {
            throw "$commit is already recorded in $csv."
        }
    }

    # --- 2. Rebuild the full system --------------------------------------------
    $pnr = Join-Path $repoRoot "target/cpu_v3_system_gowin/impl/pnr/cpu_v3_system.rpt.txt"
    $tr = Join-Path $repoRoot "target/cpu_v3_system_gowin/impl/pnr/cpu_v3_system.tr"
    $paths = Join-Path $repoRoot "target/cpu_v3_system_gowin/impl/pnr/cpu_v3_system.timing_paths"

    if ($SkipBuild) {
        Write-Warning "-SkipBuild: no rebuild, and the report is not checked against HEAD."
    } else {
        Write-Host "Rebuilding the full system ($commit)..."
        & (Join-Path $repoRoot "scripts/run-cargo.ps1") -Subcommand run -Label "post-commit build $commit" -CargoArgs @(
            "-p", "cpu-v3-tang-nano-20k", "--example", "cpu_v3_system", "--", "--build"
        )
        if ($LASTEXITCODE -ne 0) { throw "Full-system build failed; nothing was recorded." }
        $reportTime = (Get-Item $pnr).LastWriteTime
        if ($reportTime -lt $commitTime.LocalDateTime) {
            throw "Report time $reportTime predates HEAD ($($commitTime.LocalDateTime)); this is not a build of the current commit."
        }
    }

    foreach ($file in @($pnr, $tr, $paths)) {
        if (-not (Test-Path $file)) { throw "Missing report $file; run a full-system build first." }
    }

    # --- 3. Parse and append ---------------------------------------------------
    $text = Get-Content -Path $pnr -Raw
    $lutAlu = [regex]::Match($text, '--LUT,ALU,ROM16\s*\|\s*\d+\((\d+) LUT,\s*(\d+) ALU')
    if (-not $lutAlu.Success) { throw "PnR report has no LUT/ALU line." }
    $bsram = 0
    foreach ($label in @("--SDPB", "--DPB", "--pROM")) {
        $m = [regex]::Match($text, [regex]::Escape($label) + '\s*\|\s*(\d+)')
        if ($m.Success) { $bsram += [int]$m.Groups[1].Value }
    }
    $slack = ""
    $lines = Get-Content -Path $paths
    for ($i = 0; $i -lt $lines.Count - 1; $i++) {
        if ($lines[$i].Trim() -eq "SETUP") { $slack = $lines[$i + 1].Trim(); break }
    }
    if (-not $slack) { throw "timing_paths has no SETUP slack." }

    $row = [ordered]@{
        date      = $commitDay
        commit    = $commit
        state     = $State
        logic     = Get-ReportField $text 'Logic\s*\|\s*(\d+)/' "Logic"
        lut       = $lutAlu.Groups[1].Value
        alu       = $lutAlu.Groups[2].Value
        ssram     = Get-ReportField $text 'SSRAM\(RAM16\)\s*\|\s*(\d+)' "SSRAM"
        ff        = Get-ReportField $text 'Logic Register as FF\s*\|\s*(\d+)/' "logic flip-flops"
        cls       = Get-ReportField $text 'CLS\s*\|\s*(\d+)/' "CLS"
        bsram     = $bsram
        dsp       = Get-ReportField $text '--MULT18X18\s*\|\s*(\d+)' "DSP"
        fmax_mhz  = Get-ReportField (Get-Content -Path $tr -Raw) 'cpu_clk\s+\S+\(MHz\)\s+([\d.]+)\(MHz\)' "cpu_clk fmax"
        slack_ns  = $slack
        change    = $Change
        evidence  = "target/cpu_v3_system_gowin/impl/pnr/cpu_v3_system.rpt.txt"
        promotion = $Promotion
    }

    $line = (($row.Values | ForEach-Object { Quote-CsvField ([string]$_) }) -join ",")
    if ($DryRun) {
        Write-Host ""
        Write-Host "(-DryRun, nothing written) $csv"
        Write-Host $line
        return
    }
    [System.IO.File]::AppendAllText($csv, $line + "`n", (New-Object System.Text.UTF8Encoding($false)))
    Write-Host ""
    Write-Host "Appended to $csv"
    Write-Host $line
    Write-Host ""
    Write-Host "Now update today's diary (see logs/README.md, 'when to write')."
} finally {
    Pop-Location
}
