param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][byte]$TestId,
    [int]$MinimumSuccessFrames = 2,
    [int]$MaximumAgeSeconds = 30,
    [string]$ResultPath
)

$ErrorActionPreference = "Stop"
$result = [ordered]@{
    schema = 1
    outcome = "invalid"
    reason = $null
    message = $null
    expected_test_id = "0x$($TestId.ToString('x2'))"
    capture_bytes = 0
    ddht_candidates = 0
    ddht_success_frames = 0
    ddht_failure_frames = 0
    ddht_wrong_test_frames = 0
    ddht_bad_checksum_frames = 0
    boot_error_candidates = 0
    boot_error_frames = 0
    boot_bad_checksum_frames = 0
    boot_errors = @()
}

function Save-Result {
    if ([string]::IsNullOrWhiteSpace($ResultPath)) { return }
    $output = $ResultPath
    if (-not [System.IO.Path]::IsPathRooted($output)) {
        $output = Join-Path (Get-Location) $output
    }
    $parent = [System.IO.Path]::GetDirectoryName($output)
    if ($parent) { [System.IO.Directory]::CreateDirectory($parent) | Out-Null }
    $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $output -Encoding UTF8
}

function Stop-Check {
    param([string]$Reason, [string]$Message)
    $result["outcome"] = "failed"
    $result["reason"] = $Reason
    $result["message"] = $Message
    Save-Result
    [Console]::Error.WriteLine($Message)
    exit 1
}

function Get-StageName {
    param([byte]$Stage)
    switch ($Stage) {
        1 { return "Stage0" }
        2 { return "Stage1" }
        default { return "stage-$Stage" }
    }
}

function Get-CategoryName {
    param([byte]$Category)
    switch ($Category) {
        1 { return "descriptor" }
        2 { return "manifest" }
        3 { return "dma" }
        4 { return "entry" }
        5 { return "internal" }
        default { return "category-$Category" }
    }
}

try {
    $resolved = Resolve-Path -LiteralPath $Path
    $file = Get-Item -LiteralPath $resolved
    $bytes = [System.IO.File]::ReadAllBytes($resolved)
}
catch {
    Stop-Check "capture_read" "Cannot read UART capture '$Path': $($_.Exception.Message)"
}

$result["capture_bytes"] = $bytes.Length
if ($MaximumAgeSeconds -gt 0) {
    $age = (Get-Date) - $file.LastWriteTime
    if ($age.TotalSeconds -gt $MaximumAgeSeconds) {
        Stop-Check "stale_capture" "UART capture is stale ($([math]::Round($age.TotalSeconds, 1)) seconds old; maximum is $MaximumAgeSeconds)."
    }
}
if ($bytes.Length -eq 0) {
    Stop-Check "empty_capture" "UART capture contains zero bytes. Treat this as a host/VCP/physical transport failure; do not infer a DUT failure."
}

$successCount = 0
$failureCount = 0
$wrongTestCount = 0
$badChecksumCount = 0
$ddhtCandidates = 0
$failureStatuses = @{}
$bootCandidates = 0
$bootBadChecksumCount = 0
$bootFrameCount = 0
$bootErrors = @{}

for ($offset = 0; $offset -lt $bytes.Length; $offset++) {
    if ($offset -le $bytes.Length - 10 -and
        $bytes[$offset] -eq 0x43 -and $bytes[$offset + 1] -eq 0x56 -and
        $bytes[$offset + 2] -eq 0x33 -and $bytes[$offset + 3] -eq 0x42) {
        $bootCandidates++
        [byte]$checksum = 0
        for ($index = 0; $index -lt 9; $index++) {
            $checksum = $checksum -bxor $bytes[$offset + $index]
        }
        if ($checksum -ne $bytes[$offset + 9]) {
            $bootBadChecksumCount++
            $offset += 9
            continue
        }

        $bootFrameCount++
        [byte]$stage = $bytes[$offset + 4]
        [byte]$category = $bytes[$offset + 5]
        [byte]$code = $bytes[$offset + 6]
        [uint16]$detail = [uint16]$bytes[$offset + 7] -bor ([uint16]$bytes[$offset + 8] -shl 8)
        $key = "$stage/$category/$code/$detail"
        if ($bootErrors.ContainsKey($key)) {
            $bootErrors[$key]["count"]++
        } else {
            $bootErrors[$key] = [ordered]@{
                stage = $stage
                stage_name = Get-StageName $stage
                category = $category
                category_name = Get-CategoryName $category
                code = $code
                detail = $detail
                count = 1
            }
        }
        $offset += 9
        continue
    }

    if ($offset -gt $bytes.Length - 8 -or
        $bytes[$offset] -ne 0x44 -or $bytes[$offset + 1] -ne 0x44 -or
        $bytes[$offset + 2] -ne 0x48 -or $bytes[$offset + 3] -ne 0x54 -or
        $bytes[$offset + 4] -ne 0x01) {
        continue
    }
    $ddhtCandidates++

    [byte]$checksum = 0
    for ($index = 0; $index -lt 7; $index++) {
        $checksum = $checksum -bxor $bytes[$offset + $index]
    }
    if ($checksum -ne $bytes[$offset + 7]) {
        $badChecksumCount++
        $offset += 7
        continue
    }
    if ($bytes[$offset + 5] -ne $TestId) {
        $wrongTestCount++
        $offset += 7
        continue
    }
    if ($bytes[$offset + 6] -eq 0) {
        $successCount++
    } else {
        $failureCount++
        $status = $bytes[$offset + 6]
        $key = "0x$($status.ToString('x2'))"
        if ($failureStatuses.ContainsKey($key)) {
            $failureStatuses[$key]++
        } else {
            $failureStatuses[$key] = 1
        }
    }
    $offset += 7
}

$decodedBootErrors = @($bootErrors.Values | Sort-Object stage, category, code, detail)
$result["ddht_candidates"] = $ddhtCandidates
$result["ddht_success_frames"] = $successCount
$result["ddht_failure_frames"] = $failureCount
$result["ddht_wrong_test_frames"] = $wrongTestCount
$result["ddht_bad_checksum_frames"] = $badChecksumCount
$result["boot_error_candidates"] = $bootCandidates
$result["boot_error_frames"] = $bootFrameCount
$result["boot_bad_checksum_frames"] = $bootBadChecksumCount
$result["boot_errors"] = $decodedBootErrors

if ($bootFrameCount -ne 0) {
    $summary = ($decodedBootErrors | ForEach-Object {
        $stageText = $_["stage_name"]
        $categoryText = $_["category_name"]
        $codeText = ([byte]$_['code']).ToString('x2')
        $detailText = ([uint16]$_['detail']).ToString('x4')
        "$stageText/$categoryText/code=0x$codeText/detail=0x${detailText}:$($_['count'])"
    }) -join ", "
    Stop-Check "boot_error" "CPU V3 boot reported $bootFrameCount valid error frame(s) ($summary). This is a DUT result, not a UART transport failure."
}
if ($bootBadChecksumCount -ne 0) {
    Stop-Check "boot_checksum" "UART capture contains $bootBadChecksumCount CV3B candidate frame(s) with a bad checksum. Boot-error data was corrupted in transport."
}
if ($badChecksumCount -ne 0) {
    $tolerated = if ($successCount -gt 0) {
        [Math]::Max(1, [Math]::Floor($successCount * 0.01))
    } else { 0 }
    if ($badChecksumCount -gt $tolerated) {
        Stop-Check "ddht_checksum" "UART capture contains $badChecksumCount DDHT frame(s) with a bad checksum (tolerance is $tolerated for $successCount success frame(s))."
    }
    Write-Host "UART capture contains $badChecksumCount torn DDHT frame(s) within transport tolerance; continuing."
}
if ($wrongTestCount -ne 0) {
    Stop-Check "wrong_test" "UART capture contains $wrongTestCount valid DDHT frame(s) for a different test ID."
}
if ($failureCount -ne 0) {
    $summary = ($failureStatuses.GetEnumerator() | Sort-Object Name | ForEach-Object {
        "$($_.Name):$($_.Value)"
    }) -join ", "
    Stop-Check "ddht_failure" "UART test 0x$($TestId.ToString('x2')) reported $failureCount failure frame(s) ($summary)."
}
if ($successCount -lt $MinimumSuccessFrames) {
    if ($ddhtCandidates -eq 0) {
        $preview = ($bytes | Select-Object -First 32 | ForEach-Object { $_.ToString('x2') }) -join " "
        Stop-Check "no_status_frame" "UART captured $($bytes.Length) byte(s), but none begin a DDHT v1 or valid CV3B frame. First bytes: $preview. Treat this as framing/baud/physical corruption, not a DUT result."
    }
    Stop-Check "insufficient_success" "UART test 0x$($TestId.ToString('x2')) produced only $successCount valid success frame(s) from $ddhtCandidates DDHT candidate(s); expected at least $MinimumSuccessFrames."
}

$result["outcome"] = "passed"
$result["reason"] = "ddht_success"
$result["message"] = "UART test 0x$($TestId.ToString('x2')) passed ($successCount valid success frame(s), no failure frames)."
Save-Result
Write-Host $result["message"]
