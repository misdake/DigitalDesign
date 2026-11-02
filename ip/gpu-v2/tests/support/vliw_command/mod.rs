use crate::transport_support::{self as transport, native::Memory, pins::Pin};
use gpu_v2::system::vliw_cycle::{
    command::{Control, DeviceBus, Error, Input, Output, Phase},
    Physical,
};
use std::{
    collections::VecDeque,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy)]
pub struct Edge {
    pub input: Input,
    pub output: Output,
    pub phase: Phase,
    pub pc: u8,
    pub pages: u8,
}
#[derive(Default)]
pub struct Stats {
    pub submits: usize,
    pub completed: usize,
    pub cache_fetches: usize,
    pub writes: usize,
    pub ce0_writes: usize,
    pub ram_stalls: usize,
    pub flush_stalls: usize,
    pub end_stalls: usize,
    pub ce0_device_writes: usize,
    pub commands: usize,
    pub aborts: usize,
    pub ce0_retirements: usize,
}
pub struct Run {
    pub trace: Vec<Edge>,
    pub stats: Stats,
    pub native_cycles: u64,
    pub error: Option<Error>,
}
struct Ram {
    cells: [Option<u64>; 512],
    q: u64,
}
impl Default for Ram {
    fn default() -> Self {
        Self {
            cells: [None; 512],
            q: 0,
        }
    }
}
impl Ram {
    fn clock(&mut self, i: Input, o: Output) {
        if i.reset {
            self.q = 0;
            return;
        }
        let read = i.ce && i.ram_ready && o.processor.ram_read;
        let d = o.transport.dma;
        assert!(
            !(read && d.ram_write && o.processor.ram_address == d.ram_address),
            "production cross-port collision"
        );
        if read {
            self.q =
                self.cells[o.processor.ram_address as usize].expect("unpublished command RAM read");
        }
        if d.ram_write {
            self.cells[d.ram_address as usize] = Some(d.ram_data);
        }
    }
}
fn program(long: bool) -> Vec<u32> {
    let mut v = vec![0; if long { 62 } else { 1 }];
    v.extend([0x4000_0003, 0x7000_0000, 0xf000_0000]);
    v
}

/// One native MC owner. Backend idle/flush and source availability are peers.
pub fn native(long: bool, bad: Option<&str>) -> Run {
    let mut memory = Memory::new();
    let mut model = Control::default();
    let mut ram = Ram::default();
    let mut first = program(long);
    let second = program(false);
    match bad {
        Some("bad_nop") => first[0] = 1,
        Some("bad_wait") => first[1] = 0x4000_0004,
        Some("bad_flush") => first[2] = 0x7000_0001,
        Some("bad_end") => *first.last_mut().unwrap() = 0xf000_0001,
        Some("missing_fence") => first[1] = 0x4000_0001,
        Some("missing_end") => *first.last_mut().unwrap() = 0,
        Some("early_end") => first[0] = 0xf000_0000,
        Some("unsupported") => first[0] = 0x9000_0000,
        _ => {}
    }
    let mut backing = std::collections::BTreeMap::new();
    for (base, code) in [(0x1000usize, first.as_slice()), (0x1800, second.as_slice())] {
        let padded = (code.len() + 31) & !31;
        let mut halfwords = vec![0u16; padded * 2];
        for (n, &word) in code.iter().enumerate() {
            halfwords[n * 2] = word as u16;
            halfwords[n * 2 + 1] = (word >> 16) as u16;
        }
        memory.preload(base, &halfwords);
        for (n, q) in halfwords.as_chunks::<4>().0.iter().enumerate() {
            backing.insert(
                base as u32 + n as u32 * 4,
                (0..4).fold(0u64, |v, k| v | (u64::from(q[k]) << (k * 16))),
            );
        }
    }
    let mut scheduled =
        VecDeque::from([(2usize, 0u8, (first.len() - 1) as u16), (3, 1, 0x1000 / 64)]);
    let mut jobs: VecDeque<(u32, bool, usize)> = VecDeque::new();
    let mut run = Run {
        trace: Vec::new(),
        stats: Stats::default(),
        native_cycles: 0,
        error: None,
    };
    let mut was_busy = false;
    let mut waiting = 0;
    let mut flushing = 0;
    let mut ending = 0;
    let mut aborted = false;
    let mut recovery_reset = None;
    let mut recovered = false;
    for wall in 0..200000 {
        let phase = model.processor.phase();
        waiting = if phase == Phase::Waiting {
            waiting + 1
        } else {
            0
        };
        flushing = if phase == Phase::Flushing {
            flushing + 1
        } else {
            0
        };
        ending = if phase == Phase::Ending {
            ending + 1
        } else {
            0
        };
        let mut i = Input {
            reset: wall < 2,
            ce: wall % 7 > 2,
            bus: DeviceBus {
                index: 4,
                channel: 2,
                read: true,
                ..Default::default()
            },
            ram_ready: wall >= 2 && wall % 11 > 3,
            ram_q: ram.q,
            render_idle: !(phase == Phase::Waiting && waiting < 9
                || phase == Phase::Ending && ending < 7),
            flush_complete: phase == Phase::Flushing && flushing >= 13,
            ..Default::default()
        };
        if let Some((_, channel, data)) = scheduled.pop_front_if(|s| s.0 == wall) {
            i.bus = DeviceBus {
                index: 4,
                channel,
                write: true,
                data,
                ..Default::default()
            };
            i.ce = false;
        }
        if phase == Phase::Ending && ending >= 7 {
            i.ce = false;
        }
        if wall == 5 {
            i.bus = DeviceBus {
                index: 3,
                channel: 1,
                write: true,
                data: 0xffff,
                ..Default::default()
            };
        }
        if wall == 6 {
            i.bus = DeviceBus {
                index: 4,
                channel: 3,
                write: true,
                data: 0xffff,
                ..Default::default()
            };
        }
        match bad {
            Some("base_overflow") if wall == 2 => i.bus.data = 0x3fff,
            Some("base_overflow") if wall == 3 => i.bus.data = 0xffff,
            Some("no_count") if wall == 2 => {
                i.bus.channel = 1;
                i.bus.data = 0x40;
            }
            Some("count_reserved") if wall == 2 => i.bus.data |= 0xc000,
            Some("busy_start") if wall == 4 => {
                i.bus = DeviceBus {
                    index: 4,
                    channel: 1,
                    write: true,
                    data: 0x60,
                    ..Default::default()
                }
            }
            Some("busy_count") if wall == 4 => {
                i.bus = DeviceBus {
                    index: 4,
                    channel: 0,
                    write: true,
                    data: 3,
                    ..Default::default()
                }
            }
            _ => {}
        }
        if bad == Some("abort_reset") && !aborted && run.stats.writes >= 2 {
            i.abort = true;
            aborted = true;
            run.stats.aborts = 1;
        }
        if recovery_reset == Some(wall) {
            i.reset = true;
            i.ram_ready = false;
            run.stats = Stats {
                aborts: 1,
                ..Default::default()
            };
            jobs.clear();
            was_busy = false;
            scheduled.extend([(wall + 1, 0, 3), (wall + 2, 1, 0x1800 / 64)]);
            recovered = true;
        }
        i.physical = memory.feedback(model.outputs(i).transport.router, i.reset);
        let o = model.outputs(i);
        run.trace.push(Edge {
            input: i,
            output: o,
            phase,
            pc: model.processor.pc(),
            pages: model.processor.cache_pages(),
        });
        if let Err(error) = model.clock(i) {
            assert!(bad.is_some(), "wall{wall}: {error}");
            run.error = Some(error);
            run.native_cycles = memory.native_cycles();
            return run;
        }
        if !i.reset {
            if o.processor.busy && !model.outputs(i).processor.busy && !i.ce {
                run.stats.ce0_retirements += 1;
            }
            if i.ce && !o.processor.stop && phase == Phase::Exec {
                run.stats.commands += 1;
            }
            if i.bus.write && !i.ce && i.bus.index == 4 && i.bus.channel < 2 {
                run.stats.ce0_device_writes += 1;
            }
            if i.bus.write && i.bus.index == 4 && i.bus.channel == 1 {
                run.stats.submits += 1;
            }
            if o.processor.cache.valid && o.transport.dma.cache_ready {
                jobs.push_back((o.processor.cache.source_halfword, o.processor.cache.bank, 0));
                run.stats.cache_fetches += 1;
            }
            if o.transport.dma.ram_write {
                let j = jobs.front_mut().unwrap();
                assert_eq!(
                    o.transport.dma.ram_address,
                    448 + u16::from(j.1) * 16 + j.2 as u16
                );
                assert_eq!(o.transport.dma.ram_data, backing[&(j.0 + j.2 as u32 * 4)]);
                j.2 += 1;
                run.stats.writes += 1;
                if !i.ce {
                    run.stats.ce0_writes += 1;
                }
            }
            if o.transport.dma.cache_done {
                assert_eq!(jobs.pop_front().unwrap().2, 16);
            }
            if phase == Phase::Fetch && o.processor.ram_read && !i.ram_ready {
                run.stats.ram_stalls += 1;
            }
            if phase == Phase::Flushing && !i.flush_complete {
                run.stats.flush_stalls += 1;
            }
            if phase == Phase::Ending && !i.render_idle {
                run.stats.end_stalls += 1;
                assert!(o.processor.busy);
            }
            if was_busy && !o.processor.busy {
                run.stats.completed += 1;
                assert_eq!(o.device_read_data, 0, "STATUS idle, no ACK");
                if run.stats.completed == 1 {
                    scheduled.extend([(wall + 1, 0, 3), (wall + 2, 1, 0x1800 / 64)]);
                }
            }
            was_busy = o.processor.busy;
        }
        if aborted
            && !recovered
            && recovery_reset.is_none()
            && phase == Phase::Failed
            && o.transport.dma.idle
            && !o.transport.router.busy
        {
            assert!(o.processor.busy && !o.processor.submit_ready && o.processor.stop);
            assert_eq!(
                o.device_read_data, 1,
                "abort does not fabricate transport error status"
            );
            recovery_reset = Some(wall + 1);
        }
        ram.clock(i, o);
        memory.clock();
        if run.stats.completed == 2 && o.transport.dma.idle && !o.transport.router.busy {
            assert!(bad.is_none() || bad == Some("abort_reset"));
            assert!(jobs.is_empty());
            assert_eq!(run.stats.writes, if long { 64 } else { 32 });
            assert_eq!(run.stats.cache_fetches, if long { 4 } else { 2 });
            for row in 0..16 {
                assert_eq!(
                    ram.cells[448 + row],
                    Some(backing[&(0x1800 + row as u32 * 4)])
                );
            }
            run.native_cycles = memory.native_cycles();
            return run;
        }
    }
    panic!("command-control native fixture exceeded200000 logic edges");
}

pub fn root() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-command")
        .join(format!(
            "qualification-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&p).unwrap();
    p
}
pub fn fields(e: Edge) -> Vec<Pin> {
    let i = e.input;
    let o = e.output;
    let mut p = Vec::new();
    macro_rules! pin {
        ($n:literal,$w:expr,$v:expr,$out:expr) => {
            p.push(Pin {
                name: $n,
                width: $w,
                value: u64::from($v),
                output: $out,
                care: true,
            });
        };
    }
    pin!("reset", 1, i.reset, false);
    pin!("ce", 1, i.ce, false);
    pin!("abort", 1, i.abort, false);
    pin!("device_index", 3, i.bus.index, false);
    pin!("device_channel", 4, i.bus.channel, false);
    pin!("device_read_enable", 1, i.bus.read, false);
    pin!("device_write_enable", 1, i.bus.write, false);
    pin!("device_write_data", 16, i.bus.data, false);
    pin!("ram_available", 1, i.ram_ready, false);
    pin!("render_idle", 1, i.render_idle, false);
    pin!("render_fault", 1, i.render_fault, false);
    pin!("flush_complete", 1, i.flush_complete, false);
    pin!("device_read_data", 16, o.device_read_data, true);
    pin!("busy", 1, o.processor.busy, true);
    pin!("submit_ready", 1, o.processor.submit_ready, true);
    pin!("stop", 1, o.processor.stop, true);
    pin!("cp_re", 1, o.processor.ram_read, true);
    pin!("cp_addr", 9, o.processor.ram_address, true);
    pin!("cp_q", 64, i.ram_q, true);
    pin!("cache_valid", 1, o.processor.cache.valid, true);
    pin!("cache_source", 22, o.processor.cache.source_halfword, true);
    pin!("cache_bank", 1, o.processor.cache.bank, true);
    pin!("flush_request", 1, o.processor.flush_request, true);
    pin!("phase", 5, e.phase as u8, true);
    pin!("pc", 6, e.pc, true);
    pin!("cache_pages", 2, e.pages, true);
    for f in transport::pins::fields(transport::Edge {
        input: gpu_v2::system::vliw_cycle::Input {
            physical: i.physical,
            ..Default::default()
        },
        output: o.transport,
    }) {
        if f.output || f.name.starts_with("ro_") || f.name.starts_with("fb_") {
            p.push(f);
        }
    }
    p
}
fn binding() -> String {
    let s = include_str!("../../../src/system/vliw_command.v");
    s[s.find(" wire device_selected").unwrap()..s.find(" wire start,slot,stop").unwrap()]
        .replace(" wire submit_ready;", "")
}
fn wrapper(schema: &[Pin]) -> String {
    let mut s = String::from("module p1b(input clk");
    for f in schema {
        write!(
            s,
            ",{} [{}:0] {}",
            if f.output { "output" } else { "input" },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    s.push_str(");\n");
    s.push_str(&binding());
    s.push_str("wire cp_ready=ram_available&&!reset;wire[35:0]lq,rq;assign cp_q={rq[31:0],lq[31:0]};wire[35:0]dl={{4{dma_data[31]}},dma_data[31:0]},dr={{4{dma_data[63]}},dma_data[63:32]};\niz_ram36 left_ram(clk,reset,1'b1,dma_we,dma_addr,dl,ce&&cp_ready&&cp_re,cp_addr,lq);\niz_ram36 right_ram(clk,reset,1'b1,dma_we,dma_addr,dr,ce&&cp_ready&&cp_re,cp_addr,rq);\n");
    s.push_str("gpu_command_processor processor(.clk(clk),.reset(reset),.ce(ce),.abort(abort),.load_count(load_count),.submit(submit),.control_data(device_write_data),.submit_ready(submit_ready),.busy(busy),.stop(stop),.render_idle(render_idle),.render_fault(render_fault),.memory_busy(router_busy),.meshlet_ready(1'b0),.meshlet_done(1'b0),.cp_re(cp_re),.cp_addr(cp_addr),.cp_q(cp_q),.cp_ready(cp_ready),.cache_valid(cache_valid),.cache_ready(cache_ready),.cache_source(cache_source),.cache_bank(cache_bank),.cache_done(cache_done),.cache_done_bank(cache_done_bank),.work_ready(work_ready),.work_busy(work_busy),.dma_idle(dma_idle),.dma_fault(dma_fault),.mesh_loading(mesh_loading),.mesh_begin(mesh_begin),.mesh_done(mesh_done),.mesh_slot(mesh_slot),.mesh_begin_slot(mesh_begin_slot),.mesh_done_slot(mesh_done_slot),.uniform_done(uniform_done),.uniform_bank(uniform_bank),.header_we(1'b0),.header_slot(1'b0),.header_data(64'd0),.flush_request(flush_request),.flush_complete(flush_complete),.clear_ready(1'b0),.clear_complete(1'b0),.clear_ok(1'b1),.ui_done(1'b0),.copy_ready(1'b0),.copy_complete(1'b0));\nassign phase=processor.state;assign pc=processor.pc;assign cache_pages=processor.cache_pages;\np1a transport(.clk(clk)");
    for f in transport::pins::fields(transport::Edge {
        input: Default::default(),
        output: Default::default(),
    }) {
        let net = if f.output
            || f.name.starts_with("ro_")
            || f.name.starts_with("fb_")
            || matches!(
                f.name,
                "reset" | "ce" | "cache_valid" | "cache_source" | "cache_bank"
            ) {
            f.name
        } else if f.name == "stop" {
            "stop||render_fault"
        } else {
            "0"
        };
        if net == "0" {
            write!(s, ",.{}({}'d0)", f.name, f.width).unwrap();
        } else {
            write!(s, ",.{}({net})", f.name).unwrap();
        }
    }
    s.push_str(");endmodule\n");
    s.push_str(&transport::pins::wrapper());
    s
}
fn tool(mut c: Command, dir: &Path, name: &str) -> std::process::ExitStatus {
    let mut child = c
        .current_dir(dir)
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
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{name} exceeded30s");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
pub fn rtl(root: &Path, label: &str, run: &Run, mutation: Option<&str>, contract: bool) {
    let dir = root.join(label);
    fs::create_dir(&dir).unwrap();
    let mut cp = include_str!("../../../src/system/command_processor.v").to_owned();
    if let Some(m) = mutation {
        let (old, new) = match m {
            "ce_submit" => (
                "if(submit&&submit_ready)begin",
                "if(ce&&submit&&submit_ready)begin",
            ),
            "early_fetch" => (
                "FETCH:if(cache_pages[pc[5]]&&cp_ready)",
                "FETCH:if(cp_ready)",
            ),
            "ignore_flush" => ("FLUSHING:if(flush_complete)", "FLUSHING:if(1'b1)"),
            "early_idle" => (
                "if(busy&&state==ENDING&&render_idle&&dma_idle&&!memory_busy)",
                "if(busy&&state==ENDING&&dma_idle&&!memory_busy)",
            ),
            _ => panic!("unknown DUT mutation"),
        };
        assert_eq!(cp.matches(old).count(), 1);
        cp = cp.replacen(old, new, 1);
    }
    let schema = fields(run.trace[0]);
    fs::write(dir.join("dut.v"),format!("`define PORTABLE\n`define GPU_V2_COMMAND_TRANSPORT\n{cp}\n{}\n{}\n`undef GPU_V2_COMMAND_TRANSPORT\n{}\n{}",include_str!("../../../src/system/command_dma.v"),include_str!("../../../src/system/vliw_memory.v"),include_str!("../../../src/frontend/vliw/primitives/ram.v"),wrapper(&schema))).unwrap();
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
    tb.push_str("p1b dut(.clk(clk)");
    for f in &schema {
        write!(tb, ",.{}({})", f.name, f.name).unwrap();
    }
    tb.push_str(");integer fd,n,k;initial begin fd=$fopen(\"vectors.txt\",\"r\");if(!fd)$fatal;\n");
    let columns: usize = schema.iter().map(|f| if f.output { 2 } else { 1 }).sum();
    writeln!(tb,"for(n=0;n<{};n=n+1)begin @(negedge clk);k=$fscanf(fd,\"{format}\\n\"{names});if(k!={columns})$fatal;#1;if(n>0)begin {checks} end @(posedge clk);#1;end $display(\"P1B_PASS edges={}\");$finish;end initial begin #{};$fatal(1,\"watchdog\");end endmodule",run.trace.len(),run.trace.len(),run.trace.len()*10+1000).unwrap();
    fs::write(dir.join("tb.v"), tb).unwrap();
    let mut vectors = String::new();
    for e in &run.trace {
        for f in fields(*e) {
            write!(vectors, "{:x} ", f.value).unwrap();
            if f.output {
                write!(vectors, "{} ", u8::from(f.care)).unwrap();
            }
        }
        vectors.push('\n');
    }
    fs::write(dir.join("vectors.txt"), vectors).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile.args(["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"]);
    assert!(
        tool(compile, &dir, "compile").success(),
        "{label}: {}",
        fs::read_to_string(dir.join("compile.err")).unwrap()
    );
    let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    execute.arg("test.vvp");
    let status = tool(execute, &dir, "run");
    let output = fs::read_to_string(dir.join("run.out")).unwrap();
    let success = mutation.is_none() && !contract;
    assert_eq!(status.success(), success, "{label}: {output}");
    if success {
        assert!(output.contains("P1B_PASS"));
    } else if contract {
        assert!(
            output.contains("CP ") && !output.contains("DIFF"),
            "{output}"
        );
    } else {
        assert!(
            output.contains("DIFF"),
            "compiled mutation must differ: {output}"
        );
    }
    fs::write(dir.join("verdict.json"),format!("{{\"label\":\"{label}\",\"edges\":{},\"success\":{},\"mutation\":{},\"contract\":{contract}}}\n",run.trace.len(),status.success(),mutation.is_some())).unwrap();
}

/// Isolate the actual production Device4 binding: raw errors would terminate
/// the transport monitors, so this is a status-leaf qualification, not recovery.
pub fn status_rtl(root: &Path, mutation: Option<&str>) {
    use gpu_v2::system::vliw_cycle::{command::Device4, Response};
    let label = mutation.unwrap_or("device4-status");
    let dir = root.join(label);
    fs::create_dir(&dir).unwrap();
    let mut source = binding();
    if let Some(m) = mutation {
        let (old, new) = match m {
            "read_clear" => (
                "else if((ro_response_valid",
                "else if(device_read_enable)transport_error<=0;else if((ro_response_valid",
            ),
            "ignore_valid" => ("ro_response_valid&&ro_response_error", "ro_response_error"),
            _ => panic!("unknown status DUT mutation"),
        };
        assert_eq!(source.matches(old).count(), 1);
        source = source.replacen(old, new, 1);
    }
    fs::write(dir.join("dut.v"), source.clone()).unwrap();
    let mut tb=String::from("`timescale 1ns/1ps\nmodule tb;reg clk=0;always #5 clk=~clk;reg reset,busy,device_read_enable,device_write_enable;reg[2:0]device_index;reg[3:0]device_channel;reg[15:0]device_write_data;wire[15:0]device_read_data;reg ro_response_valid,ro_response_error,fb_r_response_valid,fb_r_response_error,fb_w_response_valid,fb_w_response_error;\n");
    tb.push_str(&source);
    tb.push_str("\ninitial begin\n");
    let mut d = Device4::default();
    for wall in 0..20 {
        let reset = matches!(wall, 0 | 1 | 7 | 11 | 16);
        let busy = wall % 3 == 0;
        let bus = DeviceBus {
            index: if wall == 5 { 3 } else { 4 },
            channel: if wall == 6 { 1 } else { 2 },
            read: wall != 10,
            ..Default::default()
        };
        let mut physical = [Physical::default(); 3];
        for (at, port) in [(2, 0), (9, 1), (13, 2)] {
            if wall == at {
                physical[port].response = Response {
                    valid: true,
                    error: true,
                    ..Default::default()
                };
            }
        }
        if wall == 17 {
            physical[0].response.error = true;
        }
        let error = physical.map(|p| (u8::from(p.response.valid), u8::from(p.response.error)));
        writeln!(tb,"@(negedge clk);reset={};busy={};device_index={};device_channel={};device_read_enable={};device_write_enable=0;device_write_data=0;ro_response_valid={};ro_response_error={};fb_r_response_valid={};fb_r_response_error={};fb_w_response_valid={};fb_w_response_error={};#1;",u8::from(reset),u8::from(busy),bus.index,bus.channel,u8::from(bus.read),error[0].0,error[0].1,error[1].0,error[1].1,error[2].0,error[2].1).unwrap();
        if wall != 0 {
            writeln!(
                tb,
                "if(device_read_data!==16'd{})$fatal(1,\"DIFF status edge{wall}\");",
                d.read(bus, busy)
            )
            .unwrap();
        }
        tb.push_str("@(posedge clk);#1;\n");
        d.clock(reset, physical);
    }
    tb.push_str("$display(\"P1B_STATUS_PASS edges=20\");$finish;end initial begin #1000;$fatal(1,\"watchdog\");end endmodule\n");
    fs::write(dir.join("tb.v"), tb).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile.args(["-g2012", "-s", "tb", "-o", "test.vvp", "tb.v"]);
    assert!(
        tool(compile, &dir, "compile").success(),
        "{}",
        fs::read_to_string(dir.join("compile.err")).unwrap()
    );
    let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    execute.arg("test.vvp");
    let status = tool(execute, &dir, "run");
    let output = fs::read_to_string(dir.join("run.out")).unwrap();
    assert_eq!(status.success(), mutation.is_none(), "{output}");
    assert!(output.contains(if mutation.is_none() {
        "P1B_STATUS_PASS"
    } else {
        "DIFF"
    }));
    fs::write(dir.join("verdict.json"),format!("{{\"label\":\"{label}\",\"edges\":20,\"success\":{},\"mutation\":{},\"scope\":\"isolated Device4 status binding\"}}\n",status.success(),mutation.is_some())).unwrap();
}

pub const CONTRACT_CASES: &[&str] = &[
    "no_count",
    "count_reserved",
    "base_overflow",
    "busy_start",
    "busy_count",
    "bad_nop",
    "bad_wait",
    "bad_flush",
    "bad_end",
    "missing_fence",
    "missing_end",
    "early_end",
];

pub fn metrics(r: &Run) -> String {
    format!("{{\"logic_edges\":{},\"native_cycles\":{},\"submits\":{},\"completions\":{},\"cache_fetches\":{},\"ram_writes\":{},\"ce0_ram_writes\":{},\"commands\":{},\"ram_stall_edges\":{},\"flush_stall_edges\":{},\"ending_stall_edges\":{},\"abort_count\":{},\"ce0_retirements\":{}}}",r.trace.len(),r.native_cycles,r.stats.submits,r.stats.completed,r.stats.cache_fetches,r.stats.writes,r.stats.ce0_writes,r.stats.commands,r.stats.ram_stalls,r.stats.flush_stalls,r.stats.end_stalls,r.stats.aborts,r.stats.ce0_retirements)
}
