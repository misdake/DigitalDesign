pub mod native;
pub mod rtl;
use crate::vertex_support;
use gpu_v2::system::vliw_cycle::vertex::{Bits, Error, Input, Meshlet, Phase, Triangle};
#[derive(Clone, Copy)]
pub struct Case {
    pub triangles: usize,
    pub shape: u8,
    pub cull: bool,
    pub normal18: bool,
    pub rgb: bool,
    pub slot: bool,
    pub hold_full: bool,
    pub stall_each: bool,
    pub simultaneous: bool,
    pub color: u32,
    pub seq: u8,
    pub uniform: u8,
}
impl Default for Case {
    fn default() -> Self {
        Self {
            triangles: 1,
            shape: 0,
            cull: false,
            normal18: false,
            rgb: false,
            slot: false,
            hold_full: false,
            stall_each: false,
            simultaneous: false,
            color: 0x927bad31,
            seq: 126,
            uniform: 20,
        }
    }
}
pub struct Data {
    pub rows: Vec<(u16, u64)>,
    pub vertices: Vec<u64>,
    pub packets: Vec<(u32, [u64; 16])>,
    pub per_triangle: Vec<Option<[u64; 16]>>,
}
fn cross(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn data(case: Case) -> Data {
    let mut d = vertex_support::data(vertex_support::Case {
        normal18: case.normal18,
        rgb: case.rgb,
        slot: case.slot,
        uniform: case.uniform,
        ..Default::default()
    });
    let (origin, shift, delta) = match case.shape {
        2 => (
            [-65536, -65536, -4 * 65536],
            15,
            [[0, 0, 0], [4, 0, 0], [0, 4, 0]],
        ),
        4 => (
            [-65536, -65536, -65536],
            15,
            [[0, 0, 0], [4, 0, 4], [0, 4, 0]],
        ),
        5 => ([0, 0, 1], 0, [[1, 0, 0], [0, 1, 0], [0, 0, 0]]),
        10 => (
            [33554430, 0, 4 * 65536],
            0,
            [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
        ),
        9 => (
            [-30000003, 20000001, 10000007],
            0,
            [[0, 0, 0], [1019, 7, 1], [11, 1009, 3]],
        ),
        8 => (
            [1 << 25, 0, 4 * 65536],
            0,
            [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
        ),
        7 => (
            [-65531, -65529, 4 * 65536 + 3],
            8,
            [[0, 0, 0], [511, 3, 0], [2, 507, 1]],
        ),
        6 => (
            [-(1 << 25), 0, 4 * 65536],
            15,
            [[0, 0, 0], [1023, 0, 0], [0, 4, 0]],
        ),
        _ => (
            [-65536, -65536, 4 * 65536],
            8,
            [
                [0, 0, 0],
                [512, 0, 0],
                [0, if case.shape == 1 { 0 } else { 512 }, 0],
            ],
        ),
    };
    let matrix = std::array::from_fn::<_, 12, _>(|n| {
        let q = d
            .rows
            .iter()
            .find(|(a, _)| *a == u16::from(case.uniform) * 16 + (n / 2) as u16)
            .unwrap()
            .1;
        i128::from((q >> ((n % 2) * 32)) as u32 as i32)
    });
    let positions = delta.map(|v| {
        let world = std::array::from_fn::<_, 3, _>(|k| {
            i128::from(origin[k]) + i128::from(v[k]) * (1i128 << shift)
        });
        std::array::from_fn::<_, 3, _>(|j| {
            let a = (0..3).map(|k| world[k] * matrix[j * 4 + k]).sum::<i128>()
                + 65536 * matrix[j * 4 + 3];
            i128::from((a >> 16) as u32 as i32)
        })
    });
    let base = u16::from(case.slot) * 128;
    let meta = 256 + u16::from(case.slot) * 4;
    let row = |rows: &mut Vec<(u16, u64)>, a: u16, value: u64| {
        if let Some(q) = rows.iter_mut().find(|q| q.0 == a) {
            q.1 = value;
        } else {
            rows.push((a, value));
        }
    };
    row(
        &mut d.rows,
        u16::from(case.uniform) * 16 + 11,
        u64::from(case.color),
    );
    row(
        &mut d.rows,
        meta,
        3 | (case.triangles as u64) << 8
            | (u64::from(case.uniform) << 16)
            | (5 << 24)
            | (u64::from(case.cull) << 31),
    );
    row(
        &mut d.rows,
        meta + 1,
        (u64::from(case.rgb) << 4) | (u64::from(case.seq) << 5) | (3 << 12),
    );
    row(
        &mut d.rows,
        meta + 2,
        u64::from(origin[0] as u32) | (u64::from(origin[1] as u32) << 32),
    );
    row(
        &mut d.rows,
        meta + 3,
        u64::from(origin[2] as u32)
            | (u64::from(shift as u8) << 32)
            | (u64::from(case.normal18) << 40),
    );
    for (v, xyz) in delta.iter().enumerate() {
        let w = (xyz[0] | xyz[1] << 10 | xyz[2] << 20) as u32;
        let a = base + (v * 3 / 2) as u16;
        let old = d.rows.iter().find(|q| q.0 == a).unwrap().1;
        let value = if v * 3 % 2 == 0 {
            (old & 0xffffffff00000000) | u64::from(w)
        } else {
            (old & 0xffffffff) | (u64::from(w) << 32)
        };
        row(&mut d.rows, a, value);
    }
    let mut vertices = Vec::new();
    for (v, p) in positions.iter().enumerate() {
        vertices.extend(p.map(|n| n as u32 as u64));
        vertices.extend([d.expected[v * 5 + 3], d.expected[v * 5 + 4]]);
    }
    let refs: Vec<[usize; 3]> = (0..case.triangles)
        .map(|t| {
            if case.shape == 3 {
                [t % 3, (t + 2) % 3, (t + 1) % 3]
            } else {
                [t % 3, (t + 1) % 3, (t + 2) % 3]
            }
        })
        .collect();
    for (group, triples) in refs.chunks(3).enumerate() {
        let value = triples.iter().enumerate().fold(0u64, |s, (j, v)| {
            s | ((v[0] | v[1] << 6 | v[2] << 12) as u64) << (j * 18)
        });
        row(&mut d.rows, base + 5 + group as u16, value);
    }
    let mut packets = Vec::new();
    let mut per_triangle = Vec::new();
    for (t, refs) in refs.iter().enumerate() {
        let p = refs.map(|v| positions[v]);
        let mut n = [cross(p[1], p[2]), cross(p[2], p[0]), cross(p[0], p[1])];
        let mut q = [0u32; 3];
        for (j, n) in n.iter_mut().enumerate() {
            q[j] = n
                .iter()
                .map(|&v| 128 - (if v < 0 { !v } else { v }).unsigned_abs().leading_zeros())
                .max()
                .unwrap()
                .saturating_sub(26);
            for v in n {
                *v = if *v < 0 { -((-*v) >> q[j]) } else { *v >> q[j] };
            }
        }
        let det = p[0].into_iter().zip(n[0]).map(|(a, b)| a * b).sum::<i128>();
        if p.iter().all(|p| p[2] <= 0) || det == 0 || det < 0 && case.cull {
            per_triangle.push(None);
            continue;
        }
        let mut words = [0u64; 16];
        for j in 0..3 {
            let [x, y, z] = n[j];
            words[j * 3] = (x as u64 & ((1 << 36) - 1)) | (u64::from(q[j]) << 48);
            words[j * 3 + 1] = y as u64 & ((1 << 54) - 1);
            words[j * 3 + 2] = (-399 * x - 239 * y + 512 * z) as u64 & ((1 << 54) - 1);
        }
        let bit = 127 - det.unsigned_abs().leading_zeros();
        let k = if bit >= 30 {
            det.unsigned_abs() >> (bit - 30)
        } else {
            det.unsigned_abs() << (30 - bit)
        };
        words[9] = k as u64 | (u64::from(bit + q[0]) << 31);
        for (j, &v) in refs.iter().enumerate() {
            words[10 + j * 2] = vertices[v * 5 + 3];
            words[11 + j * 2] = vertices[v * 5 + 4];
        }
        words[10] |= u64::from(case.color & ((1 << 18) - 1)) << 36;
        words[11] |= u64::from(case.color >> 18) << 36;
        let desc = (3 << 28)
            | u32::from(case.seq.wrapping_add(t as u8) & 127) << 21
            | u32::from(case.normal18) << 20
            | u32::from(case.rgb) << 19
            | u32::from(det < 0) << 18
            | (refs[0] | refs[1] << 6 | refs[2] << 12) as u32;
        packets.push((desc, words));
        per_triangle.push(Some(words));
    }
    Data {
        rows: d.rows,
        vertices,
        packets,
        per_triangle,
    }
}
#[derive(Clone)]
pub struct Edge {
    pub vertex: vertex_support::Edge,
    pub t: Triangle,
    pub pq: Bits,
    pub head: Bits,
    pub produced: u8,
    pub consumed: u8,
    pub import_state: u8,
    pub owner: bool,
    pub fetch: u8,
    pub response: u8,
    pub low_q: Bits,
    pub high_q: Bits,
    pub group: u64,
    pub group_position: u8,
    pub cursor: u8,
    pub triangles: u8,
    pub sequence: u8,
}
pub fn edge(m: &Meshlet, i: Input) -> Edge {
    let e = &m.core.export;
    Edge {
        vertex: vertex_support::edge(m, i),
        t: m.core.triangle.clone(),
        pq: m.core.position_q,
        head: e.head(),
        produced: e.produced,
        consumed: e.consumed,
        import_state: e.import_state,
        owner: e.result_owner,
        fetch: e.fetch_index,
        response: e.response_index,
        low_q: e.low_q,
        high_q: e.high_q,
        group: m.index_group,
        group_position: m.group_position,
        cursor: m.index_cursor,
        triangles: m.triangles,
        sequence: m.sequence,
    }
}
#[derive(Default)]
pub struct Stats {
    pub admitted: usize,
    pub publish: usize,
    pub packets: usize,
    pub words: usize,
    pub v_writes: usize,
    pub full_edges: usize,
    pub held_words: usize,
    pub done_before_drain: bool,
    pub max_queue: u8,
    pub resets: usize,
    pub epoch_blocked: usize,
    pub last_word_stalls: usize,
    pub overlapping_v: usize,
    pub stalled_items: u16,
    pub simultaneous: usize,
}
pub struct Run {
    pub error: Option<Error>,
    pub trace: Vec<Edge>,
    pub stats: Stats,
}
pub fn run(case: Case) -> Run {
    drive(case, None, None)
}
pub fn reset_run(case: Case, phase: &str) -> Run {
    drive(case, Some(phase), None)
}
pub fn fault(name: &str) -> Run {
    let case = Case {
        triangles: if name == "too_many" {
            97
        } else if name == "cursor" {
            4
        } else {
            1
        },
        shape: if name == "position" { 8 } else { 0 },
        ..Default::default()
    };
    drive(case, None, Some(name))
}
fn drive(case: Case, reset_phase: Option<&str>, fault: Option<&str>) -> Run {
    let mut d = data(case);
    if let Some(f) = fault {
        let metadata = d
            .rows
            .iter_mut()
            .find(|(a, _)| *a == 256 + u16::from(case.slot) * 4)
            .unwrap();
        match f {
            "zero_vertices" => metadata.1 &= !127,
            "offset" => metadata.1 = (metadata.1 & !(127 << 24)) | (4 << 24),
            "cursor" => metadata.1 = (metadata.1 & !(127 << 24)) | (127 << 24),
            _ => {}
        }
        if f == "refs" {
            d.rows
                .iter_mut()
                .find(|(a, _)| *a == u16::from(case.slot) * 128 + 5)
                .unwrap()
                .1 |= 63;
        }
        if f == "cursor" {
            let first = d.rows.iter().find(|(a, _)| *a == 5).unwrap().1;
            d.rows.push((127, first));
        }
    }
    let mut m = Meshlet::with_triangles();
    let mut r = Run {
        trace: Vec::new(),
        error: None,
        stats: Stats::default(),
    };
    let mut loaded = 0;
    let mut started = false;
    let mut done = false;
    let mut drain = !case.hold_full;
    let mut full_hold = 0;
    let mut wi = 0;
    let mut reset_done = false;
    let mut each_held = 0u16;
    let mut reads = 0usize;
    for wall in 0..100000 {
        let mut i = Input {
            reset: wall < 2,
            ce: wall % 9 > 2,
            mesh_color: case.color,
            slot: case.slot,
            output_ready: wall % 13 > 3,
            packet_ready: drain && wall % 7 > 1,
            word_ready: drain && wall % 17 > 5,
            ..Default::default()
        };
        if wall >= 2 && loaded < d.rows.len() {
            let (a, v) = d.rows[loaded];
            i.dma_write = true;
            i.dma_address = a;
            i.dma_data = v;
            i.ce = false;
            loaded += 1;
        }
        if loaded == d.rows.len() && !started && !i.dma_write && m.outputs(i).ready {
            i.start = true;
            i.ce = true;
            started = true;
        }
        let runtime_reset = !reset_done
            && match reset_phase {
                Some("queue") => m.core.export.full(),
                Some("Q") => m.core.export.result_owner,
                Some("word") => m.core.export.valid,
                _ => false,
            };
        let retained = std::array::from_fn::<_, 16, _>(|n| m.core.rf_word(n as u8));
        if runtime_reset {
            i.reset = true;
            i.ce = false;
            i.start = false;
            i.dma_write = false;
            loaded = 0;
            started = false;
            done = false;
            drain = !case.hold_full;
            full_hold = 0;
            wi = 0;
            each_held = 0;
            reads = 0;
            reset_done = true;
            r.stats = Stats {
                resets: 1,
                ..Default::default()
            };
        }
        if !i.reset && m.core.regs.triangle_mode && m.core.regs.busy {
            match fault {
                Some("cp_owner") => {
                    i.cp_read = true;
                    i.cp_address = 320;
                }
                Some("active_page") => {
                    i.dma_write = true;
                    i.dma_address = u16::from(m.active_slot) * 128;
                    i.dma_data = 0xabcdef;
                }
                _ => {}
            }
        }
        if case.simultaneous && r.stats.packets == 1 {
            let raw = m.outputs(i);
            if raw.core.word_valid && raw.core.word_index == 15 {
                i.word_ready = raw.core.publish;
            }
        }
        if case.stall_each && !i.reset {
            let o = m.outputs(i);
            if o.core.word_valid && i.ce && each_held & (1 << o.core.word_index) == 0 {
                i.word_ready = false;
                each_held |= 1 << o.core.word_index;
            }
        }
        let e = edge(&m, i);
        let o = e.vertex.output;
        r.stats.max_queue = r.stats.max_queue.max(m.core.export.count());
        if m.core.export.full() {
            r.stats.full_edges += 1;
            full_hold += usize::from(i.ce);
            if full_hold > 12 {
                drain = true;
            }
            assert!(!(m.phase == Phase::TStart && o.core.triangle_ready));
        }
        if i.ce && m.phase == Phase::TStart && o.core.triangle_ready {
            r.stats.admitted += 1;
        }
        if o.core.coefficient_write {
            let expected =
                d.per_triangle[r.stats.admitted - 1].expect("rejected triangle wrote coefficient");
            assert_eq!(
                o.core.coefficient_data.get(54, "coefficient").unwrap(),
                expected[(o.core.coefficient_address >> 4) as usize],
                "T{} row{} pc{}",
                r.stats.admitted - 1,
                o.core.coefficient_address >> 4,
                m.core.regs.pc
            );
        }
        if o.core.publish {
            r.stats.publish += 1;
            r.stats.simultaneous += usize::from(o.core.release_slot);
        }
        if o.core.position_write || o.core.attribute_write {
            let n = r.stats.v_writes;
            assert_eq!(o.core.emit_address, n as u16);
            assert_eq!(o.core.emit_data.get(36, "V result").unwrap(), d.vertices[n]);
            r.stats.v_writes += 1;
        }
        if o.core.packet_take {
            each_held = 0;
            reads = usize::from(o.core.export_read);
            assert_eq!(
                o.core.descriptor.get(32, "descriptor").unwrap() as u32,
                d.packets[r.stats.packets].0
            );
            r.stats.packets += 1;
            wi = 0;
        }
        if !o.core.packet_take && o.core.export_read {
            reads += 1;
        }
        if o.core.release_slot {
            assert_eq!(reads, 16, "exactly sixteen paid reads before slot release");
        }
        if o.core.word_take {
            assert_eq!(o.core.word_index, wi);
            assert_eq!(
                o.core.word_data.get(54, "T16 word").unwrap(),
                d.packets[r.stats.packets - 1].1[wi as usize],
                "packet{} word{wi}",
                r.stats.packets - 1
            );
            wi += 1;
            r.stats.words += 1;
        }
        if o.core.word_valid && (!i.ce || !i.word_ready) {
            r.stats.held_words += 1;
            r.stats.stalled_items |= 1 << o.core.word_index;
        }
        if o.done && i.ce {
            done = true;
            r.stats.done_before_drain |= !o.drained;
        }
        r.trace.push(e);
        if let Err(error) = m.clock(i) {
            assert!(
                fault.is_some(),
                "wall{wall} phase{:?} pc{}: {error}",
                m.phase,
                m.core.regs.pc
            );
            r.error = Some(error);
            return r;
        }
        if runtime_reset {
            for (n, &value) in retained.iter().enumerate() {
                assert_eq!(m.core.rf_word(n as u8), value);
            }
            assert!(!m.core.export.valid && m.core.export.empty());
        }
        if done && o.drained && !m.input_active {
            assert_eq!(r.stats.admitted, case.triangles);
            assert_eq!(r.stats.publish, d.packets.len());
            assert_eq!(r.stats.packets, d.packets.len());
            assert_eq!(r.stats.words, d.packets.len() * 16);
            assert_eq!(r.stats.v_writes, 15);
            if case.hold_full {
                assert!(r
                    .trace
                    .iter()
                    .any(|e| e.produced.wrapping_sub(e.consumed) & 31 == 16
                        && e.vertex.output.core.release_slot));
            }
            if case.simultaneous {
                assert!(r.stats.simultaneous > 0);
            }
            return r;
        }
    }
    panic!("T closure exceeded100000 logic edges")
}

pub fn epochs() -> Run {
    let cases = [
        Case::default(),
        Case {
            slot: true,
            shape: 5,
            rgb: true,
            normal18: true,
            color: 0x4567_fedc,
            ..Default::default()
        },
        Case {
            shape: 6,
            color: 0xa123_5678,
            ..Default::default()
        },
    ];
    let data = cases.map(data);
    let packets: Vec<_> = data
        .iter()
        .flat_map(|d| d.packets.iter().copied())
        .collect();
    let mut m = Meshlet::with_triangles();
    let mut r = Run {
        trace: Vec::new(),
        error: None,
        stats: Stats::default(),
    };
    let mut loaded = [0; 3];
    let mut started = 0;
    let mut completed = 0;
    let mut drain = false;
    let mut wi = 0;
    let mut writes = [0usize; 3];
    let rows: Vec<Vec<_>> = data.iter().map(|d| d.rows.clone()).collect();
    for wall in 0..100000 {
        let mut i = Input {
            reset: wall < 2,
            ce: wall % 9 > 2,
            slot: started == 1,
            mesh_color: cases[if started > completed {
                started - 1
            } else {
                started.min(2)
            }]
            .color,
            output_ready: wall % 13 > 3,
            packet_ready: drain && wall % 7 > 1,
            word_ready: drain && wall % 17 > 5,
            ..Default::default()
        };
        let next = completed.min(2);
        if wall >= 2 && completed < 3 && loaded[next] < rows[next].len() {
            let (a, v) = rows[next][loaded[next]];
            i.dma_write = true;
            i.dma_address = a;
            i.dma_data = v;
            i.ce = false;
            loaded[next] += 1;
        }
        if started < 3
            && completed == started
            && loaded[started] == rows[started].len()
            && !i.dma_write
            && m.outputs(i).ready
            && i.ce
        {
            i.start = true;
            assert!(
                started != 2 || m.core.export.consumed >= 1,
                "third epoch admitted before A last export"
            );
        }
        if completed == 2 && loaded[2] == rows[2].len() && m.core.export.consumed == 0 {
            assert!(!m.outputs(i).ready);
            if i.ce {
                r.stats.epoch_blocked += 1;
            }
            if r.stats.epoch_blocked > 12 {
                drain = true;
            }
        }
        let raw = m.outputs(i);
        if r.stats.packets == 1
            && raw.core.word_valid
            && raw.core.word_index == 15
            && r.stats.last_word_stalls < 13
        {
            i.word_ready = false;
            if i.ce {
                r.stats.last_word_stalls += 1;
            }
        }
        if r.stats.packets == 2 && raw.core.word_valid && raw.core.word_index == 10 && completed < 3
        {
            i.word_ready = false;
        }
        let e = edge(&m, i);
        let o = e.vertex.output;
        if started > 0 && m.core.export.consumed == 0 && completed > 0 {
            assert_eq!(
                m.core.attribute(320).get(36, "A epoch held").unwrap(),
                data[0].vertices[3]
            );
            assert_eq!(
                m.core.epoch_color(true).get(32, "A color held").unwrap(),
                u64::from(cases[0].color)
            );
        }
        if o.core.position_write || o.core.attribute_write {
            let n = started - 1;
            let item = writes[n];
            assert_eq!(
                o.core.emit_data.get(36, "epoch V result").unwrap(),
                data[n].vertices[item]
            );
            assert_eq!(o.core.emit_address, item as u16);
            writes[n] += 1;
            r.stats.v_writes += 1;
            if r.stats.packets == 2 && o.core.word_valid && !i.word_ready {
                r.stats.overlapping_v += 1;
            }
        }
        if i.ce && m.phase == Phase::TStart && o.core.triangle_ready {
            r.stats.admitted += 1;
        }
        if o.core.coefficient_write {
            assert_eq!(
                o.core
                    .coefficient_data
                    .get(54, "epoch coefficient")
                    .unwrap(),
                data[started - 1].per_triangle[0].unwrap()
                    [(o.core.coefficient_address >> 4) as usize]
            );
        }
        if o.core.publish {
            r.stats.publish += 1;
        }
        if o.core.packet_take {
            assert_eq!(
                o.core.descriptor.get(32, "epoch descriptor").unwrap(),
                u64::from(packets[r.stats.packets].0)
            );
            r.stats.packets += 1;
            wi = 0;
        }
        if o.core.word_take {
            assert_eq!(o.core.word_index, wi);
            assert_eq!(
                o.core.word_data.get(54, "epoch T16").unwrap(),
                packets[r.stats.packets - 1].1[wi as usize]
            );
            wi += 1;
            r.stats.words += 1;
        }
        if o.done && i.ce {
            assert_eq!(writes[completed], 15);
            completed += 1;
            r.stats.done_before_drain |= !o.drained;
        }
        r.trace.push(e);
        m.clock(i).unwrap_or_else(|err| {
            panic!("epochs wall{wall} started{started} complete{completed}: {err}")
        });
        if i.start {
            started += 1;
        }
        if completed == 3 && o.drained && !m.input_active {
            assert_eq!(r.stats.words, 48);
            assert_eq!(r.stats.publish, 3);
            assert!(
                r.stats.epoch_blocked > 12
                    && r.stats.last_word_stalls == 13
                    && r.stats.overlapping_v > 0
            );
            return r;
        }
    }
    panic!("three epoch closure exceeded100000 logic edges")
}
