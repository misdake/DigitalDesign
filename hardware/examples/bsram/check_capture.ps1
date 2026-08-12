param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [int]$MinimumPassBytes = 3
)

$bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
$passCount = ($bytes | Where-Object { $_ -eq 0x50 }).Count
$failCount = ($bytes | Where-Object { $_ -eq 0x46 }).Count

if ($failCount -ne 0) {
    Write-Error "BSRAM self-test reported failure ($failCount F byte(s), $passCount P byte(s))."
    exit 1
}

if ($passCount -lt $MinimumPassBytes) {
    Write-Error "BSRAM self-test produced only $passCount P byte(s); expected at least $MinimumPassBytes."
    exit 1
}

Write-Host "BSRAM hardware self-test passed ($passCount P byte(s), no F bytes)."
