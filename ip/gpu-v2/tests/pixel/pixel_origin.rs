use gpu_v2::system::pixel::origin::{self, Backend, Model, Tick};
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn aligned_origin_roundtrip_is_bijective_over_the_complete_domain() {
    for y in 0..128u8 {
        for x in 0..256u16 {
            let packed = origin::pack(x * 2, y * 2).unwrap();
            assert_eq!(packed, x | (u16::from(y) << 8));
            assert_eq!(origin::unpack(packed), (x * 2, y * 2));
        }
    }
    for (x, y) in [(511, 0), (512, 0), (0, 255), (0, 1), (1, 0)] {
        assert!(origin::pack(x, y).is_err());
    }
}

struct Edge {
    tick: Tick,
    current: u8,
    output: Option<u16>,
    read_ready: bool,
}

fn trace() -> Vec<Edge> {
    let mut model = Model::new(30_000);
    let mut rng = 0x6417056d8f31ae49u64;
    let mut inserted = 0usize;
    let mut published = 0usize;
    let mut issued = 0usize;
    let mut consumed = 0usize;
    let mut source_row = 0usize;
    let mut rop_row = 0usize;
    let mut reserved = VecDeque::<u16>::new();
    let mut values = [0u16; 32];
    let mut salt = 0usize;
    let mut edges = Vec::new();
    let mut max_used = 0;
    let mut next_held = 0;
    let mut paused_returns = 0;
    let mut old_read = None;
    let mut held = None;
    let mut row7_stalls = 0;
    for wall in 0..30_000 {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let reset = wall == 137 || wall == 819;
        let ce = rng & 7 != 0 && !(210..250).contains(&wall);
        let output = if reset {
            None
        } else {
            reserved.front().copied()
        };
        let ready = wall > 400 && rng >> 8 & 7 > 2 && !(rop_row == 7 && wall % 71 < 19);
        let retire = ce && !reset && ready && output.is_some() && rop_row == 7;
        let read = (issued < published && model.read_ready(ce) && reserved.len() < 2 && !reset)
            .then_some((issued % 32) as u8);
        let write = (ce && !reset && source_row == 0 && inserted < 220 && inserted - consumed < 32)
            .then_some((
                (inserted % 32) as u8,
                ((inserted * 977 + salt * 6191 + 0x1234) & 0x7fff) as u16,
            ));
        let tick = Tick {
            reset,
            ce,
            write,
            read,
            retire,
        };
        let current = (consumed % 32) as u8;
        let read_ready = !reset && model.read_ready(ce);
        let step = model.tick(tick).unwrap();
        assert_eq!(step.output, output, "origin ordering wall{wall}");
        if let Some(previous) = held {
            if !reset {
                assert_eq!(output, Some(previous), "held row changed");
            }
        }
        assert_eq!(step.returned.map(|(a, _, _)| a), old_read);
        if step.returned.is_some() && !ce {
            paused_returns += 1;
        }
        old_read = step.read;
        edges.push(Edge {
            tick,
            current,
            output,
            read_ready,
        });
        if reset {
            inserted = 0;
            published = 0;
            issued = 0;
            consumed = 0;
            source_row = 0;
            rop_row = 0;
            reserved.clear();
            held = None;
            old_read = None;
            salt += 1;
        } else {
            if output.is_some() && rop_row == 7 && !retire {
                row7_stalls += 1;
            }
            if ce && ready && output.is_some() {
                assert_eq!(output, Some(values[consumed % 32]));
                rop_row += 1;
                if retire {
                    reserved.pop_front();
                    consumed += 1;
                    rop_row = 0;
                }
            }
            held = output.filter(|_| !(ce && ready));
            // An entry's origin lifetime includes all eight accepted ROP rows.
            if ce {
                if let Some((address, data)) = step.write {
                    values[usize::from(address)] = data;
                    inserted += 1;
                    source_row = 1;
                } else if source_row != 0 {
                    source_row += 1;
                    if source_row == 8 {
                        source_row = 0;
                        published += 1;
                    }
                }
            }
            if let Some(address) = step.read {
                assert_eq!(usize::from(address), issued % 32);
                reserved.push_back(values[usize::from(address)]);
                issued += 1;
            }
            max_used = max_used.max(inserted - consumed);
            if reserved.len() == 2 && !retire {
                next_held += 1;
            }
            assert!(reserved.len() <= 2);
            if inserted == 220 && model.idle() && consumed == 220 {
                assert_eq!(max_used, 32);
                assert!(next_held > 20 && row7_stalls > 20 && paused_returns > 0);
                return edges;
            }
        }
    }
    panic!("origin trace watchdog");
}

#[test]
fn full_wrap_ce_reset_and_stalled_row7_preserve_current_and_next() {
    trace();
}

#[test]
fn pending_credit_and_same_address_collision_are_not_borrowed() {
    let mut m = Model::new(32);
    m.tick(Tick {
        ce: true,
        write: Some((0, 123)),
        ..Default::default()
    })
    .unwrap();
    m.tick(Tick {
        ce: true,
        write: Some((1, 456)),
        ..Default::default()
    })
    .unwrap();
    m.tick(Tick {
        ce: true,
        read: Some(0),
        ..Default::default()
    })
    .unwrap();
    assert!(!m.read_ready(true));
    assert!(m
        .tick(Tick {
            ce: true,
            read: Some(1),
            ..Default::default()
        })
        .is_err());
    assert_eq!(m.output(), Some(123));
    m.tick(Tick {
        ce: true,
        ..Default::default()
    })
    .unwrap();
    assert!(m
        .tick(Tick {
            ce: true,
            read: Some(1),
            write: Some((1, 777)),
            ..Default::default()
        })
        .is_err());
    m.tick(Tick {
        ce: true,
        read: Some(1),
        ..Default::default()
    })
    .unwrap();
    m.tick(Tick {
        ce: true,
        retire: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(m.output(), Some(456));
}

fn bounded(mut cmd: Command, dir: &Path, name: &str) {
    let mut child = cmd
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let started = Instant::now();
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
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("origin RTL wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME"]
fn origin_native_distributed_and_old_async_match_every_pre_edge() {
    let edges = trace();
    for backend in [
        Backend::Native,
        Backend::Distributed,
        Backend::AsyncReference,
    ] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/quad-sequential-20261009/origin/{backend:?}"
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("origin.v"), origin::verilog(backend)).unwrap();
        let mut tb=String::from("module tb;GSR GSR(.GSRI(1'b1));reg clk=0,reset=1,ce=0,write_valid=0,read_valid=0,retire=0;reg[4:0]write_address=0,read_address=0,current_address=0;reg[14:0]write_data=0;wire read_ready,origin_valid,collision;wire[14:0]origin_data;gpu_v2_pixel_origin dut(.*);initial begin #300000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n");
        for (i, e) in edges.iter().enumerate() {
            let t = e.tick;
            tb.push_str(&format!("reset={};ce={};write_valid={};read_valid={};retire={};write_address=5'd{};read_address=5'd{};current_address=5'd{};write_data=15'd{};#1;\n",u8::from(t.reset),u8::from(t.ce),u8::from(t.write.is_some()),u8::from(t.read.is_some()),u8::from(t.retire),t.write.map_or(0,|w|w.0),t.read.unwrap_or(0),e.current,t.write.map_or(0,|w|w.1)));
            tb.push_str(&format!(
                "if(origin_valid!==1'b{}||collision!==1'b0) $fatal(1,\"valid edge{i}\");\n",
                u8::from(e.output.is_some())
            ));
            if let Some(data) = e.output {
                tb.push_str(&format!(
                    "if(origin_data!==15'd{data}) $fatal(1,\"data edge{i}\");\n"
                ));
            }
            tb.push_str(&format!(
                "if(read_ready!==1'b{}) $fatal(1,\"credit edge{i}\");\n",
                u8::from(e.read_ready)
            ));
            tb.push_str("clk=1;#1;clk=0;#1;\n");
        }
        // Deliberately offer illegal traffic outside the accepted-read trace:
        // a collision must preserve RAM, and busy DO cannot borrow promotion.
        tb.push_str(r#"
reset=1;ce=1;write_valid=0;read_valid=0;retire=0;#1;clk=1;#1;clk=0;#1;reset=0;
write_valid=1;write_address=0;write_data=317;#1;clk=1;#1;clk=0;#1;
write_address=1;write_data=829;#1;clk=1;#1;clk=0;#1;
write_address=0;write_data=999;read_valid=1;read_address=0;current_address=0;#1;
if(collision!==1) $fatal(1,"missing collision guard");clk=1;#1;clk=0;#1;
write_valid=0;#1;if(collision!==0||read_ready!==1) $fatal(1,"collision recovery");clk=1;#1;clk=0;#1;
ce=0;read_address=1;#1;if(read_ready!==0||origin_valid!==1||origin_data!==317) $fatal(1,"CE-off DO");clk=1;#1;clk=0;#1;
ce=1;#1;if(read_ready!==0) $fatal(1,"borrowed promotion credit");clk=1;#1;clk=0;#1;
if(origin_data!==317||read_ready!==1) $fatal(1,"blocked read corrupted current");clk=1;#1;clk=0;#1;
read_valid=0;retire=1;#1;if(origin_data!==317||read_ready!==0) $fatal(1,"next overwrote current");clk=1;#1;clk=0;#1;
retire=0;current_address=1;#1;if(origin_data!==829||origin_valid!==1) $fatal(1,"next promotion");
reset=1;ce=0;#1;if(origin_valid!==0||read_ready!==0) $fatal(1,"reset gate");clk=1;#1;clk=0;#1;
reset=0;ce=1;read_valid=1;read_address=0;current_address=0;#1;clk=1;#1;clk=0;#1;
read_valid=0;#1;if(origin_valid!==1||origin_data!==317) $fatal(1,"reset or collision destroyed RAM");
"#);
        tb.push_str(&format!(
            "$display(\"PASS origin edges={}\");$finish;end endmodule\n",
            edges.len()
        ));
        fs::write(dir.join("tb.v"), tb).unwrap();
        let mut compile = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        compile
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "test.vvp", "origin.v", "tb.v"])
            .arg(
                Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        bounded(compile, &dir, "compile");
        let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap());
        run.current_dir(&dir).arg("test.vvp");
        bounded(run, &dir, "run");
        println!(
            "{backend:?}: {}",
            fs::read_to_string(dir.join("run.out")).unwrap()
        );
    }
}
