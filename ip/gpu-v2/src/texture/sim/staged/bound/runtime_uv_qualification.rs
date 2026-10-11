//! Replay actual Runtime UV commands into an independent controller instance
//! and both behavioral/native RTL. Numerical acceptance uses the scalar oracle.
use super::{qualification as q, *};
use crate::texture::{emu::uv_store, ports::Filter, rtl};
use std::{
    fmt::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn packed(uv: [i64; 8]) -> String {
    // Hex each 36-bit lane separately, preserving signed18 raw bits.
    (0..4)
        .rev()
        .map(|i| {
            format!(
                "{:09x}",
                (uv[i * 2] as u64 & 0x3ffff) | ((uv[i * 2 + 1] as u64 & 0x3ffff) << 18)
            )
        })
        .collect()
}
fn run(paused: bool, start: u64, count: usize) -> std::path::PathBuf {
    let (slots, bytes) = q::fixture();
    let mut r = q::runtime(&slots, super::super::control::Hardware::default());
    let mut mc = q::physical::Physical::new(4096, bytes, true, paused);
    let inputs: Vec<_> = (0..count)
        .map(|i| {
            let mut input = q::input(
                (i % 16) as u8,
                if paused {
                    [15, 0, 1, 10, 6, 15, 3, 0][i % 8]
                } else {
                    15
                },
                if paused {
                    [-0.03125 + (i % 7) as f64 / 32.0, 0.46875]
                } else {
                    [0.13, 0.07]
                },
                Filter::Trilinear,
            );
            input.lod_bias = 0.5;
            input.force_coarsest = paused && i % 9 == 0;
            input
        })
        .collect();
    let expected: Vec<_> = inputs
        .iter()
        .flat_map(|input| {
            if input.force_coarsest {
                // Fixture mip0 is one logical texel, RAW565 0xf800 everywhere;
                // helper fallback must select that level regardless of UV.
                (0..4)
                    .filter(|lane| input.mask >> lane & 1 != 0)
                    .map(|lane| ColorOutput {
                        key: input.quad_id * 4 + lane,
                        rgb: [255, 0, 0],
                    })
                    .collect()
            } else {
                q::golden(input)
            }
        })
        .collect();
    let mut actual = vec![];
    let mut replay = uv_store::Store::new(8).unwrap();
    let mut random = 0x673ab961u32;
    let mut admitted = 0;
    let mut writes = 0;
    let mut slot_writes = [0usize; 8];
    let mut d_before_final_write = 0;
    let mut reads = 0;
    let mut returns = 0;
    let mut paused_returns = 0;
    let mut last = None;
    let mut oldest = [None; 8];
    let mut c_lane = [0; 8];
    let mut input_uv = [[0i64; 8]; 8];
    let mut held = None;
    let mut bench=String::from("module tb;reg clk=0,reset=1,ce=0;reg[2:0]phase=0,in_slot=0,c_slot=0;reg[1:0]c_lane=0;reg in_valid=0,d_take=0,c_request=0,c_take=0;reg[143:0]in_uv=0;wire in_ready,d_valid,c_empty,c_valid,write_enable,read_enable;wire[2:0]d_slot,c_owner_slot;wire[1:0]c_owner_lane;wire[143:0]d_uv;wire[31:0]c_uv;wire[4:0]write_address,read_address;wire[35:0]write_data;gpu_v2_uv_store dut(.*);reg gs=1;GSR global_reset(.GSRI(gs));initial begin #1000000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n");
    let mut csv =
        String::from("wall,phase,CE,admit,D_take,C_take,W_address,R_address,returned_address\n");
    for wall in 0..60_000u64 {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let s = r
            .step(
                &mut mc,
                (wall >= start).then(|| inputs.get(admitted)).flatten(),
                timed::Control {
                    ce: !paused || random & 7 > 1,
                    result_ready: !paused || wall > 1000 && random % 13 > 2,
                },
            )
            .unwrap();
        let trace = &r.preparation.trace;
        let t = trace.uv_tick;
        let ready = replay.input_ready();
        let d = replay.derivative();
        let c = replay.coordinate();
        let empty = replay.coordinate_empty();
        if let Some((owner, data)) = c {
            if let Some((old_owner, old_data)) = held {
                assert_eq!((owner, data), (old_owner, old_data));
            }
        }
        held = c.filter(|_| !t.take_coordinate);
        if let Some((slot, uv)) = t.admission {
            assert!(ready && oldest[slot].is_none());
            input_uv[slot] = uv;
            slot_writes[slot] = 0;
            oldest[slot] = Some(admitted);
            c_lane[slot] = 0;
            admitted += 1;
        }
        if t.take_derivative {
            let (slot, uv) = d.unwrap();
            assert_eq!(uv, input_uv[slot]);
            d_before_final_write += usize::from(slot_writes[slot] < 4);
        }
        if t.take_coordinate {
            let (owner, uv) = c.unwrap();
            assert_eq!(
                uv,
                [
                    input_uv[owner.slot][owner.lane * 2] as u32 & 0xffff,
                    input_uv[owner.slot][owner.lane * 2 + 1] as u32 & 0xffff
                ]
            );
            assert!(owner.lane >= c_lane[owner.slot]);
            c_lane[owner.slot] = owner.lane + 1;
        }
        let edge = replay.tick(t).unwrap();
        assert_eq!(edge, trace.uv);
        assert_eq!(edge.returned, last);
        last = edge.read;
        if let Some((address, _)) = edge.write {
            slot_writes[address / 4] += 1;
        }
        writes += usize::from(edge.write.is_some());
        reads += usize::from(edge.read.is_some());
        returns += usize::from(edge.returned.is_some());
        paused_returns += usize::from(!t.ce && edge.returned.is_some());
        for event in &s.preparation.events {
            if let PreparationEvent::SharedRelease { slot, .. } = event {
                assert!(oldest[*slot].take().is_some());
            }
        }
        let (ins, inu) = t
            .admission
            .map_or((0, String::from("0")), |(slot, uv)| (slot, packed(uv)));
        let req = t
            .coordinate
            .unwrap_or(uv_store::Coordinate { slot: 0, lane: 0 });
        writeln!(bench,"ce={};phase={};in_valid={};in_slot={ins};in_uv=144'h{inu};d_take={};c_request={};c_slot={};c_lane={};c_take={};#1;",
            u8::from(t.ce),t.phase,u8::from(t.admission.is_some()),u8::from(t.take_derivative),u8::from(t.coordinate.is_some()),req.slot,req.lane,u8::from(t.take_coordinate)).unwrap();
        writeln!(bench,"if(in_ready!=={}||d_valid!=={}||c_valid!=={}||c_empty!=={}||write_enable!=={}||read_enable!=={}) $fatal(1,\"signals wall{wall}\");",
            u8::from(ready),u8::from(d.is_some()),u8::from(c.is_some()),u8::from(empty),u8::from(edge.write.is_some()),u8::from(edge.read.is_some())).unwrap();
        if let Some((slot, uv)) = d {
            writeln!(
                bench,
                "if(d_slot!==3'd{slot}||d_uv!==144'h{}) $fatal(1,\"D wall{wall}\");",
                packed(uv)
            )
            .unwrap();
        }
        if let Some((owner, uv)) = c {
            writeln!(bench,"if(c_owner_slot!==3'd{}||c_owner_lane!==2'd{}||c_uv!==32'h{:08x}) $fatal(1,\"C wall{wall}\");",owner.slot,owner.lane,u64::from(uv[0])|(u64::from(uv[1])<<16)).unwrap();
        }
        if let Some((address, data)) = edge.write {
            writeln!(bench,"if(write_address!==5'd{address}||write_data!==36'h{data:09x}) $fatal(1,\"W wall{wall}\");").unwrap();
        }
        if let Some(read) = edge.read {
            writeln!(
                bench,
                "if(read_address!==5'd{}) $fatal(1,\"R wall{wall}\");",
                read.address
            )
            .unwrap();
        }
        bench.push_str("clk=1;#1;clk=0;#1;\n");
        writeln!(
            csv,
            "{wall},{},{},{},{},{},{:?},{:?},{:?}",
            t.phase,
            t.ce,
            t.admission.is_some(),
            t.take_derivative,
            t.take_coordinate,
            edge.write.map(|w| w.0),
            edge.read.map(|r| r.address),
            edge.returned.map(|r| r.address)
        )
        .unwrap();
        actual.extend(s.results);
        if admitted == inputs.len() && r.idle() {
            break;
        }
    }
    assert!(r.idle() && replay.idle());
    assert_eq!(actual, expected);
    assert_eq!(writes, inputs.len() * 4);
    assert_eq!(reads, returns);
    assert_eq!(
        reads,
        inputs
            .iter()
            .map(|q| q.mask.count_ones() as usize)
            .sum::<usize>()
    );
    if paused {
        assert!(paused_returns > 0);
    }
    if start >= 6 {
        assert!(d_before_final_write > 0);
    }
    bench.push_str("$display(\"PASS Runtime UV calendar\");$finish;end endmodule\n");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/sampling-ram-lowering-20261008/replay-{paused}-phase{start}"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), bench).unwrap();
    std::fs::write(dir.join("calendar.csv"), csv).unwrap();
    for native in [false, true] {
        std::fs::write(
            dir.join(format!("controller-{native}.v")),
            rtl::uv_store::source(native),
        )
        .unwrap();
    }
    std::fs::write(dir.join("receipt.json"),format!("{{\"quads\":{admitted},\"writes\":{writes},\"reads\":{reads},\"returns\":{returns},\"CE_off_returns\":{paused_returns},\"D_before_final_write\":{d_before_final_write},\"read_peak\":1,\"write_peak\":1}}\n")).unwrap();
    dir
}
#[test]
fn actual_uv_calendar_and_shared_raw_head_match_oracle() {
    for paused in [false, true] {
        run(paused, 0, 64);
    }
    for phase in 1..8 {
        run(false, phase, 2);
    }
}
fn bounded(mut command: Command, dir: &Path, name: &str) {
    command.stdout(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
    ));
    command.stderr(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
    ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{name} watchdog");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "requires Icarus and Gowin primitive library"]
fn actual_uv_controller_native_and_behavioral_match_runtime_calendar() {
    for (paused, phase, count) in [(false, 0, 64), (true, 0, 64)]
        .into_iter()
        .chain((1..8).map(|phase| (false, phase, 2)))
    {
        let dir = run(paused, phase, count);
        for native in [false, true] {
            let mut compile = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
            compile
                .current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o"])
                .arg(format!("test-{native}.vvp"))
                .arg(format!("controller-{native}.v"))
                .arg("tb.v")
                .arg(
                    Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            bounded(compile, &dir, &format!("compile-{native}"));
            let mut exec = Command::new(std::env::var_os("VVP_EXE").unwrap());
            exec.current_dir(&dir).arg(format!("test-{native}.vvp"));
            bounded(exec, &dir, &format!("run-{native}"));
        }
    }
}
