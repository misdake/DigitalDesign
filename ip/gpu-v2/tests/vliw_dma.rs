use gpu_v2::frontend::vliw_dma::*;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

fn descriptor(n: u8, bytes: u16) -> Descriptor {
    Descriptor {
        page: n & 1 != 0,
        tag: n,
        config: Segment {
            source_halfword: 0x100,
            destination_word: 300,
            bytes: if n.is_multiple_of(3) { 72 } else { 0 },
        },
        metadata_source_halfword: 0x180,
        metadata_destination_word: 256 + 4 * u16::from(n & 1),
        payload: Segment {
            source_halfword: 0x200,
            destination_word: 128 * u16::from(n & 1),
            bytes,
        },
    }
}
fn pattern(halfword: u32) -> u64 {
    0xfedc_ba98_7654_3210 ^ u64::from(halfword).wrapping_mul(0x1000_0001)
}
fn expected(d: Descriptor) -> BTreeMap<u16, u64> {
    let mut result = BTreeMap::new();
    for s in [d.config, d.metadata(), d.payload] {
        for n in 0..s.bytes / 8 {
            assert!(result
                .insert(
                    s.destination_word + n,
                    pattern(s.source_halfword + u32::from(n) * 4)
                )
                .is_none());
        }
    }
    result
}
#[derive(Clone, Copy, Debug)]
enum Fault {
    None,
    Memory(u8),
    Short,
    Late,
    AbortRequest,
    AbortResponse(u8),
    AbortSelect,
}
#[derive(Clone)]
struct Edge {
    input: Input,
    output: Output,
}
#[derive(Default)]
struct Stats {
    edges: Vec<Edge>,
    jobs: usize,
    requests: [usize; 4],
    writes: usize,
    paused_writes: usize,
    blocked: usize,
}
fn edge(l: &mut Loader, s: &mut Stats, i: Input) -> Output {
    let o = l.step(i);
    s.edges.push(Edge {
        input: i,
        output: o,
    });
    o
}

// Independent physical burst source: no response ready and no compute CE.
fn run_job(l: &mut Loader, s: &mut Stats, d: Descriptor, fault: Fault) {
    assert!(d.valid());
    let mut accepted = false;
    let mut active: Option<(Request, u8, usize)> = None;
    let mut held = None;
    let mut held_completion = None;
    let mut writes = BTreeMap::new();
    let mut rng = 0x84f3_a219u32 ^ u32::from(d.tag);
    let mut fault_used = false;
    let mut abort_seen = false;
    let mut request_total = 0;
    for wall in 0..20_000 {
        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        let mut i = Input {
            descriptor: (!accepted).then_some(d),
            page_writable: wall >= 3,
            gpu_drained: wall >= 7,
            request_ready: wall >= 20 && rng & 3 != 0,
            completion_ready: wall % 23 >= 17,
            ..Input::default()
        };
        let mut terminal = false;
        if let Some((r, n, wait)) = &mut active {
            if *wait != 0 {
                *wait -= 1;
            } else if rng & 7 != 0 {
                let mut response = Response {
                    data: pattern(r.address_halfword + u32::from(*n) * 4),
                    last: *n == r.beats() - 1,
                    error: false,
                };
                if !fault_used {
                    match fault {
                        Fault::Memory(at) if *n == at.min(r.beats() - 1) => {
                            response.error = true;
                            response.last = false;
                            fault_used = true;
                        }
                        Fault::Short if *n == 1 => {
                            response.last = true;
                            fault_used = true;
                        }
                        Fault::Late if *n == r.beats() - 1 => {
                            response.last = false;
                            fault_used = true;
                        }
                        Fault::AbortResponse(at) if *n == at.min(r.beats() - 1) => {
                            i.abort = true;
                            fault_used = true;
                        }
                        _ => {}
                    }
                } else if matches!(fault, Fault::Late) && *n == r.beats() {
                    response.last = true;
                }
                terminal = response.last || response.error;
                i.response = Some(response);
                *n += 1;
            }
        }
        let preview = l.output(&i);
        if !fault_used
            && matches!(fault, Fault::AbortRequest)
            && preview.request.is_some()
            && !i.request_ready
        {
            i.abort = true;
            fault_used = true;
        }
        if !fault_used
            && matches!(fault, Fault::AbortSelect)
            && accepted
            && request_total == 0
            && preview.request.is_none()
        {
            i.abort = true;
            fault_used = true;
        }
        abort_seen |= i.abort;
        let o = edge(l, s, i);
        if let Some(r) = held {
            assert_eq!(o.request, Some(r), "blocked descriptor changed on abort");
        }
        held = o.request.filter(|_| !i.request_ready);
        if let Some(c) = held_completion {
            assert_eq!(o.completion, Some(c));
        }
        held_completion = o.completion.filter(|_| !i.completion_ready);
        if o.descriptor_ready && i.descriptor.is_some() {
            accepted = true;
        }
        if terminal {
            active = None;
        }
        if let Some(r) = o.request.filter(|_| i.request_ready) {
            assert!(active.is_none());
            assert!(matches!(r.line_count_minus_1, 0 | 1 | 3));
            assert_eq!(r.address_halfword % (u32::from(r.beats()) * 4), 0);
            s.requests[r.line_count_minus_1 as usize] += 1;
            request_total += 1;
            active = Some((r, 0, 2));
        }
        if o.request.is_some() && !i.request_ready {
            s.blocked += 1;
        }
        if let Some((address, data)) = o.write {
            assert!(!abort_seen, "write after cancellation");
            assert!(
                writes.insert(address, data).is_none(),
                "duplicate local write"
            );
            s.writes += 1;
            if wall % 11 < 8 {
                s.paused_writes += 1;
            } // A deliberately stopped compute path.
        }
        if let Some(c) = o.completion.filter(|_| i.completion_ready) {
            assert!(
                active.is_none(),
                "completed while accepted transaction was live"
            );
            assert_eq!((c.page, c.tag), (d.page, d.tag));
            match fault {
                Fault::None => {
                    assert_eq!(c.error, 0);
                    assert_eq!(writes, expected(d));
                }
                Fault::Memory(_) => assert_eq!(c.error, ERROR_MEMORY),
                Fault::Short | Fault::Late => assert_eq!(c.error, ERROR_PROTOCOL),
                _ => assert_eq!(c.error, ERROR_ABORT),
            }
            assert_eq!(c.publish(), matches!(fault, Fault::None));
            s.jobs += 1;
            return;
        }
    }
    panic!("bounded loader job timeout: {:?}", fault);
}

fn synthetic() -> Stats {
    let mut l = Loader::default();
    let mut s = Stats::default();
    edge(
        &mut l,
        &mut s,
        Input {
            reset: true,
            ..Input::default()
        },
    );
    let lengths = [
        0, 8, 16, 24, 32, 40, 56, 64, 72, 96, 120, 128, 136, 248, 512, 1016, 1024,
    ];
    for n in 0..102 {
        run_job(
            &mut l,
            &mut s,
            descriptor(n as u8, lengths[n % lengths.len()]),
            Fault::None,
        );
    }
    for (n, fault) in [
        Fault::Memory(0),
        Fault::Memory(2),
        Fault::Memory(15),
        Fault::Short,
        Fault::Late,
        Fault::AbortRequest,
        Fault::AbortResponse(0),
        Fault::AbortResponse(3),
        Fault::AbortSelect,
    ]
    .into_iter()
    .enumerate()
    {
        let mut d = descriptor(n as u8 + 110, 1024);
        d.config.bytes = 0;
        run_job(&mut l, &mut s, d, fault);
    }
    // Explicit global transport reset while no request is owned; no page publication.
    edge(
        &mut l,
        &mut s,
        Input {
            reset: true,
            ..Input::default()
        },
    );
    assert!(s.requests[0] > 0 && s.requests[1] > 0 && s.requests[3] > 0 && s.requests[2] == 0);
    assert!(s.paused_writes > 100 && s.blocked > 100);
    s
}

#[test]
fn loader_static_segments_padding_abort_and_faults() {
    let s = synthetic();
    println!(
        "jobs={} edges={} requests={:?} writes={} writes_during_compute_pause={} blocked={}",
        s.jobs,
        s.edges.len(),
        s.requests,
        s.writes,
        s.paused_writes,
        s.blocked
    );
}

fn continuous() -> Stats {
    let mut l = Loader::default();
    let mut s = Stats::default();
    let mut d = descriptor(2, 128);
    d.config.bytes = 0;
    edge(
        &mut l,
        &mut s,
        Input {
            reset: true,
            ..Input::default()
        },
    );
    let mut accepted = false;
    let mut active: Option<(Request, u8)> = None;
    let mut held_completion = None;
    let mut completion_holds = 0;
    let mut writes = BTreeMap::new();
    let mut run = 0;
    let mut max_run = 0;
    for _ in 0..300 {
        let mut i = Input {
            descriptor: (!accepted).then_some(d),
            page_writable: true,
            request_ready: true,
            completion_ready: completion_holds >= 8,
            ..Input::default()
        };
        // No compute CE exists at this boundary; the consumer compute may stay
        // stopped throughout this entire sequence of consecutive write clocks.
        if let Some((r, n)) = active {
            i.response = Some(Response {
                data: pattern(r.address_halfword + u32::from(n) * 4),
                last: n == r.beats() - 1,
                error: false,
            });
            active = if n == r.beats() - 1 {
                None
            } else {
                Some((r, n + 1))
            };
        }
        let o = edge(&mut l, &mut s, i);
        accepted |= o.descriptor_ready && i.descriptor.is_some();
        if let Some(r) = o.request {
            assert!(active.is_none());
            active = Some((r, 0));
        }
        if let Some((a, data)) = o.write {
            assert!(writes.insert(a, data).is_none());
            run += 1;
            max_run = max_run.max(run);
        } else {
            run = 0;
        }
        if let Some(c) = o.completion {
            if let Some(old) = held_completion {
                assert_eq!(c, old);
            }
            held_completion = Some(c);
            completion_holds += 1;
            assert_eq!(writes, expected(d));
            assert!(active.is_none() && c.publish());
            if i.completion_ready {
                assert!(completion_holds >= 9 && max_run == 16);
                return s;
            }
        }
    }
    panic!("continuous write/completion hold bounded timeout");
}

#[test]
fn consecutive_64bit_returns_and_completion_hold_need_no_compute_ce() {
    let _ = continuous();
}

fn invalid_samples() -> Stats {
    let mut stats = Stats::default();
    let good = descriptor(1, 1024);
    let mut bad = Vec::new();
    for bytes in [1, 7, 1025, 1032, 2040] {
        let mut d = good;
        d.payload.bytes = bytes;
        bad.push(d);
    }
    let mut d = good;
    d.payload.source_halfword = 0x201;
    bad.push(d);
    let mut d = good;
    d.payload.source_halfword = SDRAM_HALFWORDS - 64;
    bad.push(d);
    let mut d = good;
    d.metadata_destination_word = 509;
    bad.push(d);
    let mut d = good;
    d.metadata_source_halfword += 1;
    bad.push(d);
    let mut d = good;
    d.payload.destination_word = 400;
    bad.push(d);
    let mut d = good;
    d.metadata_destination_word = d.payload.destination_word;
    bad.push(d);
    let mut d = good;
    d.config = Segment {
        source_halfword: 0x100,
        destination_word: d.metadata_destination_word,
        bytes: 32,
    };
    bad.push(d);
    for d in bad {
        assert!(!d.valid());
        let mut l = Loader::default();
        let i = Input {
            descriptor: Some(d),
            page_writable: true,
            gpu_drained: true,
            ..Input::default()
        };
        edge(
            &mut l,
            &mut stats,
            Input {
                reset: true,
                ..Input::default()
            },
        );
        assert!(edge(&mut l, &mut stats, i).descriptor_ready);
        let o = edge(&mut l, &mut stats, Input::default());
        assert!(o.request.is_none());
        assert_eq!(o.completion.unwrap().error, ERROR_DESCRIPTOR);
    }
    let mut boundary = descriptor(1, 128);
    boundary.payload.source_halfword = SDRAM_HALFWORDS - 64;
    boundary.metadata_source_halfword = SDRAM_HALFWORDS - 16;
    assert!(boundary.valid());
    let mut l = Loader::default();
    edge(
        &mut l,
        &mut stats,
        Input {
            reset: true,
            ..Input::default()
        },
    );
    run_job(&mut l, &mut stats, boundary, Fault::None);
    edge(
        &mut l,
        &mut stats,
        Input {
            reset: true,
            ..Input::default()
        },
    );
    stats
}

#[test]
fn descriptor_limits_reject_before_physical_request() {
    let _ = invalid_samples();
}

fn physical() -> Stats {
    use digital_design_hardware_gowin::sdram_memory_controller::{
        emu::{idle_inputs, Combination},
        ports::OracleImage,
    };
    let mut bytes = vec![0u8; 8192];
    for (n, data) in bytes.as_chunks_mut::<8>().0.iter_mut().enumerate() {
        data.copy_from_slice(&pattern(n as u32 * 4).to_le_bytes());
    }
    // Safety: this is the named external SDRAM fixture, not a computed intermediate.
    let image =
        unsafe { OracleImage::from_host(0, bytes, "VLIW loader external byte fixture") }.unwrap();
    let mut memory = Combination::new(image, 32).unwrap();
    let mut loader = Loader::default();
    let mut s = Stats::default();
    for (job, len) in [8, 40, 72, 128, 136, 1024].into_iter().enumerate() {
        let d = descriptor(job as u8, len);
        let mut accepted = false;
        let mut writes = BTreeMap::new();
        let mut complete = false;
        for _ in 0..50_000 {
            let mut input = Input {
                descriptor: (!accepted).then_some(d),
                page_writable: true,
                gpu_drained: true,
                completion_ready: true,
                ..Input::default()
            };
            let mut mi = idle_inputs();
            if let Some(r) = loader.output(&input).request {
                mi.gpu_ro_request_valid = true;
                mi.gpu_ro_address = u64::from(r.address_halfword);
                mi.gpu_ro_line_count_minus_1 = u64::from(r.line_count_minus_1);
            }
            let mo = memory.output(&mi);
            input.request_ready = mo.gpu_ro_request_ready;
            input.response = mo.gpu_ro_response_valid.then_some(Response {
                data: mo.gpu_ro_read_data,
                last: mo.gpu_ro_response_last,
                error: mo.gpu_ro_error,
            });
            let o = edge(&mut loader, &mut s, input);
            accepted |= o.descriptor_ready && input.descriptor.is_some();
            if let Some(r) = o.request.filter(|_| input.request_ready) {
                s.requests[r.line_count_minus_1 as usize] += 1;
            }
            if let Some((a, data)) = o.write {
                assert!(writes.insert(a, data).is_none());
                s.writes += 1;
            }
            memory.tick(&mi).unwrap();
            if let Some(c) = o.completion {
                assert!(c.publish());
                assert_eq!(writes, expected(d));
                complete = true;
                s.jobs += 1;
                break;
            }
        }
        assert!(complete, "physical SDRAM combination bounded timeout");
    }
    assert!(s.requests[0] > 0 && s.requests[1] > 0 && s.requests[3] > 0);
    s
}

#[test]
fn loader_uses_real_shared_arbiter_bridge_and_sdram_pins() {
    let s = physical();
    println!(
        "physical jobs={} edges={} requests={:?} writes={}",
        s.jobs,
        s.edges.len(),
        s.requests,
        s.writes
    );
}

fn vectors(edges: &[Edge]) -> String {
    let mut result = String::new();
    for e in edges {
        let i = e.input;
        let o = e.output;
        let d = i.descriptor.unwrap_or_default();
        let r = i.response.unwrap_or_default();
        let rq = o.request.unwrap_or_default();
        let w = o.write.unwrap_or_default();
        let c = o.completion.unwrap_or_default();
        let xs = [
            u64::from(i.reset),
            u64::from(i.abort),
            u64::from(i.descriptor.is_some()),
            u64::from(i.page_writable),
            u64::from(i.gpu_drained),
            u64::from(d.page),
            u64::from(d.tag),
            u64::from(d.config.source_halfword),
            u64::from(d.metadata_source_halfword),
            u64::from(d.payload.source_halfword),
            u64::from(d.config.destination_word),
            u64::from(d.metadata_destination_word),
            u64::from(d.payload.destination_word),
            u64::from(d.config.bytes),
            u64::from(d.payload.bytes),
            u64::from(i.request_ready),
            u64::from(i.response.is_some()),
            u64::from(r.last),
            u64::from(r.error),
            r.data,
            u64::from(i.completion_ready),
            u64::from(o.descriptor_ready),
            u64::from(o.request.is_some()),
            u64::from(rq.address_halfword),
            u64::from(rq.line_count_minus_1),
            u64::from(o.write.is_some()),
            u64::from(w.0),
            w.1,
            u64::from(o.completion.is_some()),
            u64::from(c.page),
            u64::from(c.tag),
            u64::from(c.error),
            u64::from(c.publish()),
            u64::from(o.busy),
        ];
        for (n, x) in xs.into_iter().enumerate() {
            if n > 0 {
                result.push(' ');
            }
            write!(result, "{x:x}").unwrap();
        }
        result.push('\n');
    }
    result
}
const TB: &str = r#"`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;
reg reset,abort,dv,pw,gd,page,rr,rv,rl,re,cr;reg[7:0]tag;
reg[21:0]cs,ms,ps;reg[8:0]cd,md,pd;reg[10:0]cb,pb;reg[63:0]rd;
wire dr,qv,dwe,cv,cp,cpage,busy;wire[21:0]qa;wire[1:0]qc;wire[9:0]da;wire[63:0]dd;wire[7:0]ct;wire[3:0]ce;
gpu_vliw_dma dut(clk,reset,abort,dv,dr,pw,gd,page,tag,cs,ms,ps,cd,md,pd,cb,pb,qv,rr,qa,qc,rv,rl,re,rd,dwe,da,dd,cv,cr,cp,cpage,ct,ce,busy);
integer f,n,k;reg edr,eqv,edwe,ecv,ecp,ecpage,eb;reg[21:0]eqa;reg[1:0]eqc;reg[9:0]eda;reg[63:0]edd;reg[7:0]ect;reg[3:0]ece;
initial begin
f=$fopen("vectors.txt","r");if(!f)$fatal(1,"missing vectors");
for(n=0;n<COUNT;n=n+1)begin
 @(negedge clk);
 k=$fscanf(f,"%h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h\n",reset,abort,dv,pw,gd,page,tag,cs,ms,ps,cd,md,pd,cb,pb,rr,rv,rl,re,rd,cr,edr,eqv,eqa,eqc,edwe,eda,edd,ecv,ecpage,ect,ece,ecp,eb);
 if(k!=34)$fatal(1,"vector fields edge%0d got%0d",n,k);
 #1;
 if({dr,qv,dwe,cv,busy}!=={edr,eqv,edwe,ecv,eb})$fatal(1,"control edge%0d got%b want%b state%0d",n,{dr,qv,dwe,cv,busy},{edr,eqv,edwe,ecv,eb},dut.state);
 if(qv&&{qa,qc}!=={eqa,eqc})$fatal(1,"request edge%0d",n);
 if(dwe&&{da,dd}!=={eda,edd})$fatal(1,"write edge%0d got%h/%h expected%h/%h",n,da,dd,eda,edd);
 if(cv&&{cpage,ct,ce,cp}!=={ecpage,ect,ece,ecp})$fatal(1,"completion edge%0d",n);
 @(posedge clk);#1;
end
$display("PASS DMA edges=%0d",COUNT);$finish;
end
initial begin #MAXTIME;$fatal(1,"absolute watchdog");end
endmodule
"#;
fn bounded(mut cmd: std::process::Command, dir: &Path, label: &str) {
    let mut child = cmd
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{label}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{label}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{label}: {} {}",
                fs::read_to_string(dir.join(format!("{label}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{label}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{label} wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn loader_rtl_matches_every_wall_edge_synthetic_and_real_mc() {
    let mut s = synthetic();
    s.edges.extend(invalid_samples().edges);
    s.edges.extend(continuous().edges);
    s.edges.extend(physical().edges);
    // A shared transport reset separates the two independent stimuli.
    let split = s.edges.iter().position(|e| e.input.reset).unwrap();
    assert_eq!(split, 0);
    // physical starts idle. Synthetic has consumed its completion and ends in reset.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-vliw-integration-20261018/stage3/rtl");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("vectors.txt"), vectors(&s.edges)).unwrap();
    fs::write(dir.join("vliw_dma.v"), RTL).unwrap();
    fs::write(
        dir.join("tb.v"),
        TB.replace("COUNT", &s.edges.len().to_string())
            .replace("MAXTIME", &(s.edges.len() * 10 + 1000).to_string()),
    )
    .unwrap();
    let mut cmd = std::process::Command::new(
        std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
    );
    cmd.current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp", "vliw_dma.v", "tb.v"]);
    bounded(cmd, &dir, "compile");
    let mut cmd =
        std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    cmd.current_dir(&dir).arg("test.vvp");
    bounded(cmd, &dir, "run");
    let log = fs::read_to_string(dir.join("run.out")).unwrap();
    assert!(log.contains("PASS DMA"));
    println!("{log}");
}
