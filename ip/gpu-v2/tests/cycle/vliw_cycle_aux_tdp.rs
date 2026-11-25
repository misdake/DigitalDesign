//! Selected production TDP banks: raw copy and overlapping UI half-buffer ports.
#[path = "../support/vliw_pixel_source.rs"]
mod selected;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    tdp::{Cache, Input, Serial},
    vertex::Bits,
};
use std::fmt::Write as _;
use vertex_support::rtl::Pin;
fn pin(
    p: &mut Vec<Pin>,
    name: impl Into<String>,
    width: u32,
    value: u64,
    known: u64,
    output: bool,
) {
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1 << width) - 1
    };
    p.push(Pin {
        name: name.into(),
        width,
        value: (value & mask).into(),
        known: (known & mask).into(),
        output,
    });
}
fn fields(c: &Cache, i: Input) -> Vec<Pin> {
    let o = c.outputs(i);
    let s = &c.state;
    let mut p = Vec::new();
    for (n, v) in [
        ("reset", i.reset),
        ("ce", i.ce),
        ("shade_phase", i.shade_phase),
        ("ui_active", i.serial.active),
        ("ui_cmd_valid", i.serial.command_valid),
        ("ui_cmd_write", i.serial.writing),
        ("ui_lower_ready", i.serial.lower_ready),
        ("copy_active", i.serial.copy),
        ("ui_pixel_valid", i.serial.pixel_valid),
        ("ui_pixel_half", i.serial.pixel_half),
        ("mem_req_ready", i.mem_req_ready),
        ("mem_read_valid", i.mem_read_valid),
        ("mem_write_ready", i.mem_write_ready),
        ("mem_complete_valid", i.mem_complete_valid),
        ("mem_complete_ok", i.mem_complete_ok),
    ] {
        pin(&mut p, n, 1, v.into(), 1, false);
    }
    for (n, w, v, k) in [
        ("ui_tile", 9, u64::from(i.serial.tile), 511),
        (
            "copy_address",
            14,
            i.serial.copy_address.value,
            i.serial.copy_address.known,
        ),
        ("ui_pixel_word", 5, u64::from(i.serial.pixel_word), 31),
        (
            "ui_pixels",
            64,
            i.serial.pixels.value,
            i.serial.pixels.known,
        ),
        ("mem_read_data", 64, i.mem_read_data, u64::MAX),
    ] {
        pin(&mut p, n, w, v, k, false);
    }
    for (n, v) in [
        ("ui_cmd_ready", o.serial_ready),
        ("ui_cmd_done", o.serial_done),
        ("mem_req_valid", o.mem_req_valid),
        ("mem_req_write", o.mem_req_write),
        ("mem_write_valid", o.mem_write_valid),
        ("obs_mt_active", s.mt_active),
        ("obs_mt_write", s.mt_write),
        ("obs_wr_beat_valid", s.wr_beat_valid),
    ] {
        pin(&mut p, n, 1, v.into(), 1, true);
    }
    for (n, w, v, k) in [
        (
            "mem_req_addr",
            32,
            u64::from(o.mem_req_address),
            u64::from(o.mem_req_address_known),
        ),
        (
            "mem_write_data",
            64,
            o.mem_write_data.value,
            o.mem_write_data.known,
        ),
        ("obs_mt_issue", 3, u64::from(s.mt_issue), 7),
        ("obs_mt_retire", 3, u64::from(s.mt_retire), 7),
        ("obs_mt_data", 7, u64::from(s.mt_data), 127),
        ("obs_mt_remaining", 2, u64::from(s.mt_remaining), 3),
    ] {
        pin(&mut p, n, w, v, k, true);
    }
    for n in 0..8 {
        pin(&mut p, format!("obs_v{n}"), 1, s.valid[n].into(), 1, true);
    }
    for n in 0..4 {
        pin(
            &mut p,
            format!("obs_aq{n}"),
            16,
            c.banks[n].aq.value,
            c.banks[n].aq.known,
            true,
        );
        pin(
            &mut p,
            format!("obs_bq{n}"),
            16,
            c.banks[n].bq.value,
            c.banks[n].bq.known,
            true,
        );
    }
    p
}
fn source(p: &[Pin]) -> String {
    let mut s=String::from("`define GPU_V2_COMMAND_MAINTENANCE\n`define GPU_V2_COMMAND_PORTS\n`define GPU_VLIW_POOL_PORTABLE\n");
    for name in [
        "gpu_v2_framebuffer_tdp",
        "gpu_v2_framebuffer_tdp_bank",
        "gpu_v2_rop_tdp",
    ] {
        s.push_str(selected::module(name));
        s.push('\n');
    }
    s.push_str("module g2a(input clk");
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
    s.push_str(");\ngpu_v2_framebuffer_tdp #(.PREFETCH_ENABLE(1),.PREFETCH_DIRTY(1))dut(.clk(clk),.commit_allow(1'b1),.shared_rop_product(54'd0),.row_valid(1'b0),.row_tile(9'd0),.row_x(9'd0),.row_y(8'd0),.row_mask(4'd0),.row_index(3'd0),.row_data(32'd0),.row_alpha(8'd0),.depth_func(3'd0),.depth_write(1'b0),.blend(1'b0),.color_base(14'd2048),.depth_base(14'd2560),.flush_request(1'b0),.prefetch_abort(1'b0),.prefetch_valid(1'b0),.prefetch_tile(10'd0)");
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for f in p.iter().filter(|f| f.name.starts_with("obs_")) {
        let n = &f.name[4..];
        let path = if let Some(i) = n.strip_prefix("aq") {
            format!("a_q[{i}]")
        } else if let Some(i) = n.strip_prefix("bq") {
            format!("b_q[{i}]")
        } else if let Some(i) = n.strip_prefix('v') {
            format!("tvalid[{i}]")
        } else {
            n.into()
        };
        writeln!(s, "assign {}=dut.{path};", f.name).unwrap();
    }
    s.push_str("endmodule\n");
    s
}
fn pattern(address: u32) -> u64 {
    u64::from(address).wrapping_mul(0x1937_810d_237b_9405) ^ 0x18a3_7c59_96e0_42bf
}
fn overlay(half: bool, word: usize) -> u64 {
    (0..4).fold(0, |v, n| {
        let id = (word * 4 + n + usize::from(half) * 137) as u16;
        let p = id.wrapping_mul(1739) ^ 0x5ab0;
        v | (u64::from((p & !1) | u16::from(!(word + n).is_multiple_of(3))) << (n * 16))
    })
}
fn convert(p: u16) -> u16 {
    let (r, g, b) = (p >> 11 & 31, p >> 6 & 31, p >> 1 & 31);
    r << 11 | (g * 2 + g / 16) << 5 | b
}
struct Transaction {
    write: bool,
    address: u32,
    word: usize,
    terminal: usize,
}
struct Run {
    rows: Vec<Vec<Pin>>,
    error: Option<&'static str>,
}
fn run(ui: bool, bad: Option<&str>) -> Run {
    let mut c = Cache::default();
    let mut rows = Vec::new();
    let mut tx: Option<Transaction> = None;
    let mut stage = 0;
    let mut admitted = false;
    let mut halves = 0;
    let mut lower = 0;
    let mut upper = 0;
    let mut writes = 0;
    let mut reads = 0;
    let mut gated = false;
    let mut ce0_terminal = false;
    let source_base = if ui {
        2048 * 512 + 353 * 512
    } else {
        16383 * 512
    };
    let destination = if ui { source_base } else { 512 };
    let mut expected: Vec<u64> = (0..64).map(|n| pattern(source_base + n * 8)).collect();
    // Bank order swaps 2/3 against 0/1 on word bit2; the independent reference
    // maps each A address back through that physical B permutation.
    if ui {
        for half in 0..2 {
            for word in 0..32 {
                let packed = overlay(half != 0, word);
                for bank in 0..4 {
                    let p = (packed >> (bank * 16)) as u16;
                    if p & 1 != 0 {
                        let k = half * 32 + word;
                        let lane = if k & 4 != 0 { bank ^ 2 } else { bank };
                        let mask = 0xffffu64 << (lane * 16);
                        expected[k] =
                            (expected[k] & !mask) | (u64::from(convert(p)) << (lane * 16));
                    }
                }
            }
        }
    }
    for wall in 0..6000 {
        if stage == 0 && admitted && !c.state.mt_active && (!ui || lower == 32) {
            stage = 1;
            admitted = false;
        }
        let ce = if ui && stage == 1 {
            wall % 5 == 0
        } else {
            wall % 9 != 3 && wall % 9 != 4
        };
        let mut serial = Serial {
            active: wall != 0,
            command_valid: wall != 0 && !admitted,
            writing: stage == 1,
            lower_ready: !ui || upper == 32,
            tile: 353,
            copy: !ui,
            copy_address: Bits::new(if stage == 0 { 16383 } else { 1 }, 14),
            pixels: Bits::new(0, 64),
            ..Default::default()
        };
        if ui && ce {
            if halves >= 1 && lower < 32 {
                serial.pixel_valid = true;
                serial.pixel_word = lower as u8;
                serial.pixels = Bits::new(overlay(false, lower), 64);
            } else if stage == 1 && upper < 32 {
                serial.pixel_valid = true;
                serial.pixel_half = true;
                serial.pixel_word = upper as u8;
                serial.pixels = Bits::new(overlay(true, upper), 64);
            }
        }
        let mut i = Input {
            reset: wall == 0,
            ce,
            commit_allow: true,
            shade_phase: c.state.cal & 1 != 0,
            shared_rop_product: Bits::new(0, 54),
            color_base: 2048,
            depth_base: 2560,
            mem_req_ready: tx.is_none() && wall % 11 != 2,
            mem_complete_ok: true,
            serial,
            ..Default::default()
        };
        if let Some(t) = &tx {
            if t.word == 16 {
                i.mem_complete_valid = t.terminal == 0;
            } else if t.write {
                i.mem_write_ready = wall % 7 != 2;
            } else if wall % 7 != 2 {
                i.mem_read_valid = true;
                i.mem_read_data = pattern(t.address + t.word as u32 * 8);
            }
        }
        if bad == Some("collision") && i.mem_read_valid {
            i.serial.pixel_valid = true;
            i.serial.pixel_half = c.state.mt_data & 32 != 0;
            i.serial.pixel_word = c.state.mt_data & 31;
            i.serial.pixels = Bits::new(1, 64);
        }
        if bad == Some("withdraw") && c.state.mt_active && c.state.mt_write && c.state.mt_issue >= 3
        {
            i.serial.lower_ready = false;
        }
        let o = c.outputs(i);
        rows.push(fields(&c, i));
        if let Err(e) = c.clock(i) {
            return Run {
                rows,
                error: Some(e.rule),
            };
        }
        if i.serial.command_valid && o.serial_ready {
            admitted = true;
        }
        if i.serial.pixel_valid {
            if i.serial.pixel_half {
                upper += 1;
            } else {
                lower += 1;
            }
        }
        if o.serial_done {
            halves += 1;
        }
        gated |=
            ui && stage == 1 && c.state.mt_issue == 2 && !i.serial.lower_ready && !o.mem_req_valid;
        ce0_terminal |= !ce && i.mem_complete_valid;
        if let Some(t) = &mut tx {
            if t.word == 16 {
                if i.mem_complete_valid {
                    tx = None;
                } else {
                    t.terminal -= 1;
                }
            } else if t.write && o.mem_write_valid && i.mem_write_ready {
                assert_eq!(
                    t.address + t.word as u32 * 8,
                    destination + writes as u32 * 8
                );
                assert_eq!(
                    o.mem_write_data.get(64, "aux write").unwrap(),
                    expected[writes],
                    "ui={ui} word={writes}"
                );
                writes += 1;
                t.word += 1;
            } else if !t.write && i.mem_read_valid {
                reads += 1;
                t.word += 1;
            }
        }
        if o.mem_req_valid && i.mem_req_ready {
            assert!(tx.is_none());
            let base = if stage == 0 { source_base } else { destination };
            let sector = if stage == 0 { reads / 16 } else { writes / 16 };
            assert_eq!(o.mem_req_address, base + sector as u32 * 128);
            tx = Some(Transaction {
                write: o.mem_req_write,
                address: o.mem_req_address,
                word: 0,
                terminal: 3,
            });
        }
        if stage == 1 && admitted && !c.state.mt_active && tx.is_none() {
            assert_eq!((reads, writes, halves), (64, 64, 3));
            assert!(ce0_terminal);
            assert!(!ui || gated && lower == 32 && upper == 32);
            assert!(c.state.valid.iter().all(|v| !*v));
            return Run { rows, error: None };
        }
    }
    panic!("aux TDP bounded6000 ui{ui} stage{stage} lower{lower} upper{upper}")
}
fn root() -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-aux")
        .join(format!(
            "tdp-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn narrow_tile_port() -> Vec<Vec<Pin>> {
    let mut c = Cache::default();
    let mut rows = Vec::new();
    for wall in 0..4 {
        let i = Input {
            reset: wall == 0,
            commit_allow: true,
            shared_rop_product: Bits::new(0, 54),
            color_base: 2048,
            depth_base: 2560,
            serial: Serial {
                active: wall != 0,
                command_valid: wall == 1,
                tile: 1023,
                pixels: Bits::new(0, 64),
                ..Default::default()
            },
            ..Default::default()
        };
        rows.push(fields(&c, i));
        if wall == 3 {
            assert_eq!(c.outputs(i).mem_req_address, 0x13fe00);
        }
        c.clock(i).unwrap();
    }
    rows
}
#[test]
fn serial_tile_port_keeps_actual_nine_and_four_bit_widths() {
    narrow_tile_port();
}
#[test]
fn copy_and_ui_reuse_all_64_words_without_publishing_tags() {
    for ui in [false, true] {
        let r = run(ui, None);
        assert!(r.error.is_none());
    }
}
#[test]
#[ignore = "actual serial TDP, three mutations and two original UI bank monitors"]
fn serial_tdp_matches_production_and_detects_counterexamples() {
    let root = root();
    println!("AUX_TDP_ROOT {}", root.display());
    let r = narrow_tile_port();
    let s = source(&r[0]);
    vertex_support::rtl::replay(&root, "narrow_tile", r, s, false, None);
    for ui in [false, true] {
        let r = run(ui, None);
        let s = source(&r.rows[0]);
        vertex_support::rtl::replay(
            &root,
            if ui { "ui" } else { "copy" },
            r.rows,
            s,
            false,
            None,
        );
    }
    for (ui, name, old, new) in [
        (
            false,
            "copy_address",
            "{9'd0,copy_address,mt_issue[1:0],7'd0}",
            "{9'd0,(copy_address+14'd1),mt_issue[1:0],7'd0}",
        ),
        (
            true,
            "upper_preread",
            "&&(!ui_active||!maintenance_read_word[5]||ui_lower_ready)",
            "",
        ),
        (
            true,
            "serial_publication",
            "if(!ui_active&&!mt_write&&!stop_publication&&!mt_cancel)",
            "if(!mt_write&&!stop_publication&&!mt_cancel)",
        ),
    ] {
        let r = run(ui, None);
        let s = source(&r.rows[0]);
        assert_eq!(s.matches(old).count(), 1, "{name}");
        vertex_support::rtl::replay(&root, name, r.rows, s.replacen(old, new, 1), true, None);
    }
    for (bad, rule) in [
        ("collision", "UI half-buffer collision"),
        ("withdraw", "UI lower readiness withdrew after publication"),
    ] {
        let r = run(true, Some(bad));
        assert_eq!(r.error, Some(rule));
        let s = source(&r.rows[0]);
        vertex_support::rtl::replay(&root, bad, r.rows, s, false, Some(rule));
    }
}
