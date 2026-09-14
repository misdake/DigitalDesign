# Run from any directory: scripts/test-benchmark-ledger.ps1
# Exercise identity against edited copies, without running a suite or writing a ledger.
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$source = Join-Path $PSScriptRoot "log-post-commit-benchmark.ps1"
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors -join "`n") }
foreach ($name in @("Get-TextDigest", "Get-SuiteFiles", "Get-SuiteDigest", "Assert-SuitePrograms")) {
    $definition = $ast.Find({ param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
    }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}

Push-Location $repoRoot
try {
    $suite = Join-Path $repoRoot "systems/cpu-v3-tang-nano-20k/benchmarks/suite"
    $scratch = Join-Path $repoRoot ("target/ledger-identity-test-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $scratch | Out-Null
    Get-ChildItem -LiteralPath $suite -File | Where-Object { $_.Extension -in @(".rs", ".hex") } |
        Copy-Item -Destination $scratch
    $before = Get-SuiteDigest $scratch
    if ($before -ne (Get-SuiteDigest $suite)) { throw "A copied suite changed identity." }
    $committed = Get-SuiteDigest $scratch "HEAD"
    $file = Get-ChildItem -LiteralPath $scratch -Filter "*.rs" | Sort-Object Name | Select-Object -First 1
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $original = [IO.File]::ReadAllText($file.FullName).Replace("`r`n", "`n")
    [IO.File]::WriteAllText($file.FullName, $original, $utf8)
    if ((Get-SuiteDigest $scratch) -ne $before) { throw "LF conversion changed identity." }
    [IO.File]::WriteAllText($file.FullName, $original.Replace("`n", "`r`n"), $utf8)
    if ((Get-SuiteDigest $scratch) -ne $before) { throw "CRLF conversion changed identity." }
    [IO.File]::WriteAllText($file.FullName, $original + "`n// changed workload`n", $utf8)
    if ((Get-SuiteDigest $scratch) -eq $before) { throw "An edited suite kept its old identity." }
    if ((Get-SuiteDigest $scratch "HEAD") -ne $committed) { throw "A historical digest read the edited checkout." }
    $rows = @(Get-SuiteFiles $suite | ForEach-Object {
            [pscustomobject]@{ name = [IO.Path]::GetFileNameWithoutExtension($_.Name) }
        })
    Assert-SuitePrograms $rows $suite
    Assert-SuitePrograms $rows $suite "HEAD"
    $extraRejected = $false
    try { Assert-SuitePrograms @($rows + [pscustomobject]@{ name = "transient-probe" }) $suite "HEAD" }
    catch { $extraRejected = $_.Exception.Message -match "extra: transient-probe" }
    if (-not $extraRejected) { throw "A CSV with an extra transient program was silently accepted." }
    $missingRejected = $false
    try { Assert-SuitePrograms @($rows | Select-Object -Skip 1) $suite "HEAD" }
    catch { $missingRejected = $_.Exception.Message -match "missing:" }
    if (-not $missingRejected) { throw "A CSV missing a frozen program was silently accepted." }
    $rejected = $false
    try { Get-SuiteDigest $scratch "0000000000000000000000000000000000000000" | Out-Null }
    catch { $rejected = $true }
    if (-not $rejected) { throw "An unknown historical commit was silently accepted." }
    Write-Host "Benchmark ledger identity passed. Fixtures: $scratch"
} finally {
    Pop-Location
}
