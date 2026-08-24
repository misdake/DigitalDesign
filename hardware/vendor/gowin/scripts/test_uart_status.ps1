$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../../../..")
$testDirectory = Join-Path $repoRoot "target/uart-status-self-test"
$checker = Join-Path $PSScriptRoot "check_uart_status.ps1"
New-Item -ItemType Directory -Force -Path $testDirectory | Out-Null

function Add-XorChecksum {
    param([byte[]]$Body)
    [byte]$checksum = 0
    foreach ($byte in $Body) { $checksum = $checksum -bxor $byte }
    return [byte[]]($Body + $checksum)
}

function New-DdhtFrame {
    param([byte]$TestId, [byte]$Status)
    return Add-XorChecksum ([byte[]](0x44, 0x44, 0x48, 0x54, 1, $TestId, $Status))
}

function Invoke-CheckerCase {
    param(
        [string]$Name,
        [byte[]]$Bytes,
        [bool]$ShouldPass,
        [string]$ExpectedReason
    )

    $capture = Join-Path $testDirectory "$Name.bin"
    $resultPath = Join-Path $testDirectory "$Name.json"
    [System.IO.File]::WriteAllBytes($capture, $Bytes)
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $checker `
        -Path $capture -TestId 0x07 -MinimumSuccessFrames 2 -MaximumAgeSeconds 0 `
        -ResultPath $resultPath 2>&1
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $previousErrorAction
    if ($ShouldPass -and $exitCode -ne 0) {
        throw "$Name unexpectedly failed: $($output -join [Environment]::NewLine)"
    }
    if (-not $ShouldPass -and $exitCode -eq 0) {
        throw "$Name unexpectedly passed"
    }
    $decoded = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    if ($decoded.reason -ne $ExpectedReason) {
        throw "$Name reason is '$($decoded.reason)', expected '$ExpectedReason'"
    }
    return $decoded
}

$success = New-DdhtFrame 0x07 0
$bootError = Add-XorChecksum ([byte[]](0x43, 0x56, 0x33, 0x42, 2, 2, 6, 0x34, 0x12))

$decoded = Invoke-CheckerCase "success" ([byte[]](@(0xaa) + $success + $success)) $true "ddht_success"
if ($decoded.ddht_success_frames -ne 2 -or $decoded.boot_error_frames -ne 0) {
    throw "success case counts are incorrect"
}

$decoded = Invoke-CheckerCase "boot-error" ([byte[]]($bootError + $bootError)) $false "boot_error"
$errorReport = $decoded.boot_errors[0]
if ($decoded.boot_error_frames -ne 2 -or $errorReport.stage_name -ne "Stage1" -or
    $errorReport.category_name -ne "manifest" -or $errorReport.code -ne 6 -or
    $errorReport.detail -ne 0x1234 -or $errorReport.count -ne 2) {
    throw "boot-error case did not preserve its structured report"
}

$corruptBootError = [byte[]]$bootError.Clone()
$corruptBootError[$corruptBootError.Length - 1] = $corruptBootError[$corruptBootError.Length - 1] -bxor 1
$decoded = Invoke-CheckerCase "boot-checksum" $corruptBootError $false "boot_checksum"
if ($decoded.boot_bad_checksum_frames -ne 1 -or $decoded.boot_error_frames -ne 0) {
    throw "corrupt boot-error case counts are incorrect"
}

$decoded = Invoke-CheckerCase "mixed" ([byte[]]($success + $success + $bootError)) $false "boot_error"
if ($decoded.ddht_success_frames -ne 2 -or $decoded.boot_error_frames -ne 1) {
    throw "a valid boot error did not take precedence over DDHT success"
}

$failure = New-DdhtFrame 0x07 0x35
$decoded = Invoke-CheckerCase "ddht-failure" ([byte[]]($failure + $failure)) $false "ddht_failure"
if ($decoded.ddht_failure_frames -ne 2) { throw "DDHT failure count is incorrect" }

$wrongTest = New-DdhtFrame 0x04 0
$decoded = Invoke-CheckerCase "wrong-test" ([byte[]]($wrongTest + $wrongTest)) $false "wrong_test"
if ($decoded.ddht_wrong_test_frames -ne 2) { throw "wrong-test count is incorrect" }

$torn = [byte[]]$success.Clone()
$torn[$torn.Length - 1] = $torn[$torn.Length - 1] -bxor 1
$decoded = Invoke-CheckerCase "one-torn-ddht" ([byte[]]($success + $torn + $success)) $true "ddht_success"
if ($decoded.ddht_bad_checksum_frames -ne 1) { throw "torn DDHT count is incorrect" }

$decoded = Invoke-CheckerCase "too-many-torn-ddht" ([byte[]]($success + $torn + $torn + $success)) $false "ddht_checksum"
if ($decoded.ddht_bad_checksum_frames -ne 2) { throw "excess torn DDHT count is incorrect" }

Write-Host "UART status decoder self-test passed."
