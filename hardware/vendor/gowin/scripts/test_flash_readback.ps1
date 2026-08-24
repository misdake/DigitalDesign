$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../../../..")
$testDirectory = Join-Path $repoRoot "target/flash-readback-script-test"
[System.IO.Directory]::CreateDirectory($testDirectory) | Out-Null
$expectedPath = Join-Path $testDirectory "expected.bin"
$capturePath = Join-Path $testDirectory "capture.bin"
$resultPath = Join-Path $testDirectory "result.json"
$checker = Join-Path $PSScriptRoot "check_flash_readback.ps1"
[byte[]]$expected = 0x43, 0x50, 0x55, 0x33, 0x00, 0xff
[System.IO.File]::WriteAllBytes($expectedPath, $expected)

function Write-Capture {
    param([byte[]]$Values)
    $frames = [System.Collections.Generic.List[byte]]::new()
    for ($copy = 0; $copy -lt 2; $copy++) {
        for ($offset = 0; $offset -lt $Values.Length; $offset++) {
            [byte[]]$frame = 0x46, 0x42, 0x52, 0x31,
                ($offset -band 0xff), (($offset -shr 8) -band 0xff), $Values[$offset], 0
            [byte]$checksum = 0
            for ($index = 0; $index -lt 7; $index++) {
                $checksum = $checksum -bxor $frame[$index]
            }
            $frame[7] = $checksum
            $frames.AddRange($frame)
        }
    }
    [System.IO.File]::WriteAllBytes($capturePath, $frames.ToArray())
}

Write-Capture $expected
& powershell -NoProfile -ExecutionPolicy Bypass -File $checker `
    -CapturePath $capturePath -ExpectedPath $expectedPath -MinimumCopies 2 `
    -ResultPath $resultPath
if ($LASTEXITCODE -ne 0) { throw "matching Flash readback fixture failed" }
$matching = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
if ($matching.reason -ne "exact_match" -or $matching.minimum_observations_per_offset -ne 2) {
    throw "matching Flash readback fixture produced unexpected evidence"
}

[byte[]]$corrupt = $expected.Clone()
$corrupt[3] = $corrupt[3] -bxor 1
Write-Capture $corrupt
& powershell -NoProfile -ExecutionPolicy Bypass -File $checker `
    -CapturePath $capturePath -ExpectedPath $expectedPath -MinimumCopies 2 `
    -ResultPath $resultPath
if ($LASTEXITCODE -ne 1) { throw "corrupt Flash readback fixture did not fail" }
$failed = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
if ($failed.reason -ne "content_mismatch" -or $failed.first_mismatch.flash_address -ne "0x100003") {
    throw "corrupt Flash readback fixture did not locate byte 0x100003"
}

Write-Output "Flash readback decoder self-test passed."
