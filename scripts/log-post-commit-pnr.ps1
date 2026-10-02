# Run this only right after a commit: rebuild the full system and append one PnR row
# to the project ledger (.agent/projects/<project>/resources.csv).
#
# Why: that CSV records "the complete build of a committed tree". Copying the numbers by
# hand is slow, error-prone, and easy to forget. This script does four things:
#   1. preconditions: the worktree must be clean (only committed builds are recorded), and
#      after the rebuild the PnR report must be newer than HEAD;
#   2. rebuild the full system through scripts/run-cargo.ps1 (includes the Gowin audits);
#   3. split the total Logic across the major components using the synthesis hierarchy
#      report (a component's Logic = LUT + ALU + 6 x SSRAM over its whole subtree, the same
#      rule resource-analysis.md uses);
#   4. archive the reports the row was read from under records/resources/ and append the row.
#
# Do not run it casually: it is only meaningful for a freshly committed complete build, and
# WIP or intermediate commits must not be recorded.
#
# Usage:
#   scripts/log-post-commit-pnr.ps1 -Change "cache valid-array write port" `
#       [-Promotion "ledger:Cache valid-array write port"] [-State dirty] [-Project <name>]
#   # record a probe build that lives somewhere other than target/cpu_v3_system_gowin:
#   scripts/log-post-commit-pnr.ps1 -Change "..." -BuildDir target/valid-leaf-probe/system_valid_fix
# Inspect what it would write first:
#   scripts/log-post-commit-pnr.ps1 -Change "..." -SkipBuild -DryRun

param(
    [Parameter(Mandatory = $true)][string]$Change,
    # Empty means "resolve the active project from .agent/README.md's project table".
    [string]$Project = "",
    [string]$Promotion = "exploratory",
    [string]$State = "clean",
    # Exported Gowin project directory that holds impl/pnr and impl/gwsynthesis.
    [string]$BuildDir = "target/cpu_v3_system_gowin",
    [switch]$SkipBuild,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

if (-not $Project) {
    # The active project is the one whose project.md says `status: active`.
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

# Count physical blocks, not ROM/retention roles. A primitive's name does not
# identify its purpose. Percentage-only PnR headers have no numeric total;
# optional Gowin synthesis XML supplies an independent cross-check in that case.
function Get-GowinBsramUsage([string]$text, [string]$SynthesisReportPath = "") {
    $kinds = [ordered]@{ SP = 0; SPX9 = 0; SDPB = 0; SDPX9B = 0;
        DPB = 0; DPX9B = 0; pROM = 0; pROMX9 = 0 }
    $lines = $text -split '\r?\n'
    $headers = @()
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^\s*BSRAM\s*\|\s*(.*?)\s*$') {
            $headers += [pscustomobject]@{ Index = $i; Usage = $Matches[1] }
        }
    }
    if ($headers.Count -ne 1) { throw "Expected one PnR BSRAM section; found $($headers.Count)." }
    $header = $headers[0]
    $pnrTotal = $null
    $percentage = $null
    if ($header.Usage -match '^(\d+)(?:/\d+)?(?:\s+(?:<\s*)?\d+%)?$') {
        $pnrTotal = [int]$Matches[1]
    } elseif ($header.Usage -match '^(<\s*)?(\d+)%$') {
        $percentage = [int]$Matches[2]
        if ($percentage -gt 100) { throw "Invalid PnR BSRAM percentage: $($header.Usage)." }
        if (-not $Matches[1] -and $percentage -eq 0) { $pnrTotal = 0 }
    } else { throw "Unrecognized PnR BSRAM usage: $($header.Usage)." }

    $seen = @{}
    for ($i = $header.Index + 1; $i -lt $lines.Count; $i++) {
        if ([string]::IsNullOrWhiteSpace($lines[$i])) { continue }
        if ($lines[$i] -notmatch '^\s*--([^|]+)\|\s*(.*?)\s*$') {
            if ($lines[$i] -match '^\s*--') { throw "Malformed BSRAM primitive row: $($lines[$i])." }
            break
        }
        $kind = $Matches[1].Trim()
        $usage = $Matches[2]
        if (-not $kinds.Contains($kind)) { throw "Unknown BSRAM primitive kind: $kind." }
        if ($seen.ContainsKey($kind)) { throw "Duplicate BSRAM primitive kind: $kind." }
        if ($usage -notmatch '^(\d+)(?:/\d+)?(?:\s+(?:<\s*)?\d+%)?$') {
            throw "Invalid BSRAM primitive count for ${kind}: $usage."
        }
        $kinds[$kind] = [int]$Matches[1]
        $seen[$kind] = $true
    }
    $total = [int](($kinds.Values | Measure-Object -Sum).Sum)
    if ($seen.Count -eq 0 -and $pnrTotal -ne 0) {
        throw "PnR BSRAM section has no primitive counts."
    }
    if ($null -ne $pnrTotal -and $total -ne $pnrTotal) {
        throw "PnR BSRAM total mismatch: header=$pnrTotal, primitive sum=$total."
    }
    if ($null -ne $percentage -and $percentage -gt 0 -and $total -eq 0) {
        throw "Nonzero PnR BSRAM percentage has zero primitive sum."
    }

    $synthesisTotal = $null
    if ($SynthesisReportPath) {
        if (-not (Test-Path -LiteralPath $SynthesisReportPath)) {
            throw "Synthesis BSRAM report not found: $SynthesisReportPath."
        }
        $document = New-Object System.Xml.XmlDocument
        $document.Load($SynthesisReportPath)
        if ($document.DocumentElement.get_LocalName() -ne "Module") {
            throw "Unknown Gowin synthesis hierarchy schema: $SynthesisReportPath."
        }
        # Gowin Module/SubModule attributes are OWN counts, not inclusive
        # subtree totals (the component Logic parser uses the same convention).
        # Visit each node exactly once; do not sum a separately computed subtree
        # total again at its parent. Inclusive/other XML must fail the PnR check.
        $synthesisTotal = 0
        foreach ($node in $document.SelectNodes("/Module | /Module//SubModule")) {
            $value = $node.GetAttribute("Bsram")
            if ($value -eq "") { continue }
            if ($value -notmatch '^\d+$') { throw "Invalid synthesis Bsram count: $value." }
            $synthesisTotal += [int]$value
        }
        if ($synthesisTotal -ne $total) {
            throw "BSRAM synthesis/PnR mismatch: synthesis=$synthesisTotal, PnR primitive sum=$total."
        }
    }
    return [pscustomobject]@{ Total = $total; ByKind = $kinds;
        PnrUsage = $header.Usage; PnrTotal = $pnrTotal; SynthesisTotal = $synthesisTotal }
}

# The synthesis hierarchy report lists every module with its *own* numbers, so a component is
# the sum over its whole subtree (accumulating recovers the totals resource-analysis.md quotes:
# 3,902 registers, 7,593 LUT). Modules that are not under a named root are board glue. A
# component's Logic is LUT + ALU + 6 x SSRAM unit, the same rule the analysis document uses;
# it is the *synthesis* split, so the components do not add up to the PnR total (Gowin divides
# LUT/ALU differently in the two stages).
$componentRoots = [ordered]@{
    core       = @("u_core")
    icache     = @("u_instruction_fetch_queue", "u_instruction_cache")
    dcache     = @("u_data_cache")
    sdram_port = @("u_shared_sdram_port")
    display    = @("u_display")
    boot       = @("u_boot", "u_boot_dma_engine", "u_boot_dma_device")
    flash      = @("u_flash")
    # Keep the core-only category across the legacy and native wrappers.
    sdram_ctrl = @("u_sdram_controller", "u_controller")
    sysctl     = @("u_sysctl", "u_memory_arbiter")
}

function Get-NodeNumber($node, [string]$attribute) {
    $value = $node.GetAttribute($attribute)
    if ([string]::IsNullOrWhiteSpace($value)) { return 0 }
    return [int]$value
}

function Add-SynthesisNode($node, [string]$component, $totals) {
    # Only look for a component root while none has been matched yet; once inside one, the
    # whole subtree belongs to it. A node that matches nothing falls into `glue`, but its
    # children are still checked (the root module itself is glue, its submodules are not).
    if (-not $component) {
        $name = $node.GetAttribute("name")
        foreach ($key in $script:componentRoots.Keys) {
            if ($script:componentRoots[$key] -contains $name) { $component = $key; break }
        }
    }
    $bucket = if ($component) { $component } else { "glue" }
    $totals[$bucket].Lut += Get-NodeNumber $node "Lut"
    $totals[$bucket].Alu += Get-NodeNumber $node "Alu"
    $totals[$bucket].Ssram += Get-NodeNumber $node "Ssram"
    foreach ($child in $node.SelectNodes("SubModule")) {
        Add-SynthesisNode $child $component $totals
    }
}

# Returns an ordered map component -> Logic, keyed exactly like $componentRoots plus `glue`.
function Get-ComponentLogic([string]$reportPath) {
    if (-not (Test-Path $reportPath)) { throw "Synthesis hierarchy report not found: $reportPath" }
    $document = New-Object System.Xml.XmlDocument
    $document.Load($reportPath)
    $totals = [ordered]@{}
    foreach ($key in $componentRoots.Keys) { $totals[$key] = @{ Lut = 0; Alu = 0; Ssram = 0 } }
    $totals["glue"] = @{ Lut = 0; Alu = 0; Ssram = 0 }
    Add-SynthesisNode $document.DocumentElement "" $totals
    $logic = [ordered]@{}
    foreach ($key in $totals.Keys) {
        $logic[$key] = $totals[$key].Lut + $totals[$key].Alu + 6 * $totals[$key].Ssram
    }
    return $logic
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

    $csv = Join-Path $repoRoot ".agent/projects/$Project/resources.csv"
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
    $buildRoot = (Resolve-Path -LiteralPath (Join-Path $repoRoot $BuildDir)).Path
    $pnr = Join-Path $buildRoot "impl/pnr/cpu_v3_system.rpt.txt"
    $tr = Join-Path $buildRoot "impl/pnr/cpu_v3_system.tr"
    $paths = Join-Path $buildRoot "impl/pnr/cpu_v3_system.timing_paths"
    $synRsc = Join-Path $buildRoot "impl/gwsynthesis/cpu_v3_system_syn_rsc.xml"
    $manifest = Join-Path $buildRoot "gowin-build.manifest"

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

    foreach ($file in @($pnr, $tr, $paths, $synRsc)) {
        if (-not (Test-Path $file)) { throw "Missing report $file; run a full-system build first." }
    }

    # --- 3. Keep the reports the row is read from -------------------------------
    # target/ is scratch: reports there get overwritten by the next build (an earlier row's
    # evidence is already gone). The archived copy is what a later reader can actually check.
    $archiveDir = Join-Path $repoRoot ".agent/projects/$Project/records/resources/$($commitDay)-$commit-pnr"
    $suffix = 2
    while (Test-Path $archiveDir) {
        $archiveDir = Join-Path $repoRoot ".agent/projects/$Project/records/resources/$($commitDay)-$commit-pnr-$suffix"
        $suffix++
    }
    $archiveFiles = @(
        @{ Source = $pnr; Name = "cpu_v3_system.rpt.txt" },
        @{ Source = $tr; Name = "cpu_v3_system.tr" },
        @{ Source = $paths; Name = "cpu_v3_system.timing_paths" },
        @{ Source = $synRsc; Name = "cpu_v3_system_syn_rsc.xml" }
    )
    if (Test-Path $manifest) {
        $archiveFiles += @{ Source = $manifest; Name = "gowin-build.manifest" }
    }
    $relativeEvidence = ".agent/projects/$Project/records/resources/" + (Split-Path -Leaf $archiveDir)

    # --- 4. Parse and append ---------------------------------------------------
    $text = Get-Content -Path $pnr -Raw
    $lutAlu = [regex]::Match($text, '--LUT,ALU,ROM16\s*\|\s*\d+\((\d+) LUT,\s*(\d+) ALU')
    if (-not $lutAlu.Success) { throw "PnR report has no LUT/ALU line." }
    $bsramUsage = Get-GowinBsramUsage $text $synRsc
    $slack = ""
    $lines = Get-Content -Path $paths
    for ($i = 0; $i -lt $lines.Count - 1; $i++) {
        if ($lines[$i].Trim() -eq "SETUP") { $slack = $lines[$i + 1].Trim(); break }
    }
    if (-not $slack) { throw "timing_paths has no SETUP slack." }
    $componentLogic = Get-ComponentLogic $synRsc

    $row = [ordered]@{
        date              = $commitDay
        commit            = $commit
        state             = $State
        logic             = Get-ReportField $text 'Logic\s*\|\s*(\d+)/' "Logic"
        logic_core        = $componentLogic["core"]
        logic_icache      = $componentLogic["icache"]
        logic_dcache      = $componentLogic["dcache"]
        logic_sdram_port  = $componentLogic["sdram_port"]
        logic_display     = $componentLogic["display"]
        logic_boot        = $componentLogic["boot"]
        logic_flash       = $componentLogic["flash"]
        logic_sdram_ctrl  = $componentLogic["sdram_ctrl"]
        logic_sysctl      = $componentLogic["sysctl"]
        logic_glue        = $componentLogic["glue"]
        lut               = $lutAlu.Groups[1].Value
        alu               = $lutAlu.Groups[2].Value
        ssram             = Get-ReportField $text 'SSRAM\(RAM16\)\s*\|\s*(\d+)' "SSRAM"
        ff                = Get-ReportField $text 'Logic Register as FF\s*\|\s*(\d+)/' "logic flip-flops"
        cls               = Get-ReportField $text 'CLS\s*\|\s*(\d+)/' "CLS"
        bsram             = $bsramUsage.Total
        dsp               = Get-ReportField $text '--MULT18X18\s*\|\s*(\d+)' "DSP"
        fmax_mhz          = Get-ReportField (Get-Content -Path $tr -Raw) 'cpu_clk\s+\S+\(MHz\)\s+([\d.]+)\(MHz\)' "cpu_clk fmax"
        slack_ns          = $slack
        change            = $Change
        evidence          = $relativeEvidence
        promotion         = $Promotion
    }

    $line = (($row.Values | ForEach-Object { Quote-CsvField ([string]$_) }) -join ",")
    if ($DryRun) {
        Write-Host ""
        Write-Host "(-DryRun, nothing written) $csv"
        Write-Host "  archive: $archiveDir"
        Write-Host $line
        Write-Host ("BSRAM physical blocks by kind: " + ($bsramUsage.ByKind | ConvertTo-Json -Compress))
        Write-Host ""
        Write-Host "component Logic (synthesis split; sums to $((($componentLogic.Values | Measure-Object -Sum).Sum))):"
        foreach ($key in $componentLogic.Keys) { Write-Host ("  {0,-12} {1}" -f $key, $componentLogic[$key]) }
        return
    }
    New-Item -ItemType Directory -Force -Path $archiveDir | Out-Null
    foreach ($entry in $archiveFiles) {
        Copy-Item -LiteralPath $entry.Source -Destination (Join-Path $archiveDir $entry.Name) -Force
    }
    # Preserve the complete kind breakdown without changing the historical CSV
    # schema. This is accounting evidence, not a semantic ROM/retention split.
    [System.IO.File]::WriteAllText((Join-Path $archiveDir "bsram-usage.json"),
        ($bsramUsage | ConvertTo-Json -Depth 4) + "`n", (New-Object System.Text.UTF8Encoding($false)))
    [System.IO.File]::AppendAllText($csv, $line + "`n", (New-Object System.Text.UTF8Encoding($false)))
    Write-Host ""
    Write-Host "Archived $archiveDir"
    Write-Host "Appended to $csv"
    Write-Host $line
    Write-Host ""
    Write-Host "Now update today's diary (see .agent/logs.md, 'when to write')."
} finally {
    Pop-Location
}
