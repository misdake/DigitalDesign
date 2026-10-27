//! Compiled RCC CPU recording, then separate bounded native GPU replay.
use super::{compile, CpuV3Sim, RunOutcome};
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::VerilogProject;
use std::{
    any::Any,
    cell::RefCell,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct State {
    pub(super) kind: u16,
    pub(super) matrices: Vec<Vec<u16>>,
    pub(super) counts: Vec<u16>,
    pub(super) starts: usize,
    pub(super) completions: usize,
    pub(super) maintenance: usize,
    pub(super) objects: Vec<Vec<u16>>,
    pub(super) commands: Vec<Vec<u16>>,
}
pub(super) struct Peer {
    pub(super) device: u8,
    pub(super) state: Rc<RefCell<State>>,
}
impl cpu_v3::Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        // Read-only Display ownership fixture: active Front A, RGB565, no Pending.
        // This peer does not scan pixels, model frame boundaries or accept present.
        if self.device == 3 && channel == 2 {
            0x20
        } else {
            0
        }
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut s = self.state.borrow_mut();
        match (self.device, channel) {
            (3, _) => panic!("static fixture must not present or change Display"),
            (0, 1) | (0, 4) => s.maintenance += 1,
            (4, 0) => s.counts.push(value),
            (4, 1) => {
                assert_eq!(value, 0x9200);
                s.starts += 1;
                let words = (usize::from(*s.counts.last().unwrap()) + 1) * 2;
                let padded_words = words.next_multiple_of(64);
                s.commands
                    .push(memory[0x490000 / 2..0x490000 / 2 + padded_words].to_vec());
                s.objects.push(memory[0x4a0000 / 2..0x4b0000 / 2].to_vec());
            }
            (7, 2) => s.kind = value,
            (7, 1) => {
                let mut row = vec![s.kind];
                row.extend_from_slice(&memory[usize::from(value)..usize::from(value) + 42]);
                s.matrices.push(row);
            }
            (7, 0) => {
                s.completions += 1;
                assert_eq!(usize::from(value), s.completions);
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn bounded_status(command: &mut Command, log: &Path) -> Output {
    let mut child = command
        .stdout(std::fs::File::create(log).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(1200) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("bounded static-scene tool timed out: {}", log.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut output = child.wait_with_output().unwrap();
    output.stdout = std::fs::read(log).unwrap();
    output
}

pub(super) fn bounded(command: &mut Command, log: &Path) -> Output {
    let output = bounded_status(command, log);
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn python(mode: &str, out: &Path) {
    let mut command = Command::new("python");
    command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/bloxorz/tools/static-scene.py"))
        .arg(mode)
        .arg(out);
    bounded(&mut command, &out.join(format!("{mode}.log")));
}

fn record(out: &Path) -> Vec<u16> {
    python("assets", out);
    let code = compile(include_str!("rcc/bloxorz_static.rs"));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code);
    let assets = std::fs::read(out.join("assets.bin")).unwrap();
    for (i, w) in assets.chunks_exact(2).enumerate() {
        cpu.physical_memory_mut()[0x4b0000 / 2 + i] = u16::from_le_bytes([w[0], w[1]]);
    }
    let state = Rc::new(RefCell::new(State::default()));
    for device in [0, 3, 4, 7] {
        cpu.attach_device(
            device,
            Box::new(Peer {
                device,
                state: state.clone(),
            }),
        );
    }
    let result = cpu.run(20_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    let s = state.borrow();
    assert_eq!((s.starts, s.completions, s.matrices.len()), (2, 2, 18));
    assert!(s.maintenance > 0);
    assert_eq!(s.counts.len(), 2);
    assert_eq!(s.counts[0], s.counts[1]);
    assert_eq!(
        s.commands[0], s.commands[1],
        "both actual CPU submissions match"
    );
    assert_eq!(
        s.objects[0], s.objects[1],
        "identical static scenes retain identical objects"
    );
    std::fs::write(
        out.join("matrices.txt"),
        s.matrices
            .iter()
            .map(|row| {
                row.iter()
                    .map(|v| format!("{v:04x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
                    + "\n"
            })
            .collect::<String>(),
    )
    .unwrap();
    let bytes = (usize::from(s.counts[0]) + 1) * 4;
    let padded = bytes.next_multiple_of(128);
    let cb = &cpu.physical_memory_mut()[0x490000 / 2..(0x490000 + padded) / 2];
    assert_eq!(cb, s.commands[0]);
    assert!(cb[bytes / 2..].iter().all(|w| *w == 0));
    std::fs::write(
        out.join("cpu-commandbuffer.bin"),
        cb.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(
        out.join("cpu-commandbuffer.hex"),
        cb.iter().map(|w| format!("{w:04x}\n")).collect::<String>(),
    )
    .unwrap();
    std::fs::write(out.join("submit-count.txt"), s.counts[0].to_string()).unwrap();
    let mut seed = format!("@{:x}\n", 0x490000 / 8);
    for q in cb.chunks_exact(4) {
        let value = q
            .iter()
            .enumerate()
            .map(|(i, w)| u64::from(*w) << (16 * i))
            .sum::<u64>();
        seed += &format!("{value:016x}\n");
    }
    seed += &format!("@{:x}\n", 0x4a0000 / 8);
    for q in s.objects[0].chunks_exact(4) {
        let value = q
            .iter()
            .enumerate()
            .map(|(i, w)| u64::from(*w) << (16 * i))
            .sum::<u64>();
        seed += &format!("{value:016x}\n");
    }
    std::fs::write(out.join("cpu-gpu-input.hex"), seed).unwrap();
    std::fs::write(out.join("cpu-recording.txt"),format!("{result:?}\ncode_words={} commands_bytes={bytes} padded_bytes={padded} draws=18\ndefault optimizer; control peer does not render\n",code.len())).unwrap();
    python("golden", out);
    std::fs::write(
        out.join("program.hex"),
        code.iter()
            .map(|w| format!("{w:04x}\n"))
            .collect::<String>(),
    )
    .unwrap();
    code
}

const SURFACE: &str = r#"
task check_surface;input[15:0]frame;integer q,x,y,off,half,cd,dd,diff,fc,fd;reg[63:0]expected;reg[15:0]actual,want;begin
 cd=0;dd=0;
 for(q='h400000/8-16;q<'h4b0000/8+16;q=q+1)begin
   expected=64'h55aa55aa55aa55aa;
   if(q>='h430000/8&&q<('h430000+192000)/8)expected=mem[q];
   else if(q>='h460000/8&&q<('h460000+192000)/8)expected=mem[q];
   else if((q>='h490000/8&&q<('h490000+((cb_bytes+127)/128)*128)/8)||(q>='h4a0000/8&&q<'h4b0000/8))expected=mem[q];
   if(q>='h4b0000/8)expected=asset_initial[q-'h4b0000/8];
   if(mem[q]!==expected)$fatal(1,"Front/padding/neighbour mismatch q=%h",q);
 end
 for(q=0;q<528;q=q+1)if(mem['h4b0000/8+q]!==asset_initial[q])$fatal(1,"Immutable asset changed");
 for(q=0;q<((cb_bytes+127)/128)*128;q=q+2)
   if(mem[('h490000+q)/8][(q%8)*8+:16]!==goldcb[q/2])$fatal(1,"Submitted CPU CB changed at byte %0d",q);
 fc=$fopen("__ACTUAL_COLOR__","w");fd=$fopen("__ACTUAL_DEPTH__","w");
 for(y=0;y<240;y=y+1)for(x=0;x<400;x=x+1)begin
   off=((y/16)*25+x/16)*512+(y%16)*32+(x%16)*2;half=off%8*8;
   actual=mem[('h430000+off)/8]>>half;want=frame==1?gold1[y*400+x]:gold2[y*400+x];
   $fdisplay(fc,"%04h",actual);
   if(actual!==want)begin if(cd<8)$display("COLOR_DIFF x=%0d y=%0d actual=%h expected=%h",x,y,actual,want);cd=cd+1;end
   actual=mem[('h460000+off)/8]>>half;want=frame==1?depth1[y*400+x]:depth2[y*400+x];
   $fdisplay(fd,"%04h",actual);diff=actual>want?actual-want:want-actual;
   if((^actual===1'bx)||(want==16'hffff&&actual!==want)||(want!=16'hffff&&diff>1))begin if(dd<8)$display("DEPTH_DIFF x=%0d y=%0d actual=%h expected=%h",x,y,actual,want);dd=dd+1;end
 end
 $fclose(fc);$fclose(fd);
 if(cd||dd)$fatal(1,"Bloxorz surface mismatch frame=%0d color=%0d depth=%0d",frame,cd,dd);
end endtask
"#;

fn gpu_bench(out: &Path, frames: usize, draws: usize, assets: usize, slots: bool) -> String {
    let original = include_str!("../../tests/gpu_rcc_draw_tb.v");
    let gpu_start = original.find("wire frr,").unwrap();
    let gpu_end = original.find("// Cache burst memory").unwrap();
    let memory_start = original.find("integer ro_active=").unwrap();
    let memory_end = original.find("integer starts=").unwrap();
    let mut tb = format!(
        r#"`timescale 1ns/1ps
module tb;
reg clk=0,reset=1;always #5 clk=~clk;GSR GSR(.GSRI(1'b1));
integer cycle=0,idx,frame;
reg[63:0]mem[0:1048575],asset_initial[0:527];
reg[15:0]gold1[0:95999],gold2[0:95999],depth1[0:95999],depth2[0:95999],goldcb[0:32767];
reg[2:0]di=4;reg[3:0]ch=2;reg ren=1,wen=0;reg[15:0]wdata=0;wire[15:0]gdata;
integer cb_bytes=__CB_BYTES__;
{}
{}
{}
always @(posedge clk)begin
 cycle<=cycle+1;if(cycle>3000000)$fatal(1,"GPU scene watchdog");
 if(cycle%20000==0)begin $display("GPU_SCENE cycle=%0d state=%0d writes=%0d",cycle,gpu.processor.state,writes);$fflush();end
end
task send;input[3:0]channel;input[15:0]value;begin
 @(negedge clk);ch=channel;wdata=value;wen=1;ren=0;
 @(negedge clk);wen=0;ren=1;ch=2;
end endtask
initial begin
 for(idx='h400000/8-16;idx<'h4b1080/8+16;idx=idx+1)mem[idx]=64'h55aa55aa55aa55aa;
 $readmemh("__ASSETS__",mem);$readmemh("__CPU_INPUT__",mem);
 for(idx=0;idx<528;idx=idx+1)asset_initial[idx]=mem['h4b0000/8+idx];
 $readmemh("__GOLD1__",gold1);$readmemh("__GOLD2__",gold2);$readmemh("__DEPTH1__",depth1);$readmemh("__DEPTH2__",depth2);$readmemh("__GOLD_CB__",goldcb,0,((cb_bytes+127)/128)*64-1);
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 repeat(5)@(posedge clk);
 for(frame=1;frame<=2;frame=frame+1)begin
  send(0,cb_bytes/4-1);send(1,'h9200);
  #1;if(!gdata[0])$fatal(1,"GPU did not accept captured CPU submission");
  while(gdata[0])begin @(posedge clk);#1;end
  if(gdata[1])$fatal(1,"GPU transport error");
  if(wa||ro_active||fr_active||terms!=writes||draws!=frame*9)$fatal(1,"Completion before drain");
  check_surface(frame);
  $display("BLOX_FRAME_PASS frame=%0d cycles=%0d draws=%0d",frame,cycle,draws);$fflush();
 end
 if(draws!=18||writes<6000||terms!=writes||blocked==0||ce0==0||slot0==0||fb_reads==0)$fatal(1,"GPU scene coverage");
 $display("BLOX_GPU_PASS cycles=%0d draws=%0d writes=%0d terminal=%0d",cycle,draws,writes,terms);$finish;
end
endmodule
"#,
        &original[gpu_start..gpu_end],
        &original[memory_start..memory_end],
        SURFACE
    );
    tb = tb
        .replace("[0:527]", &format!("[0:{}]", assets / 8 - 1))
        .replace("q<528", &format!("q<{}", assets / 8))
        .replace("idx<528", &format!("idx<{}", assets / 8))
        .replace(
            "idx<'h4b1080/8+16",
            &format!("idx<('h4b0000+{assets})/8+16"),
        )
        .replace("frame<=2", &format!("frame<={frames}"))
        .replace("draws!=frame*9", &format!("draws!=frame*{draws}"))
        .replace(
            "draws!=18||writes<6000",
            &format!("draws!={}||writes<{}", frames * draws, frames * 3000),
        );
    if slots {
        tb = tb.replace(
            "||slot0==0||fb_reads==0",
            "||slot0==0||slot1==0||fb_reads==0",
        );
    }
    let count: usize = std::fs::read_to_string(out.join("submit-count.txt"))
        .unwrap()
        .parse()
        .unwrap();
    tb = tb.replace("__CB_BYTES__", &((count + 1) * 4).to_string());
    for (key, name) in [
        ("__ASSETS__", "assets.hex"),
        ("__CPU_INPUT__", "cpu-gpu-input.hex"),
        ("__GOLD1__", "gold1.hex"),
        ("__GOLD2__", "gold2.hex"),
        ("__DEPTH1__", "depth1.hex"),
        ("__DEPTH2__", "depth2.hex"),
        ("__GOLD_CB__", "cpu-commandbuffer.hex"),
        ("__ACTUAL_COLOR__", "actual-color.hex"),
        ("__ACTUAL_DEPTH__", "actual-depth.hex"),
    ] {
        tb = tb.replace(key, &out.join(name).to_string_lossy().replace('\\', "/"));
    }
    tb
}

#[test]
fn static_scene_default_cpu_recording() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bloxorz-static-native");
    std::fs::create_dir_all(&out).unwrap();
    record(&out);
    prepare_native(&out, 2, 9, false);
}

#[test]
#[ignore = "bounded compiled-CPU recording then native GPU replay; Python/Icarus/Gowin models"]
fn static_scene_cpu_recorded_native_gpu() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bloxorz-static-native");
    std::fs::create_dir_all(&out).unwrap();
    record(&out);
    native_replay(&out, 2, 9, false);
    println!("BLOXORZ_STATIC_PASS: CPU-recorded commands; separate native GPU replay");
}

pub(super) fn prepare_native(out: &Path, frames: usize, draws: usize, slots: bool) -> Vec<PathBuf> {
    let project = VerilogProject::generate::<CpuV3Gpu>().unwrap();
    let mut paths = Vec::new();
    for (i, (_, source)) in project
        .files
        .iter()
        .filter(|(_, s)| !s.contains("module tb"))
        .enumerate()
    {
        let path = out.join(format!("source_{i}.v"));
        std::fs::write(&path, source).unwrap();
        paths.push(path);
    }
    let tb = out.join("tb.v");
    std::fs::write(
        &tb,
        gpu_bench(
            out,
            frames,
            draws,
            std::fs::metadata(out.join("assets.bin")).unwrap().len() as usize,
            slots,
        ),
    )
    .unwrap();
    paths
}

pub(super) fn native_replay(out: &Path, frames: usize, draws: usize, slots: bool) {
    let paths = prepare_native(out, frames, draws, slots);
    let tb = out.join("tb.v");
    let exe = out.join("scene.vvp");
    let mut command =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    command
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(&exe)
        .arg(
            PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                .join("IDE/simlib/gw2a/prim_sim.v"),
        )
        .args(paths)
        .arg(tb);
    bounded(&mut command, &out.join("compile.log"));
    let mut sim = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    sim.arg(exe);
    let result = bounded(&mut sim, &out.join("run.log"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("BLOX_GPU_PASS"));
    python("preview", out);
}

#[test]
fn depth_guard_rejects_unknown_bits() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bloxorz-depth-guard");
    std::fs::create_dir_all(&out).unwrap();
    let line = SURFACE
        .lines()
        .find(|s| s.contains("want==16'hffff&&actual"))
        .unwrap();
    let condition = line
        .split("if(")
        .nth(1)
        .unwrap()
        .split(")begin")
        .next()
        .unwrap();
    let bench = format!(
        r#"module tb;reg[15:0]actual,want;integer diff,which;
initial begin
 if(!$value$plusargs("CASE=%d",which))$fatal(1,"missing case");
 want=16'h1234;actual=16'h1235;
 case(which) 1:actual=16'hxxxx;2:actual=16'h12z4;3:begin actual=16'hxxxx;want=16'hffff;end endcase
 diff=actual>want?actual-want:want-actual;
 if({condition})$fatal(1,"DEPTH_REJECTED");
 $display("DEPTH_ACCEPTED");$finish;end
initial begin #10;$fatal(1,"watchdog");end endmodule"#
    );
    let tb = out.join("tb.v");
    std::fs::write(&tb, bench).unwrap();
    let exe = out.join("test.vvp");
    let mut compiler =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compiler
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(&exe)
        .arg(tb);
    bounded(&mut compiler, &out.join("compile.log"));
    for case in 0..4 {
        let mut sim = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        sim.arg(&exe).arg(format!("+CASE={case}"));
        let result = bounded_status(&mut sim, &out.join(format!("case-{case}.log")));
        assert_eq!(result.status.success(), case == 0, "case {case}");
        let marker = if case == 0 {
            "DEPTH_ACCEPTED"
        } else {
            "DEPTH_REJECTED"
        };
        assert!(String::from_utf8_lossy(&result.stdout).contains(marker));
    }
}
