$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$metadata = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
$packages = @{}
foreach ($package in $metadata.packages) { $packages[$package.name] = $package }

function Get-Layer {
    param([string]$ManifestPath)
    $resolvedManifest = [System.IO.Path]::GetFullPath($ManifestPath)
    $prefix = $repoRoot.TrimEnd("\") + "\"
    if (-not $resolvedManifest.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "workspace manifest is outside the repository: $ManifestPath"
    }
    $relative = $resolvedManifest.Substring($prefix.Length)
    $relative = $relative.Replace("\", "/")
    if ($relative -eq "circuit/Cargo.toml") { return [pscustomobject]@{ Name = "circuit"; Rank = 0; Path = $relative } }
    if ($relative.StartsWith("hardware/")) { return [pscustomobject]@{ Name = "hardware"; Rank = 1; Path = $relative } }
    if ($relative.StartsWith("ip/")) { return [pscustomobject]@{ Name = "ip"; Rank = 2; Path = $relative } }
    if ($relative.StartsWith("compiler/")) { return [pscustomobject]@{ Name = "compiler"; Rank = 2; Path = $relative } }
    if ($relative.StartsWith("systems/")) { return [pscustomobject]@{ Name = "systems"; Rank = 3; Path = $relative } }
    throw "workspace package has no architecture layer: $relative"
}

$errors = @()
foreach ($owner in $metadata.packages) {
    $ownerLayer = Get-Layer $owner.manifest_path
    foreach ($dependency in $owner.dependencies) {
        if (-not $packages.ContainsKey($dependency.name)) { continue }
        $target = $packages[$dependency.name]
        $targetLayer = Get-Layer $target.manifest_path
        if ($targetLayer.Rank -gt $ownerLayer.Rank) {
            $errors += "$($owner.name) ($($ownerLayer.Name)) must not depend upward on $($target.name) ($($targetLayer.Name))"
        }
        if ($ownerLayer.Name -eq "systems" -and $targetLayer.Name -eq "systems") {
            $errors += "$($owner.name) must compose lower layers, not another complete system $($target.name)"
        }
    }
}

# The target-independent RCC frontend must not acquire a processor backend.
$rccDependencies = @($packages["rcc"].dependencies | ForEach-Object { $_.name })
foreach ($dependency in $rccDependencies) {
    if (-not $packages.ContainsKey($dependency)) { continue }
    $layer = Get-Layer $packages[$dependency].manifest_path
    if ($layer.Path.StartsWith("ip/cpu-") -or $layer.Path.StartsWith("ip/gpu-")) {
        $errors += "rcc must not depend on processor IP $dependency"
    }
}

if ($errors.Count -ne 0) { throw ($errors -join [Environment]::NewLine) }
Write-Host "Layering constraints passed."
