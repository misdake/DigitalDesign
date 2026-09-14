# Run the frozen CPU V3 benchmark suite and append one aggregate row to the project
# performance ledger (.agent/projects/<project>/performance.csv), archiving the raw
# suite CSV under that project's records/performance/.
#
# Why: the ledger answers "what did the whole frozen suite cost at this commit". A row per
# program would bury the signal (22 programs x 44 columns per run), so every row is a fixed
# aggregate and the raw file is the evidence. Aggregates are geometric means over programs,
# never sums (see benchmarks/README.md, "Comparing runs"). The suite revision and the
# metric-set revision are fingerprinted into each row, because cross-revision comparisons
# are not valid.
#
# The row's commit is read back from the raw CSV - the tree that was actually measured - and
# every row of that file must agree on it. When the script runs the suite itself it also
# requires a clean worktree, so the recorded commit really is the tree that ran.
#
# Usage:
#   scripts/log-post-commit-benchmark.ps1 -Change "dirty window scan"
#   # log a suite run that was already made (e.g. by benchmarks/run-suite.ps1):
#   scripts/log-post-commit-benchmark.ps1 -Change "..." -RawCsv target/stage15-results.csv
#   # inspect what it would write first:
#   scripts/log-post-commit-benchmark.ps1 -Change "..." -RawCsv <file> -DryRun
#
# Unlike its PnR sibling the suite is cheap (well under a minute), so running it on demand is
# fine; what still holds is that a row only means something for a committed tree.

param(
    [Parameter(Mandatory = $true)][string]$Change,
    # Empty means "resolve the active project from .agent/README.md's project table".
    [string]$Project = "",
    [string]$Promotion = "exploratory",
    # clean | dirty | baseline: `dirty` marks a probe measured on an uncommitted tree,
    # `baseline` marks a historical row backfilled for comparison.
    [string]$State = "clean",
    # Use an existing suite CSV instead of running the suite.
    [string]$RawCsv = "",
    # Required when importing dirty/reconstructed runs: their workload cannot be
    # recovered from the CSV's emulator commit. Supply the recorded v2 digest.
    [ValidatePattern('^[0-9a-f]{12}$')]
    [string]$SuiteDigest = "",
    # Run label handed to benchmarks/run-suite.ps1; it only names the scratch trace directory
    # and the scratch CSV, never a milestone. The durable copy is the archived raw CSV.
    [int]$StageLabel = 900,
    # Record the commit even if the ledger already has a row for it (determinism re-runs).
    [switch]$Force,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$inv = [Globalization.CultureInfo]::InvariantCulture

$ledgerColumns = @(
    "date", "commit", "state", "suite", "schema", "programs",
    "cycles_geomean", "retired_geomean", "cpi_geomean",
    "fetch_wait_pct_geomean", "data_path_pct_geomean",
    "icache_refills_geomean1", "dcache_refills_geomean1", "dcache_writebacks_geomean1",
    "change", "raw", "promotion"
)

function Quote-CsvField([string]$value) {
    if ($null -eq $value) { return "" }
    if ($value -match '[",\r\n]') { return '"' + ($value -replace '"', '""') + '"' }
    return $value
}

function Get-TextDigest([string]$text) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($text)
        $hash = $sha.ComputeHash($bytes)
        return (($hash | ForEach-Object { $_.ToString('x2') }) -join '').Substring(0, 12)
    } finally {
        $sha.Dispose()
    }
}

# Return the suite files from either a worktree directory or a committed tree.
function Get-SuiteFiles([string]$suiteDirectory, [string]$sourceCommit = "") {
    $suitePath = "systems/cpu-v3-tang-nano-20k/benchmarks/suite"
    if ($sourceCommit) {
        $resolved = @(& git rev-parse --verify --end-of-options "$sourceCommit^{commit}" 2>$null)
        if ($LASTEXITCODE -ne 0 -or $resolved.Count -ne 1) {
            throw "Cannot recover the suite for commit '$sourceCommit'; supply -SuiteDigest from the measured run."
        }
        $entries = @(& git ls-tree ($resolved[0] + ":" + $suitePath))
        if ($LASTEXITCODE -ne 0) { throw "Cannot read the suite tree at '$sourceCommit'." }
        $files = foreach ($entry in $entries) {
            if ($entry -match '^\d+ blob ([0-9a-f]+)\t(.+\.(rs|hex))$') {
                [pscustomobject]@{ Name = $Matches[2]; Blob = $Matches[1] }
            }
        }
    } else {
        $files = foreach ($file in (Get-ChildItem -LiteralPath $suiteDirectory -File |
                Where-Object { $_.Extension -in @(".rs", ".hex") })) {
            # Git's clean filters normalize checkout line endings, so an
            # unchanged worktree and its committed tree have the same identity.
            $blob = & git hash-object "--path=$suitePath/$($file.Name)" -- $file.FullName
            if ($LASTEXITCODE -ne 0) { throw "Cannot fingerprint $($file.FullName)." }
            [pscustomobject]@{ Name = $file.Name; Blob = $blob }
        }
    }
    if (@($files).Count -eq 0) { throw "The measured suite contains no .rs/.hex programs." }
    return @($files)
}

# One digest over the whole program set: renaming, editing or adding a program changes it,
# which is exactly the "suite revision" boundary the benchmark README talks about.
function Get-SuiteDigest([string]$suiteDirectory, [string]$sourceCommit = "") {
    $files = @(Get-SuiteFiles $suiteDirectory $sourceCommit)
    $lines = @($files | Sort-Object Name | ForEach-Object { $_.Name + " " + $_.Blob })
    return Get-TextDigest ("git-blobs-v2`n" + [string]::Join("`n", $lines))
}

function Assert-SuitePrograms($rows, [string]$suiteDirectory, [string]$sourceCommit = "") {
    $actual = @($rows | ForEach-Object { "$($_.name)" } | Sort-Object)
    if (@($actual | Where-Object { [string]::IsNullOrWhiteSpace($_) }).Count -ne 0) {
        throw "Raw suite CSV has an empty program name."
    }
    $unique = @($actual | Sort-Object -Unique)
    if ($unique.Count -ne $actual.Count) {
        throw "Raw suite CSV contains duplicate program rows."
    }
    $expected = @(Get-SuiteFiles $suiteDirectory $sourceCommit |
        ForEach-Object { [IO.Path]::GetFileNameWithoutExtension($_.Name) } |
        Sort-Object -Unique)
    $difference = @(Compare-Object -ReferenceObject $expected -DifferenceObject $unique)
    if ($difference.Count -ne 0) {
        $where = if ($sourceCommit) { "commit '$sourceCommit'" } else { "the measured worktree" }
        $missing = @($difference | Where-Object SideIndicator -eq '<=' | ForEach-Object InputObject)
        $extra = @($difference | Where-Object SideIndicator -eq '=>' | ForEach-Object InputObject)
        $missingText = if ($missing.Count) { $missing -join ', ' } else { "none" }
        $extraText = if ($extra.Count) { $extra -join ', ' } else { "none" }
        throw "Raw CSV program set does not match $where (missing: $missingText; extra: $extraText)."
    }
}

# The metric set is the exported column list; reordering or adding a column changes it.
function Get-SchemaDigest([string]$rawPath) {
    $header = (Get-Content -LiteralPath $rawPath -TotalCount 1)
    if ($null -eq $header) { throw "Raw suite CSV is empty: $rawPath" }
    $header = ([string]$header).TrimStart([char]0xFEFF)
    $columns = $header -split ',' | ForEach-Object { $_.Trim().Trim('"').ToLowerInvariant() }
    return Get-TextDigest ($columns -join ',')
}

# Geometric mean over per-program values; adding across programs is banned (see
# benchmarks/README.md, "Comparing runs"). -PlusOne smooths counters that may be zero.
function Get-ColumnGeomean($rows, [string]$name, [switch]$Required, [switch]$PlusOne) {
    $logSum = 0.0
    $count = 0
    $found = $false
    foreach ($row in $rows) {
        $property = $row.PSObject.Properties[$name]
        if ($null -eq $property) { continue }
        $found = $true
        $text = "$($property.Value)"
        if ($text -eq "") { continue }
        $value = [double]$text
        if ($PlusOne) { $value += 1.0 }
        if ($value -le 0.0) { throw "Cannot geomean non-positive $name value '$text'." }
        $logSum += [Math]::Log($value)
        $count++
    }
    if ($Required -and -not $found) { throw "Raw suite CSV has no '$name' column." }
    if ($count -eq 0) { return 0.0 }
    return [Math]::Exp($logSum / $count)
}

# Geomean of a per-program ratio: sum the numerator columns within each program
# (summing across programs is what is banned), divide by the denominator, then geomean.
function Get-RatioGeomean($rows, [string[]]$numerators, [string]$denominator) {
    $logSum = 0.0
    $count = 0
    foreach ($row in $rows) {
        $n = 0.0
        foreach ($column in $numerators) { $n += [double]"$($row.PSObject.Properties[$column].Value)" }
        $d = [double]"$($row.PSObject.Properties[$denominator].Value)"
        if ($n -le 0.0 -or $d -le 0.0) { continue }
        $logSum += [Math]::Log($n / $d)
        $count++
    }
    if ($count -eq 0) { return 0.0 }
    return [Math]::Exp($logSum / $count)
}

function Get-BenchTier([string]$suiteDirectory, [string]$name) {
    foreach ($extension in @(".rs", ".hex")) {
        $file = Join-Path $suiteDirectory ($name + $extension)
        if (-not (Test-Path $file)) { continue }
        $head = @(Get-Content -LiteralPath $file -TotalCount 8) -join "`n"
        $match = [regex]::Match($head, 'bench-tier:\s*(\S+)')
        if ($match.Success) { return $match.Groups[1].Value }
    }
    return "unknown"
}

if (-not $Project) {
    $projectsRoot = Join-Path $repoRoot ".agent/projects"
    $active = @()
    if (Test-Path $projectsRoot) {
        $active = @(Get-ChildItem -Path $projectsRoot -Directory | Where-Object {
                $meta = Join-Path $_.FullName "project.md"
                (Test-Path $meta) -and (Select-String -Path $meta -Pattern '^status:\s*active' -Quiet)
            })
    }
    if ($active.Count -ne 1) {
        throw "Cannot resolve the active project: $($active.Count) found with 'status: active' under .agent/projects. Pass -Project <name>."
    }
    $Project = $active[0].Name
}

Push-Location $repoRoot
try {
    $projectRoot = Join-Path $repoRoot ".agent/projects/$Project"
    if (-not (Test-Path $projectRoot)) { throw "Project directory not found: $projectRoot" }
    $ledger = Join-Path $projectRoot "performance.csv"
    $archiveDir = Join-Path $projectRoot "records/performance"
    $suiteDir = Join-Path $repoRoot "systems/cpu-v3-tang-nano-20k/benchmarks/suite"
    if (-not (Test-Path $suiteDir)) { throw "Frozen suite not found: $suiteDir" }

    # --- 1. Get the raw suite CSV ----------------------------------------------
    $dirty = (@(& git status --porcelain) -join "").Trim().Length -ne 0
    if ($RawCsv) {
        $rawPath = (Resolve-Path -LiteralPath $RawCsv).Path
        Write-Host "Reusing raw suite CSV: $rawPath"
    } else {
        if ($SuiteDigest) { throw "-SuiteDigest is only for importing an existing -RawCsv." }
        if ($dirty -and $State -ne "dirty") {
            if ($DryRun) {
                Write-Warning "Worktree is not clean; previewing anyway because -DryRun writes nothing."
            } else {
                throw "Worktree is not clean; the row would not describe a committed tree. Commit first, or pass -State dirty to record a probe."
            }
        }
        $rawPath = Join-Path $repoRoot "target/bench-log-$([guid]::NewGuid().ToString('N').Substring(0,8)).csv"
        $measuredSuite = Get-SuiteDigest $suiteDir
        Write-Host "Running the frozen suite (run label stage$StageLabel)..."
        & (Join-Path $repoRoot "systems/cpu-v3-tang-nano-20k/benchmarks/run-suite.ps1") `
            -Stage $StageLabel `
            -OutputFile $rawPath
        if ($LASTEXITCODE -ne 0) { throw "Benchmark suite failed; nothing was recorded." }
        if (-not (Test-Path $rawPath)) { throw "Suite run produced no CSV at $rawPath" }
        if ((Get-SuiteDigest $suiteDir) -ne $measuredSuite) {
            throw "Suite sources changed during measurement; nothing was recorded."
        }
    }

    # --- 2. Validate the raw CSV and read its identity --------------------------
    $rows = @(Import-Csv -LiteralPath $rawPath)
    if ($rows.Count -eq 0) { throw "Raw suite CSV has no program rows: $rawPath" }
    $commits = @($rows | ForEach-Object { "$($_.commit)" } | Sort-Object -Unique)
    if ($commits.Count -ne 1) {
        throw "Raw suite CSV mixes commits ($($commits -join ', ')); it is not one tree's measurement."
    }
    $commit = $commits[0]
    if ([string]::IsNullOrWhiteSpace($commit)) { throw "Raw suite CSV has an empty commit column; rerun the suite." }
    if ($RawCsv) {
        if ($SuiteDigest) {
            $measuredSuite = $SuiteDigest
        } else {
            if ($State -eq "dirty" -or @($rows | Where-Object { $_.config -eq "reconstructed" }).Count -ne 0) {
                throw "A dirty/reconstructed CSV does not identify its source suite; supply -SuiteDigest from the measured run."
            }
            Assert-SuitePrograms $rows $suiteDir $commit
            $measuredSuite = Get-SuiteDigest $suiteDir $commit
        }
    } else {
        Assert-SuitePrograms $rows $suiteDir
    }

    $authorDate = ""
    $log = @(& git show -s --format=%aI $commit 2>$null)
    if ($LASTEXITCODE -eq 0 -and $log.Count -gt 0) { $authorDate = ([string]$log[0]).Trim() }
    if ($authorDate) {
        # Same 06:00 day boundary as the journal, so a row matches its daily entry.
        $date = [datetime]::Parse($authorDate).AddHours(-6).ToString("yyyy-MM-dd")
    } else {
        $date = (Get-Date).AddHours(-6).ToString("yyyy-MM-dd")
        Write-Warning "Cannot resolve the author date of $commit; using today ($date)."
    }

    if (Test-Path $ledger) {
        $existing = @(Import-Csv -LiteralPath $ledger)
        $duplicate = $existing | Where-Object { $_.commit -eq $commit -and $_.state -eq $State }
        if (-not $Force -and $duplicate) {
            if ($DryRun) {
                Write-Warning "$commit ($State) is already recorded in $ledger; previewing anyway."
            } else {
                throw "$commit ($State) is already recorded in $ledger. Pass -Force to record a determinism re-run."
            }
        }
    }

    # --- 3. Aggregate (geomean over programs; never summed, see benchmarks/README.md) --
    $cycles = Get-ColumnGeomean $rows "cycles" -Required
    $retired = Get-ColumnGeomean $rows "retired_instructions" -Required
    # Zero-numerator programs (no fetch wait / no data traffic) are excluded from
    # that percentage's geomean rather than smoothed.
    $fetchWaitPct = 100.0 * (Get-RatioGeomean $rows @("fetch_wait_cycles") "cycles")
    $dataPathPct = 100.0 * (Get-RatioGeomean $rows @("data_request_cycles", "data_response_cycles") "cycles")
    $cpi = Get-RatioGeomean $rows @("cycles") "retired_instructions"
    $icacheRefills = Get-ColumnGeomean $rows "icache_demand_refills" -PlusOne
    $dcacheRefills = Get-ColumnGeomean $rows "dcache_refills" -Required -PlusOne
    $dcacheWritebacks = Get-ColumnGeomean $rows "dcache_writebacks" -Required -PlusOne

    $stage = "$($rows[0].stage)"
    $label = if ($stage) { "stage$stage" } else { "suite" }
    $baseName = "$date-$commit-$label"
    $archivePath = Join-Path $archiveDir "$baseName.csv"
    $suffix = 2
    while (Test-Path $archivePath) {
        $archivePath = Join-Path $archiveDir "$baseName-$suffix.csv"
        $suffix++
    }
    $relativeRaw = ".agent/projects/$Project/records/performance/" + (Split-Path -Leaf $archivePath)

    $row = [ordered]@{
        date                       = $date
        commit                     = $commit
        state                      = $State
        suite                      = $measuredSuite
        schema                     = Get-SchemaDigest $rawPath
        programs                   = $rows.Count
        cycles_geomean             = $cycles.ToString("F1", $inv)
        retired_geomean            = $retired.ToString("F1", $inv)
        cpi_geomean                = $cpi.ToString("F4", $inv)
        fetch_wait_pct_geomean     = $fetchWaitPct.ToString("F3", $inv)
        data_path_pct_geomean      = $dataPathPct.ToString("F3", $inv)
        icache_refills_geomean1    = $icacheRefills.ToString("F2", $inv)
        dcache_refills_geomean1    = $dcacheRefills.ToString("F2", $inv)
        dcache_writebacks_geomean1 = $dcacheWritebacks.ToString("F2", $inv)
        change                     = $Change
        raw                        = $relativeRaw
        promotion                  = $Promotion
    }
    $line = (($ledgerColumns | ForEach-Object { Quote-CsvField ([string]$row[$_]) }) -join ",")

    # --- 4. Breakdown (for the console and for the analysis document) -----------
    # Groups are the programs' own `bench-tier` metadata, which is what the harness reports.
    # The `fpu-*` subset cuts across those tiers (its programs declare short/medium/stress),
    # so it is printed separately rather than invented as a sixth tier.
    function Get-GroupLine([string]$label, $group) {
        $groupCycles = Get-ColumnGeomean $group "cycles"
        $groupRetired = Get-ColumnGeomean $group "retired_instructions"
        $groupCpi = Get-RatioGeomean $group @("cycles") "retired_instructions"
        $groupFetch = 100.0 * (Get-RatioGeomean $group @("fetch_wait_cycles") "cycles")
        $groupData = 100.0 * (Get-RatioGeomean $group @("data_request_cycles", "data_response_cycles") "cycles")
        return "| {0} | {1} | {2} | {3} | {4} | {5} | {6} |" -f `
            $label, $group.Count, $groupCycles.ToString("F1", $inv), $groupRetired.ToString("F1", $inv),
            $groupCpi.ToString("F4", $inv), $groupFetch.ToString("F2", $inv), $groupData.ToString("F2", $inv)
    }

    $tierNames = @{}
    foreach ($programRow in $rows) { $tierNames["$($programRow.name)"] = Get-BenchTier $suiteDir "$($programRow.name)" }
    $groupLines = foreach ($tier in @("short", "medium", "long", "frame", "stress", "unknown")) {
        $group = @($rows | Where-Object { $tierNames["$($_.name)"] -eq $tier })
        if ($group.Count -eq 0) { continue }
        Get-GroupLine $tier $group
    }
    $fpuSubset = @($rows | Where-Object { "$($_.name)" -like 'fpu-*' })
    if ($fpuSubset.Count -gt 0) { $groupLines += Get-GroupLine "fpu-* subset" $fpuSubset }

    if ($DryRun) {
        Write-Host ""
        Write-Host "(-DryRun, nothing written)"
        Write-Host "  ledger : $ledger"
        Write-Host "  archive: $archivePath"
        Write-Host $line
    } else {
        New-Item -ItemType Directory -Force -Path $archiveDir | Out-Null
        Copy-Item -LiteralPath $rawPath -Destination $archivePath -Force
        if (-not (Test-Path $ledger)) {
            [System.IO.File]::WriteAllText($ledger, (($ledgerColumns -join ",") + "`n"),
                (New-Object System.Text.UTF8Encoding($false)))
        }
        [System.IO.File]::AppendAllText($ledger, $line + "`n", (New-Object System.Text.UTF8Encoding($false)))
        Write-Host ""
        Write-Host "Archived $archivePath"
        Write-Host "Appended to $ledger"
        Write-Host $line
    }

    Write-Host ""
    Write-Host "Per-group geometric means (equal weight per program, never summed):"
    Write-Host "| group | programs | cycles | retired | CPI | fetch-wait % | data-path % |"
    Write-Host "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"
    $groupLines | ForEach-Object { Write-Host $_ }
    if (-not $DryRun) {
        Write-Host ""
        Write-Host "Now record what this delta means in records/performance-analysis.md,"
        Write-Host "and update today's diary (see .agent/logs.md, 'when to write')."
    }
} finally {
    Pop-Location
}
