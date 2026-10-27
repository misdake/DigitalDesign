param([string]$OutputDirectory = 'target/dualshock2-ui-boot')
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$savedExample = $env:CPU_V3_BOOT_EXAMPLE
Push-Location $repoRoot
try {
    $env:CPU_V3_BOOT_EXAMPLE = 'dualshock2_ui'
    & scripts/run-cargo.ps1 -Subcommand run -Label 'ds2-ui-boot-assets' -CargoArgs @(
        '-p','cpu-v3-tang-nano-20k','--bin','cpu-v3-boot-assets','--',$OutputDirectory
    )
    if ($LASTEXITCODE -ne 0) { throw 'DS2 UI boot build failed' }
    & scripts/run-cargo.ps1 -Subcommand run -Label 'ds2-ui-independent-pack' -CargoArgs @(
        '-p','cpu-v3-tang-nano-20k','--bin','cpu-v3-pack','--',
        (Join-Path $OutputDirectory 'application.v3bin'),'--project',
        (Join-Path $OutputDirectory 'ds2-ui-repack.conf'),'-o',
        (Join-Path $OutputDirectory 'independent.bin')
    )
    if ($LASTEXITCODE -ne 0) { throw 'Independent DS2 UI package failed' }
    $generated = (Get-FileHash (Join-Path $OutputDirectory 'cpu-v3-boot.bin') -Algorithm SHA256).Hash
    $repacked = (Get-FileHash (Join-Path $OutputDirectory 'independent.bin') -Algorithm SHA256).Hash
    if ($generated -ne $repacked) { throw 'DS2 UI packages differ' }
    Write-Host "PASS generated/independent DS2 UI CB06 SHA256=$generated"
    & scripts/run-cargo.ps1 -Subcommand run -Label 'ds2-ui-hardware-export' -CargoArgs @(
        '-p','cpu-v3-tang-nano-20k','--example','cpu_v3_system','--',
        (Join-Path $OutputDirectory 'gateware')
    )
    if ($LASTEXITCODE -ne 0) { throw 'DS2 UI hardware export failed' }
} finally {
    if ($null -eq $savedExample) { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue }
    else { $env:CPU_V3_BOOT_EXAMPLE = $savedExample }
    Pop-Location
}
