param(
    [Parameter(Mandatory = $true)]
    [string]$Subcommand,

    [string]$Label = "",

    [string]$LogDirectory = "",

    [string[]]$CargoArgs = @()
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($LogDirectory)) {
    $LogDirectory = Join-Path $repoRoot "target/cargo-summaries"
}
New-Item -ItemType Directory -Force -Path $LogDirectory | Out-Null

$displayName = if ([string]::IsNullOrWhiteSpace($Label)) { $Subcommand } else { $Label }
$safeName = (($displayName -replace '[^A-Za-z0-9_-]+', '-').Trim('-')).ToLowerInvariant()
if ([string]::IsNullOrWhiteSpace($safeName)) { $safeName = $Subcommand }
$logPath = Join-Path $LogDirectory "$safeName.log"
$summaryPath = Join-Path $LogDirectory "$safeName.json"
$relativeLogPath = $logPath.Substring($repoRoot.Length + 1).Replace('\', '/')

$ErrorActionPreference = "Continue"
$output = & cargo $Subcommand @CargoArgs 2>&1
$exitCode = $LASTEXITCODE
$ErrorActionPreference = "Stop"
if ($null -eq $exitCode) { $exitCode = 1 }

$lines = @()
if ($null -ne $output) {
    $lines = @($output | ForEach-Object { ("$_") -replace "`e\[[0-9;]*[A-Za-z]", "" })
}

if ($lines.Count -eq 0) {
    New-Item -ItemType File -Force -Path $logPath | Out-Null
} else {
    $lines | Set-Content -LiteralPath $logPath -Encoding UTF8
}

$warningCount = @($lines | Where-Object { $_ -match '^warning:' }).Count
$errorCount = @($lines | Where-Object { $_ -match '^error(:|\[)' }).Count

$summary = [ordered]@{
    schema = 1
    subcommand = $Subcommand
    label = $displayName
    exit_code = $exitCode
    ok = ($exitCode -eq 0)
    log_path = $relativeLogPath
    warnings = $warningCount
    errors = $errorCount
}

Write-Output ""
Write-Output "== cargo $Subcommand $($CargoArgs -join ' ') =="
Write-Output "log:    $relativeLogPath"
Write-Output "result: $(if ($exitCode -eq 0) { 'PASS' } else { 'FAIL' })  exit=$exitCode"
Write-Output "warnings: $warningCount  errors: $errorCount"

switch ($Subcommand) {
    "test" {
        $passed = 0
        $failed = 0
        $ignored = 0
        $measured = 0
        $filtered = 0
        $suites = 0
        foreach ($line in $lines) {
            if ($line -match '^test result: (?<status>\w+)[.] (?<passed>\d+) passed; (?<failed>\d+) failed; (?<ignored>\d+) ignored; (?<measured>\d+) measured; (?<filtered>\d+) filtered out;') {
                $passed += [int]$Matches['passed']
                $failed += [int]$Matches['failed']
                $ignored += [int]$Matches['ignored']
                $measured += [int]$Matches['measured']
                $filtered += [int]$Matches['filtered']
                $suites++
            }
        }
        $failures = [System.Collections.Generic.List[string]]::new()
        for ($i = $lines.Count - 1; $i -ge 0; $i--) {
            if ($lines[$i] -match '^failures:$') {
                for ($j = $i + 1; $j -lt $lines.Count; $j++) {
                    $line = $lines[$j]
                    if ($line -match '^test result:') { break }
                    if ($line -match '^\s+(\S.*)$') {
                        $name = $Matches[1].Trim()
                        if ($name -ne '' -and $name -notmatch '^----') { $failures.Add($name) }
                    }
                }
                break
            }
        }
        $summary.Add("tests_passed", $passed)
        $summary.Add("tests_failed", $failed)
        $summary.Add("tests_ignored", $ignored)
        $summary.Add("test_suites", $suites)
        $summary.Add("failures", @($failures))
        Write-Output "tests:  $passed passed / $failed failed / $ignored ignored ($suites suites)"
        if ($failures.Count -gt 0) {
            Write-Output "failures:"
            foreach ($f in $failures) { Write-Output "  - $f" }
        }
    }
    "run" {
        $tailCount = if ($exitCode -ne 0) { 40 } else { 20 }
        $tail = @($lines | Select-Object -Last $tailCount)
        $summary.Add("tail", @($tail))
        Write-Output "tail:"
        foreach ($t in $tail) { Write-Output "  $t" }
    }
    default {
        $finished = ($lines | Where-Object { $_ -match '^\s*Finished ' } | Select-Object -Last 1)
        if ($finished) { $summary.Add("finished", $finished.Trim()) }
        $diagnostics = @($lines | Where-Object { $_ -match '^warning:' -or $_ -match '^error(:|\[)' -or $_ -match '^\s*-->' } | Select-Object -First 30)
        $summary.Add("diagnostics", @($diagnostics))
        if ($finished) { Write-Output $finished }
        if ($diagnostics.Count -gt 0) {
            Write-Output "diagnostics:"
            foreach ($d in $diagnostics) { Write-Output "  $d" }
        }
    }
}

if ($exitCode -ne 0 -and $summary["diagnostics"].Count -eq 0) {
    $failureTail = @($lines | Where-Object { $_ -match '^warning:' -or $_ -match '^error(:|\[)' -or $_ -match '^\s*-->' } | Select-Object -First 30)
    $summary["diagnostics"] = @($failureTail)
    if ($failureTail.Count -gt 0) {
        Write-Output "diagnostics:"
        foreach ($d in $failureTail) { Write-Output "  $d" }
    }
}

$summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $summaryPath -Encoding UTF8
Write-Output "summary: $($summaryPath.Substring($repoRoot.Length + 1).Replace('\', '/'))"

exit $exitCode
