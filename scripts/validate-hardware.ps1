param(
    [ValidateSet("quick", "iverilog", "audit", "pnr", "all")]
    [string]$Mode = "quick"
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$evidenceDirectory = Join-Path $repoRoot "target/hardware-validation"
$completedSteps = [System.Collections.Generic.List[string]]::new()

function Invoke-ValidationStep {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Executable,
        [string[]]$Arguments = @()
    )

    Write-Host ""
    Write-Host "== $Name =="
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
    $completedSteps.Add($Name)
}

function Invoke-Cargo {
    param([string]$Name, [string[]]$Arguments)
    Invoke-ValidationStep -Name $Name -Executable "cargo" -Arguments $Arguments
}

function Invoke-BootArtifactValidation {
    $bootDirectory = Join-Path $repoRoot "target/cpu-v3-boot"
    $manifest = Join-Path $repoRoot "systems/cpu-v3-tang-nano-20k/examples/cpu_v3_boot/boot.cpu-v3-manifest"
    $generatedPackage = Join-Path $bootDirectory "cpu-v3-boot.bin"
    $repackedPackage = Join-Path $bootDirectory "cpu-v3-boot.repacked.bin"
    $repackedMap = Join-Path $bootDirectory "cpu-v3-boot.repacked.map"

    Invoke-Cargo "materialize generated CPU V3 boot assets" @(
        "run", "-p", "cpu-v3-tang-nano-20k", "--bin", "cpu-v3-boot-assets", "--", $bootDirectory
    )
    Invoke-Cargo "repack CPU V3 boot manifest" @(
        "run", "-p", "cpu-v3-tang-nano-20k", "--bin", "cpu-v3-pack", "--",
        $manifest, "-o", $repackedPackage, "--map", $repackedMap
    )

    Write-Host ""
    Write-Host "== boot package byte-for-byte comparison =="
    $generated = [System.IO.File]::ReadAllBytes($generatedPackage)
    $repacked = [System.IO.File]::ReadAllBytes($repackedPackage)
    if ($generated.Length -ne $repacked.Length) {
        throw "repacked boot package has $($repacked.Length) bytes; generated package has $($generated.Length)"
    }
    for ($index = 0; $index -lt $generated.Length; $index++) {
        if ($generated[$index] -ne $repacked[$index]) {
            throw "repacked boot package differs at byte $index"
        }
    }
    Write-Host "Boot package is reproducible ($($generated.Length) identical bytes)."
    $completedSteps.Add("boot package byte-for-byte comparison")
}

function Invoke-QuickValidation {
    Invoke-Cargo "workspace tests" @("test", "--workspace")
    Invoke-Cargo "strict workspace clippy" @(
        "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"
    )
    Invoke-ValidationStep -Name "layering constraints" -Executable (Join-Path $repoRoot "scripts/check-layering.ps1")
    Invoke-ValidationStep -Name "source hygiene constraints" -Executable (Join-Path $repoRoot "scripts/check-source-hygiene.ps1")
    Invoke-BootArtifactValidation
}

function Invoke-IverilogValidation {
    Invoke-Cargo "common hardware Icarus" @(
        "test", "-p", "digital-design-hardware-common", "--", "--ignored", "--nocapture"
    )
    Invoke-Cargo "Gowin primitive Icarus" @(
        "test", "-p", "digital-design-hardware-gowin", "--lib", "--", "--ignored", "--nocapture"
    )
    Invoke-Cargo "Gowin example Icarus" @(
        "test", "-p", "digital-design-hardware-gowin", "--examples", "--", "--ignored", "--nocapture"
    )
    Invoke-Cargo "CPU V3 RTL Icarus" @(
        "test", "-p", "cpu-v3", "--", "--ignored", "--nocapture"
    )
    Invoke-Cargo "CPU V3 system RTL Icarus" @(
        "test", "-p", "cpu-v3-tang-nano-20k", "--lib", "--", "--ignored", "--nocapture"
    )
    Invoke-Cargo "CPU V3 system example Icarus" @(
        "test", "-p", "cpu-v3-tang-nano-20k", "--examples", "--", "--ignored", "--nocapture"
    )
}

function Invoke-GowinBuild {
    param([string]$Package, [string]$Example)
    Invoke-Cargo "$Example Gowin build" @(
        "run", "-p", $Package, "--example", $Example, "--", "--build"
    )
    Invoke-GowinAudit $Package $Example
}

function Invoke-GowinAudit {
    param([string]$Package, [string]$Example)
    Invoke-Cargo "$Example artifact audit" @(
        "run", "-p", $Package, "--example", $Example, "--", "--check-existing"
    )
}

function Invoke-AuditValidation {
    Invoke-GowinAudit "digital-design-hardware-gowin" "board_health"
    Invoke-GowinAudit "cpu-v3-tang-nano-20k" "cpu_v3_cpu"
    Invoke-GowinAudit "cpu-v3-tang-nano-20k" "cpu_v3_sdram"
    Invoke-GowinAudit "cpu-v3-tang-nano-20k" "cpu_v3_boot"
}

function Invoke-PnrValidation {
    Invoke-GowinBuild "digital-design-hardware-gowin" "board_health"
    Invoke-GowinBuild "cpu-v3-tang-nano-20k" "cpu_v3_cpu"
    Invoke-GowinBuild "cpu-v3-tang-nano-20k" "cpu_v3_sdram"
    Invoke-GowinBuild "cpu-v3-tang-nano-20k" "cpu_v3_boot"
}

Push-Location $repoRoot
try {
    $startedAt = [DateTime]::UtcNow
    if ($Mode -eq "quick" -or $Mode -eq "all") { Invoke-QuickValidation }
    if ($Mode -eq "iverilog" -or $Mode -eq "all") { Invoke-IverilogValidation }
    if ($Mode -eq "audit") { Invoke-AuditValidation }
    if ($Mode -eq "pnr" -or $Mode -eq "all") { Invoke-PnrValidation }

    New-Item -ItemType Directory -Force -Path $evidenceDirectory | Out-Null
    $commit = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw "git rev-parse failed" }
    $dirty = @(& git status --porcelain).Count -ne 0
    if ($LASTEXITCODE -ne 0) { throw "git status failed" }
    $evidence = [ordered]@{
        schema = 1
        mode = $Mode
        commit = $commit
        worktree_dirty = $dirty
        started_utc = $startedAt.ToString("o")
        completed_utc = [DateTime]::UtcNow.ToString("o")
        completed_steps = @($completedSteps)
    }
    $evidencePath = Join-Path $evidenceDirectory "$Mode.json"
    $evidence | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $evidencePath -Encoding UTF8
    Write-Host ""
    Write-Host "Hardware validation '$Mode' passed. Evidence: $evidencePath"
} finally {
    Pop-Location
}
