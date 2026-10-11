//! Independent RAM golden from actual accepted foundation accesses.
use gpu_v2::system::pixel::{
    foundation::{self, Beat, Step, Store},
    row_store::FOUNDATION_SHAPES,
    LightWrite, SampleWrite,
};
use std::{
    collections::VecDeque,
    fmt::Write,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct Recorder {
    ram: [Vec<u32>; 3],
    written: [Vec<bool>; 3],
    pending: [Option<(usize, u32, u64)>; 3],
    wall: u64,
    tb: String,
    csv: String,
    origin_csv: String,
    origin_ram: [Option<u16>; 32],
    origin_heads: VecDeque<u16>,
    origin_previous: Option<(usize, u16, u64)>,
    origin_written: usize,
    origin_requested: usize,
    origin_consumed: usize,
}

pub struct OutputEdge {
    pub ce: bool,
    pub ready: bool,
    pub data: Option<u32>,
    pub expected: Option<(u32, u8)>,
}

impl Recorder {
    pub fn new() -> Self {
        let mut tb = String::from("module tb; reg clk=0,reset=1;\n");
        for (name, shape) in ["basic", "light", "sample"]
            .into_iter()
            .zip(FOUNDATION_SHAPES)
        {
            writeln!(tb, "reg {name}_read_en=0,{name}_write_en=0;reg [{}:0] {name}_read_address=0,{name}_write_address=0;reg [{}:0] {name}_write_data=0;wire {name}_return_valid,{name}_collision;wire [{}:0] {name}_return_data;",
                shape.rows.ilog2()-1, shape.width-1, shape.width-1).unwrap();
        }
        tb.push_str("reg q_ce=0,q_ready=0,q_valid=0;reg [31:0] q_data=0;wire q_in_ready,q_out_valid,q_last,q_published,q_read;wire [31:0] q_out_data;wire [3:0] q_row;wire [9:0] q_address;\n");
        tb.push_str("gpu_v2_published_spsc output_store(.clk(clk),.reset(reset),.ce(q_ce),.in_valid(q_valid),.in_data(q_data),.in_ready(q_in_ready),.out_ready(q_ready),.out_valid(q_out_valid),.out_data(q_out_data),.out_row(q_row),.out_last(q_last),.published(q_published),.read_request(q_read),.read_address(q_address));\n");
        tb.push_str("reg o_write=0,o_read=0,o_retire=0;reg[4:0]o_wa=0,o_ra=0,o_current=0;reg[14:0]o_data=0;wire o_ready,o_valid,o_collision;wire[14:0]o_output;gpu_v2_pixel_origin origin_store(.clk(clk),.reset(reset),.ce(q_ce),.write_valid(o_write),.read_valid(o_read),.retire(o_retire),.write_address(o_wa),.read_address(o_ra),.current_address(o_current),.write_data(o_data),.read_ready(o_ready),.origin_valid(o_valid),.collision(o_collision),.origin_data(o_output));\n");
        tb.push_str("gpu_v2_pixel_foundation_banks dut(.*);initial begin #300000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n");
        Self {
            ram: FOUNDATION_SHAPES.map(|s| vec![0; s.rows]),
            written: FOUNDATION_SHAPES.map(|s| vec![false; s.rows]),
            pending: [None; 3],
            wall: 0,
            tb,
            csv: String::from("wall,bank,read,write,data,return_address,return_data\n"),
            origin_csv: String::from("wall,ce,source_write,source_data,read,return_address,return_data,current_slot,current_data,reserved,rop_row,rop_accepted,retire\n"),
            origin_ram: [None; 32],
            origin_heads: VecDeque::new(),
            origin_previous: None,
            origin_written: 0,
            origin_requested: 0,
            origin_consumed: 0,
        }
    }

    pub fn edge(
        &mut self,
        step: &Step,
        input: Option<Beat>,
        light: Option<LightWrite>,
        sample: Option<SampleWrite>,
        output: OutputEdge,
    ) {
        self.wall += 1;
        writeln!(
            self.tb,
            "q_ce={};q_ready={};q_valid={};q_data={};#1;",
            u8::from(output.ce),
            u8::from(output.ready),
            u8::from(output.data.is_some()),
            output.data.unwrap_or(0)
        )
        .unwrap();
        writeln!(
            self.tb,
            "if(q_out_valid!=={}) $fatal(1,\"output valid edge {}\");",
            u8::from(output.expected.is_some()),
            self.wall
        )
        .unwrap();
        if let Some((data, row)) = output.expected {
            writeln!(self.tb, "if(q_out_data!==32'd{data} || q_row!==4'd{row} || q_last!=={}) $fatal(1,\"output data/row edge {}\");",
                u8::from(row==7), self.wall).unwrap();
        }
        if output.data.is_some() {
            self.tb
                .push_str("if(q_in_ready!==1) $fatal(1,\"output borrowed credit\");\n");
        }
        let origin_read = step
            .accesses
            .iter()
            .find(|a| a.store == Store::Origin && !a.write)
            .map(|a| a.address);
        let origin_write = step
            .accesses
            .iter()
            .find(|a| a.store == Store::Origin && a.write)
            .map(|a| a.address);
        let origin_data = origin_write.map_or(0, |_| {
            assert!(step.input_accepted);
            let h = input.unwrap().begin.unwrap().header;
            assert!(h.mask != 0);
            (h.x / 2) | (u16::from(h.y / 2) << 8)
        });
        let read_value =
            origin_read.map(|a| self.origin_ram[a].expect("origin read before source write"));
        assert_eq!(
            origin_read,
            step.accesses
                .iter()
                .find(|a| a.store == Store::Output && !a.write && a.address % 8 == 0)
                .map(|a| a.address / 8)
        );
        let actual_return = step.returns.iter().find(|r| r.store == Store::Origin);
        assert_eq!(actual_return.is_some(), self.origin_previous.is_some());
        if let Some((address, _, issued)) = self.origin_previous {
            let r = actual_return.unwrap();
            assert_eq!(
                (r.address, r.issued, r.captured),
                (address, issued, self.wall)
            );
        }
        writeln!(
            self.tb,
            "o_write={};o_read={};o_retire={};o_wa={};o_ra={};o_current={};o_data={};#1;",
            u8::from(origin_write.is_some()),
            u8::from(origin_read.is_some()),
            u8::from(step.retired.is_some()),
            origin_write.unwrap_or(0),
            origin_read.unwrap_or(0),
            self.origin_consumed % 32,
            origin_data
        )
        .unwrap();
        writeln!(
            self.tb,
            "if(o_collision!==0||o_valid!=={}) $fatal(1,\"origin valid edge{}\");",
            u8::from(!self.origin_heads.is_empty()),
            self.wall
        )
        .unwrap();
        if let Some(&expected) = self.origin_heads.front() {
            writeln!(
                self.tb,
                "if(o_output!==15'd{expected}) $fatal(1,\"origin held data edge{}\");",
                self.wall
            )
            .unwrap();
            if let Some(rop) = step.signals.rop {
                assert_eq!(
                    (rop.header.x / 2) | (u16::from(rop.header.y / 2) << 8),
                    expected,
                    "origin ROP/header mismatch"
                );
                assert_eq!(usize::from(rop.ticket.quad), self.origin_consumed % 32);
            }
        } else {
            assert!(step.signals.rop.is_none());
        }
        if origin_read.is_some() {
            assert!(
                self.origin_heads.len() < 2,
                "same-edge origin reservation credit borrowed"
            );
            self.tb
                .push_str("if(o_ready!==1) $fatal(1,\"origin pending credit\");\n");
        }
        writeln!(
            self.csv,
            "{},origin,{},{},{origin_data},{},{}",
            self.wall,
            origin_read.map_or(-1, |a| a as i64),
            origin_write.map_or(-1, |a| a as i64),
            self.origin_previous.map_or(-1, |r| r.0 as i64),
            self.origin_previous.map_or(0, |r| r.1)
        )
        .unwrap();
        writeln!(
            self.origin_csv,
            "{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.wall,
            u8::from(output.ce),
            origin_write.map_or(-1, |a| a as i64),
            origin_data,
            origin_read.map_or(-1, |a| a as i64),
            self.origin_previous.map_or(-1, |r| r.0 as i64),
            self.origin_previous.map_or(0, |r| r.1),
            self.origin_consumed % 32,
            self.origin_heads.front().map_or(-1, |d| i64::from(*d)),
            self.origin_heads.len(),
            step.signals.rop.map_or(-1, |r| i64::from(r.row)),
            u8::from(output.ce && output.ready && step.signals.rop.is_some()),
            u8::from(step.retired.is_some())
        )
        .unwrap();
        if step.retired.is_some() {
            self.origin_heads
                .pop_front()
                .expect("origin retire without owner");
            self.origin_consumed += 1;
        }
        self.origin_previous = origin_read.map(|a| (a, read_value.unwrap(), self.wall));
        if let Some(address) = origin_read {
            assert_eq!(address, self.origin_requested % 32);
            self.origin_requested += 1;
            self.origin_heads.push_back(read_value.unwrap());
        }
        if let Some(address) = origin_write {
            assert_eq!(address, self.origin_written % 32);
            self.origin_written += 1;
            self.origin_ram[address] = Some(origin_data);
        }
        for (i, (name, store)) in ["basic", "light", "sample"]
            .into_iter()
            .zip([Store::Basic, Store::Light, Store::Sample])
            .enumerate()
        {
            let reads: Vec<_> = step
                .accesses
                .iter()
                .filter(|a| a.store == store && !a.write)
                .collect();
            let writes: Vec<_> = step
                .accesses
                .iter()
                .filter(|a| a.store == store && a.write)
                .collect();
            assert!(reads.len() <= 1 && writes.len() <= 1);
            let read = reads.first().map(|a| a.address);
            let write = writes.first().map(|a| a.address);
            assert!(read.is_none() || read != write, "physical collision {name}");
            let data = if write.is_some() {
                match store {
                    Store::Basic => {
                        assert!(step.input_accepted);
                        input.unwrap().basic
                    }
                    Store::Light => {
                        let v = light.unwrap().value;
                        u32::from(v.g) | (u32::from(v.h) << 9)
                    }
                    Store::Sample => {
                        let c = sample.unwrap().rgb;
                        u32::from_le_bytes([c[0], c[1], c[2], 0])
                    }
                    _ => unreachable!(),
                }
            } else {
                0
            };
            assert!(data < 1 << FOUNDATION_SHAPES[i].width);
            let actual_return = step.returns.iter().find(|r| r.store == store);
            assert_eq!(actual_return.is_some(), self.pending[i].is_some());
            if let Some((address, _, issued)) = self.pending[i] {
                let r = actual_return.unwrap();
                assert_eq!(
                    (r.address, r.issued, r.captured),
                    (address, issued, self.wall)
                );
            }
            writeln!(self.tb, "{name}_read_en={};{name}_read_address={};{name}_write_en={};{name}_write_address={};{name}_write_data={};",
                u8::from(read.is_some()), read.unwrap_or(0), u8::from(write.is_some()), write.unwrap_or(0), data).unwrap();
            writeln!(self.tb, "if({name}_collision!==0 || {name}_return_valid!=={}) $fatal(1,\"{name} valid edge {}\");",
                u8::from(self.pending[i].is_some()), self.wall).unwrap();
            if let Some((_, value, _)) = self.pending[i] {
                writeln!(
                    self.tb,
                    "if({name}_return_data!=={value}) $fatal(1,\"{name} data edge {}\");",
                    self.wall
                )
                .unwrap();
            }
            writeln!(
                self.csv,
                "{},{name},{},{},{data},{},{}",
                self.wall,
                read.map_or(-1, |r| r as i64),
                write.map_or(-1, |r| r as i64),
                self.pending[i].map_or(-1, |r| r.0 as i64),
                self.pending[i].map_or(0, |r| r.1)
            )
            .unwrap();
            self.pending[i] = read.map(|address| {
                assert!(
                    self.written[i][address],
                    "read before actual write {name} row {address}"
                );
                (address, self.ram[i][address], self.wall)
            });
            if let Some(address) = write {
                self.ram[i][address] = data;
                self.written[i][address] = true;
            }
        }
        // Compare old DO before the next edge, including compute CE-off edges.
        // Let address/CE combinational wiring settle before the active edge.
        self.tb.push_str("#1;clk=1;#1;clk=0;#1;\n");
    }

    pub fn finish(mut self, name: &str) {
        assert!(self.pending.iter().all(Option::is_none));
        assert!(self.origin_heads.is_empty() && self.origin_previous.is_none());
        writeln!(
            self.tb,
            "$display(\"PASS {} actual controller edges\");$finish;end endmodule",
            self.wall
        )
        .unwrap();
        let dir = directory(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("banks.v"), foundation::storage_verilog().unwrap()).unwrap();
        fs::write(dir.join("tb.v"), self.tb).unwrap();
        fs::write(dir.join("ports.csv"), self.csv).unwrap();
        fs::write(dir.join("origin-ports.csv"), self.origin_csv).unwrap();
    }
}

fn directory(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/quad-ram-lowering-20261008")
        .join(name)
}

pub fn replay(name: &str) {
    let dir = directory(name);
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    let home = std::env::var_os("GOWIN_HOME").expect("native bank replay requires GOWIN_HOME");
    compile
        .current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp", "banks.v", "tb.v"])
        .arg(Path::new(&home).join("IDE/simlib/gw2a/prim_sim.v"));
    bounded(compile, &dir, "compile");
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    run.current_dir(&dir).arg("test.vvp");
    bounded(run, &dir, "run");
    assert!(fs::read_to_string(dir.join("run.out"))
        .unwrap()
        .contains("actual controller edges"));
}

fn bounded(mut command: Command, dir: &Path, name: &str) {
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
            panic!("native banks watchdog");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
