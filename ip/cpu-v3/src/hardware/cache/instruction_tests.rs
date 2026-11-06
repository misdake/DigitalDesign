use super::*;
use digital_design_circuit::{build_circuit, Circuit};
use digital_design_hardware::{ResourceAmount, ResourceKind, VerilogProject};
use std::collections::{HashMap, VecDeque};

struct InterleavedImage;

impl BsramImage<16> for InterleavedImage {
    const WORDS: [u64; CPU_V3_CACHE_WORDS_PER_WAY] = {
        let mut words = [0; CPU_V3_CACHE_WORDS_PER_WAY];
        let mut index = 0;
        while index < CPU_V3_CACHE_WORDS_PER_WAY {
            words[index] = index as u64;
            index += 1;
        }
        words
    };
}

impl CpuV3CacheImage for InterleavedImage {
    const INITIAL_VALID: u64 = 1;
}

/// A line-serving memory model behind the cache port: a read request
/// returns four ordered 64-bit beats, a write
/// request returns one completion beat.
struct MemoryModel {
    words: HashMap<u32, u16>,
    beats: VecDeque<(u64, bool)>,
    requests: usize,
    /// Beat index within a line response that carries an error instead.
    error_on_beat: Option<usize>,
}

impl MemoryModel {
    fn new() -> Self {
        Self {
            words: HashMap::new(),
            beats: VecDeque::new(),
            requests: 0,
            error_on_beat: None,
        }
    }

    fn accept(&mut self, write: bool, address: u32, write_data: u16) {
        self.requests += 1;
        if write {
            self.words.insert(address, write_data);
            self.beats.push_back((0, false));
        } else {
            for beat in 0..CPU_V3_CACHE_MEMORY_BEATS as u32 {
                let word_0 = self.word(address + 4 * beat);
                let word_1 = self.word(address + 4 * beat + 1);
                let word_2 = self.word(address + 4 * beat + 2);
                let word_3 = self.word(address + 4 * beat + 3);
                let error = self.error_on_beat == Some(beat as usize);
                self.beats.push_back((
                    u64::from(word_0)
                        | u64::from(word_1) << 16
                        | u64::from(word_2) << 32
                        | u64::from(word_3) << 48,
                    error,
                ));
            }
        }
    }

    fn word(&self, address: u32) -> u16 {
        self.words.get(&address).copied().unwrap_or(0)
    }
}

fn drive(
    circuit: &mut Circuit,
    input: &CpuV3TwoWayCacheInput,
    cpu_request: Option<(bool, u32, u16)>,
    cpu_response_ready: bool,
    memory_response: Option<(u64, bool)>,
    invalidate_all: bool,
) {
    let (cpu_write, cpu_address, cpu_write_data) = cpu_request.unwrap_or_default();
    input.drive(
        circuit,
        &CpuV3TwoWayCacheInputValue {
            reset: false,
            invalidate_all,
            cpu_request_valid: cpu_request.is_some(),
            cpu_write,
            cpu_address: u64::from(cpu_address),
            cpu_write_data: u64::from(cpu_write_data),
            cpu_response_ready,
            memory_request_ready: true,
            memory_response_valid: memory_response.is_some(),
            memory_read_data: memory_response.unwrap_or_default().0,
            memory_error: memory_response.unwrap_or_default().1,
        },
    );
}

fn transact(
    circuit: &mut Circuit,
    input: &CpuV3TwoWayCacheInput,
    output: &CpuV3TwoWayCacheOutput,
    memory: &mut MemoryModel,
    write: bool,
    address: u32,
    write_data: u16,
) -> (u16, bool, usize) {
    let requests_before = memory.requests;
    let mut cpu_request = Some((write, address, write_data));
    for _ in 0..300 {
        drive(
            circuit,
            input,
            cpu_request,
            false,
            memory.beats.pop_front(),
            false,
        );
        circuit.execute_gates();
        let value = output.sample(circuit);
        if cpu_request.is_some() && value.cpu_request_ready {
            cpu_request = None;
        }
        if value.memory_request_valid {
            memory.accept(
                value.memory_write,
                value.memory_address as u32,
                value.memory_write_data as u16,
            );
        }
        if value.cpu_response_valid {
            let result = (
                value.cpu_read_data as u16,
                value.cpu_error,
                memory.requests - requests_before,
            );
            // An error-terminated line response drops the model's
            // unsent beats, exactly like the arbiter drops them.
            memory.beats.clear();
            drive(circuit, input, None, true, None, false);
            circuit.clock_tick();
            return result;
        }
        circuit.clock_tick();
    }
    panic!("cache transaction did not complete")
}

fn fixture() -> (Circuit, CpuV3TwoWayCacheInput, CpuV3TwoWayCacheOutput) {
    let (circuit, (input, output)) = build_circuit(|| {
        let input = CpuV3TwoWayCacheInput::allocate();
        let output = CpuV3TwoWayCache::emu(&input);
        (input, output)
    });
    (circuit, input, output)
}

#[test]
fn cache_image_is_split_by_parity_and_initializes_only_way_zero() {
    for address in 0..CPU_V3_CACHE_WORDS_PER_WAY / 2 {
        assert_eq!(
            parity_bank_image::<InterleavedImage, false>()[address],
            (2 * address) as u64
        );
        assert_eq!(
            parity_bank_image::<InterleavedImage, true>()[address],
            (2 * address + 1) as u64
        );
    }
    assert!(
        parity_bank_image::<InterleavedImage, false>()[CPU_V3_CACHE_WORDS_PER_WAY / 2..]
            .iter()
            .all(|word| *word == 0)
    );
    assert!(
        parity_bank_image::<InterleavedImage, true>()[CPU_V3_CACHE_WORDS_PER_WAY / 2..]
            .iter()
            .all(|word| *word == 0)
    );
}

#[test]
fn miss_refills_one_line_through_four_beats_and_hits_afterwards() {
    let (mut circuit, input, output) = fixture();
    let mut memory = MemoryModel::new();
    for address in 0x120..0x130 {
        memory.words.insert(address, (0x8000 | address) as u16);
    }
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
        (0x8123, false, 1)
    );
    // Even and odd words come from the low and high beat halves.
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x12e, 0),
        (0x812e, false, 0)
    );
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x12f, 0),
        (0x812f, false, 0)
    );
}

#[test]
fn write_through_conflict_and_full_invalidate_follow_physical_tags() {
    let (mut circuit, input, output) = fixture();
    let mut memory = MemoryModel::new();
    for address in 0x120..0x130 {
        memory.words.insert(address, (0x8000 | address) as u16);
    }
    transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0);
    assert_eq!(
        transact(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            true,
            0x123,
            0x4567,
        ),
        (0, false, 1)
    );
    assert_eq!(memory.words[&0x123], 0x4567);
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
        (0x4567, false, 0)
    );

    drive(&mut circuit, &input, None, false, None, true);
    circuit.clock_tick();
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
        (0x4567, false, 1)
    );

    for address in 0x520..0x530 {
        memory.words.insert(address, (0x2000 | address) as u16);
    }
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x523, 0),
        (0x2523, false, 1)
    );
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
        (0x4567, false, 0)
    );
    for address in 0x920..0x930 {
        memory.words.insert(address, (0x3000 | address) as u16);
    }
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x923, 0),
        (0x3923, false, 1)
    );
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x523, 0),
        (0x2523, false, 0)
    );
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
        (0x4567, false, 1)
    );
}

#[test]
fn refill_error_aborts_without_installing_a_partial_line() {
    let (mut circuit, input, output) = fixture();
    let mut memory = MemoryModel::new();
    for address in 0x120..0x130 {
        memory.words.insert(address, (0x8000 | address) as u16);
    }
    memory.error_on_beat = Some(3);
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
        (0, true, 1)
    );
    // The failed line never became valid: the next read misses again and
    // the memory observes one more line request.
    memory.error_on_beat = None;
    assert_eq!(
        transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
        (0x8123, false, 1)
    );
}

#[test]
fn address_beyond_fitted_physical_memory_faults_without_downstream_io() {
    let (mut circuit, input, output) = fixture();
    let mut memory = MemoryModel::new();
    assert_eq!(
        transact(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            false,
            0x0040_0000,
            0,
        ),
        (0, true, 0)
    );
}

#[test]
fn export_claims_two_data_bsrams_and_one_packed_metadata_bsram() {
    let project = VerilogProject::generate::<CpuV3TwoWayCache>().unwrap();
    let resources: Vec<_> = project
        .resource_claims
        .iter()
        .flat_map(|claim| claim.resources.iter().copied())
        .collect();
    assert!(resources.contains(&ResourceAmount::new(ResourceKind::Bsram18K, 2)));
    assert_eq!(
        resources
            .iter()
            .filter(|r| r.kind == ResourceKind::Bsram18K)
            .map(|r| r.amount)
            .sum::<u64>(),
        3
    );
    let ssram: u64 = resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::SsramBit)
        .map(|resource| resource.amount)
        .sum();
    assert_eq!(ssram, 0);
}

#[test]
#[ignore = "explicit external simulation of the CpuV3 two-way cache"]
fn verify_verilog_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3TwoWayCache>().unwrap();
}

#[test]
fn data_cache_exports_two_data_bsrams_and_one_metadata_bsram() {
    let project = VerilogProject::generate::<CpuV3DataCache>().unwrap();
    let resources: Vec<_> = project
        .resource_claims
        .iter()
        .flat_map(|claim| claim.resources.iter().copied())
        .collect();
    assert!(resources.contains(&ResourceAmount::new(ResourceKind::Bsram18K, 2)));
    assert_eq!(
        resources
            .iter()
            .filter(|r| r.kind == ResourceKind::Bsram18K)
            .map(|r| r.amount)
            .sum::<u64>(),
        3
    );
    let ssram: u64 = resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::SsramBit)
        .map(|resource| resource.amount)
        .sum();
    assert_eq!(ssram, 0);
}

#[test]
#[ignore = "explicit external simulation of the write-back data cache"]
fn verify_data_cache_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3DataCache>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of synchronous D-cache metadata"]
fn verify_data_cache_metadata_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3DataCacheMetadata>().unwrap();
}

#[test]
#[ignore = "official Gowin DPB model for synchronous D-cache metadata"]
fn verify_data_cache_metadata_with_vendor_dpb() {
    verify_metadata_with_vendor::<ZeroBsramImage>();
}

struct SparseMetadataImage;
impl BsramImage<16> for SparseMetadataImage {
    const WORDS: [u64; 1024] = [0; 1024];
}
impl CpuV3CacheImage for SparseMetadataImage {
    const INITIAL_VALID: u64 = 0x915a_c342_f0e1_268d;
}

#[test]
#[ignore = "sparse metadata initialization in portable and official DPB models"]
fn verify_preloaded_metadata_with_vendor_dpb() {
    digital_design_hardware::verify_verilog_with_iverilog::<
        CpuV3CacheMetadataWithImage<SparseMetadataImage>,
    >()
    .unwrap();
    verify_metadata_with_vendor::<SparseMetadataImage>();
}

fn verify_metadata_with_vendor<I: CpuV3CacheImage>() {
    let directory = std::env::temp_dir().join(format!(
        "cache-metadata-vendor-{}-{:016x}",
        std::process::id(),
        I::INITIAL_VALID
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("metadata.v"),
        CpuV3CacheMetadataWithImage::<I>::verilog_source().unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.join("tb.v"),
        CpuV3CacheMetadataWithImage::<I>::verilog_testbench().unwrap(),
    )
    .unwrap();
    let prim = std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap())
        .join("IDE/simlib/gw2a/prim_sim.v");
    let exe = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let compile = std::process::Command::new(exe)
        .current_dir(&directory)
        .args([
            "-g2012",
            "-D__ICARUS__",
            "-DCPU_V3_CACHE_METADATA_VENDOR",
            "-s",
            "tb",
            "-o",
            "sim.vvp",
            "metadata.v",
            "tb.v",
        ])
        .arg(prim)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "metadata vendor compile failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let exe = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let run = std::process::Command::new(exe)
        .current_dir(&directory)
        .arg("sim.vvp")
        .output()
        .unwrap();
    assert!(
        run.status.success()
            && String::from_utf8_lossy(&run.stdout).contains("DIGITAL_DESIGN_PASS"),
        "metadata vendor simulation failed: {}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "explicit external simulation of the read-only instruction cache"]
fn verify_instruction_cache_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3InstructionCache>().unwrap();
}

// ---- emulator vs RTL co-simulation ----

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CycleOut {
    cpu_request_ready: bool,
    cpu_response_valid: bool,
    cpu_read_data: u16,
    cpu_error: bool,
    memory_request_valid: bool,
    memory_write: bool,
    memory_address: u32,
    memory_response_ready: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CycleIn {
    invalidate_all: bool,
    cpu_request_valid: bool,
    cpu_write: bool,
    cpu_address: u32,
    cpu_write_data: u16,
    cpu_response_ready: bool,
    memory_response_valid: bool,
    memory_read_data: u64,
    memory_error: bool,
}

#[allow(clippy::too_many_arguments)]
fn cosim_step(
    circuit: &mut Circuit,
    input: &CpuV3TwoWayCacheInput,
    output: &CpuV3TwoWayCacheOutput,
    memory: &mut MemoryModel,
    cpu_request: Option<(bool, u32, u16)>,
    cpu_response_ready: bool,
    invalidate_all: bool,
    trace: &mut Vec<(CycleIn, CycleOut)>,
) -> CycleOut {
    assert!(trace.len() < 20_000, "cache co-simulation cycle limit");
    let memory_response = memory.beats.pop_front();
    let (cpu_write, cpu_address, cpu_write_data) = cpu_request.unwrap_or((false, 0, 0));
    input.drive(
        circuit,
        &CpuV3TwoWayCacheInputValue {
            reset: false,
            invalidate_all,
            cpu_request_valid: cpu_request.is_some(),
            cpu_write,
            cpu_address: u64::from(cpu_address),
            cpu_write_data: u64::from(cpu_write_data),
            cpu_response_ready,
            memory_request_ready: true,
            memory_response_valid: memory_response.is_some(),
            memory_read_data: memory_response.unwrap_or_default().0,
            memory_error: memory_response.unwrap_or_default().1,
        },
    );
    circuit.execute_gates();
    let value = output.sample(circuit);
    if value.memory_request_valid {
        memory.accept(
            value.memory_write,
            value.memory_address as u32,
            value.memory_write_data as u16,
        );
    }
    let out = CycleOut {
        cpu_request_ready: value.cpu_request_ready,
        cpu_response_valid: value.cpu_response_valid,
        cpu_read_data: value.cpu_read_data as u16,
        cpu_error: value.cpu_error,
        memory_request_valid: value.memory_request_valid,
        memory_write: value.memory_write,
        memory_address: value.memory_address as u32,
        memory_response_ready: value.memory_response_ready,
    };
    let cin = CycleIn {
        invalidate_all,
        cpu_request_valid: cpu_request.is_some(),
        cpu_write,
        cpu_address,
        cpu_write_data,
        cpu_response_ready,
        memory_response_valid: memory_response.is_some(),
        memory_read_data: memory_response.unwrap_or_default().0,
        memory_error: memory_response.unwrap_or_default().1,
    };
    trace.push((cin, out));
    circuit.clock_tick();
    out
}

fn cosim_read(
    circuit: &mut Circuit,
    input: &CpuV3TwoWayCacheInput,
    output: &CpuV3TwoWayCacheOutput,
    memory: &mut MemoryModel,
    address: u32,
    trace: &mut Vec<(CycleIn, CycleOut)>,
) -> u16 {
    let mut request = Some((false, address, 0));
    while request.is_some() {
        let out = cosim_step(circuit, input, output, memory, request, false, false, trace);
        if out.cpu_request_ready {
            request = None;
        }
    }
    loop {
        let out = cosim_step(circuit, input, output, memory, None, false, false, trace);
        if out.cpu_response_valid {
            let data = out.cpu_read_data;
            cosim_step(circuit, input, output, memory, None, true, false, trace);
            return data;
        }
    }
}

fn cosim_write(
    circuit: &mut Circuit,
    input: &CpuV3TwoWayCacheInput,
    output: &CpuV3TwoWayCacheOutput,
    memory: &mut MemoryModel,
    address: u32,
    write_data: u16,
    trace: &mut Vec<(CycleIn, CycleOut)>,
) {
    let mut request = Some((true, address, write_data));
    while request.is_some() {
        let out = cosim_step(circuit, input, output, memory, request, false, false, trace);
        if out.cpu_request_ready {
            request = None;
        }
    }
    loop {
        let out = cosim_step(circuit, input, output, memory, None, false, false, trace);
        if out.cpu_response_valid {
            cosim_step(circuit, input, output, memory, None, true, false, trace);
            return;
        }
    }
}

fn run_emu_trace() -> Vec<(CycleIn, CycleOut)> {
    let (mut circuit, input, output) = fixture();
    let mut memory = MemoryModel::new();
    for address in 0x120..0x130 {
        memory
            .words
            .insert(address, (0x8000 | (address & 0xff)) as u16);
    }
    let mut trace = Vec::new();

    // Cold miss refills line 0x120, then a hit.
    assert_eq!(
        cosim_read(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            0x123,
            &mut trace
        ),
        0x8023
    );
    assert_eq!(
        cosim_read(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            0x124,
            &mut trace
        ),
        0x8024
    );

    // Write-through, then a hit returns the written value.
    cosim_write(
        &mut circuit,
        &input,
        &output,
        &mut memory,
        0x123,
        0x4567,
        &mut trace,
    );
    assert_eq!(
        cosim_read(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            0x123,
            &mut trace
        ),
        0x4567
    );

    // Back-to-back reads exercise the pipelined hit path: the second
    // request is accepted in the same cycle the first lookup resolves.
    cosim_step(
        &mut circuit,
        &input,
        &output,
        &mut memory,
        Some((false, 0x123, 0)),
        false,
        false,
        &mut trace,
    );
    cosim_step(
        &mut circuit,
        &input,
        &output,
        &mut memory,
        Some((false, 0x124, 0)),
        false,
        false,
        &mut trace,
    );
    for _ in 0..8 {
        let out = cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            false,
            &mut trace,
        );
        if out.cpu_response_valid {
            cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                true,
                false,
                &mut trace,
            );
        }
    }

    // An invalidate racing an outstanding refill may return its old-epoch
    // response, but it must not install that line as a subsequent hit.
    for address in 0x2520..0x2530 {
        memory
            .words
            .insert(address, (0xa000 | (address & 0xff)) as u16);
    }
    let out = cosim_step(
        &mut circuit,
        &input,
        &output,
        &mut memory,
        Some((false, 0x2523, 0)),
        false,
        false,
        &mut trace,
    );
    assert!(out.cpu_request_ready);
    let mut refill_data = None;
    for cycle in 0..40 {
        // Pulse invalidate partway through the refill drain.
        let out = cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            cycle == 3,
            &mut trace,
        );
        if out.cpu_response_valid {
            refill_data = Some(out.cpu_read_data);
            cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                true,
                false,
                &mut trace,
            );
            break;
        }
    }
    assert_eq!(refill_data, Some(0xa023));
    let requests_before = memory.requests;
    assert_eq!(
        cosim_read(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            0x2524,
            &mut trace
        ),
        0xa024
    );
    assert_eq!(
        memory.requests,
        requests_before + 1,
        "invalidate during refill exposed a stale installed line"
    );

    // Invalidate clears the cache through the 64-set valid-array sweep; a
    // second pulse mid-sweep restarts it, and the next read still misses
    // and refetches the line.
    cosim_step(
        &mut circuit,
        &input,
        &output,
        &mut memory,
        None,
        false,
        true,
        &mut trace,
    );
    for _ in 0..3 {
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            false,
            &mut trace,
        );
    }
    cosim_step(
        &mut circuit,
        &input,
        &output,
        &mut memory,
        None,
        false,
        true,
        &mut trace,
    );
    cosim_step(
        &mut circuit,
        &input,
        &output,
        &mut memory,
        None,
        false,
        false,
        &mut trace,
    );
    assert_eq!(
        cosim_read(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            0x123,
            &mut trace
        ),
        0x4567
    );

    trace
}

fn generate_cosim_tb(trace: &[(CycleIn, CycleOut)], module_name: &str) -> String {
    let mut t = format!(
            "module tb;\n\
             reg clk = 0;\n\
             reg reset, invalidate_all;\n\
             reg cpu_request_valid, cpu_write, cpu_response_ready;\n\
             reg [31:0] cpu_address;\n\
             reg [15:0] cpu_write_data;\n\
             reg memory_request_ready, memory_response_valid, memory_error;\n\
             reg [63:0] memory_read_data;\n\
             wire cpu_request_ready, cpu_response_valid, cpu_error;\n\
             wire [15:0] cpu_read_data;\n\
             wire memory_request_valid, memory_write, memory_line, memory_response_ready;\n\
             wire [21:0] memory_address;\n\
             wire [63:0] memory_write_data;\n\n\
             {module_name} dut(.*);\n\n\
             always #5 clk = ~clk;\n\n\
             initial begin\n\
                 reset = 1; invalidate_all = 0; cpu_request_valid = 0; cpu_write = 0; cpu_address = 0;\n\
                 cpu_write_data = 0; cpu_response_ready = 0; memory_request_ready = 1;\n\
                 memory_response_valid = 0; memory_read_data = 0; memory_error = 0;\n\
                 repeat (2) @(posedge clk);\n\
                 reset = 0;\n\
                 @(posedge clk);\n\
                 @(negedge clk);\n",
        );
    for (i, (cin, _)) in trace.iter().enumerate() {
        t.push_str(&format!(
                "    // cycle {i}\n\
                 cpu_request_valid = 1'b{crv}; cpu_write = 1'b{cw}; cpu_address = 32'h{ca:08x}; cpu_write_data = 16'h{cwd:04x};\n\
                 cpu_response_ready = 1'b{crr}; invalidate_all = 1'b{inv};
\
                 memory_response_valid = 1'b{mrv}; memory_read_data = 64'h{mrd:016x}; memory_error = 1'b{me};\n\
                 #1;\n\
                 $display(\"OUT %0d %0d %0d %0d %0d %0d %0d %0d %0d\", {i}, cpu_request_ready, cpu_response_valid, cpu_read_data, cpu_error, memory_request_valid, memory_write, memory_address, memory_response_ready);\n\
                 @(posedge clk);\n\
                 @(negedge clk);\n",
                crv = u8::from(cin.cpu_request_valid),
                cw = u8::from(cin.cpu_write),
                ca = cin.cpu_address,
                cwd = cin.cpu_write_data,
                crr = u8::from(cin.cpu_response_ready),
                inv = u8::from(cin.invalidate_all),
                mrv = u8::from(cin.memory_response_valid),
                mrd = cin.memory_read_data,
                me = u8::from(cin.memory_error),
            ));
    }
    t.push_str(&format!(
            "    $display(\"TRACE_END\");\n    $finish;\nend\n\n\
             initial begin\n    repeat ({}) @(posedge clk);\n    $display(\"TIMEOUT\");\n    $finish(1);\nend\nendmodule\n",
            trace.len() * 5 + 500
        ));
    t
}

fn cache_cosim_sources() -> String {
    let mut s = String::new();
    s.push_str(&CpuV3DualPortCacheData::<ZeroBsramImage>::verilog_source().unwrap());
    s.push('\n');
    s.push_str(&CpuV3DataCacheMetadata::verilog_source().unwrap());
    s.push('\n');
    s.push_str(&CpuV3TwoWayCache::verilog_source().unwrap());
    s
}

fn run_iverilog_cosim(sources: &str, tb: &str) -> Vec<CycleOut> {
    let directory = std::env::temp_dir().join(format!(
        "cache-cosim-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("modules.v"), sources).unwrap();
    std::fs::write(directory.join("tb.v"), tb).unwrap();
    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let output_path = directory.join("sim.vvp");
    let compile = std::process::Command::new(&iverilog)
        .current_dir(&directory)
        .args(["-g2005", "-s", "tb", "-o"])
        .arg(&output_path)
        .arg(directory.join("modules.v"))
        .arg(directory.join("tb.v"))
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "iverilog compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let simulation = std::process::Command::new(&vvp)
        .current_dir(&directory)
        .arg(&output_path)
        .output()
        .unwrap();
    assert!(
        simulation.status.success(),
        "vvp failed:\n{}",
        String::from_utf8_lossy(&simulation.stderr)
    );
    let stdout = String::from_utf8_lossy(&simulation.stdout);
    let mut outputs = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("OUT ") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            assert_eq!(fields.len(), 9, "unexpected OUT line: {line}");
            outputs.push(CycleOut {
                cpu_request_ready: fields[1] == "1",
                cpu_response_valid: fields[2] == "1",
                cpu_read_data: fields[3].parse().unwrap(),
                cpu_error: fields[4] == "1",
                memory_request_valid: fields[5] == "1",
                memory_write: fields[6] == "1",
                memory_address: fields[7].parse().unwrap(),
                memory_response_ready: fields[8] == "1",
            });
        } else if line == "TRACE_END" {
            break;
        }
    }
    std::fs::remove_dir_all(&directory).ok();
    outputs
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of the two-way cache"]
fn emu_matches_rtl_verilog() {
    let trace = run_emu_trace();
    let module_name = CpuV3TwoWayCache::verilog_identity().module_name();
    let tb = generate_cosim_tb(&trace, &module_name);
    let rtl = run_iverilog_cosim(&cache_cosim_sources(), &tb);
    assert_eq!(rtl.len(), trace.len(), "cycle count mismatch");
    for (i, ((_, expected), actual)) in trace.iter().zip(&rtl).enumerate() {
        assert_eq!(*actual, *expected, "emu/RTL output mismatch at cycle {i}");
    }
}

#[test]
#[ignore = "held lookups across metadata sweep writes and completion"]
fn held_lookup_matches_rtl_across_invalidate_sweep() {
    for (hold, restart_at) in [
        (0, None),
        (1, None),
        (17, None),
        (18, None),
        (19, None),
        (63, None),
        (64, None),
        (66, None),
        (80, None),
        (20, Some(18)),
        (66, Some(63)),
        (80, Some(0)),
    ] {
        let (mut circuit, input, output) = fixture();
        let mut memory = MemoryModel::new();
        let mut trace = Vec::new();
        cosim_read(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            0x123,
            &mut trace,
        );
        // One completed response holds the slot while a second hit's
        // lookup remains outstanding. Sweep set 18 across that lookup.
        for address in [0x123, 0x124] {
            let out = cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                Some((false, address, 0)),
                false,
                false,
                &mut trace,
            );
            assert!(out.cpu_request_ready);
        }
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            true,
            &mut trace,
        );
        for cycle in 0..hold {
            cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                false,
                restart_at == Some(cycle),
                &mut trace,
            );
        }
        let mut responses = 0;
        for _ in 0..256 {
            let out = cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                true,
                false,
                &mut trace,
            );
            responses += usize::from(out.cpu_response_valid);
            if responses == 2 {
                break;
            }
        }
        assert_eq!(responses, 2, "bounded completion hold={hold}");
        let tb = generate_cosim_tb(&trace, &CpuV3TwoWayCache::verilog_identity().module_name());
        let rtl = run_iverilog_cosim(&cache_cosim_sources(), &tb);
        assert_eq!(rtl.len(), trace.len());
        for (cycle, ((_, expected), actual)) in trace.iter().zip(&rtl).enumerate() {
            assert_eq!(actual, expected, "hold={hold} cycle={cycle}");
        }
    }
}
