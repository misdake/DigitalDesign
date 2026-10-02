$ErrorActionPreference = 'Stop'
$unitRoot = Split-Path -Parent $PSScriptRoot
$geometryRoot = Join-Path (Split-Path -Parent $unitRoot) 'geometry'
$geometrySource = Join-Path $geometryRoot 'src/counted.rs'
$expectedSha = 'f472e2b1dc578364dd3abae35a10a23883a6cbbddd03e6d280da3285d9ce666f'
if ((Get-FileHash -LiteralPath $geometrySource -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedSha) {
    throw 'Geometry reference source differs from the frozen intake identity.'
}
$original = Get-Content -LiteralPath $geometrySource -Raw
$frozen = Get-Content -LiteralPath (Join-Path $unitRoot 'src/reference.rs') -Raw
$originalStart = $original.IndexOf('fn scaled_output<')
$originalEnd = $original.IndexOf('fn distance(')
$frozenStart = $frozen.IndexOf('fn scaled_output<')
$frozenEnd = $frozen.IndexOf('fn typed<')
if ($originalStart -lt 0 -or $originalEnd -le $originalStart -or $frozenStart -lt 0 -or $frozenEnd -le $frozenStart) {
    throw 'Reference extraction boundaries missing.'
}
$originalTokens = $original.Substring($originalStart,$originalEnd-$originalStart) -replace '\s',''
$frozenTokens = $frozen.Substring($frozenStart,$frozenEnd-$frozenStart) -replace '\s',''
if ($originalTokens -cne $frozenTokens) { throw 'Frozen helper tokens differ.' }
Write-Output 'Frozen geometry source SHA and helper token equality verified.'
