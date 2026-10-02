use super::*;
use digital_design_circuit::{build_circuit, Circuit};
use digital_design_hardware::VerilogProject;

#[derive(Clone, Copy)]
enum Transfer {
    Read { base: usize, beat: usize },
    Write { base: usize, beat: usize },
    Response,
}

struct Harness {
    circuit: Circuit,
    input: CpuV3DataCacheInput,
    output: CpuV3DataCacheOutput,
    memory: Vec<u16>,
    transfer: Option<Transfer>,
    cycles: usize,
    writes: usize,
    reads: usize,
    stalled: bool,
    trace: Vec<(CpuV3DataCacheInputValue, CpuV3DataCacheOutputValue)>,
}

fn idle() -> CpuV3DataCacheInputValue {
    CpuV3DataCacheInputValue {
        memory_write_data_ready: false,
        reset: false,
        clean_all: false,
        invalidate_all: false,
        line_copy_start: false,
        line_copy_source: 0,
        line_copy_destination_page: 0,
        line_clean_start: false,
        line_clean_address: 0,
        cpu_request_valid: false,
        cpu_write: false,
        cpu_address: 0,
        cpu_write_data: 0,
        cpu_response_ready: true,
        memory_request_ready: true,
        memory_response_valid: false,
        memory_read_data: 0,
        memory_error: false,
    }
}

impl Harness {
    fn new(stalled: bool) -> Self {
        let (circuit, (input, output)) = build_circuit(|| {
            let input = CpuV3DataCacheInput::allocate();
            let output = CpuV3DataCache::emu(&input);
            (input, output)
        });
        let mut h = Self {
            circuit,
            input,
            output,
            memory: (0..262144)
                .map(|word| 0x8a31 ^ (word as u16).wrapping_mul(73) ^ (word >> 4) as u16)
                .collect(),
            transfer: None,
            cycles: 0,
            writes: 0,
            reads: 0,
            stalled,
            trace: Vec::new(),
        };
        let mut input = idle();
        input.reset = true;
        h.step(input);
        for _ in 0..66 {
            h.step(idle());
        }
        h
    }

    fn step(&mut self, mut input: CpuV3DataCacheInputValue) -> CpuV3DataCacheOutputValue {
        assert!(self.cycles < 200000, "cache harness cycle limit");
        input.memory_request_ready = !self.stalled || self.cycles % 7 >= 3;
        match self.transfer {
            Some(Transfer::Read { base, beat }) => {
                input.memory_response_valid = true;
                input.memory_read_data = (0..4)
                    .map(|lane| u64::from(self.memory[base + 4 * beat + lane]) << (16 * lane))
                    .fold(0, |a, b| a | b);
            }
            Some(Transfer::Write { .. }) => {
                input.memory_write_data_ready = !self.stalled || self.cycles % 5 == 4;
            }
            Some(Transfer::Response) => input.memory_response_valid = true,
            None => {}
        }
        self.input.drive(&mut self.circuit, &input);
        self.circuit.execute_gates();
        let out = self.output.sample(&self.circuit);
        assert!(!out.valid_sweep || !out.cpu_request_ready);
        self.trace.push((input.clone(), out.clone()));
        match self.transfer {
            Some(Transfer::Read { base, beat }) if out.memory_response_ready => {
                self.transfer = (beat != 3).then_some(Transfer::Read {
                    base,
                    beat: beat + 1,
                });
            }
            Some(Transfer::Write { base, beat }) if input.memory_write_data_ready => {
                for lane in 0..4 {
                    self.memory[base + 4 * beat + lane] =
                        (out.memory_write_data >> (16 * lane)) as u16;
                }
                self.transfer = Some(if beat == 3 {
                    Transfer::Response
                } else {
                    Transfer::Write {
                        base,
                        beat: beat + 1,
                    }
                });
            }
            Some(Transfer::Response) if out.memory_response_ready => self.transfer = None,
            _ => {}
        }
        if out.memory_request_valid && input.memory_request_ready {
            assert!(self.transfer.is_none(), "one memory transaction owner");
            let base = out.memory_address as usize;
            if out.memory_write {
                self.writes += 1;
                self.transfer = Some(Transfer::Write { base, beat: 0 });
            } else {
                self.reads += 1;
                self.transfer = Some(Transfer::Read { base, beat: 0 });
            }
        }
        self.circuit.clock_tick();
        self.cycles += 1;
        out
    }

    fn access(&mut self, write: bool, address: u32, value: u16) -> u16 {
        let mut pending = true;
        for _ in 0..500 {
            let mut input = idle();
            input.cpu_request_valid = pending;
            input.cpu_write = write;
            input.cpu_address = u64::from(address);
            input.cpu_write_data = u64::from(value);
            let out = self.step(input);
            if pending && out.cpu_request_ready {
                pending = false;
            }
            if out.cpu_response_valid {
                assert!(!out.cpu_error);
                return out.cpu_read_data as u16;
            }
        }
        panic!("cache access timeout");
    }

    fn clean(&mut self) -> usize {
        let start = self.cycles;
        let mut input = idle();
        input.clean_all = true;
        let out = self.step(input);
        if out.maintenance_done {
            return self.cycles - start;
        }
        for _ in 0..10000 {
            let out = self.step(idle());
            if out.maintenance_done {
                assert!(!out.maintenance_error);
                return self.cycles - start;
            }
        }
        panic!("full clean timeout");
    }
}

fn density_scenario(h: &mut Harness, density: usize, layout: usize) -> usize {
    let mut golden = h.memory[..2096].to_vec();
    for line in 0..128 {
        assert_eq!(h.access(false, (line * 16) as u32, 0), golden[line * 16]);
    }
    for n in 0..density {
        let line = match layout {
            0 => n,
            1 => 127 - n,
            _ => n * 128 / density,
        };
        for word in 0..16 {
            let address = line * 16 + word;
            let value = 0x52ac ^ (address as u16).wrapping_mul(97);
            golden[address] = value;
            h.access(true, address as u32, value);
        }
    }
    let writes = h.writes;
    let cycles = h.clean();
    assert_eq!(h.writes - writes, density);
    assert_eq!(
        &h.memory[..2096],
        golden.as_slice(),
        "full payload and guards"
    );
    assert!(h.clean() <= 3, "sticky hint must skip repeated empty clean");
    assert_eq!(h.writes - writes, density);
    let reads = h.reads;
    for line in 0..128 {
        assert_eq!(h.access(false, (line * 16) as u32, 0), golden[line * 16]);
    }
    assert_eq!(h.reads, reads, "clean retains all resident lines");
    cycles
}

#[test]
fn metadata_cycle_model_cleans_every_density_and_layout() {
    for stalled in [false, true] {
        for layout in 0..3 {
            for density in [0, 1, 8, 32, 64, 128] {
                let mut h = Harness::new(stalled);
                let cycles = density_scenario(&mut h, density, layout);
                println!("CACHE_CLEAN density={density} layout={layout} stalled={stalled} cycles={cycles}");
            }
        }
    }
}

#[test]
fn held_cpu_request_waits_for_metadata_scrub_acceptance() {
    let mut h = Harness::new(false);
    let mut input = idle();
    input.reset = true;
    h.step(input);
    let reads = h.reads;
    let mut accepted = false;
    let mut replied = false;
    for _ in 0..100 {
        let mut input = idle();
        input.cpu_request_valid = !accepted;
        input.cpu_write = true;
        input.cpu_address = 0x20;
        input.cpu_write_data = 0x6a5c;
        let out = h.step(input);
        if out.valid_sweep {
            assert!(!out.cpu_request_ready && !out.cpu_response_valid);
            assert!(!out.memory_request_valid);
        }
        if !accepted && out.cpu_request_ready {
            accepted = true;
        }
        if out.cpu_response_valid {
            assert!(accepted && !out.cpu_error);
            replied = true;
            break;
        }
    }
    assert!(accepted && replied);
    assert_eq!(h.reads - reads, 1);
    assert_eq!(h.access(false, 0x20, 0), 0x6a5c);
}

fn trace_tb(trace: &[(CpuV3DataCacheInputValue, CpuV3DataCacheOutputValue)]) -> String {
    use std::fmt::Write;
    let mut tb = String::from("module tb;reg clk=0;always #5 clk=~clk;\n");
    for value in CpuV3DataCacheInput::verilog_values(&idle()) {
        writeln!(tb, "reg [{}:0] {};", value.width - 1, value.name).unwrap();
    }
    for value in CpuV3DataCacheOutput::verilog_values(&trace[0].1) {
        writeln!(tb, "wire [{}:0] {};", value.width - 1, value.name).unwrap();
    }
    writeln!(
        tb,
        "{} dut(.*);initial begin",
        CpuV3DataCache::verilog_identity().module_name()
    )
    .unwrap();
    for (cycle, (input, output)) in trace.iter().enumerate() {
        for value in CpuV3DataCacheInput::verilog_values(input) {
            writeln!(tb, "{}={}'h{:x};", value.name, value.width, value.value).unwrap();
        }
        tb.push_str("#1;\n");
        for value in CpuV3DataCacheOutput::verilog_values(output) {
            let check = match value.name {
                "cpu_read_data" => output.cpu_response_valid,
                "memory_address" | "memory_write" | "memory_line" => output.memory_request_valid,
                "memory_write_data" => {
                    input.memory_write_data_ready
                        || output.memory_request_valid && output.memory_write
                }
                _ => true,
            };
            if check {
                writeln!(tb, "if({} !== {}'h{:x}) $fatal(1,\"cycle={cycle} {} expected=%h actual=%h\", {}'h{:x}, {});",
                    value.name, value.width, value.value, value.name, value.width, value.value, value.name).unwrap();
            }
        }
        tb.push_str("@(posedge clk);@(negedge clk);\n");
    }
    writeln!(tb, "$display(\"DIGITAL_DESIGN_PASS\");$finish;end\ninitial begin #{};$fatal(1,\"cycle replay timeout\");end\nendmodule", (trace.len()+10)*10).unwrap();
    tb
}

#[test]
#[ignore = "independent cache cycle model replay under portable and official DPB models"]
fn metadata_cycle_model_matches_rtl() {
    let mut h = Harness::new(true);
    density_scenario(&mut h, 128, 0);
    // Additional sparse holes after a clean must be found by the synchronous
    // pipeline, and a held request must not enter during the final scrub edge.
    h.access(true, 0x21, 0x7654);
    h.access(true, 0x7f1, 0x8abc);
    h.clean();
    let mut input = idle();
    input.reset = true;
    h.step(input);
    let mut accepted = false;
    for _ in 0..100 {
        let mut input = idle();
        input.cpu_request_valid = !accepted;
        input.cpu_address = 0x20;
        let output = h.step(input);
        if output.cpu_request_ready {
            accepted = true;
        }
        if output.cpu_response_valid {
            break;
        }
    }
    assert!(accepted);
    let tb = trace_tb(&h.trace);
    for vendor in [false, true] {
        run_rtl_golden(&tb, vendor);
    }
}

fn run_rtl_golden(tb: &str, vendor: bool) {
    static NEXT_DIRECTORY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let sequence = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "dcache-lifecycle-{}-{}-{}",
        std::process::id(),
        vendor,
        sequence
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let project = VerilogProject::generate::<CpuV3DataCache>().unwrap();
    let mut sources: Vec<String> = project.files.into_values().collect();
    if vendor {
        let data = CpuV3DualPortCacheData::<ZeroBsramImage>::verilog_source().unwrap();
        let header = data.split("reg [15:0] bank_0_memory").next().unwrap();
        let replacement = format!(
            "{header}{}",
            include_str!("cpu_v3_data_cache_vendor_data_body.vh")
        );
        let name = CpuV3DualPortCacheData::<ZeroBsramImage>::verilog_identity().module_name();
        let source = sources
            .iter_mut()
            .find(|s| s.contains(&format!("module {name}(")))
            .unwrap();
        *source = replacement;
    }
    std::fs::write(directory.join("modules.v"), sources.join("\n")).unwrap();
    std::fs::write(directory.join("tb.v"), tb).unwrap();
    let exe = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let mut compile = std::process::Command::new(exe);
    compile
        .current_dir(&directory)
        .args(["-g2012", "-D__ICARUS__", "-s", "tb", "-o", "sim.vvp"]);
    if vendor {
        compile.arg("-DCPU_V3_CACHE_METADATA_VENDOR").arg(
            std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap())
                .join("IDE/simlib/gw2a/prim_sim.v"),
        );
    }
    compile.args(["modules.v", "tb.v"]);
    let compile = compile.output().unwrap();
    assert!(
        compile.status.success(),
        "{}",
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
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "full-memory density and partial-error goldens under portable and official DPB models"]
fn verify_data_cache_lifecycle_goldens() {
    for vendor in [false, true] {
        for tb in [
            include_str!("cpu_v3_data_cache_lifecycle_tb.v"),
            include_str!("cpu_v3_data_cache_error_prefix_tb.v"),
        ] {
            run_rtl_golden(tb, vendor);
        }
    }
}
