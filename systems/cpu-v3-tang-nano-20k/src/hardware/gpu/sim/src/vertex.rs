//! Minimal vertex microprogram experiment for the standalone cmodel.
//!
//! The four operations are scheduler instructions, not an ISA encoding. Q16
//! is the former vertex profile and remains provisional until the v2 spec sets
//! the formats and overflow response.

use crate::fixed::{round_shift_ties_even, NumericFault, Q16};
use crate::timing::{Mult36Pipeline, RamRead, SyncRam64};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawVertex {
    pub position: [Q16; 4],
    pub rgb565: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VertexOp {
    LoadRaw { vertex: usize },
    Mvp,
    Publish { destination: u8 },
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VertexResult {
    pub destination: u8,
    pub clip: [Q16; 4],
    pub rgb565: u16,
    /// First row whose rounded result required a 32-bit hardware wrap.
    pub overflow_row: Option<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VertexStep {
    pub edge: u64,
    pub pc: usize,
    pub read_address: Option<usize>,
    pub issued_product: Option<u8>,
    pub retired_product: Option<u8>,
    pub published: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VertexRun {
    pub edges: u64,
    pub results: Vec<VertexResult>,
    pub trace: Vec<VertexStep>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VertexError {
    EmptyProgram,
    MissingStop,
    VertexOutOfRange,
    NoLoadedVertex,
    NoTransform,
    RamCollision,
    Numeric(NumericFault),
    CycleLimit(u64),
}

impl From<NumericFault> for VertexError {
    fn from(value: NumericFault) -> Self {
        Self::Numeric(value)
    }
}

fn ram_data(output: RamRead) -> Result<u64, VertexError> {
    match output {
        RamRead::Data(value) => Ok(value),
        RamRead::Collision => Err(VertexError::RamCollision),
    }
}

fn retire_product(
    tag: u64,
    product: i128,
    sums: &mut [i128; 4],
    clip: &mut [Q16; 4],
    overflow_row: &mut Option<u8>,
) -> Result<u8, VertexError> {
    let row = (tag / 4) as usize;
    sums[row] += product;
    if tag % 4 == 3 {
        let rounded = round_shift_ties_even(sums[row], 16)?;
        if Q16::from_raw(rounded).is_err() && overflow_row.is_none() {
            *overflow_row = Some(row as u8);
        }
        clip[row] = Q16::wrapping_from_raw(rounded)?;
    }
    Ok(tag as u8)
}

/// Execute a bounded sequence against a packed, synchronous 64-bit raw store.
/// Vertices occupy three words each; the 4x4 Q16 matrix occupies eight more.
/// Each MVP issues one signed 36x36 product per edge and rounds once per row.
pub fn run_vertex_program(
    vertices: &[RawVertex],
    matrix: [[Q16; 4]; 4],
    program: &[VertexOp],
    max_edges: u64,
) -> Result<VertexRun, VertexError> {
    if program.is_empty() {
        return Err(VertexError::EmptyProgram);
    }
    let zero = Q16::from_raw(0)?;
    let matrix_base = vertices.len() * 3;
    let mut words = vec![0_u64; matrix_base + 8];
    for (index, vertex) in vertices.iter().enumerate() {
        let bits = vertex.position.map(|value| value.raw() as i32 as u32);
        words[index * 3] = u64::from(bits[0]) | (u64::from(bits[1]) << 32);
        words[index * 3 + 1] = u64::from(bits[2]) | (u64::from(bits[3]) << 32);
        words[index * 3 + 2] = u64::from(vertex.rgb565);
    }
    for pair in 0..8 {
        let row = pair / 2;
        let column = (pair % 2) * 2;
        words[matrix_base + pair] = u64::from(matrix[row][column].raw() as i32 as u32)
            | (u64::from(matrix[row][column + 1].raw() as i32 as u32) << 32);
    }
    let mut ram = SyncRam64::new(words);
    let mut loaded: Option<RawVertex> = None;
    let mut load_phase: Option<(usize, u8, [u64; 3])> = None;
    let mut clip = [zero; 4];
    let mut transformed = false;
    let mut overflow_row = None;
    let mut pc = 0;
    let mut next_product: Option<u8> = None;
    let mut coefficient_high = 0_i32;
    let mut sums = [0_i128; 4];
    let mut dsp = Mult36Pipeline::default();
    let mut results = Vec::new();
    let mut trace = Vec::new();

    for edge in 1..=max_edges {
        let old_pc = pc;
        let mut read_address = None;
        let mut issued_product = None;
        let mut retired_product = None;
        let mut published = false;
        if let Some((vertex, received, mut raw_words)) = load_phase.take() {
            raw_words[usize::from(received)] = ram_data(ram.output())?;
            if received < 2 {
                read_address = Some(vertex * 3 + usize::from(received) + 1);
                load_phase = Some((vertex, received + 1, raw_words));
            } else {
                let position = [
                    Q16::from_raw(i128::from(raw_words[0] as u32 as i32))?,
                    Q16::from_raw(i128::from((raw_words[0] >> 32) as u32 as i32))?,
                    Q16::from_raw(i128::from(raw_words[1] as u32 as i32))?,
                    Q16::from_raw(i128::from((raw_words[1] >> 32) as u32 as i32))?,
                ];
                loaded = Some(RawVertex {
                    position,
                    rgb565: raw_words[2] as u16,
                });
                transformed = false;
                pc += 1;
            }
        } else if let Some(next) = next_product {
            let issue = if next < 16 {
                let vertex = loaded.ok_or(VertexError::NoLoadedVertex)?;
                let coefficient = if next & 1 == 0 {
                    let pair = ram_data(ram.output())?;
                    coefficient_high = (pair >> 32) as u32 as i32;
                    if next < 14 {
                        read_address = Some(matrix_base + usize::from(next / 2 + 1));
                    }
                    pair as u32 as i32
                } else {
                    coefficient_high
                };
                issued_product = Some(next);
                Some((
                    u64::from(next),
                    i64::from(coefficient),
                    vertex.position[usize::from(next % 4)].raw(),
                ))
            } else {
                None
            };
            if let Some((tag, product)) = dsp.tick(issue)? {
                retired_product = Some(retire_product(
                    tag,
                    product,
                    &mut sums,
                    &mut clip,
                    &mut overflow_row,
                )?);
            }
            if retired_product == Some(15) {
                next_product = None;
                transformed = true;
                pc += 1;
            } else {
                next_product = Some(next.saturating_add(1));
            }
        } else {
            match program.get(pc).ok_or(VertexError::MissingStop)? {
                VertexOp::LoadRaw { vertex } => {
                    if *vertex >= vertices.len() {
                        return Err(VertexError::VertexOutOfRange);
                    }
                    read_address = Some(*vertex * 3);
                    load_phase = Some((*vertex, 0, [0; 3]));
                }
                VertexOp::Mvp => {
                    if loaded.is_none() {
                        return Err(VertexError::NoLoadedVertex);
                    }
                    sums = [0; 4];
                    clip = [zero; 4];
                    overflow_row = None;
                    read_address = Some(matrix_base);
                    next_product = Some(0);
                }
                VertexOp::Publish { destination } => {
                    if !transformed {
                        return Err(VertexError::NoTransform);
                    }
                    results.push(VertexResult {
                        destination: *destination,
                        clip,
                        rgb565: loaded.unwrap().rgb565,
                        overflow_row,
                    });
                    published = true;
                    pc += 1;
                }
                VertexOp::Stop => {
                    trace.push(VertexStep {
                        edge,
                        pc: old_pc,
                        read_address,
                        issued_product,
                        retired_product,
                        published,
                    });
                    return Ok(VertexRun {
                        edges: edge,
                        results,
                        trace,
                    });
                }
            }
        }
        ram.tick(read_address, None);
        trace.push(VertexStep {
            edge,
            pc: old_pc,
            read_address,
            issued_product,
            retired_product,
            published,
        });
    }
    Err(VertexError::CycleLimit(max_edges))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn q(raw: i128) -> Q16 {
        Q16::from_raw(raw).unwrap()
    }

    #[test]
    fn identity_mvp_has_exact_three_edge_dsp_retirement() {
        let vertex = RawVertex {
            position: [q(0x10000), q(-0x20000), q(0x38000), q(0x10000)],
            rgb565: 0xf81f,
        };
        let matrix = std::array::from_fn(|row| {
            std::array::from_fn(|col| q(if row == col { 0x10000 } else { 0 }))
        });
        let program = [
            VertexOp::LoadRaw { vertex: 0 },
            VertexOp::Mvp,
            VertexOp::Publish { destination: 7 },
            VertexOp::Stop,
        ];
        let run = run_vertex_program(&[vertex], matrix, &program, 28).unwrap();
        assert_eq!(run.results[0].clip, vertex.position);
        assert_eq!(run.results[0].rgb565, 0xf81f);
        assert_eq!(
            run.trace
                .iter()
                .filter(|step| step.issued_product.is_some())
                .count(),
            16
        );
        let first_issue = run
            .trace
            .iter()
            .find(|step| step.issued_product == Some(0))
            .unwrap()
            .edge;
        let first_retire = run
            .trace
            .iter()
            .find(|step| step.retired_product == Some(0))
            .unwrap()
            .edge;
        assert_eq!(first_retire - first_issue, 2);
        assert_eq!(
            run.trace
                .iter()
                .filter(|step| step.read_address.is_some())
                .count(),
            11
        );
        assert_eq!(run.edges, 25);
    }

    #[test]
    fn row_rounds_once_and_reports_hardware_wrap() {
        let vertex = RawVertex {
            position: [q(1), q(1), q(0), q(0)],
            rgb565: 0,
        };
        let mut matrix = [[q(0); 4]; 4];
        matrix[0][0] = q(0x8000);
        matrix[0][1] = q(0x8000);
        let run = run_vertex_program(
            &[vertex],
            matrix,
            &[
                VertexOp::LoadRaw { vertex: 0 },
                VertexOp::Mvp,
                VertexOp::Publish { destination: 0 },
                VertexOp::Stop,
            ],
            28,
        )
        .unwrap();
        assert_eq!(run.results[0].clip[0].raw(), 1);
        assert_eq!(run.results[0].overflow_row, None);

        let large = RawVertex {
            position: [q(i32::MAX.into()), q(i32::MAX.into()), q(0), q(0)],
            rgb565: 0,
        };
        matrix[1][0] = q(i32::MAX.into());
        matrix[1][1] = q(i32::MAX.into());
        let large_run = run_vertex_program(
            &[large],
            matrix,
            &[
                VertexOp::LoadRaw { vertex: 0 },
                VertexOp::Mvp,
                VertexOp::Publish { destination: 0 },
                VertexOp::Stop,
            ],
            28,
        )
        .unwrap();
        let full = 2_i128 * i128::from(i32::MAX) * i128::from(i32::MAX);
        let rounded = round_shift_ties_even(full, 16).unwrap();
        assert_eq!(
            large_run.results[0].clip[1],
            Q16::wrapping_from_raw(rounded).unwrap()
        );
        assert_eq!(large_run.results[0].overflow_row, Some(1));
    }

    #[test]
    fn execution_limit_is_explicit() {
        let vertex = RawVertex {
            position: [q(0); 4],
            rgb565: 0,
        };
        let matrix = [[q(0); 4]; 4];
        assert_eq!(
            run_vertex_program(
                &[vertex],
                matrix,
                &[
                    VertexOp::LoadRaw { vertex: 0 },
                    VertexOp::Mvp,
                    VertexOp::Stop
                ],
                5
            ),
            Err(VertexError::CycleLimit(5))
        );
    }

    #[test]
    fn packed_matrix_and_vertex_match_independent_integer_golden() {
        const VALUES: [i32; 7] = [i32::MIN, -0x18000, -1, 0, 1, 0x8000, i32::MAX];
        let program = [
            VertexOp::LoadRaw { vertex: 0 },
            VertexOp::Mvp,
            VertexOp::Publish { destination: 3 },
            VertexOp::Stop,
        ];
        for case in 0..16 {
            let vertex = RawVertex {
                position: std::array::from_fn(|column| {
                    q(i128::from(VALUES[(case + column * 3) % 7]))
                }),
                rgb565: case as u16,
            };
            let matrix = std::array::from_fn(|row| {
                std::array::from_fn(|column| {
                    q(i128::from(VALUES[(case * 2 + row * 5 + column) % 7]))
                })
            });
            let run = run_vertex_program(&[vertex], matrix, &program, 32).unwrap();
            let mut first_overflow = None;
            for (row, coefficients) in matrix.iter().enumerate() {
                let sum = (0..4)
                    .map(|column| {
                        i128::from(coefficients[column].raw())
                            * i128::from(vertex.position[column].raw())
                    })
                    .sum::<i128>();
                let floor = sum >> 16;
                let fraction = sum - (floor << 16);
                let rounded =
                    floor + i128::from(fraction > 0x8000 || (fraction == 0x8000 && floor & 1 != 0));
                if (rounded < i128::from(i32::MIN) || rounded > i128::from(i32::MAX))
                    && first_overflow.is_none()
                {
                    first_overflow = Some(row as u8);
                }
                assert_eq!(
                    run.results[0].clip[row].raw(),
                    i64::from(rounded as i32),
                    "case {case} row {row}"
                );
            }
            assert_eq!(run.results[0].overflow_row, first_overflow, "case {case}");
        }
    }
}
