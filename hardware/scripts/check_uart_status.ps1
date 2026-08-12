param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [byte]$SuccessByte = 0x50,
    [byte]$FailureByte = 0x46,
    [int]$MinimumSuccessBytes = 3
)

$bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
$successCount = ($bytes | Where-Object { $_ -eq $SuccessByte }).Count
$failureCount = ($bytes | Where-Object { $_ -eq $FailureByte }).Count

if ($failureCount -ne 0) {
    Write-Error "UART status reported failure ($failureCount failure byte(s), $successCount success byte(s))."
    exit 1
}

if ($successCount -lt $MinimumSuccessBytes) {
    Write-Error "UART status produced only $successCount success byte(s); expected at least $MinimumSuccessBytes."
    exit 1
}

Write-Host "UART status passed ($successCount success byte(s), no failure bytes)."
