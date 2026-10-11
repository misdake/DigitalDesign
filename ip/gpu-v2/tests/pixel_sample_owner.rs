use gpu_v2::system::pixel::sample_owner::{self, Owner, ResultBeat, Tick};
use std::{
    fmt::Write,
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct Edge {
    tick: Tick,
    ready: bool,
    key: Option<u8>,
    accepted: bool,
}

/// Independent public-lane oracle retains masks; the DUT adapter retains none.
fn trace() -> Vec<Edge> {
    let mut owner = Owner::default();
    let mut masks = [0u8; 16];
    let mut destinations = [0u8; 16];
    let mut edges = Vec::new();
    let mut blocked_releases = 0;
    let mut paused_results = 0;
    for bank in 0..2 {
        for coverage in 1..16u8 {
            for id in 0..16u8 {
                let slot = bank * 16 + id;
                let t = Tick {
                    ce: true,
                    admission: Some(slot),
                    ..Default::default()
                };
                assert!(owner.input_ready(slot));
                let s = owner.tick(t);
                assert!(s.accepted && !s.fault);
                masks[id as usize] = coverage;
                destinations[id as usize] = slot;
                edges.push(Edge {
                    tick: t,
                    ready: true,
                    key: None,
                    accepted: true,
                });
            }
            for lane in 0..4u8 {
                if coverage & (1 << lane) == 0 {
                    continue;
                }
                for id in (0..16u8).rev() {
                    let key = id * 4 + lane;
                    let last = masks[id as usize] == 1 << lane;
                    // The other global half deliberately aliases the owned ID.
                    for ce in [false, true] {
                        let slot = destinations[id as usize] ^ 16;
                        let expected = destinations[id as usize] * 4 + lane;
                        assert_eq!(owner.destination(key), Some(expected));
                        assert!(!owner.input_ready(slot));
                        let t = Tick {
                            ce,
                            admission: Some(slot),
                            result: Some(ResultBeat {
                                key,
                                quad_last: last,
                            }),
                            ..Default::default()
                        };
                        let s = owner.tick(t);
                        assert_eq!(s.result_key, ce.then_some(expected));
                        assert!(!s.accepted && !s.fault);
                        edges.push(Edge {
                            tick: t,
                            ready: false,
                            key: Some(expected),
                            accepted: false,
                        });
                        if ce {
                            masks[id as usize] &= !(1 << lane);
                            blocked_releases += usize::from(last);
                        } else {
                            paused_results += 1;
                        }
                    }
                }
            }
            assert!(owner.idle());
            assert!(masks.iter().all(|&m| m == 0));
        }
    }
    assert_eq!(blocked_releases, 2 * 15 * 16);
    assert!(paused_results > 1000);
    assert!(edges.len() < 10_000);
    edges
}

#[test]
fn all_masks_and_both_global_halves_use_last_result_and_old_alias_credit() {
    assert_eq!(sample_owner::DESTINATION_BITS, 32);
    trace();
}

#[test]
fn unowned_result_faults_before_any_admission_and_reset_releases_ownership() {
    let mut owner = Owner::default();
    assert!(
        owner
            .tick(Tick {
                ce: true,
                admission: Some(31),
                ..Default::default()
            })
            .accepted
    );
    let bad = owner.tick(Tick {
        ce: true,
        admission: Some(0),
        result: Some(ResultBeat {
            key: 0,
            quad_last: true,
        }),
        ..Default::default()
    });
    assert!(bad.fault && !bad.accepted && bad.result_key.is_none());
    assert!(!owner.input_ready(0));
    assert!(
        !owner
            .tick(Tick {
                ce: true,
                admission: Some(0),
                ..Default::default()
            })
            .accepted
    );
    owner.tick(Tick {
        reset: true,
        ..Default::default()
    });
    assert!(owner.idle() && owner.input_ready(0) && owner.input_ready(31));
}

fn bounded(mut command: Command) -> String {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            let out = child.wait_with_output().unwrap();
            assert!(
                out.status.success(),
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            return String::from_utf8(out.stdout).unwrap();
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            panic!("sample-owner simulator watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires IVERILOG_EXE and VVP_EXE"]
fn actual_adapter_rtl_matches_independent_public_lane_oracle() {
    let root = std::env::var_os("GPU_SAMPLE_OWNER_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/pixel-sample-owner")
        });
    fs::create_dir_all(&root).unwrap();
    let edges = trace();
    let mut tb = String::from("module tb;reg clk=0,reset=1,ce=0,in_valid=0,result_valid=0,result_quad_last=0;reg[4:0]in_slot=0;reg[5:0]result_key=0;wire in_ready,result_owned,accepted,write_fire,fault;wire[6:0]global_key;gpu_v2_pixel_sample_owner dut(.*);initial begin #100000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n");
    for (wall, e) in edges.iter().enumerate() {
        let r = e.tick.result.unwrap_or(ResultBeat {
            key: 0,
            quad_last: false,
        });
        writeln!(tb, "ce={};in_valid={};in_slot={};result_valid={};result_quad_last={};result_key={};#1;if(in_ready!==1'b{} || accepted!==1'b{} || fault!==0)$fatal(1,\"ready wall{wall}\");", u8::from(e.tick.ce),u8::from(e.tick.admission.is_some()),e.tick.admission.unwrap_or(0),u8::from(e.tick.result.is_some()),u8::from(r.quad_last),r.key,u8::from(e.ready),u8::from(e.accepted)).unwrap();
        if let Some(key) = e.key {
            writeln!(tb, "if(!result_owned || global_key!==7'd{key} || write_fire!==1'b{})$fatal(1,\"destination wall{wall}\");",u8::from(e.tick.ce)).unwrap();
        }
        tb.push_str("clk=1;#1;clk=0;#1;if(fault!==0)$fatal(1,\"unexpected fault\");\n");
    }
    tb.push_str("ce=1;in_valid=1;in_slot=0;result_valid=1;result_key=0;result_quad_last=1;#1;if(accepted || write_fire)$fatal(1,\"preflight\");clk=1;#1;clk=0;#1;if(!fault)$fatal(1,\"fault latch\");reset=1;clk=1;#1;clk=0;reset=0;result_valid=0;#1;if(fault || !in_ready)$fatal(1,\"reset\");$display(\"PASS sample owner\");$finish;end endmodule\n");
    fs::write(root.join("dut.v"), sample_owner::verilog()).unwrap();
    fs::write(root.join("tb.v"), tb).unwrap();
    let mut compile = Command::new(std::env::var_os("IVERILOG_EXE").expect("IVERILOG_EXE"));
    compile
        .current_dir(&root)
        .args(["-g2012", "-s", "tb", "-o", "sim.vvp", "dut.v", "tb.v"]);
    bounded(compile);
    let mut run = Command::new(std::env::var_os("VVP_EXE").expect("VVP_EXE"));
    run.current_dir(&root).arg("sim.vvp");
    assert!(bounded(run).contains("PASS sample owner"));
}
