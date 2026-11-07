//! All-command CP, real synchronous Q and bounded physical-owner peers.
#[allow(dead_code)]
#[path = "../support/vliw_pixel_source.rs"]
mod actual;
#[allow(dead_code)]
#[path = "../support/vliw_draw/mod.rs"]
mod cp_support;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::{
    vliw_command::Command,
    vliw_cycle::{command::*, vertex::Bits},
};
use std::{collections::VecDeque, fmt::Write as _};
use vertex_support::rtl::Pin;
struct Run {
    rows: Vec<Vec<Pin>>,
    error: Option<Error>,
    clears: usize,
    copies: usize,
    uis: usize,
}
fn fields(cp: &Processor, i: ProcessorInput, o: ProcessorOutput) -> Vec<Pin> {
    let edge = cp_support::Edge {
        input: i,
        output: o,
        phase: cp.phase(),
        pc: cp.pc(),
        cache_pages: cp.cache_pages(),
        status: cp.draw_status().unwrap(),
    };
    let mut p: Vec<_> = cp_support::fields(&edge)
        .into_iter()
        .map(|f| Pin {
            name: f.name.into(),
            width: f.width,
            value: f.value.into(),
            known: (1u128 << f.width) - 1,
            output: f.output,
        })
        .collect();
    let mut pin = |name: &str, width: u32, value: u128, known: u128, output: bool| {
        p.push(Pin {
            name: name.into(),
            width,
            value,
            known,
            output,
        })
    };
    let (a, b) = (o.auxiliary, i.auxiliary);
    for (n, v) in [
        ("clear_ready", b.clear_ready),
        ("clear_complete", b.clear_complete),
        ("clear_ok", b.clear_ok),
        ("ui_done", b.ui_done),
        ("copy_ready", b.copy_ready),
        ("copy_complete", b.copy_complete),
    ] {
        pin(n, 1, v as u128, 1, false);
    }
    for (n, v) in [
        ("clear_active", a.clear_active),
        ("clear_request", a.clear_request),
        ("ui_start", a.ui_start),
        ("copy_active", a.copy_active),
        ("copy_request", a.copy_request),
        ("copy_write", a.copy_write),
    ] {
        pin(n, 1, v as u128, 1, true);
    }
    for (n, w, v) in [
        ("clear_address", 32, u128::from(a.clear_address)),
        ("clear_value", 16, u128::from(a.clear_value)),
        ("ui_first", 10, u128::from(a.ui_first)),
        ("ui_last", 10, u128::from(a.ui_last)),
    ] {
        pin(n, w, v, (1u128 << w) - 1, true);
    }
    pin(
        "copy_address",
        14,
        a.copy_address.value.into(),
        a.copy_address.known.into(),
        true,
    );
    p
}
fn run(bad: Option<&str>, abort_kind: Option<u8>, long_copy: bool) -> Run {
    let mut cp = Processor::with_all_commands();
    let mut cells = [0u64; 512];
    let destination = if long_copy { 4096 } else { 37 };
    let descriptor = 11 | (destination << 14) | ((if long_copy { 2047 } else { 2 }) << 28);
    cells[264] = 2048 | (2560u64 << 14);
    cells[272] = descriptor;
    cells[276] = 0x1234 | (7u64 << 16);
    let mut code = vec![
        Command::wait(true, true).0,
        Command::FLUSH.0,
        Command::set_target(264).unwrap().0,
        Command::clear(false, 0xb6a5).0,
        Command::clear(true, 0x218f).0,
        Command::FLUSH.0,
        Command::memcpy(272).unwrap().0,
        Command::ui_grid_range(276, 127, 130).unwrap().0,
        Command::ui_grid(276).unwrap().0,
        Command::END.0,
    ];
    match bad {
        Some("clear_target") => code[2] = 0,
        Some("clear_cursor") => code[3] |= 1 << 17,
        Some("copy_fence") => code[5] = 0,
        Some("copy_overlap") => cells[272] = 11 | (12u64 << 14) | (2u64 << 28),
        Some("copy_end") => cells[272] = 16383 | (37u64 << 14) | (1u64 << 28),
        Some("copy_reserved") => cells[272] |= 1u64 << 39,
        Some("ui_range") => code[7] = 0xb8000000 | (749 << 7) | (748 << 17) | 69,
        Some("ui_descriptor") => cells[276] |= 1u64 << 23,
        _ => {}
    }
    for (n, pair) in code.chunks(2).enumerate() {
        cells[448 + n] = u64::from(pair[0]) | (u64::from(*pair.get(1).unwrap_or(&0)) << 32);
    }
    let mut rows = Vec::new();
    let mut q = 0;
    let mut cache = None;
    let mut flush = 0;
    let mut clear_done = VecDeque::new();
    let mut copy_done = VecDeque::new();
    let mut ui_done = None;
    let mut clear_index = [0usize; 2];
    let mut copies = 0;
    let mut uis = 0;
    let mut aborted = None;
    let mut guard_injected = false;
    let mut clear_hold = None;
    for wall in 0..100000 {
        let mut i = ProcessorInput {
            reset: wall < 2,
            ce: wall % 5 != 2 && wall % 13 != 8,
            render_idle: true,
            dma_idle: true,
            ram_ready: wall % 11 != 3,
            ram_q: q,
            auxiliary: AuxiliaryInput {
                clear_ready: wall % 7 != 3,
                clear_ok: true,
                copy_ready: wall % 5 != 3,
                ..Default::default()
            },
            ..Default::default()
        };
        if wall == 3 || wall == 4 {
            i.bus = DeviceBus {
                index: 4,
                write: true,
                channel: if wall == 3 { 0 } else { 1 },
                data: if wall == 3 {
                    (code.len() - 1) as u16
                } else {
                    0x400
                },
                ..Default::default()
            };
        }
        if cache.is_some_and(|at| wall >= at) {
            i.cache_done = true;
            i.cache_done_bank = false;
            cache = None;
        }
        if clear_done.front().is_some_and(|at| wall >= *at) {
            i.auxiliary.clear_complete = true;
            clear_done.pop_front();
        }
        if copy_done.front().is_some_and(|at| wall >= *at) {
            i.auxiliary.copy_complete = true;
            copy_done.pop_front();
        }
        if ui_done.is_some_and(|at| wall >= at) {
            i.auxiliary.ui_done = true;
        }
        let a = cp.outputs(i).auxiliary;
        let trigger = abort_kind.is_some_and(|kind| match kind {
            0 => a.clear_request && clear_index[0] == 3,
            1 => cp.phase() == Phase::CopyWait && copies == 1,
            _ => false,
        });
        if trigger && aborted.is_none() {
            aborted = Some(wall);
        }
        if aborted.is_some() {
            i.abort = true;
            i.ce = false;
        }
        if bad == Some("copy_q") && a.copy_active && !guard_injected {
            i.ram_q ^= 1;
            guard_injected = true;
        }
        if bad == Some("clear_status") && i.auxiliary.clear_complete {
            i.auxiliary.clear_ok = false;
        }
        let o = cp.outputs(i);
        let a = o.auxiliary;
        if o.cache.valid {
            i.cache_ready = true;
            if cache.is_none() {
                cache = Some(wall + 3);
            }
        }
        if o.flush_request {
            flush += 1;
            i.flush_complete = flush > 2;
        } else {
            flush = 0;
        }
        if a.clear_request {
            if let Some(old) = clear_hold {
                assert_eq!((a.clear_address, a.clear_value), old);
            }
            clear_hold = Some((a.clear_address, a.clear_value));
            if i.auxiliary.clear_ready {
                let plane = usize::from(a.clear_value == 0x218f);
                assert_eq!(
                    a.clear_address,
                    [0x100000, 0x140000][plane] + clear_index[plane] as u32 * 128
                );
                clear_index[plane] += 1;
                clear_done.push_back(wall + 4);
                clear_hold = None;
            }
        }
        if a.copy_request && i.auxiliary.copy_ready {
            let block = copies / 2;
            let dest = copies % 2 != 0;
            assert_eq!(a.copy_write, dest);
            assert_eq!(
                a.copy_address,
                Bits::new((if dest { destination } else { 11 }) + block as u64, 14)
            );
            copies += 1;
            if dest {
                copy_done.push_back(wall + 5);
            } else {
                copy_done.push_back(wall + 3);
                copy_done.push_back(wall + 6);
            }
        }
        if a.ui_start {
            assert_eq!(
                (a.ui_first, a.ui_last),
                if uis == 0 { (127, 129) } else { (0, 749) }
            );
            uis += 1;
            ui_done = Some(wall + 11);
        }
        if !long_copy || rows.is_empty() {
            rows.push(fields(&cp, i, o));
        }
        let result = cp.clock(i);
        if let Err(e) = result {
            println!("AUX_CP_ERROR wall={wall} phase={:?} {e}", cp.phase());
            return Run {
                rows,
                error: Some(e),
                clears: clear_index.into_iter().sum(),
                copies,
                uis,
            };
        }
        if o.ram_read && i.ram_ready && i.ce {
            q = cells[usize::from(o.ram_address)];
        }
        if aborted.is_some()
            && cp.phase() == Phase::Failed
            && clear_done.is_empty()
            && copy_done.is_empty()
        {
            return Run {
                rows,
                error: None,
                clears: clear_index.into_iter().sum(),
                copies,
                uis,
            };
        }
        if wall > 10 && !cp.outputs(i).busy {
            assert_eq!(clear_index, [1600, 1600]);
            assert_eq!(copies, if long_copy { 4096 } else { 6 });
            assert_eq!(uis, 2);
            return Run {
                rows,
                error: None,
                clears: 3200,
                copies,
                uis,
            };
        }
    }
    panic!("aux CP bound {:?}", cp.phase());
}
fn source(p: &[Pin]) -> String {
    let mut s = actual::module("gpu_command_processor").to_owned();
    s.push_str("\nmodule g2a(input clk");
    for f in p {
        write!(
            s,
            ",{}wire[{}:0]{}",
            if f.output { "output " } else { "input " },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    s.push_str(");gpu_command_processor dut(.clk(clk)");
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for (ext, int) in [
        ("phase", "state"),
        ("pc", "pc"),
        ("cache_pages", "cache_pages"),
        ("global_valid", "global_valid"),
        ("render_valid", "render_valid"),
        ("target_valid", "target_valid"),
        ("page_valid", "page_valid"),
        ("uniform_valid", "uniform_valid"),
        ("page_uniform", "page_uniform"),
        ("object_index", "object_index"),
    ] {
        writeln!(s, "assign obs_{ext}=dut.{int};").unwrap();
    }
    s.push_str("endmodule\n");
    s
}
fn root(label: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-aux")
        .join(format!(
            "{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn auxiliary_cp_counts_real_requests_and_retirements() {
    let r = run(None, None, false);
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!((r.clears, r.copies, r.uis), (3200, 6, 2));
}
#[test]
#[ignore = "actual full CP:allCLEAR addresses, copy/UI Q and CE0 physical retirement"]
fn auxiliary_cp_matches_production_each_wall() {
    let r = run(None, None, false);
    assert!(r.error.is_none());
    let root = root("cp");
    println!("AUX_CP_ROOT {}", root.display());
    let source = source(&r.rows[0]);
    vertex_support::rtl::replay(&root, "all_aux", r.rows, source, false, None);
}
const BAD: [(&str, &str); 9] = [
    ("clear_target", "CP plane CLEAR contract"),
    ("clear_cursor", "CP plane CLEAR contract"),
    ("copy_fence", "CP MEMCPY drain contract"),
    ("copy_overlap", "CP state data contract"),
    ("copy_end", "CP state data contract"),
    ("copy_reserved", "CP state data contract"),
    ("copy_q", "CP MEMCPY descriptor Q changed"),
    ("ui_range", "CP UI drain contract"),
    ("ui_descriptor", "CP UI descriptor"),
];
#[test]
fn auxiliary_cp_rejects_original_contracts_and_drains_abort() {
    for (bad, rule) in BAD {
        let r = run(Some(bad), None, false);
        assert_eq!(
            r.error.unwrap().to_string(),
            format!("command_processor: {rule}")
        );
    }
    let r = run(Some("clear_status"), None, false);
    assert_eq!(
        r.error.unwrap().to_string(),
        "command_processor: CP CLEAR memory response"
    );
    for kind in 0..2 {
        let r = run(None, Some(kind), false);
        assert!(r.error.is_none());
        if kind == 0 {
            assert!(r.clears < 3200);
        } else {
            assert_eq!(r.copies, 1);
        }
    }
}
#[test]
fn maximum_copy_counter_advances_all_2048_blocks() {
    let r = run(None, None, true);
    assert!(r.error.is_none());
    assert_eq!(r.copies, 4096);
}
#[test]
#[ignore = "actual CP original contracts, two CE0 stop phases and three compiled mutations"]
fn auxiliary_cp_ownership_and_counterexamples_are_sensitive() {
    let root = root("cp-negative");
    println!("AUX_CP_NEGATIVE_ROOT {}", root.display());
    for (bad, rule) in BAD
        .into_iter()
        .chain([("clear_status", "CP CLEAR memory response")])
    {
        let r = run(Some(bad), None, false);
        assert_eq!(
            r.error.unwrap().to_string(),
            format!("command_processor: {rule}")
        );
        let s = source(&r.rows[0]);
        vertex_support::rtl::replay(&root, bad, r.rows, s, false, Some(rule));
    }
    for kind in 0..2 {
        let r = run(None, Some(kind), false);
        assert!(r.error.is_none());
        let s = source(&r.rows[0]);
        vertex_support::rtl::replay(&root, &format!("abort{kind}"), r.rows, s, false, None);
    }
    for (name, old, new) in [
        (
            "clear_final_sector",
            "if(clear_sector==1599||stop)",
            "if(clear_sector==1598||stop)",
        ),
        (
            "first_read_half",
            "if(object_index==0)object_index<=1;",
            "if(object_index==0)object_index<=2;",
        ),
        (
            "copy_wall_address",
            "if(state==COPY_ADDRESS)begin",
            "if(ce&&state==COPY_ADDRESS)begin",
        ),
    ] {
        let r = run(None, None, false);
        let s = source(&r.rows[0]);
        assert_eq!(s.matches(old).count(), 1, "{name}");
        vertex_support::rtl::replay(&root, name, r.rows, s.replacen(old, new, 1), true, None);
    }
}
