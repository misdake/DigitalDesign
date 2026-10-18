use super::*;
use std::collections::VecDeque;
const PAYLOAD_SLOTS: u8 = 2;
const REQUEST_SLOTS: u8 = 2;

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
    // Test-only full-address provenance, populated from accepted memory responses.
    queued_addresses: VecDeque<u32>,
    trace: Vec<(
        CpuV3InstructionFetchQueueInputValue,
        CpuV3InstructionFetchQueueOutputValue,
    )>,
    cycle: usize,
    version: u16,
    error_address: Option<u32>,
    response_presented: bool,
}

impl Rig {
    fn new() -> Self {
        Self {
            state: Default::default(),
            pending: VecDeque::new(),
            queued_addresses: VecDeque::new(),
            trace: Vec::new(),
            cycle: 0,
            version: 0,
            error_address: None,
            response_presented: false,
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
            .filter(|p| (release || self.response_presented) && p.due <= self.cycle);
        if response.is_some() {
            self.response_presented = true;
        }
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
                *self.queued_addresses.front().unwrap()
            } else {
                self.pending.front().unwrap().address
            };
            assert_eq!(source, address, "response provenance");
        }
        if flush || sig.restart {
            self.queued_addresses.clear();
        } else {
            if sig.queue_pop {
                self.queued_addresses.pop_front().unwrap();
            }
            if sig.enqueue {
                self.queued_addresses.push_back(response.unwrap().address);
            }
        }
        if out.memory_response_ready && input.memory_response_valid {
            self.pending.pop_front();
            self.response_presented = false;
        }
        if out.memory_request_valid && memory_ready {
            self.pending.push_back(Pending {
                address: out.memory_address as u32,
                due: self.cycle + 2,
                version: self.version,
            });
        }
        self.state.clock(&input);
        assert!(
            self.state.queue_count <= PAYLOAD_SLOTS && self.state.metadata_count <= REQUEST_SLOTS
        );
        assert!(
            self.state.queue_count + self.state.metadata_count <= PAYLOAD_SLOTS + REQUEST_SLOTS
        );
        assert_eq!(self.state.metadata_count as usize, self.pending.len());
        assert_eq!(self.state.queue_count as usize, self.queued_addresses.len());
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
        self.response_presented = false;
        self.queued_addresses.clear();
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
        assert_eq!(r.state.queue_count, PAYLOAD_SLOTS);
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
        assert_eq!(r.state.metadata_count, REQUEST_SLOTS);
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
    // A delayed same-offset response must remain stale across repeated segment
    // redirects. Drain it while accepting the current stream request.
    r.reset(0x91abc);
    r.tick(Some(0x71abc), false, true, false, false);
    r.tick(Some(0x81abc), false, false, false, false);
    r.tick(Some(0x91abc), false, false, false, false);
    let out = r.tick(Some(0x91abc), true, true, true, false);
    assert!(!out.core_response_valid);
    assert!(out.memory_response_ready && out.memory_request_valid);
    r.pair(0x91abc);
    // Fill and reuse a metadata slot without relying on BTC being enabled.
    r.reset(0xb1abc);
    for segment in 7..7 + u32::from(REQUEST_SLOTS) {
        let out = r.tick(Some((segment << 16) | 0x1abc), false, true, false, false);
        assert!(out.memory_request_valid);
    }
    assert_eq!(r.state.metadata_count, REQUEST_SLOTS);
    let reused = r.state.metadata_head as usize;
    let out = r.tick(Some(0xb1abc), false, true, true, false);
    assert!(!out.core_response_valid);
    assert!(out.memory_request_valid && out.memory_response_ready);
    assert_eq!(r.state.metadata_count, REQUEST_SLOTS);
    assert!(
        r.state.metadata_current[reused],
        "new slot owner wins stale drain"
    );
    r.pair(0xb1abc);
    r.pair(0xaffff);
    r.fetch(0xa0001);
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
    if CPU_V3_BTC_ENTRIES != 0 {
        // Target data may remain upstream. Once presented, that producer
        // must keep valid/data stable even when the delay generator closes.
        for target in [0x61001, 0x6ffff, 0x7fffe, 0x80000] {
            r.reset(target);
            for _ in 0..12 {
                r.tick(Some(target), false, true, true, false);
            }
            assert_eq!(r.state.queue_count, PAYLOAD_SLOTS);
            assert_eq!(r.state.statistics.installed, 0);
            for _ in 0..4 {
                assert!(
                    r.tick(Some(target), false, false, false, false)
                        .core_response_valid
                );
            }
            assert_eq!(r.fetch(target), 0);
            let second = next_word(target, 1);
            for _ in 0..12 {
                r.tick(Some(second), false, true, true, false);
            }
            assert_eq!(r.state.statistics.installed, 0);
            for _ in 0..4 {
                assert!(
                    r.tick(Some(second), false, false, false, false)
                        .core_response_valid
                );
            }
            assert_eq!(r.fetch(second), 0);
            assert_eq!(r.state.statistics.installed, 1);
            for offset in 2..5 {
                r.fetch(next_word(target, offset));
            }
            r.fetch(0x90000);
            assert_eq!(r.fetch(target), 0, "atomic install lost held target data");
            assert_eq!(r.fetch(second), 0);
        }
        for half in 0..2 {
            let target = 0xa00ff;
            r.reset(target);
            r.error_address = Some(next_word(target, half));
            for _ in 0..12 {
                r.tick(Some(target), false, true, true, false);
            }
            for offset in 0..4 {
                r.fetch(next_word(target, offset));
            }
            assert_eq!(r.state.statistics.installed, 0);
            r.error_address = None;
        }
        // A held first target response has no publication rights. Redirect
        // before accepting it must preserve every previously complete pair.
        r.reset(0xb0000);
        let targets: Vec<_> = (0..CPU_V3_BTC_ENTRIES)
            .map(|entry| 0xb0000 + entry as u32 * 16)
            .collect();
        for &target in &targets {
            r.pair(target);
        }
        let installs = r.state.statistics.installed;
        for _ in 0..12 {
            r.tick(Some(0xcffff), false, true, true, false);
        }
        assert_eq!(r.state.statistics.installed, installs);
        for target in targets {
            assert_eq!(r.fetch(target), 0, "unaccepted target evicted an old pair");
            assert_eq!(r.fetch(next_word(target, 1)), 0);
        }
        // Every slot is replaced repeatedly with distinct low/high words.
        // A first-word-only fill must leave all complete pairs untouched;
        // hold both replay words while continuation requests/responses stall.
        for round in 0..24_u32 {
            r.invalidate();
            r.version = (round as u16).wrapping_mul(0x1357);
            let targets: Vec<u32> = (0..CPU_V3_BTC_ENTRIES)
                .map(|i| {
                    if i + 1 == CPU_V3_BTC_ENTRIES && round & 1 != 0 {
                        0x3fffff
                    } else {
                        0x20000 + round * 0x800 + i as u32 * 16
                    }
                })
                .collect();
            for &target in &targets {
                r.pair(target);
            }
            for &target in &targets {
                for half in 0..2 {
                    let address = next_word(target, half);
                    for stall in 0..3 {
                        let out = r.tick(Some(address), false, stall & 1 != 0, stall == 2, false);
                        assert!(
                            out.core_response_valid,
                            "complete pair must replay during stalls"
                        );
                    }
                    assert_eq!(r.fetch(address), 0);
                }
            }
            let incomplete = 0x90000 + round * 16;
            let installs = r.state.btc_statistics().installed;
            r.fetch(incomplete);
            assert_eq!(
                r.fetch(targets[0]),
                0,
                "partial fill damaged the old victim"
            );
            r.fetch(next_word(targets[0], 1));
            assert_eq!(r.state.btc_statistics().installed, installs);
            r.pair(incomplete);
            assert_eq!(
                r.fetch(targets[0]),
                0,
                "atomic install evicted the wrong pair"
            );
        }
    }
    r
}

#[test]
fn bounded_btc_protocol_and_address_scoreboard() {
    scenarios();
}

fn btc_protocol_fixture() -> (String, usize) {
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
            tb.push_str("if (dut.btc_response) source_address = dut.next_word({10'b0,dut.btc_tag[dut.response_entry]},dut.first_btc_word ? 2'd0 : 2'd1); else source_address=dut.expected_core_address;\nif (source_address !== core_address) $fatal(1, \"BTC provenance\");\n");
        }
        writeln!(tb, "if (memory_address !== 32'h{:08x}) $fatal(1, \"BTC request address cycle {cycle}\");\n@(negedge clk);\nif (dut.queue_count > 2 || dut.metadata_count > 2 || dut.queue_count + dut.metadata_count > 4) $fatal(1, \"BTC reservation overflow\");",o.memory_address).unwrap();
    }
    tb.push_str("$display(\"BTC_PASS\"); $finish; end\ninitial begin repeat(20000) @(posedge clk); $fatal(1,\"BTC timeout\"); end\nendmodule\n");
    (tb, rig.trace.len())
}

#[test]
#[ignore = "explicit bounded BTC protocol/provenance co-simulation"]
fn btc_protocol_matches_iverilog() {
    run_btc_protocol_fixture(false);
}

#[test]
#[ignore = "explicit BTC protocol replay through official Gowin RAM16SDP4; requires GOWIN_HOME"]
fn btc_protocol_matches_gowin_ram16sdp4() {
    run_btc_protocol_fixture(true);
}

fn btc_vendor_primitive_library() -> std::path::PathBuf {
    let home = std::env::var_os("GOWIN_HOME")
        .expect("official BTC RAM16SDP4 test requires GOWIN_HOME; it must not silently skip");
    let home = std::path::PathBuf::from(home);
    let path = [
        home.join("IDE/simlib/gw2a/prim_sim.v"),
        home.join("simlib/gw2a/prim_sim.v"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap_or_else(|| {
        panic!(
            "official BTC RAM16SDP4 library missing under GOWIN_HOME={}; expected IDE/simlib/gw2a/prim_sim.v",
            home.display()
        )
    });
    path.canonicalize().unwrap()
}

fn btc_replace_once(source: &str, old: &str, new: &str) -> String {
    assert_eq!(
        source.matches(old).count(),
        1,
        "BTC primitive adapter source drift: {old}"
    );
    source.replacen(old, new, 1)
}

// Test-only substitution of the one inferred 16-bit payload RAM. All queue,
// BTC publication, ownership and output logic stays byte-for-byte unchanged.
// Four nibble slices implement a 16x16 bank; BTC8 needs two such banks.
fn btc_vendor_payload_source(source: &str) -> String {
    let source = btc_replace_once(
        source,
        "(* syn_ramstyle = \"distributed_ram\" *) reg [15:0] payload [0:PAYLOAD_WORDS-1];",
        "wire [15:0] vendor_payload_data;",
    );
    let write_block = "always @(posedge clk) begin\n    if (payload_write)\n        payload[payload_write_address] <= memory_read_data;\nend"
        .replace('\n', if source.contains("\r\n") { "\r\n" } else { "\n" });
    let source = btc_replace_once(&source, &write_block, "");
    let source = btc_replace_once(
        &source,
        "payload[payload_read_address]",
        "vendor_payload_data",
    );
    let banks = if CPU_V3_BTC_ENTRIES == 8 { 2 } else { 1 };
    let write_bank = if banks == 1 {
        "1'b1"
    } else {
        "(payload_write_address[4] == vb)"
    };
    let read_bank = if banks == 1 {
        "vendor_q[0]"
    } else {
        "vendor_q[payload_read_address[4]]"
    };
    let native = r#"
wire [15:0] vendor_q [0:BANKS-1];
genvar vb,vs;
generate for(vb=0;vb<BANKS;vb=vb+1) begin:vendor_bank
 for(vs=0;vs<4;vs=vs+1) begin:vendor_slice
  RAM16SDP4 ram(.DO(vendor_q[vb][vs*4+:4]),.DI(memory_read_data[vs*4+:4]),
   .RAD(payload_read_address[3:0]),.WAD(payload_write_address[3:0]),
   .WRE(payload_write && BANK_WRITE),.CLK(clk));
 end
end endgenerate
assign vendor_payload_data=READ_BANK;
"#
    .replace("BANKS", &banks.to_string())
    .replace("BANK_WRITE", write_bank)
    .replace("READ_BANK", read_bank);
    btc_replace_once(&source, "\nendmodule", &format!("\n{native}\nendmodule"))
}

// These observers add coverage assertions only. Both tests run precisely the
// same scenario-generated vectors, expected outputs and address scoreboard.
fn btc_protocol_coverage(fixture: &str, source: &str) -> String {
    let queued = if source.contains("reg queue_valid") {
        "dut.queue_valid"
    } else {
        assert!(
            source.contains("queue_count"),
            "unknown BTC queue occupancy"
        );
        "(dut.queue_count != 0)"
    };
    assert!(source.contains("enqueue_response"), "missing enqueue event");
    let monitor = r#"
always @(posedge clk) if(!reset && !flush) begin
 if(dut.core_pop && !dut.btc_response && QUEUED && dut.payload_write &&
    dut.payload_read_address==dut.payload_write_address)
  ram_collision_events=ram_collision_events+1;
 if(dut.install && dut.enqueue_response)
  install_enqueue_events=install_enqueue_events+1;
end
"#
    .replace("QUEUED", queued);
    let tb = btc_replace_once(
        fixture,
        "module tb;",
        "module tb;\ninteger ram_collision_events=0,install_enqueue_events=0;",
    );
    let tb = btc_replace_once(&tb, "\nendmodule", &format!("\n{monitor}\nendmodule"));
    let checks = format!(
        "$display(\"BTC_EVENTS collisions=%0d install_enqueue=%0d\",ram_collision_events,install_enqueue_events);\n\
         if(ram_collision_events==0)$fatal(1,\"missing same-edge old-Q collision coverage\");\n\
         if({CPU_V3_BTC_ENTRIES}>0 && install_enqueue_events==0)$fatal(1,\"missing BTC install plus enqueue coverage\");\n\
         $display(\"BTC_PASS\");"
    );
    btc_replace_once(&tb, "$display(\"BTC_PASS\");", &checks)
}

fn btc_bounded_command(
    command: &mut std::process::Command,
    directory: &std::path::Path,
    label: &str,
) -> (std::process::ExitStatus, String, String) {
    let stdout = directory.join(format!("{label}.stdout.log"));
    let stderr = directory.join(format!("{label}.stderr.log"));
    let mut child = command
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > std::time::Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "BTC {label} exceeded 60 seconds; logs in {}",
                directory.display()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    (
        status,
        std::fs::read_to_string(stdout).unwrap(),
        std::fs::read_to_string(stderr).unwrap(),
    )
}

fn run_btc_protocol_fixture(official: bool) {
    // Resolve before generating the fixture: a missing installation is a hard
    // error for the explicitly requested official test, never a skipped pass.
    let library = official.then(btc_vendor_primitive_library);
    let (fixture, cycles) = btc_protocol_fixture();
    let reference = CpuV3InstructionFetchQueue::verilog_source().unwrap();
    let tb = btc_protocol_coverage(&fixture, &reference);
    let source = if official {
        btc_vendor_payload_source(&reference)
    } else {
        reference.clone()
    };
    let flavor = if official {
        "gowin-ram16sdp4"
    } else {
        "inferred"
    };
    let directory = std::env::temp_dir().join(format!(
        "btc-protocol-{}-{CPU_V3_BTC_ENTRIES}-{flavor}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    for (name, text) in [
        ("dut.v", source.as_str()),
        ("reference_dut.v", reference.as_str()),
        ("tb.v", tb.as_str()),
        ("fixture.v", fixture.as_str()),
    ] {
        std::fs::write(directory.join(name), text).unwrap();
    }
    std::fs::write(directory.join("cycles.txt"), cycles.to_string()).unwrap();
    if let Some(path) = &library {
        std::fs::write(
            directory.join("primitive-path.txt"),
            path.display().to_string(),
        )
        .unwrap();
    }
    let mut command = std::process::Command::new(
        std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
    );
    command
        .current_dir(&directory)
        .args(["-g2005", "-s", "tb", "-o", "sim.vvp", "dut.v", "tb.v"]);
    if let Some(path) = &library {
        command.arg(path);
    }
    let (status, stdout, stderr) = btc_bounded_command(&mut command, &directory, "compile");
    assert!(
        status.success(),
        "BTC {flavor} compile failed: {stdout}\n{stderr}"
    );
    let mut command =
        std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    command.current_dir(&directory).arg("sim.vvp");
    let (status, stdout, stderr) = btc_bounded_command(&mut command, &directory, "run");
    if let Ok(root) = std::env::var("CPU_V3_BTC_EVIDENCE") {
        let mut evidence =
            std::path::PathBuf::from(root).join(format!("entries-{CPU_V3_BTC_ENTRIES}"));
        if official {
            evidence = evidence.join("gowin-ram16sdp4");
        }
        std::fs::create_dir_all(&evidence).unwrap();
        for name in [
            "dut.v",
            "reference_dut.v",
            "tb.v",
            "fixture.v",
            "cycles.txt",
            "compile.stdout.log",
            "compile.stderr.log",
            "run.stdout.log",
            "run.stderr.log",
        ] {
            std::fs::copy(directory.join(name), evidence.join(name)).unwrap();
        }
        if official {
            std::fs::copy(
                directory.join("primitive-path.txt"),
                evidence.join("primitive-path.txt"),
            )
            .unwrap();
        }
    }
    assert!(
        status.success() && stdout.contains("BTC_PASS"),
        "BTC {flavor} failed: {stdout}\n{stderr}"
    );
    println!("BTC_PROTOCOL flavor={flavor} entries={CPU_V3_BTC_ENTRIES} cycles={cycles}\n{stdout}");
    let resolved = directory.canonicalize().unwrap();
    let temporary_root = std::env::temp_dir().canonicalize().unwrap();
    assert!(resolved.starts_with(&temporary_root) && resolved != temporary_root);
    std::fs::remove_dir_all(resolved).unwrap();
}
