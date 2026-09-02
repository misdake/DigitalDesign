param(
    [Parameter()]
    [string]$ProgramsDirectory = (Join-Path $PSScriptRoot "suite"),

    [Parameter(Mandatory = $true)]
    [int]$Stage,

    [string]$OutputFile = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
$programRoot = (Resolve-Path $ProgramsDirectory).Path
$traceRoot = Join-Path $repoRoot "target/cpu-v3-bench-stage$Stage"
if ([string]::IsNullOrWhiteSpace($OutputFile)) {
    $OutputFile = Join-Path $repoRoot "target/stage$Stage-results.csv"
}
$outputParent = Split-Path -Parent $OutputFile
if (-not [string]::IsNullOrWhiteSpace($outputParent)) {
    New-Item -ItemType Directory -Force -Path $outputParent | Out-Null
}

$programNames = @(
    Get-ChildItem -LiteralPath $programRoot -File |
        Where-Object { $_.Extension -in @(".rs", ".hex") } |
        ForEach-Object { $_.BaseName } |
        Sort-Object -Unique
)
if ($programNames.Count -eq 0) {
    throw "No .rs or .hex benchmark programs found in $programRoot"
}

$oldBenchDirectory = $env:CPU_V3_BENCH_DIR
$oldBenchOutput = $env:CPU_V3_BENCH_OUTPUT
try {
    $env:CPU_V3_BENCH_DIR = $programRoot
    $env:CPU_V3_BENCH_OUTPUT = $traceRoot
    Push-Location $repoRoot
    try {
        & scripts/run-cargo.ps1 `
            -Subcommand test `
            -Label "stage$Stage benchmark suite" `
            -CargoArgs @(
                "-p", "cpu-v3-tang-nano-20k",
                "--test", "bench_emu",
                "tests::benchmark_suite::run_benchmark_directory",
                "--release",
                "--", "--ignored", "--exact", "--nocapture"
            )
        if ($LASTEXITCODE -ne 0) {
            throw "Stage $Stage benchmark suite failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }

    $commit = (& git -C $repoRoot rev-parse --short HEAD).Trim()
    & (Join-Path $PSScriptRoot "export-results.ps1") `
        -Stage $Stage `
        -Commit $commit `
        -InputRoot $traceRoot `
        -OutputFile $OutputFile `
        -IncludeNames $programNames
    Write-Host "Wrote $OutputFile"
} finally {
    $env:CPU_V3_BENCH_DIR = $oldBenchDirectory
    $env:CPU_V3_BENCH_OUTPUT = $oldBenchOutput
}
