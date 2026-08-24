param(
    [Parameter(Mandatory = $true)][string]$Port,
    [int]$Baud = 115200,
    [int]$Seconds = 8,
    [Parameter(Mandatory = $true)][string]$Out
)
$ErrorActionPreference = "Stop"
$p = New-Object System.IO.Ports.SerialPort $Port, $Baud, ([System.IO.Ports.Parity]::None), 8, ([System.IO.Ports.StopBits]::One)
$p.ReadTimeout = 500
$ms = New-Object System.IO.MemoryStream
$buf = New-Object byte[] 4096
$deadline = (Get-Date).AddSeconds($Seconds)
try {
    $p.Open()
    while ((Get-Date) -lt $deadline) {
        $available = $p.BytesToRead
        if ($available -le 0) {
            Start-Sleep -Milliseconds 10
            continue
        }
        $n = $p.Read($buf, 0, [Math]::Min($buf.Length, $available))
        if ($n -gt 0) { $ms.Write($buf, 0, $n) }
    }
} finally {
    if ($p.IsOpen) { $p.Close() }
    $p.Dispose()
}
$outPath = $Out
if (-not [System.IO.Path]::IsPathRooted($outPath)) {
    $outPath = Join-Path (Get-Location) $outPath
}
$parent = [System.IO.Path]::GetDirectoryName($outPath)
if ($parent) {
    [System.IO.Directory]::CreateDirectory($parent) | Out-Null
}
[System.IO.File]::WriteAllBytes($outPath, $ms.ToArray())
Write-Output ("captured {0} bytes -> {1}" -f $ms.Length, $outPath)
