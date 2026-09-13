use super::*;
use std::collections::VecDeque;

fn pattern(address: u32, version: u16) -> u16 {
    (address as u16) ^ ((address >> 16) as u16).rotate_left(7) ^ 0x6a95 ^ version
}

struct Pending {
    address: u32,
    due: usize,
    version: u16,
}

struct Rig {
    state: CpuV3InstructionFetchQueueState,
    pending: VecDeque<Pending>,
    trace: Vec<(
        CpuV3InstructionFetchQueueInputValue,
        CpuV3InstructionFetchQueueOutputValue,
    )>,
    cycle: usize,
    version: u16,
    error_address: Option<u32>,
}

impl Rig {
    fn new() -> Self {
        Self {
            state: Default::default(),
            pending: VecDeque::new(),
            trace: Vec::new(),
            cycle: 0,
            version: 0,
            error_address: None,
        }
    }

    fn tick(
        &mut self,
        address: Option<u32>,
        ready: bool,
        memory_ready: bool,
        release: bool,
        flush: bool,
    ) -> CpuV3InstructionFetchQueueOutputValue {
        assert!(self.cycle < 20_000, "bounded fetch test timed out");
        let response = self
            .pending
            .front()
            .filter(|p| release && p.due <= self.cycle);
        let input = CpuV3InstructionFetchQueueInputValue {
            reset: false,
            flush,
            core_request_valid: address.is_some(),
            core_address: address.unwrap_or(0).into(),
            core_response_ready: ready,
            memory_request_ready: memory_ready,
            memory_response_valid: response.is_some(),
            memory_read_data: response.map_or(0, |p| pattern(p.address, p.version)).into(),
            memory_error: response
                .is_some_and(|p| p.address >> 22 != 0 || Some(p.address) == self.error_address),
        };
        let sig = self.state.signals(&input);
        let out = sig.output;
        if out.core_response_valid {
            let address = address.unwrap();
            assert!(!flush);
            assert_eq!(
                out.core_error,
                address >> 22 != 0 || Some(address) == self.error_address,
                "wrong fault at cycle {} address {address:08x}",
                self.cycle
            );
            assert_eq!(
                out.core_read_data as u16,
                pattern(address, self.version),
                "wrong/stale data at cycle {} address {address:08x}",
                self.cycle
            );
            // Check the full address independently of the 16-bit data hash.
            let source = if sig.btc_response {
                let entry = sig.hit.unwrap_or(self.state.replay_entry);
                next_word(
                    self.state.btc[entry].tag,
                    u32::from(sig.hit.is_none() && self.state.replay_remaining == 1),
                )
            } else if self.state.queue_count != 0 {
                self.state.queue_address[self.state.queue_head as usize]
            } else {
                self.pending.front().unwrap().address
            };
            assert_eq!(source, address, "response provenance");
        }
        if out.memory_response_ready && input.memory_response_valid {
            self.pending.pop_front();
        }
        if out.memory_request_valid && memory_ready {
            self.pending.push_back(Pending {
                address: out.memory_address as u32,
                due: self.cycle + 2,
                version: self.version,
            });
        }
        self.state.clock(&input);
        assert!(self.state.queue_count <= 4 && self.state.metadata_count <= 4);
        assert!(self.state.queue_count + self.state.metadata_count <= 4);
        assert_eq!(self.state.metadata_count as usize, self.pending.len());
        let mut ranks: Vec<_> = self
            .state
            .btc
            .iter()
            .filter(|e| e.valid)
            .map(|e| e.rank)
            .collect();
        ranks.sort();
        assert_eq!(
            ranks,
            (0..ranks.len() as u8).collect::<Vec<_>>(),
            "LRU permutation"
        );
        self.trace.push((input, out.clone()));
        self.cycle += 1;
        out
    }

    fn fetch(&mut self, address: u32) -> usize {
        for wait in 0..200 {
            if self
                .tick(Some(address), true, true, true, false)
                .core_request_ready
            {
                return wait;
            }
        }
        panic!("fetch timeout at {address:08x}");
    }

    fn pair(&mut self, target: u32) {
        self.fetch(target);
        self.fetch(next_word(target, 1));
    }

    fn invalidate(&mut self) {
        self.tick(None, true, true, true, true);
    }

    fn reset(&mut self, address: u32) {
        let input = CpuV3InstructionFetchQueueInputValue {
            reset: true,
            flush: false,
            core_request_valid: true,
            core_address: address.into(),
            core_response_ready: true,
            memory_request_ready: true,
            memory_response_valid: true,
            memory_read_data: 0xbeef,
            memory_error: true,
        };
        let out = self.state.signals(&input).output;
        assert!(
            !out.core_response_valid && !out.memory_request_valid && !out.memory_response_ready
        );
        self.state.clock(&input);
        self.pending.clear();
        self.trace.push((input, out));
        self.cycle += 1;
    }
}

fn scenarios() -> Rig {
    let mut r = Rig::new();
    // Different segments and line/PC boundary crossings, including PC ffff.
    for target in [0x1100f, 0x2100f, 0x3fffff, 0x10000] {
        r.pair(target);
    }
    if CPU_V3_BTC_ENTRIES != 0 {
        for target in [0x1100f, 0x2100f, 0x3fffff, 0x10000] {
            let out = r.tick(Some(target), true, true, true, false);
            assert!(
                out.core_request_ready,
                "warm target must respond in restart cycle"
            );
            if out.memory_request_valid {
                assert_eq!(out.memory_address, next_word(target, 2) as u64);
            }
            assert_eq!(r.fetch(next_word(target, 1)), 0);
            assert_eq!(
                r.fetch(next_word(target, 2)),
                0,
                "two-cycle hit continuation"
            );
        }
        // Backpressure both BTC words while the ordinary queue fills with T+2.
        let target = 0x1100f;
        for _ in 0..12 {
            let out = r.tick(Some(target), false, true, true, false);
            assert!(out.core_response_valid);
        }
        assert_eq!(r.state.queue_count, 4);
        assert_eq!(r.fetch(target), 0);
        for _ in 0..4 {
            assert!(
                r.tick(Some(target + 1), false, true, true, false)
                    .core_response_valid
            );
        }
        assert_eq!(r.fetch(target + 1), 0);
        assert_eq!(r.fetch(target + 2), 0);

        // Reject T+2 for several cycles; BTC delivery does not wait for it.
        assert!(
            r.tick(Some(target), true, false, true, false)
                .core_request_ready
        );
        let out = r.tick(Some(target + 1), true, false, true, false);
        assert!(out.core_request_ready);
        for _ in 0..4 {
            let out = r.tick(Some(target + 2), true, false, true, false);
            assert!(!out.core_response_valid);
            assert_eq!(out.memory_address, (target + 2) as u64);
        }
        r.fetch(target + 2);
        assert!(r.state.statistics.continuation_wait_cycles >= 4);

        // Fill every metadata slot; no responses until multiple fast restarts
        // have occurred. On the next restart the full FIFO drains and reuses
        // its head/tail slot in the same cycle.
        for target in [0x2100f, 0x1100f, 0x2100f, 0x1100f, 0x2100f, 0x1100f] {
            assert!(
                r.tick(Some(target), true, true, false, false)
                    .core_request_ready
            );
        }
        assert_eq!(r.state.metadata_count, 4);
        let out = r.tick(Some(0x2100f), true, true, true, false);
        assert!(out.core_request_ready && out.memory_request_valid && out.memory_response_ready);
        r.fetch(0x21010);
        r.fetch(0x21011);

        // Exact LRU: touch the oldest, install one new pair, evict the second.
        r.invalidate();
        for i in 0..CPU_V3_BTC_ENTRIES {
            r.pair(0x50000 + i as u32 * 16);
        }
        assert_eq!(r.fetch(0x50000), 0);
        r.fetch(0x50001);
        r.pair(0x60000);
        assert_eq!(r.fetch(0x50000), 0);
        assert!(r.fetch(0x50010) > 0);

        // A partial fill must not replace a complete entry.
        r.pair(0x71000);
        let installs = r.state.statistics.installed;
        r.fetch(0x72000);
        assert_eq!(r.fetch(0x71000), 0);
        assert_eq!(r.state.statistics.installed, installs);
        assert!(r.state.statistics.cancelled_fills > 0);
    }
    // Invalid high address must not alias a resident legal target.
    r.pair(0x12340);
    assert!(r.fetch(0x00412340) > 0);
    assert!(r.fetch(0x80412340) > 0);
    // Error in either half must never install instruction data.
    for error in [0x22340, 0x22341] {
        r.invalidate();
        r.error_address = Some(error);
        r.pair(0x22340);
        let installs = r.state.statistics.installed;
        r.fetch(0x33333);
        assert!(r.fetch(0x22340) > 0);
        assert_eq!(r.state.statistics.installed, installs);
        r.error_address = None;
    }
    // Flush wins over a hit; mutate backing instructions while stale responses
    // are outstanding, then verify refetches use the new version.
    r.pair(0x31000);
    let out = r.tick(Some(0x31000), true, true, false, true);
    assert!(!out.core_response_valid && !out.memory_request_valid);
    r.version = 0x9876;
    r.pair(0x31000);
    // Also invalidate halfway through a replay.
    r.fetch(0x32000);
    r.fetch(0x31000);
    r.invalidate();
    r.version = 0x1234;
    r.pair(0x31000);
    r.fetch(0x32000);
    r.reset(0x31000);
    r.version = 0x5678;
    assert!(r.fetch(0x31000) > 0);
    r.fetch(0x31001);

    // Deterministic adversarial traffic: redirects, request gaps, flushes,
    // response delays and core/downstream backpressure. Every response is
    // checked against the requested full address and independent memory data.
    let mut rng = 0x912a_87c3u32;
    let mut address = 0x31002;
    for _ in 0..2500 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        if rng & 15 == 0 {
            address = 0x10000 + ((rng >> 8) & 7) * 16;
        }
        let out = r.tick(
            if rng & 32 != 0 { Some(address) } else { None },
            rng & 64 != 0,
            rng & 128 != 0,
            rng & 256 != 0,
            rng & 1023 == 0,
        );
        if out.core_request_ready {
            address = next_word(address, 1);
        }
    }
    r
}

#[test]
fn bounded_btc_protocol_and_address_scoreboard() {
    scenarios();
}

#[test]
#[ignore = "explicit bounded BTC protocol/provenance co-simulation"]
fn btc_protocol_matches_iverilog() {
    let rig = scenarios();
    let mut tb = String::from("module tb;\nreg clk=0; always #5 clk=~clk;\nreg reset=1,flush=0,core_request_valid=0,core_response_ready=0,memory_request_ready=0,memory_response_valid=0,memory_error=0;\nreg [31:0] core_address=0; reg [15:0] memory_read_data=0;\nwire core_request_ready,core_response_valid,core_error,memory_request_valid,memory_response_ready; wire [15:0] core_read_data; wire [31:0] memory_address;\nCpuV3InstructionFetchQueue dut(.*);\nreg [31:0] source_address;\ninitial begin repeat(2) @(negedge clk); reset=0;\n");
    for (cycle, (i, o)) in rig.trace.iter().enumerate() {
        use std::fmt::Write;
        writeln!(tb, "reset={};", u8::from(i.reset)).unwrap();
        writeln!(tb, "flush={}; core_request_valid={}; core_address=32'h{:08x}; core_response_ready={}; memory_request_ready={}; memory_response_valid={}; memory_read_data=16'h{:04x}; memory_error={}; #1;",
            u8::from(i.flush),u8::from(i.core_request_valid),i.core_address,u8::from(i.core_response_ready),u8::from(i.memory_request_ready),u8::from(i.memory_response_valid),i.memory_read_data,u8::from(i.memory_error)).unwrap();
        writeln!(tb, "if ({{core_request_ready,core_response_valid,memory_request_valid,memory_response_ready}} !== 4'b{}{}{}{}) $fatal(1, \"BTC handshake cycle {cycle}\");",
            u8::from(o.core_request_ready),u8::from(o.core_response_valid),u8::from(o.memory_request_valid),u8::from(o.memory_response_ready)).unwrap();
        if o.core_response_valid {
            writeln!(tb, "if (core_read_data !== 16'h{:04x} || core_error !== 1'b{}) $fatal(1, \"BTC data cycle {cycle}\");",o.core_read_data,u8::from(o.core_error)).unwrap();
            tb.push_str("if (dut.btc_response) source_address = dut.next_word({10'b0,dut.btc_tag[dut.response_entry]},dut.first_btc_word ? 2'd0 : 2'd1); else if (dut.response_bypass) source_address=dut.metadata_address[dut.metadata_head]; else source_address=dut.queue_address[dut.queue_head];\nif (source_address !== core_address) $fatal(1, \"BTC provenance\");\n");
        }
        writeln!(tb, "if (memory_address !== 32'h{:08x}) $fatal(1, \"BTC request address cycle {cycle}\");\n@(negedge clk);\nif (dut.queue_count > 4 || dut.metadata_count > 4 || dut.queue_count + dut.metadata_count > 4) $fatal(1, \"BTC reservation overflow\");",o.memory_address).unwrap();
    }
    tb.push_str("$display(\"BTC_PASS\"); $finish; end\ninitial begin repeat(20000) @(posedge clk); $fatal(1,\"BTC timeout\"); end\nendmodule\n");
    let directory = std::env::temp_dir().join(format!("btc-protocol-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("dut.v"),
        CpuV3InstructionFetchQueue::verilog_source().unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("tb.v"), tb).unwrap();
    let compile = std::process::Command::new(
        std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
    )
    .current_dir(&directory)
    .args(["-g2005", "-s", "tb", "-o", "sim.vvp", "dut.v", "tb.v"])
    .output()
    .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run =
        std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
            .current_dir(&directory)
            .arg("sim.vvp")
            .output()
            .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && stdout.contains("BTC_PASS"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    std::fs::remove_dir_all(directory).unwrap();
}
