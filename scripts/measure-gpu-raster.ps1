param(
    [string]$OutputRoot = "target/gpu-raster-candidates/fit",
    [string]$GowinHome = $env:GOWIN_HOME,
    [ValidateRange(1, 4)][int]$Jobs = 2
)

# Experimental fits only: export once, clone immutable inputs, and change
# the raster parameters in each clone. Never program a board or reuse the
# production artifact manifest after changing its generated Verilog.
$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
$run = Join-Path (Join-Path $repo $OutputRoot) ([DateTime]::UtcNow.ToString("yyyyMMddTHHmmssfffZ"))
$base = Join-Path $run "base"
$tool = Join-Path $GowinHome "IDE/bin/gw_sh.exe"
if (!(Test-Path -LiteralPath $tool)) { throw "Gowin gw_sh not found: $tool" }
New-Item -ItemType Directory -Path $run -Force | Out-Null
& (Join-Path $PSScriptRoot "run-cargo.ps1") -Subcommand run -Label "gpu-raster-candidate-export" -CargoArgs @(
    "-p", "cpu-v3-tang-nano-20k", "--example", "cpu_v3_system", "--", $base
)
if ($LASTEXITCODE -ne 0) { throw "System export failed" }
$sourceRelative = "src/generated/systems/cpu_v3_tang_nano_20k/gpu/cpu_v3_gpu.v"
$variants = @(
    @{ name = "tile-1pixel"; pixels = 1; scanline = 0 },
    @{ name = "tile-2pixel"; pixels = 2; scanline = 0 },
    @{ name = "scanline-1pixel"; pixels = 1; scanline = 1 },
    @{ name = "scanline-2pixel"; pixels = 2; scanline = 1 }
)
$commit = (& git -C $repo rev-parse HEAD).Trim()
$dirty = [bool](& git -C $repo status --porcelain)
foreach ($variant in $variants) {
    $directory = Join-Path $run $variant.name
    New-Item -ItemType Directory -Path $directory | Out-Null
    Copy-Item -LiteralPath (Join-Path $base "src") -Destination (Join-Path $directory "src") -Recurse
    Copy-Item -LiteralPath (Join-Path $base "build.tcl") -Destination $directory
    $source = Join-Path $directory $sourceRelative
    $text = [IO.File]::ReadAllText($source)
    $anchor = "CpuV3GpuRasterPixel #(.PREFETCH_LIMIT(1)) raster"
    if (!$text.Contains($anchor)) { throw "Raster parameter anchor missing" }
    $replacement = "CpuV3GpuRasterPixel #(.PREFETCH_LIMIT(1), .PIXELS_PER_CYCLE($($variant.pixels)), .SCANLINE($($variant.scanline))) raster"
    [IO.File]::WriteAllText($source, $text.Replace($anchor, $replacement), [Text.UTF8Encoding]::new($false))
    $variant.directory = $directory
    $variant.log = Join-Path $directory "gowin.log"
    $sourceList = Get-ChildItem -LiteralPath (Join-Path $directory "src") -File -Recurse |
        Sort-Object FullName | ForEach-Object {
            $_.FullName.Substring($directory.Length + 1).Replace('\', '/') + " " +
                (Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        }
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $sourceHash = ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(
            $sourceList -join "`n")))).Replace("-", "").ToLowerInvariant()
    } finally { $sha.Dispose() }
    $variant.metadata = [ordered]@{
        schema = 1; experimental = $true; name = $variant.name
        commit = $commit; worktree_dirty = $dirty
        pixels_per_cycle = $variant.pixels; scanline = $variant.scanline
        gpu_source_sha256 = (Get-FileHash -LiteralPath $source).Hash.ToLowerInvariant()
        source_tree_sha256 = $sourceHash
        constraints_sha256 = (Get-FileHash -LiteralPath (Join-Path $directory "src/generated/board.sdc")).Hash.ToLowerInvariant()
        pins_sha256 = (Get-FileHash -LiteralPath (Join-Path $directory "src/generated/board.cst")).Hash.ToLowerInvariant()
    }
    $variant.metadata | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $directory "variant.json") -Encoding UTF8
}

$running = @()
$next = 0
while ($next -lt $variants.Count -or $running.Count -gt 0) {
    while ($next -lt $variants.Count -and $running.Count -lt $Jobs) {
        $variant = $variants[$next]
        $script = Join-Path $variant.directory "build.tcl"
        $process = Start-Process -FilePath $tool -ArgumentList @('"' + $script + '"') -WorkingDirectory $variant.directory `
            -RedirectStandardOutput $variant.log -RedirectStandardError ($variant.log + ".stderr") -WindowStyle Hidden -PassThru
        # Retain the handle before exit; Windows PowerShell can otherwise
        # report a null ExitCode for short-lived Start-Process children.
        $null = $process.Handle
        $running += @{ process = $process; variant = $variant }
        Write-Host "Started $($variant.name)"
        $next++
    }
    $pending = @()
    foreach ($job in $running) {
        if ($job.process.HasExited) {
            $job.process.WaitForExit()
            $job.variant.exit_code = $job.process.ExitCode
            Write-Host "Finished $($job.variant.name): exit $($job.variant.exit_code)"
        } else { $pending += $job }
    }
    $running = $pending
    if ($running.Count -gt 0) { Start-Sleep -Seconds 2 }
}

function Get-Amounts($node) {
    $values = @{ Lut = 0; Alu = 0; Ssram = 0; Register = 0; Bsram = 0; Dsp = 0 }
    foreach ($item in @($node) + @($node.SelectNodes(".//*"))) {
        foreach ($key in @($values.Keys)) { $values[$key] += [int]$item.GetAttribute($key) }
    }
    return $values
}

$rows = foreach ($variant in $variants) {
    if ($variant.exit_code -ne 0) { throw "Fit failed: $($variant.name); inspect $($variant.log)" }
    $report = Get-Content -LiteralPath (Join-Path $variant.directory "impl/pnr/cpu_v3_system.rpt.txt") -Raw
    $timing = Get-Content -LiteralPath (Join-Path $variant.directory "impl/pnr/cpu_v3_system.tr") -Raw
    [xml]$hierarchy = Get-Content -LiteralPath (Join-Path $variant.directory "impl/gwsynthesis/cpu_v3_system_syn_rsc.xml") -Raw
    $gpu = $hierarchy.SelectSingleNode(".//*[@name='u_gpu']")
    $raster = $gpu.SelectSingleNode(".//*[@name='raster']")
    $r = Get-Amounts $raster
    $g = Get-Amounts $gpu
    $logic = [int][regex]::Match($report, 'Logic\s+\|\s+(\d+)').Groups[1].Value
    $ff = [int][regex]::Match($report, '--Logic Register as FF\s+\|\s+(\d+)').Groups[1].Value
    $ram = [int][regex]::Match($report, '--SSRAM\(RAM16\)\s+\|\s+(\d+)').Groups[1].Value
    $fmax = [double][regex]::Match($timing, 'cpu_clk\s+54\.000\(MHz\)\s+(\d+\.\d+)').Groups[1].Value
    $slack = [double][regex]::Match($timing, '(?m)^\s+1\s+(-?\d+\.\d+)\s+').Groups[1].Value
    $rLogic = $r.Lut + $r.Alu + 6 * $r.Ssram
    $pathTable = [regex]::Match($timing, '(?m)^3\.1\.1 Setup Paths Table[\s\S]*?(?=^3\.1\.2 Hold Paths Table)').Value
    $paths = [regex]::Matches($pathTable, '(?m)^\s+(\d+)\s+(-?\d+\.\d+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+')
    if ($paths.Count -eq 0) { throw "Setup paths missing: $($variant.name)" }
    $worst = $paths[0]
    $cpu = $paths | Where-Object { $_.Groups[5].Value -eq 'cpu_clk:[R]' -and $_.Groups[6].Value -eq 'cpu_clk:[R]' } |
        Select-Object -First 1
    $owners = foreach ($node in $hierarchy.SelectNodes("./Module/SubModule | ./Module/SubModule[@name='u_logic']/SubModule")) {
        if ($node.GetAttribute('name') -eq 'u_logic') { continue }
        $a = Get-Amounts $node
        [pscustomobject]@{ module = $node.GetAttribute('name'); lut = $a.Lut; alu = $a.Alu
            ram16 = $a.Ssram; logic = $a.Lut + $a.Alu + 6 * $a.Ssram; ff = $a.Register }
    }
    $owners | Export-Csv -LiteralPath (Join-Path $variant.directory "module-resources.csv") -NoTypeInformation -Encoding UTF8
    [pscustomobject]@{
        variant = $variant.name; system_logic = $logic; system_ff = $ff; system_ram16 = $ram
        cpu_fmax_mhz = $fmax; worst_setup_slack_ns = $slack
        raster_lut = $r.Lut; raster_alu = $r.Alu; raster_ram16 = $r.Ssram
        raster_logic = $rLogic; raster_ff = $r.Register
        raster_bsram = $r.Bsram; raster_dsp = $r.Dsp
        gpu_control_logic = $g.Lut + $g.Alu + 6 * $g.Ssram - $rLogic
        worst_setup_from = $worst.Groups[3].Value; worst_setup_to = $worst.Groups[4].Value
        cpu_critical_from = if ($cpu) { $cpu.Groups[3].Value } else { '' }
        cpu_critical_to = if ($cpu) { $cpu.Groups[4].Value } else { '' }
        timing_pass = $slack -ge 0 -and ![regex]::IsMatch($timing, '(?m)^\s+\S+\s+(setup|hold)\s+-')
    }
}
$rows | Export-Csv -LiteralPath (Join-Path $run "fits.csv") -NoTypeInformation -Encoding UTF8
$rows | Format-Table -AutoSize
Write-Host "Candidate results: $run"
if ($rows | Where-Object { !$_.timing_pass }) { throw "One or more candidate fits fail timing" }
