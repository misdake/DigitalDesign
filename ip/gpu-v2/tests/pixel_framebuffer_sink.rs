//! Held Final lanes through actual four-bank framebuffer/ROP and real transport.
use gpu_v2::{
    framebuffer::ports::{Blend, Context, DepthFunc, Fragment, Header, MaterializedSurface, Quad},
    system::pixel::framebuffer_sink,
};
use std::{fs, path::Path, process::Command, time::Duration};

fn surface() -> MaterializedSurface {
    MaterializedSurface {
        color_base_bytes: 0,
        depth_base_bytes: 16384,
        width: 160,
        height: 48,
    }
}
fn image() -> Vec<u8> {
    (0..32768)
        .map(|i| ((i as u8).wrapping_mul(37).wrapping_add(11)) ^ ((i >> 3) as u8))
        .collect()
}
fn cases() -> Vec<(Quad, Context)> {
    let initial = image();
    (0..40)
        .map(|i| {
            let tile = i % 12;
            let x = (tile % 10 * 16 + (i % 8) * 2) as u16;
            let y = (tile / 10 * 16 + (i % 6) * 2) as u8;
            let q = Quad {
                header: Header {
                    x,
                    y,
                    mask: (i % 15 + 1) as u8,
                },
                pixels: std::array::from_fn(|lane| {
                    let wx = usize::from(x) + lane % 2;
                    let wy = usize::from(y) + lane / 2;
                    let offset = ((wy / 16 * 10 + wx / 16) * 256 + wy % 16 * 16 + wx % 16) * 2;
                    let a = 16384 + offset;
                    let old = u16::from_le_bytes([initial[a], initial[a + 1]]);
                    Fragment {
                        rgba: [
                            (i as u8).wrapping_mul(37).wrapping_add(lane as u8 * 53),
                            (i as u8).wrapping_mul(19).wrapping_add(lane as u8 * 31),
                            255u8.wrapping_sub(i as u8).wrapping_sub(lane as u8 * 17),
                            [0, 255, 97, 173][lane],
                        ],
                        depth: match lane {
                            0 => old,
                            1 => old.saturating_sub(1),
                            2 => old.saturating_add(1),
                            _ => (i * 7 + 1000) as u16,
                        },
                    }
                }),
            };
            let depth = [
                DepthFunc::Never,
                DepthFunc::Less,
                DepthFunc::Equal,
                DepthFunc::LessEqual,
                DepthFunc::Greater,
                DepthFunc::NotEqual,
                DepthFunc::GreaterEqual,
                DepthFunc::Always,
            ][i % 8];
            (
                q,
                Context {
                    depth,
                    depth_write: i % 3 != 0,
                    blend: if i % 2 == 0 {
                        Blend::Replace
                    } else {
                        Blend::SrcOver
                    },
                },
            )
        })
        .collect()
}

// Independent whole-byte reference uses real-number blending/quantization,
// never the cache calendar, framebuffer arithmetic helpers or DUT bank mapping.
fn gold(cases: &[(Quad, Context)], mut bytes: Vec<u8>) -> Vec<u8> {
    for (q, c) in cases {
        for lane in 0..4 {
            if q.header.mask & (1 << lane) == 0 {
                continue;
            }
            let x = usize::from(q.header.x) + lane % 2;
            let y = usize::from(q.header.y) + lane / 2;
            let offset = ((y / 16 * 10 + x / 16) * 256 + y % 16 * 16 + x % 16) * 2;
            let d = 16384 + offset;
            let old_c = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            let old_d = u16::from_le_bytes([bytes[d], bytes[d + 1]]);
            let f = q.pixels[lane];
            if ![
                false,
                f.depth < old_d,
                f.depth == old_d,
                f.depth <= old_d,
                f.depth > old_d,
                f.depth != old_d,
                f.depth >= old_d,
                true,
            ][c.depth as usize]
            {
                continue;
            }
            let codes = [old_c >> 11, old_c >> 5 & 63, old_c & 31];
            let mut color = 0u16;
            for (ch, code) in codes.into_iter().enumerate() {
                let bits = if ch == 1 { 6 } else { 5 };
                let destination = ((code << (8 - bits)) + (code >> (2 * bits - 8))) as f64;
                let blended = if c.blend == Blend::Replace {
                    f.rgba[ch] as f64
                } else {
                    (f.rgba[3] as f64 / 255.0 * f.rgba[ch] as f64
                        + (1.0 - f.rgba[3] as f64 / 255.0) * destination)
                        .round()
                };
                color |=
                    ((blended * ((1 << bits) - 1) as f64 / 255.0).round() as u16) << [11, 5, 0][ch];
            }
            bytes[offset..offset + 2].copy_from_slice(&color.to_le_bytes());
            if c.depth_write {
                bytes[d..d + 2].copy_from_slice(&f.depth.to_le_bytes());
            }
        }
    }
    bytes
}
fn bounded(mut command: Command, dir: &Path, label: &str) {
    use std::{process::Stdio, time::Instant};
    command
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{label}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{label}.err"))).unwrap(),
        ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{} {}",
                fs::read_to_string(dir.join(format!("{label}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{label}.err"))).unwrap()
            );
            break;
        }
        let limit = if label == "compile" { 120 } else { 60 };
        if start.elapsed() > Duration::from_secs(limit) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("process watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run(
    name: &str,
    cases: &[(Quad, Context)],
    bad_lane: bool,
    bad_mask: bool,
    fail_read: bool,
    closed_depth: bool,
) -> (Vec<u8>, Vec<u32>, String) {
    run_profile(
        name,
        cases,
        bad_lane,
        bad_mask,
        fail_read,
        u8::from(closed_depth),
    )
}

fn run_profile(
    name: &str,
    cases: &[(Quad, Context)],
    bad_lane: bool,
    bad_mask: bool,
    fail_read: bool,
    profile: u8,
) -> (Vec<u8>, Vec<u32>, String) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-framebuffer-sink-20261010")
        .join(name);
    fs::create_dir_all(&dir).unwrap();
    let mut rows = String::new();
    let bad_quad = if name.contains("late-owned") { 3 } else { 1 };
    for (qi, (q, c)) in cases.iter().enumerate() {
        for (r, data) in q.rows().into_iter().enumerate() {
            let mask = if bad_mask && qi == if name.contains("cold-invalid") { 0 } else { 1 } {
                0
            } else {
                q.header.mask
            };
            let w = u64::from(data)
                | ((r as u64) << 32)
                | (u64::from(mask) << 35)
                | (u64::from(q.header.y) << 39)
                | (u64::from(q.header.x) << 47)
                | ((c.depth as u64) << 56)
                | (u64::from(c.blend == Blend::SrcOver) << 59)
                | (u64::from(c.depth_write) << 60)
                | (u64::from(bad_lane && qi == bad_quad) << 61);
            rows.push_str(&format!("{w:016x}\n"));
        }
    }
    let original = include_str!("fixtures/framebuffer_cache_actual_tb.v");
    let start = original.find("    framebuffer_cache #(").unwrap();
    let end = original.find("    // ---- External memory model").unwrap();
    let instance = r#"
    wire ce=(cyc%5!=0)&&(cyc%7!=0);
    wire lane_ready,quad_accepted;
    gpu_v2_pixel_framebuffer_sink dut(.clk(clk),.reset(reset),.ce(ce),
      .lane_valid(row_valid),.lane(row_index[2:1] ^ {1'b0,inject_bad}),
      .rgba(stim_data),.depth(stim[si+1][15:0]),.x(row_x),.y(row_y),.mask(row_mask),
      .depth_func(ctx_func),.depth_write(ctx_dw),.blend(ctx_blend),
      .lane_ready(lane_ready),.lane_accept(row_accept),.quad_accepted(quad_accepted),
      .commit_valid(commit_valid),.commit_x(commit_x),.commit_y(commit_y),.commit_mask(commit_mask),
      .flush_request(flush_request),.flush_complete(flush_complete),.fault(fault),.idle(idle),
      .mem_req_valid(mem_req_valid),.mem_req_write(mem_req_write),.mem_req_addr(mem_req_addr),
      .mem_req_ready(mem_req_ready),.mem_read_data(mem_read_data),.mem_read_valid(mem_read_valid),
      .mem_write_valid(mem_write_valid),.mem_write_data(mem_write_data),.mem_write_ready(mem_write_ready),
      .mem_complete_valid(mem_complete_valid),.mem_complete_ok(mem_complete_ok));
    integer admitted=0,ready_ce0=0,transport_ce0=0;
    always @(posedge clk)begin
      if(!reset)begin
        if(row_accept && (!ce || !lane_ready))$fatal(1,"lane accept without raw-ready/CE");
        if(ce&&lane_ready&&row_valid&&!row_accept&&!fault)$fatal(1,"reserved depth cannot ACK");
        if(!ce&&lane_ready)ready_ce0=ready_ce0+1;
        if(!ce&&(mem_read_valid||(mem_write_valid&&mem_write_ready)))transport_ce0=transport_ce0+1;
        if(quad_accepted)begin
          if(row_index!=6||!row_accept)$fatal(1,"retirement is actual lane3 depth ACK");
          admitted=admitted+1;
        end
        if(flush_complete&&!fault)begin
          if(admitted!=STIM/8||ready_ce0==0||transport_ce0==0)$fatal(1,"coverage");
          $display("SINK accepted%0d rawreadyCE0%0d transportCE0%0d walls%0d",admitted,ready_ce0,transport_ce0,cyc);
        end
      end
    end
"#;
    let mut tb = format!("{}{}{}", &original[..start], instance, &original[end..]);
    if profile == 3 {
        // Actual enabled-edge lifetime witness: every terminal return occurs
        // after all four old operands have been accepted by the real leaf.
        let end = tb.rfind("endmodule").unwrap();
        tb.insert_str(
            end,
            r#"
    reg [2:0] old_operands_accepted=0;
    always @(posedge clk) if(!reset&&ce&&!fault) begin
      if(dut.cache.rst==dut.cache.RS_CAP)old_operands_accepted<=0;
      if(dut.cache.leaf_in_valid)old_operands_accepted<=old_operands_accepted+1'b1;
      if(dut.cache.leaf_out_valid &&
         (old_operands_accepted!=4 || dut.cache.rst!=dut.cache.RS_WAIT))
        $fatal(1,"dead-row reuse before all four actual leaf inputs");
    end
"#,
        );
    }
    if profile == 6 {
        // Source scoreboard follows public commits rather than DUT bank or
        // queue indexes. CE0 must not create a duplicate stage-1 acceptance.
        let end = tb.rfind("endmodule").unwrap();
        tb.insert_str(
            end,
            r#"
    integer source_quad=0,source_lane=0,source_inputs=0;
    always @(posedge clk) if(!reset) begin
      if(commit_valid)begin
        if(source_lane!=4)$fatal(1,"commit before four source pairs");
        source_quad<=source_quad+1;source_lane<=0;
      end
      if(ce&&!fault&&dut.cache.leaf_in_valid&&dut.cache.leaf_in_ready)begin
        if(dut.cache.leaf_in_key!=source_lane)$fatal(1,"source pair order");
        if({dut.cache.leaf_src_a,dut.cache.leaf_src_b,dut.cache.leaf_src_g,dut.cache.leaf_src_r}
             !==stim[source_quad*8+source_lane*2][31:0])$fatal(1,"RGBA paid return");
        if(dut.cache.leaf_src_depth!==stim[source_quad*8+source_lane*2+1][15:0])
          $fatal(1,"depth paid return");
        source_lane<=source_lane+1;source_inputs<=source_inputs+1;
      end
      if(ce&&!fault&&dut.cache.leaf_out_valid&&source_lane!=4)
        $fatal(1,"return before all four actual inputs");
      if(flush_complete&&!fault&&source_inputs!=STIM/2)$fatal(1,"source input count");
    end
"#,
        );
    }
    if profile >= 7 {
        let end = tb.rfind("endmodule").unwrap();
        tb.insert_str(end, r#"
    integer source_quad=0,source_lane=0,return_quad=0,return_lane=0;
    integer commit_seen=0,enabled_edges=0,last_commit=-1;
    always @(posedge clk) if(!reset) begin
      if(fault&&idle&&dut.cache.ahead!=0)$fatal(1,"fault retained borrowed owner phase");
      if(ce)enabled_edges=enabled_edges+1;
      if(commit_valid)begin
        if(return_quad!=commit_seen+1 || return_lane!=0)$fatal(1,"all-results commit fence");
        if(last_commit>=0)$display("RESIDENT commit%0d enabled_interval%0d wall%0d",commit_seen,enabled_edges-last_commit,cyc);
        last_commit=enabled_edges;commit_seen=commit_seen+1;
      end
      if(ce&&!fault&&dut.cache.leaf_in_valid&&dut.cache.leaf_in_ready)begin
        if(dut.cache.leaf_in_key!=source_lane)$fatal(1,"overlap source lane order");
        if({dut.cache.leaf_src_a,dut.cache.leaf_src_b,dut.cache.leaf_src_g,dut.cache.leaf_src_r}
           !==stim[source_quad*8+source_lane*2][31:0])$fatal(1,"overlap RGBA return");
        if(dut.cache.leaf_src_depth!==stim[source_quad*8+source_lane*2+1][15:0])
           $fatal(1,"overlap depth return");
        if(source_lane==3)begin source_lane=0;source_quad=source_quad+1;end
        else source_lane=source_lane+1;
      end
      if(ce&&!fault&&dut.cache.leaf_out_valid)begin
        if(return_quad!=commit_seen)$fatal(1,"next result before owner promotion");
        if(dut.cache.leaf_out_key!=return_lane)$fatal(1,"terminal ordered key");
        if(return_lane==3)begin return_lane=0;return_quad=return_quad+1;end
        else return_lane=return_lane+1;
      end
      if(ce&&!fault&&(dut.cache.rst==dut.cache.RS_WRCOL||dut.cache.rst==dut.cache.RS_WRDEP)
          &&dut.cache.got!=15)$fatal(1,"partial bank write before all results");
      if(flush_complete&&!fault&&source_quad!=STIM/8)$fatal(1,"overlap source count");
    end
"#);
    }
    if profile >= 11 {
        tb = tb.replace("dut.cache.ahead!=0", "dut.cache.depth_pending!=0")
            .replace("if(ce&&!fault&&dut.cache.leaf_out_valid)begin", "if(dut.cache.leaf_ce&&!fault&&dut.cache.leaf_out_valid)begin")
            .replace("      if(ce&&!fault&&(dut.cache.rst==dut.cache.RS_WRCOL||dut.cache.rst==dut.cache.RS_WRDEP)\n          &&dut.cache.got!=15)$fatal(1,\"partial bank write before all results\");", "      if(dut.cache.return_fire&&!dut.cache.slot_ready[dut.cache.head])$fatal(1,\"prefix return before publication\");");
        let end = tb.rfind("endmodule").unwrap();
        tb.insert_str(
            end,
            r#"
    integer exchanged_credit=0;
    always @(posedge clk)if(!reset)begin
      if(mem_req_valid&&dut.cache.rop_active&&!fault)$fatal(1,"maintenance overlaps unretired prefix owner");
      if(dut.cache.row0_fire&&dut.cache.slot_ready[dut.cache.tail])begin
        if(!dut.cache.depth_fire||dut.cache.depth_key!=3||dut.cache.tail!=dut.cache.head)
          $fatal(1,"full Output2 exchanged without old terminal write");
        exchanged_credit=exchanged_credit+1;
      end
      if(flush_complete&&!fault)$display("PREFIX atomic_exchange%0d",exchanged_credit);
    end
"#,
        );
    }
    if name.contains("/warm") {
        tb = tb
            .replace("wire ce=(cyc%5!=0)&&(cyc%7!=0);", "wire ce=1'b1;")
            .replace(
                "admitted!=STIM/8||ready_ce0==0||transport_ce0==0",
                "admitted!=STIM/8",
            );
    }
    if matches!(profile, 8 | 10 | 12) {
        tb = tb.replace("module tb;", "module tb;\n GSR GSR(.GSRI(1'b1));");
    }
    tb = tb
        .replace("si <= si + 1;", "si <= si + 2;")
        .replace(
            "if (si + 1 < STIM) cur <= stim[si + 1];",
            "if (si + 2 < STIM) cur <= stim[si + 2];",
        )
        .replace("    reg [31:0] reqs;", "")
        .replace(
            "wire        row_valid  = (si < STIM);",
            "reg [31:0] reqs;\n    wire row_valid = (si < STIM) && (!__BAD_WAIT__ || si < 8 || reqs != 0);",
        )
        .replace("__BAD_WAIT__", if bad_lane || bad_mask { "1" } else { "0" });
    if name.contains("prefix-bubbles") {
        assert_eq!(tb.matches("wire row_valid = (si < STIM)").count(), 1);
        tb = tb.replace(
            "wire row_valid = (si < STIM)",
            "wire row_valid = (si < STIM) && (si%8==0 || cyc%11>=4)",
        );
    }
    if name.contains("pace15") {
        assert_eq!(tb.matches("wire row_valid = (si < STIM)").count(), 1);
        tb=tb.replace("wire row_valid = (si < STIM)",
            "integer next_quad_start=0;\n    wire row_valid = (si < STIM) && (si%8!=0||cyc>=next_quad_start)");
        let end = tb.rfind("endmodule").unwrap();
        tb.insert_str(
            end,
            "\n always @(posedge clk) if(!reset&&quad_accepted) next_quad_start<=cyc+8;\n",
        );
    }
    if name.contains("bad-write-edge") {
        assert_eq!(tb.matches("wire row_valid = (si < STIM)").count(), 1);
        tb = tb.replace(
            "wire row_valid = (si < STIM)",
            "wire row_valid = ((si < STIM) || (dut.cache.rst==dut.cache.RS_WRDEP))",
        );
        tb = tb.replace(
            "        end else if (!done) begin",
            "        end else if (1'b0) begin",
        );
    }
    for (key, value) in [
        ("__STIM__", (cases.len() * 8).to_string()),
        ("__BYTES__", "32768".into()),
        ("__MAXC__", (cases.len() + 8).to_string()),
        ("__COLOR__", "0".into()),
        ("__DEPTH__", "16384".into()),
        ("__WIDTH__", "160".into()),
        ("__HEIGHT__", "48".into()),
        ("__CE__", "0".into()),
        (
            "__FAIL_READ__",
            if fail_read { "1".into() } else { "0".into() },
        ),
        ("__FAIL_WRITE__", "0".into()),
        ("__STALL__", "1".into()),
        ("__READ_ON_ACCEPT__", "1".into()),
        ("__WATCH__", "75000".into()),
    ] {
        tb = tb.replace(key, &value);
    }
    fs::write(
        dir.join("sink.v"),
        match profile {
            0 => framebuffer_sink::verilog(surface()).unwrap(),
            1 => framebuffer_sink::verilog_closed_depth(surface()).unwrap(),
            2 => framebuffer_sink::verilog_closed_bank_depth(surface()).unwrap(),
            3 => framebuffer_sink::verilog_closed_bank_reuse_depth(surface()).unwrap(),
            4 => framebuffer_sink::verilog_closed_bank_block(surface()).unwrap(),
            5 => framebuffer_sink::verilog_closed_bank_color(surface()).unwrap(),
            6 => framebuffer_sink::verilog_closed_bank_stream(surface()).unwrap(),
            7 => framebuffer_sink::verilog_closed_bank_overlap(surface()).unwrap(),
            8 => gpu_v2::system::pixel::framebuffer_native::bind_products(
                framebuffer_sink::verilog_closed_bank_overlap(surface()).unwrap(),
            ),
            9 => framebuffer_sink::verilog_closed_bank_overlap_commit(surface()).unwrap(),
            10 => gpu_v2::system::pixel::framebuffer_native::bind_products(
                framebuffer_sink::verilog_closed_bank_overlap_commit(surface()).unwrap(),
            ),
            11 => framebuffer_sink::verilog_closed_bank_prefix(surface()).unwrap(),
            12 => gpu_v2::system::pixel::framebuffer_native::bind_products(
                framebuffer_sink::verilog_closed_bank_prefix(surface()).unwrap(),
            ),
            _ => panic!("unknown sink profile"),
        },
    )
    .unwrap();
    fs::write(dir.join("tb.v"), tb).unwrap();
    fs::write(dir.join("rows.hex"), rows).unwrap();
    fs::write(
        dir.join("mem.hex"),
        image()
            .iter()
            .map(|b| format!("{b:02x}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let mut compile = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
    compile
        .current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp", "sink.v", "tb.v"]);
    if matches!(profile, 8 | 10 | 12) {
        compile.arg(
            Path::new(&std::env::var_os("GOWIN_HOME").unwrap()).join("IDE/simlib/gw2a/prim_sim.v"),
        );
    }
    bounded(compile, &dir, "compile");
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap());
    run.current_dir(&dir).arg("test.vvp");
    bounded(run, &dir, "run");
    let output = fs::read_to_string(dir.join("run.out")).unwrap();
    assert!(
        output.contains("PASS") && !output.contains("FAIL"),
        "{output}"
    );
    let values = |name: &str| {
        fs::read_to_string(dir.join(name))
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with("//") && !l.trim().is_empty())
            .map(|l| u32::from_str_radix(l.trim(), 16).unwrap())
            .collect::<Vec<_>>()
    };
    (
        values("cache_out.hex")
            .into_iter()
            .map(|b| b as u8)
            .collect(),
        values("commits.hex")
            .into_iter()
            .filter(|c| *c != 0)
            .collect(),
        output,
    )
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn held_final_lanes_through_actual_framebuffer() {
    let cases = cases();
    let (bytes, commits, output) = run("depth-blend-flush", &cases, false, false, false, false);
    assert_eq!(bytes, gold(&cases, image()));
    assert_eq!(
        commits,
        cases
            .iter()
            .map(|(q, _)| (u32::from(q.header.x) << 12)
                | (u32::from(q.header.y) << 4)
                | u32::from(q.header.mask))
            .collect::<Vec<_>>()
    );
    assert!(output.contains("fault=0"));
    println!("{output}");
    for (name, lane, mask, fail) in [
        ("bad-lane", true, false, false),
        ("bad-mask", false, true, false),
        ("read-terminal-failure", false, false, true),
    ] {
        let (_, _, output) = run(name, &cases[..2], lane, mask, fail, false);
        assert!(output.contains("fault=1") && output.contains("FAULT reqs="));
        assert!(
            !output.contains("FAULT reqs=0 "),
            "fault must drain real accepted transport"
        );
        println!("{name}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_depth_matches_checked() {
    let cases = cases();
    let expected = gold(&cases, image());
    let mut reference = None;
    for closed in [false, true] {
        let name = if closed {
            "closed-depth/borrowed"
        } else {
            "closed-depth/checked"
        };
        let (bytes, commits, output) = run(name, &cases, false, false, false, closed);
        assert_eq!(bytes, expected, "Independent whole-byte depth/blend gold");
        assert!(output.contains("fault=0"));
        if let Some((old_bytes, old_commits, old_output)) = &reference {
            assert_eq!(&bytes, old_bytes, "Matched whole memory");
            assert_eq!(&commits, old_commits, "Matched ordered ROP commits");
            assert_eq!(
                &output, old_output,
                "Same protocol/calendar and CE0 witnesses"
            );
        } else {
            reference = Some((bytes, commits, output.clone()));
        }
        println!("closed_depth={closed}: {output}");
    }
}

#[test]
fn closed_bank_geometry_matches_original_address_algebra() {
    let address =
        |plane: usize, line: usize, x: usize, y: usize| plane * 512 + line * 64 + y * 4 + (x >> 2);
    let bank = |x: usize, y: usize| (x & 3) ^ ((y & 1) * 2);
    for plane in 0..2 {
        for line in 0..8 {
            for y in (0..16).step_by(2) {
                for x in (0..16).step_by(2) {
                    for lane in 0..4 {
                        let b = lane ^ (x & 2);
                        assert_eq!(b, bank(x + lane % 2, y + lane / 2));
                        let direct = (plane << 9)
                            | (line << 6)
                            | (y << 2)
                            | (((b >> 1) ^ ((x >> 1) & 1)) << 2)
                            | (x >> 2);
                        assert_eq!(direct, address(plane, line, x + lane % 2, y + lane / 2));
                    }
                }
            }
            for sector in 0..4 {
                for beat in 0..16 {
                    for lane in 0..4 {
                        let pixel = sector * 64 + beat * 4 + lane;
                        assert_eq!(lane ^ ((beat & 4) >> 1), bank(pixel & 15, pixel >> 4));
                        assert_eq!(
                            (plane << 9) | (line << 6) | (sector << 4) | beat,
                            address(plane, line, pixel & 15, pixel >> 4)
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_depth_matches_checked() {
    let cases = cases();
    let expected = gold(&cases, image());
    let mut reference = None;
    for profile in [1, 2] {
        let name = if profile == 1 {
            "closed-bank-depth/checked"
        } else {
            "closed-bank-depth/direct"
        };
        let (bytes, commits, output) = run_profile(name, &cases, false, false, false, profile);
        assert_eq!(bytes, expected, "Independent whole-byte depth/blend gold");
        assert!(output.contains("fault=0"));
        if let Some((old_bytes, old_commits, old_output)) = &reference {
            assert_eq!(&bytes, old_bytes, "Matched whole memory");
            assert_eq!(&commits, old_commits, "Matched ordered ROP commits");
            assert_eq!(
                &output, old_output,
                "Same protocol/calendar and CE0 witnesses"
            );
        } else {
            reference = Some((bytes, commits, output.clone()));
        }
        println!("bank_profile={profile}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_reuse_depth_matches_checked() {
    let cases = cases();
    let expected = gold(&cases, image());
    let mut reference = None;
    for profile in [2, 3] {
        let name = if profile == 2 {
            "closed-bank-reuse/checked"
        } else {
            "closed-bank-reuse/reused"
        };
        let (bytes, commits, output) = run_profile(name, &cases, false, false, false, profile);
        assert_eq!(bytes, expected, "Independent whole-byte depth/blend gold");
        assert!(output.contains("fault=0"));
        if let Some((old_bytes, old_commits, old_output)) = &reference {
            assert_eq!(&bytes, old_bytes, "Matched whole memory");
            assert_eq!(&commits, old_commits, "Matched ordered ROP commits");
            assert_eq!(
                &output, old_output,
                "Same protocol/calendar and CE0 witnesses"
            );
        } else {
            reference = Some((bytes, commits, output.clone()));
        }
        println!("reuse_profile={profile}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_block_and_color_match_checked() {
    let cases = cases();
    let expected = gold(&cases, image());
    let mut reference = None;
    for profile in [2, 4, 5] {
        let name = match profile {
            2 => "closed-bank-block-color/checked",
            4 => "closed-bank-block-color/block",
            5 => "closed-bank-block-color/color",
            _ => unreachable!(),
        };
        let (bytes, commits, output) = run_profile(name, &cases, false, false, false, profile);
        assert_eq!(bytes, expected, "Independent whole-byte depth/blend gold");
        assert!(output.contains("fault=0"));
        if let Some((old_bytes, old_commits, old_output)) = &reference {
            assert_eq!(&bytes, old_bytes, "Matched whole memory");
            assert_eq!(&commits, old_commits, "Matched ordered ROP commits");
            assert_eq!(
                &output, old_output,
                "Same protocol/calendar and CE0 witnesses"
            );
        } else {
            reference = Some((bytes, commits, output.clone()));
        }
        println!("transport_profile={profile}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_stream_matches_checked() {
    // Resolve every source anchor before the costly reference compilation.
    framebuffer_sink::verilog_closed_bank_stream(surface()).unwrap();
    let cases = cases();
    let expected = gold(&cases, image());
    let mut reference = None;
    for profile in [2, 6] {
        let name = if profile == 2 {
            "closed-bank-stream/checked"
        } else {
            "closed-bank-stream/stream"
        };
        let (bytes, commits, output) = run_profile(name, &cases, false, false, false, profile);
        assert_eq!(bytes, expected, "Independent whole-byte depth/blend gold");
        assert!(output.contains("fault=0"));
        if let Some((old_bytes, old_commits)) = &reference {
            assert_eq!(&bytes, old_bytes, "Matched whole memory");
            assert_eq!(&commits, old_commits, "Matched ordered ROP commits");
        } else {
            reference = Some((bytes, commits));
        }
        println!("stream_profile={profile}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_overlap_matches_checked() {
    framebuffer_sink::verilog_closed_bank_overlap(surface()).unwrap();
    let mut cases = cases();
    // Resident runs include independent quads, repeated addresses requiring
    // ordered blend/depth RAW, sparse masks and every depth predicate.
    for (i, (q, _)) in cases.iter_mut().enumerate() {
        q.header.x = if i % 7 == 1 { 0 } else { ((i % 7) * 2) as u16 };
        q.header.y = ((i / 14) * 2) as u8;
    }
    cases.extend(crate::cases());
    let expected = gold(&cases, image());
    let mut reference = None;
    for profile in [6, 7, 8] {
        let name = match profile {
            6 => "closed-bank-overlap/checked",
            7 => "closed-bank-overlap/overlap",
            _ => "closed-bank-overlap/native",
        };
        let (bytes, commits, output) = run_profile(name, &cases, false, false, false, profile);
        assert_eq!(
            bytes, expected,
            "Independent complete image with dependent lanes"
        );
        if let Some((old_bytes, old_commits)) = &reference {
            assert_eq!(&bytes, old_bytes);
            assert_eq!(&commits, old_commits);
        } else {
            reference = Some((bytes, commits));
        }
        println!("overlap_profile={profile}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_overlap_warm_calendar_and_faults() {
    let mut resident = cases();
    for (i, (q, _)) in resident.iter_mut().enumerate() {
        q.header.x = ((i % 8) * 2) as u16;
        q.header.y = ((i / 8) * 2) as u8;
    }
    let expected = gold(&resident, image());
    let (bytes, _, output) = run_profile(
        "closed-bank-overlap/warm",
        &resident,
        false,
        false,
        false,
        7,
    );
    assert_eq!(bytes, expected);
    let intervals: Vec<usize> = output
        .lines()
        .filter(|l| l.starts_with("RESIDENT "))
        .map(|l| {
            l.split("enabled_interval")
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    assert_eq!(intervals.len(), resident.len() - 1);
    assert!(
        intervals[8..].iter().all(|n| *n <= 19),
        "Actual steady resident intervals {intervals:?}"
    );
    println!("{output}");
    for (name, lane, mask, fail) in [
        ("bad-lane", true, false, false),
        ("bad-mask", false, true, false),
        ("read-terminal-failure", false, false, true),
    ] {
        let (_, _, output) = run_profile(
            &format!("closed-bank-overlap/{name}"),
            &resident[..2],
            lane,
            mask,
            fail,
            7,
        );
        assert!(output.contains("fault=1") && !output.contains("FAULT reqs=0 "));
        println!("{name}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_overlap_late_owner_fault() {
    let mut resident = cases();
    for (i, (q, _)) in resident.iter_mut().enumerate() {
        q.header.x = ((i % 8) * 2) as u16;
        q.header.y = 0;
    }
    let (bytes, commits, output) = run_profile(
        "closed-bank-overlap/late-owned-lane",
        &resident[..6],
        true,
        false,
        false,
        7,
    );
    assert_eq!(bytes, image(), "Fault leaves dirty cache rows unflushed");
    assert!(output.contains("fault=1") && commits.len() >= 2 && commits.len() < 6);
    println!("{output}");
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_commit_matches_checked() {
    framebuffer_sink::verilog_closed_bank_overlap_commit(surface()).unwrap();
    let mut mixed = cases();
    for (i, (q, _)) in mixed.iter_mut().enumerate() {
        q.header.x = if i % 7 == 1 { 0 } else { ((i % 7) * 2) as u16 };
        q.header.y = ((i / 14) * 2) as u8;
    }
    mixed.extend(cases());
    let expected = gold(&mixed, image());
    let mut reference = None;
    for profile in [7, 9, 10] {
        let name = match profile {
            7 => "closed-bank-commit/checked",
            9 => "closed-bank-commit/commit",
            _ => "closed-bank-commit/native",
        };
        let (bytes, commits, output) = run_profile(name, &mixed, false, false, false, profile);
        assert_eq!(bytes, expected);
        if let Some((old_bytes, old_commits)) = &reference {
            assert_eq!(&bytes, old_bytes);
            assert_eq!(&commits, old_commits);
        } else {
            reference = Some((bytes, commits));
        }
        println!("commit_profile={profile}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_commit_resident_calendar() {
    let mut resident = cases();
    for (i, (q, _)) in resident.iter_mut().enumerate() {
        q.header.x = ((i % 8) * 2) as u16;
        q.header.y = ((i / 8) * 2) as u8;
    }
    for name in ["closed-bank-commit/warm", "closed-bank-commit/warm-pace15"] {
        let (bytes, commits, output) = run_profile(name, &resident, false, false, false, 9);
        assert_eq!(bytes, gold(&resident, image()));
        assert_eq!(commits.len(), 40);
        let intervals: Vec<usize> = output
            .lines()
            .filter(|l| l.starts_with("RESIDENT "))
            .map(|l| {
                l.split("enabled_interval")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert_eq!(intervals.len(), 39);
        if name.contains("pace15") {
            assert!(
                intervals[20..].iter().all(|n| *n == 15),
                "Real paced intervals {intervals:?}"
            );
        } else {
            assert!(
                intervals[8..].windows(2).all(|n| n[0] + n[1] <= 30),
                "Actual saturated intervals {intervals:?}"
            );
        }
        println!("{name}: {output}");
    }
}

#[test]
#[ignore = "requires Icarus actual framebuffer and ROP"]
fn closed_bank_commit_bad_write_edge_matches_fault_fence() {
    // Malformed next ingress on the actual final bank-write edge must cancel
    // notification, just as the old separate DONE is suppressed by fault.
    for profile in [7, 9] {
        let selected = vec![cases()[7]]; // ALWAYS + DW: a real bank depth write.
        let (bytes, commits, output) = run_profile(
            &format!("closed-bank-commit/bad-write-edge-{profile}"),
            &selected,
            false,
            false,
            false,
            profile,
        );
        assert_eq!(bytes, image());
        assert!(commits.is_empty());
        assert!(output.contains("fault=1"));
        println!("{output}");
    }
}

#[test]
fn closed_bank_prefix_factory_anchors() {
    framebuffer_sink::verilog_closed_bank_prefix(surface()).unwrap();
}

#[test]
#[ignore = "requires Icarus actual prefix framebuffer and ROP"]
fn closed_bank_prefix_gold_and_calendar() {
    let mut resident = cases();
    for (i, (q, _)) in resident.iter_mut().enumerate() {
        q.header.x = ((i % 8) * 2) as u16;
        q.header.y = ((i / 8) * 2) as u8;
    }
    for (name, profile) in [
        ("closed-bank-prefix/warm", 11),
        ("closed-bank-prefix/native", 12),
    ] {
        let (bytes, commits, output) = run_profile(name, &resident, false, false, false, profile);
        assert_eq!(bytes, gold(&resident, image()));
        assert_eq!(
            commits,
            resident
                .iter()
                .map(|(q, _)| (u32::from(q.header.x) << 12)
                    | (u32::from(q.header.y) << 4)
                    | u32::from(q.header.mask))
                .collect::<Vec<_>>()
        );
        if name.contains("warm") {
            let intervals: Vec<usize> = output
                .lines()
                .filter(|l| l.starts_with("RESIDENT "))
                .map(|l| {
                    l.split("enabled_interval")
                        .nth(1)
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap()
                })
                .collect();
            assert!(
                intervals[8..].iter().all(|n| *n == 8),
                "Actual intervals {intervals:?}"
            );
            assert!(
                !output.contains("PREFIX atomic_exchange0"),
                "No full old-credit witness"
            );
        }
        println!("{name}: {output}");
    }
    let mut mixed = resident[..24].to_vec();
    // Repeated pixels must see all earlier depth/color writes before read.
    for (i, (q, _)) in mixed.iter_mut().enumerate() {
        q.header.x = if i % 3 == 1 { 0 } else { ((i % 6) * 2) as u16 };
        q.header.y = 0;
    }
    mixed.extend(cases());
    let (bytes, commits, output) = run_profile(
        "closed-bank-prefix/prefix-bubbles-mixed",
        &mixed,
        false,
        false,
        false,
        11,
    );
    assert_eq!(bytes, gold(&mixed, image()));
    assert_eq!(
        commits,
        mixed
            .iter()
            .map(|(q, _)| (u32::from(q.header.x) << 12)
                | (u32::from(q.header.y) << 4)
                | u32::from(q.header.mask))
            .collect::<Vec<_>>()
    );
    println!("{output}");
}

#[test]
#[ignore = "requires Icarus actual prefix framebuffer fault cleanup"]
fn closed_bank_prefix_faults_abandon_private_tile() {
    let mut resident = cases();
    for (i, (q, _)) in resident.iter_mut().enumerate() {
        q.header.x = ((i % 8) * 2) as u16;
        q.header.y = 0;
    }
    for (name, lane, mask, read) in [
        ("late-owned-lane", true, false, false),
        ("bad-mask", false, true, false),
        ("cold-invalid-mask", false, true, false),
        ("read-terminal-failure", false, false, true),
    ] {
        let (bytes, commits, output) = run_profile(
            &format!("closed-bank-prefix/{name}"),
            &resident[..6],
            lane,
            mask,
            read,
            11,
        );
        assert_eq!(bytes, image(), "Fault prohibits partial tile writeback");
        assert!(output.contains("fault=1"));
        if name.contains("late-owned") {
            assert!(!commits.is_empty() && commits.len() < 6);
        }
        println!("{name}: {output}");
    }
}
