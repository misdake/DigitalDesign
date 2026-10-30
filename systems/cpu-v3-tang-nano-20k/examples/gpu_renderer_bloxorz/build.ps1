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
$packageAbiSource = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'systems/cpu-v3-tang-nano-20k/rcc/gpu_capture_abi.rs')
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
$savedExample = $env:CPU_V3_BOOT_EXAMPLE
Push-Location $repoRoot
try {
    $env:CPU_V3_BOOT_EXAMPLE = 'gpu_renderer_bloxorz'
    if ($AutomaticCapture) {
        $env:CPU_V3_BOOT_EXAMPLE = if ($CaptureBrightness -eq 255) { 'gpu_renderer_bloxorz_capture_full' } else { 'gpu_renderer_bloxorz_capture' }
    }
    & scripts/run-cargo.ps1 -Subcommand run -Label "$buildLabel-boot" -CargoArgs @(
        '-p', 'cpu-v3-tang-nano-20k', '--bin', 'cpu-v3-boot-assets', '--', $OutputDirectory
    )
    if ($LASTEXITCODE -ne 0) { throw 'GPU renderer boot build failed' }
    $packArgs = @(
        '-p', 'cpu-v3-tang-nano-20k', '--bin', 'cpu-v3-pack', '--',
        (Join-Path $OutputDirectory 'application.v3bin'), '--project',
        (Join-Path $OutputDirectory 'renderer-repack.conf'), '-o',
        (Join-Path $OutputDirectory 'independent.bin')
    )
    $hardware = $null
    if ($HardwareArtifactDirectory -ne '') {
        $hardwarePath = if ([IO.Path]::IsPathRooted($HardwareArtifactDirectory)) {
            [IO.Path]::GetFullPath($HardwareArtifactDirectory)
        } else { [IO.Path]::GetFullPath((Join-Path $repoRoot $HardwareArtifactDirectory)) }
        if (-not $hardwarePath.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Keep hardware artifacts inside this worktree target/'
        }
        & systems/cpu-v3-tang-nano-20k/examples/rtt400_board/build.ps1 -Mode Audit `
            -OutputDirectory $hardwarePath -BootExample $env:CPU_V3_BOOT_EXAMPLE -Label "$buildLabel-hardware-audit"
        $manifest = @{}
        foreach ($line in Get-Content -LiteralPath (Join-Path $hardwarePath 'gowin-build.manifest')) {
            if ($line -match '^([^=]+)=(.*)$') { $manifest[$matches[1]] = $matches[2] }
        }
        $bitstreamPath = Join-Path $hardwarePath $manifest['bitstream_path']
        $configurationPath = [IO.Path]::ChangeExtension($bitstreamPath, '.bin')
        # Bind the raw Flash configuration to the exact audited SRAM bitstream.
        $checkConfiguration = @'
from pathlib import Path
import sys
fs, binary = map(Path, sys.argv[1:])
bits = ''.join(line.strip() for line in fs.read_text().splitlines() if line.strip() and not line.startswith('//'))
raw = binary.read_bytes()
assert set(bits) <= {'0', '1'} and len(bits) == len(raw) * 8, 'Invalid configuration encoding/length'
assert bytes(int(bits[i:i+8], 2) for i in range(0, len(bits), 8)) == raw, 'Configuration binary differs from audited FS'
'@
        & python -B -c $checkConfiguration $bitstreamPath $configurationPath
        if ($LASTEXITCODE -ne 0) { throw 'FPGA configuration identity check failed' }
        $packArgs += @('--configuration-bin', $configurationPath, '--flash-image', (Join-Path $OutputDirectory 'complete-flash.bin'))
        $hardware = [ordered]@{
            profile='production';
            bitstream_sha256=(Get-FileHash -LiteralPath $bitstreamPath -Algorithm SHA256).Hash;
            configuration_sha256=(Get-FileHash -LiteralPath $configurationPath -Algorithm SHA256).Hash
        }
    }
    & scripts/run-cargo.ps1 -Subcommand run -Label "$buildLabel-independent-pack" -CargoArgs $packArgs
    if ($LASTEXITCODE -ne 0) { throw 'Independent GPU renderer package failed' }
    $generatedHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'cpu-v3-boot.bin')).Hash
    $independentHash = (Get-FileHash -Algorithm SHA256 (Join-Path $OutputDirectory 'independent.bin')).Hash
    if ($generatedHash -ne $independentHash) { throw 'Generated and independent CB06 packages differ' }
    if ($null -ne $hardware) {
        $hardware['complete_flash_sha256'] = (Get-FileHash -LiteralPath (Join-Path $OutputDirectory 'complete-flash.bin') -Algorithm SHA256).Hash
    }
    [ordered]@{ profile='production'; command_abi=$packageCommandAbi; layout_abi=2;
        automatic_capture=[bool]$AutomaticCapture; capture_brightness=$(if ($AutomaticCapture) { $CaptureBrightness } else { $null }); boot_sha256=$generatedHash; hardware=$hardware;
        scope='Offline packaging only; optional hardware binding audits an existing fit and composes a complete Flash image. No programming or board qualification.'
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'profile.json') -Encoding utf8
    Write-Host "PASS generated/independent CB06 SHA256=$generatedHash"
} finally {
    if ($null -eq $savedExample) { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue }
    else { $env:CPU_V3_BOOT_EXAMPLE = $savedExample }
    Pop-Location
}
