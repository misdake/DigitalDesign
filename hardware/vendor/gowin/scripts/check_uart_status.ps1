param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [Parameter(Mandatory = $true)]
    [byte]$TestId,

    [int]$MinimumSuccessFrames = 2,
    [int]$MaximumAgeSeconds = 30
)

$ErrorActionPreference = "Stop"

try {
    $resolved = Resolve-Path -LiteralPath $Path
    $file = Get-Item -LiteralPath $resolved
    $bytes = [System.IO.File]::ReadAllBytes($resolved)
}
catch {
    [Console]::Error.WriteLine("Cannot read UART capture '$Path': $($_.Exception.Message)")
    exit 1
}

if ($MaximumAgeSeconds -gt 0) {
    $age = (Get-Date) - $file.LastWriteTime
    if ($age.TotalSeconds -gt $MaximumAgeSeconds) {
        Write-Error "UART capture is stale ($([math]::Round($age.TotalSeconds, 1)) seconds old; maximum is $MaximumAgeSeconds)."
        exit 1
    }
}

$successCount = 0
$failureCount = 0
$wrongTestCount = 0
$badChecksumCount = 0

for ($offset = 0; $offset -le $bytes.Length - 8; $offset++) {
    if ($bytes[$offset] -ne 0x44 -or
        $bytes[$offset + 1] -ne 0x44 -or
        $bytes[$offset + 2] -ne 0x48 -or
        $bytes[$offset + 3] -ne 0x54 -or
        $bytes[$offset + 4] -ne 0x01) {
        continue
    }

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
    }
    $offset += 7
}

if ($badChecksumCount -ne 0) {
    # A raw serial capture can drop a byte on the host side; a torn frame is
    # indistinguishable from DUT corruption only in isolation. Tolerate a
    # small number of transport errors while keeping failure frames fatal.
    $tolerated = [Math]::Max(1, [Math]::Floor($successCount * 0.01))
    if ($badChecksumCount -gt $tolerated) {
        Write-Error "UART capture contains $badChecksumCount status frame(s) with a bad checksum (tolerance is $tolerated for $successCount success frame(s))."
        exit 1
    }
    Write-Host "UART capture contains $badChecksumCount torn frame(s) within transport tolerance; continuing."
}

if ($wrongTestCount -ne 0) {
    Write-Error "UART capture contains $wrongTestCount valid status frame(s) for a different test ID."
    exit 1
}

if ($failureCount -ne 0) {
    Write-Error "UART test 0x$($TestId.ToString('x2')) reported $failureCount failure frame(s)."
    exit 1
}

if ($successCount -lt $MinimumSuccessFrames) {
    Write-Error "UART test 0x$($TestId.ToString('x2')) produced only $successCount valid success frame(s); expected at least $MinimumSuccessFrames."
    exit 1
}

Write-Host "UART test 0x$($TestId.ToString('x2')) passed ($successCount valid success frame(s), no failure frames)."
