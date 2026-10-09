# Run from any directory, without a build or ledger writes:
#   scripts/test-pnr-ledger.ps1
# Optional real reports: -ReportFixtures @(@{ Report=...; Xml=...; Expected=8 })
param([hashtable[]]$ReportFixtures = @())
$ErrorActionPreference = "Stop"
$source = Join-Path $PSScriptRoot "log-post-commit-pnr.ps1"
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors -join "`n") }
$definition = $ast.Find({ param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -eq "Get-GowinBsramUsage"
}, $true)
if (-not $definition) { throw "BSRAM parser function is missing." }
. ([scriptblock]::Create($definition.Extent.Text))

function Assert-Rejected([scriptblock]$action, [string]$message) {
    $rejected = $false
    try { & $action | Out-Null }
    catch {
        if ($_.Exception.Message -notmatch $message) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "Expected rejection matching: $message." }
}

$six = "  BSRAM | 14%`n --SDPB | 2`n --pROM | 4`n DSP | 29%`n --OTHER_DSP | 1"
$eight = " BSRAM | 18%`n --SPX9 | 2`n --SDPB | 2`n --pROM | 4`n DSP | 29%"
$compact = " BSRAM | 14%`n --SDPB | 3`n --pROM | 3"
foreach ($fixture in @(@{ Text = $six; Expected = 6 }, @{ Text = $eight; Expected = 8 },
        @{ Text = $compact; Expected = 6 })) {
    $usage = Get-GowinBsramUsage $fixture.Text
    if ($usage.Total -ne $fixture.Expected) { throw "Fixture total mismatch." }
    if ($null -ne $usage.SynthesisTotal) { throw "Missing XML was treated as a numeric total." }
}
if ((Get-GowinBsramUsage $eight).ByKind.SPX9 -ne 2) { throw "SPX9 breakdown was lost." }
if ((Get-GowinBsramUsage "BSRAM | 0%`nDSP | 0%").Total -ne 0) { throw "Empty zero section failed." }
if ((Get-GowinBsramUsage "BSRAM | <1%`n --SP | 1").Total -ne 1) { throw "Small percentage failed." }
$allKinds = "BSRAM | 8/46 18%`n" +
    ((@("SP", "SPX9", "SDPB", "SDPX9B", "DPB", "DPX9B", "pROM", "pROMX9") |
        ForEach-Object { " --$_ | 1" }) -join "`n")
$usage = Get-GowinBsramUsage $allKinds
if ($usage.Total -ne 8 -or @($usage.ByKind.Values | Where-Object { $_ -ne 1 }).Count) {
    throw "Full primitive inventory was not preserved."
}
Assert-Rejected { Get-GowinBsramUsage ($eight -replace 'SPX9', 'NEW_BSRAM') } 'Unknown BSRAM primitive'
Assert-Rejected { Get-GowinBsramUsage ($allKinds -replace '8/46', '7/46') } 'PnR BSRAM total mismatch'
Assert-Rejected { Get-GowinBsramUsage ($eight + "`nBSRAM | 18%") } 'Expected one PnR BSRAM section'
Assert-Rejected { Get-GowinBsramUsage ($eight -replace '--SPX9 \| 2', "--SPX9 | 2`n --SPX9 | 2") } 'Duplicate BSRAM'
Assert-Rejected { Get-GowinBsramUsage ($eight -replace '--SPX9 \| 2', '--SPX9 | -2') } 'Invalid BSRAM primitive count'
Assert-Rejected { Get-GowinBsramUsage "BSRAM | 18%`nDSP | 0%" } 'has no primitive counts'
Assert-Rejected { Get-GowinBsramUsage "BSRAM | 0%`n --SPX9 | 2" } 'PnR BSRAM total mismatch'
Assert-Rejected { Get-GowinBsramUsage "BSRAM | 18%`n --SPX9" } 'Malformed BSRAM primitive'
Assert-Rejected { Get-GowinBsramUsage "BSRAM | 18%`n --SPX9 | 0" } 'zero primitive sum'

$repoRoot = Split-Path -Parent $PSScriptRoot
$scratch = Join-Path $repoRoot ("target/pnr-ledger-test-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $scratch | Out-Null
$xmlPath = Join-Path $scratch "hierarchy.xml"
# Own counts at parent and child are disjoint in Gowin's hierarchy format.
[IO.File]::WriteAllText($xmlPath,
    '<Module name="top" Bsram="1"><SubModule name="a" Bsram="2"><SubModule name="b" Bsram="3"/></SubModule></Module>')
if ((Get-GowinBsramUsage $six $xmlPath).SynthesisTotal -ne 6) { throw "Hierarchy counts were double-counted or lost." }
Assert-Rejected { Get-GowinBsramUsage $eight $xmlPath } 'BSRAM synthesis/PnR mismatch'
# Reject an inclusive-total schema rather than treating it as own counts silently.
[IO.File]::WriteAllText($xmlPath, '<Module Bsram="6"><SubModule Bsram="6"/></Module>')
Assert-Rejected { Get-GowinBsramUsage $six $xmlPath } 'BSRAM synthesis/PnR mismatch'
Assert-Rejected { Get-GowinBsramUsage $six (Join-Path $scratch 'missing.xml') } 'report not found'
[IO.File]::WriteAllText($xmlPath, '<Resource Bsram="6"/>')
Assert-Rejected { Get-GowinBsramUsage $six $xmlPath } 'Unknown Gowin synthesis hierarchy schema'

foreach ($fixture in $ReportFixtures) {
    $usage = Get-GowinBsramUsage (Get-Content -Raw -LiteralPath $fixture.Report) $fixture.Xml
    if ($usage.Total -ne $fixture.Expected) { throw "Real report mismatch: $($fixture.Report)." }
    # A normal percentage-only report must remain readable without XML.
    if ((Get-GowinBsramUsage (Get-Content -Raw -LiteralPath $fixture.Report)).Total -ne $fixture.Expected) {
        throw "Real report failed without XML: $($fixture.Report)."
    }
    Write-Host ("Verified {0}: BSRAM={1}; {2}" -f $fixture.Report, $usage.Total,
        ($usage.ByKind | ConvertTo-Json -Compress))
}
Write-Host "PnR BSRAM accounting passed. Fixtures: $scratch"
