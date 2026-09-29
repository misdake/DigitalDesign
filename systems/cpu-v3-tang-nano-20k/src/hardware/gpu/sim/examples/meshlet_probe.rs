//! DMA -> compact vertex transform -> clocked triangle input assembly.
//! The optional JSON is input to an untimed, floating-point preview.

use gpu_v2_cmodel::dma::DmaDesc;
use gpu_v2_cmodel::fixed::{Q14, Q16};
use gpu_v2_cmodel::format::{CompactGrid, Header, InputVertex, MeshletBounds, Uniform};
use gpu_v2_cmodel::microkernel::{Command, DrawDesc, Microkernel};
use gpu_v2_cmodel::normal_contract::{check_scale_preserving, TRIAL_GRAM_TOLERANCE};
use gpu_v2_cmodel::result_store::{TransformedVertex, TriangleRef};
use gpu_v2_cmodel::setup::{SetupEngine, TriangleSetupInput};
use gpu_v2_cmodel::timing::MemoryTiming;
use std::fs;

fn q16(raw: i32) -> Q16 {
    Q16::from_raw(i128::from(raw)).unwrap()
}
fn q14(raw: i16) -> Q14 {
    Q14::from_raw(i128::from(raw)).unwrap()
}
fn words(bytes: &[u8]) -> Vec<u64> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_le_bytes(*c))
        .collect()
}
fn u32_at(b: &[u8], p: usize) -> usize {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap()) as usize
}
fn i32_at(b: &[u8], p: usize) -> i32 {
    i32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn round(sum: i128, shift: u32) -> i128 {
    let unit = 1_i128 << shift;
    let q = sum.div_euclid(unit);
    let r = sum.rem_euclid(unit);
    q + i128::from(r * 2 > unit || (r * 2 == unit && q & 1 != 0))
}
fn golden(u: &Uniform, v: &InputVertex) -> TransformedVertex {
    TransformedVertex {
        clip: std::array::from_fn(|row| {
            Q16::from_raw(round(
                (0..4)
                    .map(|col| {
                        i128::from(u.mvp[row][col].raw()) * i128::from(v.position[col].raw())
                    })
                    .sum(),
                16,
            ))
            .unwrap()
        }),
        normal: std::array::from_fn(|row| {
            Q14::from_raw(round(
                (0..3)
                    .map(|col| {
                        i128::from(u.normal[row][col].raw()) * i128::from(v.normal[col].raw())
                    })
                    .sum(),
                14,
            ))
            .unwrap()
        }),
        rgba: v.rgba,
    }
}
fn expanded(file: &[u8], index: usize) -> (Vec<InputVertex>, Vec<TriangleRef>) {
    assert_eq!(u16::from_le_bytes(file[4..6].try_into().unwrap()), 2);
    let d = 64 + index * 32;
    let (mut p, end) = (u32_at(file, d), u32_at(file, d) + u32_at(file, d + 4));
    let (mut vertices, mut triangles) = (Vec::new(), Vec::new());
    while p < end {
        let h = Header::decode(u64::from_le_bytes(file[p..p + 8].try_into().unwrap())).unwrap();
        p += 8;
        match h {
            Header::Vertex { id } => {
                assert_eq!(usize::from(id), vertices.len());
                vertices.push(
                    InputVertex::decode_payload(words(&file[p..p + 40]).try_into().unwrap())
                        .unwrap(),
                );
                p += 40;
            }
            Header::Triangle { refs } => triangles.push(TriangleRef { vertices: refs }),
            Header::End => assert_eq!(p, end),
            Header::CompactVertex { .. } => panic!("compact reference"),
        }
    }
    (vertices, triangles)
}
fn json_record(r: &TriangleSetupInput) -> String {
    let vs = r.vertices.map(|v| {
        format!(
            "{{\"clip\":{:?},\"normal\":{:?},\"rgba\":{:?}}}",
            v.clip.map(|x| x.raw()),
            v.normal.map(|x| x.raw()),
            v.rgba
        )
    });
    format!(
        "{{\"refs\":{:?},\"vertices\":[{}]}}",
        r.refs.vertices,
        vs.join(",")
    )
}
fn run_one(
    file: &[u8],
    index: usize,
    mvp: Option<&[[i32; 4]; 4]>,
    normal: Option<&[[i16; 3]; 3]>,
    reference: Option<&[u8]>,
) -> (u64, String) {
    assert_eq!(&file[..4], b"GV2M");
    let version = u16::from_le_bytes(file[4..6].try_into().unwrap());
    assert!((2..=4).contains(&version));
    let d = 64 + index * 32;
    let (offset, len) = (u32_at(file, d), u32_at(file, d + 4));
    let vc = u16::from_le_bytes(file[d + 8..d + 10].try_into().unwrap());
    let tc = u16::from_le_bytes(file[d + 10..d + 12].try_into().unwrap());
    assert!((3..=64).contains(&vc) && (1..=128).contains(&tc));
    let grid = (version >= 3).then(|| CompactGrid {
        origin: std::array::from_fn(|a| i32_at(file, 32 + a * 4)),
        base_step: u32_at(file, 44) as u32,
        meshlet_base: std::array::from_fn(|a| i32_at(file, d + 16 + a * 4)),
        level: file[d + 12],
    });
    let stream = &file[offset..offset + len];
    let bounds = (version == 4).then(|| {
        MeshletBounds::decode(words(&stream[..MeshletBounds::BYTES]).try_into().unwrap()).unwrap()
    });
    let mut p = if bounds.is_some() {
        MeshletBounds::BYTES
    } else {
        0
    };
    let (mut vertices, mut triangles) = (Vec::new(), Vec::new());
    while p < len {
        let h = Header::decode(u64::from_le_bytes(stream[p..p + 8].try_into().unwrap())).unwrap();
        p += 8;
        match h {
            Header::Vertex { id } => {
                assert!(grid.is_none());
                assert_eq!(usize::from(id), vertices.len());
                vertices.push(
                    InputVertex::decode_payload(words(&stream[p..p + 40]).try_into().unwrap())
                        .unwrap(),
                );
                p += 40;
            }
            Header::CompactVertex { id } => {
                assert_eq!(usize::from(id), vertices.len());
                let word = u64::from_le_bytes(stream[p..p + 8].try_into().unwrap());
                vertices.push(InputVertex::decode_compact(word, grid.unwrap()).unwrap());
                p += 8;
            }
            Header::Triangle { refs } => triangles.push(TriangleRef { vertices: refs }),
            Header::End => assert_eq!(p, len),
        }
    }
    assert_eq!(
        (vertices.len(), triangles.len()),
        (usize::from(vc), usize::from(tc))
    );
    if let Some(reference) = reference {
        let (v, t) = expanded(reference, index);
        assert_eq!(
            (vertices.as_slice(), triangles.as_slice()),
            (v.as_slice(), t.as_slice())
        );
    }
    let mut uniform = Uniform {
        mvp: [[q16(0); 4]; 4],
        normal: [[q14(0); 3]; 3],
    };
    for a in 0..4 {
        uniform.mvp[a][a] = q16(65536);
    }
    for a in 0..3 {
        uniform.normal[a][a] = q14(16384);
    }
    if let Some(m) = mvp {
        for (target, source) in uniform.mvp.iter_mut().zip(m) {
            for (value, raw) in target.iter_mut().zip(source) {
                *value = q16(*raw);
            }
        }
    }
    if let Some(m) = normal {
        for (target, source) in uniform.normal.iter_mut().zip(m) {
            for (value, raw) in target.iter_mut().zip(source) {
                *value = q14(*raw);
            }
        }
    }
    check_scale_preserving(uniform.normal, TRIAL_GRAM_TOLERANCE).unwrap();
    let padded = len.next_multiple_of(32);
    let mut memory = vec![0_u8; 256 + padded];
    memory[..128].copy_from_slice(&uniform.encode());
    memory[256..256 + len].copy_from_slice(stream);
    let mut model = Microkernel::new(
        words(&memory),
        MemoryTiming {
            grant_wait: 2,
            first_beat: 3,
            beat_gap: 1,
            jitter: 2,
        },
        19,
    );
    for command in [
        Command::Dma(DmaDesc {
            physical_addr: 0,
            scratchpad_addr: 0,
            byte_count: 128,
            completion_token: 0,
        }),
        Command::Dma(DmaDesc {
            physical_addr: 256,
            scratchpad_addr: 256,
            byte_count: padded,
            completion_token: 1,
        }),
        Command::Draw(DrawDesc {
            uniform_token: 0,
            vertex_token: 1,
            uniform_addr: 0,
            stream_addr: 256,
            stream_bytes: len,
            vertex_count: vc as u8,
            triangle_count: tc,
            compact_grid: grid,
        }),
    ] {
        model.submit(command).unwrap();
    }
    let edges = model.run(30_000).unwrap();
    assert_eq!(model.meshlet_bounds, bounds);
    for (id, v) in vertices.iter().enumerate() {
        assert_eq!(
            model.results.vertex(id),
            Some(golden(&uniform, v)),
            "vertex {id}"
        );
    }
    assert_eq!(
        model.setup_queue.iter().copied().collect::<Vec<_>>(),
        triangles
    );
    let mut published = 0;
    let mut pushed = 0;
    for step in &model.trace {
        if step.vertex_published.is_some() {
            published += 1;
        }
        if step.triangle_pushed {
            assert!(triangles[pushed]
                .vertices
                .iter()
                .all(|&id| usize::from(id) < published));
            pushed += 1;
        }
    }
    assert_eq!(pushed, triangles.len());
    let mut setup = SetupEngine::new(128);
    let setup_edges = setup
        .drain(&mut model.setup_queue, &mut model.results, 2_000)
        .unwrap();
    assert_eq!(setup.output().len(), triangles.len());
    for (record, refs) in setup.output().iter().zip(&triangles) {
        assert_eq!(record.refs, *refs);
        for c in 0..3 {
            assert_eq!(
                record.vertices[c],
                golden(&uniform, &vertices[usize::from(refs.vertices[c])])
            );
        }
    }
    if grid.is_some() {
        assert_eq!(
            model
                .trace
                .iter()
                .filter(|s| s.normal_rom_read.is_some())
                .count(),
            usize::from(vc)
        );
    }
    println!("meshlet={index} vertices={vc} triangles={tc} micro_edges={edges} setup_edges={setup_edges}");
    let records = setup
        .output()
        .iter()
        .map(json_record)
        .collect::<Vec<_>>()
        .join(",");
    (
        edges,
        format!("{{\"index\":{index},\"triangles\":[{records}]}}"),
    )
}
fn read_mvp(path: &str) -> [[i32; 4]; 4] {
    let b = fs::read(path).unwrap();
    assert_eq!(b.len(), 64);
    std::array::from_fn(|r| std::array::from_fn(|c| i32_at(&b, (r * 4 + c) * 4)))
}
fn read_normal(path: &str) -> [[i16; 3]; 3] {
    let b = fs::read(path).unwrap();
    assert_eq!(b.len(), 18);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            i16::from_le_bytes(b[(r * 3 + c) * 2..(r * 3 + c) * 2 + 2].try_into().unwrap())
        })
    })
}
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("meshlet_probe MESH.gvm2 [index|all] [MVP.bin|-] [REFERENCE.gvm2|-] [--normal NORMAL.bin] [--setup-json OUTPUT.json]");
    let selection = args.next().unwrap_or_else(|| "0".into());
    let mvp = args.next().filter(|p| p != "-").map(|p| read_mvp(&p));
    let reference = args
        .next()
        .filter(|p| p != "-")
        .map(|p| fs::read(p).unwrap());
    let (mut normal, mut output) = (None, None);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--normal" => normal = Some(read_normal(&args.next().unwrap())),
            "--setup-json" => output = args.next(),
            _ => panic!("unknown option {flag}"),
        }
    }
    let file = fs::read(path).unwrap();
    let count = u32_at(&file, 8);
    let indices: Vec<usize> = if selection == "all" {
        (0..count).collect()
    } else {
        vec![selection.parse().unwrap()]
    };
    let mut total = 0;
    let mut records = Vec::new();
    for index in indices {
        assert!(index < count);
        let (edges, json) = run_one(
            &file,
            index,
            mvp.as_ref(),
            normal.as_ref(),
            reference.as_deref(),
        );
        total += edges;
        records.push(json);
    }
    println!(
        "meshlets={} micro_edges={total} all-transformed-and-setup",
        records.len()
    );
    if let Some(path) = output {
        fs::write(&path, format!("{{\"meshlets\":[{}]}}\n", records.join(","))).unwrap();
        println!("setup_json={path}");
    }
}
