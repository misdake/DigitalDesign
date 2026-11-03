//! Actual UI task/Q calendar; RAM, finite DMA and framebuffer terminals are peers.
#[path = "support/vliw_pixel_source.rs"]
mod selected;
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::ui::{Engine, Input};
use std::fmt::Write as _;
use vertex_support::rtl::Pin;
fn fields(e: &Engine, i: Input) -> Vec<Pin> {
    let s = e.state;
    let o = e.outputs(i);
    let mut p = Vec::new();
    let mut add = |name: &str, width: u32, v: u64, out: bool| {
        p.push(Pin {
            name: name.into(),
            width,
            value: v.into(),
            known: if width == 64 {
                u64::MAX.into()
            } else {
                (1u128 << width) - 1
            },
            output: out,
        })
    };
    for (n, v) in [
        ("reset", i.reset),
        ("ce", i.ce),
        ("start", i.start),
        ("stop", i.stop),
        ("cp_ready", i.cp_ready),
        ("work_ready", i.work_ready),
        ("dma_idle", i.dma_idle),
        ("cmd_ready", i.command_ready),
        ("cmd_done", i.command_done),
    ] {
        add(n, 1, v.into(), false);
    }
    for (n, w, v) in [
        ("grid_base", 16, u64::from(i.grid_base)),
        ("atlas_base", 7, i.atlas_base.into()),
        ("first_cell", 10, i.first_cell.into()),
        ("last_cell", 10, i.last_cell.into()),
        ("cp_q", 64, i.cp_q),
    ] {
        add(n, w, v, false);
    }
    for (n, v) in [
        ("active", o.active),
        ("done", o.done),
        ("cp_re", o.cp_read),
        ("work_valid", o.work_valid),
        ("cmd_valid", o.command_valid),
        ("cmd_write", o.writing),
        ("lower_ready", o.lower_ready),
        ("pixel_valid", o.pixel_valid),
        ("pixel_half", o.pixel_half),
        ("obs_initialized", s.initialized),
        ("obs_scan_done", s.scan_done),
        ("obs_pair_valid", s.pair_valid),
        ("obs_put", s.put),
        ("obs_get", s.get),
        ("obs_loader", s.loader),
        ("obs_overlay", s.overlay),
        ("obs_overlay_half", s.overlay_half),
        ("obs_q_valid", s.q_valid),
        ("obs_q_pixel", s.q_pixel),
        ("obs_q_half", s.q_half),
    ] {
        add(n, 1, v.into(), true);
    }
    for (n, w, v) in [
        ("cp_addr", 9, u64::from(o.cp_address)),
        ("work_source", 22, o.work_source.into()),
        ("work_destination", 7, o.work_destination.into()),
        ("tile", 9, o.tile.into()),
        ("pixel_word", 5, o.pixel_word.into()),
        ("pixels", 64, o.pixels),
        ("obs_atlas", 7, s.atlas.into()),
        ("obs_init_sector", 3, s.init_sector.into()),
        ("obs_scan", 9, s.scan.into()),
        ("obs_window", 16, s.window.into()),
        (
            "obs_valid",
            2,
            u64::from(s.valid[0]) | u64::from(s.valid[1]) << 1,
        ),
        (
            "obs_loaded",
            2,
            u64::from(s.loaded[0]) | u64::from(s.loaded[1]) << 1,
        ),
        ("obs_load_part", 3, s.load_part.into()),
        ("obs_state", 4, s.phase.into()),
        ("obs_pixel", 6, s.pixel.into()),
        ("obs_q_word", 5, s.q_word.into()),
        ("obs_task0", 9, s.task_tile[0].into()),
        ("obs_task1", 9, s.task_tile[1].into()),
        ("obs_ids0", 16, s.ids[0].into()),
        ("obs_ids1", 16, s.ids[1].into()),
    ] {
        add(n, w, v, true);
    }
    p
}
fn source(p: &[Pin]) -> String {
    let mut s = String::from(selected::module("gpu_ui_grid"));
    s.push_str("\nmodule g2a(input clk");
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
    s.push_str(");\ngpu_ui_grid dut(.clk(clk)");
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for f in p.iter().filter(|f| f.name.starts_with("obs_")) {
        let n = &f.name[4..];
        let path = match n {
            "task0" => "task_tile[0]",
            "task1" => "task_tile[1]",
            "ids0" => "ids[0]",
            "ids1" => "ids[1]",
            _ => n,
        };
        writeln!(s, "assign {}=dut.{path};", f.name).unwrap();
    }
    s.push_str("endmodule\n");
    s
}
fn glyph(address: u32) -> u64 {
    u64::from(address).wrapping_mul(0x3917_135d_8759_1237) ^ 0xc481_37a0_19ef_68b5
}
fn run(case: usize, abort: bool, double_start: bool) -> (Vec<Vec<Pin>>, Option<&'static str>) {
    let (first, last) = match case {
        0 => (127, 130),
        1 => (749, 749),
        _ => (0, 749),
    };
    let mut grid = [255u8; 768];
    if case == 0 {
        grid[127] = 1;
        grid[128] = 2;
        grid[130] = 3;
    }
    if case == 1 {
        grid[749] = 4;
    }
    let tasks: Vec<u16> = ((first / 2)..=(last / 2))
        .filter(|t| {
            (*t * 2..*t * 2 + 2).any(|c| c >= first && c <= last && grid[c as usize] != 255)
        })
        .collect();
    let mut e = Engine::default();
    let mut ram = [0u64; 512];
    let mut q = 0;
    let mut dma: Option<(u32, usize, usize)> = None;
    let mut terminals = std::collections::VecDeque::<(usize, bool)>::new();
    let mut rows = Vec::new();
    let mut seen = Vec::new();
    let mut peak = 0;
    let mut stop = false;
    let mut pixels = 0;
    let mut ce0 = false;
    let mut dma_count = 0;
    for wall in 0..10000 {
        stop |= abort && e.state.phase == 4;
        let ready = terminals.is_empty();
        let done = terminals.front().is_some_and(|(at, _)| {
            *at <= wall
                && (!terminals.front().unwrap().1
                    || e.outputs(Input {
                        stop,
                        ..Default::default()
                    })
                    .lower_ready)
        });
        let i = Input {
            reset: wall == 0,
            ce: !done && wall % 13 != 3 && wall % 13 != 4,
            start: wall == 1 || double_start && wall == 2,
            stop,
            grid_base: 0x1234,
            atlas_base: 7,
            first_cell: first,
            last_cell: last,
            cp_ready: wall % 7 != 2,
            cp_q: q,
            work_ready: dma.is_none() && wall % 11 != 8,
            dma_idle: dma.is_none(),
            command_ready: ready,
            command_done: done,
        };
        let o = e.outputs(i);
        rows.push(fields(&e, i));
        if let Err(x) = e.clock(i) {
            return (rows, Some(x.rule));
        }
        peak = peak.max(e.state.valid.into_iter().filter(|v| *v).count());
        pixels += usize::from(o.pixel_valid);
        ce0 |= done && !i.ce;
        if done {
            terminals.pop_front();
        }
        if o.command_valid && i.command_ready {
            seen.push((o.tile, o.writing));
            if o.writing {
                terminals.push_back((wall + 70, true));
            } else {
                terminals.extend([(wall + 7, false), (wall + 65, false)]);
            }
        }
        if i.ce && o.cp_read && i.cp_ready {
            q = ram[usize::from(o.cp_address)];
            if o.cp_address >= 128 {
                let slot = usize::from((o.cp_address >> 6) & 1);
                let side = (o.cp_address >> 5) & 1;
                let id = (e.state.ids[slot] >> (side * 8)) & 255;
                let a = (7 << 16)
                    + u32::from(id) * 256
                    + u32::from((o.cp_address >> 4) & 1) * 128
                    + u32::from(o.cp_address & 15) * 8;
                assert_eq!(q, glyph(a));
            }
        }
        if let Some((address, destination, word)) = dma.as_mut() {
            let a = *address + *word as u32 * 8;
            ram[*destination + *word] = if (0x1234 * 128..0x1234 * 128 + 768).contains(&a) {
                u64::from_le_bytes(
                    grid[(a - 0x1234 * 128) as usize..(a - 0x1234 * 128) as usize + 8]
                        .try_into()
                        .unwrap(),
                )
            } else {
                glyph(a)
            };
            *word += 1;
            if *word == 16 {
                dma = None;
            }
        }
        if i.ce && o.work_valid && i.work_ready {
            dma_count += 1;
            dma = Some((o.work_source * 2, usize::from(o.work_destination) * 4, 0));
        }
        if wall > 1 && o.done {
            if !abort {
                assert_eq!(
                    seen,
                    tasks
                        .iter()
                        .flat_map(|t| [(*t, false), (*t, true)])
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    pixels,
                    if case == 0 {
                        96
                    } else if case == 1 {
                        32
                    } else {
                        0
                    }
                );
                assert!(case != 0 || peak == 2);
                assert_eq!(
                    dma_count,
                    if case == 0 {
                        8
                    } else if case == 1 {
                        3
                    } else {
                        6
                    }
                );
            }
            assert!(case == 2 || ce0 || abort);
            return (rows, None);
        }
    }
    panic!(
        "UI exceeded10000 case{case} phase{} scan{}",
        e.state.phase, e.state.scan
    )
}
fn root() -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-aux")
        .join(format!(
            "ui-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn ui_ranges_two_tasks_and_empty_grid_have_bounded_ownership() {
    for case in 0..3 {
        assert!(run(case, false, false).1.is_none());
    }
    assert!(run(0, true, false).1.is_none());
    assert_eq!(run(0, false, true).1, Some("UI start while active"));
}
#[test]
#[ignore = "production UI scan/overlay calendar, CE0 stop and actual storage/Q mutations"]
fn ui_engine_matches_production_and_detects_ownership_mutations() {
    let root = root();
    println!("AUX_UI_ROOT {}", root.display());
    for (case, abort) in [(0, false), (1, false), (2, false), (0, true)] {
        let (r, error) = run(case, abort, false);
        assert!(error.is_none());
        let s = source(&r[0]);
        vertex_support::rtl::replay(
            &root,
            &format!("case{case}-abort{abort}"),
            r,
            s,
            false,
            None,
        );
    }
    let (r, error) = run(0, false, true);
    assert_eq!(error, Some("UI start while active"));
    let s = source(&r[0]);
    vertex_support::rtl::replay(&root, "double_start", r, s, false, error);
    for (name, old, new) in [
        (
            "atlas_q_address",
            "{1'b1,get,side,overlay_half,pixel[4:2],pixel[0]}",
            "{1'b0,get,side,overlay_half,pixel[4:2],pixel[0]}",
        ),
        (
            "early_task_release",
            "WRITE0:state<=WRITE0_WAIT;",
            "WRITE0:begin state<=WRITE0_WAIT;valid[get]<=0;end",
        ),
        (
            "q_calendar",
            "q_valid<=read_take;q_pixel<=pixel_read;",
            "q_valid<=cp_re;q_pixel<=pixel_read;",
        ),
    ] {
        let (r, _) = run(0, false, false);
        let s = source(&r[0]);
        assert_eq!(s.matches(old).count(), 1, "{name}");
        vertex_support::rtl::replay(&root, name, r, s.replacen(old, new, 1), true, None);
    }
}
