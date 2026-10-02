//! Replays a GV2M v6 asset through the post-DMA single-core-port trial.
//! Usage: cargo run --example mvp_port_probe -- mesh.gvm2 matrix.mvp.bin normal.bin

use std::fs;

use gpu_v2_cmodel::fixed::{round_shift_ties_even, Q14, Q16};
use gpu_v2_cmodel::format::{CompactGrid, Uniform};
use gpu_v2_cmodel::mvp_pair_trial::PairProfile;
use gpu_v2_cmodel::mvp_port_trial::MvpPortTrial;
use gpu_v2_cmodel::mvp_width_trial::MeshletMvpPlan;
use gpu_v2_cmodel::scratchpad::Scratchpad;
use gpu_v2_cmodel::stream96::{decode_stream, encode_stream, CellReader, Record};

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn preload(scratch: &mut Scratchpad, start: usize, bytes: &[u8]) {
    for (index, chunk) in bytes.chunks(8).enumerate() {
        let mut word = [0_u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        scratch.tick(
            None,
            None,
            None,
            Some((start + index, u64::from_le_bytes(word))),
        );
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mesh = fs::read(args.next().expect("mesh path")).unwrap();
    let mvp = fs::read(args.next().expect("MVP path")).unwrap();
    let normal = fs::read(args.next().expect("normal matrix path")).unwrap();
    let vertex_first = match args.next().as_deref() {
        None => false,
        Some("--vertex-first") => true,
        Some(value) => panic!("unknown option: {value}"),
    };
    assert!(args.next().is_none());
    assert_eq!(&mesh[..4], b"GV2M");
    assert_eq!(u16::from_le_bytes(mesh[4..6].try_into().unwrap()), 6);
    assert_eq!((mvp.len(), normal.len()), (64, 18));
    let uniform = Uniform {
        mvp: std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                Q16::from_raw(i128::from(i32_at(&mvp, (row * 4 + column) * 4))).unwrap()
            })
        }),
        normal: std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                let at = (row * 3 + column) * 2;
                Q14::from_raw(i128::from(i16::from_le_bytes(
                    normal[at..at + 2].try_into().unwrap(),
                )))
                .unwrap()
            })
        }),
    };
    let meshlets = u32_at(&mesh, 8);
    for digit_bits in [1, 2] {
        let mut edges = 0_usize;
        let mut header_reads = 0_usize;
        let mut stream_reads = 0_usize;
        let mut pair_issues = 0_usize;
        let mut residual_steps = 0_usize;
        let mut prepare_issues = 0_usize;
        let mut result_writes = 0_usize;
        let mut vertices = 0_usize;
        let mut triangles = 0_usize;
        let mut vertex_phase_edges = 0_usize;
        let mut triangle_phase_edges = 0_usize;
        for meshlet in 0..meshlets {
            let directory = 64 + meshlet * 32;
            let offset = u32_at(&mesh, directory);
            let length = u32_at(&mesh, directory + 4);
            let expected_vertices = usize::from(u16::from_le_bytes(
                mesh[directory + 8..directory + 10].try_into().unwrap(),
            ));
            let expected_triangles = usize::from(u16::from_le_bytes(
                mesh[directory + 10..directory + 12].try_into().unwrap(),
            ));
            let cells = expected_vertices * 3 + expected_triangles + 1;
            let stream = &mesh[offset + 24..offset + length];
            assert_eq!(stream.len().div_ceil(8), cells.div_ceil(2));
            let words: Vec<u64> = stream
                .chunks(8)
                .map(|chunk| {
                    let mut bytes = [0_u8; 8];
                    bytes[..chunk.len()].copy_from_slice(chunk);
                    u64::from_le_bytes(bytes)
                })
                .collect();
            let records = decode_stream(&mut CellReader::new(words), cells).unwrap();
            let scheduled_records = if vertex_first {
                records
                    .iter()
                    .filter(|record| matches!(record, Record::Vertex { .. }))
                    .chain(
                        records
                            .iter()
                            .filter(|record| matches!(record, Record::Triangle(_))),
                    )
                    .cloned()
                    .chain(std::iter::once(Record::End))
                    .collect::<Vec<_>>()
            } else {
                records.clone()
            };
            let scheduled_words = encode_stream(&scheduled_records).unwrap();
            let grid = CompactGrid {
                origin: std::array::from_fn(|axis| i32_at(&mesh, 32 + axis * 4)),
                base_step: u32_at(&mesh, 44) as u32,
                meshlet_base: std::array::from_fn(|axis| i32_at(&mesh, directory + 16 + axis * 4)),
                level: mesh[directory + 12],
            };
            let mut scratch = Scratchpad::new(0xa55a);
            preload(&mut scratch, 0, &uniform.encode());
            preload(&mut scratch, 16, &mesh[offset..offset + 24]);
            for (slot, word) in scheduled_words.iter().enumerate() {
                scratch.tick(None, None, None, Some((32 + slot, *word)));
            }
            let mut trial = MvpPortTrial::new(
                scratch,
                32,
                cells.div_ceil(2),
                cells,
                grid,
                PairProfile {
                    pair_latency: 2,
                    residual_digit_bits: digit_bits,
                },
                2,
            )
            .unwrap();
            let mut consumed = 0_usize;
            let mut actual_refs = Vec::new();
            let mut last_vertex_publish = 0_usize;
            for _ in 0..100_000 {
                // Setup consumes at most one previously queued triangle per edge.
                if let Some(reference) = trial.pop_triangle() {
                    consumed += 1;
                    actual_refs.push(reference.vertices);
                }
                let cycle = trial
                    .tick()
                    .unwrap_or_else(|fault| panic!("meshlet {meshlet}: {fault:?}"));
                if let Some(address) = cycle.core_read {
                    if address < 19 {
                        header_reads += 1;
                    } else {
                        stream_reads += 1;
                    }
                }
                pair_issues += usize::from(cycle.pair.issue.is_some());
                residual_steps += usize::from(cycle.pair.residual_row.is_some());
                prepare_issues += usize::from(cycle.prepare_issue);
                result_writes += usize::from(cycle.result_write.is_some());
                if cycle.publish == Some(expected_vertices - 1) {
                    last_vertex_publish = trial.edges();
                }
                assert!(
                    !cycle.credit_stall,
                    "one-triangle-per-edge consumer should keep up"
                );
                if trial.done() {
                    break;
                }
            }
            assert!(trial.done(), "meshlet {meshlet} exceeded edge budget");
            assert!(last_vertex_publish > 0);
            while let Some(reference) = trial.pop_triangle() {
                consumed += 1;
                actual_refs.push(reference.vertices);
            }
            assert_eq!(
                (trial.published_vertices(), consumed),
                (expected_vertices, expected_triangles)
            );
            let expected_refs: Vec<_> = records
                .iter()
                .filter_map(|record| {
                    if let Record::Triangle(refs) = record {
                        Some(*refs)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(
                actual_refs, expected_refs,
                "meshlet {meshlet} triangle order"
            );
            let plan = MeshletMvpPlan::new(uniform.mvp, grid).unwrap();
            for (id, record) in records
                .iter()
                .filter(|record| matches!(record, Record::Vertex { .. }))
                .enumerate()
            {
                let Record::Vertex {
                    xyz10,
                    normal,
                    color565,
                    ..
                } = record
                else {
                    unreachable!()
                };
                let stored = trial.results().vertex(id).unwrap();
                assert_eq!(
                    stored.clip,
                    plan.transform(*xyz10).unwrap().0,
                    "meshlet {meshlet} vertex {id} clip"
                );
                let expected_normal = std::array::from_fn(|row| {
                    let sum: i128 = (0..3)
                        .map(|axis| {
                            i128::from(uniform.normal[row][axis].raw())
                                * i128::from(normal[axis].raw())
                        })
                        .sum();
                    Q14::from_raw(round_shift_ties_even(sum, 14).unwrap()).unwrap()
                });
                assert_eq!(
                    stored.normal, expected_normal,
                    "meshlet {meshlet} vertex {id} normal"
                );
                let r = ((color565 >> 11) & 31) as u8;
                let g = ((color565 >> 5) & 63) as u8;
                let b = (color565 & 31) as u8;
                assert_eq!(
                    stored.rgba,
                    [
                        (r << 3) | (r >> 2),
                        (g << 2) | (g >> 4),
                        (b << 3) | (b >> 2),
                        255
                    ]
                );
            }
            edges += trial.edges();
            vertex_phase_edges += last_vertex_publish;
            triangle_phase_edges += trial.edges() - last_vertex_publish;
            vertices += expected_vertices;
            triangles += expected_triangles;
        }
        println!(
            "vertex_first={vertex_first} digit_bits={digit_bits} meshlets={meshlets} vertices={vertices} triangles={triangles} edges={edges} vertex_phase_edges={vertex_phase_edges} triangle_phase_edges={triangle_phase_edges} header_reads={header_reads} stream_reads={stream_reads} prepare_issue_estimate={prepare_issues} pair_issues={pair_issues} residual_steps={residual_steps} result_writes={result_writes}"
        );
    }
}
