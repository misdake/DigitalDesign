param(
    [Parameter(Mandatory = $true)][string]$CapturePath,
    [Parameter(Mandatory = $true)][string]$ExpectedPath,
    [uint32]$FlashBase = 0x100000,
    [int]$MinimumCopies = 2,
    [string]$RecoveredPath,
    [string]$ResultPath
)

$ErrorActionPreference = "Stop"
if ($MinimumCopies -le 0) { throw "minimum copy count must be positive" }

$capture = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $CapturePath))
$expectedFile = Resolve-Path -LiteralPath $ExpectedPath
$expected = [System.IO.File]::ReadAllBytes($expectedFile)
if ($expected.Length -eq 0 -or $expected.Length -gt [uint16]::MaxValue) {
    throw "expected image length must be 1..65535 bytes"
}

$recovered = New-Object byte[] $expected.Length
$seen = New-Object bool[] $expected.Length
$counts = New-Object int[] $expected.Length
$candidates = 0
$validFrames = 0
$badChecksums = 0
$outOfRange = 0
$conflicts = 0

for ($captureOffset = 0; $captureOffset -le $capture.Length - 8; $captureOffset++) {
    if ($capture[$captureOffset] -ne 0x46 -or $capture[$captureOffset + 1] -ne 0x42 -or
        $capture[$captureOffset + 2] -ne 0x52 -or $capture[$captureOffset + 3] -ne 0x31) {
        continue
    }
    $candidates++
    [byte]$checksum = 0
    for ($index = 0; $index -lt 7; $index++) {
        $checksum = $checksum -bxor $capture[$captureOffset + $index]
    }
    if ($checksum -ne $capture[$captureOffset + 7]) {
        $badChecksums++
        $captureOffset += 7
        continue
    }

    $recordOffset = [int]$capture[$captureOffset + 4] -bor ([int]$capture[$captureOffset + 5] -shl 8)
    $value = $capture[$captureOffset + 6]
    if ($recordOffset -ge $expected.Length) {
        $outOfRange++
        $captureOffset += 7
        continue
    }
    $validFrames++
    if ($seen[$recordOffset] -and $recovered[$recordOffset] -ne $value) {
        $conflicts++
    } else {
        $recovered[$recordOffset] = $value
        $seen[$recordOffset] = $true
        $counts[$recordOffset]++
    }
    $captureOffset += 7
}

$missing = 0
$minimumObserved = [int]::MaxValue
$mismatches = 0
$firstMismatch = $null
for ($index = 0; $index -lt $expected.Length; $index++) {
    $minimumObserved = [Math]::Min($minimumObserved, $counts[$index])
    if ($counts[$index] -lt $MinimumCopies) { $missing++ }
    if ($seen[$index] -and $recovered[$index] -ne $expected[$index]) {
        $mismatches++
        if ($null -eq $firstMismatch) {
            $firstMismatch = [ordered]@{
                offset = $index
                flash_address = "0x$(($FlashBase + $index).ToString('x6'))"
                expected = "0x$($expected[$index].ToString('x2'))"
                actual = "0x$($recovered[$index].ToString('x2'))"
            }
        }
    }
}

if (-not [string]::IsNullOrWhiteSpace($RecoveredPath) -and $missing -eq 0) {
    $recoveredOutput = $RecoveredPath
    if (-not [System.IO.Path]::IsPathRooted($recoveredOutput)) {
        $recoveredOutput = Join-Path (Get-Location) $recoveredOutput
    }
    $parent = [System.IO.Path]::GetDirectoryName($recoveredOutput)
    if ($parent) { [System.IO.Directory]::CreateDirectory($parent) | Out-Null }
    [System.IO.File]::WriteAllBytes($recoveredOutput, $recovered)
}

$expectedSha = (Get-FileHash -Algorithm SHA256 -LiteralPath $expectedFile).Hash.ToLowerInvariant()
$recoveredSha = if ($missing -eq 0) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($sha.ComputeHash($recovered))).Replace("-", "").ToLowerInvariant() }
    finally { $sha.Dispose() }
} else { $null }

$reason = if ($capture.Length -eq 0) { "empty_capture" }
    elseif ($validFrames -eq 0) { "no_valid_frames" }
    elseif ($outOfRange -ne 0) { "out_of_range_frame" }
    elseif ($conflicts -ne 0) { "conflicting_frames" }
    elseif ($missing -ne 0) { "incomplete_coverage" }
    elseif ($mismatches -ne 0) { "content_mismatch" }
    else { "exact_match" }
$passed = $reason -eq "exact_match"
$message = if ($passed) {
    "Flash readback exactly matches $($expected.Length) expected bytes; every offset was observed at least $MinimumCopies time(s)."
} elseif ($reason -eq "content_mismatch") {
    "Flash readback differs at $mismatches byte(s); first mismatch is at $($firstMismatch.flash_address)."
} else {
    "Flash readback validation failed: $reason (valid=$validFrames, missing=$missing, conflicts=$conflicts)."
}

$result = [ordered]@{
    schema = 1
    outcome = if ($passed) { "passed" } else { "failed" }
    reason = $reason
    message = $message
    flash_base = "0x$($FlashBase.ToString('x6'))"
    expected_bytes = $expected.Length
    capture_bytes = $capture.Length
    frame_candidates = $candidates
    valid_frames = $validFrames
    bad_checksum_frames = $badChecksums
    out_of_range_frames = $outOfRange
    conflicting_frames = $conflicts
    offsets_below_minimum_copies = $missing
    minimum_observations_per_offset = $minimumObserved
    mismatched_bytes = $mismatches
    first_mismatch = $firstMismatch
    expected_sha256 = $expectedSha
    recovered_sha256 = $recoveredSha
}

if (-not [string]::IsNullOrWhiteSpace($ResultPath)) {
    $resultOutput = $ResultPath
    if (-not [System.IO.Path]::IsPathRooted($resultOutput)) {
        $resultOutput = Join-Path (Get-Location) $resultOutput
    }
    $parent = [System.IO.Path]::GetDirectoryName($resultOutput)
    if ($parent) { [System.IO.Directory]::CreateDirectory($parent) | Out-Null }
    $result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $resultOutput -Encoding UTF8
}

if (-not $passed) {
    [Console]::Error.WriteLine($message)
    exit 1
}
Write-Output $message
