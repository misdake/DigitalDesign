param(
    [Parameter(Mandatory = $true)]
    [string]$CapturePath,
    [int]$MinimumCopies = 2,
    [string]$ResultPath
)

$ErrorActionPreference = "Stop"

function Write-ResultAndStop {
    param([hashtable]$Result, [int]$ExitCode)
    if ($ResultPath) {
        $parent = Split-Path -Parent $ResultPath
        if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
        $Result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $ResultPath -Encoding UTF8
    }
    if ($ExitCode -ne 0) {
        [Console]::Error.WriteLine($Result.message)
        exit $ExitCode
    }
    Write-Host $Result.message
    exit 0
}

if ($MinimumCopies -le 0) { throw "MinimumCopies must be positive" }
if (-not (Test-Path -LiteralPath $CapturePath -PathType Leaf)) {
    throw "capture does not exist: $CapturePath"
}

$bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $CapturePath))
$records = [System.Collections.Generic.List[byte[]]]::new()
$badChecksums = 0
for ($offset = 0; $offset + 13 -le $bytes.Length; $offset++) {
    if ($bytes[$offset] -ne 0x46 -or $bytes[$offset + 1] -ne 0x44 -or
        $bytes[$offset + 2] -ne 0x53 -or $bytes[$offset + 3] -ne 0x31) { continue }
    [byte]$checksum = 0
    for ($index = 0; $index -lt 12; $index++) {
        $checksum = $checksum -bxor $bytes[$offset + $index]
    }
    if ($checksum -ne $bytes[$offset + 12]) {
        $badChecksums++
        continue
    }
    $record = [byte[]]::new(13)
    [Array]::Copy($bytes, $offset, $record, 0, 13)
    $records.Add($record)
}

$result = [ordered]@{
    schema = 1; outcome = "failed"; reason = $null; message = $null
    capture_bytes = $bytes.Length; valid_frames = $records.Count
    bad_checksum_frames = $badChecksums; minimum_copies = $MinimumCopies
    conflicting_frames = 0; jedec_id = $null
    sr1_before = $null; sr2 = $null; sr3 = $null
    sr1_after_wren = $null; sr1_after_wrdi = $null
    busy_before = $null; wel_before = $null
    wel_after_wren = $null; wel_after_wrdi = $null
    block_protect = $null; complement_protect = $null
}

if ($records.Count -lt $MinimumCopies) {
    $result.reason = "insufficient_frames"
    $result.message = "Flash diagnostics produced $($records.Count) valid FDS1 frame(s); expected at least $MinimumCopies."
    Write-ResultAndStop $result 1
}

$first = $records[0]
for ($recordIndex = 1; $recordIndex -lt $records.Count; $recordIndex++) {
    if (-not [Linq.Enumerable]::SequenceEqual([byte[]]$first, [byte[]]$records[$recordIndex])) {
        $result.conflicting_frames++
    }
}
if ($result.conflicting_frames -ne 0) {
    $result.reason = "conflicting_frames"
    $result.message = "Flash diagnostics contained $($result.conflicting_frames) valid frame(s) that conflict with the first snapshot."
    Write-ResultAndStop $result 1
}

$jedec = ([uint32]$first[4] -shl 16) -bor ([uint32]$first[5] -shl 8) -bor $first[6]
$sr1 = $first[7]; $sr2 = $first[8]; $sr3 = $first[9]
$sr1Wren = $first[10]; $sr1Wrdi = $first[11]
$result.jedec_id = "0x$($jedec.ToString('x6'))"
$result.sr1_before = "0x$($sr1.ToString('x2'))"
$result.sr2 = "0x$($sr2.ToString('x2'))"
$result.sr3 = "0x$($sr3.ToString('x2'))"
$result.sr1_after_wren = "0x$($sr1Wren.ToString('x2'))"
$result.sr1_after_wrdi = "0x$($sr1Wrdi.ToString('x2'))"
$result.busy_before = [bool]($sr1 -band 1)
$result.wel_before = [bool]($sr1 -band 2)
$result.wel_after_wren = [bool]($sr1Wren -band 2)
$result.wel_after_wrdi = [bool]($sr1Wrdi -band 2)
$result.block_protect = ($sr1 -shr 2) -band 7
$result.complement_protect = [bool]($sr2 -band 0x40)

if ($jedec -ne 0xef4017) {
    $result.reason = "unexpected_jedec_id"
    $result.message = "Flash diagnostics read JEDEC $($result.jedec_id); this fitted board expects 0xef4017."
    Write-ResultAndStop $result 1
}
if ($result.busy_before -or $result.wel_before -or -not $result.wel_after_wren -or $result.wel_after_wrdi) {
    $result.reason = "write_enable_latch_failed"
    $result.message = "Flash write-enable latch sequence failed: SR1 $($result.sr1_before) -> $($result.sr1_after_wren) -> $($result.sr1_after_wrdi)."
    Write-ResultAndStop $result 1
}
if ($result.block_protect -ne 0 -or $result.complement_protect) {
    $result.reason = "array_protection_enabled"
    $result.message = "Flash status enables array protection: BP=$($result.block_protect), CMP=$($result.complement_protect), SR1=$($result.sr1_before), SR2=$($result.sr2)."
    Write-ResultAndStop $result 1
}

$result.outcome = "passed"
$result.reason = "ok"
$result.message = "Flash diagnostics passed ($($records.Count) stable frame(s), JEDEC $($result.jedec_id), WEL toggled, BP/CMP clear)."
Write-ResultAndStop $result 0
