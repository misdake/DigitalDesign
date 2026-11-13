param(
    [string]$OutputDirectory = 'target/bloxorz-rtt/boot',
    [string]$Label = '',
    [switch]$AutomaticCapture,
    [ValidateSet(128,255)][int]$CaptureBrightness = 128,
    [string]$HardwareArtifactDirectory = ''
)
$buildLabel = if ($Label -eq '') { 'bloxorz-rtt-' + [guid]::NewGuid().ToString('N').Substring(0,8) } else { $Label }
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$packageAbiSource = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/snapshot_abi.rs')
$packageCommandAbiMatch = [regex]::Match($packageAbiSource, 'pub const GPU_CAPTURE_RTT400_COMMAND_ABI: u16 = ([0-9]+);')
if (-not $packageCommandAbiMatch.Success) { throw 'Missing canonical RTT command ABI' }
$packageCommandAbi = [int]$packageCommandAbiMatch.Groups[1].Value
$outputPath = if ([IO.Path]::IsPathRooted($OutputDirectory)) { [IO.Path]::GetFullPath($OutputDirectory) }
else { [IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory)) }
$targetRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target')) + [IO.Path]::DirectorySeparatorChar
if (-not $outputPath.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Keep Bloxorz build products inside this worktree target/'
}
$OutputDirectory = $outputPath
Push-Location $repoRoot
try {
    $application = if ($AutomaticCapture) {
        if ($CaptureBrightness -eq 255) { 'gpu_renderer_bloxorz_capture_full' } else { 'gpu_renderer_bloxorz_capture' }
    } else { 'gpu_renderer_bloxorz' }
    & systems/cpu-v3-tang-nano-20k/tools/build-application.ps1 -Application $application -OutputDirectory $OutputDirectory -Label $buildLabel
    $generatedHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'cpu-v3-boot.bin')).Hash
    $hardware = $null
    if ($HardwareArtifactDirectory -ne '') {
        $hardwarePath = if ([IO.Path]::IsPathRooted($HardwareArtifactDirectory)) {
            [IO.Path]::GetFullPath($HardwareArtifactDirectory)
        } else { [IO.Path]::GetFullPath((Join-Path $repoRoot $HardwareArtifactDirectory)) }
        if (-not $hardwarePath.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Keep hardware artifacts inside this worktree target/'
        }
        & python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py assemble $hardwarePath $OutputDirectory --out (Join-Path $OutputDirectory 'deployment')
        if ($LASTEXITCODE -ne 0) { throw 'Hardware-bound deployment assembly failed' }
        $hardware = Get-Content -Raw -LiteralPath (Join-Path $OutputDirectory 'deployment/deployment.json') | ConvertFrom-Json
    }
    [ordered]@{ profile='production'; command_abi=$packageCommandAbi; layout_abi=2;
        automatic_capture=[bool]$AutomaticCapture; capture_brightness=$(if ($AutomaticCapture) { $CaptureBrightness } else { $null }); boot_sha256=$generatedHash; hardware=$hardware;
        scope='Offline packaging only; optional hardware binding audits an existing fit and composes a complete Flash image. No programming or board qualification.'
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'profile.json') -Encoding utf8
    Write-Host "PASS verified application CB06 SHA256=$generatedHash"
} finally { Pop-Location }
