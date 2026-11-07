param(
    [Parameter(Mandatory, Position=0)][string]$Application,
    [Parameter(Mandatory, Position=1)][string]$OutputDirectory,
    [string[]]$BuildArguments = @(),
    [string]$Label = ''
)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
if ($Label -eq '') { $Label = "app-$Application" }
$outputPath = if ([IO.Path]::IsPathRooted($OutputDirectory)) { [IO.Path]::GetFullPath($OutputDirectory) }
else { [IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory)) }
& (Join-Path $repoRoot 'scripts/run-cargo.ps1') -Subcommand run -Label $Label -CargoArgs (@(
    '--manifest-path', (Join-Path $repoRoot 'Cargo.toml'), '-p', 'cpu-v3-tools', '--bin', 'cpu-v3-app', '--',
    'build', $Application, '--out', $outputPath
) + $BuildArguments)
if ($LASTEXITCODE -ne 0) { throw "Application $Application build failed" }
