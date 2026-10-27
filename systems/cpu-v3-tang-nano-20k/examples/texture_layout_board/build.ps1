param(
    [ValidateSet('Export', 'Build', 'Audit')]
    [string]$Mode = 'Build',
    [string]$OutputDirectory = 'target/texture-column16-pure/board',
    [string]$BootExample = '',
    [ValidateSet('texture16x8-row','texture16x8-column16')]
    [string]$Profile = 'texture16x8-column16',
    [ValidateSet('production')]
    [string]$Implementation = 'production'
)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$outputPath = if ([IO.Path]::IsPathRooted($OutputDirectory)) {
    [IO.Path]::GetFullPath($OutputDirectory)
} else { [IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory)) }
$targetRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target')) + [IO.Path]::DirectorySeparatorChar
if (-not $outputPath.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Use an experiment output directory inside this worktree target/'
}
if ($Mode -ne 'Audit' -and (Test-Path -LiteralPath $outputPath)) {
    throw 'Export/Build requires a fresh output directory; use Audit for existing artifacts'
}
$savedProfile = $env:CPU_V3_GPU_PROFILE
$savedExample = $env:CPU_V3_BOOT_EXAMPLE
$savedSpi = $env:CPU_V3_SPI_HALF_PERIOD
$outcome = 'failed'
$failure = $null
Push-Location $repoRoot
try {
    $env:CPU_V3_GPU_PROFILE = $Profile
    $env:CPU_V3_SPI_HALF_PERIOD = '2'
    if ($BootExample -eq '') { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue }
    else { $env:CPU_V3_BOOT_EXAMPLE = $BootExample }
    $cargoArgs = @('-p', 'cpu-v3-tang-nano-20k', '--example', 'cpu_v3_system', '--', $outputPath)
    if ($Mode -eq 'Build') { $cargoArgs += '--build' }
    if ($Mode -eq 'Audit') { $cargoArgs += '--check-existing' }
    & scripts/run-cargo.ps1 -Subcommand run -Label "layout-$Profile-$Implementation-$Mode-$(Split-Path -Leaf $outputPath)" -CargoArgs $cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Layout board $Mode failed; inspect target/cargo-summaries/" }
    $outcome = 'passed'
} catch {
    $failure = $_.Exception.Message
    throw
} finally {
    if (Test-Path -LiteralPath $outputPath) {
        $hashes = [ordered]@{}
        foreach ($name in @('gowin-build.manifest', 'impl/pnr/cpu_v3_system.fs', 'impl/pnr/cpu_v3_system.rpt.txt', 'impl/pnr/cpu_v3_system.tr')) {
            $path = Join-Path $outputPath $name
            if (Test-Path -LiteralPath $path) { $hashes[$name] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
        }
        $receipt = [ordered]@{
            status = $outcome; mode = $Mode; hardware_profile = $Profile; implementation = $Implementation;
            source_commit = (& git rev-parse HEAD); branch = (& git branch --show-current);
            dirty = [bool](& git status --porcelain); spi_half_period = 2;
            boot_example = $(if ($BootExample -eq '') { 'boot-applications.conf' } else { $BootExample });
            failure = $failure; hashes_sha256 = $hashes;
            scope = 'Offline complete CPU/GPU/Display/MC board export and fit. No programming, UART or capture. Default firmware is a CPU Display diagnostic; matching layout firmware/static assets are required before device use. RTT is disabled.'
        }
        $receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $outputPath "layout-$($Mode.ToLowerInvariant())-receipt.json") -Encoding utf8
    }
    if ($null -eq $savedProfile) { Remove-Item Env:CPU_V3_GPU_PROFILE -ErrorAction SilentlyContinue } else { $env:CPU_V3_GPU_PROFILE = $savedProfile }
    if ($null -eq $savedExample) { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue } else { $env:CPU_V3_BOOT_EXAMPLE = $savedExample }
    if ($null -eq $savedSpi) { Remove-Item Env:CPU_V3_SPI_HALF_PERIOD -ErrorAction SilentlyContinue } else { $env:CPU_V3_SPI_HALF_PERIOD = $savedSpi }
    Pop-Location
}
