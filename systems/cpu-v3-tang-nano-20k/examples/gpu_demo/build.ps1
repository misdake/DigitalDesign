param(
    [string]$OutputDirectory = 'target/gpu-demo-boot',
    [ValidateSet('default','column16')][string]$Profile = 'default',
    [string]$LayoutContract = ''
)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$savedExample = $env:CPU_V3_BOOT_EXAMPLE
Push-Location $repoRoot
try {
    if($Profile -eq 'column16' -and [string]::IsNullOrWhiteSpace($LayoutContract)){ throw 'Column scene requires the explicit layout contract path' }
    $env:CPU_V3_BOOT_EXAMPLE = if($Profile -eq 'column16'){'gpu_demo_column16'}else{'gpu_demo'}
    & scripts/run-cargo.ps1 -Subcommand run -Label "gpu-demo-$Profile-boot" -CargoArgs @(
        '-p', 'cpu-v3-tang-nano-20k', '--bin', 'cpu-v3-boot-assets', '--', $OutputDirectory
    )
    if ($LASTEXITCODE -ne 0) { throw 'GPU demo boot build failed' }
    & scripts/run-cargo.ps1 -Subcommand run -Label "gpu-demo-$Profile-independent-pack" -CargoArgs @(
        '-p', 'cpu-v3-tang-nano-20k', '--bin', 'cpu-v3-pack', '--',
        (Join-Path $OutputDirectory 'application.v3bin'), '--project',
        (Join-Path $OutputDirectory 'demo-repack.conf'), '-o',
        (Join-Path $OutputDirectory 'independent.bin')
    )
    if ($LASTEXITCODE -ne 0) { throw 'Independent GPU demo package failed' }
    $generatedHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'cpu-v3-boot.bin')).Hash
    $independentHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'independent.bin')).Hash
    if ($generatedHash -ne $independentHash) { throw 'Generated and independent CB06 packages differ' }
    Write-Host "PASS generated/independent CB06 SHA256=$generatedHash"
    if($Profile -eq 'column16') {
        & python -B (Join-Path $PSScriptRoot 'tools/column_scene.py') --output $OutputDirectory --contract $LayoutContract
        if($LASTEXITCODE -ne 0){ throw 'Column scene address/asset/identity audit failed' }
    }
} finally {
    if ($null -eq $savedExample) { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue }
    else { $env:CPU_V3_BOOT_EXAMPLE = $savedExample }
    Pop-Location
}
