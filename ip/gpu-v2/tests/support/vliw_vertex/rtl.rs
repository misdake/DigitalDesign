use super::*;
use std::{
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub(crate) struct Pin {
    pub name: String,
    pub width: u32,
    pub value: u128,
    pub known: u128,
    pub output: bool,
}
pub(crate) fn schema(e: &Edge) -> Vec<Pin> {
    let mut p = Vec::new();
    let i = e.input;
    let o = e.output;
    let r = &e.regs;
    macro_rules! pin {
        ($name:expr,$width:expr,$v:expr,$out:expr) => {
            p.push(Pin {
                name: $name.into(),
                width: $width,
                value: ($v) as u128,
                known: (1u128 << $width) - 1,
                output: $out,
            });
        };
    }
    macro_rules! bits {
        ($name:expr,$width:expr,$v:expr) => {
            p.push(Pin {
                name: $name.into(),
                width: $width,
                value: u128::from(($v).value),
                known: u128::from(($v).known),
                output: true,
            });
        };
    }
    for (name, v) in [
        ("reset", i.reset),
        ("ce", i.ce),
        ("start", i.start),
        ("slot", i.slot),
        ("dma_we", i.dma_write),
        ("output_ready", i.output_ready),
        ("cp_re", i.cp_read),
    ] {
        pin!(name, 1, v, false);
    }
    pin!("dma_addr", 10, i.dma_address, false);
    pin!("dma_data", 64, i.dma_data, false);
    pin!("mesh_color", 32, i.mesh_color, false);
    pin!("cp_addr", 9, i.cp_address, false);
    for (name, v) in [
        ("ready", o.ready),
        ("busy", o.busy),
        ("done", o.done),
        ("drained", o.drained),
        ("dma_ready", o.dma_ready),
        ("cp_ready", o.cp_ready),
        ("emit_valid", o.core.emit_valid),
        ("core_busy", o.core.busy),
        ("done_v", o.core.done_v),
        ("source_ready", o.core.source_ready),
        ("epoch", r.epoch),
        ("bounds_safe", r.bounds_safe),
        ("position_write", o.core.position_write),
        ("attribute_write", o.core.attribute_write),
    ] {
        pin!(name, 1, v, true);
    }
    bits!("cp_q", 64, o.cp_q);
    bits!("emit_data", 36, o.core.emit_data);
    pin!("emit_addr", 10, o.core.emit_address, true);
    pin!("published_vertices", 7, o.core.published_vertices, true);
    pin!("position_address", 8, o.core.position_address & 255, true);
    pin!("attribute_address", 9, o.core.attribute_address, true);
    pin!("state", 4, e.phase as u8, true);
    pin!("active_slot", 1, e.slot, true);
    pin!("pc", 9, r.pc, true);
    pin!("control", 72, r.control, true);
    pin!("remaining", 7, r.remaining, true);
    pin!("vp", 11, r.vertex_pointer, true);
    pin!("op", 10, r.emit_pointer, true);
    pin!("vid", 6, r.vertex_id, true);
    pin!("vfield", 3, r.field, true);
    bits!("lq", 36, e.lq);
    bits!("rq", 36, e.rq);
    bits!("acc", 54, r.acc);
    bits!("pl", 54, r.pl);
    bits!("ph", 54, r.ph);
    bits!("mac_y", 54, e.y);
    bits!("pack_buffer", 54, r.packed);
    bits!("unpacked", 32, r.unpacked);
    bits!("color0", 32, e.colors[0]);
    bits!("color1", 32, e.colors[1]);
    pin!("input_active", 1, e.input_active, true);
    pin!("active_page", 1, e.monitored_page, true);
    for n in 0..16 {
        bits!(format!("rf{n}"), 32, e.rf[n]);
    }
    p
}
pub(crate) fn wrapper(p: &[Pin]) -> String {
    let mut s = String::from("module g2a(input clk");
    for f in p {
        write!(
            s,
            ",{}[{}:0]{}",
            if f.output { "output" } else { "input" },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    s.push_str(");\nshared_meshlet dut(.clk(clk),.reset(reset),.ce(ce),.start(start),.slot(slot),.mesh_color(mesh_color),.dma_we(dma_we),.dma_addr(dma_addr),.dma_data(dma_data),.output_ready(output_ready),.packet_ready(1'b0),.word_ready(1'b0),.cp_re(cp_re),.cp_addr(cp_addr),.cp_ready(cp_ready),.cp_q(cp_q),.ready(ready),.busy(busy),.done(done),.drained(drained),.dma_ready(dma_ready),.published_vertices(published_vertices),.emit_valid(emit_valid),.emit_addr(emit_addr),.emit_data(emit_data));\n");
    for (name, net) in [
        ("core_busy", "dut.core.busy"),
        ("done_v", "dut.core.done_v"),
        ("source_ready", "dut.core.source_ready"),
        ("epoch", "dut.core.vertex_epoch"),
        ("bounds_safe", "dut.core.bounds_safe"),
        ("position_write", "dut.core.vwrite&&dut.core.vfield<3"),
        ("attribute_write", "dut.core.vwrite&&dut.core.vfield>=3"),
        ("position_address", "{dut.core.vfield[1:0],dut.core.vid}"),
        ("attribute_address", "dut.core.export_wa"),
        ("state", "dut.state"),
        ("active_slot", "dut.active_slot"),
        ("pc", "dut.core.pc"),
        ("control", "dut.core.control"),
        ("remaining", "dut.core.remaining"),
        ("vp", "dut.core.vp"),
        ("op", "dut.core.op"),
        ("vid", "dut.core.vid"),
        ("vfield", "dut.core.vfield"),
        ("lq", "dut.core.lq"),
        ("rq", "dut.core.rq"),
        ("acc", "dut.core.acc"),
        ("pl", "dut.core.mac.pl"),
        ("ph", "dut.core.mac.ph"),
        ("mac_y", "dut.core.acc_next"),
        ("pack_buffer", "dut.core.packed_buffer"),
        ("unpacked", "dut.core.unpacked"),
    ] {
        writeln!(s, "assign {name}={net};").unwrap();
    }
    for n in 0..16 {
        writeln!(s, "assign rf{n}=dut.core.rf.mem[{n}];").unwrap();
    }
    s.push_str("assign color0=dut.core.epoch_color[0];assign color1=dut.core.epoch_color[1];\n");
    let parent = include_str!("../../../src/system/vliw_command.v");
    let owner = &parent[parent.find(" reg input_active,active_page;").unwrap()
        ..parent.find(" assign pipeline_idle").unwrap()];
    s.push_str(
        &owner
            .replace(" reg input_active,active_page;", "")
            .replace("meshlet_done", "done")
            .replace("if(start_fire)", "if(ce&&start&&ready)"),
    );
    // The outputs are registers in the unchanged parent ownership binding.
    s = s
        .replace("output[0:0]input_active", "output reg[0:0]input_active")
        .replace("output[0:0]active_page", "output reg[0:0]active_page");
    s.push_str("always @(posedge clk)if(!reset)begin\n");
    for line in parent.lines().filter(|s| {
        s.contains("if(dma_we&&!dma_ready)")
            || s.contains("if(cp_re&&input_active)")
            || s.contains("if(input_active&&dma_we&&dma_addr<256")
    }) {
        s.push_str(line);
        s.push('\n');
    }
    s.push_str("end\n");
    s.push_str("endmodule\n");
    s
}
pub fn root() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-vertex")
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
fn tool(mut c: Command, dir: &Path, label: &str, limit: u64) -> std::process::ExitStatus {
    let mut child = c
        .current_dir(dir)
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
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        if start.elapsed() > Duration::from_secs(limit) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{label} exceeded{limit}s");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
pub fn compare(root: &Path, label: &str, run: &Run) {
    qualify(root, label, run, None, false);
}
pub fn qualify(root: &Path, label: &str, run: &Run, mutation: Option<&str>, contract: bool) {
    let p = schema(&run.trace[0]);
    let mut source = production(&p);
    if let Some(m) = mutation {
        source = mutate(source, m);
    }
    let expected = if contract {
        Some(match label.strip_prefix("contract-").unwrap() {
            "bad_header" | "bad_uniform" => "GPU contract: invalid_header",
            "cp_owner" => "CP read while V/T owns input Q",
            "collision" => "iz_ram36跨口同址冲突",
            "bad_dma" | "active_payload" | "active_metadata" | "active_uniform" => {
                "DMA sink lost its statically independent write port"
            }
            _ => panic!("unknown contract {label}"),
        })
    } else {
        None
    };
    replay(
        root,
        label,
        run.trace.iter().map(schema).collect(),
        source,
        mutation.is_some(),
        expected,
    );
}
pub(crate) fn replay(
    root: &Path,
    label: &str,
    rows: Vec<Vec<Pin>>,
    source: String,
    difference: bool,
    expected: Option<&str>,
) {
    replay_bounded(root, label, rows, source, difference, expected, 30);
}
/// Whole DRAW has a separately approved sixty-second simulator budget.
#[allow(dead_code)] // Shared leaf test binaries intentionally use replay only.
pub(crate) fn replay_whole(
    root: &Path,
    label: &str,
    rows: Vec<Vec<Pin>>,
    source: String,
    difference: bool,
    expected: Option<&str>,
) {
    replay_bounded(root, label, rows, source, difference, expected, 60);
}
fn replay_bounded(
    root: &Path,
    label: &str,
    rows: Vec<Vec<Pin>>,
    source: String,
    difference: bool,
    expected: Option<&str>,
    limit: u64,
) {
    let dir = root.join(label);
    fs::create_dir(&dir).unwrap();
    let p = &rows[0];
    fs::write(dir.join("dut.v"), source).unwrap();
    let mut tb = String::from(
        "`timescale 1ns/1ps\nmodule tb;reg clk=0;always #5 clk=~clk;integer fd,n,k,b;\n",
    );
    let mut fmt = String::new();
    let mut names = String::new();
    let mut checks = String::new();
    for f in p {
        if f.output {
            writeln!(
                tb,
                "wire[{}:0]{};reg[{}:0]e_{},m_{};",
                f.width - 1,
                f.name,
                f.width - 1,
                f.name,
                f.name
            )
            .unwrap();
            fmt.push_str("%h %h ");
            write!(names, ",e_{},m_{}", f.name, f.name).unwrap();
            writeln!(checks,"for(b=0;b<{};b=b+1)begin if(m_{}[b])begin if({}[b]!==e_{}[b])$fatal(1,\"DIFF {} edge%0d got%h expected%h mask%h\",n,{},e_{},m_{});end else if({}[b]===1'b0||{}[b]===1'b1)$fatal(1,\"KNOWN {} edge%0d bit%0d\",n,b);end",f.width,f.name,f.name,f.name,f.name,f.name,f.name,f.name,f.name,f.name,f.name).unwrap();
        } else {
            writeln!(tb, "reg[{}:0]{};", f.width - 1, f.name).unwrap();
            fmt.push_str("%h ");
            write!(names, ",{}", f.name).unwrap();
        }
    }
    tb.push_str("g2a top(.clk(clk)");
    for f in p {
        write!(tb, ",.{}({})", f.name, f.name).unwrap();
    }
    tb.push_str(");initial begin fd=$fopen(\"vectors.txt\",\"r\");if(!fd)$fatal;\n");
    let columns: usize = p.iter().map(|p| if p.output { 2 } else { 1 }).sum();
    writeln!(tb,"for(n=0;n<{};n=n+1)begin @(negedge clk);k=$fscanf(fd,\"{fmt}\\n\"{names});if(k!={columns})$fatal(1,\"vectors\");#1;if(n>0)begin {checks} end @(posedge clk);#1;end $display(\"G2A_PASS edges={}\");$finish;end initial begin #{};$fatal(1,\"watchdog\");end endmodule",rows.len(),rows.len(),rows.len()*10+1000).unwrap();
    fs::write(dir.join("tb.v"), tb).unwrap();
    let mut vectors = String::new();
    for row in &rows {
        for f in row {
            write!(vectors, "{:x} ", f.value).unwrap();
            if f.output {
                write!(vectors, "{:x} ", f.known).unwrap();
            }
        }
        vectors.push('\n');
    }
    fs::write(dir.join("vectors.txt"), vectors).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile.args(["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"]);
    assert!(
        tool(compile, &dir, "compile", 30).success(),
        "{}",
        fs::read_to_string(dir.join("compile.err")).unwrap()
    );
    let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    execute.arg("test.vvp");
    let status = tool(execute, &dir, "run", limit);
    let text = fs::read_to_string(dir.join("run.out")).unwrap();
    let contract = expected.is_some();
    let success = !difference && !contract;
    fs::write(dir.join("verdict.json"),format!("{{\"label\":\"{label}\",\"edges\":{},\"expected_success\":{success},\"actual_success\":{},\"contract_rejection\":{contract}}}\n",rows.len(),status.success())).unwrap();
    assert_eq!(status.success(), success, "{label}: {text}");
    if success {
        assert!(text.contains("G2A_PASS"));
    } else if contract {
        assert!(
            !text.contains("DIFF") && !text.contains("KNOWN"),
            "monitor must reject first: {text}"
        );
        assert!(
            text.contains(expected.unwrap()),
            "{label} expected {expected:?}: {text}"
        );
    } else {
        assert!(
            text.contains("DIFF") || text.contains("KNOWN"),
            "compiled DUT mutation must diverge: {text}"
        );
    }
}

fn mutate(mut s: String, m: &str) -> String {
    let changes:Vec<(&str,&str)>=match m {
 "ce_dma"=>vec![(".dma_we(dma_we&&dma_ready)",".dma_we(ce&&dma_we&&dma_ready)")],
 "ram_combinational"=>vec![("reg [35:0] memory[0:511];reg [35:0] rd;\n assign q=rd;","reg [35:0] memory[0:511];reg [35:0] rd;\n assign q=memory[ra];")],
 "rom_combinational"=>vec![("always @(posedge clk) if(reset)data<=0;else if(en)data<=mem[addr];","always @* if(reset)data=0;else if(en)data=mem[addr];")],
 "metadata_page"=>vec![("{7'd32,active_slot,metadata_word}","{7'd32,!active_slot,metadata_word}")],
 "acc_writeback"=>vec![("wire [31:0] acc_wb=acc[47:16];","wire [53:0] acc_next;wire [31:0] acc_wb=acc_next[47:16];"),("wire [53:0] acc_input=acc_clear?54'd0:acc;wire [53:0] acc_next;","wire [53:0] acc_input=acc_clear?54'd0:acc;")],
 "mac_pipeline"=>vec![("always @(posedge clk) begin\n   if(reset) begin pl<=0; ph<=0; end\n   else if(en) begin pl<=al_signed*b_signed; ph<=ah_signed*b_signed; end\n end","always @* begin if(reset)begin pl=0;ph=0;end else if(en)begin pl=al_signed*b_signed;ph=ah_signed*b_signed;end end")],
 "normal16_order"=>vec![("normal12(packed_normal,normal18)","normal12(pack_value[17:0],1'b1)")],
 "early_publish"=>vec![("published_vertices<=published_vertices+1'b1;",""),("if(vfield==4)begin vfield<=0;","if(vfield==3)published_vertices<=published_vertices+1'b1; if(vfield==4)begin vfield<=0;")],
 "bounds_disabled"=>vec![(".BOUNDS_CULL(BOUNDS_CULL)) core(",".BOUNDS_CULL(0)) core(")],
 "lane_swap"=>vec![("dl={{4{dma_data[31]}},dma_data[31:0]}","dl={{4{dma_data[63]}},dma_data[63:32]}"),("dr={{4{dma_data[63]}},dma_data[63:32]}","dr={{4{dma_data[31]}},dma_data[31:0]}")],
 "delayed_c"=>vec![("assign lo=pl+c;","reg[53:0]delayed_c;always@(posedge clk)if(reset)delayed_c<=0;else if(en)delayed_c<=c;assign lo=pl+delayed_c;")],
 "early_release"=>vec![("if(done)input_active<=0;","if(done_v)input_active<=0;")],
 "rf_reset"=>vec![("module shared_rf(input clk,we,","module shared_rf(input clk,reset,we,"),("shared_rf rf(.clk(clk),.we(","shared_rf rf(.clk(clk),.reset(reset),.we("),("always @(posedge clk) if(we) mem[wa]<=wd;","always @(posedge clk) if(reset)for(i=0;i<16;i=i+1)mem[i]<=0;else if(we) mem[wa]<=wd;")],
 _=>panic!("unknown actual DUT mutation {m}")
 };
    let begin = if m == "lane_swap" {
        s.find("module rs_static_scratchpad").unwrap()
    } else {
        0
    };
    let mut end = if m == "lane_swap" {
        begin + s[begin..].find("endmodule").unwrap()
    } else {
        s.len()
    };
    for (old, new) in changes {
        let target = &s[begin..end];
        assert_eq!(
            target.matches(old).count(),
            1,
            "mutant {m} exact source anchor {old}"
        );
        let changed = target.replacen(old, new, 1);
        s.replace_range(begin..end, &changed);
        end = begin + changed.len();
    }
    s
}

fn production(p: &[Pin]) -> String {
    let mut source=String::from("`define GPU_V2_COMMAND_PORTS\n`define GPU_V2_TRUSTED_FRONTEND\n`define GPU_V2_LIVE_GLOBALS\n");
    source.push_str(&gpu_v2::frontend::vliw::verilog(true).replace("\r\n", "\n"));
    source.push_str(&wrapper(p));
    source
}
pub const MUTATIONS: [&str; 13] = [
    "ce_dma",
    "ram_combinational",
    "rom_combinational",
    "metadata_page",
    "acc_writeback",
    "mac_pipeline",
    "normal16_order",
    "early_publish",
    "bounds_disabled",
    "rf_reset",
    "lane_swap",
    "delayed_c",
    "early_release",
];
pub fn audit_mutation_anchors(run: &Run) {
    let s = production(&schema(&run.trace[0]));
    for m in MUTATIONS {
        let _ = mutate(s.clone(), m);
    }
}
