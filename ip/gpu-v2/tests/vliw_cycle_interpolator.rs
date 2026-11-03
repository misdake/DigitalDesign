//! Real prepare/context/compute composition. Raster and pool are controlled peers.
#[allow(dead_code)]
#[path = "support/vliw_compute.rs"]
mod compute_fixture;
#[path = "support/vliw_stream.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "support/vliw_triangle/mod.rs"]
mod triangle_support;
#[allow(dead_code)]
#[path = "support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    compute::{Compute, Input as ComputeInput, Output as ComputeOutput},
    interpolator::{Input, Stream, StreamInput, StreamOutput},
    prepare::Params,
    vertex::Bits,
};
struct Edge {
    i: ComputeInput,
    o: ComputeOutput,
    p: Compute,
}
struct StreamEdge {
    i: StreamInput,
    o: StreamOutput,
    p: Stream,
}
fn signed(v: u64) -> i64 {
    ((v << 34) as i64) >> 34
}
fn run() -> Vec<StreamEdge> {
    let mut packets: Vec<_> = [(0, true), (3, false), (9, true), (0, false)]
        .into_iter()
        .map(|(shape, rgb)| {
            triangle_support::data(triangle_support::Case {
                shape,
                rgb,
                ..Default::default()
            })
            .packets[0]
        })
        .collect();
    for (n, (_, words)) in packets.iter_mut().enumerate() {
        let color = [0x12ab34cd_u64, 0x98fedc76, 0x55443322, 0x88776655][n];
        words[10] = (words[10] & ((1 << 36) - 1)) | ((color & 0x3ffff) << 36);
        words[11] = (words[11] & ((1 << 36) - 1)) | ((color >> 18) << 36);
    }
    let mut p = Stream::default();
    let mut rows = Vec::new();
    let mut packet = 0;
    let mut started = false;
    let mut cursor = 0;
    let mut r_busy = false;
    let mut r_bound = false;
    let mut owner = usize::MAX;
    let mut geometry = [Bits::default(); 12];
    let mut quad_live = false;
    let mut lane = 0u8;
    let mut qid = 0;
    let mut quads = 0;
    let mut cover_mask = 0;
    let mut anchor_seen = false;
    for wall in 0..100000 {
        let phase = p.compute.state.phase;
        let (descriptor, words) = packets[packet.min(3)];
        let ce = wall % 7 > 1;
        let prep_ready = p
            .context
            .prepare
            .outputs(gpu_v2::system::vliw_cycle::prepare::PrepareInput::default());
        let finish =
            (owner != packet && packet < 4 && prep_ready.context_ready && !prep_ready.bound_empty)
                || (owner == 3 && quads >= 8);
        let pool_ready = wall % 11 != 0;
        let ready = pool_ready && !p.context.state.raster_pending;
        let running = r_busy && r_bound && !p.context.state.raster_pending;
        let quad_valid = running && !quad_live && phase == 0 && !finish;
        let quad_take = quad_valid && ready;
        let sample_valid = running && phase == 0 && (quad_live || quad_take);
        let x = 196 + 2 * (quads % 3) + usize::from(lane & 1);
        let y = 116 + usize::from(lane >> 1);
        let h: [u32; 3] = std::array::from_fn(|e| {
            (signed(geometry[e].value) * x as i64
                + signed(geometry[4 + e].value) * y as i64
                + signed(geometry[8 + e].value)) as u32
                & 0x3fffffff
        });
        let cover = (0..3).all(|e| {
            let value = signed(h[e].into());
            let nx = signed(geometry[e].value);
            let ny = signed(geometry[4 + e].value);
            value > 0 || (value == 0 && (nx > 0 || (nx == 0 && ny > 0)))
        });
        let control = Input {
            reset: wall < 2,
            ce,
            prepare_start: wall >= 2
                && packet < 4
                && !started
                && !p.context.prepare.control.regs.active
                && !p.context.state.replay,
            execution_idle: !r_busy,
            descriptor,
            raw_valid: started && cursor < 16 && wall % 5 != 0,
            raw_index: cursor as u8,
            raw_data: Bits::new(words[cursor.min(15)], 54),
            params: Params {
                far_z: (31 << 16) << packet.min(3),
                depth_shift: packet.min(3) as u8,
                tex_u: 6,
                tex_v: 5,
                max_mip: 6,
                bias: -3,
            },
            compute_busy: true,
            pool_ready,
            pool_quad_id: qid,
            quad_valid,
            sample_lane: lane,
            sample_valid,
        };
        let i = StreamInput {
            control,
            sample_h: if phase < 3 { h[phase as usize] } else { 0 },
            sample_cover: cover,
            sample_anchor: cover && !anchor_seen,
            quad_end_valid: running && quad_live && lane == 3 && phase == 2,
            quad_mask: cover_mask | (u8::from(cover) << lane),
        };
        let o = p.outputs(i);
        rows.push(StreamEdge { i, o, p: p.clone() });
        p.clock(i).unwrap_or_else(|e| {
            panic!("wall{wall}/packet{packet}/owner{owner}/q{quads}/lane{lane}/phase{phase}: {e}")
        });
        if ce && !control.reset {
            if control.prepare_start {
                started = true;
            }
            if control.raw_valid && o.context.prepare.raw_ready {
                cursor += 1;
            }
            if quad_take {
                quad_live = true;
                cover_mask = 0;
                anchor_seen = false;
                qid = (qid + 1) & 7;
            }
            if running && quad_live && phase == 2 {
                cover_mask |= u8::from(cover) << lane;
                if cover {
                    anchor_seen = true;
                }
            }
            if running && quad_live && phase == 3 {
                if lane == 3 {
                    quad_live = false;
                    lane = 0;
                    quads += 1;
                } else {
                    lane += 1;
                }
            }
            if finish && !quad_live && r_bound {
                r_busy = false;
            }
            if o.context.raster_start {
                r_busy = true;
                r_bound = false;
                owner = packet;
                quads = 0;
                lane = 0;
                quad_live = false;
            }
            if o.context.geo_write {
                geometry[o.context.geo_address as usize] = o.context.geo_data;
            }
            if o.context.bound_valid && !o.context.bound_empty {
                r_bound = true;
            }
            if o.context.prepare.done {
                assert_eq!(cursor, 16);
                packet += 1;
                started = false;
                cursor = 0;
            }
        }
        if packet == 4 && !r_busy && !p.outputs(i).context.busy {
            return rows;
        }
    }
    panic!("stream exceeded100000 wall edges");
}

#[test]
#[ignore = "actual interpolator stream with private prepare, two runtime pages and compute"]
fn combined_prepare_context_and_compute_match_every_production_edge() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "stream-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    let rows = run();
    let pins = fixture::fields(&rows[0]);
    vertex_support::rtl::replay(
        &root,
        "actual-prepare-I",
        rows.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        false,
        None,
    );
    println!("STREAM_CHECKPOINT {}", root.display());
}

#[test]
#[ignore = "actual context ownership DUT mutations and sample-before-switch monitor"]
fn context_ownership_rejects_early_commit_and_sampling() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "stream-negative-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    let mut rows = run();
    let pins = fixture::fields(&rows[0]);
    for (name, old, new) in [
        (
            "commit_before_I_drain",
            "wire execute_grant=discard_context||((execution_idle||raster_pending)&&!p_busy);",
            "wire execute_grant=discard_context||(execution_idle||raster_pending);",
        ),
        (
            "pending_reads_inactive_page",
            ".read_bank(active_bank),.write_bank(!active_bank)",
            ".read_bank(raster_pending?!active_bank:active_bank),.write_bank(!active_bank)",
        ),
    ] {
        let source = fixture::production(&pins);
        assert_eq!(source.matches(old).count(), 1);
        vertex_support::rtl::replay(
            &root,
            name,
            rows.iter().map(fixture::fields).collect(),
            source.replacen(old, new, 1),
            true,
            None,
        );
    }
    let n = rows
        .iter()
        .position(|r| {
            r.i.control.ce && r.o.compute.phase == 0 && !r.o.context.run_enable && r.o.compute.busy
        })
        .unwrap();
    rows.truncate(n + 1);
    let row = rows.last_mut().unwrap();
    row.i.control.sample_valid = true;
    row.o = row.p.outputs(row.i);
    let mut invalid = row.p.clone();
    let error = invalid.clock(row.i).unwrap_err();
    assert_eq!(error.module, "interpolator");
    assert_eq!(error.rule, "new raster sample before I context switch");
    vertex_support::rtl::replay(
        &root,
        "sample_before_switch",
        rows.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        false,
        Some(error.rule),
    );
    println!("STREAM_NEGATIVE {}", root.display());
}
#[test]
fn real_compute_drains_before_context_flip_while_new_geometry_replays() {
    let rows = run();
    let enabled: Vec<_> = rows
        .iter()
        .filter(|r| r.i.control.ce && !r.i.control.reset)
        .collect();
    assert_eq!(
        enabled.iter().filter(|r| r.o.context.raster_start).count(),
        3
    );
    assert_eq!(
        enabled
            .iter()
            .filter(|r| r.o.context.prepare.cfg_apply)
            .count(),
        3
    );
    assert_eq!(enabled.iter().filter(|r| r.o.context.geo_write).count(), 27);
    assert_eq!(
        enabled.iter().filter(|r| r.o.context.bound_valid).count(),
        4
    );
    assert!(enabled
        .iter()
        .any(|r| r.o.context.raster_start && r.o.compute.busy));
    assert!(enabled
        .iter()
        .any(|r| r.o.compute.busy && !r.o.context.run_enable));
    assert!(enabled.iter().any(|r| r.o.context.bound_valid
        && r.o.context.bound_empty
        && r.o.compute.busy
        && !r.i.control.execution_idle));
    assert!(enabled.iter().any(|r| r.o.compute.result_we));
    assert!(enabled.iter().any(|r| r.o.compute.lod_valid));
    assert!(enabled
        .iter()
        .all(|r| !r.o.context.prepare.cfg_apply || !r.o.compute.busy));
    for pair in rows.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.i.control.reset {
            continue;
        }
        if !a.o.context.prepare.cfg_apply {
            let bank = usize::from(a.o.context.read_bank);
            for n in 0..2 {
                assert_eq!(
                    &a.p.compute.params[n][bank * 4..bank * 4 + 3],
                    &b.p.compute.params[n][bank * 4..bank * 4 + 3]
                );
            }
            assert_eq!(
                &a.p.compute.lod.b[bank * 8..bank * 8 + 8],
                &b.p.compute.lod.b[bank * 8..bank * 8 + 8]
            );
            assert_eq!(
                &a.p.compute.lod.c[bank * 8..bank * 8 + 8],
                &b.p.compute.lod.c[bank * 8..bank * 8 + 8]
            );
        }
    }
}
