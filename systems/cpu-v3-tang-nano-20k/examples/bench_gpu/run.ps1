param(
    [string]$OutputDirectory = 'target/gpu-bench-run',
    [string]$HardwareDirectory = 'target/gpu-bench-hardware',
    [switch]$BuildHardware,
    [switch]$Program,
    [ValidateSet('micro','full')][string]$Scene = 'micro',
    [string]$Port,
    [ValidateRange(120, 600)][int]$CaptureSeconds = 150,
    [string]$Ffmpeg = 'ffmpeg',
    [string]$Ffprobe = 'ffprobe'
)
$ErrorActionPreference = 'Stop'
# examples/bench_gpu is four levels below the repository root.
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$oldSelection = $env:CPU_V3_BOOT_EXAMPLE
$oldScene = $env:GPU_BENCH_SCENE
$jobs = @()
$runLabel = 'gpu-bench'
function Invoke-BenchCargo([string]$Label, [string[]]$Arguments) {
    & (Join-Path $repo 'scripts/run-cargo.ps1') -Subcommand run -Label "$runLabel-$Label" -CargoArgs $Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Label failed" }
}
Push-Location $repo
try {
    if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh output directory for each run' }
    if ($Program -and [string]::IsNullOrWhiteSpace($Port)) { throw 'Programming/capture requires an explicit UART port' }
    New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
    $out = (Resolve-Path -LiteralPath $OutputDirectory).Path
    $runLabel = 'gpu-bench-' + ((Split-Path $out -Leaf) -replace '[^A-Za-z0-9_-]', '-')
    $env:CPU_V3_BOOT_EXAMPLE = 'gpu_bench'
    if ($Scene -eq 'full') { $env:GPU_BENCH_SCENE = 'full' }
    else { Remove-Item Env:GPU_BENCH_SCENE -ErrorAction SilentlyContinue }
    & systems/cpu-v3-tang-nano-20k/tools/build-application.ps1 -Application 'bench-gpu' -OutputDirectory "$out/boot" -BuildArguments @('--scene', $Scene) -Label "$runLabel-application"
    if ($BuildHardware) {
        Invoke-BenchCargo 'gpu-bench-hardware-build' @('-p','cpu-v3-tang-nano-20k','--bin','cpu-v3-system','--',$HardwareDirectory,'--build')
    }
    if (-not $Program) { return }
    # The profiler owns this shared implementation. Never restore the old JSON
    # timing producer to make this consumer work on an older base.
    $captureScript = Join-Path $repo 'hardware/vendor/gowin/scripts/capture_bl616_uart.ps1'
    $captureParameters = (Get-Command $captureScript).Parameters
    if (-not $captureParameters.ContainsKey('WriteTiming') -or
        -not $captureParameters.ContainsKey('StopAfterIdleSeconds')) {
        throw 'Board capture requires the delivered profiler live-CSV capture interface; integrate it first'
    }
    & python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py assemble $HardwareDirectory "$out/boot" --out "$out/deployment"
    if ($LASTEXITCODE -ne 0) { throw 'Hardware/application deployment assembly failed' }
    $deployment = Get-Content -Raw "$out/deployment/deployment.json" | ConvertFrom-Json
    $hw = (Resolve-Path -LiteralPath $HardwareDirectory).Path
    $assets = Get-Content -LiteralPath "$out/boot/gpu-bench-assets/assets.json" -Raw | ConvertFrom-Json
    $sourceHashes = [ordered]@{}
    $sourceFiles = @(Get-ChildItem -LiteralPath $PSScriptRoot -Recurse -File |
        Where-Object { $_.Extension -in @('.py', '.v', '.rs', '.ps1', '.md') })
    $sourceFiles += Get-Item 'systems/cpu-v3-tang-nano-20k/applications/image/build.rs',
        'systems/cpu-v3-tang-nano-20k/applications/image/src/bin/cpu-v3-boot-assets.rs', $captureScript
    foreach ($sourceFile in ($sourceFiles | Sort-Object FullName)) {
        $relative = [IO.Path]::GetRelativePath($repo, $sourceFile.FullName).Replace('\', '/')
        $sourceHashes[$relative] = (Get-FileHash -LiteralPath $sourceFile.FullName).Hash.ToLowerInvariant()
    }
    $identity = [ordered]@{
        commit = (& git rev-parse HEAD).Trim()
        dirty = [bool](& git status --porcelain)
        configuration_sha256 = $deployment.files.'configuration.bin'.sha256
        bitstream_sha256 = (Get-FileHash "$hw/impl/pnr/cpu_v3_system.fs").Hash
        boot_sha256 = (Get-FileHash "$out/boot/cpu-v3-boot.bin").Hash
        full_flash_sha256 = $deployment.files.'complete-flash.bin'.sha256
        asset_token = $assets.token
        cases = $assets.cases.Count
        cpu_clock_hz = 54000000
        mc_clock_hz = 108000000
        manifest_sha256 = (Get-FileHash "$out/boot/gpu-bench-assets/assets.json").Hash.ToLowerInvariant()
        source_sha256 = $sourceHashes
        programmed = $false
        timing_interface = 'profiler live CSV: offset,count,utc'
    }
    $identity | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$out/identity.json" -Encoding utf8
    & python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py program-flash $HardwareDirectory "$out/deployment" --kind complete --receipt "$out/program-flash.json"
    if ($LASTEXITCODE -ne 0) { throw 'Flash programming failed' }
    $jobs += Start-Job -ScriptBlock {
        param($root,$com,$seconds,$destination)
        $ErrorActionPreference = 'Stop'
        Set-Location $root
        & "$root/hardware/vendor/gowin/scripts/capture_bl616_uart.ps1" -Port $com -Seconds $seconds -Out "$destination/uart.bin" -WriteTiming
    } -ArgumentList $repo,$Port,$CaptureSeconds,$out
    $jobs += Start-Job -ScriptBlock {
        param($exe,$seconds,$destination)
        $ErrorActionPreference = 'Stop'
        [DateTime]::UtcNow.ToString('o') | Set-Content "$destination/video-launch-utc.txt"
        & $exe -hide_banner -nostdin -loglevel warning -f dshow -video_size 1600x1200 -pixel_format yuyv422 -framerate 5 `
            -use_wallclock_as_timestamps 1 -i 'video=USB Video' -frames:v ($seconds*5) -copyts -vf 'setparams=range=full' `
            -c:v ffv1 -level 3 -pix_fmt yuv422p "$destination/capture.mkv" *> "$destination/video.log"
        if ($LASTEXITCODE -ne 0) { throw 'HDMI capture failed; see video.log' }
    } -ArgumentList $Ffmpeg,$CaptureSeconds,$out
    Start-Sleep -Seconds 3
    & python -B systems/cpu-v3-tang-nano-20k/tools/hardware.py program-sram $HardwareDirectory "$out/deployment" --receipt "$out/program-sram.json"
    if ($LASTEXITCODE -ne 0) { throw 'SRAM programming failed' }
    $identity.programmed = $true
    $identity | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$out/identity.json" -Encoding utf8
    $null = Wait-Job -Job $jobs -Timeout ($CaptureSeconds+30)
    if (@($jobs | Where-Object State -ne 'Completed').Count -ne 0) { throw 'Capture failed or exceeded its wall limit' }
    Receive-Job -Job $jobs
    $identity.capture_sha256 = (Get-FileHash "$out/uart.bin").Hash.ToLowerInvariant()
    $identity | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$out/identity.json" -Encoding utf8
    python "$PSScriptRoot/tools/report.py" --manifest "$out/boot/gpu-bench-assets/assets.json" --capture "$out/uart.bin" --out "$out/report"
    if ($LASTEXITCODE -ne 0) { throw 'Incomplete/failed board measurements' }
    python "$PSScriptRoot/tools/gallery.py" --board $out --manifest "$out/boot/gpu-bench-assets/assets.json" --ffmpeg $Ffmpeg --ffprobe $Ffprobe
    if ($LASTEXITCODE -ne 0) { throw 'Gallery association failed; retain the raw evidence' }
} finally {
    foreach ($job in $jobs) {
        if ($job.State -eq 'Running') { Stop-Job -Job $job }
        Remove-Job -Job $job
    }
    if ($null -eq $oldSelection) { Remove-Item Env:CPU_V3_BOOT_EXAMPLE -ErrorAction SilentlyContinue }
    else { $env:CPU_V3_BOOT_EXAMPLE = $oldSelection }
    if ($null -eq $oldScene) { Remove-Item Env:GPU_BENCH_SCENE -ErrorAction SilentlyContinue }
    else { $env:GPU_BENCH_SCENE = $oldScene }
    Pop-Location
}
