//! Independent byte/owner stimulus and actual-port RTL replay.
use super::*;
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(crate) struct Replay {
    tb: String,
    edges: usize,
    fault: bool,
}
impl Replay {
    pub(crate) fn new() -> Self {
        Self{tb:String::from("module tb;reg clk=0,reset=0,ce=0,reserve_valid=0,write_valid=0,owner_request=0,consume=0;reg[5:0]reserve_owner=0;reg[71:0]write_payload=0;wire producer_ready,front_valid,fault,bad;wire[71:0]front_payload,returned_payload;wire[4:0]producer_count,unwritten_count;wire[5:0]group_count,row_count,allocate_row,write_row,transfer_row,read_row,reclaim_row,strobes,owner_value;wire[3:0]owner_address;wire[1:0]head_valid_out;wire pending_out;\n`ifdef NATIVE\nGSR GSR(.GSRI(1'b1));\n`endif\ngpu_v2_packet_store dut(.*);initial begin #8000000;$fatal(1,\"watchdog\");end initial begin\n"),edges:0,fault:false}
    }
    pub(crate) fn inject(&mut self, text: &str) {
        self.tb.push_str(text);
        self.tb.push('\n');
    }
    pub(crate) fn sample(
        &mut self,
        t: Tick,
        e: Result<Edge, String>,
        before: Option<u128>,
        after: (Control, Option<u128>, bool),
    ) {
        let error = e.is_err();
        let e = e.unwrap_or_default();
        let b = |v| u8::from(v);
        writeln!(self.tb,"reset={};ce={};reserve_valid={};reserve_owner=6'd{};write_valid={};write_payload=72'h{:018x};owner_request={};consume={};#1;",b(t.reset),b(t.ce),b(t.reserve.is_some()),t.reserve.unwrap_or(0),b(t.write.is_some()),t.write.unwrap_or(0),b(t.owner_read),b(t.consume)).unwrap();
        let active = t.ce && !t.reset && !error;
        let strobes = if active {
            b(e.reserve.is_some())
                | b(e.write.is_some()) << 1
                | b(e.transfer.is_some()) << 2
                | b(e.read.is_some()) << 3
                | b(e.capture.is_some()) << 4
                | b(e.consume.is_some()) << 5
        } else {
            0
        };
        writeln!(
            self.tb,
            "if(bad!==1'b{} || strobes!==6'd{})$fatal(1,\"ports {}\");",
            b(error && t.ce && !self.fault),
            strobes,
            self.edges
        )
        .unwrap();
        if !t.reset {
            writeln!(
                self.tb,
                "if(front_valid!==1'b{})$fatal(1,\"old valid {}\");",
                b(before.is_some()),
                self.edges
            )
            .unwrap();
            if let Some(word) = before {
                writeln!(
                    self.tb,
                    "if(front_payload!==72'h{word:018x})$fatal(1,\"old head {}\");",
                    self.edges
                )
                .unwrap();
            }
        }
        if active {
            if let Some(row) = e.owner_read {
                writeln!(
                    self.tb,
                    "if(owner_address!==4'd{row})$fatal(1,\"owner RAD\");"
                )
                .unwrap();
            }
            if let Some(word) = t.write {
                let key = ((word >> 66) & 15) * 4 + ((word >> 70) & 3);
                writeln!(self.tb, "if(owner_value!==6'd{key})$fatal(1,\"owner DO\");").unwrap();
            }
            if let Some((_, _, word)) = e.capture {
                writeln!(
                    self.tb,
                    "if(returned_payload!==72'h{word:018x})$fatal(1,\"payload DO\");"
                )
                .unwrap();
            }
        }
        self.tb.push_str("clk=1;#1;\n");
        let (c, front, fault) = after;
        for (name, value) in [
            ("alloc", u64::from(c.allocate)),
            ("wptr", c.write.into()),
            ("tptr", c.transfer.into()),
            ("rptr", c.read.into()),
            ("reclaim", c.reclaim.into()),
            ("pc", c.producer.into()),
            ("gc", c.groups.into()),
            ("rows", c.rows.into()),
            ("uw", c.unwritten.into()),
            ("published", c.published.into()),
            ("hv", (b(c.head_valid[0]) | b(c.head_valid[1]) << 1).into()),
            ("rh", c.read_head.into()),
            ("ph", c.pop_head.into()),
            ("pending", b(c.pending).into()),
            ("prow", c.pending_row.into()),
            ("phead", c.pending_head.into()),
            ("owner_valid", b(c.owner_valid).into()),
            ("owner_row", c.owner_row.into()),
            ("fault", b(fault).into()),
        ] {
            writeln!(
                self.tb,
                "if(dut.{name}!=={value})$fatal(1,\"state {name} {} got=%h\",dut.{name});",
                self.edges
            )
            .unwrap();
        }
        if let Some(word) = front {
            writeln!(
                self.tb,
                "if(!front_valid || front_payload!==72'h{word:018x})$fatal(1,\"new head {}\");",
                self.edges
            )
            .unwrap();
        }
        self.tb.push_str("clk=0;#1;\n");
        self.edges += 1;
        self.fault = fault;
    }
    pub(crate) fn finish(mut self, path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        self.tb
            .push_str("$display(\"PASS packet store\");$finish;end\n`ifdef NATIVE\n");
        for b in 0..2 {
            writeln!(self.tb,"always @(posedge clk) if(dut.payload_hardware.banks[{b}].bank.CEA!==dut.pw || dut.payload_hardware.banks[{b}].bank.CEB!==dut.pr || dut.payload_hardware.banks[{b}].bank.DI!==write_payload[{}+:36] || dut.payload_hardware.banks[{b}].bank.ADA!=={{3'b0,dut.wptr,1'b0,4'b1111}} || dut.payload_hardware.banks[{b}].bank.ADB!=={{3'b0,dut.rptr,5'b0}})$fatal(1,\"payload native pins\");",b*36).unwrap();
        }
        self.tb.push_str("`ifdef BSRAM_OWNER\nalways @(posedge clk)begin if(dut.ordered.synchronous_owner.hardware.bank.CEA!==dut.ow || dut.ordered.synchronous_owner.hardware.bank.CEB!==dut.or_en || dut.ordered.synchronous_owner.hardware.bank.DI!=={30'b0,reserve_owner} || dut.ordered.synchronous_owner.hardware.bank.ADA!=={5'b0,dut.alloc[3:0],1'b0,4'b1111} || dut.ordered.synchronous_owner.hardware.bank.ADB!=={5'b0,dut.owner_ra,5'b0})$fatal(1,\"owner BSRAM pins\");if(dut.pw && dut.ordered.synchronous_owner.hardware.q[35:6]!==30'b0)$fatal(1,\"owner padding\");end\n`endif\n`ifdef SSRAM_OWNER\n");
        for i in 0..2 {
            writeln!(self.tb,"always @(posedge clk) if(dut.ordered.asynchronous_owner.hardware.cells[{i}].bank.WRE!==dut.ow || dut.ordered.asynchronous_owner.hardware.cells[{i}].bank.WAD!==dut.alloc[3:0] || dut.ordered.asynchronous_owner.hardware.cells[{i}].bank.RAD!==dut.wptr[3:0] || dut.ordered.asynchronous_owner.hardware.cells[{i}].bank.DI!==dut.ordered.asynchronous_owner.hardware.d[{}+:4])$fatal(1,\"owner SSRAM pins\");",i*4).unwrap();
        }
        self.tb.push_str("always @(posedge clk)if(dut.pw&&dut.ordered.asynchronous_owner.hardware.q[7:6]!==2'b0)$fatal(1,\"SRAM padding\");\n`endif\n`endif\nendmodule\n");
        std::fs::write(path.join("tb.v"), self.tb).unwrap();
        for (name, mode, native) in [
            ("reference", 0, true),
            ("portable", 1, false),
            ("bsram", 1, true),
            ("ssram", 2, true),
        ] {
            std::fs::write(
                path.join(format!("{name}.v")),
                crate::texture::rtl::packet_store::source(mode, native).unwrap(),
            )
            .unwrap();
        }
        std::fs::write(path.join("receipt.json"),format!("{{\"edges\":{},\"scope\":\"actual complete pool ports and control, model versus four RTL variants\"}}",self.edges)).unwrap();
    }
}
fn bounded(mut c: Command, path: &Path, label: &str) {
    c.stdout(Stdio::from(
        std::fs::File::create(path.join(format!("{label}.out"))).unwrap(),
    ))
    .stderr(Stdio::from(
        std::fs::File::create(path.join(format!("{label}.err"))).unwrap(),
    ));
    let mut child = c.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{label}: {} {}",
                std::fs::read_to_string(path.join(format!("{label}.out"))).unwrap(),
                std::fs::read_to_string(path.join(format!("{label}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("packet RTL process watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(crate) fn run(path: &Path) {
    std::thread::scope(|scope| {
        for name in ["reference", "portable", "bsram", "ssram"] {
            scope.spawn(move || run_variant(path, name));
        }
    });
}
fn run_variant(path: &Path, name: &str) {
    let mut c = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
    if name != "portable" {
        c.arg("-DNATIVE");
    }
    if name == "bsram" {
        c.arg("-DBSRAM_OWNER");
    }
    if name == "ssram" {
        c.arg("-DSSRAM_OWNER");
    }
    c.current_dir(path).args([
        "-g2012",
        "-s",
        "tb",
        "-o",
        &format!("{name}.vvp"),
        &format!("{name}.v"),
        "tb.v",
    ]);
    if name != "portable" {
        c.arg(
            PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap())
                .join("IDE/simlib/gw2a/prim_sim.v"),
        );
    }
    bounded(c, path, &format!("compile-{name}"));
    let mut c = Command::new(std::env::var_os("VVP_EXE").unwrap());
    c.current_dir(path).arg(format!("{name}.vvp"));
    bounded(c, path, &format!("run-{name}"));
    assert!(
        std::fs::read_to_string(path.join(format!("run-{name}.out")))
            .unwrap()
            .contains("PASS packet store")
    );
}
fn step(s: &mut Store, r: &mut Replay, t: Tick) -> Result<Edge, String> {
    let before = s.front();
    let e = s.tick(t);
    r.sample(t, e.clone(), before, (s.control, s.front(), s.faulted()));
    e
}
fn word(i: u64) -> u128 {
    let quad = (i * 7) % 16;
    let lane = (i * 3) % 4;
    // Distinct data, first/last bits and key; no DUT packet packing helper.
    ((u128::from(i) * 0x123456789ab_u128) & ((1_u128 << 66) - 1))
        | (u128::from(quad) << 66)
        | (u128::from(lane) << 70)
}
fn directed(path: &Path) {
    let mut s = Store::default();
    let mut r = Replay::new();
    let mut rng = 0x79237_u32;
    step(
        &mut s,
        &mut r,
        Tick {
            reset: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut poison_pipe = [None; 3];
    for i in 0..32 {
        let value = 20_000 + i;
        step(
            &mut s,
            &mut r,
            Tick {
                ce: true,
                reserve: Some((((value * 7) % 16) * 4 + (value * 3) % 4) as u8),
                write: poison_pipe[2].map(word),
                owner_read: poison_pipe[1].is_some(),
                ..Default::default()
            },
        )
        .unwrap();
        poison_pipe = [Some(value), poison_pipe[0], poison_pipe[1]];
        if s.control.pending && s.front().is_some() {
            break;
        }
        assert!(i < 31, "poison live boundary");
    }
    assert!(s.control.pending && s.front().is_some());
    for epoch in 0..3 {
        step(
            &mut s,
            &mut r,
            Tick {
                reset: true,
                ..Default::default()
            },
        )
        .unwrap();
        let salt = 1001 + 257 * epoch;
        let mut issued = 0u64;
        let mut retired = 0u64;
        let mut pipe = [None; 3];
        for wall in 0..30_000 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let ce = wall % 23 != 0 && !rng.is_multiple_of(5);
            let reserve = (issued < 256 && s.producer_ready()).then_some(issued + salt);
            let consume = wall > 600 && s.front().is_some() && !rng.is_multiple_of(7);
            let t = Tick {
                ce,
                reserve: reserve.map(|i| (((i * 7) % 16) * 4 + (i * 3) % 4) as u8),
                write: pipe[2].map(word),
                owner_read: pipe[1].is_some(),
                consume,
                ..Default::default()
            };
            if ce && consume {
                assert_eq!(s.front(), Some(word(retired + salt)));
                retired += 1;
            }
            step(&mut s, &mut r, t).unwrap();
            if ce {
                pipe = [reserve, pipe[0], pipe[1]];
                issued += u64::from(reserve.is_some());
            }
            if issued == 256 && s.idle() {
                assert_eq!(retired, 256);
                break;
            }
            assert!(wall < 29_999, "directed drain {epoch}");
        }
    }
    r.finish(path);
}
#[test]
fn packet_storage_model_poison_reset_wrap_and_credits() {
    directed(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sampling-packet-storage-20261008/directed"),
    );
}
#[test]
#[ignore = "requires Icarus and Gowin native primitive library"]
fn packet_storage_directed_native_replay() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sampling-packet-storage-20261008/directed-rtl");
    directed(&p);
    run(&p);
}

fn faults(path: &Path) {
    let mut r = Replay::new();
    for fault in 0..12 {
        let mut s = Store::default();
        step(
            &mut s,
            &mut r,
            Tick {
                reset: true,
                ..Default::default()
            },
        )
        .unwrap();
        let mut pipe = [None; 3];
        for wall in 0..64 {
            let issued = wall;
            if s.control.pending && s.control.owner_valid && s.front().is_some() {
                break;
            }
            let t = Tick {
                ce: true,
                reserve: Some((((issued * 7) % 16) * 4 + (issued * 3) % 4) as u8),
                write: pipe[2].map(word),
                owner_read: pipe[1].is_some(),
                ..Default::default()
            };
            step(&mut s, &mut r, t).unwrap();
            pipe = [Some(issued), pipe[0], pipe[1]];
            assert!(wall < 63, "fault boundary");
        }
        match fault {
            0 => {
                s.control.allocate ^= 1;
                r.inject(&format!("dut.alloc=6'd{};", s.control.allocate));
            }
            1 => {
                s.control.write ^= 1;
                r.inject(&format!("dut.wptr=6'd{};", s.control.write));
            }
            2 => {
                s.control.transfer ^= 1;
                r.inject(&format!("dut.tptr=6'd{};", s.control.transfer));
            }
            3 => {
                s.control.read = s.control.write;
                r.inject(&format!("dut.rptr=6'd{};", s.control.read));
            }
            4 => {
                s.control.reclaim ^= 1;
                r.inject(&format!("dut.reclaim=6'd{};", s.control.reclaim));
            }
            5 => {
                s.control.producer = 16;
                r.inject("dut.pc=5'd16;");
            }
            6 => {
                s.control.pending_row ^= 1;
                r.inject(&format!("dut.prow=6'd{};", s.control.pending_row));
            }
            7 => {
                s.control.owner_row ^= 1;
                r.inject(&format!("dut.owner_row=4'd{};", s.control.owner_row));
            }
            8 => {
                s.owner_do ^= 1;
                r.inject(&format!("force dut.stored_owner=6'd{};", s.owner_do));
            }
            9 => {
                s.control.published ^= 1 << (s.control.transfer & 15);
                r.inject(&format!("dut.published=16'd{};", s.control.published));
            }
            10 => {
                s.control.pending_head ^= 1;
                r.inject(&format!("dut.phead=1'b{};", s.control.pending_head));
            }
            _ => {
                s.control.read_head ^= 1;
                r.inject(&format!("dut.rh=1'b{};", s.control.read_head));
            }
        }
        let before = (
            s.control,
            s.owners,
            s.payload,
            s.owner_do,
            s.payload_do,
            s.heads,
        );
        let t = Tick {
            ce: true,
            reserve: Some(0),
            write: pipe[2].map(word),
            owner_read: pipe[1].is_some(),
            consume: true,
            ..Default::default()
        };
        assert!(step(&mut s, &mut r, t).is_err());
        assert_eq!(
            (
                s.control,
                s.owners,
                s.payload,
                s.owner_do,
                s.payload_do,
                s.heads
            ),
            before,
            "fault {fault} atomicity"
        );
        assert!(s.faulted());
        assert!(step(&mut s, &mut r, Tick { ce: false, ..t }).is_err());
        if fault == 8 {
            r.inject("release dut.stored_owner;");
        }
    }
    r.finish(path);
}
#[test]
fn packet_storage_fault_preflight_atomicity() {
    faults(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sampling-packet-storage-20261008/faults"),
    );
}
#[test]
#[ignore = "requires Icarus and native Gowin memory primitives"]
fn packet_storage_fault_native_replay() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sampling-packet-storage-20261008/faults-rtl");
    faults(&p);
    run(&p);
}
