//! Closed pixel lighting: all data operations and reads go through audited Frame.
use super::super::{format::*, ports::*};
use audited::{Fault, Fixed, Frame, FrameReport, Memory, Model};

#[derive(Debug)]
pub enum Error {
    Input(InputError),
    Audit(Fault),
}
impl From<Fault> for Error {
    fn from(f: Fault) -> Self {
        Self::Audit(f)
    }
}
impl From<InputError> for Error {
    fn from(f: InputError) -> Self {
        Self::Input(f)
    }
}

pub struct Report {
    pub output: LightingOutput,
    pub frame: FrameReport,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// Use signed quadratic chords to avoid the second abs in each normalize.
    pub signed_square: bool,
    /// Truncate only the nonnegative power interpolation correction.
    pub power_floor: bool,
}
impl Config {
    pub fn optimized() -> Self {
        Self {
            signed_square: true,
            power_floor: true,
        }
    }
}
enum SquareTable {
    Magnitude(Memory<14, 0, false>),
    Signed(Memory<15, 0, false>),
}
struct Tables {
    square: SquareTable,
    rsqrt: Memory<24, 0, false>,
    power: Memory<28, 0, false>,
    context: Memory<43, 0, false>,
}

fn clamp<const B: u32, const F: u32, const S: bool>(
    f: &Frame<'_>,
    x: Fixed<B, F, S>,
    lo: Fixed<B, F, S>,
    hi: Fixed<B, F, S>,
) -> Result<Fixed<B, F, S>, Fault> {
    let x = f.select(f.less(x, lo)?, lo, x)?;
    f.select(f.less(hi, x)?, hi, x)
}
fn magnitude(f: &Frame<'_>, x: Direction) -> Result<Magnitude, Fault> {
    // Extend before negation, including the signed minimum.
    let wide = f.resize_exact::<17, 14, true>(x)?;
    let zero = Fixed::<17, 14, true>::constant::<0>();
    let neg = f.sub_same(zero, wide)?;
    f.resize_exact(f.select(f.less(wide, zero)?, neg, wide)?)
}
fn shifted(f: &Frame<'_>, x: Direction, amount: Fixed<18, 0, true>) -> Result<Direction, Fault> {
    // Keep all possible discarded bits until the fixed-format RNE operation.
    let x = f.resize_exact::<32, 14, true>(x)?;
    let x = f.shift_left_const::<14, 32, 14, true>(x)?;
    let x: NormalizedWork = f.binary_scale(x)?;
    f.round_to(f.shift(x, amount)?)
}
fn normalize(
    f: &Frame<'_>,
    raw: [Direction; 3],
    threshold: Magnitude,
    bounded: bool,
    t: &Tables,
    prefix: &str,
) -> Result<[Direction; 3], Fault> {
    let mags = [
        magnitude(f, raw[0])?,
        magnitude(f, raw[1])?,
        magnitude(f, raw[2])?,
    ];
    let mut m = mags[0];
    for v in &mags[1..] {
        m = f.select(f.less(m, *v)?, *v, m)?;
    }
    let zero = f.less(m, threshold)?;
    let safe = [
        Direction::constant::<8192>(),
        Direction::constant::<0>(),
        Direction::constant::<0>(),
    ];
    let mut v = raw;
    for i in 0..3 {
        v[i] = f.select(zero, safe[i], v[i])?;
    }
    m = f.select(zero, Magnitude::constant::<8192>(), m)?;
    let mut amount = if bounded {
        Fixed::<18, 0, true>::constant::<0>()
    } else {
        f.sub_same(f.leading_zeros(m)?, Fixed::<18, 0, true>::constant::<3>())?
    };
    if !bounded {
        // RNE can turn 32767/2 into 16384. Keep the SQ index strictly below 128.
        let mw = f.resize_exact::<32, 14, true>(m)?;
        let mw = f.binary_scale::<32, 28, true>(f.shift_left_const::<14, 32, 14, true>(mw)?)?;
        let rounded: Magnitude = f.round_to(f.shift(mw, amount)?)?;
        let below = f.less(rounded, Magnitude::constant::<16384>())?;
        amount = f.select(
            below,
            amount,
            f.sub_same(amount, Fixed::<18, 0, true>::constant::<1>())?,
        )?;
    }
    f.publish(&format!("{prefix}.shift"), amount)?;
    for value in &mut v {
        *value = shifted(f, *value, amount)?;
    }
    let mut squares = [SquareSum::constant::<0>(); 3];
    for i in 0..3 {
        squares[i] = match t.square {
            SquareTable::Magnitude(table) => {
                let u = magnitude(f, v[i])?;
                let a = f.slice::<7, 0, false, 7>(u)?;
                let b = f.slice::<7, 0, false, 0>(u)?;
                let base = f.read(table.indexed(a))?;
                let a = f.resize_exact::<8, 0, false>(a)?;
                let twice = f.shift_left_const::<1, 8, 0, false>(a)?;
                let slope = f.add_same(twice, Fixed::<8, 0, false>::constant::<1>())?;
                let correction: Fixed<15, 0, false> = f.product(slope, b)?;
                let base = f.shift_left_const::<14, 30, 0, false>(f.resize_exact(base)?)?;
                let correction =
                    f.shift_left_const::<7, 30, 0, false>(f.resize_exact(correction)?)?;
                f.binary_scale(f.add_same(base, correction)?)?
            }
            SquareTable::Signed(table) => {
                let a = f.slice::<8, 0, false, 7>(v[i])?;
                let b = f.slice::<7, 0, false, 0>(v[i])?;
                let base = f.read(table.indexed(a))?;
                let signed_a = f.slice::<8, 0, true, 0>(a)?;
                let twice = f.shift_left_const::<1, 9, 0, true>(f.resize_exact(signed_a)?)?;
                let slope = f.add_same(twice, Fixed::<9, 0, true>::constant::<1>())?;
                let tail: Fixed<8, 0, true> = f.resize_exact(b)?;
                let correction: Fixed<17, 0, true> = f.product(slope, tail)?;
                let base = f.shift_left_const::<14, 30, 0, false>(f.resize_exact(base)?)?;
                let correction =
                    f.shift_left_const::<7, 30, 0, true>(f.resize_exact(correction)?)?;
                f.binary_scale(f.add::<30, 0, false>(base, correction)?)?
            }
        };
    }
    let q = f.add_same(f.add_same(squares[0], squares[1])?, squares[2])?;
    f.publish(&format!("{prefix}.q"), q)?;
    let zeros = f.leading_zeros(q)?;
    let exponent = f.sub_same(Fixed::<18, 0, true>::constant::<1>(), zeros)?;
    let align = f.sub_same(zeros, Fixed::<18, 0, true>::constant::<15>())?;
    let mantissa = f.shift(q, align)?;
    let segment = f.slice::<6, 0, false, 8>(mantissa)?;
    let fraction = f.slice::<8, 8, false, 0>(mantissa)?;
    let parity = f.slice::<1, 0, false, 0>(exponent)?;
    let page = f.shift_left_const::<6, 7, 0, false>(f.resize_exact(parity)?)?;
    let address = f.add_same(page, f.resize_exact(segment)?)?;
    let entry = f.read(t.rsqrt.indexed(address))?;
    let base = f.slice::<16, 15, false, 0>(entry)?;
    let delta = f.slice::<8, 15, false, 16>(entry)?;
    let correction: Fixed<16, 23, false> = f.product(delta, fraction)?;
    let r0 = f.sub_same(base, f.round_to(correction)?)?;
    let half_exp = f.shift(exponent, Fixed::<18, 0, true>::constant::<-1>())?;
    let restore = f.sub_same(Fixed::<18, 0, true>::constant::<0>(), half_exp)?;
    let r: Reciprocal = f.shift(f.resize_exact(r0)?, restore)?;
    f.publish(&format!("{prefix}.r"), r)?;
    for (i, value) in v.iter_mut().enumerate() {
        let product: Fixed<33, 29, true> = f.product(*value, r)?;
        let rounded = f.round_to::<18, 14, true>(product)?;
        let clamped = clamp(
            f,
            rounded,
            Fixed::<18, 14, true>::constant::<-16384>(),
            Fixed::<18, 14, true>::constant::<16384>(),
        )?;
        *value = f.select(zero, Direction::constant::<0>(), f.resize_exact(clamped)?)?;
        f.publish(&format!("{prefix}.{i}"), *value)?;
    }
    Ok(v)
}

fn dot(f: &Frame<'_>, a: [Direction; 3], b: [Direction; 3]) -> Result<Dot, Fault> {
    let p: [Fixed<32, 28, true>; 3] = [
        f.product(a[0], b[0])?,
        f.product(a[1], b[1])?,
        f.product(a[2], b[2])?,
    ];
    let xy: Dot = f.add(p[0], p[1])?;
    f.add(xy, p[2])
}

fn power(
    f: &Frame<'_>,
    x: Specular,
    code: Fixed<5, 0, false>,
    t: &Tables,
    floor: bool,
) -> Result<Specular, Fault> {
    f.branch_value(
        f.less(x, Specular::constant::<32768>())?,
        |f| {
            let context = f.read(t.context.indexed(code))?;
            let boundary = f.slice::<15, 0, false, 0>(context)?;
            let wide = f.slice::<4, 0, false, 15>(context)?;
            let fine = f.slice::<4, 0, false, 19>(context)?;
            let base_w = f.slice::<10, 0, false, 23>(context)?;
            let base_f = f.slice::<10, 0, false, 33>(context)?;
            let raw = f.binary_scale::<16, 0, false>(x)?;
            let coarse = f.less(raw, boundary)?;
            let shift: Fixed<18, 0, true> = f.resize_exact(f.select(coarse, wide, fine)?)?;
            let base = f.select(coarse, base_w, base_f)?;
            let neg = f.sub_same(Fixed::<18, 0, true>::constant::<0>(), shift)?;
            let index = f.shift(raw, neg)?;
            let offset = f.add::<17, 0, false>(base, index)?;
            let address = f.slice::<10, 0, false, 0>(offset)?;
            let aligned = f.shift(index, shift)?;
            let tail = f.sub_same(raw, aligned)?;
            let tail = f.resize_exact::<12, 0, false>(tail)?;
            let entry = f.read(t.power.indexed(address))?;
            let left = f.slice::<16, 0, false, 0>(entry)?;
            let delta = f.slice::<12, 0, false, 16>(entry)?;
            let product: Fixed<24, 0, false> = f.product(delta, tail)?;
            let padded = f.shift_left_const::<12, 36, 0, false>(f.resize_exact(product)?)?;
            let padded = f.binary_scale::<36, 12, false>(f.shift(padded, neg)?)?;
            let correction = if floor {
                // Preserve every nonfractional source bit before checked narrowing.
                f.resize_exact(f.slice::<24, 0, false, 12>(padded)?)?
            } else {
                f.round_to(padded)?
            };
            let result = f.add_same(left, correction)?;
            f.binary_scale(result)
        },
        |_| Ok(Specular::constant::<32768>()),
    )
}

struct Inputs {
    pixel: Memory<36, 0, false>,
    light: Memory<16, 14, true>,
    projection: Memory<16, 14, true>,
    intensities: Memory<9, 8, false>,
    mode: Memory<2, 0, false>,
    code: Memory<5, 0, false>,
}
fn read3(f: &Frame<'_>, m: Memory<16, 14, true>) -> Result<[Direction; 3], Fault> {
    Ok([
        f.read(m.at::<0>())?,
        f.read(m.at::<1>())?,
        f.read(m.at::<2>())?,
    ])
}
fn outputs(f: &Frame<'_>, g: Intensity, h: Intensity) -> Result<(), Fault> {
    f.publish("g", g)?;
    f.publish("h", h)
}
fn kernel(f: &Frame<'_>, input: &Inputs, t: &Tables, config: Config) -> Result<(), Fault> {
    let mode = f.read(input.mode.at::<0>())?;
    f.branch(
        f.less(mode, Fixed::<2, 0, false>::constant::<1>())?,
        |f| outputs(f, Intensity::constant::<256>(), Intensity::constant::<0>()),
        |f| {
            let ia = f.read(input.intensities.at::<0>())?;
            f.branch(
                f.less(mode, Fixed::<2, 0, false>::constant::<2>())?,
                |f| outputs(f, ia, Intensity::constant::<0>()),
                |f| {
                    let id = f.read(input.intensities.at::<1>())?;
                    let row0 = f.read(input.pixel.at::<0>())?;
                    let row1 = f.read(input.pixel.at::<1>())?;
                    let normal = [
                        f.slice::<16, 14, true, 0>(row0)?,
                        f.slice::<16, 14, true, 16>(row0)?,
                        f.slice::<16, 14, true, 0>(row1)?,
                    ];
                    let n = normalize(f, normal, Magnitude::constant::<4>(), false, t, "n")?;
                    let l = read3(f, input.light)?;
                    let nl = dot(f, n, l)?;
                    f.publish("nl", nl)?;
                    let d = clamp(f, nl, Dot::constant::<0>(), Dot::constant::<268435456>())?;
                    let d: Intensity = f.round_to(d)?;
                    f.publish("d", d)?;
                    let product: Fixed<18, 16, false> = f.product(id, d)?;
                    let diffuse: Intensity = f.round_to(product)?;
                    let sum: IntensitySum = f.add(ia, diffuse)?;
                    let sum = clamp(
                        f,
                        sum,
                        IntensitySum::constant::<0>(),
                        IntensitySum::constant::<511>(),
                    )?;
                    let g: Intensity = f.resize_exact(sum)?;
                    f.branch(
                        f.less(mode, Fixed::<2, 0, false>::constant::<3>())?,
                        |f| outputs(f, g, Intensity::constant::<0>()),
                        |f| {
                            let x: Ndc = f.slice::<18, 16, true, 16>(row1)?;
                            let y: Ndc =
                                f.slice::<18, 16, true, 0>(f.read(input.pixel.at::<2>())?)?;
                            let px: Fixed<34, 30, true> =
                                f.product(x, f.read(input.projection.at::<0>())?)?;
                            let py: Fixed<34, 30, true> =
                                f.product(y, f.read(input.projection.at::<1>())?)?;
                            let ray = [
                                f.round_to(px)?,
                                f.round_to(py)?,
                                f.read(input.projection.at::<2>())?,
                            ];
                            for (i, value) in ray.iter().enumerate() {
                                f.publish(&format!("ray.{i}"), *value)?;
                            }
                            let v = normalize(f, ray, Magnitude::constant::<4>(), true, t, "v")?;
                            let mut half = [Direction::constant::<0>(); 3];
                            for i in 0..3 {
                                let sum: HalfSum = f.add(l[i], v[i])?;
                                let sum = f.binary_scale::<17, 15, true>(sum)?;
                                half[i] = f.round_to(sum)?;
                            }
                            let h = normalize(f, half, Magnitude::constant::<64>(), false, t, "h")?;
                            let nh = dot(f, n, h)?;
                            f.publish("nh", nh)?;
                            let nh =
                                clamp(f, nh, Dot::constant::<0>(), Dot::constant::<268435456>())?;
                            let x: Specular = f.round_to(nh)?;
                            f.publish("x", x)?;
                            let p =
                                power(f, x, f.read(input.code.at::<0>())?, t, config.power_floor)?;
                            f.publish("power", p)?;
                            let p9: Intensity = f.round_to(p)?;
                            let p9 = f.select(
                                f.less(Dot::constant::<0>(), nl)?,
                                p9,
                                Intensity::constant::<0>(),
                            )?;
                            f.publish("p9", p9)?;
                            let product: Fixed<18, 16, false> = f.product(id, p9)?;
                            outputs(f, g, f.round_to(product)?)
                        },
                    )
                },
            )
        },
    )
}

pub fn evaluate(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    max_events: usize,
) -> Result<Report, Error> {
    evaluate_with_config(
        pixel,
        material,
        light,
        projection,
        max_events,
        Config::default(),
    )
}

pub fn evaluate_with_config(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    max_events: usize,
    config: Config,
) -> Result<Report, Error> {
    validate(pixel, material, light, projection)?;
    let mut model = Model::numerical();
    let mode = if material.unlit {
        0
    } else if light.directional == 0 {
        1
    } else if material.specular_color == [0; 3] {
        2
    } else {
        3
    };
    let input = Inputs {
        pixel: model.input("pixel.rows", &PixelRows::encode(pixel)?.0.map(i128::from))?,
        light: model.input("context.light", &light.direction.map(i128::from))?,
        projection: model.input(
            "context.projection",
            &[
                i128::from(projection.ray_scale[0]),
                i128::from(projection.ray_scale[1]),
                i128::from(projection.k),
            ],
        )?,
        intensities: model.input(
            "context.intensity",
            &[i128::from(light.ambient), i128::from(light.directional)],
        )?,
        mode: model.input("context.mode", &[mode])?,
        code: model.input("context.shininess", &[i128::from(material.shininess_code)])?,
    };
    let t = Tables {
        square: if config.signed_square {
            SquareTable::Signed(model.table("SQ", &SQUARE_SIGNED)?)
        } else {
            SquareTable::Magnitude(model.table("SQ", &SQUARE)?)
        },
        rsqrt: model.table("RSQRT", &RSQRT)?,
        power: model.table("POWER", &POWER)?,
        context: model.table("POWER_CONTEXT", &CONTEXT)?,
    };
    let f = model.compute("pixel_lighting", max_events)?;
    kernel(&f, &input, &t, config)?;
    let frame = f.finish();
    frame.audit()?;
    if !frame.valid {
        return Err(Error::Audit(frame.faults[0].clone()));
    }
    let get = |name| {
        frame
            .outputs
            .iter()
            .find(|v| v.name == name)
            .expect("kernel output")
            .raw as u16
    };
    Ok(Report {
        output: LightingOutput {
            g: get("g"),
            h: get("h"),
        },
        frame,
    })
}
