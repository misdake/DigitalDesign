//! Actual G3/R/I/pool/Final/TDP RGB bypass; framebuffer transport is a peer.
#[path = "../support/vliw_render.rs"]
mod fixture;
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
use gpu_v2::system::vliw_cycle::{
    frontend,
    postquad::{self, Profile},
    prepare::Params,
    producer,
    render::{Input, Output, Renderer},
    vertex::Bits,
};
use std::collections::BTreeSet;
struct Transaction {
    write: bool,
    address: usize,
    word: usize,
    wait: usize,
}
struct Edge {
    i: Input,
    o: Output,
    s: postquad::State,
    light: Bits,
    sample: Bits,
}
fn native_offset(x: usize, y: usize) -> usize {
    ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16
}
fn run(profile: Profile) -> Vec<Edge> {
    let data = tiny::data();
    let coverage = tiny::coverage(&data);
    assert!(!coverage.is_empty() && coverage.len() < 32);
    let final_rgb = (0..3).fold(0u32, |v, c| {
        v | ((((0x7bad31 >> (8 * c)) & 255) * ((0xa1b2c3 >> (8 * c)) & 255) * 2 + 255) / 510)
            << (8 * c)
    });
    let packed = (0..3).fold(0u16, |v, c| {
        let channel = (final_rgb >> (8 * c)) & 255;
        let max = if c == 1 { 63 } else { 31 };
        v | ((((channel * max * 2 + 255) / 510) as u16) << [11, 5, 0][c])
    });
    {
        let mut r = Renderer::with_triangles(profile);
        let mut loaded = 0;
        let mut started = false;
        let mut seen_done = false;
        let mut flushing = false;
        let mut memory: Vec<u16> = (0..0x800000 / 2)
            .map(|n| (n as u16).wrapping_mul(731).wrapping_add(0x1234))
            .collect();
        let mut expected = memory.clone();
        for &(x, y) in &coverage {
            expected[0x100000 / 2 + native_offset(x, y)] = packed;
        }
        let mut transaction: Option<Transaction> = None;
        let mut raw = 0;
        let mut headers = 0;
        let mut final_count = 0;
        let mut commits = 0;
        let mut committed = BTreeSet::new();
        let mut done = false;
        let mut pixels = 0;
        let mut rows = Vec::new();
        for wall in 0..20000usize {
            let ce = wall % 7 > 1;
            let (address, word) = data.rows[loaded.min(data.rows.len() - 1)];
            let mut memory_input = postquad::Memory::default();
            if let Some(t) = &transaction {
                if t.word < 16 {
                    if t.write {
                        memory_input.write_ready = wall % 5 != 3;
                    } else if wall % 5 != 2 {
                        memory_input.read_valid = true;
                        for h in 0..4 {
                            memory_input.read_data |=
                                u64::from(memory[t.address / 2 + t.word * 4 + h]) << (h * 16);
                        }
                    }
                } else {
                    memory_input.complete = t.wait == 3;
                    memory_input.ok = true;
                }
            } else {
                memory_input.request_ready = wall % 11 != 7;
            }
            let i = Input {
                globals: Default::default(),
                producer: producer::Input {
                    frontend: frontend::Input {
                        reset: wall < 2,
                        ce,
                        start: !started && loaded == data.rows.len(),
                        slot: false,
                        mesh_color: 0x927bad31,
                        dma_write: wall >= 2 && loaded < data.rows.len(),
                        dma_address: address,
                        dma_data: word,
                        output_ready: true,
                        params: Params {
                            far_z: 100 << 16,
                            tex_u: 6,
                            tex_v: 6,
                            max_mip: 6,
                            bias: -3,
                        },
                        ..Default::default()
                    },
                    draw_unlit: true,
                    ..Default::default()
                },
                backend: postquad::Input {
                    ce,
                    sampling_drained: true,
                    specular: 0xabcdef,
                    depth_func: 7,
                    depth_write: false,
                    color_base: 2048,
                    depth_base: 2560,
                    flush_request: flushing,
                    memory: memory_input,
                    ..Default::default()
                },
                ..Default::default()
            };
            let o = r.outputs(i);
            rows.push(Edge {
                i,
                o,
                s: r.backend.state.clone(),
                light: r.backend.light_bank.q,
                sample: r.backend.sample_bank.q,
            });
            let fo = o.producer.frontend;
            let po = o.producer.pool;
            let bo = o.backend;
            let sink = bo.sink.cache;
            if !i.producer.frontend.reset {
                if i.producer.frontend.dma_write && fo.meshlet.dma_ready {
                    loaded += 1;
                }
                if ce {
                    if i.producer.frontend.start && fo.meshlet.ready {
                        started = true;
                    }
                    seen_done |= fo.meshlet.done;
                    if fo.header_take {
                        assert_eq!(
                            fo.meshlet.core.descriptor.value as u32,
                            data.packets[headers].0
                        );
                        headers += 1;
                    }
                    if fo.meshlet.core.word_valid && fo.word_ready {
                        let index = usize::from(fo.meshlet.core.word_index);
                        assert_eq!(
                            fo.meshlet.core.word_data.get(54, "tiny T16").unwrap(),
                            data.packets[0].1[index]
                        );
                        raw += 1;
                    }
                }
                if bo.final_issue {
                    assert_eq!(
                        po.basic_data.get(24, "constant RGB loan").unwrap(),
                        0xa1b2c3
                    );
                    final_count += 1;
                }
                if bo.final_result {
                    assert_eq!(
                        bo.final_stage.rgb.get(24, "unlit Final result").unwrap(),
                        u64::from(final_rgb)
                    );
                    pixels += 1;
                }
                if sink.commit_valid {
                    let x = sink.commit_x.get(9, "commit X").unwrap() as usize;
                    let y = sink.commit_y.get(8, "commit Y").unwrap() as usize;
                    let mask = sink.commit_mask.get(4, "commit mask").unwrap();
                    for lane in 0..4 {
                        if mask & (1 << lane) != 0 {
                            assert!(committed.insert((x + (lane & 1), y + (lane >> 1))));
                        }
                    }
                    commits += 1;
                }
                if started && seen_done && o.producer.idle && bo.pipeline_idle {
                    flushing = true;
                }
            }
            if let Some(t) = &mut transaction {
                if t.word < 16
                    && (memory_input.read_valid
                        || t.write && memory_input.write_ready && sink.mem_write_valid)
                {
                    if t.write {
                        let value = sink.mem_write_data.get(64, "RGB physical write").unwrap();
                        for h in 0..4 {
                            memory[t.address / 2 + t.word * 4 + h] = (value >> (h * 16)) as u16;
                        }
                    }
                    t.word += 1;
                } else if t.word == 16 {
                    t.wait += 1;
                }
                if memory_input.complete {
                    transaction = None;
                }
            }
            if sink.mem_req_valid && memory_input.request_ready {
                assert!(transaction.is_none());
                transaction = Some(Transaction {
                    write: sink.mem_req_write,
                    address: sink.mem_req_address as usize,
                    word: 0,
                    wait: 0,
                });
            }
            r.clock(i)
                .unwrap_or_else(|e| panic!("wall{wall} raw{raw} Final{final_count}/{pixels}: {e}"));
            if flushing && sink.flush_complete && o.idle {
                assert!(transaction.is_none());
                assert_eq!((headers, raw), (1, 16));
                assert_eq!(final_count, coverage.len());
                assert_eq!(pixels, coverage.len());
                assert_eq!(committed, coverage);
                assert!(commits > 0);
                if let Some((at, (a, b))) = memory
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .find(|(_, (a, b))| a != b)
                {
                    panic!("physical byte{:x}: {a:04x}!={b:04x}", at * 2);
                }
                println!(
                    "RGB_RENDER {profile:?} walls={} pixels={pixels} quads={commits}",
                    wall + 1
                );
                done = true;
                break;
            }
        }
        assert!(done, "RGB renderer exceeded20000 walls");
        rows
    }
}
#[test]
fn real_rgb_pipeline_flushes_an_independently_covered_constant_color_triangle() {
    for profile in [Profile::Portable, Profile::Native] {
        run(profile);
    }
}
#[test]
#[ignore = "one bounded actual selected GPU root; direct source-RAM and framebuffer-sector peers"]
fn closed_rgb_root_matches_production() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "rgb-root-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let rows = run(Profile::Portable);
    let pins = fixture::fields(&rows[0]);
    vertex_support::rtl::replay(
        &root,
        "rgb_unlit",
        rows.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        false,
        None,
    );
    println!("RGB_ROOT_CHECKPOINT {}", root.display());
}
