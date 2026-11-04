//! Actual G3/R/I/pool composition with controlled final consumer events.
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
    frontend,
    prepare::Params,
    producer::{Backend, Input, Producer},
};
#[test]
fn live_indexed_frontend_and_authoritative_pool_close_under_credit_pressure() {
    for rgb in [true, false] {
        let case = triangle_support::Case {
            rgb,
            shape: if rgb { 0 } else { 3 },
            ..Default::default()
        };
        let data = triangle_support::data(case);
        let mut p = Producer::with_triangles();
        let mut loaded = 0;
        let mut started = false;
        let mut meshlet_done = false;
        let mut quads = 0;
        let mut published = 0;
        let mut retired = 0;
        let mut headers = 0;
        let mut raw_words = 0;
        let mut results = 0;
        let mut depths = 0;
        let mut lods = 0;
        let mut lights = 0;
        let mut samples = 0;
        let mut peak = 0;
        let mut high_slot = false;
        let mut done = false;
        for wall in 0..100000 {
            let (address, word) = data.rows[loaded.min(data.rows.len() - 1)];
            let front = frontend::Input {
                reset: wall < 2,
                ce: wall % 7 > 1,
                start: !started && loaded == data.rows.len(),
                slot: case.slot,
                mesh_color: case.color,
                dma_write: wall >= 2 && loaded < data.rows.len(),
                dma_address: address,
                dma_data: word,
                output_ready: true,
                params: Params {
                    far_z: 100 << 16,
                    tex_u: 6,
                    tex_v: 5,
                    max_mip: 6,
                    bias: -3,
                },
                ..Default::default()
            };
            let mut i = Input {
                frontend: front,
                draw_unlit: rgb,
                backend: Backend {
                    ce: wall % 5 != 0,
                    publish_ready: wall > 2000,
                    light_ready: wall > 10000 && wall % 11 != 0,
                    sample_ready: wall > 10000 && wall % 13 != 0,
                    ..Default::default()
                },
            };
            let pre = p.outputs(i);
            let po = pre.pool;
            if wall > 10000 && published != retired {
                let light_cursor = (published as u8).wrapping_sub(po.light_pending) & 63;
                let sample_cursor = (published as u8).wrapping_sub(po.sample_pending) & 63;
                if po.empty_head {
                    i.backend.empty_skip = po.empty_skip_ready;
                } else if light_cursor != (retired as u8 & 63)
                    && sample_cursor != (retired as u8 & 63)
                {
                    i.backend.retire_valid = true;
                    i.backend.retire_id = (retired & 31) as u8;
                }
            }
            let o = p.outputs(i);
            let fo = o.frontend;
            let co = fo.interpolation.compute;
            let po = o.pool;
            assert_eq!(po.global_used, (quads - retired) as u8);
            peak = peak.max(po.global_used);
            p.clock(i).unwrap_or_else(|e| {
                panic!("rgb{rgb}/wall{wall}/q{quads}/p{published}/r{retired}: {e}")
            });
            if !front.reset {
                if front.dma_write && fo.meshlet.dma_ready {
                    loaded += 1;
                }
                if front.ce {
                    if front.start && fo.meshlet.ready {
                        started = true;
                    }
                    meshlet_done |= fo.meshlet.done;
                    if fo.header_take {
                        headers += 1;
                    }
                    if fo.meshlet.core.word_valid && fo.word_ready {
                        raw_words += 1;
                    }
                    if fo.quad_valid && po.quad_ready {
                        assert_eq!(po.quad_id, (quads & 7) as u8);
                        quads += 1;
                    }
                    if co.result_we {
                        results += 1;
                    }
                    if co.depth_valid {
                        depths += 1;
                    }
                    if co.lod_valid {
                        lods += 1;
                    }
                }
                if po.published {
                    assert_eq!(po.published_slot, (published & 31) as u8);
                    po.published_begin
                        .get(23, "real producer publication")
                        .unwrap();
                    published += 1;
                    high_slot |= po.published_slot >= 16;
                }
                if i.backend.ce {
                    if po.light_valid && i.backend.light_ready {
                        po.light_normal.get(36, "real normal consumer").unwrap();
                        po.light_position.get(32, "real NDC consumer").unwrap();
                        lights += 1;
                    }
                    if po.sample_valid && i.backend.sample_ready {
                        po.sample_data.get(36, "real UV consumer").unwrap();
                        po.sample_last.get(1, "real UV LAST").unwrap();
                        samples += 1;
                    }
                    if i.backend.retire_valid || i.backend.empty_skip {
                        assert_eq!(po.retire_slot, (retired & 31) as u8);
                        retired += 1;
                    }
                }
                if started && meshlet_done && front.ce && o.idle && !co.lod_valid {
                    done = true;
                    println!("PRODUCER rgb{rgb} edges={} quads={quads} published={published} retired={retired} peak={peak} lights={lights} samples={samples}",wall+1);
                    break;
                }
            }
        }
        assert!(done);
        assert_eq!(headers, data.packets.len());
        assert_eq!(raw_words, headers * 16);
        assert!(quads > 64 && high_slot);
        assert_eq!((published, retired), (quads, quads));
        assert_eq!(peak, 24);
        assert_eq!(results, quads * 12);
        assert_eq!(depths, quads * 4);
        assert_eq!(lods, quads);
        if rgb {
            assert_eq!((lights, samples), (0, 0));
        } else {
            assert!(lights > 0 && samples > 0);
        }
    }
}
