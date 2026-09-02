# Runs the frozen benchmark images against one commit or tag and exports one CSV.
#
# HEAD runs the suite from source with the current harness. Any other -Ref is a
# historical commit/tag (normally a stageN-bench tag): a temporary worktree runs
# its own harness against the prebuilt hex images, because historical commits
# predate the current compiler.
param(
    [Parameter()]
    [string]$Ref = "HEAD",

    [Parameter()]
    [string]$Label = "",

    [Parameter()]
    [string]$Images = "",

    [Parameter()]
    [string]$OutDir = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
if ($Label -eq "") { $Label = ($Ref -replace "[^a-zA-Z0-9-]", "-") }
if ($Images -eq "") { $Images = Join-Path $repoRoot "target/bench-history/images" }
if ($OutDir -eq "") { $OutDir = Join-Path $repoRoot "target/bench-history" }
$traceRoot = Join-Path $OutDir "traces-$Label"
$outFile = Join-Path $OutDir "$Label.csv"

if (-not (Test-Path (Join-Path $Images "*.hex"))) {
    throw "no hex images under $Images (build them with cpu-v3-bench-images first)"
}

$commit = if ($Ref -eq "HEAD") {
    (& git -C $repoRoot rev-parse --short HEAD).Trim()
} else {
    (& git -C $repoRoot rev-parse --short "$Ref^{}").Trim()
}
$isCurrent = $Ref -eq "HEAD"
$config = if ($isCurrent) { "current" } else { "reconstructed" }

$worktree = Join-Path $OutDir "wt-$Label"
$oldBenchDirectory = $env:CPU_V3_BENCH_DIR
$oldBenchOutput = $env:CPU_V3_BENCH_OUTPUT
try {
    if ($isCurrent) {
        $suiteDir = Join-Path $repoRoot "systems/cpu-v3-tang-nano-20k/benchmarks/suite"
        $env:CPU_V3_BENCH_DIR = $suiteDir
    } else {
        if (Test-Path $worktree) {
            # clean up an interrupted previous run instead of failing
            $prevEap = $ErrorActionPreference
            $ErrorActionPreference = "Continue"
            & git -C $repoRoot worktree remove -f -f $worktree 2>$null | Out-Null
            $ErrorActionPreference = $prevEap
            if (Test-Path $worktree) { throw "leftover worktree $worktree" }
        }
        # git writes worktree progress to stderr; keep ErrorActionPreference
        # from turning that into a terminating NativeCommandError
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        & git -C $repoRoot worktree add $worktree $Ref 2>$null | Out-Null
        $gitExit = $LASTEXITCODE
        $ErrorActionPreference = $prevEap
        if ($gitExit -ne 0) { throw "git worktree add $Ref failed" }
        $env:CPU_V3_BENCH_DIR = (Resolve-Path $Images).Path
    }
    $env:CPU_V3_BENCH_OUTPUT = $traceRoot
    $cargoRoot = if ($isCurrent) { $repoRoot } else { $worktree }
    Push-Location $cargoRoot
    try {
        & (Join-Path $repoRoot "scripts/run-cargo.ps1") `
            -Subcommand test `
            -Label "bench history $Label" `
            -CargoArgs @(
                "-p", "cpu-v3-tang-nano-20k",
                "--test", "bench_emu",
                "tests::benchmark_suite::run_benchmark_directory",
                "--release",
                "--", "--ignored", "--exact", "--nocapture"
            )
        if ($LASTEXITCODE -ne 0) {
            throw "benchmark run at $Ref failed with exit code $LASTEXITCODE (see target/cargo-summaries/bench history $Label.log)"
        }
    } finally {
        Pop-Location
    }

    & (Join-Path $PSScriptRoot "export-results.ps1") `
        -Stage $Label `
        -Commit $commit `
        -ConfigLabel $config `
        -InputRoot $traceRoot `
        -OutputFile $outFile
    Write-Host "wrote $outFile"
} finally {
    $env:CPU_V3_BENCH_DIR = $oldBenchDirectory
    $env:CPU_V3_BENCH_OUTPUT = $oldBenchOutput
    if (-not $isCurrent -and (Test-Path $worktree)) {
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        & git -C $repoRoot worktree remove -f -f $worktree 2>$null | Out-Null
        $ErrorActionPreference = $prevEap
    }
}
