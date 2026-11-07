param(
    [string]$OutputDirectory = 'target/gpu-demo-boot',
    [ValidateSet('default','column16')][string]$Profile = 'default',
    [string]$LayoutContract = ''
)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
if ($Profile -eq 'column16' -and [string]::IsNullOrWhiteSpace($LayoutContract)) { throw 'Column scene requires the explicit layout contract path' }
$OutputDirectory = if ([IO.Path]::IsPathRooted($OutputDirectory)) { [IO.Path]::GetFullPath($OutputDirectory) }
else { [IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory)) }
$application = if ($Profile -eq 'column16') { 'demo-column16' } else { 'demo' }
& (Join-Path $repoRoot 'systems/cpu-v3-tang-nano-20k/tools/build-application.ps1') -Application $application -OutputDirectory $OutputDirectory
if ($Profile -eq 'column16') {
    # This explicit qualification retains input reconstruction and an independent decoder.
    & (Join-Path $repoRoot 'scripts/run-cargo.ps1') -Subcommand run -Label 'column-package-reconstruction' -CargoArgs @(
        '--manifest-path', (Join-Path $repoRoot 'Cargo.toml'), '-p', 'cpu-v3-boot-format', '--bin', 'cpu-v3-pack', '--',
        (Join-Path $OutputDirectory 'application.v3bin'), '--project', (Join-Path $OutputDirectory 'demo-repack.conf'),
        '-o', (Join-Path $OutputDirectory 'repacked.bin'))
    if ($LASTEXITCODE -ne 0) { throw 'Column package reconstruction failed' }
    $contractPath = if ([IO.Path]::IsPathRooted($LayoutContract)) { $LayoutContract } else { Join-Path $repoRoot $LayoutContract }
    & python -B (Join-Path $PSScriptRoot 'tools/column_scene.py') --output $OutputDirectory --contract $contractPath
    if ($LASTEXITCODE -ne 0) { throw 'Column scene address/asset/identity audit failed' }
}
