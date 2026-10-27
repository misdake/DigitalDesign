# Documentation constraints for this repository:
#   1. line budget - a current-state section ("## Current ...") stays at most 40 lines;
#   2. one home    - a fitted number printed in one current-state section must not also be
#                    printed in another document's current-state section; the milestone ledger
#                    table and the generated reports are the other allowed homes;
#   3. local index - documents under .agent/ that are referenced by id carry
#                    id/status/last-verified, ids are unique, and each document is linked
#                    from the README in its own directory;
#   4. links       - markdown links in active documents resolve. Preserved daily/weekly
#                    logs are historical snapshots, not current navigation.
#
# "## Current implementation progress" is the milestone ledger, which the documentation rules
# keep separate from a current-state section, so headings naming progress or milestones are
# exempt from 1 and 2. Vendored third-party documents are out of scope.
#
# .agent/ is local and untracked: when it is absent (fresh clone), checks 3 and 4 skip the
# local tree instead of failing, because the tracked documents are then authoritative.
#
# Run: scripts/check-docs.ps1

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$agentRoot = Join-Path $repoRoot ".agent"
$currentSectionBudget = 40

$currentHeading = [regex]::new('^##\s+.*\bCurrent\b', [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
$ledgerHeading = [regex]::new('\b(progress|milestone)\b', [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
$fittedNumber = [regex]::new('\b\d{1,3}(?:,\d{3})+\b|\b\d+\.\d+\s*MHz\b')
$validStatus = @("active", "planned", "frozen", "retired")

$errors = [System.Collections.Generic.List[string]]::new()

function Get-DocumentLines([string]$Path) {
    return [System.IO.File]::ReadAllLines($Path)
}

function Resolve-DocumentationLink([string]$DocumentPath, [string]$Target) {
    $targetPath = [Uri]::UnescapeDataString(($Target -split '#', 2)[0].Trim())
    if ($targetPath.StartsWith('/')) {
        return [System.IO.Path]::GetFullPath((Join-Path $repoRoot $targetPath.TrimStart('/')))
    }
    return [System.IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $DocumentPath) $targetPath))
}

# --- 1 and 2: current-state sections -----------------------------------------
$trackedDocs = @(& git -C $repoRoot ls-files "*.md")
if ($LASTEXITCODE -ne 0) { throw "git ls-files failed" }

$numberHomes = @{}
foreach ($relativePath in $trackedDocs) {
    if ($relativePath -match '(^|/)vendor/') { continue }
    $path = Join-Path $repoRoot $relativePath
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
    $lines = Get-DocumentLines $path

    for ($index = 0; $index -lt $lines.Count; $index++) {
        if (-not $currentHeading.IsMatch($lines[$index])) { continue }
        if ($ledgerHeading.IsMatch($lines[$index])) { continue }
        $end = $lines.Count
        for ($scan = $index + 1; $scan -lt $lines.Count; $scan++) {
            if ($lines[$scan] -match '^##\s') { $end = $scan; break }
        }
        $bodyCount = $end - $index - 1
        if ($bodyCount -gt $currentSectionBudget) {
            $errors.Add("current-state section is $bodyCount lines (budget $currentSectionBudget): $relativePath`:$($index + 1) '$($lines[$index].Trim())'")
        }
        for ($scan = $index + 1; $scan -lt $end; $scan++) {
            foreach ($match in $fittedNumber.Matches($lines[$scan])) {
                $token = ($match.Value -replace '\s+', ' ')
                if (-not $numberHomes.ContainsKey($token)) { $numberHomes[$token] = [System.Collections.Generic.List[string]]::new() }
                if (-not $numberHomes[$token].Contains($relativePath)) { $numberHomes[$token].Add($relativePath) }
            }
        }
    }

    foreach ($line in $lines) {
        foreach ($match in [regex]::Matches($line, '\]\(([^)]+)\)')) {
            $target = $match.Groups[1].Value.Trim()
            if ($target -match '^(https?:|mailto:|#)') { continue }
            $target = ($target -split '#')[0].Trim()
            if (-not $target) { continue }
            if (-not (Test-Path -LiteralPath (Resolve-DocumentationLink $path $target))) {
                $errors.Add("markdown link does not resolve: $relativePath -> $target")
            }
        }
    }
}

foreach ($token in ($numberHomes.Keys | Sort-Object)) {
    if ($numberHomes[$token].Count -gt 1) {
        $errors.Add("fitted number '$token' appears in $($numberHomes[$token].Count) current-state sections: $($numberHomes[$token] -join ', ')")
    }
}

# --- 3 and 4: the local .agent tree ------------------------------------------
function Get-RelativeAgentPath([string]$Path) {
    return $Path.Substring($agentRoot.Length).TrimStart('\', '/') -replace '\\', '/'
}

function Test-IsLocalDocument([string]$RelativePath) {
    # Installed third-party dependencies are not authored project documents.
    if ($RelativePath -match '(^|/)(node_modules|vendor)/') { return $false }
    $name = Split-Path -Leaf $RelativePath
    if ($name -eq "README.md" -or $name -eq "todo.md") { return $false }
    if ($RelativePath -match '(^|/)(daily|weekly)/') { return $false }
    return $RelativePath.EndsWith(".md")
}

function Get-FrontMatter([string]$Path) {
    $lines = Get-DocumentLines $Path
    if ($lines.Count -lt 4 -or $lines[0].Trim() -ne "---") { return $null }
    $fields = @{}
    for ($index = 1; $index -lt $lines.Count; $index++) {
        if ($lines[$index].Trim() -eq "---") { break }
        $match = [regex]::Match($lines[$index], '^([a-z-]+):\s*(\S+)\s*$')
        if ($match.Success) { $fields[$match.Groups[1].Value] = $match.Groups[2].Value }
    }
    return $fields
}

function Test-DirectoryRegistration([string]$DocumentPath) {
    $directory = Split-Path -Parent $DocumentPath
    $indexPath = Join-Path $directory "README.md"
    if (-not (Test-Path -LiteralPath $indexPath -PathType Leaf)) { return $false }
    foreach ($line in (Get-DocumentLines $indexPath)) {
        foreach ($match in [regex]::Matches($line, '\]\(([^)]+)\)')) {
            $target = $match.Groups[1].Value.Trim()
            if ($target -match '^(https?:|mailto:|#)') { continue }
            $target = [Uri]::UnescapeDataString(($target -split '#')[0].Trim())
            if (-not $target) { continue }
            $full = Resolve-DocumentationLink $indexPath $target
            if ($full -eq [System.IO.Path]::GetFullPath($DocumentPath)) { return $true }
        }
    }
    return $false
}

if (Test-Path -LiteralPath $agentRoot -PathType Container) {
    $localIds = @{}
    $localDocuments = @{}
    foreach ($file in (Get-ChildItem $agentRoot -Recurse -File -Filter "*.md")) {
        $relative = Get-RelativeAgentPath $file.FullName
        if (-not (Test-IsLocalDocument $relative)) { continue }
        $localDocuments[$relative] = $true
        $front = Get-FrontMatter $file.FullName
        if ($null -eq $front) {
            $errors.Add("local document has no front matter: .agent/$relative")
            continue
        }
        if (-not $front.ContainsKey("id") -or -not $front["id"]) {
            $errors.Add("local document has no id: .agent/$relative")
        } elseif ($localIds.ContainsKey($front["id"])) {
            $errors.Add("duplicate id '$($front['id'])' in .agent/$relative and .agent/$($localIds[$front['id']])")
        } else {
            $localIds[$front["id"]] = $relative
        }
        if (-not $front.ContainsKey("status") -or $validStatus -notcontains $front["status"]) {
            $errors.Add("local document status must be one of $($validStatus -join '|'): .agent/$relative")
        }
        if (-not $front.ContainsKey("last-verified") -or $front["last-verified"] -notmatch '^\d{4}-\d{2}-\d{2}$') {
            $errors.Add("local document last-verified must be a date: .agent/$relative")
        }
    }

    foreach ($relative in ($localDocuments.Keys | Sort-Object)) {
        if (-not (Test-DirectoryRegistration (Join-Path $agentRoot $relative))) {
            $errors.Add("local document is not linked from its own directory's README.md: .agent/$relative")
        }
    }

    foreach ($file in (Get-ChildItem $agentRoot -Recurse -File -Filter "*.md")) {
        $relative = Get-RelativeAgentPath $file.FullName
        if ($relative -match '(^|/)(node_modules|vendor|daily|weekly)/') { continue }
        foreach ($line in (Get-DocumentLines $file.FullName)) {
            foreach ($match in [regex]::Matches($line, '\]\(([^)]+)\)')) {
                $target = $match.Groups[1].Value.Trim()
                if ($target -match '^(https?:|mailto:|#)') { continue }
                $target = ($target -split '#')[0].Trim()
                if (-not $target) { continue }
                if (-not (Test-Path -LiteralPath (Resolve-DocumentationLink $file.FullName $target))) {
                    $errors.Add("markdown link does not resolve: .agent/$relative -> $target")
                }
            }
        }
    }
}

if ($errors.Count -ne 0) {
    throw ($errors -join [Environment]::NewLine)
}

Write-Host "Documentation constraints passed."
