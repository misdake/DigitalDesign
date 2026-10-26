param([string]$OutputDirectory = 'target/gpu-renderer-boot')
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$savedExample = $env:CPU_V3_BOOT_EXAMPLE
Push-Location $repoRoot
try {
    $env:CPU_V3_BOOT_EXAMPLE = 'gpu_renderer'
    & scripts/run-cargo.ps1 -Subcommand run -Label 'gpu-renderer-boot' -CargoArgs @(
        '-p', 'cpu-v3-tang-nano-20k', '--bin', 'cpu-v3-boot-assets', '--', $OutputDirectory
    )
    if ($LASTEXITCODE -ne 0) { throw 'GPU renderer boot build failed' }
    & scripts/run-cargo.ps1 -Subcommand run -Label 'gpu-renderer-independent-pack' -CargoArgs @(
        '-p', 'cpu-v3-tang-nano-20k', '--bin', 'cpu-v3-pack', '--',
        (Join-Path $OutputDirectory 'application.v3bin'), '--project',
        (Join-Path $OutputDirectory 'renderer-repack.conf'), '-o',
        (Join-Path $OutputDirectory 'independent.bin')
    )
    if ($LASTEXITCODE -ne 0) { throw 'Independent GPU renderer package failed' }
    $generatedHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'cpu-v3-boot.bin')).Hash
    $independentHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'independent.bin')).Hash
    if ($generatedHash -ne $independentHash) { throw 'Generated and independent CB06 packages differ' }
    Write-Host "PASS generated/independent CB06 SHA256=$generatedHash"
} finally {
    if ($null -eq $savedExample) { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue }
    else { $env:CPU_V3_BOOT_EXAMPLE = $savedExample }
    Pop-Location
}
