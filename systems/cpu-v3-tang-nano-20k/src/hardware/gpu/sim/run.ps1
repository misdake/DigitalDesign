param(
    [ValidateSet('test', 'vertex', 'memory', 'microkernel', 'meshlet', 'lint')]
    [string] $Mode = 'test',
    [UInt64] $Seed = 19,
    [switch] $Staged,
    [switch] $DualWide,
    [switch] $Trace,
    [string] $MeshPath = '',
    [UInt32] $MeshletIndex = 0,
    [switch] $AllMeshlets,
    [string] $MatrixPath = '',
    [string] $ReferenceMeshPath = '',
    [string] $NormalMatrixPath = '',
    [string] $SetupJsonOut = ''
)

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../../../../..')).Path
$manifest = Join-Path $PSScriptRoot 'Cargo.toml'
$targetDir = Join-Path $repoRoot 'target/gpu-v2-cmodel'

if ($Mode -eq 'test') {
    & cargo test --manifest-path $manifest --target-dir $targetDir
} elseif ($Mode -eq 'lint') {
    & cargo clippy --manifest-path $manifest --target-dir $targetDir --all-targets -- -D warnings
} elseif ($Mode -eq 'memory') {
    & cargo run --example memory_probe --manifest-path $manifest --target-dir $targetDir -- $Seed
} elseif ($Mode -eq 'microkernel') {
    $exampleArgs = @([string]$Seed)
    if ($Staged) { $exampleArgs += '--staged' }
    if ($DualWide) { $exampleArgs += '--dualwide' }
    if ($Trace) { $exampleArgs += '--trace' }
    & cargo run --example microkernel_probe --manifest-path $manifest --target-dir $targetDir -- @exampleArgs
} elseif ($Mode -eq 'meshlet') {
    if (-not $MeshPath) { throw 'MeshPath is required for meshlet mode' }
    $selection = if ($AllMeshlets) { 'all' } else { [string] $MeshletIndex }
    $exampleArgs = @($MeshPath, $selection)
    if ($MatrixPath) { $exampleArgs += $MatrixPath }
    if ($ReferenceMeshPath -or $NormalMatrixPath -or $SetupJsonOut) {
        if (-not $MatrixPath) { $exampleArgs += '-' }
        $exampleArgs += $(if ($ReferenceMeshPath) { $ReferenceMeshPath } else { '-' })
    }
    if ($NormalMatrixPath) { $exampleArgs += @('--normal', $NormalMatrixPath) }
    if ($SetupJsonOut) { $exampleArgs += @('--setup-json', $SetupJsonOut) }
    & cargo run --example meshlet_probe --manifest-path $manifest --target-dir $targetDir -- @exampleArgs
} else {
    & cargo run --example vertex_smoke --manifest-path $manifest --target-dir $targetDir
}
exit $LASTEXITCODE
