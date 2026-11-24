use gpu_v2::system::vliw_cycle::vertex::{Bits, Error, Input, Meshlet, Output, Phase, Registers};
pub mod native;
pub mod rtl;

#[derive(Clone, Copy)]
pub struct Case {
    pub vertices: usize,
    pub normal18: bool,
    pub rgb: bool,
    pub billboard: bool,
    pub world: bool,
    pub slot: bool,
    pub cull: u8,
    pub uniform: u8,
}
impl Default for Case {
    fn default() -> Self {
        Self {
            vertices: 3,
            normal18: false,
            rgb: false,
            billboard: false,
            world: false,
            slot: false,
            cull: 0,
            uniform: 20,
        }
    }
}
pub struct Data {
    pub rows: Vec<(u16, u64)>,
    pub expected: Vec<u64>,
    pub rejected: bool,
}
fn signed(v: u64, n: u32) -> i64 {
    ((v << (64 - n)) as i64) >> (64 - n)
}
fn rne(n: i64) -> i64 {
    let a = n.unsigned_abs();
    let q = a / 64;
    let r = a % 64;
    let v = q + u64::from(r * 2 > 64 || r * 2 == 64 && q & 1 != 0);
    if n < 0 {
        -(v as i64)
    } else {
        v as i64
    }
}
pub fn data(case: Case) -> Data {
    let mut uniform = [0u32; 32];
    for at in [0, 5, 10, 12, 16, 20] {
        uniform[at] = 65536;
    }
    uniform[0] = match case.uniform {
        21 => 131072,
        22 => (-65536i32) as u32,
        23 => 0x7fffffff,
        _ => 65536,
    };
    if case.uniform == 23 {
        uniform[5] = 0x80000001;
    }
    uniform[12] = 131300;
    uniform[16] = 16896 + u32::from(case.uniform.saturating_sub(20)) * 4096;
    uniform[20] = 65537;
    uniform[21] = 65536;
    uniform[22] = 0xffa1_b2c3;
    if case.billboard {
        uniform[24..30].copy_from_slice(&[65536, 32768, 0, 65536, 16384, (-8192i32) as u32]);
    }
    let mut origin = if case.uniform == 23 {
        [-0x70000000i32, 0x60000000, 4 * 65536]
    } else {
        [-16384i32, 8192, 4 * 65536]
    };
    let mut shift = 6;
    match case.cull {
        1 => origin[2] = -4 * 65536,
        2 => origin[0] = -20 * 65536,
        3 => origin[0] = 20 * 65536,
        4 => shift = 15,
        _ => {}
    }
    let rejected = !case.billboard && (1..=3).contains(&case.cull) && case.vertices != 0;
    let header = [
        case.vertices as u64
            | (u64::from(case.uniform) << 16)
            | ((case.vertices * 3).div_ceil(2) as u64) << 24,
        u64::from(case.billboard) | (u64::from(case.rgb) << 4) | (3 << 5) | (2 << 12),
        u64::from(origin[0] as u32) | (u64::from(origin[1] as u32) << 32),
        u64::from(origin[2] as u32)
            | (u64::from(shift as u8) << 32)
            | (u64::from(case.normal18) << 40),
    ];
    let mut payload = Vec::new();
    let mut expected = Vec::new();
    for v in 0..case.vertices {
        let xyz = [
            (v * 97 + 3) % 1024,
            (v * 31 + 8) % 1024,
            (v * 7 + 19) % 1024,
        ];
        let normal = [
            if v % 3 == 0 { 511i32 } else { -511 },
            1,
            if case.billboard {
                0
            } else {
                256 - v as i32 * 3
            },
        ];
        let p = (xyz[0] | xyz[1] << 10 | xyz[2] << 20) as u32;
        let n = normal
            .iter()
            .enumerate()
            .fold(0u32, |a, (k, &n)| a | ((n as u32 & 1023) << (k * 10)));
        let attr = if case.rgb {
            0x00a1_b2c3u32.wrapping_add(v as u32)
        } else {
            0xfedc_007bu32.wrapping_add(v as u32 * 17)
        };
        payload.extend([p, n, attr]);
        if !rejected {
            for k in 0..3 {
                let offset = if case.billboard {
                    128 * (i64::from(normal[0]) * i64::from(uniform[24 + 2 * k] as i32)
                        + i64::from(normal[1]) * i64::from(uniform[25 + 2 * k] as i32))
                } else {
                    0
                };
                expected.push(u64::from(
                    ((i64::from(origin[k].wrapping_add((xyz[k] as i32).wrapping_shl(shift)))
                        * i64::from(uniform[k * 5] as i32)
                        + offset)
                        >> 16) as u32,
                ));
            }
            let mut packed = 0u64;
            for (k, &n) in normal.iter().enumerate().filter(|_| !case.billboard) {
                let raw = (i64::from(n) * 128 * i64::from(uniform[12 + k * 3 + k] as i32)) >> 16;
                let q = if case.normal18 {
                    signed(raw as u64, 18)
                } else {
                    4 * signed((raw >> 2) as u64, 16)
                };
                packed |= (rne(q).clamp(-2048, 2047) as u64 & 4095) << (k * 12);
            }
            expected.extend([packed, u64::from(attr)]);
        }
    }
    while payload.len() % 32 != 0 {
        payload.push(0);
    }
    let mut rows = Vec::new();
    let mut uploaded = uniform;
    if case.world {
        assert!(!case.billboard && case.uniform <= 21);
        uploaded[0] /= 2;
        uploaded[5] /= 4;
        uploaded[12] *= 2;
        uploaded[16] *= 4;
        uploaded[16..25].copy_from_slice(&uniform[12..21]);
        uploaded[16] *= 2;
        uploaded[20] *= 4;
        uploaded[12] = 65536;
        uploaded[13..16].fill(0);
        uploaded[25..].fill(0);
        uploaded[30] = uniform[22];
        let mut view = [0u32; 32];
        for (index, value) in [
            (0, 131072),
            (5, 262144),
            (10, 65536),
            (12, 32768),
            (16, 16384),
            (20, 65536),
            (21, 65536),
        ] {
            view[index] = value;
        }
        for (n, q) in view.as_chunks::<2>().0.iter().enumerate() {
            rows.push((368 + n as u16, u64::from(q[0]) | (u64::from(q[1]) << 32)));
        }
    }
    for (n, q) in uploaded.as_chunks::<2>().0.iter().enumerate() {
        rows.push((
            u16::from(case.uniform) * 16 + n as u16,
            u64::from(q[0]) | (u64::from(q[1]) << 32),
        ));
    }
    for (n, &q) in header.iter().enumerate() {
        rows.push((256 + u16::from(case.slot) * 4 + n as u16, q));
    }
    for (n, q) in payload.as_chunks::<2>().0.iter().enumerate() {
        rows.push((
            u16::from(case.slot) * 128 + n as u16,
            u64::from(q[0]) | (u64::from(q[1]) << 32),
        ));
    }
    Data {
        rows,
        expected,
        rejected,
    }
}
#[derive(Clone)]
pub struct Edge {
    pub input: Input,
    pub output: Output,
    pub phase: Phase,
    pub slot: bool,
    pub regs: Registers,
    pub rf: [Bits; 16],
    pub lq: Bits,
    pub rq: Bits,
    pub y: Bits,
    pub colors: [Bits; 2],
    pub input_active: bool,
    pub monitored_page: bool,
}
#[derive(Default)]
pub struct Stats {
    pub writes: usize,
    pub ce0_dma: usize,
    pub stalled_emit: usize,
    pub ce0_busy: usize,
    pub position_writes: usize,
    pub attribute_writes: usize,
}
pub struct Run {
    pub trace: Vec<Edge>,
    pub stats: Stats,
    pub error: Option<Error>,
}
pub fn edge(model: &Meshlet, input: Input) -> Edge {
    Edge {
        input,
        output: model.outputs(input),
        phase: model.phase,
        slot: model.active_slot,
        regs: model.core.regs.clone(),
        rf: std::array::from_fn(|n| model.core.rf_word(n as u8)),
        lq: model.core.input.left_q(),
        rq: model.core.input.right_q(),
        y: model.core.mac_y(),
        colors: [model.core.epoch_color(false), model.core.epoch_color(true)],
        input_active: model.input_active,
        monitored_page: model.monitored_page,
    }
}
pub fn run(case: Case) -> Run {
    drive(case, None, 1)
}
pub fn invalid(fault: &str) -> Run {
    drive(Case::default(), Some(fault), 1)
}
pub fn warm() -> Run {
    drive(Case::default(), None, 2)
}
fn drive(case: Case, fault: Option<&str>, repeats: usize) -> Run {
    let mut source = data(case);
    if let Some(fault) = fault {
        let header = &mut source
            .rows
            .iter_mut()
            .find(|(a, _)| *a == 256 + u16::from(case.slot) * 4)
            .unwrap()
            .1;
        match fault {
            "unsupported" => *header |= 1 << 8,
            "bad_header" => *header = (*header & !127) | 65,
            "bad_uniform" => *header = (*header & !(63 << 16)) | (24 << 16),
            _ => {}
        }
    }
    let mut model = Meshlet::default();
    let mut run = Run {
        trace: Vec::new(),
        stats: Stats::default(),
        error: None,
    };
    let mut loaded = 0;
    let mut launched = false;
    let mut completed = 0;
    for wall in 0usize..50000 {
        let mut i = Input {
            reset: wall < 2,
            ce: wall % 9 > 2,
            slot: case.slot,
            mesh_color: 0xffa1_b2c3,
            world_transform: case.world,
            output_ready: wall % 13 > 3,
            ..Default::default()
        };
        if wall >= 2 && loaded < source.rows.len() {
            let (a, d) = source.rows[loaded];
            i.dma_write = true;
            i.dma_address = a;
            i.dma_data = d;
            i.ce = false;
            loaded += 1;
        }
        if loaded == source.rows.len() && !launched && model.outputs(i).ready && !i.dma_write {
            i.start = true;
            i.ce = true;
            launched = true;
        }
        match fault {
            Some("bad_dma") if wall == 2 => i.dma_address = 512,
            Some("cp_owner") if model.input_active && model.core.regs.busy => {
                i.cp_read = true;
                i.cp_address = 320;
            }
            Some("active_payload") if model.input_active && model.core.regs.busy => {
                i.dma_write = true;
                i.dma_address = u16::from(model.active_slot) * 128;
                i.dma_data = 0x55;
            }
            Some("active_metadata") if model.input_active && model.core.regs.busy => {
                i.dma_write = true;
                i.dma_address = 256 + u16::from(model.active_slot) * 4;
                i.dma_data = 0x66;
            }
            Some("active_uniform") if model.input_active && model.core.regs.busy => {
                i.dma_write = true;
                i.dma_address = 320;
                i.dma_data = 0x77;
            }
            Some("collision") if i.start => {
                i.start = false;
                i.cp_read = true;
                i.cp_address = 320;
                i.dma_write = true;
                i.dma_address = 320;
                i.dma_data = 0x88;
            }
            _ => {}
        }
        let e = edge(&model, i);
        let o = e.output;
        run.trace.push(e);
        if let Err(error) = model.clock(i) {
            assert!(
                fault.is_some(),
                "wall{wall} pc{} phase{:?}: {error}",
                model.core.regs.pc,
                model.phase
            );
            run.error = Some(error);
            return run;
        }
        if i.dma_write && !i.ce {
            run.stats.ce0_dma += 1;
        }
        if o.busy && !i.ce {
            run.stats.ce0_busy += 1;
        }
        if o.core.emit_valid && (!i.ce || !i.output_ready) {
            run.stats.stalled_emit += 1;
        }
        if o.core.position_write || o.core.attribute_write {
            let n = run.stats.writes - completed * source.expected.len();
            assert_eq!(o.core.emit_address, n as u16);
            assert_eq!(
                o.core.emit_data.get(36, "fixture result").unwrap(),
                source.expected[n],
                "result{n} pc{}",
                run.trace.last().unwrap().regs.pc
            );
            let field = n % 5;
            let vertex = n / 5;
            if field < 3 {
                assert!(o.core.position_write);
                assert_eq!(o.core.position_address, (field * 64 + vertex) as u16);
                run.stats.position_writes += 1;
            } else {
                assert!(o.core.attribute_write);
                assert_eq!(
                    o.core.attribute_address,
                    (256 + (field - 3) * 128 + 64 + vertex) as u16
                );
                run.stats.attribute_writes += 1;
            }
            run.stats.writes += 1;
        }
        if o.done {
            assert_eq!(run.stats.writes, (completed + 1) * source.expected.len());
            assert_eq!(
                o.core.published_vertices,
                if source.rejected {
                    0
                } else {
                    case.vertices as u8
                }
            );
            for v in 0..case.vertices {
                if !source.rejected {
                    for field in 0..5 {
                        let value = if field < 3 {
                            model.core.position((field * 64 + v) as u16)
                        } else {
                            model
                                .core
                                .attribute((256 + (field - 3) * 128 + 64 + v) as u16)
                        };
                        assert_eq!(
                            value.get(36, "final result bank").unwrap(),
                            source.expected[v * 5 + field]
                        );
                    }
                }
            }
            let rf = std::array::from_fn::<_, 16, _>(|n| model.core.rf_word(n as u8));
            let stored = model.core.input.peek(u16::from(case.uniform) * 16);
            for suffix in 0..5 {
                let i = Input {
                    reset: suffix == 0,
                    ce: suffix == 2 || suffix == 4,
                    cp_read: suffix == 2,
                    cp_address: u16::from(case.uniform) * 16,
                    ..Default::default()
                };
                run.trace.push(edge(&model, i));
                model.clock(i).unwrap();
                for (n, &v) in rf.iter().enumerate() {
                    assert_eq!(
                        model.core.rf_word(n as u8),
                        v,
                        "RF cells survive runtime reset"
                    );
                }
                assert_eq!(
                    model.core.input.peek(u16::from(case.uniform) * 16),
                    stored,
                    "input cells survive reset"
                );
            }
            completed += 1;
            if completed == repeats {
                return run;
            }
            launched = false;
        }
    }
    panic!("V-mode exceeded50000 logic edges")
}
