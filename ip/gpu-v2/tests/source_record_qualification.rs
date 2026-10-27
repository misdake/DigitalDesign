use gpu_v2::frontend::source_record::{self, Inputs, Output, Read, RecordEmu, WORD_MASK};
use std::collections::VecDeque;
use std::fmt::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_EDGES: usize = 40000;
struct Driver {
    emu: RecordEmu,
    complete: VecDeque<Vec<u64>>,
    prefix: Vec<u64>,
    paid: Option<(u8, u8, u64)>,
    allocate: usize,
    retire: usize,
    edges: usize,
    trace: String,
    ce0: usize,
    holds: usize,
    partial_reads: usize,
    reads: usize,
    writes: usize,
    publications: usize,
    releases: usize,
}
impl Driver {
    fn new() -> Self {
        Self {
            emu: RecordEmu::default(),
            complete: VecDeque::new(),
            prefix: Vec::new(),
            paid: None,
            allocate: 0,
            retire: 0,
            edges: 0,
            trace: String::new(),
            ce0: 0,
            holds: 0,
            partial_reads: 0,
            reads: 0,
            writes: 0,
            publications: 0,
            releases: 0,
        }
    }
    fn check(&self, i: Inputs) -> Output {
        let o = self.emu.output(i);
        assert_eq!(usize::from(o.used), self.complete.len());
        assert_eq!(usize::from(o.producer_slot), self.allocate & 15);
        assert_eq!(usize::from(o.write_row), self.prefix.len());
        assert_eq!(o.return_valid, self.paid.is_some());
        if let Some((slot, row, data)) = self.paid {
            assert_eq!(
                (o.return_slot, o.return_row, o.return_data),
                (slot, row, data)
            );
        }
        o
    }
    fn step(&mut self, i: Inputs) -> Output {
        assert!(self.edges < MAX_EDGES);
        let before = self.check(i);
        let advance = i.ce && !i.reset && !before.fault;
        let next_paid = if before.read_issued {
            let q = i.read.unwrap();
            let (slot, words) = if q.prepare {
                self.partial_reads += 1;
                (self.allocate, &self.prefix)
            } else {
                (self.retire, self.complete.front().unwrap())
            };
            Some(((slot & 15) as u8, q.row, words[usize::from(q.row)]))
        } else if advance && i.return_ready {
            None
        } else {
            self.paid
        };
        let actual = self.emu.tick(i);
        assert_eq!(actual.packed(), before.packed());
        if i.reset {
            self.complete.clear();
            self.prefix.clear();
            self.paid = None;
            self.allocate = 0;
            self.retire = 0;
        } else {
            self.paid = next_paid;
            if actual.written {
                self.writes += 1;
                self.prefix.push(i.write.unwrap());
            }
            if actual.published {
                self.publications += 1;
                assert_eq!(self.prefix.len(), 32);
                self.complete.push_back(std::mem::take(&mut self.prefix));
                self.allocate += 1;
            }
            if actual.released {
                self.releases += 1;
                self.complete.pop_front().unwrap();
                self.retire += 1;
            }
            self.reads += usize::from(actual.read_issued);
        }
        self.ce0 += usize::from(!i.ce && !i.reset);
        self.holds += usize::from(before.return_valid && !i.return_ready && !i.reset);
        let after = self.check(i);
        let flags = u8::from(actual.written)
            | u8::from(actual.read_issued) << 1
            | u8::from(actual.published) << 2
            | u8::from(actual.released) << 3;
        writeln!(
            self.trace,
            "{} {} {} {} {:09x} {} {} {} {} {} {:016x} {:016x} {:x}",
            u8::from(i.reset),
            u8::from(i.ce),
            u8::from(i.abort),
            u8::from(i.write.is_some()),
            i.write.unwrap_or(0),
            u8::from(i.read.is_some()),
            u8::from(i.read.is_some_and(|q| q.prepare)),
            i.read.map_or(0, |q| q.row),
            u8::from(i.return_ready),
            u8::from(i.release),
            before.packed(),
            after.packed(),
            flags
        )
        .unwrap();
        self.edges += 1;
        actual
    }
    fn active(&mut self, i: Inputs) -> Output {
        self.step(Inputs {
            ce: true,
            return_ready: true,
            ..i
        })
    }
    fn fill(&mut self, serial: usize) {
        assert!(self.prefix.is_empty() && self.complete.len() < 16);
        for row in 0u8..32 {
            let data = ((serial as u64 * 0x1456327b) ^ (row as u64 * 0x198765c9)) & WORD_MASK;
            let out = self.active(Inputs {
                write: Some(data),
                read: (row > 0).then_some(Read {
                    prepare: true,
                    row: row.saturating_sub(1),
                }),
                ..Inputs::default()
            });
            assert!(out.written);
            assert_eq!(out.published, row == 31);
        }
    }
    fn consume(&mut self) {
        assert!(!self.complete.is_empty());
        for row in 0..32 {
            assert!(
                self.active(Inputs {
                    read: Some(Read {
                        prepare: false,
                        row
                    }),
                    release: true,
                    ..Inputs::default()
                })
                .read_issued
            );
        }
        // Paid oldest read must be acknowledged before its source is released.
        assert!(
            !self
                .active(Inputs {
                    release: true,
                    ..Inputs::default()
                })
                .released
        );
        assert!(
            self.active(Inputs {
                release: true,
                ..Inputs::default()
            })
            .released
        );
    }
}

fn bounded_process(mut command: Command, log: &Path) {
    let file = std::fs::File::create(log).unwrap();
    command.stdout(Stdio::from(file.try_clone().unwrap()));
    command.stderr(Stdio::from(file));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{}",
                std::fs::read_to_string(log).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            panic!("bounded HDL process watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn rtl(dir: &Path, d: &Driver, native: bool) {
    let name = if native { "native" } else { "portable" };
    let out = dir.join(name);
    std::fs::create_dir_all(&out).unwrap();
    let source = if native {
        source_record::native_rtl()
    } else {
        source_record::portable_rtl()
    };
    std::fs::write(out.join("dut.v"), source).unwrap();
    std::fs::write(out.join("trace.txt"), &d.trace).unwrap();
    let tb = r#"module tb;
reg clk=0,reset=1,ce=0,abort=0,write_valid=0,read_valid=0,read_prepare=0,return_ready=0,release_valid=0;
reg [35:0]write_data=0;reg[4:0]read_row=0;
wire write_ready,read_ready,release_ready,return_valid,published,released,fault;
wire[4:0]write_row,used,return_row;wire[3:0]producer_slot,return_slot;wire[35:0]return_data;
gpu_v2_source_record dut(.*);
wire[63:0]contract={return_valid?return_data:36'd0,return_row,return_slot,return_valid,fault,release_ready,read_ready,write_row,producer_slot,used,write_ready};
integer fd,n,k,cycles=0;integer ir,ic,ia,iw,iq,ip,row,io,il;reg[63:0]pre,post;reg[3:0]flags,actual;
initial begin #1000000;$fatal(1,"finite HDL global watchdog");end
initial begin
 clk=1;#1;clk=0;#1;
 fd=$fopen("trace.txt","r");
 while(!$feof(fd))begin
  n=$fscanf(fd,"%d %d %d %d %h %d %d %d %d %d %h %h %h\n",ir,ic,ia,iw,write_data,iq,ip,row,io,il,pre,post,flags);
  if(n!=13)$fatal(1,"malformed finite trace");
  reset=ir;ce=ic;abort=ia;write_valid=iw;read_valid=iq;read_prepare=ip;read_row=row;return_ready=io;release_valid=il;#2;
  if(contract!==pre)$fatal(1,"preedge owner/payload mismatch edge=%0d got=%h expected=%h",cycles,contract,pre);
  actual={released,published,ce&&read_valid&&read_ready,ce&&write_valid&&write_ready};
  if(actual!==flags)$fatal(1,"old paid action mismatch edge=%0d",cycles);
  if(dut.write_fire&&dut.read_fire&&dut.write_address==dut.read_address)$fatal(1,"same row collision");
  clk=1;#2;
  if(contract!==post)$fatal(1,"postedge owner/payload mismatch edge=%0d got=%h expected=%h",cycles,contract,post);
  clk=0;#1;cycles=cycles+1;
  if(cycles>40000)$fatal(1,"trace edge bound");
 end
 $display("PASS actual source-record RTL edges=%0d",cycles);$finish;
end
endmodule
"#;
    std::fs::write(out.join("tb.v"), tb).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&out)
        .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
    if native {
        let home = std::env::var_os("GOWIN_HOME")
            .expect("GOWIN_HOME is required for native primitive simulation");
        compile.arg(std::path::PathBuf::from(home).join("IDE/simlib/gw2a/prim_sim.v"));
    }
    compile.args(["dut.v", "tb.v"]);
    bounded_process(compile, &out.join("compile.log"));
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    run.current_dir(&out).arg("sim.vvp");
    bounded_process(run, &out.join("run.log"));
}

#[test]
fn source_record_qualified_ownership_native_portable() {
    let mut d = Driver::new();
    d.step(Inputs {
        reset: true,
        ..Inputs::default()
    });
    // Neither unpublished empty RAM nor an unwritten producer prefix is readable.
    assert!(
        !d.active(Inputs {
            read: Some(Read {
                prepare: false,
                row: 0
            }),
            ..Inputs::default()
        })
        .read_issued
    );
    assert!(
        !d.active(Inputs {
            read: Some(Read {
                prepare: true,
                row: 0
            }),
            ..Inputs::default()
        })
        .read_issued
    );
    for serial in 0..16 {
        d.fill(serial);
    }
    assert_eq!(d.complete.len(), 16);
    let old_full = d.step(Inputs {
        ce: true,
        write: Some(42),
        return_ready: false,
        release: true,
        ..Inputs::default()
    });
    assert!(!old_full.written && old_full.released);
    // Hold a real paid producer return while writing a different free slot.
    let held = d.emu.regs;
    for _ in 0..5 {
        d.step(Inputs {
            ce: false,
            write: Some(55),
            release: true,
            ..Inputs::default()
        });
    }
    assert_eq!(d.emu.regs, held);
    d.consume();
    d.fill(900);
    for _ in 0..15 {
        d.consume();
    }
    d.fill(901);
    d.consume();
    // Both physical16-slot and logical32-pointer wrap, simultaneous prefix read/write.
    for serial in 1000..1064 {
        d.fill(serial);
        d.consume();
    }
    // Random producer/oldest reading, return hold, sparse CE, and queue pressure.
    let mut seed = 0x9371u64;
    for edge in 0..12000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let q = if seed & 4 != 0 {
            Some(Read {
                prepare: seed & 8 != 0,
                row: ((seed >> 8) & 31) as u8,
            })
        } else {
            None
        };
        d.step(Inputs {
            ce: seed & 1 != 0,
            write: (seed & 2 != 0).then_some((seed ^ (edge as u64) << 12) & WORD_MASK),
            read: q,
            return_ready: seed & 16 != 0,
            release: seed & 32 != 0,
            ..Inputs::default()
        });
    }
    // Fault during a paid return; no new issue, consumption or release escapes.
    if d.complete.is_empty() {
        while !d.prefix.is_empty() {
            d.active(Inputs {
                write: Some(111),
                ..Inputs::default()
            });
        }
        if d.complete.is_empty() {
            d.fill(3000);
        }
    }
    d.active(Inputs {
        read: Some(Read {
            prepare: false,
            row: 7,
        }),
        ..Inputs::default()
    });
    assert!(d.paid.is_some());
    d.step(Inputs {
        abort: true,
        ce: false,
        return_ready: true,
        release: true,
        write: Some(77),
        ..Inputs::default()
    });
    let stopped = d.emu.regs;
    for _ in 0..16 {
        d.step(Inputs {
            ce: true,
            read: Some(Read {
                prepare: false,
                row: 8,
            }),
            return_ready: true,
            release: true,
            write: Some(88),
            ..Inputs::default()
        });
        assert_eq!(d.emu.regs, stopped);
    }
    d.step(Inputs {
        reset: true,
        ce: false,
        ..Inputs::default()
    });
    // Slot reuse after reset writes new authority; old payload array is not cleared.
    for serial in 4000..4003 {
        d.fill(serial);
    }
    for _ in 0..3 {
        d.consume();
    }
    d.active(Inputs {
        write: Some(0x0f12345678),
        ..Inputs::default()
    });
    assert_eq!(d.prefix.len(), 1);
    d.step(Inputs {
        reset: true,
        ..Inputs::default()
    });
    d.fill(5000);
    d.consume();
    assert!(
        d.edges < MAX_EDGES
            && d.ce0 > 1000
            && d.holds > 1000
            && d.partial_reads > 1000
            && d.publications > 80
            && d.releases > 80
    );
    let dir = std::env::var_os("SOURCE_RECORD_QUAL_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or("target/source-record-qualification".into());
    std::fs::create_dir_all(&dir).unwrap();
    rtl(&dir, &d, false);
    rtl(&dir, &d, true);
    let receipt=format!("{{\"status\":\"PASS\",\"edges\":{},\"writes\":{},\"reads\":{},\"prefix_reads\":{},\"publications\":{},\"releases\":{},\"CE0\":{},\"return_holds\":{},\"max_edges\":{MAX_EDGES},\"watchdog_seconds\":60}}\n",d.edges,d.writes,d.reads,d.partial_reads,d.publications,d.releases,d.ce0,d.holds);
    std::fs::write(dir.join("qualification.json"), &receipt).unwrap();
    println!("{receipt}");
}
