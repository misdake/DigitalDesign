//! Actual I producer; downstream payload accept/read/retire ports are peers.
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    compute::{Compute, Input as ComputeInput},
    pool::{Input, Output, Pool, Shadow, State},
    vertex::Bits,
};
use std::collections::VecDeque;
#[path = "../support/vliw_pool.rs"]
mod fixture;
#[derive(Clone, Copy, Debug)]
struct Header {
    ordinal: usize,
    x: u8,
    y: u8,
    rgb: bool,
    unlit: bool,
    color: u32,
}
impl Header {
    fn bits(self) -> u64 {
        (u64::from(self.y) << 12)
            | (u64::from(self.x) << 4)
            | (u64::from(self.rgb) << 2)
            | (u64::from(self.unlit) << 1)
    }
}
struct Edge {
    i: Input,
    o: Output,
    s: State,
    h: Shadow,
    q: [Bits; 5],
}
struct Run {
    rows: Vec<Edge>,
    full: bool,
    producer_full: bool,
    old_full_retire: bool,
    old_full_publish: bool,
    masks: u16,
    reads: usize,
    lights: usize,
    samples: usize,
}
fn ndc_axis(n: i64, d: i64) -> u16 {
    let negative = n < 0;
    let a = n.unsigned_abs();
    let d = d as u64;
    let q = a / d;
    let r = a % d;
    let q = q + u64::from(2 * r > d || (2 * r == d && q & 1 != 0));
    (if negative { -(q as i64) } else { q as i64 }) as u16
}
fn run(case: usize) -> Run {
    let mut pool = Pool::default();
    let mut compute = Compute::default();
    let mut rows = Vec::new();
    let mut batch = 0;
    let mut setup = 0;
    let mut emitted = 0;
    let mut lane = 0u8;
    let mut in_quad = false;
    let mut pixel = false;
    let mut tag = 0;
    let mut current_color = u32::MAX;
    let mut current_unlit = false;
    let mut allocated = 0usize;
    let mut published = 0usize;
    let mut retired = 0usize;
    let mut attr_pixel = 0usize;
    let mut attr_word = 0usize;
    let mut depth_pixel = 0usize;
    let mut lod_count = 0usize;
    let mut headers = [None; 32];
    let mut words = [[0u64; 3]; 128];
    let mut depths = [0u32; 128];
    let mut masks = [0u8; 32];
    let mut lods = [0u8; 32];
    let mut sample_seen = [0u8; 32];
    let mut queue = VecDeque::new();
    let mut read_mask = 0u8;
    let mut loan = None;
    let mut full = false;
    let mut producer_full = false;
    let mut old_full_retire = false;
    let mut old_full_publish = false;
    let mut seen_masks = 0u16;
    let mut reads = 0;
    let mut lights = 0;
    let mut samples = 0;
    for wall in 0..100000 {
        let reset = wall < 2;
        let front_ce = wall % 7 > 1;
        let backend_ce = wall % 5 != 0;
        let phase = compute.state.phase;
        let rgb = if case == 0 { batch == 0 } else { batch != 0 };
        let unlit = batch == 0;
        let mut ci = ComputeInput {
            reset,
            ce: front_ce,
            read_bank: false,
            write_bank: false,
            cfg_rgb: rgb,
            cfg_far: 20 << 16,
            cfg_z_bias: 8,
            cfg_tex_u: 6,
            cfg_tex_v: 5,
            cfg_max_mip: 6,
            cfg_bias: -3,
            ..Default::default()
        };
        if batch < 2 && setup < 16 && !reset {
            if setup < 6 {
                let k = setup as usize;
                let values = if rgb {
                    [
                        [-601, 800, 300, 23, 131, 219],
                        [511, -900, 1000, 250, 20, 99],
                        [41, 200, -500, 50, 180, 111],
                    ]
                } else {
                    [
                        [-601, 800, 300, 1000, 30000, 0],
                        [511, -900, 1000, 60000, 1200, 0],
                        [41, 200, -500, 25000, 10000, 0],
                    ]
                };
                let base = values[2][k];
                let a = values[0][k] - base;
                let b = values[1][k] - base;
                ci.param_valid = true;
                ci.param_index = (k / 2) as u8;
                ci.param_bank = !k.is_multiple_of(2);
                ci.param_data = Bits::new(
                    ((base as u64 & 0x1ffff) << 34)
                        | ((b as u64 & 0x1ffff) << 17)
                        | (a as u64 & 0x1ffff),
                    51,
                );
            }
            if (6..9).contains(&setup) {
                ci.coefficient_bwe = true;
                ci.coefficient_bwa = [0, 1, 4][(setup - 6) as usize];
                ci.coefficient_bwd =
                    Bits::new([-8i64, 4, 1 << 28][(setup - 6) as usize] as u64, 36);
            }
            if (6..10).contains(&setup) {
                ci.coefficient_cwe = true;
                ci.coefficient_cwa = (setup - 6) as u8;
                ci.coefficient_cwd = Bits::new(
                    [1i64 << 30, 1 << 29, -(1 << 28), 1 << 31][(setup - 6) as usize] as u64,
                    36,
                );
            }
            ci.cfg_valid = setup == 10;
        }
        let base_input = Input {
            reset,
            front_ce,
            backend_ce,
            phase,
            quad_rgb: compute.state.rgb,
            quad_color: current_color,
            draw_unlit: current_unlit,
            publish_ready: wall > 1000,
            light_ready: wall > 3000 && wall % 9 != 0,
            sample_ready: wall > 3000 && wall % 11 != 0,
            descriptor_issue_slot: queue
                .front()
                .copied()
                .map_or((allocated & 31) as u8, |n: usize| (n & 31) as u8),
            ..Default::default()
        };
        let ready = pool.outputs(base_input).quad_ready;
        let sending = batch < 2 && setup == 16 && emitted < 40;
        let quad_valid = sending && !in_quad && phase == 0;
        let allocate = front_ce && quad_valid && ready && !reset;
        let source_ordinal = if in_quad { allocated - 1 } else { allocated };
        let h = if source_ordinal.is_multiple_of(6) {
            [1024, 0, 0]
        } else {
            match lane {
                0 => [16384, 24576, 32768],
                1 => [50000, 7002, 90000],
                2 => [1024, 0, 0],
                _ => {
                    if source_ordinal.is_multiple_of(3) {
                        [50000, 7002, 90000]
                    } else {
                        [0, 0, 0]
                    }
                }
            }
        };
        ci.sample_valid = sending && phase == 0 && (in_quad || quad_valid && ready);
        ci.sample_pid = ((if in_quad {
            tag
        } else {
            pool.outputs(base_input).quad_id
        }) << 2)
            | lane;
        ci.sample_h = if phase < 3 { h[phase as usize] } else { 0 };
        ci.sample_cover = true;
        ci.sample_anchor = lane == 0;
        ci.quad_end_valid = in_quad && lane == 3 && phase == 2;
        ci.quad_end_qid = tag;
        ci.quad_end_mask = 15;
        let co = compute.outputs(ci);
        let mut i = Input {
            quad_valid,
            quad_x: ((source_ordinal * 7) % 200) as u8,
            quad_y: ((source_ordinal * 3) % 120) as u8,
            quad_end_valid: ci.quad_end_valid,
            quad_end_id: tag,
            quad_end_mask: 15,
            result_write: co.result_we,
            result_address: co.result_address,
            result_data: co.result_data,
            depth_valid: co.depth_valid,
            depth_pid: co.depth_pid,
            depth_z16: co.depth_z16,
            depth_pass: co.depth_pass,
            lod_valid: co.lod_valid,
            lod_id: co.lod_id,
            lod_value: co.lod_value,
            ..base_input
        };
        let pre = pool.outputs(i);
        let mut consuming = None;
        if let Some(&(address, basic, depth, _lane)) = loan.as_ref() {
            assert_eq!(
                pre.basic_data.get(24, "basic read loan").unwrap(),
                basic,
                "case{case}/wall{wall}/addr{address}"
            );
            assert_eq!(pre.depth_data.get(24, "depth read loan").unwrap(), depth);
            if backend_ce {
                consuming = loan;
            }
        } else if backend_ce && wall > 3000 {
            if let Some(&ordinal) = queue.front() {
                let slot = ordinal & 31;
                let mask = masks[slot];
                if mask == 0 {
                    i.empty_skip = pre.empty_skip_ready;
                } else if let Some(l) =
                    (0..4).find(|l| mask & (1 << l) != 0 && read_mask & (1 << l) == 0)
                {
                    let address = slot * 4 + l;
                    let w = words[address];
                    let header: Header = headers[slot].unwrap();
                    let basic = if header.rgb {
                        ((w[1] >> 12) & 255) | ((w[2] & 255) << 8) | (((w[2] >> 8) & 255) << 16)
                    } else {
                        0xffffff
                    };
                    i.basic_read = true;
                    i.basic_address = address as u8;
                    i.depth_read = true;
                    i.depth_address = address as u8;
                    loan = Some((address, basic, u64::from(depths[address]), l));
                } else {
                    let light_cursor = (published as u8).wrapping_sub(pre.light_pending) & 63;
                    let sample_cursor = (published as u8).wrapping_sub(pre.sample_pending) & 63;
                    if light_cursor != (retired as u8 & 63) && sample_cursor != (retired as u8 & 63)
                    {
                        i.retire_valid = true;
                        i.retire_id = slot as u8;
                    }
                }
            }
        }
        let o = pool.outputs(i);
        if let Some(&ordinal) = queue.front() {
            let slot = ordinal & 31;
            let header: Header = headers[slot].unwrap();
            assert_eq!(
                o.issue_color.get(24, "issued material color").unwrap(),
                u64::from(header.color & 0xffffff)
            );
            assert_eq!(
                o.descriptor_head.get(23, "retire head descriptor").unwrap(),
                (header.bits() << 4) | u64::from(masks[slot])
            );
        }
        rows.push(Edge {
            i,
            o,
            s: pool.state.clone(),
            h: pool.shadow.clone(),
            q: [
                pool.normal.q,
                pool.uv.q,
                pool.basic.q,
                pool.depth.q,
                pool.ndc_q,
            ],
        });
        assert_eq!(o.global_used, (allocated - retired) as u8);
        assert_eq!(o.producer_used, (allocated - published) as u8);
        if o.light_valid {
            let slot = usize::from(o.light_descriptor & 31);
            let header: Header = headers[slot].unwrap();
            let l = usize::from(o.light_lane);
            let w = words[slot * 4 + l];
            assert!(!header.unlit);
            assert_eq!((o.light_descriptor >> 5) & 15, u16::from(masks[slot]));
            let normal = (w[0] & 4095) | (((w[0] >> 12) & 4095) << 12) | ((w[1] & 4095) << 24);
            assert_eq!(o.light_normal.get(36, "lighting normal").unwrap(), normal);
            let x = usize::from(header.x) * 2 + (l & 1);
            let y = usize::from(header.y) * 2 + (l >> 1);
            let position = u32::from(ndc_axis((2 * x as i64 + 1 - 400) * 16384, 400))
                | (u32::from(ndc_axis((240 - 2 * y as i64 - 1) * 16384, 240)) << 16);
            assert_eq!(
                o.light_position.get(32, "lighting NDC").unwrap(),
                u64::from(position)
            );
            if backend_ce && i.light_ready {
                lights += 1;
            }
        }
        if o.sample_valid {
            let slot = o.sample_descriptor as usize & 31;
            let header: Header = headers[slot].unwrap();
            let l = usize::from(o.sample_row);
            let w = words[slot * 4 + l];
            assert!(!header.rgb);
            assert!(masks[slot] & (1 << l) != 0);
            assert_eq!((o.sample_descriptor >> 5) & 15, u32::from(masks[slot]));
            assert_eq!(o.sample_descriptor >> 11, u32::from(lods[slot]));
            assert_eq!(
                (o.sample_descriptor >> 10) & 1,
                u32::from(sample_seen[slot] == 0)
            );
            assert_eq!(
                o.sample_data.get(36, "UV read").unwrap(),
                ((w[2] & 65535) << 18) | ((w[1] >> 12) & 65535)
            );
            assert_eq!(
                o.sample_last.get(1, "sample LAST").unwrap(),
                u64::from(masks[slot] >> (l + 1) == 0)
            );
            if backend_ce && i.sample_ready {
                assert!(sample_seen[slot] & (1 << l) == 0);
                sample_seen[slot] |= 1 << l;
                samples += 1;
            }
        }
        full |= o.global_used == 24;
        producer_full |= o.producer_used == 8;
        old_full_retire |= o.global_used == 24 && i.retire_valid;
        old_full_publish |= o.producer_used == 8 && o.published;
        if o.global_used == 24 || o.producer_used == 8 {
            assert!(!o.quad_ready);
        }
        pool.clock(i).unwrap_or_else(|e|panic!("case{case}/wall{wall}/batch{batch}/phase{phase}/alloc{allocated}/pub{published}/ret{retired}: {e}"));
        compute.clock(ci).unwrap();
        if !reset {
            if allocate {
                assert_eq!(o.quad_id, (allocated & 7) as u8);
                tag = o.quad_id;
                in_quad = true;
                let slot = allocated & 31;
                headers[slot] = Some(Header {
                    ordinal: allocated,
                    x: i.quad_x,
                    y: i.quad_y,
                    rgb: i.quad_rgb,
                    unlit: i.draw_unlit,
                    color: i.quad_color,
                });
                masks[slot] = 0;
                sample_seen[slot] = 0;
                allocated += 1;
            }
            if front_ce {
                if ci.cfg_valid {
                    current_color = [0x7f123456, 0x43abcdef][batch];
                    current_unlit = unlit;
                }
                if batch < 2 && setup < 16 {
                    setup += 1;
                }
                if ci.sample_valid {
                    pixel = true;
                }
                if pixel && phase == 3 {
                    pixel = false;
                    if lane == 3 {
                        lane = 0;
                        in_quad = false;
                        emitted += 1;
                    } else {
                        lane += 1;
                    }
                }
                if co.result_we {
                    assert_eq!(
                        co.result_address,
                        (((attr_pixel & 31) << 2) | attr_word) as u8
                    );
                    words[attr_pixel & 127][attr_word] =
                        co.result_data.get(36, "I result").unwrap();
                    if attr_word == 2 {
                        attr_word = 0;
                        attr_pixel += 1;
                    } else {
                        attr_word += 1;
                    }
                }
                if co.depth_valid {
                    let slot = (depth_pixel / 4) & 31;
                    let l = depth_pixel % 4;
                    depths[depth_pixel & 127] =
                        ((i.quad_color >> 24) << 16) | u32::from(co.depth_z16);
                    if co.depth_pass {
                        masks[slot] |= 1 << l;
                    }
                    depth_pixel += 1;
                }
                if co.lod_valid {
                    lods[lod_count & 31] = co.lod_value;
                    lod_count += 1;
                }
                if batch < 2 && emitted == 40 && !in_quad && !co.busy {
                    batch += 1;
                    setup = 0;
                    emitted = 0;
                }
            }
            if o.published {
                let slot = published & 31;
                let header: Header = headers[slot].unwrap();
                assert_eq!(header.ordinal, published);
                assert_eq!(o.published_slot, slot as u8);
                assert_eq!(
                    o.published_begin.get(23, "publication").unwrap(),
                    (header.bits() << 4) | u64::from(masks[slot])
                );
                queue.push_back(published);
                seen_masks |= 1 << masks[slot];
                published += 1;
            }
            if let Some((_, _, _, l)) = consuming {
                read_mask |= 1 << l;
                loan = None;
                reads += 1;
            }
            if backend_ce && (i.retire_valid || i.empty_skip) {
                assert_eq!(queue.pop_front(), Some(retired));
                retired += 1;
                read_mask = 0;
            }
        }
        if batch == 2 && !o.busy && !compute.outputs(ComputeInput::default()).busy {
            assert_eq!((allocated, published, retired), (80, 80, 80));
            assert_eq!(attr_pixel, 320);
            assert_eq!(depth_pixel, 320);
            assert_eq!(lod_count, 80);
            return Run {
                rows,
                full,
                producer_full,
                old_full_retire,
                old_full_publish,
                masks: seen_masks,
                reads,
                lights,
                samples,
            };
        }
    }
    panic!("pool case{case} exceeded100000 wall edges");
}
#[test]
fn actual_i_returns_publish_and_retire_sparse_dense_slots_with_old_credits() {
    for case in 0..2 {
        let r = run(case);
        println!(
            "POOL_CASE {case} edges={} masks={:04x} reads={} lights={} samples={}",
            r.rows.len(),
            r.masks,
            r.reads,
            r.lights,
            r.samples
        );
        assert!(r.full && r.producer_full && r.old_full_publish && r.old_full_retire);
        assert!(r.masks & 1 != 0 && r.masks & (1 << 2) != 0 && r.masks & (1 << 10) != 0);
        assert!(r.reads > 0 && r.lights > 0 && r.samples > 0);
        assert!(r.rows.iter().any(|e| e.i.front_ce && !e.i.backend_ce));
        assert!(r.rows.iter().any(|e| !e.i.front_ce && e.i.backend_ce));
    }
}

#[test]
#[ignore = "two bounded actual command-pool mixed-mode lifetime profiles"]
fn every_pool_edge_matches_production() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "pool-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    for case in 0..2 {
        let run = run(case);
        let pins = fixture::fields(&run.rows[0]);
        vertex_support::rtl::replay(
            &root,
            &format!("case{case}"),
            run.rows.iter().map(fixture::fields).collect(),
            fixture::production(&pins),
            false,
            None,
        );
    }
    println!("POOL_CHECKPOINT {}", root.display());
}

#[test]
#[ignore = "actual publication/old-full/sparse-owner mutations and retire monitor"]
fn pool_negative_controls_and_retire_monitor_are_sensitive() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "pool-negative-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    let mut run = run(0);
    let pins = fixture::fields(&run.rows[0]);
    for (name, old, new) in [
        (
            "publish_last_W_bypass",
            "assign published=backend_ce&&publish_ready&&complete_head&&!reset&&!fault;",
            "assign published=backend_ce&&publish_ready&&(complete_head||complete_event)&&!reset&&!fault;",
        ),
        (
            "retire_credit_bypass",
            "assign quad_ready=!reset&&!fault&&global_used<24&&producer_used<8;",
            "assign quad_ready=!reset&&!fault&&(global_used<24||(backend_ce&&retire_valid&&!reset&&!fault))&&producer_used<8;",
        ),
        (
            "sparse_lane_zero",
            "wire[1:0]sample_read_lane=sample_lane==0?first_live_lane(sample_mask):sample_lane;",
            "wire[1:0]sample_read_lane=sample_lane;",
        ),
    ] {
        let source = fixture::production(&pins);
        assert_eq!(source.matches(old).count(), 1);
        vertex_support::rtl::replay(
            &root,
            name,
            run.rows.iter().map(fixture::fields).collect(),
            source.replacen(old, new, 1),
            true,
            None,
        );
    }
    let n = run
        .rows
        .iter()
        .position(|r| r.i.backend_ce && r.i.retire_valid)
        .unwrap();
    run.rows.truncate(n + 1);
    run.rows[n].i.retire_id ^= 1;
    let mut invalid = Pool::default();
    for row in &run.rows[..n] {
        invalid.clock(row.i).unwrap();
    }
    let error = invalid.clock(run.rows[n].i).unwrap_err();
    assert_eq!(error.module, "pool");
    assert_eq!(error.rule, "GPU contract: fault");
    vertex_support::rtl::replay(
        &root,
        "wrong_retire_id",
        run.rows.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        false,
        Some(error.rule),
    );
    println!("POOL_NEGATIVE {}", root.display());
}
