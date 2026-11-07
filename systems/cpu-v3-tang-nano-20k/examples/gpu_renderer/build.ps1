param([string]$OutputDirectory = '', [switch]$Changes, [switch]$Members)
if ($Changes -and $Members) { throw 'Choose either Changes or Members' }
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = if ($Members) { 'target/gpu-renderer-members-boot' } elseif ($Changes) { 'target/gpu-renderer-changes-boot' } else { 'target/gpu-renderer-boot' }
}
$application = if ($Members) { 'renderer-members' } elseif ($Changes) { 'renderer-changes' } else { 'renderer' }
& (Join-Path $PSScriptRoot '../../tools/build-application.ps1') -Application $application -OutputDirectory $OutputDirectory
