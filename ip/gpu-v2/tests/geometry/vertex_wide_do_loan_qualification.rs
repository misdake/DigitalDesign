//! Bounded independent numeric and ownership qualification of the standalone leaf.
use gpu_v2::vertex::{
    do_loan::{Registers, State, VertexEmu},
    emu::{CheckedContext, Header, Inputs, Outputs, Ticket},
    ports::{Context, PackedVertex},
    sim::{counted, oracle},
};
use std::{
    fs,
    io::{BufWriter, Write},
    path::PathBuf,
};
const MAX_VERTEX_CYCLES: usize = 2000;
const MAX_LOAD_CYCLES: usize = 200;

// Independent magnitude/division RNE; execution uses signed floor and remainder.
fn gold_round(value: i128, shift: u32) -> i128 {
    let sign = if value < 0 { -1 } else { 1 };
    let magnitude = value.abs();
    let divisor = 1_i128 << shift;
    let quotient = magnitude / divisor;
    let remainder = magnitude % divisor;
    sign * (quotient
        + i128::from(2 * remainder > divisor || (2 * remainder == divisor && quotient % 2 == 1)))
}
struct Gold {
    position: [i64; 3],
    normal: [i16; 3],
    clip_sum: [i128; 4],
    normal_sum: [i128; 3],
    rows: Vec<u64>,
    fault: u8,
}
fn gold(context: &Context, vertex: PackedVertex) -> Gold {
    let bits = vertex.bits();
    let position = std::array::from_fn(|i| {
        i64::from(context.base[i])
            + ((((bits >> (2 + 10 * i)) & 1023) as i64) << context.grid_shift)
    });
    let normal = std::array::from_fn(|i| (bits >> (32 + 8 * i)) as u8 as i8 as i16 * 128);
    let clip_sum = std::array::from_fn(|i| {
        (0..3)
            .map(|j| i128::from(context.mvp[i][j]) * i128::from(position[j]))
            .sum::<i128>()
            + i128::from(context.mvp[i][3]) * 65536
    });
    let normal_sum = std::array::from_fn(|i| {
        (0..3)
            .map(|j| i128::from(context.normal_matrix[i][j]) * i128::from(normal[j]))
            .sum::<i128>()
    });
    let mut g = Gold {
        position,
        normal,
        clip_sum,
        normal_sum,
        rows: Vec::new(),
        fault: 0,
    };
    if bits & 3 != 0 {
        g.fault = 3;
        return g;
    }
    if position.iter().any(|&p| i32::try_from(p).is_err()) {
        g.fault = 4;
        return g;
    }
    for &sum in &clip_sum {
        let rounded = gold_round(sum, 16);
        if let Ok(value) = i32::try_from(rounded) {
            g.rows.push(u64::from(value as u32));
        } else {
            g.fault = 5;
            return g;
        }
    }
    let mut pack = 0;
    for (i, &sum) in normal_sum.iter().enumerate() {
        let rounded = gold_round(sum, 18);
        if !(-2048..=2047).contains(&rounded) {
            g.fault = 6;
            return g;
        }
        pack |= (rounded as u64 & 4095) << (12 * i);
    }
    g.rows.extend([
        pack,
        ((bits >> 56) & 0xffffff) as u64,
        (bits >> 80) as u16 as u64,
    ]);
    g
}
fn packed(fields: &[(u128, usize)]) -> String {
    let width: usize = fields.iter().map(|f| f.1).sum();
    let padding = (4 - width % 4) % 4;
    let mut bits = vec![false; padding];
    for &(value, n) in fields {
        for shift in (0..n).rev() {
            bits.push(value & (1_u128 << shift) != 0);
        }
    }
    bits.chunks(4)
        .map(|n| char::from_digit(n.iter().fold(0, |a, &b| a * 2 + u32::from(b)), 16).unwrap())
        .collect()
}
fn input_bits(i: &Inputs) -> String {
    let h = i.config.unwrap_or_default();
    let (v, t) = i
        .vertex
        .unwrap_or((PackedVertex([0; 3]), Ticket::default()));
    packed(&[
        (i.reset as u128, 1),
        (i.ce as u128, 1),
        (i.config.is_some() as u128, 1),
        (h.base[0] as u32 as u128, 32),
        (h.base[1] as u32 as u128, 32),
        (h.base[2] as u32 as u128, 32),
        (h.grid_shift as u128, 5),
        (i.coefficient.is_some() as u128, 1),
        (i.coefficient.unwrap_or(0) as u32 as u128, 32),
        (i.vertex.is_some() as u128, 1),
        (v.bits(), 96),
        (t.slot as u128, 1),
        (t.epoch as u128, 32),
        (t.vertex as u128, 6),
        (i.write_ready as u128, 1),
        (i.publish_ready as u128, 1),
    ])
}
fn output_bits(o: &Outputs) -> String {
    packed(&[
        (o.config_ready as u128, 1),
        (o.coefficient_ready as u128, 1),
        (o.coefficient_index as u128, 5),
        (o.vertex_ready as u128, 1),
        (o.write_valid as u128, 1),
        (o.row_index as u128, 3),
        (o.row_data as u128, 36),
        (o.publish_valid as u128, 1),
        (o.ticket.slot as u128, 1),
        (o.ticket.epoch as u128, 32),
        (o.ticket.vertex as u128, 6),
        (o.fault as u128, 3),
    ])
}
fn register_bits(r: &Registers) -> String {
    packed(&[
        (r.state as u8 as u128, 4),
        (r.loaded as u128, 1),
        (r.header.base[0] as u32 as u128, 32),
        (r.header.base[1] as u32 as u128, 32),
        (r.header.base[2] as u32 as u128, 32),
        (r.header.grid_shift as u128, 5),
        (r.coefficient_index as u128, 5),
        (r.raw, 96),
        (r.position[0] as u32 as u128, 32),
        (r.position[1] as u32 as u128, 32),
        (r.position[2] as u32 as u128, 32),
        (r.row as u128, 3),
        (r.normal_row as u128, 2),
        (r.col as u128, 2),
        (r.coefficient_q as u32 as u128, 32),
        (r.wide_product as u128, 72),
        (r.narrow_product as u32 as u128, 32),
        (r.accumulator as u128, 66),
        (r.normal_pack as u128, 36),
        (r.row_data as u128, 36),
        (r.ticket.slot as u128, 1),
        (r.ticket.epoch as u128, 32),
        (r.ticket.vertex as u128, 6),
        (r.fault as u128, 3),
    ])
}
struct Driver {
    emu: VertexEmu,
    file: Option<BufWriter<fs::File>>,
    cycles: usize,
    states: [usize; 12],
    pauses: [usize; 12],
    writes: usize,
    publications: usize,
    row_holds: [usize; 7],
    publication_holds: usize,
}
impl Driver {
    fn new() -> Self {
        let file = std::env::var_os("VERTEX_QUAL_DIR").map(|p| {
            let p = PathBuf::from(p);
            fs::create_dir_all(&p).unwrap();
            BufWriter::new(fs::File::create(p.join("calendar.txt")).unwrap())
        });
        Self {
            emu: VertexEmu::default(),
            file,
            cycles: 0,
            states: [0; 12],
            pauses: [0; 12],
            writes: 0,
            publications: 0,
            row_holds: [0; 7],
            publication_holds: 0,
        }
    }
    fn step(&mut self, input: Inputs) -> Outputs {
        let before = self.emu.regs.clone();
        self.states[before.state as usize] += 1;
        if !input.ce && !input.reset {
            self.pauses[before.state as usize] += 1;
        }
        let out = self.emu.tick(input);
        if !input.reset && before.state == State::Write && !(input.ce && input.write_ready) {
            self.row_holds[before.row as usize] += 1;
            assert_eq!(self.emu.regs, before, "held private row and identity");
        }
        if !input.reset && before.state == State::Publish && !(input.ce && input.publish_ready) {
            self.publication_holds += 1;
            assert_eq!(self.emu.regs, before, "held publication and identity");
        }
        if !input.reset && matches!(before.state, State::TermReturn | State::Accumulate) {
            assert_eq!(
                self.emu.regs.coefficient_q, before.coefficient_q,
                "actual RAM DO lease cannot be replaced before product consumption"
            );
        }
        if !input.ce && !input.reset {
            assert_eq!(self.emu.regs, before, "CE0 owned registers");
        }
        if out.write_valid && input.write_ready && input.ce && !input.reset {
            self.writes += 1;
        }
        if out.publish_valid && input.publish_ready && input.ce && !input.reset {
            self.publications += 1;
        }
        if let Some(file) = &mut self.file {
            writeln!(
                file,
                "{} {} {}",
                input_bits(&input),
                output_bits(&out),
                register_bits(&self.emu.regs)
            )
            .unwrap();
        }
        self.cycles += 1;
        out
    }
    fn reset(&mut self) {
        self.step(Inputs {
            reset: true,
            ..Inputs::default()
        });
    }
    fn load(&mut self, context: &Context, checked: bool) {
        let c = if checked {
            CheckedContext::new(context).unwrap()
        } else {
            CheckedContext {
                header: Header {
                    base: context.base,
                    grid_shift: context.grid_shift,
                },
                words: std::array::from_fn(|i| {
                    if i < 16 {
                        context.mvp[i / 4][i % 4]
                    } else {
                        i32::from(context.normal_matrix[(i - 16) / 3][(i - 16) % 3])
                    }
                }),
            }
        };
        self.step(Inputs {
            ce: false,
            config: Some(c.header),
            vertex: Some((PackedVertex([0; 3]), Ticket::default())),
            ..Inputs::default()
        });
        let output = self.step(Inputs {
            ce: true,
            config: Some(c.header),
            vertex: Some((PackedVertex([0; 3]), Ticket::default())),
            ..Inputs::default()
        });
        assert!(output.config_ready);
        assert!(!output.vertex_ready, "config arbitration");
        let mut written = 0;
        for cycle in 0..MAX_LOAD_CYCLES {
            if written == 25 {
                break;
            }
            let input = Inputs {
                ce: cycle % 5 != 0,
                coefficient: Some(c.words[written]),
                vertex: Some((PackedVertex([0; 3]), Ticket::default())),
                ..Inputs::default()
            };
            let out = self.step(input);
            assert!(out.coefficient_ready);
            assert!(!out.vertex_ready);
            assert_eq!(out.coefficient_index as usize, written);
            if input.ce {
                written += 1;
            }
        }
        assert_eq!(written, 25, "bounded Context loader");
        assert!(self.emu.regs.loaded);
    }
    fn vertex(
        &mut self,
        context: &Context,
        vertex: PackedVertex,
        case: usize,
        stalls: bool,
    ) -> usize {
        let g = gold(context, vertex);
        if context.validate().is_ok() {
            let oracle = oracle::run(context, vertex, &oracle::Config::default());
            if g.fault == 0 {
                assert_eq!(oracle.unwrap().output.rows().as_slice(), g.rows);
                assert_eq!(
                    counted::run(context, vertex)
                        .unwrap()
                        .output
                        .rows()
                        .as_slice(),
                    g.rows
                );
            } else {
                assert!(oracle.is_err());
                assert!(counted::run(context, vertex).is_err());
            }
        }
        let ticket = Ticket {
            slot: case % 2 == 1,
            epoch: 0xf0000000 + case as u32,
            vertex: (case % 64) as u8,
        };
        for _ in 0..2 {
            let offer = self.step(Inputs {
                ce: false,
                vertex: Some((vertex, ticket)),
                ..Inputs::default()
            });
            assert!(offer.vertex_ready, "CE0 does not hide the old ready offer");
        }
        let accepted = self.step(Inputs {
            ce: true,
            vertex: Some((vertex, ticket)),
            ..Inputs::default()
        });
        assert!(accepted.vertex_ready);
        let mut rows = Vec::new();
        let mut last_write = None;
        let mut edges = 1;
        let stray = (
            PackedVertex([0xffff_fffc, 0xabcd1234, 0x55555555]),
            Ticket {
                slot: !ticket.slot,
                epoch: !ticket.epoch,
                vertex: 63,
            },
        );
        for cycle in 0..MAX_VERTEX_CYCLES {
            let before = self.emu.regs.clone();
            let ce = !stalls || !(cycle + case).is_multiple_of(7);
            let input = Inputs {
                ce,
                vertex: Some(stray),
                config: Some(Header {
                    base: [-1; 3],
                    grid_shift: 31,
                }),
                coefficient: Some(i32::MIN),
                write_ready: !stalls || cycle % 5 == 0,
                publish_ready: !stalls || cycle % 11 == 0,
                ..Inputs::default()
            };
            let out = self.step(input);
            if ce {
                edges += 1;
            }
            assert!(
                !out.vertex_ready && !out.config_ready && !out.coefficient_ready,
                "busy ownership"
            );
            if before.state != State::Fault {
                assert_eq!(out.ticket, ticket);
            }
            if matches!(before.state, State::TermReturn | State::Accumulate) {
                let col = before.col as usize;
                let coefficient = if before.row < 4 {
                    context.mvp[before.row as usize][col]
                } else {
                    i32::from(context.normal_matrix[before.normal_row as usize][col])
                };
                assert_eq!(
                    before.coefficient_q, coefficient,
                    "leased DO contains the actual coefficient"
                );
                if ce && before.state == State::TermReturn {
                    if before.row < 4 {
                        assert_eq!(
                            self.emu.regs.wide_product,
                            i128::from(coefficient) * i128::from(g.position[col]),
                            "single native edge registers full signed72 product from RAM DO"
                        );
                    } else {
                        assert_eq!(
                            self.emu.regs.narrow_product,
                            i32::from(coefficient as i16) * i32::from(g.normal[col]),
                            "one-edge narrow result from registered DO"
                        );
                    }
                }
            }
            if ce && before.state == State::Accumulate {
                let c = usize::from(before.col);
                let expected = if before.row < 4 {
                    let row = usize::from(before.row);
                    assert_eq!(
                        before.wide_product,
                        i128::from(context.mvp[row][c]) * i128::from(g.position[c]),
                        "actual wide operand/product"
                    );
                    i128::from(context.mvp[row][3]) * 65536
                        + (0..=c)
                            .map(|k| i128::from(context.mvp[row][k]) * i128::from(g.position[k]))
                            .sum::<i128>()
                } else {
                    let row = usize::from(before.normal_row);
                    assert_eq!(
                        before.narrow_product,
                        i32::from(context.normal_matrix[row][c]) * i32::from(g.normal[c]),
                        "actual narrow operand/product"
                    );
                    (0..=c)
                        .map(|k| {
                            i128::from(context.normal_matrix[row][k]) * i128::from(g.normal[k])
                        })
                        .sum::<i128>()
                };
                assert_eq!(
                    self.emu.regs.accumulator, expected,
                    "independent sum prefix"
                );
            }
            if before.state == State::Round {
                let sum = if before.row < 4 {
                    g.clip_sum[before.row as usize]
                } else {
                    g.normal_sum[before.normal_row as usize]
                };
                assert_eq!(before.accumulator, sum, "independent complete row sum");
            }
            if out.write_valid && ce && input.write_ready {
                assert_eq!(out.row_index as usize, rows.len());
                rows.push(out.row_data);
                last_write = Some(cycle);
            }
            if out.publish_valid {
                assert_eq!(rows.len(), 7);
                assert!(cycle > last_write.unwrap(), "publication has its own edge");
                if ce && input.publish_ready {
                    assert_eq!(g.fault, 0);
                    assert_eq!(rows, g.rows);
                    assert_eq!(self.emu.regs.state, State::Idle);
                    return edges;
                }
            }
            if self.emu.regs.state == State::Fault {
                assert_eq!(self.emu.regs.fault, g.fault);
                assert_eq!(rows, g.rows, "private partial rows before arithmetic fault");
                for _ in 0..3 {
                    let o = self.step(Inputs {
                        ce: true,
                        vertex: Some(stray),
                        write_ready: true,
                        publish_ready: true,
                        ..Inputs::default()
                    });
                    assert!(!o.publish_valid && !o.write_valid && !o.vertex_ready);
                }
                return edges;
            }
        }
        panic!(
            "bounded vertex timeout case={case} state={:?}",
            self.emu.regs.state
        );
    }
}

#[test]
fn bounded_wide_do_loan_numeric_ownership_calendar() {
    let mut d = Driver::new();
    let mut case = 0;
    let v = PackedVertex::encode([1023, 511, 0], [-128, 127, -1], [4095, 0], 65535).unwrap();
    d.reset();
    d.load(&Context::default(), true);
    let baseline = d.vertex(&Context::default(), v, case, false);
    case += 1;
    assert_eq!(
        baseline, 90,
        "sequential baseline accept through publication"
    );
    // Same loaded Context, changed vertex and destination: no hidden precomputed frame.
    d.vertex(
        &Context::default(),
        PackedVertex::encode([1, 2, 3], [0, -128, 127], [0, 4095], 0).unwrap(),
        case,
        true,
    );
    case += 1;
    let zero = PackedVertex::encode([0; 3], [-128, 127, -128], [0, 4095], 0x8410).unwrap();
    for base in [
        -98305, -98304, -98303, -32769, -32768, -32767, 32767, 32768, 32769, 98303, 98304, 98305,
    ] {
        let mut c = Context {
            base: [base, 0, 0],
            grid_shift: 0,
            ..Context::default()
        };
        c.mvp = [[0; 4]; 4];
        c.mvp[0][0] = 1;
        c.mvp[1][0] = -1;
        c.mvp[2][3] = i32::MIN;
        c.mvp[3][3] = i32::MAX;
        d.reset();
        d.load(&c, true);
        d.vertex(&c, zero, case, true);
        case += 1;
    }
    for (translation, base) in [
        (i32::MIN, -32767),
        (i32::MIN, -32768),
        (i32::MIN, -32769),
        (i32::MAX, 32767),
        (i32::MAX, 32768),
        (i32::MAX, 32769),
    ] {
        let mut c = Context {
            base: [base, 0, 0],
            grid_shift: 0,
            ..Context::default()
        };
        c.mvp = [[0; 4]; 4];
        c.mvp[3][0] = 1;
        c.mvp[3][3] = translation;
        d.reset();
        d.load(&c, true);
        d.vertex(&c, zero, case, true);
        case += 1;
    }
    for (base, shift, xyz) in [
        (i32::MIN, 30, 1),
        (i32::MIN, 30, 3),
        (0, 30, 2),
        (i32::MAX, 0, 1),
        (i32::MIN, 0, 0),
    ] {
        let mut c = Context {
            base: [base, 0, 0],
            grid_shift: shift,
            ..Context::default()
        };
        c.mvp = [[0; 4]; 4];
        c.mvp[0][0] = 1;
        let v = PackedVertex::encode([xyz, 0, 0], [-128, 0, 127], [4095; 2], 0).unwrap();
        d.reset();
        d.load(&c, true);
        d.vertex(&c, v, case, true);
        case += 1;
    }
    // Full signed 32x32 products and 66-bit sums; faults occur only at row rounding.
    for coeff in [i32::MIN, i32::MAX, -65536, 65536] {
        let mut c = Context {
            base: [i32::MIN, i32::MAX, -1],
            grid_shift: 0,
            ..Context::default()
        };
        c.mvp = [[coeff, coeff, coeff, 0]; 4];
        d.reset();
        d.load(&c, true);
        d.vertex(&c, zero, case, true);
        case += 1;
    }
    for coefficient in [i32::MIN, i32::MAX] {
        let mut c = Context {
            base: [i32::MIN; 3],
            grid_shift: 0,
            ..Context::default()
        };
        c.mvp = [[0; 4]; 4];
        c.mvp[3] = [coefficient, coefficient, coefficient, 0];
        d.reset();
        d.load(&c, true);
        d.vertex(&c, zero, case, true);
        case += 1;
    }
    // Position-width successor: every high-part boundary with negative and
    // positive base. Independent gold still uses the untruncated 40-bit delta.
    for grid in 22..=30 {
        let limit = 1023_u16.min((((1_u64 << 32) - 1) >> grid) as u16);
        for xyz in [limit, (limit + 1).min(1023)] {
            for base in [i32::MIN, 0, i32::MAX] {
                let c = Context {
                    base: [base; 3],
                    grid_shift: grid,
                    mvp: [[0; 4]; 4],
                    ..Context::default()
                };
                let v = PackedVertex::encode([xyz; 3], [-128, 127, -1], [4095, 0], 0x7bef).unwrap();
                d.reset();
                d.load(&c, true);
                d.vertex(&c, v, case, true);
                case += 1;
            }
        }
    }
    let mut seed = 0x5736_38ab_19de_1123u64;
    fn next(s: &mut u64) -> u32 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s as u32
    }
    for n in 0..160 {
        let normal_matrix = match n % 3 {
            0 => [[16384, 0, 0], [0, -16384, 0], [0, 0, -16384]],
            1 => [[11585, -11585, 0], [11585, 11585, 0], [0, 0, 16384]],
            _ => [[0, 0, -16384], [16384, 0, 0], [0, -16384, 0]],
        };
        let c = Context {
            mvp: std::array::from_fn(|_| {
                std::array::from_fn(|_| (next(&mut seed) % 1_000_001) as i32 - 500000)
            }),
            normal_matrix,
            base: std::array::from_fn(|_| (next(&mut seed) % 100_000_001) as i32 - 50000000),
            grid_shift: (n % 17) as u8,
        };
        let v = PackedVertex::encode(
            std::array::from_fn(|_| (next(&mut seed) & 1023) as u16),
            std::array::from_fn(|_| next(&mut seed) as i8),
            std::array::from_fn(|_| (next(&mut seed) & 4095) as u16),
            next(&mut seed) as u16,
        )
        .unwrap();
        d.reset();
        d.load(&c, true);
        d.vertex(&c, v, case, true);
        case += 1;
    }
    // Malformed tag; checked-context normal overflow cannot occur, but raw bad
    // coefficients must still fail privately if driver preflight is bypassed.
    d.reset();
    d.load(&Context::default(), true);
    let mut bad = zero;
    bad.0[0] |= 1;
    d.vertex(&Context::default(), bad, case, true);
    case += 1;
    for coefficient in [-32768, 32767] {
        let c = Context {
            normal_matrix: [[coefficient; 3]; 3],
            ..Context::default()
        };
        d.reset();
        d.load(&c, false);
        d.vertex(
            &c,
            PackedVertex::encode([0; 3], [-128; 3], [0; 2], 0).unwrap(),
            case,
            true,
        );
        case += 1;
    }
    // Defensive normal RNE ties/bounds using explicitly unchecked Context beats.
    for coefficient in [
        -24, -8, 8, 24, -32768, -32760, -32759, 32759, 32760, 32761, 32767,
    ] {
        let mut c = Context {
            normal_matrix: [[0; 3]; 3],
            ..Context::default()
        };
        c.normal_matrix[0][0] = coefficient;
        d.reset();
        d.load(&c, false);
        d.vertex(
            &c,
            PackedVertex::encode([0; 3], [-128, 0, 0], [0; 2], 0).unwrap(),
            case,
            true,
        );
        case += 1;
    }
    for (main, extra) in [
        (32767, 1151),
        (32767, 1152),
        (32767, 1153),
        (-32752, -1023),
        (-32752, -1024),
        (-32752, -1025),
    ] {
        let mut c = Context {
            normal_matrix: [[0; 3]; 3],
            ..Context::default()
        };
        c.normal_matrix[0] = [main, extra, 0];
        let v = PackedVertex::encode([0; 3], [-128, -1, 0], [0; 2], 0).unwrap();
        d.reset();
        d.load(&c, false);
        d.vertex(&c, v, case, true);
        case += 1;
    }
    // Invalid grid and normal-word transport: no coefficient write/publication.
    d.reset();
    d.step(Inputs {
        ce: true,
        config: Some(Header {
            grid_shift: 31,
            ..Header::default()
        }),
        ..Inputs::default()
    });
    assert_eq!(d.emu.regs.fault, 1);
    d.reset();
    d.step(Inputs {
        ce: true,
        config: Some(Header::default()),
        ..Inputs::default()
    });
    for _ in 0..16 {
        d.step(Inputs {
            ce: true,
            coefficient: Some(0),
            ..Inputs::default()
        });
    }
    d.step(Inputs {
        ce: true,
        coefficient: Some(65535),
        ..Inputs::default()
    });
    assert_eq!(d.emu.regs.fault, 2);
    d.reset();
    d.step(Inputs {
        ce: true,
        config: Some(Header::default()),
        ..Inputs::default()
    });
    for _ in 0..24 {
        d.step(Inputs {
            ce: true,
            coefficient: Some(0),
            ..Inputs::default()
        });
    }
    assert_eq!(d.emu.regs.state, State::Load);
    d.reset();
    assert!(!d.emu.regs.loaded, "reset discards an incomplete Context");
    // Reset every control state after real ownership/partial writes. Loading
    // a fresh Context afterward proves reset cannot expose abandoned RAM data.
    for target in [
        State::Position,
        State::InitRead,
        State::InitReturn,
        State::TermRead,
        State::TermReturn,
        State::Accumulate,
        State::Round,
        State::Write,
        State::Publish,
    ] {
        d.reset();
        d.load(&Context::default(), true);
        d.step(Inputs {
            ce: true,
            vertex: Some((
                v,
                Ticket {
                    slot: true,
                    epoch: u32::MAX,
                    vertex: 63,
                },
            )),
            ..Inputs::default()
        });
        let mut reached = false;
        for _ in 0..MAX_VERTEX_CYCLES {
            if d.emu.regs.state == target {
                reached = true;
                break;
            }
            d.step(Inputs {
                ce: true,
                write_ready: true,
                publish_ready: false,
                ..Inputs::default()
            });
        }
        assert!(reached);
        for _ in 0..2 {
            d.step(Inputs {
                ce: false,
                write_ready: true,
                publish_ready: true,
                vertex: Some((v, Ticket::default())),
                config: Some(Header::default()),
                coefficient: Some(i32::MIN),
                ..Inputs::default()
            });
        }
        d.reset();
        for _ in 0..3 {
            let o = d.step(Inputs {
                ce: true,
                vertex: Some((v, Ticket::default())),
                write_ready: true,
                publish_ready: true,
                ..Inputs::default()
            });
            assert!(!o.vertex_ready && !o.write_valid && !o.publish_valid);
        }
    }
    d.reset();
    d.load(&Context::default(), true);
    d.vertex(&Context::default(), v, case, true);
    case += 1;
    assert!(d.states.iter().all(|&n| n > 0));
    // Rebind at idle without reset; actual coefficient beats replace the Context.
    let rebound = Context {
        base: [-98304, 32768, -2],
        grid_shift: 3,
        ..Context::default()
    };
    for _ in 0..3 {
        d.load(&rebound, true);
        d.vertex(&rebound, v, case, true);
        case += 1;
    }
    let mut costs = Vec::new();
    for n in [1, 3, 8, 64] {
        d.reset();
        let c = CheckedContext::new(&Context::default()).unwrap();
        let start = d.cycles;
        d.step(Inputs {
            ce: true,
            config: Some(c.header),
            ..Inputs::default()
        });
        for word in c.words {
            d.step(Inputs {
                ce: true,
                coefficient: Some(word),
                ..Inputs::default()
            });
        }
        assert_eq!(d.cycles - start, 26);
        assert!(d.emu.regs.loaded);
        let mut vertex_edges = 0;
        for _ in 0..n {
            vertex_edges += d.vertex(&Context::default(), v, case, false);
            case += 1;
        }
        assert_eq!(vertex_edges, n * 90);
        costs.push(format!("{{\"vertices\":{n},\"load_edges\":26,\"vertex_edges\":{vertex_edges},\"total_enabled_edges\":{},\"old_position32_total_enabled_edges\":{},\"amortized_enabled_edges_per_vertex\":{}}}",26+vertex_edges,26+n*123,(26+vertex_edges)as f64/n as f64));
    }
    assert!(d.row_holds.iter().all(|&n| n > 0));
    assert!(d.publication_holds > 0);
    if let Some(path) = std::env::var_os("VERTEX_QUAL_DIR") {
        let p = PathBuf::from(path);
        fs::write(p.join("draw-cost.json"), format!("[{}]\n", costs.join(","))).unwrap();
        fs::write(p.join("lease-offer-coverage.json"),format!("{{\"dense_states\":12,\"row_holds\":{:?},\"publication_holds\":{},\"ram_do_checked_at_issue_and_consumption\":true,\"wide_calendar\":\"TERM_RETURN full signed72 product, ACCUMULATE old-product consume\",\"narrow_calendar\":\"TERM_RETURN output, ACCUMULATE old-product consume\"}}\n",d.row_holds,d.publication_holds)).unwrap();
    }
    // Arithmetic/read/return/write/publication CE0 was exercised.
    for state in 0..=11 {
        assert!(d.pauses[state] > 0, "pause coverage state {state}");
    }
    if let Some(mut file) = d.file {
        file.flush().unwrap();
    }
    if let Some(path) = std::env::var_os("VERTEX_QUAL_DIR") {
        fs::write(PathBuf::from(path).join("emu-receipt.json"),format!("{{\"cases\":{case},\"cycles\":{},\"sequential_enabled_edges\":{baseline},\"writes\":{},\"publications\":{},\"state_coverage\":{:?},\"ce0_state_coverage\":{:?},\"maximum_vertex_cycles\":{MAX_VERTEX_CYCLES},\"maximum_load_cycles\":{MAX_LOAD_CYCLES}}}\n",d.cycles,d.writes,d.publications,d.states,d.pauses)).unwrap();
    }
    println!(
        "PASS cases={case} cycles={} enabled_edges={baseline} writes={} publications={}",
        d.cycles, d.writes, d.publications
    );
}
