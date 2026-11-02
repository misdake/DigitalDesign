//! Exercise actual frontend flush routing without resetting its pending owners.
use super::*;
use std::collections::VecDeque;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cause {
    Icache,
    Halt,
    Fault,
}
#[derive(Clone, Copy)]
enum Phase {
    Reset,
    Warm,
    Arm,
    Held,
    Fresh,
}
struct Fetch {
    state: Rc<RefCell<CpuV3InstructionFetchQueueState>>,
    input: CpuV3InstructionFetchQueueInput,
    output: CpuV3InstructionFetchQueueOutput,
}
impl External for Fetch {
    fn execute(&mut self, c: &mut CircuitWires) {
        CpuV3InstructionFetchQueue::execute_emu(
            &mut self.state.borrow_mut(),
            c,
            &self.input,
            &self.output,
        );
    }
    fn clock(&mut self, c: &mut CircuitWires) {
        CpuV3InstructionFetchQueue::clock_emu(
            &mut self.state.borrow_mut(),
            c,
            &self.input,
            &self.output,
        );
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
struct Fixture {
    circuit: Circuit,
    input: CpuV3InstructionFetchQueueInput,
    output: CpuV3InstructionFetchQueueOutput,
    system: SystemControlDeviceInput,
    core: CpuV3CoreOutput,
    state: Rc<RefCell<CpuV3InstructionFetchQueueState>>,
    pending: VecDeque<(u32, u16)>,
    cause: Cause,
    version: u16,
    ticks: usize,
    drained: usize,
}
fn word(version: u16, address: u32) -> u16 {
    version << 12 | (address as u16 & 0x0fff)
}
impl Fixture {
    fn new(cause: Cause) -> Self {
        let state = Rc::new(RefCell::new(CpuV3InstructionFetchQueueState::default()));
        let (circuit, (input, output, system, core)) = build_circuit(|| {
            let mut input = CpuV3InstructionFetchQueueInput::allocate();
            let output = CpuV3InstructionFetchQueueOutput::allocate();
            let system = SystemControlDeviceInput::allocate();
            let so = SystemControlDeviceOutput::allocate();
            let core = CpuV3CoreOutput::allocate();
            input.flush = frontend_flush(&so, &core);
            external(Fetch {
                state: state.clone(),
                input: input.clone(),
                output: output.clone(),
            });
            SystemControlDevice::<469>::emu_connect(&system, &so);
            (input, output, system, core)
        });
        Self {
            circuit,
            input,
            output,
            system,
            core,
            state,
            pending: VecDeque::new(),
            cause,
            version: 5,
            ticks: 0,
            drained: 0,
        }
    }
    fn tick(
        &mut self,
        request: Option<u32>,
        deliver: bool,
        phase: Phase,
    ) -> CpuV3InstructionFetchQueueOutputValue {
        assert!(self.ticks < 100, "bounded frontend fixture");
        self.ticks += 1;
        let reset = matches!(phase, Phase::Reset);
        let terminal = matches!(phase, Phase::Arm | Phase::Held);
        self.circuit.set_wire(
            self.core.halted,
            (terminal && self.cause == Cause::Halt).into(),
        );
        self.circuit.set_wire(
            self.core.fault,
            (terminal && self.cause == Cause::Fault).into(),
        );
        let mut s = self.system.sample(&self.circuit);
        s.reset = reset;
        s.device_index = 0;
        s.device_channel = 0;
        s.device_read_enable = false;
        s.device_write_enable = matches!(phase, Phase::Arm) && self.cause == Cause::Icache;
        s.device_write_data = 0;
        s.dcache_maintenance_ready = false;
        s.dcache_maintenance_done = false;
        s.dcache_maintenance_error = false;
        s.watch_read_data = 0;
        self.system.drive(&mut self.circuit, &s);
        self.circuit.set_wire(self.input.reset, reset.into());
        self.circuit
            .set_wire(self.input.core_request_valid, request.is_some().into());
        bits(
            &mut self.circuit,
            self.input.core_address,
            u64::from(request.unwrap_or(0)),
        );
        self.circuit.set_wire(self.input.core_response_ready, 1);
        self.circuit.set_wire(self.input.memory_request_ready, 1);
        let response = if deliver {
            self.pending.front().copied()
        } else {
            None
        };
        self.circuit
            .set_wire(self.input.memory_response_valid, response.is_some().into());
        bits(
            &mut self.circuit,
            self.input.memory_read_data,
            u64::from(response.map_or(0, |(_, v)| v)),
        );
        self.circuit.set_wire(self.input.memory_error, 0);
        for _ in 0..3 {
            self.circuit.execute_gates();
        }
        let o = self.output.sample(&self.circuit);
        if response.is_some() && o.memory_response_ready {
            self.pending.pop_front();
            self.drained += 1;
        }
        if o.memory_request_valid {
            let address = o.memory_address as u32;
            self.pending
                .push_back((address, word(self.version, address)));
        }
        self.circuit.clock_tick();
        o
    }
}
#[test]
fn frontend_flush_sources_cancel_inflight_and_drop_cached_btc() {
    for cause in [Cause::Icache, Cause::Halt, Cause::Fault] {
        let mut f = Fixture::new(cause);
        f.tick(None, false, Phase::Reset);
        f.tick(None, false, Phase::Reset);
        let base = 0x1200;
        let mut warmed = 0;
        for _ in 0..20 {
            let o = f.tick(Some(base + warmed), true, Phase::Warm);
            if o.core_response_valid {
                assert_eq!(o.core_read_data, u64::from(word(5, base + warmed)));
                warmed += 1;
                if warmed == 2 {
                    break;
                }
            }
        }
        assert_eq!(warmed, 2);
        assert!(
            f.state.borrow().btc_statistics().installed > 0,
            "BTC must really be warm: {cause:?}"
        );
        for _ in 0..3 {
            if !f.pending.is_empty() {
                break;
            }
            f.tick(None, false, Phase::Warm);
        }
        assert!(
            !f.pending.is_empty(),
            "accepted lower response is genuinely pending"
        );
        assert!(f.pending.iter().all(|(_, v)| v >> 12 == 5));
        // Buffer a prefetched old word while retaining other accepted returns.
        f.tick(None, true, Phase::Warm);
        f.tick(None, false, Phase::Warm);
        assert!(f.state.borrow().btc_statistics().queue_cycles[1] > 0);
        assert!(!f.pending.is_empty());
        let hits = f.state.borrow().btc_statistics().hits;
        let old = f.pending.len();
        f.version = 6;
        f.tick(None, false, Phase::Arm);
        let held = f.tick(Some(base), true, Phase::Held);
        assert!(
            !held.core_response_valid && !held.memory_request_valid,
            "flush source gates both sides: {cause:?}"
        );
        assert!(
            held.memory_response_ready,
            "old response drains during flush: {cause:?}"
        );
        let drained = f.drained;
        let mut received = 0;
        for _ in 0..20 {
            let o = f.tick(Some(base + received), true, Phase::Fresh);
            if o.core_response_valid {
                assert_eq!(
                    o.core_read_data,
                    u64::from(word(6, base + received)),
                    "old BTC/queued/in-flight payload leaked: {cause:?}"
                );
                received += 1;
                if received == 2 {
                    break;
                }
            }
        }
        assert_eq!(received, 2, "fresh stream completes: {cause:?}");
        assert!(f.drained >= drained + old.saturating_sub(1));
        assert_eq!(
            f.state.borrow().btc_statistics().hits,
            hits,
            "no old BTC hit while the fresh stream resumes"
        );
        assert!(f.pending.iter().all(|(_, v)| v >> 12 == 6));
        eprintln!("frontend flush {cause:?}: warm BTC, queued old word, {old} pending returns; fresh words={received}");
    }
}
