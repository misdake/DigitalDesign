param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("board-health", "cpu-v3-cpu", "cpu-v3-sdram", "cpu-v3-boot")]
    [string]$Profile,

    [ValidateSet("Audit", "Observe", "Program", "Full")]
    [string]$Mode = "Audit",

    [string]$Port,
    [switch]$WriteBootFlash,
    [int]$CaptureSeconds = 8,
    [int]$PortWaitSeconds = 15,
    [int]$MinimumSuccessFrames = 2
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../../../..")
$completedStages = [System.Collections.Generic.List[string]]::new()
$currentStage = "argument validation"
$failure = $null
$capturePath = $null
$bootPackagePath = $null

function Get-ProfileConfiguration {
    param([string]$Name)

    switch ($Name) {
        "board-health" {
            return @{
                Package = "digital-design-hardware-gowin"
                Example = "board_health"
                Output = "target/board_health_gowin"
                TestId = 0x0a
            }
        }
        "cpu-v3-cpu" {
            return @{
                Package = "cpu-v3-tang-nano-20k"
                Example = "cpu_v3_cpu"
                Output = "target/cpu_v3_cpu_gowin"
                TestId = 0x04
            }
        }
        "cpu-v3-sdram" {
            return @{
                Package = "cpu-v3-tang-nano-20k"
                Example = "cpu_v3_sdram"
                Output = "target/cpu_v3_sdram_gowin"
                TestId = 0x05
            }
        }
        "cpu-v3-boot" {
            return @{
                Package = "cpu-v3-tang-nano-20k"
                Example = "cpu_v3_boot"
                Output = "target/cpu_v3_boot_gowin"
                TestId = 0x07
            }
        }
    }
    throw "unknown board-validation profile '$Name'"
}

function Invoke-Stage {
    param(
        [string]$Name,
        [string]$Executable,
        [string[]]$Arguments
    )

    $script:currentStage = $Name
    Write-Host ""
    Write-Host "== $Name =="
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
    $script:completedStages.Add($Name)
}

function Invoke-CargoStage {
    param([string]$Name, [string[]]$Arguments)
    Invoke-Stage -Name $Name -Executable "cargo" -Arguments $Arguments
}

function Wait-SerialPort {
    param([string]$Name, [int]$TimeoutSeconds)

    $script:currentStage = "wait for serial port"
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        if ([System.IO.Ports.SerialPort]::GetPortNames() -contains $Name) {
            $script:completedStages.Add("wait for serial port")
            return
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "serial port '$Name' did not appear within $TimeoutSeconds seconds; no USB reset or programming retry was attempted"
}

function Read-KeyValueFile {
    param([string]$Path)

    $values = @{}
    foreach ($line in Get-Content -LiteralPath $Path) {
        $parts = $line -split "=", 2
        if ($parts.Count -eq 2) { $values[$parts[0]] = $parts[1] }
    }
    return $values
}

if (($Mode -eq "Observe" -or $Mode -eq "Full") -and [string]::IsNullOrWhiteSpace($Port)) {
    throw "-$Mode requires -Port"
}
if ($CaptureSeconds -le 0 -or $PortWaitSeconds -lt 0 -or $MinimumSuccessFrames -le 0) {
    throw "capture duration and minimum frame count must be positive; port wait cannot be negative"
}
if ($WriteBootFlash -and $Profile -ne "cpu-v3-boot") {
    throw "-WriteBootFlash is valid only for the cpu-v3-boot profile"
}
if ($WriteBootFlash -and $Mode -ne "Program" -and $Mode -ne "Full") {
    throw "-WriteBootFlash requires -Mode Program or -Mode Full"
}

$configuration = Get-ProfileConfiguration $Profile
$startedAt = [DateTime]::UtcNow
$runId = $startedAt.ToString("yyyyMMddTHHmmssfffZ")
$runDirectory = Join-Path $repoRoot "target/board-validation/$Profile/$runId"
New-Item -ItemType Directory -Force -Path $runDirectory | Out-Null
$capturePath = Join-Path $runDirectory "uart.bin"

Push-Location $repoRoot
try {
    if ($Mode -ne "Observe") {
        Invoke-CargoStage "audit existing bitstream" @(
            "run", "-p", $configuration.Package, "--example", $configuration.Example,
            "--", "--check-existing"
        )
    }

    if ($WriteBootFlash) {
        $bootAssetsDirectory = Join-Path $repoRoot "target/cpu-v3-boot"
        $bootPackagePath = Join-Path $bootAssetsDirectory "cpu-v3-boot.bin"
        Invoke-CargoStage "materialize generated boot package" @(
            "run", "-p", "cpu-v3-tang-nano-20k", "--bin", "cpu-v3-boot-assets",
            "--", $bootAssetsDirectory
        )
        Invoke-CargoStage "program external boot Flash once" @(
            "run", "-p", $configuration.Package, "--example", $configuration.Example,
            "--", "--program-flash", "0x100000", $bootPackagePath
        )
    }

    if ($Mode -eq "Program" -or $Mode -eq "Full") {
        Invoke-CargoStage "program audited SRAM bitstream once" @(
            "run", "-p", $configuration.Package, "--example", $configuration.Example,
            "--", "--program-existing"
        )
    }

    if ($Mode -eq "Observe" -or $Mode -eq "Full") {
        Wait-SerialPort -Name $Port -TimeoutSeconds $PortWaitSeconds
        Invoke-Stage "capture UART" "powershell" @(
            "-ExecutionPolicy", "Bypass", "-File", (Join-Path $PSScriptRoot "capture_uart.ps1"),
            "-Port", $Port, "-Seconds", $CaptureSeconds, "-Out", $capturePath
        )
        Invoke-Stage "validate DDHT status" "powershell" @(
            "-ExecutionPolicy", "Bypass", "-File", (Join-Path $PSScriptRoot "check_uart_status.ps1"),
            "-Path", $capturePath, "-TestId", $configuration.TestId,
            "-MinimumSuccessFrames", $MinimumSuccessFrames,
            "-MaximumAgeSeconds", ($CaptureSeconds + 30)
        )
    }
}
catch {
    $failure = $_.Exception.Message
}
finally {
    Pop-Location

    $artifactDirectory = Join-Path $repoRoot $configuration.Output
    $artifactManifestPath = Join-Path $artifactDirectory "gowin-build.manifest"
    $artifact = $null
    if (Test-Path -LiteralPath $artifactManifestPath) {
        $manifest = Read-KeyValueFile $artifactManifestPath
        $bitstreamPath = if ($manifest.ContainsKey("bitstream_path")) {
            Join-Path $artifactDirectory $manifest["bitstream_path"]
        } else { $null }
        $artifact = [ordered]@{
            manifest = $configuration.Output + "/gowin-build.manifest"
            manifest_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $artifactManifestPath).Hash.ToLowerInvariant()
            source_fingerprint = $manifest["source_fingerprint"]
            bitstream_fingerprint = $manifest["bitstream_fingerprint"]
            bitstream_sha256 = if ($bitstreamPath -and (Test-Path -LiteralPath $bitstreamPath)) {
                (Get-FileHash -Algorithm SHA256 -LiteralPath $bitstreamPath).Hash.ToLowerInvariant()
            } else { $null }
        }
    }

    $commit = (& git -C $repoRoot rev-parse HEAD).Trim()
    $dirty = @(& git -C $repoRoot status --porcelain).Count -ne 0
    $evidence = [ordered]@{
        schema = 1
        profile = $Profile
        mode = $Mode
        result = if ($failure) { "failed" } else { "passed" }
        commit = $commit
        worktree_dirty = $dirty
        started_utc = $startedAt.ToString("o")
        completed_utc = [DateTime]::UtcNow.ToString("o")
        completed_stages = @($completedStages)
        failed_stage = if ($failure) { $currentStage } else { $null }
        failure = $failure
        artifact_audited = $completedStages.Contains("audit existing bitstream")
        boot_flash_programmed = $completedStages.Contains("program external boot Flash once")
        sram_programmed = $completedStages.Contains("program audited SRAM bitstream once")
        uart_validated = $completedStages.Contains("validate DDHT status")
        port = if ($Port) { $Port } else { $null }
        expected_test_id = "0x$($configuration.TestId.ToString('x2'))"
        artifact = $artifact
        boot_package_sha256 = if ($bootPackagePath -and (Test-Path -LiteralPath $bootPackagePath)) {
            (Get-FileHash -Algorithm SHA256 -LiteralPath $bootPackagePath).Hash.ToLowerInvariant()
        } else { $null }
        uart_capture_sha256 = if (Test-Path -LiteralPath $capturePath) {
            (Get-FileHash -Algorithm SHA256 -LiteralPath $capturePath).Hash.ToLowerInvariant()
        } else { $null }
    }
    $evidencePath = Join-Path $runDirectory "evidence.json"
    $evidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $evidencePath -Encoding UTF8
    Write-Host ""
    Write-Host "Board validation evidence: $evidencePath"
}

if ($failure) {
    [Console]::Error.WriteLine("Board validation stopped at '$currentStage': $failure")
    exit 1
}
Write-Host "Board validation '$Profile'/$Mode passed."
