param([string]$OutputDirectory = 'target/dualshock2-boot', [switch]$ExportHardware)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
& (Join-Path $repoRoot 'systems/cpu-v3-tang-nano-20k/tools/build-application.ps1') -Application 'dualshock2' -OutputDirectory $OutputDirectory
if ($ExportHardware) {
    & (Join-Path $repoRoot 'systems/cpu-v3-tang-nano-20k/examples/rtt400_board/build.ps1') -Mode Export -BootExample 'dualshock2' -OutputDirectory (Join-Path $OutputDirectory 'gateware')
}
