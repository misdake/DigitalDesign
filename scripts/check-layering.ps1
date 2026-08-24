$ErrorActionPreference = "Stop"
$metadata = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
$packages = @{}
foreach ($package in $metadata.packages) { $packages[$package.name] = $package }

$forbidden = @{
    "digital-design-circuit" = @("digital-design-hardware", "digital-design-hardware-common", "digital-design-hardware-gowin", "digital-design-ip-common", "cpu-v1", "cpu-v2", "cpu-v3", "rcc", "cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
    "digital-design-hardware" = @("digital-design-hardware-gowin", "digital-design-ip-common", "cpu-v1", "cpu-v2", "cpu-v3", "rcc", "cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
    "digital-design-hardware-common" = @("digital-design-hardware-gowin", "cpu-v1", "cpu-v2", "cpu-v3", "rcc", "cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
    "digital-design-hardware-gowin" = @("digital-design-ip-common", "cpu-v1", "cpu-v2", "cpu-v3", "rcc", "cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
    "digital-design-ip-common" = @("cpu-v1", "cpu-v2", "cpu-v3", "rcc", "cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
    "cpu-v1" = @("cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
    "cpu-v2" = @("cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
    "cpu-v3" = @("cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
    "rcc" = @("digital-design-hardware", "digital-design-hardware-common", "digital-design-hardware-gowin", "cpu-v1", "cpu-v2", "cpu-v3", "cpu-v1-sim", "cpu-v2-sim", "cpu-v3-tang-nano-20k")
}

$errors = @()
foreach ($owner in $forbidden.Keys) {
    $dependencies = @($packages[$owner].dependencies | ForEach-Object { $_.name })
    foreach ($name in $forbidden[$owner]) {
        if ($dependencies -contains $name) { $errors += "$owner must not depend on $name" }
    }
}
if ($errors.Count -ne 0) { throw ($errors -join [Environment]::NewLine) }
Write-Host "Layering constraints passed."
