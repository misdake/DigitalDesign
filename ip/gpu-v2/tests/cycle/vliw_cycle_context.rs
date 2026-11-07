//! Context/replay qualification. Runtime busy and the pool remain explicit peers.
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_triangle/mod.rs"]
mod triangle_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    interpolator::{Context, Input, Output},
    prepare::Params,
    vertex::Bits,
};
struct Edge {
    i: Input,
    o: Output,
    p: Context,
    packet: usize,
    raster_packet: usize,
}
fn run() -> Vec<Edge> {
    let cases = [(0, true), (3, false), (9, true), (0, false)];
    let packets: Vec<_> = cases
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
    let mut p = Context::default();
    let mut rows = Vec::new();
    let mut packet = 0;
    let mut started = false;
    let mut cursor = 0;
    let mut waits = 0;
    let mut qid = 0;
    let mut raster_packet = usize::MAX;
    for wall in 0..100000 {
        let (descriptor, words) = packets[packet.min(3)];
        let start = wall >= 2
            && packet < 4
            && !started
            && !p.prepare.control.regs.active
            && !p.state.replay;
        let i = Input {
            reset: wall < 2,
            ce: wall % 7 > 1,
            prepare_start: start,
            execution_idle: packet != 2,
            descriptor,
            raw_valid: started && cursor < 16 && wall % 5 != 0,
            raw_index: cursor as u8,
            raw_data: Bits::new(words[cursor.min(15)], 54),
            params: Params {
                far_z: 100 << 16,
                tex_u: 6,
                tex_v: 5,
                max_mip: 4,
                bias: -3,
            },
            compute_busy: packet == 2 || ((packet == 1 || packet == 3) && waits < 20),
            pool_ready: wall % 11 != 0,
            pool_quad_id: qid,
            quad_valid: wall > 6 && wall % 4 != 0,
            sample_lane: (wall & 3) as u8,
            sample_valid: false,
        };
        let o = p.outputs(i);
        rows.push(Edge {
            i,
            o,
            p: p.clone(),
            packet,
            raster_packet,
        });
        p.clock(i)
            .unwrap_or_else(|e| panic!("packet{packet}/wall{wall}: {e}"));
        if i.ce {
            if o.raster_start {
                raster_packet = packet;
            }
            if start {
                started = true;
            }
            if i.raw_valid && o.prepare.raw_ready {
                cursor += 1;
            }
            if o.prepare.context_ready {
                waits += 1;
            }
            if o.quad_take {
                qid = (qid + 1) & 7;
            }
            if o.prepare.done {
                assert_eq!(cursor, 16);
                packet += 1;
                started = false;
                cursor = 0;
                waits = 0;
            }
        }
        if packet == 4 && !p.state.replay {
            return rows;
        }
    }
    panic!("context fixture exceeded100000 edges");
}
#[test]
fn geometry_replays_once_and_empty_does_not_publish_context() {
    let rows = run();
    let enabled: Vec<_> = rows.iter().filter(|r| r.i.ce && !r.i.reset).collect();
    assert_eq!(enabled.iter().filter(|r| r.o.prepare.cfg_apply).count(), 3);
    assert_eq!(enabled.iter().filter(|r| r.o.raster_start).count(), 3);
    assert_eq!(enabled.iter().filter(|r| r.o.bound_valid).count(), 4);
    assert!(enabled.iter().any(|r| r.o.raster_start && r.i.compute_busy));
    assert!(enabled
        .iter()
        .any(|r| r.p.state.raster_pending && r.i.compute_busy && !r.o.run_enable));
    for packet in 0..4 {
        let subset: Vec<_> = enabled.iter().filter(|r| r.packet == packet).collect();
        let geo: Vec<_> = enabled
            .iter()
            .filter(|r| r.raster_packet == packet && r.o.geo_write)
            .collect();
        if packet == 2 {
            assert!(geo.is_empty());
            assert!(subset
                .iter()
                .all(|r| !r.o.prepare.cfg_apply && !r.o.raster_start));
            assert!(subset.iter().any(|r| r.o.bound_valid
                && r.o.bound_empty
                && !r.i.execution_idle
                && r.i.compute_busy));
        } else {
            assert_eq!(geo.len(), 9);
            for (n, r) in geo.into_iter().enumerate() {
                assert_eq!(r.o.geo_address, [0, 4, 8, 1, 5, 9, 2, 6, 10][n]);
                let source = [6, 9, 12, 7, 10, 13, 8, 11, 14][n];
                let value = r.p.prepare.storage.b[source].get(36, "replay PB").unwrap();
                let expected = if n % 3 == 2 {
                    value & 0x3fffffff
                } else {
                    (value << 1) & 0x3fffffff
                };
                assert_eq!(r.o.geo_data.get(30, "replay geometry").unwrap(), expected);
            }
        }
    }
    for adjacent in rows.windows(2) {
        let (a, b) = (&adjacent[0], &adjacent[1]);
        if a.i.reset {
            continue;
        }
        assert_eq!(
            b.p.state.active_bank,
            a.p.state.active_bank ^ (a.i.ce && a.o.prepare.cfg_apply)
        );
        if !a.i.ce {
            assert_eq!(b.p.state.replay_slot, a.p.state.replay_slot);
        }
    }
}
