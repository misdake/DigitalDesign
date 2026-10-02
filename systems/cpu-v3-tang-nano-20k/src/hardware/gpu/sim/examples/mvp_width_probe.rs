//! Read a GV2M v6 mesh and a raw 4x4 Q16 MVP; audit exact width candidates.
//! Usage: cargo run --example mvp_width_probe -- mesh.gvm2 matrix.mvp.bin

use gpu_v2_cmodel::fixed::{round_shift_ties_even, Q14, Q16};
use gpu_v2_cmodel::format::CompactGrid;
use gpu_v2_cmodel::mvp_pair_trial::{PairProfile, PairedVertexUnit};
use gpu_v2_cmodel::mvp_width_trial::{AffineMvpPlan, MeshletMvpPlan, MvpWork};
use gpu_v2_cmodel::stream96::{decode_stream, CellReader, Record};
use std::fs;

fn u32_at(bytes: &[u8], offset: usize) -> usize {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn q16(raw: i128) -> Q16 {
    Q16::from_raw(raw).unwrap()
}

fn q14(raw: i32) -> Q14 {
    Q14::from_raw(i128::from(raw)).unwrap()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mesh = fs::read(args.next().expect("mesh path")).unwrap();
    let mvp = fs::read(args.next().expect("MVP path")).unwrap();
    assert!(args.next().is_none());
    assert_eq!(&mesh[..4], b"GV2M");
    assert_eq!(u16::from_le_bytes(mesh[4..6].try_into().unwrap()), 6);
    assert_eq!(mvp.len(), 64);
    let matrix = std::array::from_fn(|row| {
        std::array::from_fn(|column| q16(i128::from(i32_at(&mvp, (row * 4 + column) * 4))))
    });
    let affine = AffineMvpPlan::new(matrix);
    let meshlets = u32_at(&mesh, 8);
    let mut vertices = 0_u64;
    let mut affine_work = MvpWork::default();
    let mut local_18 = 0_u64;
    let mut local_36 = 0_u64;
    let mut local_residual = 0_u64;
    let mut local_paired = 0_u64;
    let mut stream_cell_edges = 0_u64;
    let mut stream_word_reads = 0_u64;
    let mut pair_edges = [[0_u64; 4]; 2];
    let mut pair_clip_issues = 0_u64;
    let mut pair_normal_issues = 0_u64;
    let mut pair_residual_steps = [0_u64; 4];
    let mut local_eligible = 0_usize;
    let mut precompute = MvpWork::default();
    let mut row_shift_histogram = [0_u64; 9];
    let mut row_wide_fallback = 0_u64;
    let mut shift_histogram = [0_u64; 5];
    let mut wide_fallback = 0_u64;
    for row in affine.coefficient_shifts() {
        for shift in row {
            if let Some(shift) = shift {
                shift_histogram[usize::from(shift)] += 1;
            } else {
                wide_fallback += 1;
            }
        }
    }
    for index in 0..meshlets {
        let directory = 64 + index * 32;
        let offset = u32_at(&mesh, directory);
        let length = u32_at(&mesh, directory + 4);
        let vertex_count =
            u16::from_le_bytes(mesh[directory + 8..directory + 10].try_into().unwrap());
        let triangle_count =
            u16::from_le_bytes(mesh[directory + 10..directory + 12].try_into().unwrap());
        let grid = CompactGrid {
            origin: std::array::from_fn(|axis| i32_at(&mesh, 32 + axis * 4)),
            base_step: u32_at(&mesh, 44) as u32,
            meshlet_base: std::array::from_fn(|axis| i32_at(&mesh, directory + 16 + axis * 4)),
            level: mesh[directory + 12],
        };
        let local = MeshletMvpPlan::new(matrix, grid);
        if let Some(plan) = &local {
            local_eligible += 1;
            for shift in plan.row_shifts() {
                if let Some(shift) = shift {
                    row_shift_histogram[usize::from(shift)] += 1;
                } else {
                    row_wide_fallback += 1;
                }
            }
            let cost = plan.precompute_work();
            precompute.wide36x36 += cost.wide36x36;
            precompute.narrow36x18 += cost.narrow36x18;
            precompute.residual32x4 += cost.residual32x4;
            precompute.translation_adds += cost.translation_adds;
        }
        let useful = &mesh[offset + 24..offset + length];
        let mut words = Vec::with_capacity(useful.len().div_ceil(8));
        for chunk in useful.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            words.push(u64::from_le_bytes(word));
        }
        let useful_cells = usize::from(vertex_count) * 3 + usize::from(triangle_count) + 1;
        let mut reader = CellReader::new(words);
        let records = decode_stream(&mut reader, useful_cells)
            .unwrap_or_else(|error| panic!("meshlet {index}: {error:?}"));
        stream_cell_edges += reader.edges() as u64;
        stream_word_reads += reader.reads() as u64;
        let mut counted = 0;
        for record in records {
            if let Record::Vertex { xyz10, normal, .. } = record {
                let position: [Q16; 3] = std::array::from_fn(|axis| {
                    let raw = i128::from(grid.origin[axis])
                        + (i128::from(grid.meshlet_base[axis])
                            + (i128::from(xyz10[axis]) << grid.level))
                            * i128::from(grid.base_step);
                    q16(raw)
                });
                let (affine_clip, cost) = affine
                    .transform([position[0], position[1], position[2], q16(1 << 16)])
                    .unwrap();
                affine_work.wide36x36 += cost.wide36x36;
                affine_work.narrow36x18 += cost.narrow36x18;
                affine_work.residual32x4 += cost.residual32x4;
                affine_work.translation_adds += cost.translation_adds;
                if let Some(local) = &local {
                    let (local_clip, work) = local.transform(xyz10).unwrap();
                    assert_eq!(local_clip, affine_clip, "meshlet {index}, vertex {counted}");
                    local_18 += u64::from(work.products18x10);
                    local_36 += u64::from(work.products36x10);
                    local_residual += u64::from(work.residual10x8);
                    local_paired += u64::from(work.paired18_issue_lower_bound);
                    // Dense signed trial matrix keeps all three normal rows
                    // occupied. Its numerical values are a scheduler stimulus,
                    // not the asset's driver-validated normal matrix.
                    let normal_matrix = [
                        [q14(11_000), q14(-8_000), q14(6_000)],
                        [q14(7_000), q14(12_000), q14(-8_000)],
                        [q14(-9_000), q14(6_000), q14(11_000)],
                    ];
                    let expected_normal = std::array::from_fn::<_, 3, _>(|row| {
                        let sum: i128 = (0..3)
                            .map(|axis| {
                                i128::from(normal_matrix[row][axis].raw())
                                    * i128::from(normal[axis].raw())
                            })
                            .sum();
                        Q14::from_raw(round_shift_ties_even(sum, 14).unwrap()).unwrap()
                    });
                    for (latency_index, latency) in [2, 3].into_iter().enumerate() {
                        for (digit_index, digit_bits) in [1, 2, 4, 8].into_iter().enumerate() {
                            let mut unit = PairedVertexUnit::new(
                                local,
                                xyz10,
                                normal,
                                [0; 4],
                                normal_matrix,
                                PairProfile {
                                    pair_latency: latency,
                                    residual_digit_bits: digit_bits,
                                },
                            )
                            .unwrap();
                            let (output, scheduled) = unit.run_bounded(256).unwrap();
                            assert_eq!(output.clip, local_clip);
                            assert_eq!(output.normal, expected_normal);
                            assert_eq!(scheduled.clip_dsp_issues, work.paired18_issue_lower_bound);
                            pair_edges[latency_index][digit_index] += u64::from(scheduled.edges);
                            if latency_index == 0 {
                                pair_residual_steps[digit_index] +=
                                    u64::from(scheduled.residual_digit_steps);
                                if digit_index == 0 {
                                    pair_clip_issues += u64::from(scheduled.clip_dsp_issues);
                                    pair_normal_issues += u64::from(scheduled.normal_dsp_issues);
                                }
                            }
                        }
                    }
                }
                counted += 1;
                vertices += 1;
            }
        }
        assert_eq!(counted, usize::from(vertex_count));
    }
    println!(
        "meshlets={meshlets} local_eligible={local_eligible} vertices={vertices} coefficient_shifts={shift_histogram:?} wide_fallback={wide_fallback}"
    );
    println!(
        "affine: 36x36={} 36x18={} 32x4={} macro_issues={} translation_adds={}",
        affine_work.wide36x36,
        affine_work.narrow36x18,
        affine_work.residual32x4,
        affine_work.macro_issues(),
        affine_work.translation_adds
    );
    println!(
        "local: base_36x36={} base_36x18={} base_32x4={} vertex_18x10={} vertex_36x10={} residual10x8={} paired18_issue_lower_bound={} row_shifts={row_shift_histogram:?} row_wide_fallback={row_wide_fallback}",
        precompute.wide36x36, precompute.narrow36x18, precompute.residual32x4,
        local_18, local_36, local_residual, local_paired
    );
    println!(
        "paired schedule, staged input only: clip_issues={pair_clip_issues} normal_issues={pair_normal_issues} residual_steps_by_digit_width={pair_residual_steps:?} edges_pair_latency_2={:?} edges_pair_latency_3={:?}",
        pair_edges[0], pair_edges[1]
    );
    println!(
        "separate preparation/stream work: matrix_64b_reads={} aabb_64b_reads={} stream_32b_cells={} stream_64b_reads={} base_36x18_issues_if_short_products_use_dsp={}",
        meshlets * 8,
        meshlets * 3,
        stream_cell_edges,
        stream_word_reads,
        precompute.narrow36x18 + precompute.residual32x4
    );
}
