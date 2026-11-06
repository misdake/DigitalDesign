//! Write-allocate data cache, dirty eviction and maintenance owner.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3DataCacheInput {
    /// Consume one 64-bit write beat after address acceptance.
    pub memory_write_data_ready: Wire,
    pub reset: Wire,
    pub clean_all: Wire,
    pub invalidate_all: Wire,
    pub line_copy_start: Wire,
    pub line_copy_source: Wires<22>,
    pub line_clean_start: Wire,
    pub line_clean_address: Wires<22>,
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
pub struct CpuV3DataCacheOutput {
    pub line_copy_ready: Wire,
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
    pub maintenance_ready: Wire,
    pub maintenance_busy: Wire,
    pub maintenance_done: Wire,
    pub maintenance_error: Wire,
    pub valid_sweep: Wire,
}

pub struct CpuV3DataCache;

impl HardwareIdentity for CpuV3DataCache {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3DataCache").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum DataMemoryPhase {
    #[default]
    Idle,
    Lookup,
    WritebackPrime,
    WritebackCapture,
    Request,
    ReadReceive,
    WriteStream,
    WriteResponse,
    LineCommandPrime,
    CopyPrepare,
    CopyRefresh,
    CopyLookup,
    CleanLookup,
    ScanPrime,
    Scan,
    Fault,
}

/// Cloneable so a test harness can snapshot and restore the emulated cache
/// state (see the fetch-pause sync-point work in the system harness).
#[derive(Clone)]
pub struct CpuV3DataCacheState {
    cache: crate::DataCache,
    pending_cpu_request: Option<crate::CpuMemoryRequest>,
    // Mirrors the RTL pending_word for an active CPU read. The functional
    // cache independently checks the final data when the line completes.
    pending_read_word: Option<usize>,
    request: Option<crate::MainMemoryRequest>,
    phase: DataMemoryPhase,
    words: [u16; CPU_V3_CACHE_LINE_WORDS],
    beat: usize,
    response_data: u16,
    response_valid: bool,
    response_error: bool,
    maintenance_active: bool,
    line_copy_active: bool,
    line_copy_source: crate::PhysicalWordAddress,
    line_clean_active: bool,
    line_clean_address: crate::PhysicalWordAddress,
    maintenance_done: bool,
    maintenance_error: bool,
    // Full maintenance owns the cache. Its metadata read pipeline holds only
    // one addressed set and at most one next writeback candidate.
    full_maintenance: bool,
    may_have_dirty: bool,
    scan_set: u8,
    metadata_output: [Option<crate::PhysicalWordAddress>; 2],
    scan_active: bool,
    scan_valid: bool,
    scan_done: bool,
    scan_read_set: u8,
    scan_return_set: u8,
    found_index: Option<u8>,
    found_granted: bool,
    found_other: Option<crate::PhysicalWordAddress>,
    retire_index: Option<u8>,
    wb_index: u8,
    // Metadata valid/dirty scrub timing: reset, a full invalidate, or
    // a memory error clears one set of both ways per cycle; requests are
    // blocked meanwhile, and an invalidate's maintenance_done is delayed
    // until the sweep completes.
    maintenance_invalidate: bool,
    sweep_active: bool,
    sweep_set: u8,
    sweep_finishes_maintenance: bool,
}

impl Default for CpuV3DataCacheState {
    fn default() -> Self {
        Self {
            cache: crate::DataCache::default(),
            pending_cpu_request: None,
            pending_read_word: None,
            request: None,
            phase: DataMemoryPhase::Idle,
            words: [0; CPU_V3_CACHE_LINE_WORDS],
            beat: 0,
            response_data: 0,
            response_valid: false,
            response_error: false,
            maintenance_active: false,
            line_copy_active: false,
            line_copy_source: crate::PhysicalWordAddress::new(0),
            line_clean_active: false,
            line_clean_address: crate::PhysicalWordAddress::new(0),
            maintenance_done: false,
            maintenance_error: false,
            full_maintenance: false,
            may_have_dirty: false,
            scan_set: 0,
            metadata_output: [None; 2],
            scan_active: false,
            scan_valid: false,
            scan_done: false,
            scan_read_set: 0,
            scan_return_set: 0,
            found_index: None,
            found_granted: false,
            found_other: None,
            retire_index: None,
            wb_index: 0,
            maintenance_invalidate: false,
            sweep_active: false,
            sweep_set: 0,
            sweep_finishes_maintenance: false,
        }
    }
}

impl CpuV3DataCacheState {
    /// Resident cache data only; this is not the physical GPU memory view.
    pub fn cached_word(&self, address: crate::PhysicalWordAddress) -> Option<u16> {
        (address.get() >> 22 == 0)
            .then(|| self.cache.cached_word(address))
            .flatten()
    }
    fn next_request_valid(&self) -> bool {
        self.full_maintenance
            && matches!(
                self.phase,
                DataMemoryPhase::WriteStream | DataMemoryPhase::WriteResponse
            )
            && self.found_index.is_some()
            && !self.found_granted
            && self.retire_index.is_none()
    }

    fn write_response_ready(&self) -> bool {
        matches!(
            self.phase,
            DataMemoryPhase::ReadReceive | DataMemoryPhase::WriteResponse
        ) || (self.full_maintenance
            && self.phase != DataMemoryPhase::Fault
            && (self.retire_index.is_some() || self.phase == DataMemoryPhase::WriteStream))
    }

    fn promote_maintenance(&mut self, index: u8, granted: bool) {
        let other = self.found_other;
        let request = self.next_maintenance_request(index);
        self.request = Some(request);
        self.beat = 0;
        self.phase = if granted {
            DataMemoryPhase::WriteStream
        } else {
            DataMemoryPhase::Request
        };
        self.scan_set = index % 64;
        self.found_index = None;
        self.found_granted = false;
        self.scan_valid = false;
        self.scan_done = false;
        if index < 64 && other.is_some() {
            self.found_index = Some(index + 64);
            self.found_other = None;
            self.scan_active = false;
        } else if index % 64 == 63 {
            self.scan_active = false;
            self.scan_done = true;
        } else {
            self.scan_active = true;
            self.scan_read_set = index % 64 + 1;
            self.scan_return_set = self.scan_read_set;
        }
    }
    fn start_request(&mut self, request: crate::MainMemoryRequest) {
        let write_line = matches!(request, crate::MainMemoryRequest::WriteLine { .. });
        self.request = Some(request);
        self.phase = if write_line {
            DataMemoryPhase::WritebackPrime
        } else {
            DataMemoryPhase::Request
        };
        self.beat = 0;
    }

    fn apply_action(&mut self, action: crate::CacheAction) {
        match action {
            crate::CacheAction::CpuResponse(response) => {
                if self.line_clean_active {
                    self.line_clean_active = false;
                    self.maintenance_active = false;
                    self.maintenance_done = true;
                    self.request = None;
                    self.phase = DataMemoryPhase::Idle;
                    return;
                }
                self.response_data = match response {
                    crate::CpuMemoryResponse::Read { value }
                        if self.phase == DataMemoryPhase::ReadReceive =>
                    {
                        assert_eq!(self.response_data, value, "refill final-slot capture");
                        self.response_data
                    }
                    crate::CpuMemoryResponse::Read { value } => value,
                    crate::CpuMemoryResponse::WriteComplete => 0,
                };
                self.pending_read_word = None;
                self.response_error = false;
                self.response_valid = true;
                self.request = None;
                self.phase = DataMemoryPhase::Idle;
            }
            crate::CacheAction::MainMemoryRequest(request) => self.start_request(request),
        }
    }

    fn metadata_at(&self, set: u8) -> [Option<crate::PhysicalWordAddress>; 2] {
        std::array::from_fn(|way| self.cache.dirty_line_address(way, usize::from(set)))
    }

    fn metadata_read_set(&self) -> u8 {
        let scanning = self.scan_active
            && matches!(
                self.phase,
                DataMemoryPhase::Request
                    | DataMemoryPhase::WriteStream
                    | DataMemoryPhase::WriteResponse
            );
        if scanning {
            self.scan_read_set
        } else {
            match self.phase {
                DataMemoryPhase::ScanPrime => self.scan_set,
                DataMemoryPhase::Scan => self.scan_set.saturating_add(1).min(63),
                _ => self.wb_index % 64,
            }
        }
    }

    fn metadata_candidate(&self, set: u8) -> Option<u8> {
        self.metadata_output
            .iter()
            .position(Option::is_some)
            .map(|way| way as u8 * 64 + set)
    }

    /// The independent functional cache supplies data and dirty semantics.
    /// The cycle wrapper selects one addressed entry and starts its existing
    /// single-line operation; it never snapshots a full dirty bitmap.
    fn next_maintenance_request(&mut self, index: u8) -> crate::MainMemoryRequest {
        self.wb_index = index;
        self.cache.maintenance_line(index)
    }

    fn finish_maintenance(&mut self) {
        let command = if self.maintenance_invalidate {
            crate::MaintenanceCommand::Invalidate
        } else {
            crate::MaintenanceCommand::Clean
        };
        assert!(
            self.cache
                .begin_maintenance(command)
                .expect("completed single-line operation must release the functional cache")
                .is_none(),
            "a completed scan must not leave dirty data behind"
        );
        self.request = None;
        self.phase = DataMemoryPhase::Idle;
        self.full_maintenance = false;
        self.may_have_dirty = false;
        if self.maintenance_invalidate {
            self.sweep_active = true;
            self.sweep_set = 0;
            self.sweep_finishes_maintenance = true;
        } else {
            self.maintenance_active = false;
            self.maintenance_done = true;
        }
    }

    /// Capture only a single future candidate while the source data DPBs are
    /// independently streaming. A pending synchronous lookup is retested if
    /// completion interrupts it; no metadata read is consumed on the clear edge.
    fn background_scan_step(&mut self, metadata_write: bool) {
        if self.scan_active && metadata_write {
            if self.scan_valid {
                self.scan_read_set = self.scan_return_set;
            }
            self.scan_valid = false;
            return;
        }
        if !self.scan_active
            || !matches!(
                self.phase,
                DataMemoryPhase::Request
                    | DataMemoryPhase::WriteStream
                    | DataMemoryPhase::WriteResponse
            )
        {
            return;
        }
        let tested_set = self.scan_return_set;
        let old_valid = self.scan_valid;
        self.scan_return_set = self.scan_read_set;
        self.scan_read_set = self.scan_read_set.saturating_add(1).min(63);
        self.scan_valid = true;
        if old_valid {
            if let Some(index) = self.metadata_candidate(tested_set) {
                self.found_index = Some(index);
                self.found_other = self.metadata_output[1];
                self.scan_active = false;
                self.scan_valid = false;
            } else if tested_set == 63 {
                self.scan_active = false;
                self.scan_valid = false;
                self.scan_done = true;
            }
        }
    }

    fn fail_transaction(&mut self) {
        // After a physical-memory error no cache line is allowed to remain
        // architecturally visible: the controller may have accepted an
        // unknown prefix of a burst. The crate model clears instantly; the
        // RTL scrubs its metadata records, which blocks new requests.
        self.cache = crate::DataCache::default();
        self.pending_cpu_request = None;
        self.pending_read_word = None;
        self.request = None;
        self.phase = DataMemoryPhase::Idle;
        self.full_maintenance = false;
        self.may_have_dirty = false;
        self.scan_active = false;
        self.found_index = None;
        self.sweep_active = true;
        self.sweep_set = 0;
        if self.maintenance_active {
            self.maintenance_active = false;
            self.line_copy_active = false;
            self.line_clean_active = false;
            self.maintenance_done = true;
            self.maintenance_error = true;
        } else {
            self.response_data = 0;
            self.response_error = true;
            self.response_valid = true;
        }
    }

    fn complete_write(&mut self) {
        if self.line_copy_active {
            let action = self
                .cache
                .complete(crate::MainMemoryResponse::WriteComplete)
                .expect("line-copy completion must match a line transaction");
            if matches!(action, crate::CacheAction::CpuResponse(_)) {
                self.request = None;
                self.phase = DataMemoryPhase::Idle;
                self.line_copy_active = false;
                self.maintenance_active = false;
                self.maintenance_done = true;
            } else {
                self.apply_action(action);
            }
        } else if self.line_clean_active {
            let action = self
                .cache
                .complete(crate::MainMemoryResponse::WriteComplete)
                .expect("clean-line completion must match a line write");
            self.apply_action(action);
        } else if self.full_maintenance {
            self.cache.retire_maintenance_line(self.wb_index);
            if let Some(index) = self.found_index.take() {
                let request = self.next_maintenance_request(index);
                self.scan_set = index % 64;
                self.start_request(request);
            } else if self.scan_done {
                self.finish_maintenance();
            } else {
                self.scan_set = self.scan_return_set;
                self.scan_active = false;
                self.scan_valid = false;
                self.request = None;
                self.phase = DataMemoryPhase::ScanPrime;
            }
        } else {
            let action = self
                .cache
                .complete(crate::MainMemoryResponse::WriteComplete)
                .expect("data-cache completion must match a line write");
            self.apply_action(action);
        }
    }
}

impl Module for CpuV3DataCache {
    type Input = CpuV3DataCacheInput;
    type Output = CpuV3DataCacheOutput;
    type EmuState = CpuV3DataCacheState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3DataCacheState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let (write, mut address, write_data) = match state.request {
            Some(crate::MainMemoryRequest::ReadLine { line_address }) => {
                (false, line_address.get(), 0)
            }
            Some(crate::MainMemoryRequest::WriteLine {
                line_address,
                words,
            }) => {
                let index = state.beat.min(CPU_V3_CACHE_MEMORY_BEATS - 1) * 4;
                (
                    true,
                    line_address.get(),
                    u64::from(words[index])
                        | (u64::from(words[index + 1]) << 16)
                        | (u64::from(words[index + 2]) << 32)
                        | (u64::from(words[index + 3]) << 48),
                )
            }
            Some(crate::MainMemoryRequest::WriteWord { .. }) => {
                unreachable!("the write-back cache never emits word writes")
            }
            None => (false, 0, 0),
        };
        if state.next_request_valid() {
            let index = state.found_index.unwrap();
            address = state
                .cache
                .dirty_line_address(usize::from(index / 64), usize::from(index % 64))
                .unwrap()
                .get();
        }
        output.drive(
            circuit,
            &CpuV3DataCacheOutputValue {
                line_copy_ready: state.phase == DataMemoryPhase::Idle
                    && !state.response_valid
                    && !state.maintenance_active
                    && !input.clean_all
                    && !input.invalidate_all
                    && !state.sweep_active,
                cpu_request_ready: state.phase == DataMemoryPhase::Idle
                    && !state.response_valid
                    && !state.maintenance_active
                    && !input.clean_all
                    && !input.invalidate_all
                    && !state.sweep_active,
                cpu_response_valid: state.response_valid,
                cpu_read_data: u64::from(state.response_data),
                cpu_error: state.response_valid && state.response_error,
                memory_request_valid: state.phase == DataMemoryPhase::Request
                    || state.next_request_valid(),
                memory_write: write,
                memory_line: state.request.is_some(),
                memory_address: u64::from(address),
                memory_write_data: write_data,
                memory_response_ready: state.write_response_ready(),
                maintenance_ready: state.phase == DataMemoryPhase::Idle
                    && !state.response_valid
                    && !state.maintenance_active
                    && !state.sweep_active,
                maintenance_busy: state.maintenance_active,
                maintenance_done: state.maintenance_done,
                maintenance_error: state.maintenance_error,
                valid_sweep: state.sweep_active,
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
            *state = CpuV3DataCacheState::default();
            // The RTL metadata records sweep-clear after reset instead of
            // clearing in one cycle; the system holds the core for the sweep.
            state.sweep_active = true;
            state.sweep_set = 0;
            return;
        }
        // Acceptance is decided from pre-edge ownership and sweep state.
        // Clearing response-valid or finishing scrub on this edge must not
        // consume a held CPU request until ready was actually asserted.
        let maintenance_ready = state.phase == DataMemoryPhase::Idle
            && !state.response_valid
            && !state.maintenance_active
            && !state.sweep_active;
        let command_ready = maintenance_ready && !input.clean_all && !input.invalidate_all;
        let accept_cpu = command_ready
            && !input.line_copy_start
            && !input.line_clean_start
            && input.cpu_request_valid;
        let metadata_next = state.metadata_at(state.metadata_read_set());
        let metadata_old = state.metadata_output;
        let old_phase = state.phase;
        let old_found = state.found_index;
        let old_retire = state.retire_index;
        let terminal =
            state.full_maintenance && input.memory_response_valid && state.write_response_ready();
        let current_terminal = terminal && old_retire.is_none();
        let next_accept = state.next_request_valid() && input.memory_request_ready;
        let promoted = state.full_maintenance
            && old_found.is_some()
            && old_retire.is_none()
            && !input.memory_error
            && (old_phase == DataMemoryPhase::WriteResponse
                || (old_phase == DataMemoryPhase::WriteStream
                    && state.beat == 3
                    && input.memory_write_data_ready));
        let next_granted = state.found_granted || next_accept;
        let old_wb_index = state.wb_index;
        let metadata_write = terminal && !input.memory_error;
        state.background_scan_step(metadata_write);
        // A write holds that way's DPB output; the scan tag is replayed above.
        state.metadata_output = std::array::from_fn(|way| {
            if metadata_write && usize::from(old_retire.unwrap_or(old_wb_index) / 64) == way {
                metadata_old[way]
            } else {
                metadata_next[way]
            }
        });
        state.maintenance_done = false;
        if next_accept {
            state.found_granted = true;
        }
        if terminal {
            if let Some(index) = old_retire {
                if !input.memory_error {
                    state.cache.retire_maintenance_line(index);
                }
                state.retire_index = None;
            }
        }
        if state.full_maintenance && input.memory_response_valid && input.memory_error {
            state.phase = DataMemoryPhase::Fault;
            state.maintenance_error = true;
            state.maintenance_done = old_phase != DataMemoryPhase::Fault;
            state.scan_active = false;
            state.found_index = None;
            return;
        }
        if state.response_valid && input.cpu_response_ready {
            state.response_valid = false;
        }
        // Sweep control: one set of both valid ways cleared per cycle; an
        // invalidate that finished its write-backs reports done when the
        // sweep completes.
        if state.sweep_active {
            if state.sweep_set == 63 {
                state.sweep_active = false;
                state.sweep_set = 0;
                if state.sweep_finishes_maintenance {
                    state.sweep_finishes_maintenance = false;
                    state.maintenance_active = false;
                    state.maintenance_done = true;
                }
            } else {
                state.sweep_set += 1;
            }
        }

        if maintenance_ready && (input.clean_all || input.invalidate_all) {
            state.maintenance_active = true;
            state.maintenance_error = false;
            state.maintenance_invalidate = input.invalidate_all;
            state.full_maintenance = true;
            state.found_index = None;
            state.scan_set = 0;
            if state.may_have_dirty {
                state.phase = DataMemoryPhase::ScanPrime;
            } else {
                state.finish_maintenance();
            }
            return;
        }

        if command_ready && input.line_copy_start {
            state.maintenance_error = false;
            if input.line_copy_source & 15 != 0 || input.line_clean_address & 15 != 0 {
                state.maintenance_done = true;
                state.maintenance_error = true;
            } else {
                state.maintenance_active = true;
                state.line_copy_active = true;
                state.line_clean_active = false;
                state.line_clean_address =
                    crate::PhysicalWordAddress::new(input.line_clean_address as u32);
                state.line_copy_source =
                    crate::PhysicalWordAddress::new(input.line_copy_source as u32);
                state.phase = DataMemoryPhase::LineCommandPrime;
            }
            return;
        }

        if command_ready && input.line_clean_start {
            state.maintenance_error = false;
            state.maintenance_active = true;
            state.line_clean_active = true;
            state.line_clean_address =
                crate::PhysicalWordAddress::new(input.line_clean_address as u32);
            state.phase = DataMemoryPhase::LineCommandPrime;
            return;
        }

        if accept_cpu {
            if input.cpu_write {
                state.may_have_dirty = true;
            }
            let address = crate::PhysicalWordAddress::new(input.cpu_address as u32);
            state.pending_read_word = (!input.cpu_write).then_some((address.get() & 15) as usize);
            state.pending_cpu_request = Some(if input.cpu_write {
                crate::CpuMemoryRequest::Write {
                    address,
                    value: input.cpu_write_data as u16,
                }
            } else {
                crate::CpuMemoryRequest::Read { address }
            });
            state.phase = DataMemoryPhase::Lookup;
            return;
        }

        if state.phase == DataMemoryPhase::Lookup {
            if let Some(request) = state.pending_cpu_request.take() {
                let address = match request {
                    crate::CpuMemoryRequest::Read { address }
                    | crate::CpuMemoryRequest::Write { address, .. } => address,
                };
                if address.get() >> 22 != 0 {
                    state.pending_read_word = None;
                    state.response_error = true;
                    state.response_data = 0;
                    state.response_valid = true;
                    state.phase = DataMemoryPhase::Idle;
                } else {
                    let action = state
                        .cache
                        .request(request)
                        .expect("idle data cache must accept a CPU request");
                    state.apply_action(action);
                }
            }
            return;
        }

        match state.phase {
            DataMemoryPhase::Idle => {}
            DataMemoryPhase::Lookup => unreachable!(),
            DataMemoryPhase::ScanPrime => state.phase = DataMemoryPhase::Scan,
            DataMemoryPhase::Scan => {
                let candidate = metadata_old
                    .iter()
                    .position(Option::is_some)
                    .map(|way| way as u8 * 64 + state.scan_set);
                if let Some(index) = candidate {
                    let request = state.next_maintenance_request(index);
                    state.start_request(request);
                } else if state.scan_set == 63 {
                    state.finish_maintenance();
                } else {
                    state.scan_set += 1;
                }
            }
            DataMemoryPhase::WritebackPrime => {
                state.beat = 0;
                state.phase = DataMemoryPhase::WritebackCapture;
            }
            // The RTL assembles the synchronous tag result in CAPTURE.
            // Data remains in the cache DPB until each ready-qualified beat.
            DataMemoryPhase::WritebackCapture => {
                if state.full_maintenance {
                    state.found_index = None;
                    state.scan_valid = false;
                    state.scan_done = false;
                    let other = 1 - state.wb_index / 64;
                    if state.wb_index < 64 && metadata_old[usize::from(other)].is_some() {
                        state.found_index = Some(other * 64 + state.wb_index % 64);
                        state.found_other = None;
                        state.scan_active = false;
                    } else if state.wb_index % 64 == 63 {
                        state.scan_active = false;
                        state.scan_done = true;
                    } else {
                        state.scan_active = true;
                        state.scan_read_set = state.wb_index % 64 + 1;
                        state.scan_return_set = state.scan_read_set;
                    }
                }
                state.phase = DataMemoryPhase::Request;
            }
            DataMemoryPhase::Request if input.memory_request_ready => {
                state.beat = 0;
                state.phase = if matches!(
                    state.request,
                    Some(crate::MainMemoryRequest::WriteLine { .. })
                ) {
                    DataMemoryPhase::WriteStream
                } else {
                    DataMemoryPhase::ReadReceive
                };
            }
            DataMemoryPhase::Request => {}
            DataMemoryPhase::WriteStream if input.memory_response_valid && input.memory_error => {
                state.phase = DataMemoryPhase::WriteResponse;
            }
            DataMemoryPhase::WriteStream if input.memory_write_data_ready => {
                if state.beat == CPU_V3_CACHE_MEMORY_BEATS - 1 {
                    state.phase = DataMemoryPhase::WriteResponse;
                } else {
                    state.beat += 1;
                }
            }
            DataMemoryPhase::WriteStream => {}
            DataMemoryPhase::WriteResponse
                if input.memory_response_valid
                    && (!state.full_maintenance || old_retire.is_none()) =>
            {
                if input.memory_error {
                    state.fail_transaction();
                } else if state.full_maintenance && old_found.is_some() {
                    state.cache.retire_maintenance_line(old_wb_index);
                } else {
                    state.complete_write();
                }
            }
            DataMemoryPhase::WriteResponse => {}
            DataMemoryPhase::Fault => {}
            DataMemoryPhase::LineCommandPrime => {
                state.phase = if !state.line_copy_active {
                    DataMemoryPhase::CleanLookup
                } else {
                    DataMemoryPhase::CopyPrepare
                };
            }
            DataMemoryPhase::CopyPrepare => {
                state.phase = DataMemoryPhase::CopyRefresh;
            }
            DataMemoryPhase::CopyRefresh => {
                state.phase = DataMemoryPhase::CopyLookup;
            }
            DataMemoryPhase::CopyLookup => {
                let action = state
                    .cache
                    .copy_line(state.line_copy_source, state.line_clean_address)
                    .expect("prepared data cache must accept line copy");
                state.apply_action(action);
            }
            DataMemoryPhase::CleanLookup => {
                let action = state
                    .cache
                    .clean_line(state.line_clean_address)
                    .expect("idle data cache must accept a clean-line command");
                state.apply_action(action);
            }
            DataMemoryPhase::ReadReceive if input.memory_response_valid => {
                if input.memory_error {
                    state.fail_transaction();
                } else {
                    let word = 4 * state.beat;
                    if let Some(requested_word) = state.pending_read_word {
                        if (word..word + 4).contains(&requested_word) {
                            state.response_data =
                                (input.memory_read_data >> (16 * (requested_word - word))) as u16;
                        }
                    }
                    state.words[word] = input.memory_read_data as u16;
                    state.words[word + 1] = (input.memory_read_data >> 16) as u16;
                    state.words[word + 2] = (input.memory_read_data >> 32) as u16;
                    state.words[word + 3] = (input.memory_read_data >> 48) as u16;
                    if state.beat == CPU_V3_CACHE_MEMORY_BEATS - 1 {
                        // The RTL writes the banks during the receive beats and
                        // responds on the last one; there is no drain phase.
                        state.beat = 0;
                        let action = state
                            .cache
                            .complete(crate::MainMemoryResponse::ReadLine { words: state.words })
                            .expect("data-cache completion must match a line read");
                        state.apply_action(action);
                    } else {
                        state.beat += 1;
                    }
                }
            }
            DataMemoryPhase::ReadReceive => {}
        }
        if promoted {
            if !current_terminal {
                state.retire_index = Some(old_wb_index);
            } else if old_phase == DataMemoryPhase::WriteStream {
                state.cache.retire_maintenance_line(old_wb_index);
            }
            state.promote_maintenance(old_found.unwrap(), next_granted);
        }
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("cpu_v3_data_cache.v")
                .replace(
                    "__CACHE_DATA_BANKS__",
                    &CpuV3DualPortCacheData::<ZeroBsramImage>::verilog_identity().module_name(),
                )
                .replace(
                    "__CACHE_METADATA__",
                    &CpuV3DataCacheMetadata::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3DualPortCacheData<ZeroBsramImage>>("u_data_banks"),
            VerilogDependency::new::<CpuV3DataCacheMetadata>("u_metadata"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_data_cache_tb.v").to_string())
    }
}

#[cfg(test)]
#[path = "data_tests.rs"]
mod tests;
