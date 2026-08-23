//! First G16 physical-address cache: direct-mapped and write-through.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::SsramBits, HardwareIdentity, Module, ModuleIo, TargetResourceRequest,
    VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::{Bsram1R1Rw1024, ZeroBsramImage};

pub const G16_CACHE_WORDS: usize = 1024;
pub const G16_CACHE_LINE_WORDS: usize = 16;
pub const G16_CACHE_SETS: usize = G16_CACHE_WORDS / G16_CACHE_LINE_WORDS;

type CacheData = Bsram1R1Rw1024<16, ZeroBsramImage>;

const G16_CACHE_TAG_BITS: usize = 12;
const G16_CACHE_TAG_RAM16S: usize = G16_CACHE_SETS.div_ceil(16) * G16_CACHE_TAG_BITS.div_ceil(4);
const G16_CACHE_TAG_PHYSICAL_BITS: usize = G16_CACHE_TAG_RAM16S * 64;

#[derive(Clone, ModuleIo)]
pub struct G16CacheTagRamInput {
    pub write_enable: Wire,
    pub address: Wires<6>,
    pub write_data: Wires<12>,
}

#[derive(Clone, ModuleIo)]
pub struct G16CacheTagRamOutput {
    pub read_data: Wires<12>,
}

/// Characterized synchronous-write, asynchronous-read tag SSRAM.
pub struct G16CacheTagRam;

impl HardwareIdentity for G16CacheTagRam {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("G16CacheTagRam").namespace(["components", "cpu", "g16"])
    }
}

impl Module for G16CacheTagRam {
    type Input = G16CacheTagRamInput;
    type Output = G16CacheTagRamOutput;
    type EmuState = [u16; G16_CACHE_SETS];

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SsramBits::new(
            G16_CACHE_TAG_PHYSICAL_BITS as u64,
        ))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        [0; G16_CACHE_SETS]
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        output.drive(
            circuit,
            &G16CacheTagRamOutputValue {
                read_data: u64::from(state[input.address as usize]),
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        if input.write_enable {
            state[input.address as usize] = input.write_data as u16;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("g16_cache_tag_ram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("g16_cache_tag_ram_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct G16DirectMappedCacheInput {
    pub reset: Wire,
    pub invalidate_all: Wire,
    pub snoop_write_valid: Wire,
    pub snoop_write_address: Wires<22>,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_address: Wires<32>,
    pub cpu_write_data: Wires<16>,
    pub cpu_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<16>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct G16DirectMappedCacheOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<16>,
    pub memory_response_ready: Wire,
}

pub struct G16DirectMappedCache;

impl HardwareIdentity for G16DirectMappedCache {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("G16DirectMappedCache").namespace(["components", "cpu", "g16"])
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Phase {
    #[default]
    Idle,
    Check,
    MemoryRequest,
    MemoryResponse,
    CpuResponse,
}

#[derive(Clone, Copy, Default)]
struct Pending {
    write: bool,
    address: u32,
    write_data: u16,
}

pub struct G16DirectMappedCacheState {
    data: Box<[u16; G16_CACHE_WORDS]>,
    tags: [u16; G16_CACHE_SETS],
    valid: [bool; G16_CACHE_SETS],
    phase: Phase,
    pending: Pending,
    fill_word: u8,
    response_data: u16,
    response_error: bool,
}

impl Default for G16DirectMappedCacheState {
    fn default() -> Self {
        Self {
            data: Box::new([0; G16_CACHE_WORDS]),
            tags: [0; G16_CACHE_SETS],
            valid: [false; G16_CACHE_SETS],
            phase: Phase::Idle,
            pending: Pending::default(),
            fill_word: 0,
            response_data: 0,
            response_error: false,
        }
    }
}

impl Module for G16DirectMappedCache {
    type Input = G16DirectMappedCacheInput;
    type Output = G16DirectMappedCacheOutput;
    type EmuState = G16DirectMappedCacheState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        G16DirectMappedCacheState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        let refilling = state.phase == Phase::MemoryRequest && !state.pending.write;
        output.drive(
            circuit,
            &G16DirectMappedCacheOutputValue {
                cpu_request_ready: state.phase == Phase::Idle,
                cpu_response_valid: state.phase == Phase::CpuResponse,
                cpu_read_data: u64::from(state.response_data),
                cpu_error: state.phase == Phase::CpuResponse && state.response_error,
                memory_request_valid: state.phase == Phase::MemoryRequest,
                memory_write: state.pending.write,
                memory_address: u64::from(if refilling {
                    line_base(state.pending.address) | u32::from(state.fill_word)
                } else {
                    state.pending.address & 0x003f_ffff
                }),
                memory_write_data: u64::from(state.pending.write_data),
                memory_response_ready: state.phase == Phase::MemoryResponse,
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        if input.reset {
            *state = G16DirectMappedCacheState::default();
            return;
        }

        match state.phase {
            Phase::Idle if input.cpu_request_valid => {
                state.pending = Pending {
                    write: input.cpu_write,
                    address: input.cpu_address as u32,
                    write_data: input.cpu_write_data as u16,
                };
                state.response_error = false;
                if input.cpu_address >> 22 != 0 {
                    state.response_data = 0;
                    state.response_error = true;
                    state.phase = Phase::CpuResponse;
                } else {
                    state.phase = Phase::Check;
                }
            }
            Phase::Check => {
                let (set, tag, word) = decode(state.pending.address);
                let hit = state.valid[set] && state.tags[set] == tag;
                if state.pending.write {
                    if hit {
                        state.data[set * G16_CACHE_LINE_WORDS + word] = state.pending.write_data;
                    }
                    state.phase = Phase::MemoryRequest;
                } else if hit {
                    state.response_data = state.data[set * G16_CACHE_LINE_WORDS + word];
                    state.phase = Phase::CpuResponse;
                } else {
                    state.fill_word = 0;
                    state.phase = Phase::MemoryRequest;
                }
            }
            Phase::MemoryRequest if input.memory_request_ready => {
                state.phase = Phase::MemoryResponse;
            }
            Phase::MemoryResponse if input.memory_response_valid => {
                if input.memory_error {
                    state.response_data = 0;
                    state.response_error = true;
                    state.phase = Phase::CpuResponse;
                } else if state.pending.write {
                    state.response_data = 0;
                    state.phase = Phase::CpuResponse;
                } else {
                    let (set, tag, requested_word) = decode(state.pending.address);
                    let fill_word = usize::from(state.fill_word);
                    state.data[set * G16_CACHE_LINE_WORDS + fill_word] =
                        input.memory_read_data as u16;
                    if fill_word == requested_word {
                        state.response_data = input.memory_read_data as u16;
                    }
                    if fill_word + 1 == G16_CACHE_LINE_WORDS {
                        state.tags[set] = tag;
                        state.valid[set] = true;
                        state.phase = Phase::CpuResponse;
                    } else {
                        state.fill_word += 1;
                        state.phase = Phase::MemoryRequest;
                    }
                }
            }
            Phase::CpuResponse if input.cpu_response_ready => state.phase = Phase::Idle,
            _ => {}
        }

        if input.invalidate_all {
            state.valid.fill(false);
        } else if input.snoop_write_valid {
            let address = input.snoop_write_address as u32;
            let (set, _, _) = decode(address);
            state.valid[set] = false;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("g16_direct_mapped_cache.v")
                .replace(
                    "__CACHE_DATA__",
                    &CacheData::verilog_identity().module_name(),
                )
                .replace(
                    "__CACHE_TAGS__",
                    &G16CacheTagRam::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CacheData>("u_data"),
            VerilogDependency::new::<G16CacheTagRam>("u_tags"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("g16_direct_mapped_cache_tb.v").to_string())
    }
}

const fn line_base(address: u32) -> u32 {
    address & !((G16_CACHE_LINE_WORDS as u32) - 1)
}

const fn decode(address: u32) -> (usize, u16, usize) {
    let word = (address as usize) & (G16_CACHE_LINE_WORDS - 1);
    let set = ((address as usize) >> 4) & (G16_CACHE_SETS - 1);
    let tag = ((address >> 10) & 0x0fff) as u16;
    (set, tag, word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_circuit::{build_circuit, Circuit};
    use digital_design_hardware::{ResourceAmount, ResourceKind, VerilogProject};
    use std::collections::HashMap;

    fn drive(
        circuit: &mut Circuit,
        input: &G16DirectMappedCacheInput,
        cpu_request: Option<(bool, u32, u16)>,
        cpu_response_ready: bool,
        memory_response: Option<(u16, bool)>,
        snoop_write: Option<u32>,
    ) {
        let (cpu_write, cpu_address, cpu_write_data) = cpu_request.unwrap_or_default();
        input.drive(
            circuit,
            &G16DirectMappedCacheInputValue {
                reset: false,
                invalidate_all: false,
                snoop_write_valid: snoop_write.is_some(),
                snoop_write_address: u64::from(snoop_write.unwrap_or(0)),
                cpu_request_valid: cpu_request.is_some(),
                cpu_write,
                cpu_address: u64::from(cpu_address),
                cpu_write_data: u64::from(cpu_write_data),
                cpu_response_ready,
                memory_request_ready: true,
                memory_response_valid: memory_response.is_some(),
                memory_read_data: u64::from(memory_response.unwrap_or_default().0),
                memory_error: memory_response.unwrap_or_default().1,
            },
        );
    }

    fn transact(
        circuit: &mut Circuit,
        input: &G16DirectMappedCacheInput,
        output: &G16DirectMappedCacheOutput,
        memory: &mut HashMap<u32, u16>,
        write: bool,
        address: u32,
        write_data: u16,
    ) -> (u16, bool, usize) {
        let mut cpu_request = Some((write, address, write_data));
        let mut memory_response = None;
        let mut memory_requests = 0;
        for _ in 0..300 {
            drive(
                circuit,
                input,
                cpu_request,
                false,
                memory_response.take(),
                None,
            );
            circuit.execute_gates();
            let value = output.sample(circuit);
            if cpu_request.is_some() && value.cpu_request_ready {
                cpu_request = None;
            }
            if value.memory_request_valid {
                memory_requests += 1;
                let memory_address = value.memory_address as u32;
                memory_response = Some(if value.memory_write {
                    memory.insert(memory_address, value.memory_write_data as u16);
                    (0, false)
                } else {
                    (memory.get(&memory_address).copied().unwrap_or(0), false)
                });
            }
            if value.cpu_response_valid {
                let result = (value.cpu_read_data as u16, value.cpu_error, memory_requests);
                drive(circuit, input, None, true, memory_response.take(), None);
                circuit.clock_tick();
                return result;
            }
            circuit.clock_tick();
        }
        panic!("cache transaction did not complete")
    }

    #[test]
    fn miss_hit_write_through_conflict_and_dma_snoop_follow_physical_tags() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = G16DirectMappedCacheInput::allocate();
            let output = G16DirectMappedCache::emu(&input);
            (input, output)
        });
        let mut memory = HashMap::new();
        for address in 0x120..0x130 {
            memory.insert(address, (0x8000 | address) as u16);
        }
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x8123, false, 16)
        );
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x12e, 0),
            (0x812e, false, 0)
        );
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
        assert_eq!(memory[&0x123], 0x4567);
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x4567, false, 0)
        );

        drive(&mut circuit, &input, None, false, None, Some(0x123));
        circuit.clock_tick();
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x4567, false, 16)
        );

        for address in 0x520..0x530 {
            memory.insert(address, (0x2000 | address) as u16);
        }
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x523, 0),
            (0x2523, false, 16)
        );
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x4567, false, 16)
        );
    }

    #[test]
    fn address_beyond_fitted_physical_memory_faults_without_downstream_io() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = G16DirectMappedCacheInput::allocate();
            let output = G16DirectMappedCache::emu(&input);
            (input, output)
        });
        let mut memory = HashMap::new();
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
    fn export_claims_data_bsram_and_characterized_tag_ssram_leaves() {
        let project = VerilogProject::generate::<G16DirectMappedCache>().unwrap();
        assert_eq!(project.resource_claims.len(), 2);
        assert_eq!(
            project.resource_claims[0].resources,
            [ResourceAmount::new(ResourceKind::Bsram18K, 1)]
        );
        assert_eq!(
            project.resource_claims[1].resources,
            [ResourceAmount::new(
                ResourceKind::SsramBit,
                G16_CACHE_TAG_PHYSICAL_BITS as u64,
            )]
        );
    }

    #[test]
    #[ignore = "explicit external simulation of the G16 direct-mapped cache"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<G16DirectMappedCache>().unwrap();
    }
}
