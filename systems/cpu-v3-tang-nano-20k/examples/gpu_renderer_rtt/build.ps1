param([string]$OutputDirectory = '', [string]$Label = '', [switch]$Bilinear)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$packageAbiSource = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/snapshot_abi.rs')
$packageCommandAbiMatch = [regex]::Match($packageAbiSource, 'pub const GPU_CAPTURE_RTT400_COMMAND_ABI: u16 = ([0-9]+);')
if (-not $packageCommandAbiMatch.Success) { throw 'Missing canonical RTT command ABI' }
$packageCommandAbi = [int]$packageCommandAbiMatch.Groups[1].Value
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { $OutputDirectory = if ($Bilinear) { 'target/rtt/boot-bilinear' } else { 'target/rtt/boot-nearest' } }
if ([string]::IsNullOrWhiteSpace($Label)) { $Label = 'gpu-renderer-rtt-' + [guid]::NewGuid().ToString('N').Substring(0,8) }
Push-Location $repoRoot
try {
    $application = if ($Bilinear) { 'rtt-bilinear' } else { 'rtt' }
    & systems/cpu-v3-tang-nano-20k/tools/build-application.ps1 -Application $application -OutputDirectory $OutputDirectory -Label $Label
    $generatedHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'cpu-v3-boot.bin')).Hash
    [ordered]@{
        profile = 'production'; command_abi = $packageCommandAbi; layout_abi = 2; surface = 'column-major-16x16-pad16'
        demo = $(if ($Bilinear) { 'ordinary-bilinear' } else { 'fullscreen-nearest' })
        boot_sha256 = $generatedHash; hardware = $null
        status = 'software-only-pending-matched-rtl-and-board-integration'
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutputDirectory 'profile.json') -Encoding utf8
    Write-Host "PASS rtt400x240 software package SHA256=$generatedHash"
} finally { Pop-Location }
