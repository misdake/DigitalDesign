$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$sourceFiles = @(& git -C $repoRoot ls-files --cached --others --exclude-standard)
if ($LASTEXITCODE -ne 0) { throw "git ls-files failed" }

$absoluteWindowsPath = [regex]::new(
    '(?i)(?:^|[\s"''=(])(?:\\\\\?\\)?[a-z]:[\\/]',
    [System.Text.RegularExpressions.RegexOptions]::CultureInvariant
)
$legacyName = "g" + "16"
$legacyCpuPath = "cpu_v2::" + $legacyName
$legacyG16 = [regex]::new(
    "(?i)\b$([regex]::Escape($legacyName))\b|$([regex]::Escape($legacyCpuPath))",
    [System.Text.RegularExpressions.RegexOptions]::CultureInvariant
)
$manualBootArray = [regex]::new(
    '(?m)^\s*(?:pub\s+)?const\s+(?:STAGE0_PROGRAM|STAGE1_PROGRAM|FLASH_PACKAGE)\s*:',
    [System.Text.RegularExpressions.RegexOptions]::CultureInvariant
)

$errors = [System.Collections.Generic.List[string]]::new()
foreach ($relativePath in $sourceFiles) {
    $path = Join-Path $repoRoot $relativePath
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }

    try {
        $text = [System.IO.File]::ReadAllText($path)
    } catch {
        continue
    }

    if ($absoluteWindowsPath.IsMatch($text)) {
        $errors.Add("absolute Windows path: $relativePath")
    }
    if ($legacyG16.IsMatch($text)) {
        $errors.Add("legacy CPU V3 predecessor name: $relativePath")
    }
    if ($relativePath -notmatch '^systems/cpu-v3-tang-nano-20k/build\.rs$' -and
        $manualBootArray.IsMatch($text)) {
        $errors.Add("hand-maintained generated boot array: $relativePath")
    }
}

if ($errors.Count -ne 0) {
    throw ($errors -join [Environment]::NewLine)
}

Write-Host "Source hygiene constraints passed."
