param([string]$OutputDirectory = 'target/dualshock2-boot')
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$savedExample = $env:CPU_V3_BOOT_EXAMPLE
Push-Location $repoRoot
try {
    $env:CPU_V3_BOOT_EXAMPLE = 'dualshock2'
    & scripts/run-cargo.ps1 -Subcommand run -Label 'ds2-boot-assets' -CargoArgs @(
        '-p','cpu-v3-tang-nano-20k','--bin','cpu-v3-boot-assets','--',$OutputDirectory
    )
    if ($LASTEXITCODE -ne 0) { throw 'DS2 boot build failed' }
    & scripts/run-cargo.ps1 -Subcommand run -Label 'ds2-independent-pack' -CargoArgs @(
        '-p','cpu-v3-tang-nano-20k','--bin','cpu-v3-pack','--',
        (Join-Path $OutputDirectory 'application.v3bin'),'--project',
        'systems/cpu-v3-tang-nano-20k/examples/dualshock2/boot.conf','-o',
        (Join-Path $OutputDirectory 'independent.bin')
    )
    if ($LASTEXITCODE -ne 0) { throw 'Independent DS2 package failed' }
    $generatedHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'cpu-v3-boot.bin')).Hash
    $independentHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'independent.bin')).Hash
    if ($generatedHash -ne $independentHash) { throw 'Generated and independent CB06 packages differ' }
    Write-Host "PASS generated/independent DS2 CB06 SHA256=$generatedHash"
    & scripts/run-cargo.ps1 -Subcommand run -Label 'ds2-board-export' -CargoArgs @(
        '-p','cpu-v3-tang-nano-20k','--example','cpu_v3_system','--',
        (Join-Path $OutputDirectory 'gateware')
    )
    if ($LASTEXITCODE -ne 0) { throw 'DS2 board export failed' }
} finally {
    if ($null -eq $savedExample) { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue }
    else { $env:CPU_V3_BOOT_EXAMPLE = $savedExample }
    Pop-Location
}
