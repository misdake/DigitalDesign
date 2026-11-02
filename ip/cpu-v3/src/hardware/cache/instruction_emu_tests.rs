//! Actual read-only wrapper differential with independent transaction expectations.
use super::*;
use digital_design_circuit::build_circuit;
use digital_design_hardware::{ModuleTest, TestStep};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
fn pattern(address: u32) -> u16 {
    ((address.wrapping_mul(17)) ^ 0x5a5a) as u16
}
struct Read {
    address: u32,
    beat: u8,
    delay: u8,
    error: bool,
}
fn command(mut c: Command, dir: &Path, label: &str) -> std::process::ExitStatus {
    c.current_dir(dir)
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{label}.stdout"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{label}.stderr"))).unwrap(),
        ));
    let mut child = c.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        if start.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{label} timeout: {}", dir.display());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
#[ignore = "bounded production read-only I-cache emulator/RTL replay"]
fn instruction_wrapper_matches_rtl_for_held_error_refill_and_invalidation() {
    let (mut circuit, (input, output)) = build_circuit(|| {
        let i = CpuV3InstructionCacheInput::allocate();
        let o = CpuV3InstructionCache::emu(&i);
        (i, o)
    });
    let jobs = [0x123, 0x523, 0x523, 0x923, 0x123, 0x123, 0x400003];
    let (mut issued, mut completed, mut held, mut idle) = (0, 0, 0, 0);
    let mut peer: Option<Read> = None;
    let mut pending = VecDeque::new();
    let mut counts = BTreeMap::<u32, usize>::new();
    let mut invalidated = false;
    let mut stalled_requests = 0;
    let mut trace = Vec::new();
    for cycle in 0..800 {
        let mut i = input.sample(&circuit);
        i.reset = cycle < 2;
        i.invalidate_all = false;
        i.cpu_request_valid = issued < jobs.len() && cycle >= 2;
        i.cpu_address = u64::from(jobs.get(issued).copied().unwrap_or(0));
        i.cpu_response_ready = true;
        i.memory_request_ready = peer.is_none() && cycle % 5 != 2;
        i.memory_response_valid = false;
        i.memory_read_data = 0;
        i.memory_error = false;
        if let Some(r) = &peer {
            if r.delay == 0 && cycle % 3 != 1 {
                i.memory_response_valid = true;
                i.memory_read_data = (0..4).fold(0, |v, k| {
                    v | u64::from(pattern(r.address + u32::from(r.beat) * 4 + k)) << (k * 16)
                });
                i.memory_error = r.error && r.beat == 2;
            }
            if r.address == 0x120 && counts[&0x120] == 2 && r.beat == 1 && !invalidated {
                i.invalidate_all = true;
                invalidated = true;
            }
        }
        input.drive(&mut circuit, &i);
        circuit.execute_gates();
        let before = output.sample(&circuit);
        if before.cpu_response_valid && held < 8 {
            i.cpu_response_ready = false;
            held += 1;
            input.drive(&mut circuit, &i);
            circuit.execute_gates();
        }
        let o = output.sample(&circuit);
        if i.cpu_request_valid && !o.cpu_request_ready {
            stalled_requests += 1;
        }
        if i.cpu_request_valid && o.cpu_request_ready {
            pending.push_back(issued);
            issued += 1;
        }
        if o.cpu_response_valid && i.cpu_response_ready {
            let id = pending
                .pop_front()
                .expect("CPU response has a request owner");
            assert_eq!(id, completed);
            let error = id == 0 || id == 6;
            assert_eq!(o.cpu_error, error, "request {id}");
            if !error {
                assert_eq!(
                    o.cpu_read_data,
                    u64::from(pattern(jobs[id])),
                    "request {id}"
                );
            }
            completed += 1;
            held = 0;
        }
        if i.memory_response_valid && o.memory_response_ready {
            let r = peer.as_mut().unwrap();
            if i.memory_error || r.beat == 3 {
                peer = None;
            } else {
                r.beat += 1;
                r.delay = 1;
            }
        } else if let Some(r) = &mut peer {
            r.delay = r.delay.saturating_sub(1);
        }
        if o.memory_request_valid && i.memory_request_ready {
            assert!(peer.is_none());
            let a = o.memory_address as u32;
            let count = counts.entry(a).or_default();
            *count += 1;
            peer = Some(Read {
                address: a,
                beat: 0,
                delay: 2,
                error: a == 0x120 && *count == 1,
            });
        }
        circuit.clock_tick();
        circuit.execute_gates();
        trace.push(TestStep::new(i, output.sample(&circuit)));
        if completed == jobs.len() {
            idle += 1;
            if idle == 70 {
                break;
            }
        }
    }
    assert_eq!(completed, jobs.len(), "bounded completion");
    assert!(invalidated && stalled_requests > 0);
    assert!(pending.is_empty());
    assert_eq!(
        counts,
        BTreeMap::from([(0x120, 3), (0x520, 1), (0x920, 1)]),
        "failed/invalidated line is refetched; hot successor remains a hit"
    );
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/functional-devices")
        .join(format!(
            "icache-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&dir).unwrap();
    let source = [
        CpuV3DualPortCacheData::<ZeroBsramImage>::verilog_source().unwrap(),
        CpuV3DataCacheMetadata::verilog_source().unwrap(),
        CpuV3TwoWayCache::verilog_source().unwrap(),
        CpuV3InstructionCache::verilog_source().unwrap(),
    ]
    .join("\n");
    fs::write(dir.join("modules-original.v"), &source).unwrap();
    let test = ModuleTest::<CpuV3InstructionCache>::new(trace);
    let tb = test.verilog_testbench();
    let tb = format!("{tb}\n");
    fs::write(dir.join("tb.v"), tb).unwrap();
    for corrupt in [false, true] {
        let run = if corrupt {
            dir.join("mutated-address")
        } else {
            dir.join("actual")
        };
        fs::create_dir_all(&run).unwrap();
        let dut = if corrupt {
            source.replace(
                ".cpu_address(cpu_address)",
                ".cpu_address(cpu_address ^ 32'h10)",
            )
        } else {
            source.clone()
        };
        if corrupt {
            assert_ne!(dut, source);
        }
        fs::write(run.join("modules.v"), dut).unwrap();
        fs::copy(dir.join("tb.v"), run.join("tb.v")).unwrap();
        let mut compile =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        compile.args(["-g2005", "-s", "tb", "-o", "run.vvp", "modules.v", "tb.v"]);
        assert!(
            command(compile, &run, "compile").success(),
            "compile: {}",
            run.display()
        );
        let mut vvp = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        vvp.arg("run.vvp");
        let status = command(vvp, &run, "rtl");
        let stdout = fs::read_to_string(run.join("rtl.stdout")).unwrap();
        if corrupt {
            assert!(
                stdout.contains("FAIL: step") && !stdout.contains("DIGITAL_DESIGN_PASS"),
                "mutated actual DUT must fail: {stdout}"
            );
        } else {
            assert!(
                status.success() && stdout.contains("DIGITAL_DESIGN_PASS"),
                "{}: {stdout}",
                run.display()
            );
        }
    }
}
