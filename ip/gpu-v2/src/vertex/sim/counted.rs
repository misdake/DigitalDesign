//! V6 decoding, matrices, intermediate sums and stores stay in the closed ledger.
use super::super::format::*;
use super::super::ports::*;
use audited::{Address, Fault, Fixed, Frame, FrameReport, Memory, Model};
pub struct Report {
    pub output: Transformed,
    pub frame: FrameReport,
}
pub struct BatchReport {
    pub outputs: Vec<Transformed>,
    pub frame: FrameReport,
}
fn next<const B: u32>(
    f: &Frame<'_>,
    index: Fixed<B, 0, false>,
) -> Result<Fixed<B, 0, false>, Fault> {
    f.add_same(index, Fixed::<B, 0, false>::constant::<1>())
}
// The offline ROM has sixteen statically named coefficient addresses. The host
// loop chooses a program instruction, never constructs a runtime numerical value.
fn coefficient<const B: u32, const F: u32, const S: bool>(
    memory: Memory<B, F, S>,
    row: usize,
) -> Address<B, F, S> {
    match row {
        0 => memory.at::<0>(),
        1 => memory.at::<1>(),
        2 => memory.at::<2>(),
        3 => memory.at::<3>(),
        4 => memory.at::<4>(),
        5 => memory.at::<5>(),
        6 => memory.at::<6>(),
        7 => memory.at::<7>(),
        8 => memory.at::<8>(),
        9 => memory.at::<9>(),
        10 => memory.at::<10>(),
        11 => memory.at::<11>(),
        12 => memory.at::<12>(),
        13 => memory.at::<13>(),
        14 => memory.at::<14>(),
        15 => memory.at::<15>(),
        _ => unreachable!("static coefficient ROM row"),
    }
}
fn execute(context: &Context, vertices: &[PackedVertex]) -> Result<FrameReport, Fault> {
    let mut m = Model::numerical();
    let raw = m.input::<96, 0, false>(
        "v6",
        &vertices
            .iter()
            .map(|v| v.bits() as i128)
            .collect::<Vec<_>>(),
    )?;
    let base = m.input::<32, 16, true>("grid_base", &context.base.map(i128::from))?;
    let shift = m.input::<18, 0, true>("grid_shift", &[i128::from(context.grid_shift)])?;
    let matrix_values: Vec<_> = context
        .mvp
        .iter()
        .flatten()
        .copied()
        .map(i128::from)
        .collect();
    let normals: Vec<_> = context
        .normal_matrix
        .iter()
        .flatten()
        .copied()
        .map(i128::from)
        .collect();
    let matrix_input = m.input::<32, 16, true>("mvp_source", &matrix_values)?;
    let normal_input = m.input::<16, 14, true>("normal_source", &normals)?;
    let matrix = m.scratch::<32, 16, true>("MVP", 16)?;
    let normal_matrix = m.scratch::<16, 14, true>("NORMAL", 9)?;
    let output = m.scratch::<36, 0, false>("TRANSFORMED", 512)?;
    let f = m.compute("vertex-v6", 4096 * vertices.len() + 512)?;
    for row in 0..16 {
        f.write(
            coefficient(matrix, row),
            f.read(coefficient(matrix_input, row))?,
        )?;
    }
    for row in 0..9 {
        f.write(
            coefficient(normal_matrix, row),
            f.read(coefficient(normal_input, row))?,
        )?;
    }
    let mut vertex_addr = Fixed::<7, 0, false>::constant::<0>();
    let mut output_addr = Fixed::<9, 0, false>::constant::<0>();
    for vertex_index in 0..vertices.len() {
        let prefix = format!("vertex.{vertex_index}");
        let packed: Packed = f.read(raw.indexed(vertex_addr))?;
        let tag = f.slice::<2, 0, false, 0>(packed)?;
        f.require::<true>(f.less(tag, Fixed::<2, 0, false>::constant::<1>())?)?;
        let amount = f.read(shift.at::<0>())?;
        let coordinates = [
            f.slice::<10, 0, false, 2>(packed)?,
            f.slice::<10, 0, false, 12>(packed)?,
            f.slice::<10, 0, false, 22>(packed)?,
        ];
        let bases = [
            f.read(base.at::<0>())?,
            f.read(base.at::<1>())?,
            f.read(base.at::<2>())?,
        ];
        let mut position = [Position::constant::<0>(); 3];
        for k in 0..3 {
            let delta = f.shift(f.resize_exact::<64, 0, true>(coordinates[k])?, amount)?;
            let delta = f.binary_scale::<64, 16, true>(delta)?;
            position[k] = f.resize_exact(f.add::<64, 16, true>(bases[k], delta)?)?;
            f.publish(&format!("{prefix}.position.{k}"), position[k])?;
        }
        let normal8 = [
            f.slice::<8, 7, true, 32>(packed)?,
            f.slice::<8, 7, true, 40>(packed)?,
            f.slice::<8, 7, true, 48>(packed)?,
        ];
        let mut input_normal = [Normal::constant::<0>(); 3];
        for k in 0..3 {
            let extended = f.resize_exact::<16, 7, true>(normal8[k])?;
            input_normal[k] = f.binary_scale(f.shift_left_const::<7, 16, 7, true>(extended)?)?;
            f.publish(&format!("{prefix}.normal.input.{k}"), input_normal[k])?;
        }
        let mut clip = [Clip::constant::<0>(); 4];
        for (row, destination) in clip.iter_mut().enumerate() {
            let mut products = [ClipProduct::constant::<0>(); 3];
            for k in 0..3 {
                products[k] = f.product(f.read(coefficient(matrix, row * 4 + k))?, position[k])?;
            }
            let translation = f.read(coefficient(matrix, row * 4 + 3))?;
            let translation = f.binary_scale::<66, 32, true>(
                f.shift_left_const::<16, 66, 16, true>(f.resize_exact(translation)?)?,
            )?;
            let xy: ClipSum = f.add(products[0], products[1])?;
            let xyz = f.add_same(xy, f.resize_exact(products[2])?)?;
            let sum = f.add_same(xyz, translation)?;
            f.publish(&format!("{prefix}.clip.sum.{row}"), sum)?;
            *destination = f.round_to(sum)?;
            f.publish(&format!("{prefix}.clip.{row}"), *destination)?;
        }
        let mut normal = [Normal::constant::<0>(); 3];
        for (row, destination) in normal.iter_mut().enumerate() {
            let mut products = [NormalProduct::constant::<0>(); 3];
            for k in 0..3 {
                products[k] = f.product(
                    f.read(coefficient(normal_matrix, row * 3 + k))?,
                    input_normal[k],
                )?;
            }
            let xy: NormalSum = f.add(products[0], products[1])?;
            let sum = f.add_same(xy, f.resize_exact(products[2])?)?;
            f.publish(&format!("{prefix}.normal.sum.{row}"), sum)?;
            *destination = f.round_to(sum)?;
            f.publish(&format!("{prefix}.normal.{row}"), *destination)?;
        }
        let uv: [UvCode; 2] = [
            f.slice::<12, 0, false, 56>(packed)?,
            f.slice::<12, 0, false, 68>(packed)?,
        ];
        let rgb = f.slice::<16, 0, false, 80>(packed)?;
        f.publish(&format!("{prefix}.u"), uv[0])?;
        f.publish(&format!("{prefix}.v"), uv[1])?;
        f.publish(&format!("{prefix}.rgb565"), rgb)?;
        let mut rows = [TransformedRow::constant::<0>(); 7];
        for k in 0..4 {
            rows[k] = f.resize_exact(f.slice::<32, 0, false, 0>(clip[k])?)?;
        }
        let nx: Fixed<36, 0, false> = f.resize_exact(f.slice::<16, 0, false, 0>(normal[0])?)?;
        let ny: Fixed<36, 0, false> = f.resize_exact(f.slice::<16, 0, false, 0>(normal[1])?)?;
        rows[4] = f.add_same(nx, f.shift_left_const::<16, 36, 0, false>(ny)?)?;
        let nz: Fixed<36, 0, false> = f.resize_exact(f.slice::<16, 0, false, 0>(normal[2])?)?;
        rows[5] = f.add_same(
            nz,
            f.shift_left_const::<16, 36, 0, false>(f.resize_exact(uv[0])?)?,
        )?;
        rows[6] = f.add_same(
            f.resize_exact(uv[1])?,
            f.shift_left_const::<12, 36, 0, false>(f.resize_exact(rgb)?)?,
        )?;
        for (k, row) in rows.iter().enumerate() {
            f.write(output.indexed(output_addr), *row)?;
            f.publish(&format!("{prefix}.row.{k}"), *row)?;
            output_addr = next(&f, output_addr)?;
        }
        vertex_addr = next(&f, vertex_addr)?;
    }
    Ok(f.finish())
}
pub fn run(context: &Context, vertex: PackedVertex) -> Result<Report, String> {
    let batch = run_batch(context, &[vertex])?;
    Ok(Report {
        output: batch.outputs[0].clone(),
        frame: batch.frame,
    })
}
pub fn run_batch(context: &Context, vertices: &[PackedVertex]) -> Result<BatchReport, String> {
    context.validate()?;
    if vertices.is_empty() || vertices.len() > 64 {
        return Err("vertex batch must have 1..64 entries".into());
    }
    let frame = execute(context, vertices).map_err(|f| format!("vertex numerical fault: {f:?}"))?;
    frame.audit().map_err(|f| format!("vertex audit: {f:?}"))?;
    let value = |name: &str| {
        frame
            .outputs
            .iter()
            .find(|o| o.name == name)
            .expect("published vertex port")
            .raw
    };
    let outputs = (0..vertices.len())
        .map(|vertex| {
            let prefix = format!("vertex.{vertex}");
            Transformed {
                clip: std::array::from_fn(|i| value(&format!("{prefix}.clip.{i}")) as i32),
                normal: std::array::from_fn(|i| value(&format!("{prefix}.normal.{i}")) as i16),
                uv: [
                    value(&format!("{prefix}.u")) as u16,
                    value(&format!("{prefix}.v")) as u16,
                ],
                rgb565: value(&format!("{prefix}.rgb565")) as u16,
            }
        })
        .collect();
    Ok(BatchReport { outputs, frame })
}
