//! Closed pixel lighting: all data operations and reads go through audited Frame.
use super::super::LightingQuantization;
use super::super::{format::*, ports::*};
use super::quantization::Arithmetic;
use audited::{Fault, Fixed, FrameReport, Memory, Model};

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

#[derive(Clone)]
pub struct HalfPreparation {
    ray: FrameReport,
    half: FrameReport,
    ndc: [i32; 2],
    light: [i16; 3],
    projection: Projection,
}
impl HalfPreparation {
    pub fn ray_frame(&self) -> &FrameReport {
        &self.ray
    }
    pub fn half_frame(&self) -> &FrameReport {
        &self.half
    }
}
#[derive(Clone)]
pub struct FlatPreparation {
    frame: FrameReport,
    normal: [i16; 3],
    light: Light,
}
impl FlatPreparation {
    pub fn frame(&self) -> &FrameReport {
        &self.frame
    }
}
pub struct Report {
    pub output: LightingOutput,
    pub frame: FrameReport,
    /// Separate upstream NDC-ray preparation, when the prepared input profile is used.
    pub ray_preparation: Option<FrameReport>,
    /// Material update work; amortized once per context rather than per pixel.
    pub context_preparation: Option<FrameReport>,
    /// Optional exact shared screen-coordinate/view-light work.
    pub half_preparation: Option<HalfPreparation>,
    pub flat_preparation: Option<FlatPreparation>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub quantization: LightingQuantization,
    /// Explicit compact two-row S12F10 transport; Q14 working values are wiring.
    /// This format is currently a counted/oracle boundary, not a cycle profile.
    pub compact_normal: bool,
    /// Exact S13 prescale before Q14 expansion; requires compact_normal.
    /// Keeps the original compact graph selectable as a comparison baseline.
    pub compact_prescale: bool,
    /// Use signed quadratic chords to avoid the second abs in each normalize.
    pub signed_square: bool,
    /// Truncate only the nonnegative power interpolation correction.
    pub power_floor: bool,
    /// Exploit validated view bounds and latch material lookup fields before pixels.
    pub dataflow: bool,
    /// Consume a separately prepared Q14 ray in three 36-bit pixel rows.
    pub prepared_ray: bool,
    /// Cache H by screen coordinate and projection/light context.
    pub shared_half: bool,
    /// Reuse exact flat-triangle N/NL/d/g; source normal and light must stay fixed.
    pub flat_normal: bool,
    /// Normalize NH with one scalar H reciprocal instead of three H components.
    pub scalar_norm: bool,
    /// Explicit system candidate: apply N reciprocal to scalar dots, gate before RNE.
    pub scalar_normal: bool,
    /// Recover the exact input NL sign from three low bits per component.
    pub exact_normal_gate: bool,
    /// Use bounded block-floating prescale (two fixed right-RNE cases).
    pub block_prescale: bool,
    /// Replace H SQ chords with DSP squares; N and V remain numerically frozen.
    pub direct_square: bool,
    /// Explicit system candidate: direct DSP squares for all N/V/H lengths.
    pub direct_all_squares: bool,
    /// Round N/H magnitudes to U9 Q8 and square in DSP9. V stays DSP18.
    pub square9: bool,
    /// Permanently separate NL and NH DSP dot lanes.
    pub dedicated_dots: bool,
}
impl Config {
    /// Closed resource-profile contract with ROM midpoint and narrow final RNE.
    pub fn compensated_resource_profile(profile: super::super::LightingProfile) -> Self {
        Self {
            quantization: LightingQuantization::CompensatedFloor,
            ..Self::resource_profile(profile)
        }
    }
    /// Exact narrow-normal candidate; all SQ/RSQRT/dot/power semantics remain.
    pub fn compact() -> Self {
        Self {
            compact_normal: true,
            compact_prescale: true,
            ..Self::architecture()
        }
    }
    pub fn optimized() -> Self {
        Self {
            signed_square: true,
            power_floor: true,
            ..Self::default()
        }
    }
}
impl Config {
    pub fn architecture() -> Self {
        Self {
            dataflow: true,
            ..Self::optimized()
        }
    }
    pub fn scalar() -> Self {
        Self {
            scalar_norm: true,
            ..Self::architecture()
        }
    }
    pub fn scalar_pipeline() -> Self {
        Self {
            scalar_norm: true,
            block_prescale: true,
            ..Self::architecture()
        }
    }
    pub fn system_candidate(direct_all_squares: bool) -> Self {
        Self {
            scalar_norm: true,
            scalar_normal: true,
            block_prescale: true,
            direct_all_squares,
            ..Self::architecture()
        }
    }
    pub fn system_profile() -> Self {
        Self {
            exact_normal_gate: true,
            ..Self::system_candidate(true)
        }
    }
    /// Selected resource alternative; N/NL and V retain the baseline contract.
    pub fn resource_profile(profile: super::super::LightingProfile) -> Self {
        Self {
            scalar_norm: true,
            block_prescale: profile == super::super::LightingProfile::Compact,
            dedicated_dots: profile == super::super::LightingProfile::Fast,
            ..Self::architecture()
        }
    }
    pub fn prepared() -> Self {
        Self {
            prepared_ray: true,
            ..Self::architecture()
        }
    }
}
#[derive(Clone, Copy)]
enum SquareTable {
    Exact,
    Magnitude9,
    Magnitude(Memory<14, 0, false>),
    Signed(Memory<15, 0, false>),
}
struct Tables {
    direct_square: bool,
    square: SquareTable,
    rsqrt: Memory<24, 0, false>,
    power: Memory<28, 0, false>,
    context: Memory<43, 0, false>,
    context_is_latched: bool,
    static_power: bool,
}

// Source-level DAG names are metadata: no new operations, reads or publications.
fn named<const B: u32, const F: u32, const S: bool>(
    f: &Arithmetic<'_, '_>,
    name: &str,
    value: Fixed<B, F, S>,
) -> Result<Fixed<B, F, S>, Fault> {
    f.name_value(name, value)?;
    Ok(value)
}
fn name_vector(f: &Arithmetic<'_, '_>, prefix: &str, values: [Direction; 3]) -> Result<(), Fault> {
    for (axis, value) in ["x", "y", "z"].iter().zip(values) {
        f.name_value(&format!("{prefix}.{axis}"), value)?;
    }
    Ok(())
}

fn clamp<const B: u32, const F: u32, const S: bool>(
    f: &Arithmetic<'_, '_>,
    x: Fixed<B, F, S>,
    lo: Fixed<B, F, S>,
    hi: Fixed<B, F, S>,
) -> Result<Fixed<B, F, S>, Fault> {
    let x = f.select(f.less(x, lo)?, lo, x)?;
    f.select(f.less(hi, x)?, hi, x)
}
fn magnitude(f: &Arithmetic<'_, '_>, x: Direction) -> Result<Magnitude, Fault> {
    // Extend before negation, including the signed minimum.
    let wide = f.resize_exact::<17, 14, true>(x)?;
    let zero = Fixed::<17, 14, true>::constant::<0>();
    let neg = f.sub_same(zero, wide)?;
    f.resize_exact(f.select(f.less(wide, zero)?, neg, wide)?)
}
fn shifted(
    f: &Arithmetic<'_, '_>,
    x: Direction,
    amount: Fixed<18, 0, true>,
) -> Result<Direction, Fault> {
    // Keep all possible discarded bits until the fixed-format RNE operation.
    let x = f.resize_exact::<32, 14, true>(x)?;
    let x = f.shift_left_const::<14, 32, 14, true>(x)?;
    let x: NormalizedWork = f.binary_scale(x)?;
    f.round_to(f.shift(x, amount)?)
}
fn block_shifted(
    f: &Arithmetic<'_, '_>,
    value: Direction,
    amount: Fixed<18, 0, true>,
) -> Result<Direction, Fault> {
    let negative = f.less(amount, Fixed::<18, 0, true>::constant::<0>())?;
    let left = f.select(negative, Fixed::<18, 0, true>::constant::<0>(), amount)?;
    let left = f.resize_exact::<4, 0, false>(left)?;
    let left = f.shift(value, f.resize_exact::<18, 0, true>(left)?)?;
    // A 16-bit normal needs at most a two-bit downscale. Half-vector inputs
    // need at most one. Keep two static signed RNE circuits instead of a
    // wide barrel shift followed by a variable sticky-bit reduction.
    let one = f.round_to::<16, 14, true>(f.binary_scale::<16, 15, true>(value)?)?;
    let two = f.round_to::<16, 14, true>(f.binary_scale::<16, 16, true>(value)?)?;
    let right = f.select(
        f.less(amount, Fixed::<18, 0, true>::constant::<-1>())?,
        two,
        one,
    )?;
    f.select(negative, right, left)
}

#[derive(Clone, Copy)]
struct Normalization {
    scaled: [Direction; 3],
    reciprocal: Reciprocal,
    zero: Fixed<1, 0, false>,
    amount: Fixed<18, 0, true>,
}

/// Local sign/magnitude view of a compact normal, never a stored numeric ABI.
/// The negative-zero encoding is absent: signs are extracted from signed codes.
#[derive(Clone, Copy)]
struct CompactMagnitude {
    maximum: Magnitude,
    codes: [NormalInput; 3],
    maximum_code: Fixed<12, 10, false>,
    zero: Fixed<1, 0, false>,
}

fn compact_normal(
    f: &Arithmetic<'_, '_>,
    row: PixelWord,
) -> Result<([Direction; 3], CompactMagnitude), Fault> {
    let codes: [NormalInput; 3] = [
        f.slice::<12, 10, true, 0>(row)?,
        f.slice::<12, 10, true, 12>(row)?,
        f.slice::<12, 10, true, 24>(row)?,
    ];
    let mut raw = [Direction::constant::<0>(); 3];
    let mut mags = [Fixed::<12, 10, false>::constant::<0>(); 3];
    for i in 0..3 {
        let axis = ["x", "y", "z"][i];
        let sign = f.slice::<1, 0, false, 11>(codes[i])?;
        f.name_value(&format!("normal.compact.sign.{axis}"), sign)?;
        // Widen before negation: abs(-2048) is +2048, representable in U12.
        let wide = f.resize_exact::<13, 10, true>(codes[i])?;
        let neg = f.sub_same(Fixed::<13, 10, true>::constant::<0>(), wide)?;
        mags[i] = f.resize_exact(f.select(sign, neg, wide)?)?;
        f.name_value(&format!("normal.compact.magnitude.{axis}"), mags[i])?;
        let mag = f.resize_exact::<17, 10, false>(mags[i])?;
        let mag: Magnitude = f.binary_scale(f.shift_left_const::<4, 17, 10, false>(mag)?)?;
        f.name_value(&format!("normal.abs.{axis}"), mag)?;
        let code = f.resize_exact::<16, 10, true>(codes[i])?;
        raw[i] = f.binary_scale(f.shift_left_const::<4, 16, 10, true>(code)?)?;
    }
    let mut maximum = mags[0];
    for &mag in &mags[1..] {
        maximum = f.select(f.less(maximum, mag)?, mag, maximum)?;
    }
    f.name_value("normal.compact.maximum", maximum)?;
    // Q14 expansion appends four zeros: max < 4 iff all F10 codes are zero.
    let zero = f.less(maximum, Fixed::<12, 10, false>::constant::<1>())?;
    let maximum_code = maximum;
    let maximum = f.resize_exact::<17, 10, false>(maximum)?;
    let maximum = f.binary_scale(f.shift_left_const::<4, 17, 10, false>(maximum)?)?;
    Ok((
        raw,
        CompactMagnitude {
            maximum,
            maximum_code,
            codes,
            zero,
        },
    ))
}

#[derive(Clone, Copy)]
struct NormalizationPolicy {
    fast: bool,
    block: bool,
    compact: Option<CompactMagnitude>,
    compact_prescale: bool,
}
impl From<(bool, bool)> for NormalizationPolicy {
    fn from((fast, block): (bool, bool)) -> Self {
        Self {
            fast,
            block,
            compact: None,
            compact_prescale: false,
        }
    }
}

fn prepare_normalization(
    f: &Arithmetic<'_, '_>,
    raw: [Direction; 3],
    threshold: Magnitude,
    bounded: bool,
    t: &Tables,
    prefix: &str,
    policy: NormalizationPolicy,
) -> Result<Normalization, Fault> {
    let NormalizationPolicy {
        fast,
        block,
        compact,
        compact_prescale,
    } = policy;
    let stage = match prefix {
        "n" => "normal",
        "v" => "view",
        "h" => "halfway",
        _ => prefix,
    };
    // Vz >= 8192 is established at the external input boundary. The validated
    // bounded view ray cannot degenerate and has no common pre-shift.
    let (v, zero, amount) = if compact_prescale {
        let compact = compact.ok_or(Fault::Format)?;
        // max_code==0 uses the same 0.5 fallback as the original Q14 kernel.
        // For every nonzero maximum m, lz12(m) is 0..11 and
        // abs(code << lz12(m)) <= 4095. A signed13 barrel shift is sufficient.
        // Q14 prescale amount = lz12(m)-2; appending two zeros gives exactly
        // (code << 4) << amount, including negative values and -2048.
        let safe_max = f.select(
            compact.zero,
            Fixed::<12, 10, false>::constant::<512>(),
            compact.maximum_code,
        )?;
        let zeros = f.resize_exact::<4, 0, false>(f.leading_zeros(safe_max)?)?;
        let shift = f.resize_exact::<18, 0, true>(zeros)?;
        let amount = f.resize_exact::<6, 0, true>(zeros)?;
        let amount = f.resize_exact(f.sub_same(amount, Fixed::<6, 0, true>::constant::<2>())?)?;
        let mut scaled = [Direction::constant::<0>(); 3];
        for (i, value) in scaled.iter_mut().enumerate() {
            let fallback = if i == 0 {
                NormalInput::constant::<512>()
            } else {
                NormalInput::constant::<0>()
            };
            let code = f.select(compact.zero, fallback, compact.codes[i])?;
            let code = f.resize_exact::<13, 10, true>(code)?;
            let code = f.shift(code, shift)?;
            f.name_value(
                &format!("normal.compact.prescaled.{}", ["x", "y", "z"][i]),
                code,
            )?;
            let code = f.resize_exact::<16, 10, true>(code)?;
            *value = f.binary_scale(f.shift_left_const::<2, 16, 10, true>(code)?)?;
        }
        (scaled, compact.zero, amount)
    } else if bounded && fast {
        (
            raw,
            Fixed::<1, 0, false>::constant::<0>(),
            Fixed::<18, 0, true>::constant::<0>(),
        )
    } else {
        let mut m = if let Some(compact) = compact {
            compact.maximum
        } else {
            let mags = [
                magnitude(f, raw[0])?,
                magnitude(f, raw[1])?,
                magnitude(f, raw[2])?,
            ];
            for (axis, value) in ["x", "y", "z"].iter().zip(mags) {
                f.name_value(&format!("{stage}.abs.{axis}"), value)?;
            }
            let mut maximum = mags[0];
            for v in &mags[1..] {
                maximum = f.select(f.less(maximum, *v)?, *v, maximum)?;
            }
            maximum
        };
        f.name_value(&format!("{stage}.max_magnitude"), m)?;
        let zero = if let Some(compact) = compact {
            compact.zero
        } else {
            f.less(m, threshold)?
        };
        let zero = named(f, &format!("{stage}.degenerate"), zero)?;
        let safe = [
            Direction::constant::<8192>(),
            Direction::constant::<0>(),
            Direction::constant::<0>(),
        ];
        let mut v = raw;
        for i in 0..3 {
            v[i] = f.select(zero, safe[i], v[i])?;
        }
        m = named(
            f,
            &format!("{stage}.safe_magnitude"),
            f.select(zero, Magnitude::constant::<8192>(), m)?,
        )?;
        let mut amount = if bounded {
            Fixed::<18, 0, true>::constant::<0>()
        } else {
            if fast {
                let zeros = f.resize_exact::<6, 0, true>(f.leading_zeros(m)?)?;
                f.resize_exact(f.sub_same(zeros, Fixed::<6, 0, true>::constant::<3>())?)?
            } else {
                f.sub_same(f.leading_zeros(m)?, Fixed::<18, 0, true>::constant::<3>())?
            }
        };
        f.name_value(&format!("{stage}.prescale_shift"), amount)?;
        // Expanded F10 values have four zero low bits. The common shift is
        // >= -2, so right scaling is exact; max=32767 and its RNE-overflow
        // case cannot occur. Neither guard is needed for compact normals.
        if compact.is_none() && block && !bounded && prefix == "n" {
            // In the entire i16 input domain, common prescale RNE overflows
            // 16383 only at max-magnitude 32767. Handle that single exponent
            // boundary directly, without scaling and rounding the maximum.
            let below = f.less(m, Magnitude::constant::<32767>())?;
            let above = f.less(Magnitude::constant::<32767>(), m)?;
            amount = f.select(
                below,
                amount,
                f.select(above, amount, Fixed::<18, 0, true>::constant::<-2>())?,
            )?;
        } else if compact.is_none() && !bounded && !(fast && prefix == "h") {
            // RNE can turn 32767/2 into 16384. Keep the SQ index strictly below 128.
            let mw = f.resize_exact::<32, 14, true>(m)?;
            let mw = f.binary_scale::<32, 28, true>(f.shift_left_const::<14, 32, 14, true>(mw)?)?;
            f.name_value(&format!("{stage}.magnitude_work"), mw)?;
            let shifted_max = named(
                f,
                &format!("{stage}.scaled_magnitude_work"),
                f.shift(mw, amount)?,
            )?;
            let rounded: Magnitude = f.round_to(shifted_max)?;
            let below = f.less(rounded, Magnitude::constant::<16384>())?;
            amount = f.select(
                below,
                amount,
                f.sub_same(amount, Fixed::<18, 0, true>::constant::<1>())?,
            )?;
        }

        for value in &mut v {
            *value = if block {
                block_shifted(f, *value, amount)?
            } else {
                shifted(f, *value, amount)?
            };
        }
        (v, zero, amount)
    };
    name_vector(f, &format!("{stage}.scaled"), v)?;
    f.name_value(&format!("{stage}.scale_shift"), amount)?;
    f.publish(&format!("{prefix}.shift"), amount)?;
    let mut squares = [SquareSum::constant::<0>(); 3];
    for i in 0..3 {
        let square_table = if (t.direct_square && prefix == "h")
            || (matches!(t.square, SquareTable::Magnitude9) && prefix == "v")
        {
            SquareTable::Exact
        } else {
            t.square
        };
        let axis = ["x", "y", "z"][i];
        let square_name = format!("{stage}.square.{axis}");
        squares[i] = match square_table {
            SquareTable::Magnitude9 => {
                let u = magnitude(f, v[i])?;
                let u = f.round_to::<9, 8, false>(u)?;
                let product: Fixed<18, 16, false> = f.product(u, u)?;
                f.binary_scale(f.shift_left_const::<12, 30, 16, false>(f.resize_exact(product)?)?)?
            }
            SquareTable::Exact => {
                let product: Fixed<32, 28, true> = f.product(v[i], v[i])?;
                f.resize_exact(product)?
            }
            SquareTable::Magnitude(table) => {
                let u = magnitude(f, v[i])?;
                let a = f.slice::<7, 0, false, 7>(u)?;
                let b = f.slice::<7, 0, false, 0>(u)?;
                let base = named(
                    f,
                    &format!("{square_name}.lut_base"),
                    f.read(table.indexed(a))?,
                )?;
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
                let base = named(
                    f,
                    &format!("{square_name}.lut_base"),
                    f.read(table.indexed(a))?,
                )?;
                let signed_a = f.slice::<8, 0, true, 0>(a)?;
                let twice = f.shift_left_const::<1, 9, 0, true>(f.resize_exact(signed_a)?)?;
                let slope = f.add_same(twice, Fixed::<9, 0, true>::constant::<1>())?;
                let tail: Fixed<8, 0, true> = f.resize_exact(b)?;
                f.name_value(&format!("{square_name}.slope"), slope)?;
                f.name_value(&format!("{square_name}.tail"), tail)?;
                let correction: Fixed<17, 0, true> = named(
                    f,
                    &format!("{square_name}.correction"),
                    f.product(slope, tail)?,
                )?;
                let base = f.shift_left_const::<14, 30, 0, false>(f.resize_exact(base)?)?;
                let correction =
                    f.shift_left_const::<7, 30, 0, true>(f.resize_exact(correction)?)?;
                f.binary_scale(f.add::<30, 0, false>(base, correction)?)?
            }
        };
        f.name_value(&square_name, squares[i])?;
    }
    let (q, xy) = super::pipeline::square_sum(f, squares)?;
    f.name_value(&format!("{stage}.square.xy_sum"), xy)?;
    f.name_value(&format!("{stage}.length_squared"), q)?;
    f.publish(&format!("{prefix}.q"), q)?;
    let (exponent, align, mantissa, segment, fraction, address, fast_restore) = if fast {
        let head = super::pipeline::inverse_head(f, q)?;
        f.name_value(&format!("{stage}.leading_zeros"), head.zeros)?;
        (
            head.exponent,
            head.align,
            head.mantissa,
            head.segment,
            head.fraction,
            head.address,
            Some(f.resize_exact::<18, 0, true>(head.restore)?),
        )
    } else {
        let zeros = named(f, &format!("{stage}.leading_zeros"), f.leading_zeros(q)?)?;
        let exponent = f.sub_same(Fixed::<18, 0, true>::constant::<1>(), zeros)?;
        let align = f.sub_same(zeros, Fixed::<18, 0, true>::constant::<15>())?;
        let mantissa = f.shift(q, align)?;
        let segment = f.slice::<6, 0, false, 8>(mantissa)?;
        let fraction = f.slice::<8, 8, false, 0>(mantissa)?;
        let parity = f.slice::<1, 0, false, 0>(exponent)?;
        let page = f.shift_left_const::<6, 7, 0, false>(f.resize_exact(parity)?)?;
        let address = f.add_same(page, f.resize_exact(segment)?)?;
        (exponent, align, mantissa, segment, fraction, address, None)
    };
    f.name_value(&format!("{stage}.length_mantissa"), mantissa)?;
    f.name_value(&format!("{stage}.rsqrt_segment"), segment)?;
    f.name_value(&format!("{stage}.rsqrt_fraction"), fraction)?;
    f.name_value(&format!("{stage}.length_exponent"), exponent)?;
    f.name_value(&format!("{stage}.mantissa_shift"), align)?;
    f.name_value(&format!("{stage}.rsqrt_address"), address)?;
    let entry = f.read(t.rsqrt.indexed(address))?;
    f.name_value(&format!("{stage}.rsqrt_entry"), entry)?;
    let base = f.slice::<16, 15, false, 0>(entry)?;
    let delta = f.slice::<8, 15, false, 16>(entry)?;
    let correction: Fixed<16, 23, false> = f.product(delta, fraction)?;
    f.name_value(&format!("{stage}.rsqrt_correction"), correction)?;
    let restore = if let Some(restore) = fast_restore {
        restore
    } else {
        let half_exp = f.shift(exponent, Fixed::<18, 0, true>::constant::<-1>())?;
        f.sub_same(Fixed::<18, 0, true>::constant::<0>(), half_exp)?
    };
    f.name_value(&format!("{stage}.rsqrt_base"), base)?;
    f.name_value(&format!("{stage}.rsqrt_delta"), delta)?;
    f.name_value(&format!("{stage}.restore_shift"), restore)?;
    let (r, r0) = super::pipeline::inverse_tail_with_floor(
        f,
        base,
        correction,
        restore,
        f.floor_intermediates(),
    )?;
    f.name_value(&format!("{stage}.rsqrt_interpolated"), r0)?;
    f.name_value(&format!("{stage}.inverse_length"), r)?;
    f.publish(&format!("{prefix}.r"), r)?;
    Ok(Normalization {
        scaled: v,
        reciprocal: r,
        zero,
        amount,
    })
}

fn normalize(
    f: &Arithmetic<'_, '_>,
    raw: [Direction; 3],
    threshold: Magnitude,
    bounded: bool,
    t: &Tables,
    prefix: &str,
    policy: NormalizationPolicy,
) -> Result<[Direction; 3], Fault> {
    let fast = policy.fast;
    let stage = match prefix {
        "n" => "normal",
        "v" => "view",
        "h" => "halfway",
        _ => prefix,
    };
    let factors = prepare_normalization(f, raw, threshold, bounded, t, prefix, policy)?;
    let Normalization {
        scaled: mut v,
        reciprocal: r,
        zero,
        ..
    } = factors;
    for (i, value) in v.iter_mut().enumerate() {
        let product: Fixed<33, 29, true> = f.product(*value, r)?;
        f.name_value(
            &format!("{stage}.normalized_product.{}", ["x", "y", "z"][i]),
            product,
        )?;
        let (result, rounded) = super::pipeline::normalized_output_with_floor(
            f,
            product,
            if bounded && fast { None } else { Some(zero) },
            f.floor_intermediates(),
        )?;
        f.name_value(&format!("{stage}.rounded.{}", ["x", "y", "z"][i]), rounded)?;
        *value = result;
        f.name_value(&format!("{stage}.{}", ["x", "y", "z"][i]), *value)?;
        f.publish(&format!("{prefix}.{i}"), *value)?;
    }
    Ok(v)
}

fn dot(
    f: &Arithmetic<'_, '_>,
    a: [Direction; 3],
    b: [Direction; 3],
    prefix: &str,
) -> Result<Dot, Fault> {
    let p: [Fixed<32, 28, true>; 3] = [
        f.product(a[0], b[0])?,
        f.product(a[1], b[1])?,
        f.product(a[2], b[2])?,
    ];
    for (axis, value) in ["x", "y", "z"].iter().zip(p) {
        f.name_value(&format!("{prefix}.{axis}_product"), value)?;
    }
    let xy: Dot = named(f, &format!("{prefix}.xy_sum"), f.add(p[0], p[1])?)?;
    named(f, prefix, f.add(xy, p[2])?)
}

fn scalar_half_dot(
    f: &Arithmetic<'_, '_>,
    normal: [Direction; 3],
    half: Normalization,
    normal_factors: Option<Normalization>,
) -> Result<Fixed<18, 15, true>, Fault> {
    let raw = dot(f, normal, half.scaled, "normal_dot_halfway_scaled")?;
    f.publish("nh.raw", raw)?;
    // Unit N times pre-scaled H is bounded by sqrt(3). Keep two guard bits
    // in the DSP18 operand, then round once to the final specular coordinate.
    let narrow = if let Some(n) = normal_factors {
        // Two scaled vectors can have a raw dot up to three. Keep Q15 before
        // the N reciprocal; the resulting dot with unit N then fits Q16.
        let operand = f.round_to::<18, 15, true>(f.resize_exact::<31, 28, true>(raw)?)?;
        let product: Fixed<35, 30, true> = f.product(operand, n.reciprocal)?;
        let unit_n = f.round_to::<18, 16, true>(f.resize_exact::<32, 30, true>(product)?)?;
        f.select(n.zero, Fixed::<18, 16, true>::constant::<0>(), unit_n)?
    } else {
        f.round_to::<18, 16, true>(f.resize_exact::<30, 28, true>(raw)?)?
    };
    let product: Fixed<35, 31, true> = f.product(narrow, half.reciprocal)?;
    let value = f.round_to::<18, 15, true>(f.resize_exact::<34, 31, true>(product)?)?;
    f.select(half.zero, Fixed::<18, 15, true>::constant::<0>(), value)
}

// For a right prescale k=1/2, raw N = scaled N*2^k + residual.
// RNE residuals are in [-2,2] and depend only on the original three low bits.
// Reconstruct dot(raw N,L)'s sign without three more multipliers or retaining
// the original 48-bit vector. The large-dot guard safely narrows the correction.
fn exact_normal_gate(
    f: &Arithmetic<'_, '_>,
    low: Fixed<9, 0, false>,
    light: [Direction; 3],
    factors: Normalization,
    scaled_dot: Dot,
) -> Result<Fixed<1, 0, false>, Fault> {
    let two = f.less(factors.amount, Fixed::<18, 0, true>::constant::<-1>())?;
    let negative = f.less(factors.amount, Fixed::<18, 0, true>::constant::<0>())?;
    let mut terms = [Fixed::<18, 28, true>::constant::<0>(); 3];
    for axis in 0..3 {
        let lo = match axis {
            0 => f.slice::<3, 0, false, 0>(low)?,
            1 => f.slice::<3, 0, false, 3>(low)?,
            _ => f.slice::<3, 0, false, 6>(low)?,
        };
        let bit0 = f.slice::<1, 0, false, 0>(lo)?;
        let bit1 = f.slice::<1, 0, false, 1>(lo)?;
        let bit2 = f.slice::<1, 0, false, 2>(lo)?;
        let l = f.binary_scale::<18, 28, true>(f.resize_exact::<18, 14, true>(light[axis])?)?;
        let neg = f.sub_same(Fixed::<18, 28, true>::constant::<0>(), l)?;
        let twice = f.shift_left_const::<1, 18, 28, true>(l)?;
        let neg_twice = f.shift_left_const::<1, 18, 28, true>(neg)?;
        let one = f.select(
            bit0,
            f.select(bit1, neg, l)?,
            Fixed::<18, 28, true>::constant::<0>(),
        )?;
        let pair = f.select(bit2, neg_twice, twice)?;
        // Odd tails have the same +/-L residual for both prescale amounts.
        // Only an even two-bit tail 2 differs: it contributes +/-2L.
        let even_tail_two = f.select(bit0, Fixed::<1, 0, false>::constant::<0>(), bit1)?;
        let use_pair = f.select(two, even_tail_two, Fixed::<1, 0, false>::constant::<0>())?;
        terms[axis] = f.select(use_pair, pair, one)?;
    }
    let residual = f.add::<21, 28, true>(f.add::<20, 28, true>(terms[0], terms[1])?, terms[2])?;
    let tiny = f.slice::<18, 28, true, 0>(scaled_dot)?;
    let expanded = f.select(
        two,
        f.shift_left_const::<2, 20, 28, true>(f.resize_exact(tiny)?)?,
        f.shift_left_const::<1, 20, 28, true>(f.resize_exact(tiny)?)?,
    )?;
    let corrected = f.add::<21, 28, true>(expanded, residual)?;
    let small = f.select(
        f.less(scaled_dot, Dot::constant::<-131072>())?,
        Fixed::<1, 0, false>::constant::<0>(),
        f.less(scaled_dot, Dot::constant::<131072>())?,
    )?;
    let positive = f.less(Dot::constant::<0>(), scaled_dot)?;
    let corrected = f.select(
        small,
        f.less(Fixed::<21, 28, true>::constant::<0>(), corrected)?,
        positive,
    )?;
    let gate = f.select(negative, corrected, positive)?;
    f.select(factors.zero, Fixed::<1, 0, false>::constant::<0>(), gate)
}

fn power(
    f: &Arithmetic<'_, '_>,
    x: Specular,
    code: Fixed<5, 0, false>,
    t: &Tables,
    floor: bool,
) -> Result<Specular, Fault> {
    if t.static_power {
        // A streaming circuit evaluates both paths. Keep the endpoint out of
        // the last segment's address, then select its exact value afterwards.
        let endpoint = f.less(x, Specular::constant::<32768>())?;
        let safe = f.select(endpoint, x, Specular::constant::<32767>())?;
        let interpolated = power_interpolate(f, safe, code, t, floor)?;
        return f.select(endpoint, interpolated, Specular::constant::<32768>());
    }
    f.branch_value(
        f.less(x, Specular::constant::<32768>())?,
        |f| power_interpolate(f, x, code, t, floor),
        |_| Ok(Specular::constant::<32768>()),
    )
}

fn power_interpolate(
    f: &Arithmetic<'_, '_>,
    x: Specular,
    code: Fixed<5, 0, false>,
    t: &Tables,
    floor: bool,
) -> Result<Specular, Fault> {
    let context = if t.context_is_latched {
        f.read(t.context.at::<0>())?
    } else {
        f.read(t.context.indexed(code))?
    };
    let head = super::pipeline::power_head(f, x, context)?;
    let address = head.address;
    let tail = head.tail;
    let neg = head.neg;
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
    f.name_value("specular.table_address", address)?;
    f.name_value("specular.fraction", tail)?;
    f.name_value("specular.table_entry", entry)?;
    f.name_value("specular.base", left)?;
    f.name_value("specular.delta", delta)?;
    f.name_value("specular.correction_product", product)?;
    f.name_value("specular.correction", correction)?;
    let result = f.add_same(left, correction)?;
    f.binary_scale(result)
}

struct Inputs {
    normal_lsb: Option<Memory<9, 0, false>>,
    pixel: Memory<36, 0, false>,
    light: Memory<16, 14, true>,
    projection: Memory<16, 14, true>,
    intensities: Memory<9, 8, false>,
    mode: Memory<2, 0, false>,
    code: Memory<5, 0, false>,
    half: Option<Memory<16, 14, true>>,
    flat: Option<FlatInputs>,
}
struct FlatInputs {
    normal: Memory<16, 14, true>,
    nl: Memory<34, 28, true>,
    d: Memory<9, 8, false>,
    g: Memory<9, 8, false>,
}
fn read3(f: &Arithmetic<'_, '_>, m: Memory<16, 14, true>) -> Result<[Direction; 3], Fault> {
    Ok([
        f.read(m.at::<0>())?,
        f.read(m.at::<1>())?,
        f.read(m.at::<2>())?,
    ])
}
fn outputs(f: &Arithmetic<'_, '_>, g: Intensity, h: Intensity) -> Result<(), Fault> {
    f.publish("g", g)?;
    f.publish("h", h)
}
fn kernel(f: &Arithmetic<'_, '_>, input: &Inputs, t: &Tables, config: Config) -> Result<(), Fault> {
    let mode = f.read(input.mode.at::<0>())?;
    f.branch(
        named(
            f,
            "mode.unlit",
            f.less(mode, Fixed::<2, 0, false>::constant::<1>())?,
        )?,
        |f| outputs(f, Intensity::constant::<256>(), Intensity::constant::<0>()),
        |f| {
            let ia = f.read(input.intensities.at::<0>())?;
            f.branch(
                named(
                    f,
                    "mode.ambient_only",
                    f.less(mode, Fixed::<2, 0, false>::constant::<2>())?,
                )?,
                |f| outputs(f, ia, Intensity::constant::<0>()),
                |f| {
                    let id = f.read(input.intensities.at::<1>())?;
                    let row0 = if input.flat.is_none() {
                        Some(f.read(input.pixel.at::<0>())?)
                    } else {
                        None
                    };
                    let row1 = if config.compact_normal {
                        None
                    } else {
                        Some(f.read(input.pixel.at::<1>())?)
                    };
                    let mut normal_factors = None;
                    let mut normal_gate = None;
                    let (n, nl, g, l) = if let Some(flat) = &input.flat {
                        let n = read3(f, flat.normal)?;
                        for (i, value) in n.iter().enumerate() {
                            f.publish(&format!("n.{i}"), *value)?;
                        }
                        let nl = f.read(flat.nl.at::<0>())?;
                        f.publish("nl", nl)?;
                        f.publish("d", f.read(flat.d.at::<0>())?)?;
                        (n, nl, f.read(flat.g.at::<0>())?, read3(f, input.light)?)
                    } else {
                        let (normal, compact) = if config.compact_normal {
                            let (normal, compact) = compact_normal(f, row0.unwrap())?;
                            (normal, Some(compact))
                        } else {
                            (
                                [
                                    f.slice::<16, 14, true, 0>(row0.unwrap())?,
                                    f.slice::<16, 14, true, 16>(row0.unwrap())?,
                                    f.slice::<16, 14, true, 0>(row1.unwrap())?,
                                ],
                                None,
                            )
                        };
                        name_vector(f, "normal.input", normal)?;
                        let policy = NormalizationPolicy {
                            fast: config.dataflow,
                            block: config.block_prescale,
                            compact,
                            compact_prescale: config.compact_prescale,
                        };
                        let n = if config.scalar_normal {
                            let factors = prepare_normalization(
                                f,
                                normal,
                                Magnitude::constant::<4>(),
                                false,
                                t,
                                "n",
                                policy,
                            )?;
                            normal_factors = Some(factors);
                            factors.scaled
                        } else {
                            normalize(f, normal, Magnitude::constant::<4>(), false, t, "n", policy)?
                        };
                        let l = read3(f, input.light)?;
                        let raw = dot(f, n, l, "normal_dot_light")?;
                        let nl = if let Some(factors) = normal_factors {
                            f.publish("nl.raw", raw)?;
                            let gate = f.select(
                                factors.zero,
                                Fixed::<1, 0, false>::constant::<0>(),
                                f.less(Dot::constant::<0>(), raw)?,
                            )?;
                            let gate = if config.exact_normal_gate {
                                exact_normal_gate(
                                    f,
                                    f.read(input.normal_lsb.unwrap().at::<0>())?,
                                    l,
                                    factors,
                                    raw,
                                )?
                            } else {
                                gate
                            };
                            f.publish("nl.gate", gate)?;
                            normal_gate = Some(gate);
                            let narrow =
                                f.round_to::<18, 16, true>(f.resize_exact::<30, 28, true>(raw)?)?;
                            let product: Fixed<35, 31, true> =
                                f.product(narrow, factors.reciprocal)?;
                            let rounded = f.round_to::<18, 15, true>(
                                f.resize_exact::<34, 31, true>(product)?,
                            )?;
                            let rounded = f.select(
                                factors.zero,
                                Fixed::<18, 15, true>::constant::<0>(),
                                rounded,
                            )?;
                            f.binary_scale(
                                f.shift_left_const::<13, 34, 15, true>(f.resize_exact(rounded)?)?,
                            )?
                        } else {
                            raw
                        };
                        f.publish("nl", nl)?;
                        let d = clamp(f, nl, Dot::constant::<0>(), Dot::constant::<268435456>())?;
                        let d: Intensity = f.round_to(d)?;
                        f.publish("d", d)?;
                        let product: Fixed<18, 16, false> =
                            named(f, "diffuse.light_product", f.product(id, d)?)?;
                        let (g, diffuse, sum) = super::pipeline::diffuse_finish(f, product, ia)?;
                        f.name_value("diffuse.intensity", diffuse)?;
                        f.name_value("diffuse.ambient_sum", sum)?;
                        (n, nl, g, l)
                    };
                    f.branch(
                        named(
                            f,
                            "mode.diffuse_only",
                            f.less(mode, Fixed::<2, 0, false>::constant::<3>())?,
                        )?,
                        |f| outputs(f, g, Intensity::constant::<0>()),
                        |f| {
                            let mut half_factors = None;
                            let h = if let Some(half) = input.half {
                                let h = read3(f, half)?;
                                for (i, value) in h.iter().enumerate() {
                                    f.publish(&format!("h.{i}"), *value)?;
                                }
                                h
                            } else {
                                let ray = if config.prepared_ray {
                                    let row2 = f.read(input.pixel.at::<2>())?;
                                    [
                                        f.slice::<16, 14, true, 16>(row1.unwrap())?,
                                        f.slice::<16, 14, true, 0>(row2)?,
                                        f.slice::<16, 14, true, 16>(row2)?,
                                    ]
                                } else {
                                    let (x, y): (Ndc, Ndc) = if config.compact_normal {
                                        let row = f.read(input.pixel.at::<1>())?;
                                        (
                                            f.slice::<18, 16, true, 0>(row)?,
                                            f.slice::<18, 16, true, 18>(row)?,
                                        )
                                    } else {
                                        (
                                            f.slice::<18, 16, true, 16>(row1.unwrap())?,
                                            f.slice::<18, 16, true, 0>(
                                                f.read(input.pixel.at::<2>())?,
                                            )?,
                                        )
                                    };
                                    f.name_value("screen.ndc_x", x)?;
                                    f.name_value("screen.ndc_y", y)?;
                                    let px: Fixed<34, 30, true> =
                                        f.product(x, f.read(input.projection.at::<0>())?)?;
                                    let py: Fixed<34, 30, true> =
                                        f.product(y, f.read(input.projection.at::<1>())?)?;
                                    f.name_value("view_ray.x_product", px)?;
                                    f.name_value("view_ray.y_product", py)?;
                                    [
                                        f.round_to(px)?,
                                        f.round_to(py)?,
                                        f.read(input.projection.at::<2>())?,
                                    ]
                                };
                                for (i, value) in ray.iter().enumerate() {
                                    f.publish(&format!("ray.{i}"), *value)?;
                                }
                                let v = normalize(
                                    f,
                                    ray,
                                    Magnitude::constant::<4>(),
                                    true,
                                    t,
                                    "v",
                                    (config.dataflow, config.block_prescale).into(),
                                )?;
                                let mut half = [Direction::constant::<0>(); 3];
                                for i in 0..3 {
                                    let sum: HalfSum = f.add(l[i], v[i])?;
                                    let sum = f.binary_scale::<17, 15, true>(sum)?;
                                    half[i] = f.round_to(sum)?;
                                }
                                name_vector(f, "halfway.input", half)?;
                                if config.scalar_norm {
                                    let factors = prepare_normalization(
                                        f,
                                        half,
                                        Magnitude::constant::<64>(),
                                        false,
                                        t,
                                        "h",
                                        (true, config.block_prescale).into(),
                                    )?;
                                    half_factors = Some(factors);
                                    factors.scaled
                                } else {
                                    normalize(
                                        f,
                                        half,
                                        Magnitude::constant::<64>(),
                                        false,
                                        t,
                                        "h",
                                        (config.dataflow, config.block_prescale).into(),
                                    )?
                                }
                            };
                            let x: Specular = if let Some(hf) = half_factors {
                                let nh = scalar_half_dot(f, n, hf, normal_factors)?;
                                let published: Dot = f.binary_scale(
                                    f.shift_left_const::<13, 34, 15, true>(f.resize_exact(nh)?)?,
                                )?;
                                f.publish("nh", published)?;
                                f.resize_exact(clamp(
                                    f,
                                    nh,
                                    Fixed::<18, 15, true>::constant::<0>(),
                                    Fixed::<18, 15, true>::constant::<32768>(),
                                )?)?
                            } else {
                                let nh = dot(f, n, h, "normal_dot_halfway")?;
                                f.publish("nh", nh)?;
                                f.round_to(clamp(
                                    f,
                                    nh,
                                    Dot::constant::<0>(),
                                    Dot::constant::<268435456>(),
                                )?)?
                            };
                            f.publish("x", x)?;
                            let p =
                                power(f, x, f.read(input.code.at::<0>())?, t, config.power_floor)?;
                            f.publish(
                                if config.quantization == LightingQuantization::CompensatedFloor {
                                    "power.midpoint_q15"
                                } else {
                                    "power"
                                },
                                p,
                            )?;
                            let p9: Intensity = f.round_to(p)?;
                            let positive = match normal_gate {
                                Some(gate) => gate,
                                None => f.less(Dot::constant::<0>(), nl)?,
                            };
                            let p9 = f.select(positive, p9, Intensity::constant::<0>())?;
                            f.publish("p9", p9)?;
                            let product: Fixed<18, 16, false> =
                                named(f, "specular.light_product", f.product(id, p9)?)?;
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

/// New transport without implicit producer quantization. Only valid twelve-bit
/// codes enter; expansion keeps the existing square/rsqrt/power contract.
pub fn evaluate_compact(
    pixel: CompactPixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    max_events: usize,
) -> Result<Report, Error> {
    evaluate_with_config(
        pixel.expanded()?,
        material,
        light,
        projection,
        max_events,
        Config {
            compact_normal: true,
            ..Config::architecture()
        },
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
    evaluate_with_preparation(
        pixel,
        material,
        light,
        projection,
        max_events,
        config,
        (None, None, None),
    )
}
/// Complete architecture graph for static hardware lowering. Runtime power
/// endpoints use a safe address and an explicit exact-result selection.
pub(crate) fn hardware_template(full: bool) -> Result<Report, Error> {
    hardware_template_with_config(full, Config::architecture())
}
pub(crate) fn hardware_template_with_config(full: bool, config: Config) -> Result<Report, Error> {
    evaluate_with_preparation(
        PixelInput {
            normal: [0; 3],
            ndc: [0; 2],
        },
        Material::default(),
        Light::default(),
        Projection::default(),
        2048,
        config,
        (None, None, Some(if full { 3 } else { 2 })),
    )
}

/// Reuse an independently prepared H only for the exact coordinate and context.
/// Different normal, material shininess or intensity may use the same H key.
pub fn evaluate_reusing_half(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    max_events: usize,
    mut config: Config,
    cached: &HalfPreparation,
) -> Result<Report, Error> {
    if cached.ndc != pixel.ndc
        || cached.light != light.direction
        || cached.projection.ray_scale != projection.ray_scale
        || cached.projection.k != projection.k
    {
        return Err(InputError::Configuration.into());
    }
    config.shared_half = true;
    evaluate_with_preparation(
        pixel,
        material,
        light,
        projection,
        max_events,
        config,
        (Some(cached), None, None),
    )
}
fn evaluate_with_preparation(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    max_events: usize,
    config: Config,
    cached: (
        Option<&HalfPreparation>,
        Option<&FlatPreparation>,
        Option<u8>,
    ),
) -> Result<Report, Error> {
    validate(pixel, material, light, projection)?;
    if config.quantization == LightingQuantization::CompensatedFloor
        && config != Config::compensated_resource_profile(super::super::LightingProfile::Fast)
        && config != Config::compensated_resource_profile(super::super::LightingProfile::Compact)
    {
        return Err(InputError::Configuration.into());
    }
    if config.compact_prescale && !config.compact_normal {
        return Err(InputError::Configuration.into());
    }
    if config.compact_normal
        && (config.prepared_ray
            || config.shared_half
            || config.flat_normal
            || config.exact_normal_gate)
    {
        return Err(InputError::Configuration.into());
    }
    if config.scalar_normal && (!config.scalar_norm || config.flat_normal || config.shared_half) {
        return Err(InputError::Configuration.into());
    }
    let (cached_half, cached_flat, hardware_mode) = cached;
    let static_power = hardware_mode.is_some();
    let mut model = Model::numerical();
    let mode = if let Some(mode) = hardware_mode {
        i128::from(mode)
    } else if material.unlit {
        0
    } else if light.directional == 0 {
        1
    } else if material.specular_color == [0; 3] {
        2
    } else {
        3
    };
    let flat_preparation = if config.flat_normal && mode >= 2 {
        Some(if let Some(cached) = cached_flat {
            cached.clone()
        } else {
            prepare_flat(pixel.normal, light, max_events, config)?
        })
    } else {
        None
    };
    let half_preparation = if config.shared_half && mode == 3 {
        Some(if let Some(cached) = cached_half {
            cached.clone()
        } else {
            prepare_half(pixel, light, projection, max_events, config)?
        })
    } else {
        None
    };
    let (pixel_rows, ray_preparation) = if config.compact_normal {
        if pixel.normal.iter().any(|v| v & 15 != 0) {
            return Err(InputError::Configuration.into());
        }
        let compact = CompactPixelInput {
            normal: pixel.normal.map(|v| v >> 4),
            ndc: pixel.ndc,
        };
        (compact.rows()?.to_vec(), None)
    } else if config.prepared_ray && mode == 3 && !config.shared_half {
        let prepared = prepare_ray(pixel, projection, max_events)?;
        let ray = prepared.0;
        let rows = [
            u64::from(pixel.normal[0] as u16) | (u64::from(pixel.normal[1] as u16) << 16),
            u64::from(pixel.normal[2] as u16) | (u64::from(ray[0] as u16) << 16),
            u64::from(ray[1] as u16) | (u64::from(ray[2] as u16) << 16),
        ];
        (rows.to_vec(), Some(prepared.1))
    } else {
        (PixelRows::encode(pixel)?.0.to_vec(), None)
    };
    let half_input = if let Some(prep) = &half_preparation {
        Some(model.input(
            "pixel.prepared-half",
            &std::array::from_fn::<_, 3, _>(|i| {
                prep.half
                    .outputs
                    .iter()
                    .find(|o| o.name == format!("h.{i}"))
                    .unwrap()
                    .raw
            }),
        )?)
    } else {
        None
    };
    let flat_input = if let Some(prep) = &flat_preparation {
        let get = |name: &str| {
            prep.frame
                .outputs
                .iter()
                .find(|o| o.name == name)
                .unwrap()
                .raw
        };
        Some(FlatInputs {
            normal: model.input("context.flat-normal", &[get("n.0"), get("n.1"), get("n.2")])?,
            nl: model.input("context.flat-nl", &[get("nl")])?,
            d: model.input("context.flat-d", &[get("d")])?,
            g: model.input("context.flat-g", &[get("g")])?,
        })
    } else {
        None
    };
    let input = Inputs {
        normal_lsb: if config.exact_normal_gate {
            Some(model.input(
                "pixel.normal-lsb",
                &[i128::from(
                    (pixel.normal[0] as u16 & 7)
                        | ((pixel.normal[1] as u16 & 7) << 3)
                        | ((pixel.normal[2] as u16 & 7) << 6),
                )],
            )?)
        } else {
            None
        },
        flat: flat_input,
        half: half_input,
        pixel: model.input(
            if config.compact_normal {
                "pixel.compact-rows"
            } else {
                "pixel.rows"
            },
            &pixel_rows.into_iter().map(i128::from).collect::<Vec<_>>(),
        )?,
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
    let context_preparation = if config.dataflow && mode == 3 {
        Some(prepare_context(material.shininess_code, max_events)?)
    } else {
        None
    };
    let t = Tables {
        direct_square: config.direct_square,
        static_power,
        context_is_latched: config.dataflow,
        square: if config.square9 {
            SquareTable::Magnitude9
        } else if config.direct_all_squares {
            SquareTable::Exact
        } else if config.signed_square || config.direct_square {
            SquareTable::Signed(model.table("SQ", &SQUARE_SIGNED)?)
        } else {
            SquareTable::Magnitude(model.table("SQ", &SQUARE)?)
        },
        rsqrt: model.table("RSQRT", &RSQRT)?,
        power: if config.quantization == LightingQuantization::CompensatedFloor {
            model.table("POWER_MIDPOINT_Q15", &POWER_MIDPOINT)?
        } else {
            model.table("POWER", &POWER)?
        },
        context: if config.dataflow {
            // SET_MATERIAL prepares this invariant before the pixel frame starts.
            model.input(
                "context.power",
                &[context_preparation.as_ref().map_or(0, |r| r.outputs[0].raw)],
            )?
        } else {
            model.table("POWER_CONTEXT", &CONTEXT)?
        },
    };
    let f = model.compute("pixel_lighting", max_events)?;
    kernel(
        &Arithmetic {
            frame: &f,
            policy: config.quantization,
        },
        &input,
        &t,
        config,
    )?;
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
        ray_preparation,
        context_preparation,
        half_preparation,
        flat_preparation,
    })
}

/// Exact reference preparation; costs two 18x18 products and two RNE operations.
/// A raster may replace repeated products with a Q30 scan accumulator, provided
/// its seed/step represent the same quantized NDC sequence.
pub fn prepare_ray(
    pixel: PixelInput,
    projection: Projection,
    max_events: usize,
) -> Result<([i16; 3], FrameReport), Error> {
    if pixel.ndc.iter().any(|&x| !(-65536..=65536).contains(&x)) {
        return Err(InputError::ViewRay.into());
    }
    if !(8192..=12288).contains(&projection.k)
        || projection
            .ray_scale
            .iter()
            .any(|&x| i32::from(x).abs() > 12288)
    {
        return Err(InputError::ViewRay.into());
    }
    let mut m = Model::numerical();
    let ndc = m.input::<18, 16, true>("raster.ndc", &pixel.ndc.map(i128::from))?;
    let scale =
        m.input::<16, 14, true>("context.ray-scale", &projection.ray_scale.map(i128::from))?;
    let k = m.input::<16, 14, true>("context.k", &[i128::from(projection.k)])?;
    let f = m.compute("upstream ray preparation", max_events)?;
    let x: Fixed<34, 30, true> = f.product(f.read(ndc.at::<0>())?, f.read(scale.at::<0>())?)?;
    let y: Fixed<34, 30, true> = f.product(f.read(ndc.at::<1>())?, f.read(scale.at::<1>())?)?;
    f.publish("ray.0", f.round_to::<16, 14, true>(x)?)?;
    f.publish("ray.1", f.round_to::<16, 14, true>(y)?)?;
    f.publish("ray.2", f.read(k.at::<0>())?)?;
    let report = f.finish();
    report.audit()?;
    let result = std::array::from_fn(|i| report.outputs[i].raw as i16);
    Ok((result, report))
}

/// One read at material preparation, retained until the last context consumer.
pub fn prepare_context(code: u8, max_events: usize) -> Result<FrameReport, Error> {
    if code > 16 {
        return Err(InputError::Shininess.into());
    }
    let mut m = Model::numerical();
    let input = m.input::<5, 0, false>("material.shininess", &[i128::from(code)])?;
    let table = m.table("POWER_CONTEXT", &CONTEXT)?;
    let f = m.compute("material power context preparation", max_events)?;
    f.publish(
        "power-context",
        f.read(table.indexed(f.read(input.at::<0>())?))?,
    )?;
    let report = f.finish();
    report.audit()?;
    Ok(report)
}

/// Exact reusable half vector. Its key MUST include screen coordinate and the
/// immutable projection/light context. A quad or triangle identity alone is not
/// a valid key. This preparation has its own closed numerical ledger.
pub fn prepare_half(
    pixel: PixelInput,
    light: Light,
    projection: Projection,
    max_events: usize,
    config: Config,
) -> Result<HalfPreparation, Error> {
    validate(pixel, Material::default(), light, projection)?;
    let (ray, ray_report) = prepare_ray(pixel, projection, max_events)?;
    let mut m = Model::numerical();
    let input = m.input::<16, 14, true>("ray", &ray.map(i128::from))?;
    let l = m.input::<16, 14, true>("context.light", &light.direction.map(i128::from))?;
    let t = Tables {
        direct_square: config.direct_square,
        static_power: false,
        square: if config.square9 {
            SquareTable::Magnitude9
        } else if config.direct_all_squares {
            SquareTable::Exact
        } else if config.signed_square || config.direct_square {
            SquareTable::Signed(m.table("SQ", &SQUARE_SIGNED)?)
        } else {
            SquareTable::Magnitude(m.table("SQ", &SQUARE)?)
        },
        rsqrt: m.table("RSQRT", &RSQRT)?,
        power: m.table("POWER", &POWER)?,
        context: m.table("POWER_CONTEXT", &CONTEXT)?,
        context_is_latched: false,
    };
    let f = m.compute("shared half-vector preparation", max_events)?;
    let arithmetic = Arithmetic {
        frame: &f,
        policy: LightingQuantization::NearestEven,
    };
    let a = &arithmetic;
    let v = normalize(
        a,
        read3(a, input)?,
        Magnitude::constant::<4>(),
        true,
        &t,
        "v",
        (config.dataflow, config.block_prescale).into(),
    )?;
    let l = read3(a, l)?;
    let mut half = [Direction::constant::<0>(); 3];
    for i in 0..3 {
        let sum: HalfSum = a.add(l[i], v[i])?;
        half[i] = a.round_to(a.binary_scale::<17, 15, true>(sum)?)?;
    }
    normalize(
        a,
        half,
        Magnitude::constant::<64>(),
        false,
        &t,
        "h",
        (config.dataflow, config.block_prescale).into(),
    )?;
    let report = f.finish();
    report.audit()?;
    Ok(HalfPreparation {
        ray: ray_report,
        half: report,
        ndc: pixel.ndc,
        light: light.direction,
        projection,
    })
}

/// Exact linear NDC scan candidate. Seeds and step use the quantized NDC contract;
/// Q30 accumulation avoids drift from repeatedly adding already rounded Q14 rays.
/// Three one-time products (X/Y seeds and X step), one wide add per later pixel,
/// and X RNE per pixel replace two repeated products. Y RNE is shared once.
pub fn scanline_rays(
    seed: PixelInput,
    ndc_step: i32,
    count: usize,
    projection: Projection,
    max_events: usize,
) -> Result<(Vec<[i16; 3]>, FrameReport), Error> {
    if count == 0 || count > 64 || !(-65536..=65536).contains(&ndc_step) {
        return Err(InputError::Configuration.into());
    }
    let last = i64::from(seed.ndc[0]) + i64::from(ndc_step) * (count as i64 - 1);
    if !(-65536..=65536).contains(&last) {
        return Err(InputError::ViewRay.into());
    }
    validate(seed, Material::default(), Light::default(), projection)?;
    let mut m = Model::numerical();
    let xy = m.input::<18, 16, true>("raster.seed", &seed.ndc.map(i128::from))?;
    let step = m.input::<18, 16, true>("raster.step", &[i128::from(ndc_step)])?;
    let scale = m.input::<16, 14, true>("context.scale", &projection.ray_scale.map(i128::from))?;
    let k = m.input::<16, 14, true>("context.k", &[i128::from(projection.k)])?;
    let f = m.compute("exact Q30 scan rays", max_events)?;
    let mut x: Fixed<34, 30, true> = f.product(f.read(xy.at::<0>())?, f.read(scale.at::<0>())?)?;
    let y: Fixed<34, 30, true> = f.product(f.read(xy.at::<1>())?, f.read(scale.at::<1>())?)?;
    let delta: Fixed<34, 30, true> =
        f.product(f.read(step.at::<0>())?, f.read(scale.at::<0>())?)?;
    let y: Direction = f.round_to(y)?;
    let z = f.read(k.at::<0>())?;
    for i in 0..count {
        f.publish(&format!("ray.{i}.0"), f.round_to::<16, 14, true>(x)?)?;
        f.publish(&format!("ray.{i}.1"), y)?;
        f.publish(&format!("ray.{i}.2"), z)?;
        if i + 1 < count {
            x = f.add_same(x, delta)?;
        }
    }
    let report = f.finish();
    report.audit()?;
    let rows = (0..count)
        .map(|i| std::array::from_fn(|c| report.outputs[i * 3 + c].raw as i16))
        .collect();
    Ok((rows, report))
}

/// Once per explicitly flat triangle and light/intensity context. N/NL/d/g are
/// invariant; H and the specular dot/power remain per-coordinate work.
pub fn prepare_flat(
    normal: [i16; 3],
    light: Light,
    max_events: usize,
    config: Config,
) -> Result<FlatPreparation, Error> {
    validate(
        PixelInput {
            normal,
            ndc: [0; 2],
        },
        Material::default(),
        light,
        Projection::default(),
    )?;
    let mut m = Model::numerical();
    let source = m.input::<16, 14, true>("normal", &normal.map(i128::from))?;
    let l = m.input::<16, 14, true>("context.light", &light.direction.map(i128::from))?;
    let intensity = m.input::<9, 8, false>(
        "context.intensity",
        &[i128::from(light.ambient), i128::from(light.directional)],
    )?;
    let t = Tables {
        direct_square: config.direct_square,
        static_power: false,
        square: if config.square9 {
            SquareTable::Magnitude9
        } else if config.direct_all_squares {
            SquareTable::Exact
        } else if config.signed_square || config.direct_square {
            SquareTable::Signed(m.table("SQ", &SQUARE_SIGNED)?)
        } else {
            SquareTable::Magnitude(m.table("SQ", &SQUARE)?)
        },
        rsqrt: m.table("RSQRT", &RSQRT)?,
        power: m.table("POWER", &POWER)?,
        context: m.table("POWER_CONTEXT", &CONTEXT)?,
        context_is_latched: false,
    };
    let f = m.compute("flat triangle diffuse preparation", max_events)?;
    let arithmetic = Arithmetic {
        frame: &f,
        policy: LightingQuantization::NearestEven,
    };
    let a = &arithmetic;
    let n = normalize(
        a,
        read3(a, source)?,
        Magnitude::constant::<4>(),
        false,
        &t,
        "n",
        (config.dataflow, config.block_prescale).into(),
    )?;
    let nl = dot(a, n, read3(a, l)?, "normal_dot_light")?;
    f.publish("nl", nl)?;
    let d = clamp(a, nl, Dot::constant::<0>(), Dot::constant::<268435456>())?;
    let d: Intensity = a.round_to(d)?;
    f.publish("d", d)?;
    let product: Fixed<18, 16, false> = f.product(a.read(intensity.at::<1>())?, d)?;
    let diffuse: Intensity = a.round_to(product)?;
    let sum: IntensitySum = a.add(a.read(intensity.at::<0>())?, diffuse)?;
    let g = clamp(
        a,
        sum,
        IntensitySum::constant::<0>(),
        IntensitySum::constant::<511>(),
    )?;
    f.publish("g", f.resize_exact::<9, 8, false>(g)?)?;
    let frame = f.finish();
    frame.audit()?;
    Ok(FlatPreparation {
        frame,
        normal,
        light,
    })
}
/// Key-checked flat sharing. Never silently substitute a neighbor's normal.
pub fn evaluate_reusing_flat(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
    max_events: usize,
    mut config: Config,
    cached: &FlatPreparation,
) -> Result<Report, Error> {
    if cached.normal != pixel.normal
        || cached.light.direction != light.direction
        || cached.light.ambient != light.ambient
        || cached.light.directional != light.directional
    {
        return Err(InputError::Configuration.into());
    }
    config.flat_normal = true;
    evaluate_with_preparation(
        pixel,
        material,
        light,
        projection,
        max_events,
        config,
        (None, Some(cached), None),
    )
}

#[cfg(test)]
mod compact_tests {
    use super::*;

    #[test]
    fn compact_sign_magnitude_exhausts_codes_including_signed_minimum_and_zero() {
        for code in -2048..=2047 {
            let normal = [code, code / 2, if code == -2048 { 2047 } else { -code }];
            let pixel = CompactPixelInput {
                normal,
                ndc: [0; 2],
            };
            let mut model = Model::numerical();
            let rows = model
                .input::<36, 0, false>("pixel.compact-rows", &[pixel.rows().unwrap()[0] as i128])
                .unwrap();
            let tables = Tables {
                direct_square: false,
                static_power: false,
                context_is_latched: false,
                square: SquareTable::Signed(model.table("SQ", &SQUARE_SIGNED).unwrap()),
                rsqrt: model.table("RSQRT", &RSQRT).unwrap(),
                power: model.table("POWER", &POWER).unwrap(),
                context: model.table("POWER_CONTEXT", &CONTEXT).unwrap(),
            };
            let f = model.compute("compact magnitude", 512).unwrap();
            let arithmetic = Arithmetic {
                frame: &f,
                policy: LightingQuantization::NearestEven,
            };
            let a = &arithmetic;
            let (raw, compact) = compact_normal(a, a.read(rows.at::<0>()).unwrap()).unwrap();
            for (i, value) in raw.iter().enumerate() {
                f.publish(&format!("raw.{i}"), *value).unwrap();
            }
            f.publish("max", compact.maximum).unwrap();
            f.publish("zero", compact.zero).unwrap();
            normalize(
                a,
                raw,
                Magnitude::constant::<4>(),
                false,
                &tables,
                "n",
                NormalizationPolicy {
                    fast: true,
                    block: false,
                    compact: Some(compact),
                    compact_prescale: true,
                },
            )
            .unwrap();
            let report = f.finish();
            report.audit().unwrap();
            let output = |name: &str| report.outputs.iter().find(|v| v.name == name).unwrap().raw;
            for (i, value) in normal.iter().enumerate() {
                let named = |name: &str| {
                    report
                        .values
                        .iter()
                        .find(|v| v.name.as_deref() == Some(name))
                        .unwrap()
                };
                let axis = ["x", "y", "z"][i];
                let magnitude = named(&format!("normal.compact.magnitude.{axis}"));
                assert_eq!(magnitude.format.bits, 12);
                assert!(!magnitude.format.signed);
                assert_eq!(magnitude.raw, i128::from(*value).abs());
                assert_eq!(
                    named(&format!("normal.compact.sign.{axis}")).raw,
                    i128::from(*value < 0)
                );
                assert_eq!(output(&format!("raw.{i}")), i128::from(*value) * 16);
            }
            assert_eq!(
                output("max"),
                normal
                    .iter()
                    .map(|&v| i128::from(v).abs() * 16)
                    .max()
                    .unwrap()
            );
            assert_eq!(output("zero"), i128::from(normal == [0; 3]));
            let golden = super::super::oracle::evaluate(
                pixel.expanded().unwrap(),
                Material::default(),
                Light::default(),
                Projection::default(),
                super::super::oracle::Config::default(),
            )
            .unwrap();
            let observed: Vec<_> = report
                .outputs
                .iter()
                .filter(|v| v.name.starts_with("n."))
                .map(|v| (v.name.clone(), v.raw))
                .collect();
            let expected: Vec<_> = golden
                .stages
                .into_iter()
                .filter(|(name, _)| name.starts_with("n."))
                .collect();
            assert_eq!(observed, expected, "normal={normal:?}");
        }
    }
}
