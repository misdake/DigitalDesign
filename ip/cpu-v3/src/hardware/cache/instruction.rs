//! Read-only instruction boundary and the shared two-way refill engine.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3TwoWayCacheInput {
    pub reset: Wire,
    pub invalidate_all: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_address: Wires<32>,
    pub cpu_write_data: Wires<16>,
    pub cpu_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<64>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3TwoWayCacheOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_line: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<64>,
    pub memory_response_ready: Wire,
}

pub struct CpuV3TwoWayCacheWithImage<I>(PhantomData<I>);
pub type CpuV3TwoWayCache = CpuV3TwoWayCacheWithImage<ZeroBsramImage>;

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionCacheInput {
    pub reset: Wire,
    pub invalidate_all: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_address: Wires<32>,
    pub cpu_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<64>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionCacheOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_address: Wires<22>,
    pub memory_response_ready: Wire,
}

/// Production read-only I-cache boundary. The proven refill engine is
/// retained underneath, with its legacy store pins tied off so synthesis
/// removes the unreachable write-through path.
pub struct CpuV3InstructionCache;

impl HardwareIdentity for CpuV3InstructionCache {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3InstructionCache").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3InstructionCache {
    type Input = CpuV3InstructionCacheInput;
    type Output = CpuV3InstructionCacheOutput;
    // The wrapper owns only static wiring; its sole mutable cache is the same engine.
    type EmuState = (
        CpuV3TwoWayCacheState,
        CpuV3TwoWayCacheInput,
        CpuV3TwoWayCacheOutput,
    );
    const USES_MAIN_CLOCK: bool = true;
    fn create_emu(input: &Self::Input, output: &Self::Output) -> Self::EmuState {
        let child_input = CpuV3TwoWayCacheInput {
            reset: input.reset,
            invalidate_all: input.invalidate_all,
            cpu_request_valid: input.cpu_request_valid,
            cpu_write: digital_design_circuit::input_const(0),
            cpu_address: input.cpu_address,
            cpu_write_data: digital_design_circuit::input_w_const(0),
            cpu_response_ready: input.cpu_response_ready,
            memory_request_ready: input.memory_request_ready,
            memory_response_valid: input.memory_response_valid,
            memory_read_data: input.memory_read_data,
            memory_error: input.memory_error,
        };
        let child_output = CpuV3TwoWayCacheOutput {
            cpu_request_ready: output.cpu_request_ready,
            cpu_response_valid: output.cpu_response_valid,
            cpu_read_data: output.cpu_read_data,
            cpu_error: output.cpu_error,
            memory_request_valid: output.memory_request_valid,
            memory_address: output.memory_address,
            memory_response_ready: output.memory_response_ready,
            memory_write: digital_design_circuit::input(),
            memory_line: digital_design_circuit::input(),
            memory_write_data: digital_design_circuit::input_w(),
        };
        (
            CpuV3TwoWayCache::create_emu(&child_input, &child_output),
            child_input,
            child_output,
        )
    }
    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _: &Self::Input,
        _: &Self::Output,
    ) {
        CpuV3TwoWayCache::execute_emu(&mut state.0, circuit, &state.1, &state.2);
    }
    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _: &Self::Input,
        _: &Self::Output,
    ) {
        CpuV3TwoWayCache::clock_emu(&mut state.0, circuit, &state.1, &state.2);
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_instruction_cache.v").replace(
            "__CACHE__",
            &CpuV3TwoWayCache::verilog_identity().module_name(),
        ))
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<CpuV3TwoWayCache>("u_cache")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_instruction_cache_tb.v").to_string())
    }
}

impl<I: CpuV3CacheImage> HardwareIdentity for CpuV3TwoWayCacheWithImage<I> {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3TwoWayCache")
            .namespace(["components", "cpu", "cpu_v3"])
            .symbol(
                "IMAGE",
                format!(
                    "{}_v{:016x}",
                    CpuV3DualPortCacheData::<I>::verilog_identity().module_name(),
                    I::INITIAL_VALID
                ),
            )
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq, Debug)]
enum State {
    #[default]
    Idle,
    WordRequest,
    WordResponse,
    LineRequest,
    LineReceive,
}

#[derive(Clone, Copy, Default)]
struct Pending {
    write: bool,
    address: u32,
    write_data: u16,
}

#[derive(Clone)]
pub struct CpuV3TwoWayCacheState {
    data: Box<[u16; CPU_V3_CACHE_WORDS]>,
    tags: [[u16; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
    valid: [[bool; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
    victim: [usize; CPU_V3_CACHE_SETS],
    pending_way: usize,
    state: State,
    lookup_valid: bool,
    pending: Pending,
    refill_beat: u8,
    response_data: u16,
    response_error: bool,
    response_valid: bool,
    refill_discard: bool,
    // Mirror of the RTL RAM16 valid/victim arrays: `valid`/`victim` hold the
    // RAM contents and `invalidate_all` starts a 64-set sweep (both ways
    // cleared in parallel, one set per cycle) instead of an instant clear.
    // New requests are blocked through cpu_request_ready while it runs.
    sweep_active: bool,
    sweep_set: u8,
}

impl Default for CpuV3TwoWayCacheState {
    fn default() -> Self {
        Self {
            data: Box::new([0; CPU_V3_CACHE_WORDS]),
            tags: [[0; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
            valid: [[false; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
            victim: [0; CPU_V3_CACHE_SETS],
            pending_way: 0,
            state: State::Idle,
            lookup_valid: false,
            pending: Pending::default(),
            refill_beat: 0,
            response_data: 0,
            response_error: false,
            response_valid: false,
            refill_discard: false,
            sweep_active: false,
            sweep_set: 0,
        }
    }
}

impl CpuV3TwoWayCacheState {
    fn initialized<I: CpuV3CacheImage>() -> Self {
        let mut state = Self::default();
        for (target, source) in state.data[..CPU_V3_CACHE_WORDS_PER_WAY]
            .iter_mut()
            .zip(I::WORDS)
        {
            *target = source as u16;
        }
        for set in 0..CPU_V3_CACHE_SETS {
            state.valid[0][set] = I::INITIAL_VALID & (1u64 << set) != 0;
        }
        state
    }
}

impl<I: CpuV3CacheImage> Module for CpuV3TwoWayCacheWithImage<I> {
    type Input = CpuV3TwoWayCacheInput;
    type Output = CpuV3TwoWayCacheOutput;
    type EmuState = CpuV3TwoWayCacheState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3TwoWayCacheState::initialized::<I>()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let (set, tag, _) = decode(state.pending.address);
        let pending_address_valid = state.pending.address >> 22 == 0;
        let invalidating = input.invalidate_all || state.sweep_active;
        let way_0_hit = state.valid[0][set] && state.tags[0][set] == tag;
        let way_1_hit = state.valid[1][set] && state.tags[1][set] == tag;
        let pending_hit = way_0_hit || way_1_hit;
        let lookup_read_hit =
            state.lookup_valid && pending_address_valid && !state.pending.write && pending_hit;
        let response_space = !state.response_valid || input.cpu_response_ready;
        let cpu_request_ready = !invalidating
            && state.state == State::Idle
            && (!state.lookup_valid || lookup_read_hit && response_space);
        output.drive(
            circuit,
            &CpuV3TwoWayCacheOutputValue {
                cpu_request_ready,
                cpu_response_valid: state.response_valid,
                cpu_read_data: u64::from(state.response_data),
                cpu_error: state.response_valid && state.response_error,
                memory_request_valid: state.state == State::WordRequest
                    || state.state == State::LineRequest,
                memory_write: state.pending.write,
                memory_line: !state.pending.write,
                memory_address: u64::from(if state.pending.write {
                    state.pending.address & 0x003f_ffff
                } else {
                    line_base(state.pending.address)
                }),
                memory_write_data: u64::from(state.pending.write_data),
                memory_response_ready: matches!(
                    state.state,
                    State::WordResponse | State::LineReceive
                ),
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
            *state = CpuV3TwoWayCacheState::initialized::<I>();
            return;
        }

        // Combinational values evaluated against the current (pre-edge) state.
        let (set, tag, word) = decode(state.pending.address);
        let pending_address_valid = state.pending.address >> 22 == 0;
        let invalidating = input.invalidate_all || state.sweep_active;
        let way_0_hit = state.valid[0][set] && state.tags[0][set] == tag;
        let way_1_hit = state.valid[1][set] && state.tags[1][set] == tag;
        let pending_hit = way_0_hit || way_1_hit;
        let hit_way = if way_0_hit { 0 } else { 1 };
        let selected_victim = if !state.valid[0][set] {
            0
        } else if !state.valid[1][set] {
            1
        } else {
            state.victim[set]
        };
        let response_space = !state.response_valid || input.cpu_response_ready;
        let hit_write = state.state == State::Idle
            && state.lookup_valid
            && state.pending.write
            && pending_hit
            && response_space
            && !invalidating;
        let lookup_read_hit =
            state.lookup_valid && pending_address_valid && !state.pending.write && pending_hit;
        let cpu_request_ready = !invalidating
            && state.state == State::Idle
            && (!state.lookup_valid || lookup_read_hit && response_space);
        let accept_cpu_request = input.cpu_request_valid && cpu_request_ready;

        let mut next = state.clone();

        if state.response_valid && input.cpu_response_ready {
            next.response_valid = false;
        }

        // Sweep control: invalidate_all (re)starts the 64-set sweep; each
        // following edge clears both ways of one set. The RAM write port
        // gives the sweep priority over any line-request/commit write, which
        // is therefore dropped while a sweep runs.
        // The RAM clear enable sees the old sweep_active even when a new
        // invalidate restarts the cursor on this edge.
        if state.sweep_active {
            next.valid[0][state.sweep_set as usize] = false;
            next.valid[1][state.sweep_set as usize] = false;
        }
        if input.invalidate_all {
            next.sweep_active = true;
            next.sweep_set = 0;
        } else if state.sweep_active {
            next.sweep_set = state.sweep_set + 1;
            if state.sweep_set == 63 {
                next.sweep_active = false;
            }
        }

        match state.state {
            State::Idle => {
                if state.lookup_valid && response_space {
                    if !pending_address_valid {
                        next.response_data = 0;
                        next.response_error = true;
                        next.response_valid = true;
                        next.lookup_valid = false;
                    } else if state.pending.write {
                        next.lookup_valid = false;
                        next.state = State::WordRequest;
                    } else if pending_hit {
                        next.response_data = state.data[data_index(hit_way, set, word)];
                        next.response_error = false;
                        next.response_valid = true;
                        next.lookup_valid = false;
                    } else {
                        next.lookup_valid = false;
                        next.pending_way = selected_victim;
                        next.refill_beat = 0;
                        // An invalidate coincident with miss detection belongs
                        // to the old fetch epoch. Complete its protocol
                        // response, but never install the line.
                        next.refill_discard = input.invalidate_all;
                        next.state = State::LineRequest;
                    }
                }
            }
            State::WordRequest => {
                if input.memory_request_ready {
                    next.state = State::WordResponse;
                }
            }
            State::WordResponse => {
                if input.memory_response_valid {
                    next.response_data = 0;
                    next.response_error = input.memory_error;
                    next.response_valid = true;
                    next.state = State::Idle;
                }
            }
            State::LineRequest => {
                if input.memory_request_ready {
                    if !state.sweep_active {
                        next.valid[state.pending_way][set] = false;
                    }
                    next.refill_beat = 0;
                    next.state = State::LineReceive;
                }
            }
            State::LineReceive => {
                if input.memory_response_valid {
                    if input.memory_error {
                        next.response_data = 0;
                        next.response_error = true;
                        next.response_valid = true;
                        next.state = State::Idle;
                    } else {
                        let first_word = 4 * usize::from(state.refill_beat);
                        let install = !state.refill_discard && !invalidating;
                        if install {
                            for lane in 0..4 {
                                next.data[data_index(state.pending_way, set, first_word + lane)] =
                                    (input.memory_read_data >> (16 * lane)) as u16;
                            }
                        }
                        if first_word <= word && word < first_word + 4 {
                            next.response_data =
                                (input.memory_read_data >> (16 * (word - first_word))) as u16;
                        }
                        if state.refill_beat as usize + 1 == CPU_V3_CACHE_MEMORY_BEATS {
                            if install {
                                if state.pending_way == 1 {
                                    next.valid[1][set] = true;
                                } else {
                                    next.valid[0][set] = true;
                                }
                                next.tags[state.pending_way][set] = tag;
                                next.victim[set] = 1 - state.pending_way;
                            }
                            next.response_error = false;
                            next.response_valid = true;
                            next.state = State::Idle;
                        } else {
                            next.refill_beat += 1;
                        }
                    }
                }
            }
        }

        if hit_write {
            next.data[data_index(hit_way, set, word)] = state.pending.write_data;
        }

        if accept_cpu_request {
            next.pending = Pending {
                write: input.cpu_write,
                address: input.cpu_address as u32,
                write_data: input.cpu_write_data as u16,
            };
            // Accepting a successor must preserve the held response's error.
            next.refill_discard = false;
            next.lookup_valid = true;
        }

        if input.invalidate_all
            && (state.state == State::LineRequest || state.state == State::LineReceive)
        {
            next.refill_discard = true;
        }

        *state = next;
    }

    fn verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            include_str!("cpu_v3_two_way_cache.v")
                .replace(
                    "module CpuV3TwoWayCache (",
                    &format!("module {module_name} ("),
                )
                .replace(
                    "__INITIAL_VALID__",
                    &format!("64'h{:016x}", I::INITIAL_VALID),
                )
                .replace(
                    "__CACHE_DATA_BANKS__",
                    &CpuV3DualPortCacheData::<I>::verilog_identity().module_name(),
                )
                .replace(
                    "__CACHE_METADATA__",
                    &CpuV3CacheMetadataWithImage::<I>::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3DualPortCacheData<I>>("u_data_banks"),
            VerilogDependency::new::<CpuV3CacheMetadataWithImage<I>>("u_metadata"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_two_way_cache_tb.v").replace(
            "CpuV3TwoWayCache dut",
            &format!("{} dut", Self::verilog_identity().module_name()),
        ))
    }
}

#[cfg(test)]
#[path = "instruction_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "icache_held_error_tests.rs"]
mod held_error_tests;

#[cfg(test)]
#[path = "instruction_emu_tests.rs"]
mod instruction_emu_tests;
