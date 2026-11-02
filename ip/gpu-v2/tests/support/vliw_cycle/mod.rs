use gpu_v2::system::vliw_cycle::{
    command_dma::{Cache, Work},
    router::{Framebuffer, Texture},
    *,
};
use std::{
    collections::VecDeque,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod native;
mod pins;

#[derive(Clone, Copy)]
pub struct Edge {
    pub input: Input,
    pub output: Output,
}
#[derive(Default)]
pub struct Stats {
    pub max_dma_returns: u8,
    pub max_ro_owners: u8,
    pub ce0_writes: usize,
    pub held_requests: usize,
    pub delayed_write_terminals: usize,
    pub overlapped_write_payload: usize,
    pub work_done: usize,
    pub cache_done: usize,
    pub texture_done: usize,
    pub fb_done: usize,
    pub uniform_done: usize,
    pub mesh_done: usize,
    pub ro_accept_with_terminal: usize,
    pub fb_accept_with_terminal: usize,
}
pub struct Run {
    pub trace: Vec<Edge>,
    pub stats: Stats,
    pub reset_while_active: bool,
    pub native_cycles: u64,
    pub backing_write_logic_edges: usize,
}

fn pattern(address_halfword: u32, beat: usize) -> u64 {
    0x1234_abcd_5078_ef01
        ^ u64::from(address_halfword + beat as u32 * 4).wrapping_mul(0xfed1_0000_1703)
}
fn written(beat: usize) -> u64 {
    0xfedc_1234_9876_0110 ^ (beat as u64).wrapping_mul(0x1234_9175_171d)
}

struct Actor {
    cache: usize,
    work: usize,
    texture: usize,
    fb: usize,
    fb_retired: usize,
    write_beat: usize,
    ram: [u64; 512],
    known: [bool; 512],
    read_beat: usize,
}
impl Default for Actor {
    fn default() -> Self {
        Self {
            cache: 0,
            work: 0,
            texture: 0,
            fb: 0,
            fb_retired: 0,
            write_beat: 0,
            ram: [0; 512],
            known: [false; 512],
            read_beat: 0,
        }
    }
}
impl Actor {
    fn input(&self, wall: usize) -> Input {
        let works = [
            Work {
                mesh: true,
                source_halfword: 0x1000,
                blocks: 1,
                ..Default::default()
            },
            Work {
                source_halfword: 0x1400,
                destination: 92,
                size: 2,
                ..Default::default()
            },
            Work {
                mesh: true,
                source_halfword: 0x1800,
                blocks: 2,
                slot: true,
                ..Default::default()
            },
            Work {
                source_halfword: 0x1c00,
                destination: 96,
                size: 1,
                ..Default::default()
            },
            Work {
                source_halfword: 0x1e00,
                destination: 120,
                size: 0,
                ..Default::default()
            },
        ];
        let mut work = works[self.work.min(4)];
        work.valid = self.work < works.len();
        let reading = self.fb_retired >= 4;
        let fb_valid = if reading { self.fb < 8 } else { self.fb < 4 };
        Input {
            ce: wall % 11 > 4,
            cache: Cache {
                valid: self.cache < 3,
                source_halfword: 0x2000 + self.cache as u32 * 64,
                bank: self.cache & 1 != 0,
            },
            work,
            texture: Texture {
                valid: self.texture < 4,
                address_bytes: 0x6000 + self.texture as u32 * 128,
            },
            framebuffer: Framebuffer {
                valid: fb_valid,
                writing: !reading,
                address_bytes: 0x8000 + (self.fb % 4) as u32 * 128,
                write_valid: !reading && self.write_beat < 64,
                write_data: written(self.write_beat),
            },
            ..Default::default()
        }
    }
    fn observe(&mut self, i: Input, o: Output, s: &mut Stats) {
        if o.router.physical[0].valid
            && i.physical[0].request_ready
            && i.physical[0].response.valid
            && i.physical[0].response.last
        {
            s.ro_accept_with_terminal += 1;
        }
        if i.framebuffer.valid && o.router.framebuffer_ready && o.router.framebuffer_complete {
            s.fb_accept_with_terminal += 1;
        }
        if o.router.texture_read {
            assert_eq!(
                o.router.texture_data,
                pattern(
                    0x3000 + s.texture_done as u32 * 64,
                    o.router.texture_index as usize
                )
            );
        }
        if o.router.framebuffer_read {
            assert_eq!(o.router.framebuffer_data, written(self.read_beat));
            self.read_beat += 1;
        }
        if i.ce && i.work.valid && o.dma.work_ready {
            self.work += 1;
        }
        if i.cache.valid && o.dma.cache_ready {
            self.cache += 1;
        }
        if i.texture.valid && o.router.texture_ready {
            self.texture += 1;
        }
        if i.framebuffer.valid && o.router.framebuffer_ready {
            self.fb += 1;
        }
        if o.router.framebuffer_write_ready {
            self.write_beat += 1;
        }
        if o.dma.ram_write {
            self.ram[o.dma.ram_address as usize] = o.dma.ram_data;
            self.known[o.dma.ram_address as usize] = true;
            if !i.ce {
                s.ce0_writes += 1;
            }
        }
        if o.dma.work_done {
            s.work_done += 1;
        }
        if o.dma.cache_done {
            s.cache_done += 1;
        }
        if o.dma.uniform_done {
            assert_eq!(o.dma.uniform_bank, 3);
            s.uniform_done += 1;
        }
        if o.dma.mesh_done {
            s.mesh_done += 1;
        }
        if o.router.texture_complete {
            s.texture_done += 1;
        }
        if o.router.framebuffer_complete {
            self.fb_retired += 1;
            s.fb_done += 1;
        }
        if o.router
            .physical
            .iter()
            .zip(i.physical)
            .any(|(r, p)| r.valid && !p.request_ready)
        {
            s.held_requests += 1;
        }
    }
    fn done(&self, s: &Stats) -> bool {
        s.work_done == 5 && s.cache_done == 3 && s.texture_done == 4 && s.fb_done == 8
    }
    fn check_ram(&self) {
        for (destination, source, beats) in [
            (448, 0x2080, 16),
            (464, 0x2040, 16),
            (256, 0x1000, 4),
            (260, 0x1800, 4),
            (0, 0x1040, 32),
            (128, 0x1840, 48),
            (368, 0x1400, 16),
            (384, 0x1c00, 8),
            (480, 0x1e00, 4),
        ] {
            for beat in 0..beats {
                assert!(self.known[destination + beat]);
                assert_eq!(
                    self.ram[destination + beat],
                    pattern(source, beat),
                    "RAM row{}",
                    destination + beat
                );
            }
        }
        assert_eq!(self.known.iter().filter(|&&v| v).count(), 148);
        assert_eq!(self.read_beat, 64);
    }
}

struct Tx {
    request: Request,
    beat: usize,
    ready_at: usize,
    ack_at: Option<usize>,
}
#[derive(Default)]
struct Peer {
    q: [VecDeque<Tx>; 3],
    memory: std::collections::BTreeMap<u32, u64>,
    terminals: usize,
}
impl Peer {
    fn output(&self, wall: usize) -> [Physical; 3] {
        std::array::from_fn(|p| {
            let mut f = Physical {
                request_ready: self.q[p].len() < 2 && wall % 9 > 2,
                ..Default::default()
            };
            if p == 2 {
                f.write_ready =
                    self.q[p].iter().any(|t| t.beat < 16 && wall >= t.ready_at) && wall % 7 != 3;
                f.response.valid = self.q[p]
                    .front()
                    .is_some_and(|t| t.ack_at.is_some_and(|at| wall >= at));
                f.response.last = f.response.valid;
            } else if let Some(t) = self.q[p].front() {
                if wall >= t.ready_at && wall % 5 != 1 {
                    f.response = Response {
                        valid: true,
                        last: t.beat + 1 == (t.request.count as usize + 1) * 4,
                        data: if p == 1 {
                            self.memory[&(t.request.address_halfword + t.beat as u32 * 4)]
                        } else {
                            pattern(t.request.address_halfword, t.beat)
                        },
                        error: false,
                    };
                }
            }
            f
        })
    }
    fn clock(&mut self, wall: usize, i: Input, o: Output, s: &mut Stats) {
        for p in 0..3 {
            if p == 2 && i.physical[p].write_ready {
                let at = self.q[p].iter().position(|t| t.beat < 16).unwrap();
                let t = &mut self.q[p][at];
                self.memory.insert(
                    t.request.address_halfword + t.beat as u32 * 4,
                    o.router.write_data,
                );
                t.beat += 1;
                if at > 0 {
                    s.overlapped_write_payload += 1;
                }
                if t.beat == 16 {
                    t.ack_at = Some(wall + 19);
                    s.delayed_write_terminals += 1;
                }
            }
            if i.physical[p].response.valid {
                let t = self.q[p].front_mut().unwrap();
                if p != 2 {
                    t.beat += 1;
                }
                if i.physical[p].response.last {
                    self.q[p].pop_front();
                    self.terminals += 1;
                }
            }
            if o.router.physical[p].valid && i.physical[p].request_ready {
                self.q[p].push_back(Tx {
                    request: o.router.physical[p],
                    beat: 0,
                    ready_at: wall + 17,
                    ack_at: None,
                });
            }
        }
    }
}

pub fn synthetic() -> Run {
    let mut run = Run {
        trace: Vec::new(),
        stats: Stats::default(),
        reset_while_active: false,
        native_cycles: 0,
        backing_write_logic_edges: 0,
    };
    let mut t = Transport::default();
    let mut actor = Actor::default();
    let mut peer = Peer::default();
    for wall in 0..4000 {
        let reset = wall < 2 || wall == 67 || wall == 68;
        if wall == 67 {
            run.reset_while_active = t.outputs(Input::default()).router.busy;
        }
        let mut i = actor.input(wall);
        i.reset = reset;
        if !reset {
            i.physical = peer.output(wall);
        }
        let o = t.outputs(i);
        run.trace.push(Edge {
            input: i,
            output: o,
        });
        if reset {
            t.clock(i).unwrap();
            actor = Actor::default();
            peer = Peer::default();
            run.stats = Stats::default();
            continue;
        }
        let before = t.dma.occupancy();
        let owners = t.router.occupancy();
        run.stats.max_dma_returns = run.stats.max_dma_returns.max(before.1);
        run.stats.max_ro_owners = run.stats.max_ro_owners.max(owners.0);
        t.clock(i).unwrap_or_else(|e| panic!("wall {wall}: {e}"));
        actor.observe(i, o, &mut run.stats);
        peer.clock(wall, i, o, &mut run.stats);
        if actor.done(&run.stats) && o.dma.idle && !o.router.busy {
            actor.check_ram();
            for (at, value) in &peer.memory {
                let beat = ((*at - 0x4000) / 4) as usize;
                assert_eq!(*value, written(beat));
            }
            return run;
        }
    }
    panic!("synthetic transport exceeded4000 wall edges");
}

pub fn native() -> Run {
    native::run()
}

/// Legal same-edge accept/terminal and write-last/ACK, with one old snapshot.
pub fn simultaneous_trace() -> Vec<Edge> {
    let mut t = Transport::default();
    let mut trace = Vec::new();
    for wall in 0usize..38 {
        let mut i = Input {
            reset: wall == 0,
            ..Default::default()
        };
        i.texture = Texture {
            valid: matches!(wall, 1 | 2 | 17 | 18),
            address_bytes: if wall < 17 { 0x6000 } else { 0x6080 },
        };
        i.framebuffer = Framebuffer {
            valid: matches!(wall, 1 | 17),
            writing: true,
            address_bytes: if wall < 17 { 0x8000 } else { 0x8080 },
            write_valid: wall > 0 && wall <= 33,
            write_data: written(wall.saturating_sub(2)),
        };
        i.physical[0].request_ready = true;
        i.physical[2].request_ready = true;
        i.physical[2].write_ready = (2..=33).contains(&wall);
        i.physical[2].response = Response {
            valid: matches!(wall, 17 | 33),
            last: matches!(wall, 17 | 33),
            ..Default::default()
        };
        if (3..=34).contains(&wall) {
            i.physical[0].response = Response {
                valid: true,
                last: matches!(wall, 18 | 34),
                data: pattern(if wall <= 18 { 0x3000 } else { 0x3040 }, (wall - 3) % 16),
                error: false,
            };
        }
        let o = t.outputs(i);
        trace.push(Edge {
            input: i,
            output: o,
        });
        t.clock(i)
            .unwrap_or_else(|e| panic!("simultaneous{wall}: {e}"));
    }
    assert!(trace
        .iter()
        .any(|e| e.output.router.texture_ready && e.output.router.texture_complete));
    assert!(trace.iter().any(|e| e.output.router.framebuffer_ready
        && e.input.framebuffer.valid
        && e.output.router.framebuffer_complete
        && e.output.router.framebuffer_write_ready));
    assert!(!trace.last().unwrap().output.router.busy);
    trace
}

pub fn stop_trace() -> Vec<Edge> {
    let mut t = Transport::default();
    let mut peer = Peer::default();
    let reset = Input {
        reset: true,
        ..Default::default()
    };
    let mut trace = vec![Edge {
        input: reset,
        output: t.outputs(reset),
    }];
    t.clock(reset).unwrap();
    let mut offered = false;
    let mut had_write = false;
    let mut stopped = false;
    for wall in 0..500 {
        let mut i = Input {
            ce: true,
            stop: stopped,
            work: Work {
                valid: !offered,
                mesh: true,
                source_halfword: 0x1000,
                blocks: 2,
                ..Default::default()
            },
            physical: peer.output(wall),
            ..Default::default()
        };
        if had_write {
            stopped = true;
            i.stop = true;
        }
        let o = t.outputs(i);
        trace.push(Edge {
            input: i,
            output: o,
        });
        if i.work.valid && o.dma.work_ready {
            offered = true;
        }
        had_write |= o.dma.ram_write;
        if stopped {
            assert!(!o.dma.ram_write && !o.dma.work_done && !o.dma.mesh_done);
        }
        t.clock(i).unwrap();
        peer.clock(wall, i, o, &mut Stats::default());
        if stopped && o.dma.idle && !o.router.busy {
            return trace;
        }
    }
    panic!("stop failed to drain500 edges");
}

pub fn invalid_peers(good: &[Edge]) -> Vec<(&'static str, Vec<Edge>)> {
    let first = good
        .iter()
        .position(|e| {
            !e.input.reset
                && e.input.physical[0].response.valid
                && !e.input.physical[0].response.last
        })
        .unwrap();
    let mut early = good[..=first].to_vec();
    early[first].input.physical[0].response.last = true;
    let mut error = good[..=first].to_vec();
    error[first].input.physical[0].response.error = true;
    let mut hole = good.to_vec();
    let at = hole
        .iter()
        .position(|e| e.output.router.framebuffer_write_ready)
        .unwrap();
    hole.truncate(at + 1);
    hole[at].input.framebuffer.write_valid = false;
    let mut unowned = good[..2].to_vec();
    unowned.push(Edge {
        input: Input {
            physical: [
                Physical {
                    response: Response {
                        valid: true,
                        last: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                Default::default(),
                Default::default(),
            ],
            ..Default::default()
        },
        output: Output::default(),
    });
    let mut result = vec![
        ("early_last", early),
        ("peer_error", error),
        ("write_hole", hole),
        ("unowned", unowned),
    ];
    for (_, edges) in &mut result {
        let length = edges.len();
        let mut t = Transport::default();
        let mut rejected = false;
        for (n, e) in edges.iter_mut().enumerate() {
            e.output = t.outputs(e.input);
            match t.clock(e.input) {
                Ok(()) => {}
                Err(_) => {
                    assert_eq!(n + 1, length);
                    rejected = true;
                    break;
                }
            }
        }
        assert!(rejected);
    }
    result
}

pub fn evidence_root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-transport")
        .join(format!("qualification-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn tool(mut command: Command, dir: &Path, label: &str) -> std::process::ExitStatus {
    let mut child = command
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
            return status;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{label} exceeded30s: {}", dir.display());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn rtl(
    root: &Path,
    label: &str,
    trace: &[Edge],
    mutation: Option<&str>,
    success: bool,
    contract: bool,
) {
    let dir = root.join(label);
    fs::create_dir(&dir).unwrap();
    let mut dma = include_str!("../../../src/system/command_dma.v").to_owned();
    let mut router = include_str!("../../../src/system/vliw_memory.v").to_owned();
    if let Some(m) = mutation {
        let (source, old, new) = match m {
            "ce_drop" => (&mut dma, "if(dma_we)cursor<=cursor+1'b1;", "if(ce&&dma_we)cursor<=cursor+1'b1;"),
            "wrong_owner" => (&mut router, "r_owner[rput[0]]<=offer_owner;", "r_owner[rput[0]]<=0;"),
            "ignore_ready" => (&mut router, "(ro_legal?ro_request_ready:1'b1)", "1'b1"),
            "early_write_terminal" => (&mut router, "wire f_terminal=local_fb_error||(fused!=0&&fv&&(fl||fe));", "wire f_terminal=local_fb_error||(fused!=0&&((fb_write&&fb_w_data_ready&&fbeat==15)||(!fb_write&&fv&&(fl||fe))));"),
            _ => panic!("unknown actual DUT mutation"),
        };
        assert_eq!(source.matches(old).count(), 1);
        *source = source.replacen(old, new, 1);
    }
    fs::write(dir.join("dut.v"), format!("`define GPU_V2_COMMAND_TRANSPORT\n{dma}\n{router}\n`undef GPU_V2_COMMAND_TRANSPORT\n{}", pins::wrapper())).unwrap();
    let schema = pins::fields(trace[0]);
    let mut tb = String::from("`timescale 1ns/1ps\nmodule tb;reg clk=0;always #5 clk=~clk;\n");
    let mut format = String::new();
    let mut names = String::new();
    let mut checks = String::new();
    for f in &schema {
        if f.output {
            writeln!(
                tb,
                "wire[{}:0]{};reg[{}:0]e_{};reg care_{};",
                f.width - 1,
                f.name,
                f.width - 1,
                f.name,
                f.name
            )
            .unwrap();
            write!(names, ",e_{},care_{}", f.name, f.name).unwrap();
            format.push_str("%h %h ");
            writeln!(
                checks,
                "if(care_{}&&{}!==e_{})$fatal(1,\"DIFF {} edge%0d got%h expected%h\",n,{},e_{});",
                f.name, f.name, f.name, f.name, f.name, f.name
            )
            .unwrap();
        } else {
            writeln!(tb, "reg[{}:0]{};", f.width - 1, f.name).unwrap();
            write!(names, ",{}", f.name).unwrap();
            format.push_str("%h ");
        }
    }
    tb.push_str("p1a dut(.clk(clk)");
    for f in &schema {
        write!(tb, ",.{}({})", f.name, f.name).unwrap();
    }
    tb.push_str(
        ");\ninteger fd,n,k;initial begin fd=$fopen(\"vectors.txt\",\"r\");if(!fd)$fatal;\n",
    );
    let columns: usize = schema.iter().map(|f| if f.output { 2 } else { 1 }).sum();
    writeln!(tb,"for(n=0;n<{};n=n+1)begin @(negedge clk);k=$fscanf(fd,\"{format}\\n\"{names});if(k!={columns})$fatal(1,\"vectors\");#1;if(n>0)begin {checks} end @(posedge clk);#1;end $display(\"P1A_PASS edges={}\");$finish;end\ninitial begin #{};$fatal(1,\"watchdog\");end endmodule",trace.len(),trace.len(),trace.len()*10+1000).unwrap();
    let mut vectors = String::new();
    for e in trace {
        for f in pins::fields(*e) {
            write!(vectors, "{:x} ", f.value).unwrap();
            if f.output {
                write!(vectors, "{} ", u8::from(f.care)).unwrap();
            }
        }
        vectors.push('\n');
    }
    fs::write(dir.join("tb.v"), tb).unwrap();
    fs::write(dir.join("vectors.txt"), vectors).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"]);
    assert!(
        tool(compile, &dir, "compile").success(),
        "{}",
        fs::read_to_string(dir.join("compile.err")).unwrap()
    );
    let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    execute.current_dir(&dir).arg("test.vvp");
    let status = tool(execute, &dir, "run");
    let output = fs::read_to_string(dir.join("run.out")).unwrap();
    assert_eq!(status.success(), success, "{label}: {output}");
    if success {
        assert!(output.contains("P1A_PASS"));
    } else if contract {
        assert!(
            !output.contains("DIFF"),
            "peer must be rejected by monitor: {output}"
        );
        assert!(
            output.contains("GPU contract")
                || output.contains("DMA")
                || output.contains("presented request"),
            "{output}"
        );
    } else {
        assert!(
            output.contains("DIFF"),
            "compiled DUT mutation must cause a real output difference: {output}"
        );
    }
    fs::write(dir.join("verdict.json"),format!("{{\"label\":\"{label}\",\"edges\":{},\"expected_success\":{success},\"actual_success\":{},\"contract_rejection\":{contract}}}\n",trace.len(),status.success())).unwrap();
}

pub fn receipt(root: &Path, s: &Run, n: &Run) {
    fs::write(root.join("coverage.json"),format!("{{\"positive_cases\":[\"synthetic-active-reset\",\"native-feedback\",\"stop-drain\",\"same-edge-accept-terminal\"],\"same_edge_stream_ro\":{},\"same_edge_stream_fb\":{},\"direct_same_edge_case\":true,\"mesh_pages\":2,\"cache_banks\":2,\"uniform_bank\":3,\"ram_rows_verified\":148,\"raw_fb_qwords_verified\":64,\"board_request_slots\":\"not modeled\",\"CPU_Display_requesters\":\"idle\"}}\n",s.stats.ro_accept_with_terminal,s.stats.fb_accept_with_terminal)).unwrap();
    fs::write(root.join("receipt.json"), format!("{{\"scope\":\"Production command DMA/router only; no Device4/DRAW/Display/whole-system claim\",\"synthetic_edges\":{},\"native_logic_edges\":{},\"native_cycles\":{},\"backing_write_logic_edges\":{},\"max_ro_owners\":{},\"max_dma_returns\":{},\"ce0_writes\":{},\"mutations_detected\":4,\"invalid_peers_rejected\":4,\"native_rtl_mode\":\"GPU RTL replays live Rust-native-MC feedback; every request/data/output checked\"}}\n",s.trace.len(),n.trace.len(),n.native_cycles,n.backing_write_logic_edges,s.stats.max_ro_owners,s.stats.max_dma_returns,s.stats.ce0_writes)).unwrap();
}
