param(
    [Parameter(Mandatory = $true)]
    [string]$Stage,

    [Parameter(Mandatory = $true)]
    [string]$Commit,

    [Parameter()]
    [string]$ConfigLabel = "current",

    [Parameter(Mandatory = $true)]
    [string]$InputRoot,

    [Parameter(Mandatory = $true)]
    [string]$OutputFile,

    [string]$DirectoryPrefix = "",

    [string]$StripNamePrefix = "",

    [string[]]$IncludeNames = @()
)

$ErrorActionPreference = "Stop"

function Read-Summary([string]$Path) {
    $values = @{}
    foreach ($line in Get-Content -LiteralPath $Path) {
        $parts = $line -split "=", 2
        if ($parts.Count -eq 2) {
            $values[$parts[0]] = $parts[1]
        }
    }
    return $values
}

function Value-OrBlank($Values, [string]$Key) {
    if ($Values.ContainsKey($Key)) { return $Values[$Key] }
    return ""
}

$summaries = Get-ChildItem -LiteralPath $InputRoot -Filter summary.txt -Recurse
if (-not [string]::IsNullOrEmpty($DirectoryPrefix)) {
    $summaries = $summaries | Where-Object { $_.Directory.Name.StartsWith($DirectoryPrefix) }
}
if ($IncludeNames.Count -gt 0) {
    $summaries = $summaries | Where-Object { $IncludeNames -contains $_.Directory.Name }
}

$rows = foreach ($summary in $summaries) {
    $values = Read-Summary $summary.FullName
    $retiredInstructions = 0L
    foreach ($opcode in 0..15) {
        $key = "opcode_{0:x}_retired" -f $opcode
        if ($values.ContainsKey($key)) {
            $retiredInstructions += [long]$values[$key]
        }
    }
    $cycles = [double]$values.cycles
    $cyclesPerInstruction = if ($retiredInstructions -eq 0) { 0 } else { $cycles / $retiredInstructions }

    [ordered]@{
        stage = $Stage
        commit = $Commit
        config = $ConfigLabel
        name = if (-not [string]::IsNullOrEmpty($StripNamePrefix) -and
            $summary.Directory.Name.StartsWith($StripNamePrefix)) {
            $summary.Directory.Name.Substring($StripNamePrefix.Length)
        } else {
            $summary.Directory.Name
        }
        program_words = Value-OrBlank $values "program_words"
        cycles = $values.cycles
        retired_instructions = $retiredInstructions
        retired_words = $values.retired_words
        cycles_per_instruction = $cyclesPerInstruction.ToString("F6", [Globalization.CultureInfo]::InvariantCulture)
        cycles_per_retired_word = $values.cycles_per_retired_word
        fetch_wait_cycles = $values.fetch_wait_cycles
        fetch_wait_percent = $values.fetch_wait_percent
        data_request_cycles = $values.data_request_cycles
        data_response_cycles = $values.data_response_cycles
        data_path_percent = $values.data_path_percent
        data_requests = $values.data_requests
        loads = $values.loads
        stores = $values.stores
        load_average_wait_cycles = $values.load_average_wait_cycles
        store_average_wait_cycles = $values.store_average_wait_cycles
        icache_line_requests = $values.icache_line_requests
        icache_demand_requests = Value-OrBlank $values "icache_demand_requests"
        icache_demand_refills = Value-OrBlank $values "icache_demand_refills"
        icache_demand_hit_percent = Value-OrBlank $values "icache_demand_hit_percent"
        dcache_word_requests = $values.dcache_word_requests
        dcache_line_requests = $values.dcache_line_requests
        dcache_refills = $values.dcache_refills
        dcache_load_refills = $values.dcache_load_refills
        dcache_store_refills = $values.dcache_store_refills
        dcache_writebacks = $values.dcache_writebacks
        dcache_access_hit_percent = $values.dcache_access_hit_percent
        dcache_load_hit_percent = $values.dcache_load_hit_percent
        dcache_store_hit_percent = $values.dcache_store_hit_percent
        flush_cycles = Value-OrBlank $values "flush_cycles"
        flush_writebacks = Value-OrBlank $values "flush_writebacks"
        redirect_count = $values.redirect_count
        redirect_wait_cycles = $values.redirect_wait_cycles
        refreshes = $values.refreshes
        prefetch_issued = $values.prefetch_issued
        fpu_percent = if ($retiredInstructions -gt 0) { ([double]$values.opcode_d_retired / $retiredInstructions * 100).ToString("F1", [Globalization.CultureInfo]::InvariantCulture) } else { "" }
        load_store_percent = if ($retiredInstructions -gt 0) { ((([double]$values.opcode_8_retired + [double]$values.opcode_9_retired) / $retiredInstructions) * 100).ToString("F1", [Globalization.CultureInfo]::InvariantCulture) } else { "" }
        prefetch_useful = $values.prefetch_useful
        prefetch_useless = $values.prefetch_useless
        prefetch_dropped = $values.prefetch_dropped
    }
}

$rows |
    Sort-Object name |
    ForEach-Object { [pscustomobject]$_ } |
    Export-Csv -LiteralPath $OutputFile -NoTypeInformation -Encoding UTF8
