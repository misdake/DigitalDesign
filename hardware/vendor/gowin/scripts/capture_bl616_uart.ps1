param(
    [Parameter(Mandatory = $true)][string]$Port,
    [int]$Baud = 115200,
    [int]$Seconds = 8,
    [Parameter(Mandatory = $true)][string]$Out
)

$ErrorActionPreference = "Stop"
if ($Seconds -le 0) { throw "capture duration must be positive" }

$serial = New-Object System.IO.Ports.SerialPort $Port, $Baud, ([System.IO.Ports.Parity]::None), 8, ([System.IO.Ports.StopBits]::One)
$serial.ReadTimeout = 200
$serial.WriteTimeout = 1000
$serial.DtrEnable = $false
$serial.RtsEnable = $false
$memory = New-Object System.IO.MemoryStream
$buffer = New-Object byte[] 4096
$outPath = $Out
if (-not [System.IO.Path]::IsPathRooted($outPath)) {
    $outPath = Join-Path (Get-Location) $outPath
}
$parent = [System.IO.Path]::GetDirectoryName($outPath)
if ($parent) { [System.IO.Directory]::CreateDirectory($parent) | Out-Null }
$handshakePath = "$outPath.handshake.bin"

function Read-BoundedAscii {
    param([System.IO.Ports.SerialPort]$Serial, [int]$MaximumBytes)
    $available = $Serial.BytesToRead
    if ($available -le 0) { return "" }
    $count = [Math]::Min($available, $MaximumBytes)
    $bytes = New-Object byte[] $count
    $read = $Serial.Read($bytes, 0, $count)
    return [System.Text.Encoding]::ASCII.GetString($bytes, 0, $read)
}

try {
    $serial.Open()
    Write-Output "opened $Port; entering BL616 console"
    Start-Sleep -Milliseconds 500

    # The BL616 recognizes this terminal escape only with human-like spacing.
    # Keep this serial session open after `choose uart`: reopening the FTDI VCP
    # can lose the transparent FPGA-UART route on this board/firmware revision.
    foreach ($value in @(0x18, 0x03, 0x0d)) {
        [byte[]]$one = @($value)
        $serial.Write($one, 0, 1)
        Start-Sleep -Milliseconds 250
    }
    $handshake = New-Object System.IO.MemoryStream
    $promptSeen = $false
    $promptDeadline = (Get-Date).AddSeconds(2)
    try {
        do {
            $available = $serial.BytesToRead
            if ($available -gt 0) {
                $count = [Math]::Min($available, $buffer.Length)
                $read = $serial.Read($buffer, 0, $count)
                if ($read -gt 0) { $handshake.Write($buffer, 0, $read) }
                $promptText = [System.Text.Encoding]::ASCII.GetString($handshake.ToArray())
                if ($promptText -match "TangNano20K") {
                    $promptSeen = $true
                    break
                }
            }
            Start-Sleep -Milliseconds 20
        } while ((Get-Date) -lt $promptDeadline)
        [System.IO.File]::WriteAllBytes($handshakePath, $handshake.ToArray())
        $handshakeBytes = $handshake.Length
    } finally {
        $handshake.Dispose()
    }
    if (-not $promptSeen) {
        throw "BL616 console prompt was not observed in $handshakeBytes byte(s); handshake evidence: $handshakePath"
    }

    $serial.Write("choose uart`r")
    Start-Sleep -Milliseconds 300
    Write-Output "BL616 UART route selected; capturing for $Seconds second(s)"

    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $available = $serial.BytesToRead
        if ($available -le 0) {
            Start-Sleep -Milliseconds 10
            continue
        }
        $count = $serial.Read($buffer, 0, [Math]::Min($buffer.Length, $available))
        if ($count -gt 0) { $memory.Write($buffer, 0, $count) }
    }

    # Persist evidence before touching the routing mode or closing the FTDI
    # handle. Some BL616 firmware/driver combinations can block Close() while
    # the FPGA is continuously filling the receive queue.
    [System.IO.File]::WriteAllBytes($outPath, $memory.ToArray())
    Write-Output ("persisted {0} captured byte(s); returning BL616 to its console" -f $memory.Length)

    # Return to the quiet BL616 console before Close(). This does not reset or
    # re-enumerate USB and the next capture will explicitly choose UART again.
    foreach ($value in @(0x18, 0x03, 0x0d)) {
        [byte[]]$one = @($value)
        $serial.Write($one, 0, 1)
        Start-Sleep -Milliseconds 250
    }
    Start-Sleep -Milliseconds 300
    $null = Read-BoundedAscii $serial 65536
} finally {
    if ($serial.IsOpen) { $serial.Close() }
    $serial.Dispose()
    $memory.Dispose()
}

$length = (Get-Item -LiteralPath $outPath).Length
Write-Output ("captured {0} FPGA UART bytes in the confirmed BL616 session -> {1}" -f $length, $outPath)
