param([string]$OutputDirectory = '', [string]$Label = '', [switch]$Bilinear)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$packageAbiSource = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'systems/cpu-v3-tang-nano-20k/rcc/gpu_capture_abi.rs')
$packageCommandAbiMatch = [regex]::Match($packageAbiSource, 'pub const GPU_CAPTURE_RTT400_COMMAND_ABI: u16 = ([0-9]+);')
if (-not $packageCommandAbiMatch.Success) { throw 'Missing canonical RTT command ABI' }
$packageCommandAbi = [int]$packageCommandAbiMatch.Groups[1].Value
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { $OutputDirectory = if ($Bilinear) { 'target/rtt/boot-bilinear' } else { 'target/rtt/boot-nearest' } }
if ([string]::IsNullOrWhiteSpace($Label)) { $Label = 'gpu-renderer-rtt-' + [guid]::NewGuid().ToString('N').Substring(0,8) }
$savedExample = $env:CPU_V3_BOOT_EXAMPLE
Push-Location $repoRoot
try {
    $env:CPU_V3_BOOT_EXAMPLE = if ($Bilinear) { 'gpu_renderer_rtt_bilinear' } else { 'gpu_renderer_rtt' }
    & scripts/run-cargo.ps1 -Subcommand run -Label "$Label-boot" -CargoArgs @(
        '-p','cpu-v3-tang-nano-20k','--bin','cpu-v3-boot-assets','--',$OutputDirectory)
    if ($LASTEXITCODE -ne 0) { throw 'RTT software build failed' }
    & scripts/run-cargo.ps1 -Subcommand run -Label "$Label-independent-pack" -CargoArgs @(
        '-p','cpu-v3-tang-nano-20k','--bin','cpu-v3-pack','--',
        (Join-Path $OutputDirectory 'application.v3bin'),'--project',
        (Join-Path $OutputDirectory 'renderer-repack.conf'),'-o',
        (Join-Path $OutputDirectory 'independent.bin'))
    if ($LASTEXITCODE -ne 0) { throw 'Independent RTT package failed' }
    $generatedHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'cpu-v3-boot.bin')).Hash
    $independentHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'independent.bin')).Hash
    if ($generatedHash -ne $independentHash) { throw 'Generated and independent CB06 differ' }
    [ordered]@{
        profile = 'production'; command_abi = $packageCommandAbi; layout_abi = 2; surface = 'column-major-16x16-pad16'
        demo = $(if ($Bilinear) { 'ordinary-bilinear' } else { 'fullscreen-nearest' })
        boot_sha256 = $generatedHash; hardware = $null
        status = 'software-only-pending-matched-rtl-and-board-integration'
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutputDirectory 'profile.json') -Encoding utf8
    Write-Host "PASS rtt400x240 software package SHA256=$generatedHash"
} finally {
    if ($null -eq $savedExample) { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue }
    else { $env:CPU_V3_BOOT_EXAMPLE = $savedExample }
    Pop-Location
}
