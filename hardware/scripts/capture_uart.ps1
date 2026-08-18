param(
    [Parameter(Mandatory = $true)][string]$Port,
    [int]$Baud = 115200,
    [int]$Seconds = 8,
    [Parameter(Mandatory = $true)][string]$Out
)
$ErrorActionPreference = "Stop"
$p = New-Object System.IO.Ports.SerialPort $Port, $Baud, ([System.IO.Ports.Parity]::None), 8, ([System.IO.Ports.StopBits]::One)
$p.ReadTimeout = 500
$p.Open()
$ms = New-Object System.IO.MemoryStream
$buf = New-Object byte[] 4096
$deadline = (Get-Date).AddSeconds($Seconds)
try {
    while ((Get-Date) -lt $deadline) {
        try {
            $n = $p.Read($buf, 0, $buf.Length)
            if ($n -gt 0) { $ms.Write($buf, 0, $n) }
        } catch [System.TimeoutException] { }
    }
} finally {
    $p.Close()
}
$outPath = $Out
if (-not [System.IO.Path]::IsPathRooted($outPath)) {
    $outPath = Join-Path (Get-Location) $outPath
}
[System.IO.File]::WriteAllBytes($outPath, $ms.ToArray())
Write-Output ("captured {0} bytes -> {1}" -f $ms.Length, $outPath)
