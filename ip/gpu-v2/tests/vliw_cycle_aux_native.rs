//! Device 4 auxiliary commands own real CP/RAM/DMA/cache/native pin writes.
#[path = "support/vliw_aux_gpu_rtl.rs"]
mod aux_fixture;
#[path = "support/vliw_gpu_rtl.rs"]
mod fixture;
#[path = "support/vliw_gpu_native.rs"]
mod memory;
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::{
    vliw_command::{self, Command, CommandBuffer, DeviceSubmission},
    vliw_cycle::{
        command::DeviceBus,
        draw::{Gpu, Input, Output},
        postquad::Profile,
        router, Physical,
    },
};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
#[derive(Clone, Copy)]
struct Edge {
    i: Input,
    o: Output,
    phase: u8,
    pc: u8,
    pages: u8,
    installed: (bool, bool, bool, u8, u8, u8, u8),
}
fn store(b: &mut [u16], address: usize, data: &[u8]) {
    for (n, p) in data.as_chunks::<2>().0.iter().enumerate() {
        b[address / 2 + n] = u16::from_le_bytes(*p);
    }
}
fn glyph(id: usize) -> [[u16; 8]; 16] {
    std::array::from_fn(|y| {
        std::array::from_fn(|x| {
            let n = (id * 1009 + y * 137 + x * 59) as u16;
            (n.wrapping_mul(379) & !1) | u16::from(!(id + y + x).is_multiple_of(4))
        })
    })
}
fn offset(x: usize, y: usize) -> usize {
    ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16
}
fn reference_ui(b: &mut [u16], first: usize, end: usize) {
    for cell in first..end {
        let id = match cell {
            127 => 1,
            128 => 2,
            130 => 3,
            749 => 4,
            _ => continue,
        };
        let pixels = glyph(id);
        for (y, row) in pixels.iter().enumerate() {
            for (x, &p) in row.iter().enumerate() {
                if p & 1 != 0 {
                    let (r, g, blue) = (p >> 11 & 31, p >> 6 & 31, p >> 1 & 31);
                    b[0x100000 / 2 + offset(cell % 50 * 8 + x, cell / 50 * 16 + y)] =
                        r << 11 | (g * 2 + g / 16) << 5 | blue;
                }
            }
        }
    }
}
fn image(clear: bool) -> (Vec<u16>, [DeviceSubmission; 2]) {
    let mut b: Vec<_> = (0..0x400000u32)
        .map(|n| (n.wrapping_mul(0x9e3779b9).rotate_left(7) ^ (n >> 3) ^ 0x17a3) as u16)
        .collect();
    let mut objects = Vec::new();
    objects.extend_from_slice(&(2048u64 | (2560u64 << 14)).to_le_bytes());
    objects.resize(32, 0);
    objects.extend_from_slice(&vliw_command::ui_params(0x30000, 0x70000).unwrap());
    objects.extend_from_slice(&vliw_command::memcpy_params(0x50000, 0x7ffa00, 1536).unwrap());
    store(&mut b, 0x20000, &objects);
    let mut grid = [255; 768];
    for (cell, id) in [(127, 1), (128, 2), (130, 3), (749, 4)] {
        grid[cell] = id;
    }
    store(&mut b, 0x30000, &grid);
    for id in 1..=4 {
        store(
            &mut b,
            0x70000 + id * 256,
            &vliw_command::ui_char(&glyph(id)),
        );
    }
    let submissions = std::array::from_fn(|job| {
        let mut cb = CommandBuffer::new();
        for c in [
            Command::dma_local(0x20000, 264, 64).unwrap(),
            Command::dma_local(0x20040, 272, 32).unwrap(),
            Command::wait(true, true),
            Command::FLUSH,
            Command::set_target(264).unwrap(),
        ] {
            cb.push(c);
        }
        if clear {
            cb.push(Command::clear(false, 0x13a5 + job as u16));
            cb.push(Command::clear(true, 0xb628 - job as u16));
        }
        cb.push(Command::FLUSH);
        if !clear {
            cb.push(
                Command::ui_grid_range(
                    268,
                    if job == 0 { 127 } else { 749 },
                    if job == 0 { 131 } else { 750 },
                )
                .unwrap(),
            );
            cb.push(Command::memcpy(272).unwrap());
        }
        cb.push(Command::END);
        let image = cb.image().unwrap();
        let a = 0x40000 + job as u32 * 128;
        store(&mut b, a as usize, &image.storage);
        DeviceSubmission::new(a, image.valid_bytes).unwrap()
    });
    (b, submissions)
}
fn run(clear: bool, abort: u8, capture: bool) -> Vec<Edge> {
    let setup = Instant::now();
    let (b, submissions) = image(clear);
    let mut expected = b.clone();
    let mut native = memory::Memory::new(b);
    let mut gpu = Gpu::with_all_commands(Profile::Native);
    let mut rows = Vec::new();
    let (mut job, mut submission, mut prior_busy) = (0, 0, false);
    let mut stopped = None;
    let mut since = None;
    let mut ce0 = false;
    let mut clear_requests = 0;
    let mut serial_requests = 0;
    let setup_us = setup.elapsed().as_micros();
    let step = Instant::now();
    let mut cb_native_start = 0;
    for wall in 0..200000 {
        let mut i = Input {
            reset: wall < 2,
            ce: wall % 7 > 1,
            bus: DeviceBus {
                index: 4,
                channel: 2,
                read: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let cache = &gpu.render.backend.sink.cache.state;
        let should_stop = match abort {
            1 => clear_requests >= 3,
            2 => {
                gpu.control.processor.phase() as u8 == 21 && !cache.mt_write && cache.mt_retire >= 2
            }
            3 => gpu.control.processor.phase() as u8 == 21 && cache.mt_write && cache.mt_data >= 16,
            4 => gpu.ui.as_ref().unwrap().state.phase == 7 && cache.mt_write && cache.mt_issue >= 1,
            _ => false,
        };
        if stopped.is_none() && should_stop {
            stopped = Some(wall);
        }
        if stopped.is_some() {
            i.abort = true;
            i.ce = false;
        }
        if wall == 7 || job == 1 && submission == 0 {
            i.bus = DeviceBus {
                index: 4,
                channel: 0,
                write: true,
                data: submissions[job].count_minus_one,
                ..Default::default()
            };
            submission = 1;
        } else if submission == 1 {
            i.bus = DeviceBus {
                index: 4,
                channel: 1,
                write: true,
                data: submissions[job].base_sector,
                ..Default::default()
            };
            submission = 2;
        }
        let mut previous: Option<[Physical; 3]> = None;
        for settle in 0..16 {
            let o = gpu.outputs(i);
            let physical = router::Output {
                physical: o.physical,
                write_data: o.write_data,
                ..Default::default()
            };
            let f = native.feedback(physical, i.reset);
            i.physical = f;
            if previous == Some(f) {
                break;
            }
            previous = Some(f);
            assert!(settle < 15);
        }
        let o = gpu.outputs(i);
        if abort == 4 {
            if let Some(ui) = o.ui.filter(|u| u.pixel_valid) {
                for bank in 0..4 {
                    let word = usize::from(ui.pixel_word) + usize::from(ui.pixel_half) * 32;
                    let lane = bank ^ if word & 4 != 0 { 2 } else { 0 };
                    let flat = word * 4 + lane;
                    let x = usize::from(ui.tile % 25) * 16 + flat % 16;
                    let y = usize::from(ui.tile / 25) * 16 + flat / 16;
                    let cell = y / 16 * 50 + x / 8;
                    let id = match cell {
                        127 => 1,
                        128 => 2,
                        130 => 3,
                        _ => panic!("unexpected UI covered cell{cell}"),
                    };
                    let p = glyph(id)[y % 16][x % 8];
                    if p & 1 != 0 {
                        let (r, g, b) = (p >> 11 & 31, p >> 6 & 31, p >> 1 & 31);
                        expected[0x100000 / 2 + offset(x, y)] = r << 11 | (g * 2 + g / 16) << 5 | b;
                    }
                }
            }
        }
        if capture {
            rows.push(Edge {
                i,
                o,
                phase: gpu.control.processor.phase() as u8,
                pc: gpu.control.processor.pc(),
                pages: gpu.control.processor.cache_pages(),
                installed: gpu.control.processor.draw_status().unwrap(),
            });
        }
        clear_requests += usize::from(
            o.control.processor.auxiliary.clear_active
                && o.physical[2].valid
                && i.physical[2].request_ready,
        );
        serial_requests += usize::from(
            o.control.processor.auxiliary.copy_active
                && o.render.backend.sink.cache.mem_req_valid
                && o.control.transport.router.framebuffer_ready,
        );
        ce0 |= !i.ce
            && i.physical
                .iter()
                .any(|p| p.response.valid && p.response.last);
        let ended = stopped.is_none() && submission == 2 && prior_busy && !o.control.processor.busy;
        if ended {
            assert!(o.physical_drained && o.render.idle);
            if clear {
                expected[0x100000 / 2..0x100000 / 2 + 102400].fill(0x13a5 + job as u16);
                expected[0x140000 / 2..0x140000 / 2 + 102400].fill(0xb628 - job as u16);
            } else {
                reference_ui(
                    &mut expected,
                    if job == 0 { 127 } else { 749 },
                    if job == 0 { 131 } else { 750 },
                );
                expected.copy_within(0x50000 / 2..0x50000 / 2 + 768, 0x7ffa00 / 2);
            }
            if let Some((a, (got, want))) = native
                .backing()
                .iter()
                .zip(&expected)
                .enumerate()
                .find(|(_, (a, b))| a != b)
            {
                panic!(
                    "aux clear{clear} CB{job} byte{:x} {got:04x}!={want:04x}",
                    a * 2
                );
            }
        }
        prior_busy = o.control.processor.busy;
        let result = gpu.clock(i);
        native.clock();
        result.unwrap_or_else(|e| panic!("aux wall{wall} CB{job}: {e}"));
        if stopped.is_some() && o.physical_drained {
            let start = *since.get_or_insert(wall);
            if wall >= start + 5 {
                assert!(ce0);
                match abort {
                    1 => {
                        assert!((3..=5).contains(&clear_requests));
                        for n in 0..clear_requests * 64 {
                            expected[0x100000 / 2 + n] = 0x13a5;
                        }
                    }
                    2 | 3 => {
                        reference_ui(&mut expected, 127, 131);
                        if abort == 3 {
                            expected.copy_within(0x50000 / 2..0x50000 / 2 + 256, 0x7ffa00 / 2);
                        }
                        assert_eq!(serial_requests, if abort == 2 { 4 } else { 8 });
                    }
                    4 => assert_eq!(serial_requests, 0),
                    _ => unreachable!(),
                }
                assert_eq!(native.backing(), expected);
                println!(
                    "AUX_ABORT kind={abort} walls={} clear_requests={clear_requests} serial_requests={serial_requests}",
                    wall + 1
                );
                return rows;
            }
        }
        if ended {
            if job == 0 && !(capture && clear) {
                job = 1;
                submission = 0;
                cb_native_start = native.native_cycles();
            } else {
                assert!(ce0);
                assert_eq!(
                    clear_requests,
                    if clear {
                        if capture {
                            3200
                        } else {
                            6400
                        }
                    } else {
                        0
                    }
                );
                assert_eq!(serial_requests, if clear { 0 } else { 48 });
                println!("AUX_NATIVE clear={clear} walls={} native={} setup_us={setup_us} step_us={} write_accepts={} RSS=unmeasured",wall+1,native.native_cycles(),step.elapsed().as_micros(),native.write_accepts);
                return rows;
            }
        }
        assert!(
            native.native_cycles() - cb_native_start < 200000,
            "one auxiliary CB exceeded200000 native cycles"
        );
        assert!(
            step.elapsed().as_secs() < 60,
            "native auxiliary60s wall{wall}"
        );
    }
    panic!("auxiliary exceeded200000 walls")
}
fn root(label: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-aux")
        .join(format!(
            "{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn both_clear_planes_and_two_no_ack_submissions_match_whole_backing() {
    run(true, 0, false);
}
#[test]
fn ranged_ui_and_multiblock_copy_reach_native_pin_bytes() {
    run(false, 0, false);
}
#[test]
fn ce0_clear_abort_retains_offered_and_accepted_writes() {
    run(true, 1, false);
}
#[test]
fn ce0_copy_read_write_and_ui_write_abort_drain_the_owned_phase() {
    for kind in 2..=4 {
        run(false, kind, false);
    }
}
#[test]
#[ignore = "whole production GPU auxiliary ownership on actual native feedback,60s per DUT"]
fn full_auxiliary_roots_match_native_feedback() {
    let root = root("whole");
    println!("AUX_WHOLE_ROOT {}", root.display());
    for (name, clear, abort) in [
        ("ui-copy", false, 0),
        ("clear-abort", true, 1),
        ("both-clear-planes", true, 0),
        ("ui-write-abort", false, 4),
    ] {
        let r = run(clear, abort, true);
        let schema = aux_fixture::fields(&r[0]);
        vertex_support::rtl::replay_whole(
            &root,
            name,
            r.iter().map(aux_fixture::fields).collect(),
            aux_fixture::production(&schema),
            false,
            None,
        );
    }
}
#[test]
#[ignore = "three actual top-level ownership mux mutations,60s per DUT"]
fn whole_auxiliary_mux_mutations_are_observable() {
    let root = root("whole-negative");
    println!("AUX_WHOLE_NEGATIVE_ROOT {}", root.display());
    for (name, clear, abort, old, new) in [
        (
            "ui_cp_mux",
            false,
            0,
            "assign cp_re=ui_active?ui_cp_re:processor_cp_re;",
            "assign cp_re=processor_cp_re;",
        ),
        (
            "ui_dma_mux",
            false,
            0,
            "assign work_valid=ui_active?ui_work_valid:processor_work_valid;",
            "assign work_valid=processor_work_valid;",
        ),
        (
            "clear_feedback_mux",
            true,
            1,
            ".fb_w_response_valid(fb_w_response_valid&&!clear_active)",
            ".fb_w_response_valid(fb_w_response_valid)",
        ),
    ] {
        let r = run(clear, abort, true);
        let schema = aux_fixture::fields(&r[0]);
        let s = aux_fixture::production(&schema);
        assert_eq!(s.matches(old).count(), 1, "{name}");
        let original_monitor = if name == "clear_feedback_mux" {
            Some("GPU contract: fault")
        } else {
            None
        };
        vertex_support::rtl::replay_whole(
            &root,
            name,
            r.iter().map(aux_fixture::fields).collect(),
            s.replacen(old, new, 1),
            original_monitor.is_none(),
            original_monitor,
        );
    }
}
