//! Live G3-to-R/I composition; admission remains a finite controlled pool peer.
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
    frontend::{Frontend, Input},
    prepare::Params,
};
use std::collections::BTreeSet;
fn signed(n: u64, width: u32) -> i64 {
    ((n << (64 - width)) as i64) >> (64 - width)
}
fn geometry(descriptor: u32, words: [u64; 16]) -> [[i64; 3]; 3] {
    std::array::from_fn(|edge| {
        let raw = [
            signed(words[3 * edge], 27),
            signed(words[3 * edge + 1], 27),
            signed(words[3 * edge + 2], 38),
        ];
        let exp = raw
            .iter()
            .enumerate()
            .map(|(k, n)| {
                (64 - n.unsigned_abs().leading_zeros()).saturating_sub(if k == 2 { 27 } else { 17 })
            })
            .max()
            .unwrap();
        raw.map(|n| {
            let n = n / (1i64 << exp);
            if descriptor & (1 << 18) != 0 {
                -n
            } else {
                n
            }
        })
    })
}
fn covered(g: [[i64; 3]; 3], x: i64, y: i64) -> bool {
    g.into_iter().all(|n| {
        let h = 2 * n[0] * x + 2 * n[1] * y + n[2];
        h > 0 || (h == 0 && (n[0] > 0 || (n[0] == 0 && n[1] > 0)))
    })
}
#[test]
fn indexed_t16_drives_real_prepare_raster_and_interpolation() {
    for (shape, rgb, count) in [(0, true, 2), (3, false, 1), (9, false, 1)] {
        let case = triangle_support::Case {
            shape,
            rgb,
            triangles: count,
            ..Default::default()
        };
        let data = triangle_support::data(case);
        let expected: Vec<BTreeSet<_>> = data
            .packets
            .iter()
            .map(|&(descriptor, words)| {
                let g = geometry(descriptor, words);
                (0..240)
                    .flat_map(|y| (0..400).map(move |x| (x, y)))
                    .filter(|&(x, y)| covered(g, x, y))
                    .collect()
            })
            .collect();
        let mut actual: Vec<BTreeSet<(i64, i64)>> = vec![BTreeSet::new(); data.packets.len()];
        let mut p = Frontend::with_triangles();
        let mut loaded = 0;
        let mut started = false;
        let mut qid = 0;
        let mut headers = 0;
        let mut raw_words = 0;
        let mut active = usize::MAX;
        let mut pending = usize::MAX;
        let mut quads = 0;
        let mut results = 0;
        let mut depths = 0;
        let mut lods = 0;
        let mut pixel = false;
        let mut xy = (0, 0);
        let mut done = false;
        let mut meshlet_done = false;
        for wall in 0..100000 {
            let (address, word) = data.rows[loaded.min(data.rows.len() - 1)];
            let i = Input {
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
                pool_ready: wall % 13 != 0,
                pool_quad_id: qid,
                ..Default::default()
            };
            let o = p.outputs(i);
            let phase = o.interpolation.compute.phase;
            p.clock(i)
                .unwrap_or_else(|e| panic!("shape{shape}/rgb{rgb}/wall{wall}: {e}"));
            if !i.reset {
                if i.dma_write && o.meshlet.dma_ready {
                    loaded += 1;
                }
                if i.ce {
                    meshlet_done |= o.meshlet.done;
                    if i.start && o.meshlet.ready {
                        started = true;
                    }
                    if o.header_take {
                        pending = headers;
                        assert_eq!(
                            o.meshlet.core.descriptor.value as u32,
                            data.packets[headers].0
                        );
                        headers += 1;
                    }
                    if o.meshlet.core.word_valid && o.word_ready {
                        raw_words += 1;
                    }
                    if o.interpolation.context.raster_start {
                        active = pending;
                    }
                    if o.quad_valid && i.pool_ready {
                        qid = (qid + 1) & 7;
                        quads += 1;
                    }
                    if o.raster.sample_valid {
                        assert!(!pixel);
                        pixel = true;
                        xy = (
                            i64::from(o.quad_x) * 2 + i64::from(o.raster.sample_lane & 1),
                            i64::from(o.quad_y) * 2 + i64::from(o.raster.sample_lane >> 1),
                        );
                    }
                    if pixel && phase == 2 {
                        let g = geometry(data.packets[active].0, data.packets[active].1);
                        let cover = covered(g, xy.0, xy.1);
                        assert_eq!(o.raster.sample_cover, cover);
                        if cover {
                            assert!(actual[active].insert(xy));
                        }
                        pixel = false;
                    }
                    if o.interpolation.compute.result_we {
                        o.interpolation
                            .compute
                            .result_data
                            .get(36, "live I result")
                            .unwrap();
                        results += 1;
                    }
                    if o.interpolation.compute.depth_valid {
                        depths += 1;
                    }
                    if o.interpolation.compute.lod_valid {
                        lods += 1;
                    }
                }
                if started
                    && meshlet_done
                    && o.idle
                    && i.ce
                    && !o.interpolation.compute.lod_valid
                    && !o.interpolation.compute.result_we
                    && !o.interpolation.compute.depth_valid
                {
                    done = true;
                    println!(
                        "FRONTEND shape{shape} rgb{rgb} triangles{count} edges={} quads={quads}",
                        wall + 1
                    );
                    break;
                }
            }
        }
        assert!(done);
        assert_eq!(headers, data.packets.len());
        assert_eq!(raw_words, headers * 16);
        assert_eq!(actual, expected);
        assert_eq!(depths, quads * 4);
        assert_eq!(results, quads * 12);
        assert_eq!(lods, quads);
    }
}
