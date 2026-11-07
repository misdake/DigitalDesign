//! True Device4/CP/DMA/RGB DRAW with one native arbiter/endpoint/MC/pin backing.
#[path = "../support/vliw_gpu_rtl.rs"]
mod fixture;
#[path = "../support/vliw_gpu_native.rs"]
mod memory;
#[path = "../support/profiling_journal.rs"]
mod profiling_journal;
#[path = "../support/vliw_tiny.rs"]
mod tiny;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_triangle/mod.rs"]
mod triangle_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::{
    system::vliw_command::{Command, CommandBuffer, DeviceSubmission},
    system::vliw_cycle::{
        command::DeviceBus,
        draw::{Gpu, Input, Output},
        postquad::Profile,
        Physical,
    },
};
#[derive(Clone, Copy)]
struct Edge {
    i: Input,
    o: Output,
    phase: u8,
    pc: u8,
    pages: u8,
    installed: (bool, bool, bool, u8, u8, u8, u8),
}
struct Run {
    rows: Vec<Edge>,
    setup_us: u128,
    step_us: u128,
    preimage: Vec<u16>,
    journal: Vec<(
        u64,
        digital_design_hardware_gowin::sdram_memory_controller::cycle::pins::Write,
    )>,
    image_oracles: Vec<(usize, Vec<u8>, Vec<u8>)>,
}
fn store(backing: &mut [u16], address: usize, bytes: &[u8]) {
    assert!(address.is_multiple_of(2) && bytes.len().is_multiple_of(2));
    for (n, b) in bytes.as_chunks::<2>().0.iter().enumerate() {
        backing[address / 2 + n] = u16::from_le_bytes([b[0], b[1]]);
    }
}
fn offset(x: usize, y: usize) -> usize {
    ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16
}
fn color(tint: u32, lit: bool) -> u16 {
    (0..3).fold(0, |p, c| {
        let v = (((tint >> (8 * c)) & 255) * ((0xa1b2c3 >> (8 * c)) & 255) * 2 + 255) / 510;
        let v = if lit { (v * 129) >> 8 } else { v };
        let max = if c == 1 { 63 } else { 31 };
        p | ((((v * max * 2 + 255) / 510) as u16) << [11, 5, 0][c])
    })
}
fn input_image(
    lit: bool,
) -> (
    Vec<u16>,
    [DeviceSubmission; 2],
    std::collections::BTreeSet<(usize, usize)>,
) {
    let tiny = tiny::data();
    let coverage = tiny::coverage(&tiny);
    assert_eq!(coverage.len(), 6);
    let mut backing: Vec<u16> = (0..0x400000)
        .map(|n| (n as u16).wrapping_mul(731).wrapping_add(0x1234))
        .collect();
    let mut objects = [0u64; 24];
    objects[0] = 2048 | (2560u64 << 14);
    objects[8] = (100u64 << 16) | (((-8192i16) as u16 as u64) << 32) | (1234u64 << 48);
    objects[9] = 0x4000 | (0x1234u64 << 16) | (0xf500u64 << 32) | (0x4000u64 << 48);
    objects[10] = 129 | (127u64 << 16) | (1u64 << 32) | (12u64 << 34) | (7u64 << 48);
    objects[16] = (6 << 16) | (1 << 20) | (0xabcdefu64 << 32);
    objects[17] = 7 | (u64::from(!lit) << 5) | (u64::from((-3i8) as u8) << 32);
    store(
        &mut backing,
        0x20000,
        &objects
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    for (job, tint) in [0x927bad31u32, 0x42805020].into_iter().enumerate() {
        let mut uniform = [0u64; 16];
        for &(row, data) in &tiny.rows {
            if (320..336).contains(&row) {
                uniform[usize::from(row - 320)] = data;
            }
        }
        uniform[11] = tint.into();
        store(
            &mut backing,
            0x21000 + job * 256,
            &uniform
                .into_iter()
                .flat_map(u64::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    let mut record = [0u64; 32];
    for &(row, data) in &tiny.rows {
        if (256..260).contains(&row) {
            record[usize::from(row - 256)] = data;
        } else if row < 16 {
            record[16 + usize::from(row)] = data;
        }
    }
    store(
        &mut backing,
        0x22000,
        &record
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let submissions = std::array::from_fn(|job| {
        let mut cb = CommandBuffer::new();
        for c in [
            Command::dma_local(0x20000, 264, 128).unwrap(),
            Command::dma_local(0x20080, 280, 32).unwrap(),
            Command::dma_local(0x21000 + job as u32 * 256, 320, 128).unwrap(),
            Command::wait(true, true),
            Command::FLUSH,
            Command::set_target(264).unwrap(),
            Command::set_global(272).unwrap(),
            Command::set_render(280).unwrap(),
            Command::TEX_INVALIDATE,
            Command::dma_meshlet(0x22000, job as u8, 128).unwrap(),
            Command::wait(true, false),
            Command::draw_meshlet(job as u8).unwrap(),
            Command::wait(true, true),
            Command::FLUSH,
            Command::END,
        ] {
            cb.push(c);
        }
        let image = cb.image().unwrap();
        let base = 0x40000 + job as u32 * 256;
        store(&mut backing, base as usize, &image.storage);
        DeviceSubmission::new(base, image.valid_bytes).unwrap()
    });
    (backing, submissions, coverage)
}
fn run() -> Run {
    run_mode(false)
}
fn run_mode(lit: bool) -> Run {
    let setup_start = std::time::Instant::now();
    let (backing, submissions, coverage) = input_image(lit);
    let preimage = backing.clone();
    let mut image_oracles = Vec::new();
    let mut expected = backing.clone();
    let mut native = memory::Memory::new(backing);
    let mut gpu = Gpu::with_draws(Profile::Native);
    let mut phase = 0;
    let mut job = 0;
    let mut prior_busy = false;
    let mut completed = 0;
    let mut starts = 0;
    let mut final_pixels = 0;
    let mut light_accepted = 0;
    let mut light_returned = 0;
    let mut rows = Vec::new();
    let mut partial_visible = false;
    let mut draw_active_ce0 = false;
    let mut ended = false;
    let setup_us = setup_start.elapsed().as_micros();
    let step_start = std::time::Instant::now();
    for wall in 0..200000usize {
        let ce = wall % 7 > 1;
        let mut i = Input {
            reset: wall < 2,
            ce,
            bus: DeviceBus {
                index: 4,
                channel: 2,
                read: true,
                ..Default::default()
            },
            ..Default::default()
        };
        if wall == 7 || job == 1 && phase == 0 {
            i.bus = DeviceBus {
                index: 4,
                channel: 0,
                write: true,
                data: submissions[job].count_minus_one,
                ..Default::default()
            };
            phase = 1;
        } else if phase == 1 {
            i.bus = DeviceBus {
                index: 4,
                channel: 1,
                write: true,
                data: submissions[job].base_sector,
                ..Default::default()
            };
            phase = 2;
        }
        let mut previous: Option<[Physical; 3]> = None;
        for settle in 0..16 {
            let o = gpu.outputs(i);
            let feedback = native.feedback(o.control.transport.router, i.reset);
            if previous == Some(feedback) {
                i.physical = feedback;
                break;
            }
            i.physical = feedback;
            previous = Some(feedback);
            assert!(settle < 15, "whole GPU/native feedback exceeded16");
        }
        let o = gpu.outputs(i);
        rows.push(Edge {
            i,
            o,
            phase: gpu.control.processor.phase() as u8,
            pc: gpu.control.processor.pc(),
            pages: gpu.control.processor.cache_pages(),
            installed: gpu.control.processor.draw_status().unwrap(),
        });
        if !i.reset {
            assert_eq!(o.control.device_read_data & 2, 0);
            starts += usize::from(
                i.ce && o.control.processor.start && o.render.producer.frontend.meshlet.ready,
            );
            final_pixels += usize::from(o.render.backend.final_result);
            if let Some(l) = o.render.light {
                light_accepted += usize::from(l.pixel_accepted);
                light_returned += usize::from(i.ce && l.result.valid);
            }
            draw_active_ce0 |= !i.ce && o.render.producer.pool.global_used != 0;
            if phase == 2 && prior_busy && !o.control.processor.busy {
                assert!(o.render.idle && o.physical_drained);
                let packed = color([0x927bad31, 0x42805020][job], lit);
                for &(x, y) in &coverage {
                    expected[0x100000 / 2 + offset(x, y)] = packed;
                }
                if let Some((at, (a, b))) = native
                    .backing()
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .find(|(_, (a, b))| a != b)
                {
                    panic!("CB{job} physical byte{:x}: {a:04x}!={b:04x}", at * 2);
                }
                completed += 1;
                let plane = |base: usize| {
                    expected[base / 2..base / 2 + 102400]
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect()
                };
                image_oracles.push((wall, plane(0x100000), plane(0x140000)));
                if job == 0 {
                    job = 1;
                    phase = 0;
                } else {
                    ended = true;
                }
            }
        }
        prior_busy = o.control.processor.busy;
        let result = gpu.clock(i);
        native.clock();
        result.unwrap_or_else(|e| {
            panic!(
                "wall{wall} CB{job} state{:?}: {e}",
                gpu.control.processor.phase()
            )
        });
        let matches = coverage
            .iter()
            .filter(|&&(x, y)| {
                native.backing()[0x100000 / 2 + offset(x, y)]
                    == color([0x927bad31, 0x42805020][job], lit)
            })
            .count();
        partial_visible |= matches > 0 && matches < coverage.len();
        if ended {
            assert_eq!((completed, starts, final_pixels), (2, 2, 12));
            assert_eq!(
                (light_accepted, light_returned),
                if lit { (12, 12) } else { (0, 0) }
            );
            assert!(draw_active_ce0 && partial_visible);
            assert!(
                native.write_accepts >= 4
                    && native.changed_half_edges >= 4
                    && native.changed_halfwords >= 12
            );
            println!("NATIVE_RGB_CP lit={lit} light_accept={light_accepted} light_return={light_returned} walls={} native={} starts={starts} pixels={final_pixels} changed_halves={} changed_words={} write_accepts={}",
                wall+1,native.native_cycles(),native.changed_half_edges,native.changed_halfwords,native.write_accepts);
            let step_us = step_start.elapsed().as_micros();
            println!("NATIVE_RGB_HOST setup_us={setup_us} step_us={step_us} boundary=fixture_and_whole_backing_checks RSS=unmeasured");
            return Run {
                rows,
                setup_us,
                step_us,
                preimage,
                journal: native.journal,
                image_oracles,
            };
        }
    }
    panic!("native CP RGB DRAW exceeded200000 walls CB{job}/phase{phase}/completed{completed}");
}
#[test]
fn device4_cp_dma_draw_and_flush_use_one_native_pin_backing() {
    run();
}
#[test]
fn actual_pool_lighting_and_final_use_installed_live_globals() {
    run_mode(true);
}
#[test]
fn passive_gpu_journal_matches_every_original_edge_and_final_state() {
    let run = run_mode(true); // Existing independent physical color/guard oracle.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/frame-reports/actual-gpu");
    std::fs::create_dir_all(&root).unwrap();
    profiling_journal::memory(&root, &run.preimage, &run.journal, &run.image_oracles);
    let kinds = profiling_journal::record(
        &root.join("gpu-signals.jsonl"),
        Gpu::with_draws(Profile::Native),
        run.rows.iter().map(|e| (e.i, e.o)),
    );
    assert_eq!(kinds["draw"], 2);
    assert_eq!(kinds["q_alloc"], kinds["q_publish"]);
    assert_eq!(kinds["q_alloc"], kinds["q_retire"]);
    assert_eq!(kinds["rop_begin"], kinds["q_commit"]);
    assert!(kinds["cache_write"] >= 12 && kinds["dma_request"] > 2);
    println!("PASSIVE_GPU_JOURNAL {:?} {:?}", root, kinds);
}
#[test]
#[ignore = "bounded same-input RTL/emu common profiling event comparison"]
fn common_profiling_collectors_observe_the_same_draw_edges() {
    let run = run_mode(true);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/common-protocol")
        .join(format!("same-input-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let emu = root.join("emu");
    let rtl = root.join("rtl-capture");
    std::fs::create_dir(&emu).unwrap();
    std::fs::create_dir(&rtl).unwrap();
    std::fs::write(
        emu.join("initial-memory.bin"),
        run.preimage
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    profiling_journal::record_common_native(
        &emu.join("events.jsonl"),
        Gpu::with_draws(Profile::Native),
        run.rows.iter().map(|e| (e.i, e.o)),
        &run.journal,
    );
    let observer = profiling_journal::observer(&rtl.join("events.jsonl"), 2, run.rows.len() - 1);
    let pins = fixture::fields(&run.rows[0]);
    vertex_support::rtl::replay_profile(
        &root,
        "rtl",
        run.rows.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        &observer,
    );
    let raw = std::fs::read_to_string(rtl.join("events.jsonl")).unwrap();
    let last = raw.lines().last().unwrap();
    assert!(last.ends_with(",\"capture_end\",true]"));
    let end = last
        .trim_start_matches('[')
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    profiling_journal::common_metadata(&rtl, end, "gpu-rtl", true);
    println!("COMMON_PROFILING_ROOT {}", root.display());
    // Stop on the real first DRAW release, while its T/result owners remain
    // live. Both collectors see precisely this finite prefix, with no drain.
    let cut = run.rows.iter().position(|e| e.pc == 12).unwrap();
    let prefix = &run.rows[..cut];
    let partial = root.join("draw-release-prefix");
    let emu = partial.join("emu");
    let rtl = partial.join("rtl-capture");
    std::fs::create_dir_all(&emu).unwrap();
    std::fs::create_dir(&rtl).unwrap();
    profiling_journal::record_common(
        &emu.join("events.jsonl"),
        Gpu::with_draws(Profile::Native),
        prefix.iter().map(|e| (e.i, e.o)),
    );
    let observer = profiling_journal::observer(&rtl.join("events.jsonl"), 2, prefix.len() - 1);
    vertex_support::rtl::replay_profile(
        &partial,
        "rtl",
        prefix.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        &observer,
    );
    let raw = std::fs::read_to_string(rtl.join("events.jsonl")).unwrap();
    let last = raw.lines().last().unwrap();
    assert!(last.ends_with(",\"capture_end\",false]"));
    let end = last
        .trim_start_matches('[')
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    profiling_journal::common_metadata(&rtl, end, "gpu-rtl", false);
    println!("COMMON_PROFILING_ROOT {}", partial.display());
}
#[test]
#[ignore = "bounded actual reset with live T owners in both common collectors"]
fn common_profiling_reset_preserves_the_same_stop_boundary() {
    let run = run_mode(true);
    let cut = run.rows.iter().position(|e| e.pc == 12).unwrap();
    let mut rows = run.rows[..cut].to_vec();
    let mut gpu = Gpu::with_draws(Profile::Native);
    for e in &rows {
        gpu.clock(e.i).unwrap();
    }
    {
        let i = Input {
            reset: true,
            ce: false,
            physical: [Physical {
                request_ready: true,
                write_ready: true,
                response: gpu_v2::system::vliw_cycle::Response {
                    valid: true,
                    last: true,
                    data: 0x1234,
                    ..Default::default()
                },
            }; 3],
            ..Default::default()
        };
        rows.push(Edge {
            i,
            o: gpu.outputs(i),
            phase: gpu.control.processor.phase() as u8,
            pc: gpu.control.processor.pc(),
            pages: gpu.control.processor.cache_pages(),
            installed: gpu.control.processor.draw_status().unwrap(),
        });
        gpu.clock(i).unwrap();
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/common-protocol")
        .join(format!("reset-{}", std::process::id()));
    let emu = root.join("emu");
    let rtl = root.join("rtl-capture");
    std::fs::create_dir_all(&emu).unwrap();
    std::fs::create_dir(&rtl).unwrap();
    profiling_journal::record_common(
        &emu.join("events.jsonl"),
        Gpu::with_draws(Profile::Native),
        rows.iter().map(|e| (e.i, e.o)),
    );
    let observer = profiling_journal::observer(&rtl.join("events.jsonl"), 1, rows.len() - 1);
    vertex_support::rtl::replay_profile(
        &root,
        "rtl",
        rows.iter().map(fixture::fields).collect(),
        fixture::production(&fixture::fields(&rows[0])),
        &observer,
    );
    let raw = std::fs::read_to_string(rtl.join("events.jsonl")).unwrap();
    let last = raw.lines().last().unwrap();
    assert!(last.ends_with(",\"capture_end\",false]"));
    let end = last
        .trim_start_matches('[')
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    profiling_journal::common_metadata(&rtl, end, "gpu-rtl", false);
    println!("COMMON_PROFILING_ROOT {}", root.display());
}

#[test]
#[ignore = "one actual whole command GPU with covered Lighting tuples and native feedback"]
fn entire_lit_rgb_gpu_matches_native_feedback() {
    let run = run_mode(true);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "native-lit-rgb-root-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let pins = fixture::fields(&run.rows[0]);
    vertex_support::rtl::replay(
        &root,
        "lit_RGB_CBs",
        run.rows.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        false,
        None,
    );
    println!("NATIVE_LIT_RGB_ROOT {}", root.display());
}
#[test]
#[ignore = "one actual CP-to-router DRAW DUT against live native-MC feedback; finite12844-edge fixture"]
fn entire_command_gpu_matches_native_feedback() {
    use std::time::{Instant, SystemTime, UNIX_EPOCH};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "native-rgb-root-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let first = run();
    let second = run();
    assert_eq!(first.rows.len(), second.rows.len());
    let export = Instant::now();
    let pins = fixture::fields(&first.rows[0]);
    let source = fixture::production(&pins);
    let vectors = first.rows.iter().map(fixture::fields).collect();
    let export_us = export.elapsed().as_micros();
    std::fs::write(root.join("host.json"),format!("{{\"scope\":\"native RGB/CP fixture, includes settling, pin observers and whole-backing assertions\",\"host_contention\":\"uncontrolled\",\"rss\":null,\"samples\":[{{\"setup_us\":{},\"step_us\":{}}},{{\"setup_us\":{},\"step_us\":{}}}],\"export_us\":{export_us},\"wall_edges\":{}}}\n",first.setup_us,first.step_us,second.setup_us,second.step_us,first.rows.len())).unwrap();
    vertex_support::rtl::replay(&root, "two_rgb_CBs", vectors, source, false, None);
    println!("NATIVE_RGB_ROOT_CHECKPOINT {}", root.display());
}

#[test]
#[ignore = "two actual whole-root wiring/submission mutations and original CE0 COUNT monitor"]
fn command_gpu_wiring_mutations_and_busy_count_monitor() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "native-rgb-negative-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let run = run();
    let pins = fixture::fields(&run.rows[0]);
    for name in ["wrong_color_target_wire", "retain_installed_valid"] {
        let mut source = fixture::production(&pins).replace("\r\n", "\n");
        let (start, end, old, new) = if name == "wrong_color_target_wire" {
            let start = source.find(" gpu_v2_vliw_three_port core(").unwrap();
            let end = start + source[start..].find(");").unwrap();
            (
                start,
                end,
                ".color_base(color_base)",
                ".color_base(depth_base)",
            )
        } else {
            let start = source.find("if(submit&&submit_ready)begin").unwrap();
            let end = start + source[start..].find("else if(ce&&!stop)").unwrap();
            (
                start,
                end,
                "global_valid<=0;render_valid<=0;stored<=0;target_valid<=0;",
                "stored<=0;",
            )
        };
        assert_eq!(source[start..end].matches(old).count(), 1, "{name}");
        let body = source[start..end].replacen(old, new, 1);
        source.replace_range(start..end, &body);
        vertex_support::rtl::replay(
            &root,
            name,
            run.rows.iter().map(fixture::fields).collect(),
            source,
            true,
            None,
        );
    }
    let at = run
        .rows
        .iter()
        .position(|e| !e.i.reset && !e.i.ce && e.o.control.processor.busy)
        .unwrap();
    let mut gpu = Gpu::with_draws(Profile::Native);
    for e in &run.rows[..at] {
        gpu.clock(e.i).unwrap();
    }
    let mut i = run.rows[at].i;
    i.bus = DeviceBus {
        index: 4,
        channel: 0,
        write: true,
        data: 14,
        ..Default::default()
    };
    let last = Edge {
        i,
        o: gpu.outputs(i),
        ..run.rows[at]
    };
    let mut vectors: Vec<_> = run.rows[..at].iter().map(fixture::fields).collect();
    vectors.push(fixture::fields(&last));
    let error = gpu.clock(i).unwrap_err();
    assert_eq!(
        error.to_string(),
        "command_processor: CP count SEND contract"
    );
    vertex_support::rtl::replay(
        &root,
        "busy_COUNT_CE0",
        vectors,
        fixture::production(&pins),
        false,
        Some("CP count SEND contract"),
    );
    println!("NATIVE_RGB_NEGATIVE_CHECKPOINT {}", root.display());
}
