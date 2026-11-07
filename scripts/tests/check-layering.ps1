$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
function Package([string]$Name, [string]$Path, [string[]]$Dependencies = @()) {
    [pscustomobject]@{ name=$Name; manifest_path=(Join-Path $repoRoot $Path)
        dependencies=@($Dependencies | ForEach-Object { [pscustomobject]@{name=$_} }) }
}
$cases = @(
    @{Name='composition to own support'; From='systems/a/Cargo.toml'; To='systems/a/format/Cargo.toml'; Allow=$true},
    @{Name='own sibling support'; From='systems/a/tools/Cargo.toml'; To='systems/a/format/Cargo.toml'; Allow=$true},
    @{Name='support to composition'; From='systems/a/tools/Cargo.toml'; To='systems/a/Cargo.toml'; Allow=$false},
    @{Name='another composition'; From='systems/a/Cargo.toml'; To='systems/b/Cargo.toml'; Allow=$false},
    @{Name='another system support'; From='systems/a/tools/Cargo.toml'; To='systems/b/format/Cargo.toml'; Allow=$false},
    @{Name='frontend upward'; From='compiler/rcc/Cargo.toml'; To='systems/a/format/Cargo.toml'; Allow=$false; Frontend=$true},
    @{Name='frontend to CPU'; From='compiler/rcc/Cargo.toml'; To='ip/cpu-v3/Cargo.toml'; Allow=$false; Frontend=$true},
    @{Name='support to CPU'; From='systems/a/tools/Cargo.toml'; To='ip/cpu-v3/Cargo.toml'; Allow=$true},
    @{Name='modeling remains independent'; From='modeling/example/Cargo.toml'; To='compiler/other/Cargo.toml'; Allow=$false}
)
foreach ($case in $cases) {
    $ownerName = if ($case.Frontend) { 'rcc' } else { 'owner' }
    $packages = @((Package $ownerName $case.From @('target')), (Package 'target' $case.To))
    if (-not $case.Frontend) { $packages += Package 'rcc' 'compiler/rcc/Cargo.toml' }
    $fixtureJson = @{packages=$packages} | ConvertTo-Json -Depth 8
    Set-Item function:cargo -Value ({ $fixtureJson }.GetNewClosure())
    $accepted = $true
    $failure = ''
    try { & (Join-Path $repoRoot 'scripts/check-layering.ps1') }
    catch { $accepted = $false; $failure = $_.Exception.Message }
    if ($accepted -ne $case.Allow) { throw "Wrong layering outcome: $($case.Name): $failure" }
}
Write-Output "$($cases.Count) dependency-boundary contracts passed."
