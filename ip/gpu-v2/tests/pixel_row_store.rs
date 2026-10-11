//! Independent old-state RAM golden and pre-edge HDL observation.
use gpu_v2::system::pixel::row_store::{self, Bank, Return};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

type Vector = (Option<usize>, Option<(usize, u32)>, Option<Return>);

fn vectors(rows: usize, width: u32) -> Vec<Vector> {
    let mut bank = Bank::new(rows, width).unwrap();
    let mut golden = vec![0; rows];
    let mask = u32::MAX >> (32 - width);
    let mut pending = None;
    let mut result = Vec::new();
    for wall in 0..1024 {
        let write = (wall < rows || wall % 7 == 0).then_some((
            wall % rows,
            (0x9753_2461u32.wrapping_mul(wall as u32 + 1)) & mask,
        ));
        let read = (wall >= rows && wall % 11 != 0)
            .then_some((wall * 5 + 3) % rows)
            .filter(|address| Some(*address) != write.map(|w| w.0));
        let returned = bank.tick(read, write).unwrap();
        assert_eq!(returned, pending, "old-state RAM return at wall {wall}");
        result.push((read, write, pending));
        pending = read.map(|address| Return {
            address,
            data: golden[address],
            issued: wall as u64 + 1,
        });
        if let Some((address, data)) = write {
            golden[address] = data;
        }
    }
    result
}

#[test]
fn independent_rows_and_collision_rejection() {
    for (rows, width) in [(256, 24), (128, 18), (128, 24)] {
        vectors(rows, width);
    }
    let mut bank = Bank::new(128, 24).unwrap();
    bank.tick(None, Some((9, 0x123456))).unwrap();
    assert!(bank.tick(Some(9), Some((9, 0x654321))).is_err());
    assert!(bank.tick(Some(128), None).is_err());
    assert!(bank.tick(None, Some((9, 0x1000000))).is_err());
    bank.tick(Some(9), None).unwrap();
    assert_eq!(bank.tick(None, None).unwrap().unwrap().data, 0x123456);
    for (rows, width) in [(0, 24), (3, 24), (128, 0), (128, 33), (128, 37), (1024, 24)] {
        assert!(row_store::verilog_sdp36(rows, width).is_err());
    }
}

fn bounded(mut command: Command, dir: &std::path::Path, name: &str) {
    let mut child = command
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{} {}",
                fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("row bank wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn row_bank_rtl_matches_previous_read_before_edge() {
    for (rows, width, primitive) in [
        (256, 24, false),
        (128, 18, false),
        (128, 24, false),
        (256, 24, true),
        (128, 18, true),
        (128, 24, true),
    ] {
        let trace = vectors(rows, width);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/quad-sequential-storage-20261008/row-{rows}-{width}-{primitive}"
        ));
        fs::create_dir_all(&dir).unwrap();
        let source = if primitive {
            row_store::verilog_sdp36(rows, width)
        } else {
            row_store::verilog(rows, width)
        }
        .unwrap();
        fs::write(dir.join("bank.v"), source).unwrap();
        let ab = rows.ilog2() - 1;
        let db = width - 1;
        let mut tb = format!("module tb;reg clk=0,reset=1,read_en=0,write_en=0; reg [{ab}:0] read_address=0,write_address=0;reg [{db}:0] write_data=0;wire return_valid,collision;wire [{db}:0] return_data;gpu_v2_pixel_row_store dut(.*);initial begin #10000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n");
        for (i, (read, write, returned)) in trace.into_iter().enumerate() {
            let (wa, wd) = write.unwrap_or((0, 0));
            tb.push_str(&format!("read_en={};read_address={};write_en={};write_address={wa};write_data={width}'h{wd:x};#1;if(collision!==0 || return_valid!=={}) $fatal(1,\"valid edge {i}\");\n",
                u8::from(read.is_some()),read.unwrap_or(0),u8::from(write.is_some()),u8::from(returned.is_some())));
            if let Some(r) = returned {
                tb.push_str(&format!(
                    "if(return_data!=={width}'h{:x}) $fatal(1,\"data edge {i}\");\n",
                    r.data
                ));
            }
            tb.push_str("clk=1;#1;clk=0;#1;\n");
        }
        tb.push_str("$display(\"PASS 1024 edges\");$finish;end endmodule\n");
        fs::write(dir.join("tb.v"), tb).unwrap();
        let mut compile =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        compile
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "test.vvp", "bank.v", "tb.v"]);
        if primitive {
            let home =
                std::env::var_os("GOWIN_HOME").expect("SDP36 primitive test requires GOWIN_HOME");
            compile.arg(std::path::Path::new(&home).join("IDE/simlib/gw2a/prim_sim.v"));
        }
        bounded(compile, &dir, "compile");
        let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        run.current_dir(&dir).arg("test.vvp");
        bounded(run, &dir, "run");
    }
}
