//! Actual CP -> RGB then UV/lit -> depth/Q6 -> native pin writes, two CBs.
#[path = "../support/vliw_aux_gpu_rtl.rs"]
mod aux_fixture;
#[path = "../support/vliw_gpu_rtl.rs"]
mod fixture;
#[path = "../support/vliw_gpu_native.rs"]
mod memory;
#[path = "../support/profiling_journal.rs"]
mod profiling_journal;
#[path = "../support/vliw_draw_reference.rs"]
mod reference;
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
use gpu_v2::system::{
    vliw_command::{Command, CommandBuffer, DeviceSubmission},
    vliw_cycle::{
        command::DeviceBus,
        draw::{Gpu, Input, Output},
        postquad::Profile,
        vertex::Bits,
        Physical,
    },
};
use std::collections::BTreeSet;
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
    total_us: u128,
    preimage: Vec<u16>,
    journal: Vec<(
        u64,
        digital_design_hardware_gowin::sdram_memory_controller::cycle::pins::Write,
    )>,
    image_oracles: Vec<(usize, Vec<u8>, Vec<u8>)>,
}
const TINT: [u32; 2] = [0x8192_ad31, 0x4280_5020];
type Coverage = [BTreeSet<(usize, usize)>; 2];
fn store(backing: &mut [u16], address: usize, bytes: &[u8]) {
    assert!(address.is_multiple_of(2) && bytes.len().is_multiple_of(2));
    for (n, b) in bytes.as_chunks::<2>().0.iter().enumerate() {
        backing[address / 2 + n] = u16::from_le_bytes(*b);
    }
}
fn offset(x: usize, y: usize) -> usize {
    ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16
}
fn image(uv_repeats: usize, auxiliary: u8) -> (Vec<u16>, [DeviceSubmission; 2], Coverage) {
    let data = [tiny::data(), tiny::uv()];
    let coverage = data.each_ref().map(tiny::coverage);
    let overlap = coverage[0].intersection(&coverage[1]).count();
    assert!(overlap > 0 && overlap < coverage[0].len() && overlap < coverage[1].len());
    let mut backing: Vec<_> = (0..0x400000u32)
        .map(|n| ((n.wrapping_mul(0x9e37_79b9).rotate_left(11) ^ (n >> 5) ^ 0xa731) & 65535) as u16)
        .collect();
    let mut objects = [0u64; 32];
    objects[0] = 2048 | (2560u64 << 14);
    objects[8] = (100u64 << 16) | (u64::from(-9830i16 as u16) << 32);
    objects[9] = 13107 | (8192u64 << 48);
    objects[10] = 64 | (96u64 << 16) | (3u64 << 34) | (19u64 << 48);
    objects[16] = 6 << 16;
    objects[17] = 7 | 32;
    objects[20] = 0xc00 | (6 << 16) | (1 << 21) | (0x302010u64 << 32);
    objects[21] = 7 | 8 | 16;
    objects[24] = (0x30000 / 128) | (7 << 16);
    objects[28] = (0x60000 / 512) | ((0x7ffa00u64 / 512) << 14) | (2 << 28);
    store(
        &mut backing,
        0x20000,
        &objects
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    for (job, tint) in TINT.into_iter().enumerate() {
        let mut uniform = [0u64; 16];
        for &(row, value) in &data[1].rows {
            if (320..336).contains(&row) {
                uniform[usize::from(row - 320)] = value;
            }
        }
        uniform[11] = u64::from(tint);
        store(
            &mut backing,
            0x21000 + job * 256,
            &uniform
                .into_iter()
                .flat_map(u64::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    for (kind, d) in data.iter().enumerate() {
        let mut record = [0u64; 32];
        for &(row, value) in &d.rows {
            if (256..260).contains(&row) {
                record[usize::from(row - 256)] = value;
            } else if row < 16 {
                record[16 + usize::from(row)] = value;
            }
        }
        store(
            &mut backing,
            0x22000 + kind * 256,
            &record
                .into_iter()
                .flat_map(u64::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    for x in 0..64 {
        for y in 0..64 {
            let at = ((x / 16) * 4 + y / 16) * 256 + (y % 16) * 16 + x % 16;
            backing[0x60000 / 2 + at] = reference::texel(x, y);
        }
    }
    if auxiliary > 0 {
        let mut grid = [255; 768];
        grid[749] = 1;
        store(&mut backing, 0x30000, &grid);
        let pixels = [[0x17a3; 8]; 16];
        store(
            &mut backing,
            0x70100,
            &gpu_v2::system::vliw_command::ui_char(&pixels),
        );
    }
    let submissions = std::array::from_fn(|job| {
        let mut cb = CommandBuffer::new();
        if auxiliary > 0 {
            cb.push(Command::dma_local(0x200c0, 288, 64).unwrap());
        }
        for (index, command) in [
            Command::dma_local(0x20000, 264, 128).unwrap(),
            Command::dma_local(0x20080, 280, 64).unwrap(),
            Command::dma_local(0x21000 + job as u32 * 256, 320, 128).unwrap(),
            Command::wait(true, true),
            Command::FLUSH,
            Command::set_target(264).unwrap(),
            Command::set_global(272).unwrap(),
            Command::set_render(280).unwrap(),
            Command::TEX_INVALIDATE,
            Command::dma_meshlet(0x22000, 0, 128).unwrap(),
            Command::wait(true, false),
            Command::draw_meshlet(0).unwrap(),
            Command::wait(true, true),
            Command::set_render(284).unwrap(),
            Command::dma_meshlet(0x22100, 1, 128).unwrap(),
            Command::wait(true, false),
            Command::draw_meshlet(1).unwrap(),
            Command::wait(true, true),
        ]
        .into_iter()
        .enumerate()
        {
            cb.push(command);
            if auxiliary == 1 && index == 5 {
                cb.push(Command::clear(false, 0x1357 + job as u16));
                cb.push(Command::clear(true, 1024));
                cb.push(Command::FLUSH);
            }
        }
        for _ in 1..uv_repeats {
            cb.push(Command::draw_meshlet(1).unwrap());
            cb.push(Command::wait(true, true));
        }
        cb.push(Command::FLUSH);
        if auxiliary > 0 {
            cb.push(Command::ui_grid(288).unwrap());
            cb.push(Command::memcpy(292).unwrap());
        }
        cb.push(Command::END);
        let image = cb.image().unwrap();
        let address = 0x40000 + job as u32 * 256;
        store(&mut backing, address as usize, &image.storage);
        DeviceSubmission::new(address, image.valid_bytes).unwrap()
    });
    (backing, submissions, coverage)
}
fn update_expected(
    expected: &mut [u16],
    coverage: &[BTreeSet<(usize, usize)>; 2],
    job: usize,
    light: [u16; 2],
    uv_repeats: usize,
) {
    let rgb = reference::final_rgb(TINT[job], 0xa1b2c3, [256, 0], 0);
    for &(x, y) in &coverage[0] {
        let at = 0x100000 / 2 + offset(x, y);
        expected[at] = reference::rop(rgb, expected[at], 255, false);
    }
    let rgb = reference::final_rgb(TINT[job], reference::sampled(), light, 0x302010);
    for _ in 0..uv_repeats {
        for &(x, y) in &coverage[1] {
            let at = 0x100000 / 2 + offset(x, y);
            expected[at] = reference::rop(rgb, expected[at], (TINT[job] >> 24) as u8, true);
            expected[0x140000 / 2 + offset(x, y)] = 1024;
        }
    }
}
fn run() -> Vec<Edge> {
    run_case(1, false).rows
}
fn run_case(uv_repeats: usize, stop: bool) -> Run {
    run_case_with_auxiliary(uv_repeats, stop, 0)
}
fn run_case_with_auxiliary(uv_repeats: usize, stop: bool, auxiliary: u8) -> Run {
    let setup_start = std::time::Instant::now();
    let (backing, submissions, coverage) = image(uv_repeats, auxiliary);
    let preimage = backing.clone();
    let mut image_oracles = Vec::new();
    let mut expected = backing.clone();
    let light = reference::light();
    assert!(light[0] != 256 && light[1] > 0);
    let sampled = reference::sampled();
    let mut native = memory::Memory::new(backing);
    let mut gpu = if auxiliary > 0 {
        Gpu::with_all_commands(Profile::Native)
    } else {
        Gpu::with_draws(Profile::Native)
    };
    let mut rows = Vec::new();
    let (mut job, mut phase, mut previous_busy, mut ended) = (0, 0, false, false);
    let (mut starts, mut samples, mut lights, mut final_pixels, mut texture_sectors) =
        (0, 0, 0, 0, 0);
    let mut partial = false;
    let mut active_ce0 = false;
    let mut upper_sources = 0;
    let mut source_wraps = 0;
    let mut last_public = None;
    let mut aborted = None;
    let mut drained_since = None;
    let setup_us = setup_start.elapsed().as_micros();
    let step_start = std::time::Instant::now();
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
        if stop
            && aborted.is_none()
            && gpu
                .render
                .sampling
                .as_ref()
                .is_some_and(|s| s.cache.desc_started && s.cache.desc_next >= 4)
        {
            aborted = Some(wall);
        }
        if aborted.is_some() {
            i.abort = true;
            i.ce = false;
        }
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
            let mut physical = o.control.transport.router;
            physical.physical = o.physical;
            physical.write_data = o.write_data;
            let f = native.feedback(physical, i.reset);
            i.physical = f;
            if previous == Some(f) {
                break;
            }
            previous = Some(f);
            assert!(settle < 15, "DRAW/native feedback exceeded16");
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
        if !i.reset && aborted.is_none() {
            assert_eq!(o.control.device_read_data & 2, 0, "fault wall{wall}");
            starts += usize::from(
                i.ce && o.control.processor.start && o.render.producer.frontend.meshlet.ready,
            );
            final_pixels += usize::from(o.render.backend.final_result);
            active_ce0 |= !i.ce && o.render.producer.pool.global_used > 0;
            if let Some(s) = o.render.sampling {
                samples += usize::from(s.color.result_fire);
                texture_sectors +=
                    usize::from(s.cache.mem_req_valid && o.control.transport.router.texture_ready);
                if s.result.valid {
                    assert_eq!(
                        s.result.rgb,
                        Bits::new(sampled.into(), 24),
                        "sample wall{wall}"
                    );
                }
                if s.source_fire {
                    upper_sources +=
                        usize::from(o.render.producer.pool.sample_descriptor & 16 != 0);
                    let q = o.render.producer.pool.sample_data;
                    assert_eq!(q.slice(0, 18), Bits::new(0x3f00, 18));
                    assert_eq!(q.slice(18, 18), Bits::new(0x1f00, 18));
                }
                if let Some(slot) = s.publication {
                    if let Some(old) = last_public {
                        source_wraps += usize::from(old == 31 && slot == 0);
                    }
                    last_public = Some(slot);
                }
            }
            if let Some(l) = o.render.light {
                if l.pixel_accepted {
                    assert_eq!(
                        o.render.producer.pool.light_normal,
                        Bits::new(1024 << 24, 36)
                    );
                }
                if l.result.valid {
                    assert_eq!(
                        [l.result.g.value as u16, l.result.h.value as u16],
                        light,
                        "light wall{wall}"
                    );
                }
                lights += usize::from(i.ce && l.result.valid);
            }
            if phase == 2 && previous_busy && !o.control.processor.busy {
                assert!(o.render.idle && o.physical_drained);
                if auxiliary == 1 {
                    expected[0x100000 / 2..0x100000 / 2 + 102400].fill(0x1357 + job as u16);
                    expected[0x140000 / 2..0x140000 / 2 + 102400].fill(1024);
                }
                update_expected(&mut expected, &coverage, job, light, uv_repeats);
                if auxiliary > 0 {
                    let p = 0x17a3u16;
                    let (r, g, b) = (p >> 11 & 31, p >> 6 & 31, p >> 1 & 31);
                    for y in 224..240 {
                        for x in 392..400 {
                            expected[0x100000 / 2 + offset(x, y)] =
                                r << 11 | (g * 2 + g / 16) << 5 | b;
                        }
                    }
                    expected.copy_within(0x60000 / 2..0x60000 / 2 + 768, 0x7ffa00 / 2);
                }
                if let Some((at, (got, want))) = native
                    .backing()
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .find(|(_, (a, b))| a != b)
                {
                    panic!("CB{job} physical byte{:x}: {got:04x}!={want:04x}", at * 2);
                }
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
                    cb_native_start = native.native_cycles();
                } else {
                    ended = true;
                }
            }
        }
        previous_busy = o.control.processor.busy;
        let result = gpu.clock(i);
        native.clock();
        result.unwrap_or_else(|e| panic!("wall{wall} CB{job}: {e}"));
        if let Some(at) = aborted {
            if wall > at + 3 && o.physical_drained {
                let since = *drained_since.get_or_insert(wall);
                if wall < since + 5 {
                    continue;
                }
                assert_eq!(native.write_accepts, 0);
                assert_eq!(native.backing(), expected);
                assert!(!o.render.idle && o.render.producer.pool.global_used > 0);
                assert_eq!(samples, 0);
                println!("NATIVE_UV_ABORT at={at} drained_wall={wall} stopped_logical={} physical_drained=true",o.render.producer.pool.global_used);
                return Run {
                    rows,
                    setup_us,
                    step_us: step_start.elapsed().as_micros(),
                    total_us: setup_start.elapsed().as_micros(),
                    preimage,
                    journal: native.journal,
                    image_oracles,
                };
            }
        }
        partial |= native.changed_half_edges > 0 && o.control.processor.busy;
        if auxiliary > 0 {
            assert!(step_start.elapsed().as_secs() < 60, "mixed native60s bound");
            assert!(
                native.native_cycles() - cb_native_start < 200000,
                "mixed CB native-cycle bound"
            );
        }
        if ended {
            assert_eq!(starts, 2 * (1 + uv_repeats));
            assert_eq!(samples, coverage[1].len() * 2 * uv_repeats);
            assert_eq!(lights, samples);
            assert_eq!(
                final_pixels,
                (coverage[0].len() + coverage[1].len() * uv_repeats) * 2
            );
            if uv_repeats >= 5 {
                assert!(upper_sources > 0 && source_wraps > 0);
            }
            println!("NATIVE_UV_IDENTITIES upper_sources={upper_sources} public31to0={source_wraps} repeats={uv_repeats}");
            assert!(
                texture_sectors >= 16 && partial && active_ce0 && native.changed_halfwords > 24
            );
            println!("NATIVE_UV_DRAW walls={} native={} starts={starts} samples={samples} lights={lights} pixels={final_pixels} texture_sectors={texture_sectors} changed_halves={} changed_words={} light={light:?} sample={sampled:06x}",wall+1,native.native_cycles(),native.changed_half_edges,native.changed_halfwords);
            println!("NATIVE_UV_HOST setup_us={setup_us} step_us={} boundary=settle_and_whole8MiB_checks RSS=unmeasured",step_start.elapsed().as_micros());
            return Run {
                rows,
                setup_us,
                step_us: step_start.elapsed().as_micros(),
                total_us: setup_start.elapsed().as_micros(),
                preimage,
                journal: native.journal,
                image_oracles,
            };
        }
    }
    panic!("native DRAW bound CB{job} starts{starts} samples{samples}");
}
#[test]
fn rgb_then_uv_lit_depth_and_q6_use_one_native_backing() {
    run();
}
#[test]
fn full_commands_mix_clear_rgb_uv_full_ui_and_copy_in_two_native_cbs() {
    run_case_with_auxiliary(1, false, 1);
}
#[test]
#[ignore = "complete production DRAW/UI/copy root on native feedback,60s watchdog"]
fn mixed_draw_and_auxiliary_root_matches_production() {
    let root = root("native-all-commands");
    let r = run_case_with_auxiliary(1, false, 2);
    let schema = aux_fixture::fields(&r.rows[0]);
    vertex_support::rtl::replay_whole(
        &root,
        "draw-ui-copy",
        r.rows.iter().map(aux_fixture::fields).collect(),
        aux_fixture::production(&schema),
        false,
        None,
    );
    println!("ALL_COMMANDS_ROOT {}", root.display());
}
#[test]
fn native_public_upper_and_ring_wrap_keep_final_bank_identity() {
    use std::{fmt::Write as _, time::Instant};
    let first = run_case(5, false);
    let second = run_case(5, false);
    assert_eq!(first.rows.len(), second.rows.len());
    let root = root("native-uv-host");
    let export = Instant::now();
    let mut vectors = String::new();
    for row in &first.rows {
        for p in fixture::fields(row) {
            write!(vectors, "{:x} ", p.value).unwrap();
            if p.output {
                write!(vectors, "{:x} ", p.known).unwrap();
            }
        }
        vectors.push('\n');
    }
    std::fs::write(root.join("vectors.txt"), vectors).unwrap();
    let schema = fixture::fields(&first.rows[0]);
    std::fs::write(root.join("dut.v"), fixture::production(&schema)).unwrap();
    let export_us = export.elapsed().as_micros();
    std::fs::write(root.join("host.json"),format!("{{\"scope\":\"native wrap DRAW with8MiB checks and settling; export includes complete vectors and DUT source\",\"host_contention\":\"uncontrolled\",\"rss\":null,\"samples\":[{{\"setup_us\":{},\"step_us\":{},\"total_us\":{}}},{{\"setup_us\":{},\"step_us\":{},\"total_us\":{}}}],\"export_us\":{export_us},\"total_with_export_us\":{},\"walls\":{}}}\n",first.setup_us,first.step_us,first.total_us,second.setup_us,second.step_us,second.total_us,first.total_us+second.total_us+export_us,first.rows.len())).unwrap();
    println!(
        "NATIVE_UV_HOST_ROOT {} export_us={export_us}",
        root.display()
    );
}
#[test]
fn native_ce0_abort_drains_texture_without_emptying_stopped_pool() {
    run_case(1, true);
}
#[test]
#[ignore = "one whole actual CP-to-router DRAW, RGB then UV/lit depth/Q6 two CBs"]
fn entire_uv_lit_gpu_matches_native_feedback() {
    let rows = run();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "native-uv-root-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let p = fixture::fields(&rows[0]);
    let source = fixture::production(&p);
    vertex_support::rtl::replay_whole(
        &root,
        "UV_lit_two_CBs",
        rows.iter().map(fixture::fields).collect(),
        source,
        false,
        None,
    );
    println!("NATIVE_UV_ROOT {}", root.display());
}
fn root(label: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
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
#[ignore = "whole wrap DUT and actual upper-map/weight-wire mutants, bounded22471 walls"]
fn whole_draw_wrap_and_sampling_wiring_detect_counterexamples() {
    use std::time::Instant;
    let root = root("native-uv-wrap-root");
    println!("NATIVE_UV_WRAP_ROOT {}", root.display());
    let first = run_case(5, false);
    let second = run_case(5, false);
    assert_eq!(first.rows.len(), second.rows.len());
    for (a, b) in first.rows.iter().zip(&second.rows) {
        let ap = fixture::fields(a);
        let bp = fixture::fields(b);
        assert_eq!(ap.len(), bp.len());
        for (a, b) in ap.iter().zip(bp) {
            assert_eq!(
                (&a.name, a.value, a.known, a.output),
                (&b.name, b.value, b.known, b.output)
            );
        }
    }
    let export = Instant::now();
    let p = fixture::fields(&first.rows[0]);
    let source = fixture::production(&p);
    let export_us = export.elapsed().as_micros();
    std::fs::write(root.join("host.json"),format!("{{\"scope\":\"native wrap DRAW, settling and complete8MiB checks\",\"host_contention\":\"uncontrolled\",\"rss\":null,\"samples\":[{{\"setup_us\":{},\"step_us\":{},\"total_us\":{}}},{{\"setup_us\":{},\"step_us\":{},\"total_us\":{}}}],\"source_export_us\":{export_us},\"walls\":{}}}\n",first.setup_us,first.step_us,first.total_us,second.setup_us,second.step_us,second.total_us,first.rows.len())).unwrap();
    vertex_support::rtl::replay_whole(
        &root,
        "original",
        first.rows.iter().map(fixture::fields).collect(),
        source.clone(),
        false,
        None,
    );
    for (name, old, new) in [
        (
            "public_fifth_bit",
            "if(public_publish)public_upper[sample_descriptor[3:0]]<=sample_descriptor[4];",
            "if(public_publish)public_upper[sample_descriptor[3:0]]<=1'b0;",
        ),
        (
            "sole_weight_wire",
            ".weight_value(weight_value),\n .texture_invalidate(texture_invalidate)",
            ".weight_value(36'd0),\n .texture_invalidate(texture_invalidate)",
        ),
    ] {
        assert_eq!(source.matches(old).count(), 1, "{name}");
        vertex_support::rtl::replay_whole(
            &root,
            name,
            first.rows.iter().map(fixture::fields).collect(),
            source.replacen(old, new, 1),
            true,
            None,
        );
    }
}
#[test]
#[ignore = "whole CE0 abort and actual missing cache-abort wire mutant"]
fn whole_draw_ce0_abort_releases_only_physical_owners() {
    let root = root("native-uv-abort-root");
    println!("NATIVE_UV_ABORT_ROOT {}", root.display());
    let run = run_case(1, true);
    let source = fixture::production(&fixture::fields(&run.rows[0]));
    vertex_support::rtl::replay_whole(
        &root,
        "original",
        run.rows.iter().map(fixture::fields).collect(),
        source.clone(),
        false,
        None,
    );
    let old = ".abort(abort),\n .weight_write(weight_write)";
    assert_eq!(source.matches(old).count(), 1);
    vertex_support::rtl::replay_whole(
        &root,
        "missing_cache_abort",
        run.rows.iter().map(fixture::fields).collect(),
        source.replacen(old, ".abort(1'b0),\n .weight_write(weight_write)", 1),
        true,
        None,
    );
}

#[test]
fn passive_uv_wrap_and_abort_journal_preserves_real_outputs() {
    for (name, repeats, stop, auxiliary) in [
        ("uv-wrap", 5, false, 0),
        ("uv-abort", 1, true, 0),
        ("mixed", 1, false, 1),
    ] {
        let r = run_case_with_auxiliary(repeats, stop, auxiliary);
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/frame-reports")
            .join(name);
        std::fs::create_dir_all(&folder).unwrap();
        profiling_journal::memory(&folder, &r.preimage, &r.journal, &r.image_oracles);
        let gpu = if auxiliary > 0 {
            Gpu::with_all_commands(Profile::Native)
        } else {
            Gpu::with_draws(Profile::Native)
        };
        let kinds = profiling_journal::record(
            &folder.join("gpu-signals.jsonl"),
            gpu,
            r.rows.iter().map(|e| (e.i, e.o)),
        );
        assert!(kinds["sample_publish"] > 0);
        let returned = kinds.get("sample_last").copied().unwrap_or(0);
        if !stop {
            assert_eq!(kinds["sample_publish"], returned);
        } else {
            assert!(
                kinds["sample_publish"] > returned,
                "abort must retain an open Sampling owner"
            );
        }
        println!("PASSIVE_UV_JOURNAL {name} {kinds:?}");
    }
}

#[test]
#[ignore = "bounded same-input Sampling/ring/abort common profiling events"]
fn common_profiling_uv_and_abort_collectors_use_identical_points() {
    for (name, repeats, stop) in [("uv-wrap", 5, false), ("uv-abort", 1, true)] {
        let r = run_case(repeats, stop);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/common-protocol")
            .join(format!("{name}-{}", std::process::id()));
        let emu = root.join("emu");
        let rtl = root.join("rtl-capture");
        std::fs::create_dir_all(&emu).unwrap();
        std::fs::create_dir(&rtl).unwrap();
        profiling_journal::record_common(
            &emu.join("events.jsonl"),
            Gpu::with_draws(Profile::Native),
            r.rows.iter().map(|e| (e.i, e.o)),
        );
        let observer = profiling_journal::observer(&rtl.join("events.jsonl"), 2, r.rows.len() - 1);
        vertex_support::rtl::replay_profile(
            &root,
            "rtl",
            r.rows.iter().map(fixture::fields).collect(),
            fixture::production(&fixture::fields(&r.rows[0])),
            &observer,
        );
        let raw = std::fs::read_to_string(rtl.join("events.jsonl")).unwrap();
        let last = raw.lines().last().unwrap();
        let end = last
            .trim_start_matches('[')
            .split(',')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        profiling_journal::common_metadata(&rtl, end, "gpu-rtl", !stop);
        println!("COMMON_PROFILING_ROOT {}", root.display());
    }
}
