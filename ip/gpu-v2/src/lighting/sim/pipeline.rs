//! Typed reusable numerical blocks and opt-in, measured single-cycle bindings.
//! The numerical operators remain visible to counted. Timing contraction never
//! absorbs a DSP, a memory read, a branch, or an observable intermediate value.
use crate::lighting::format::{Direction, Intensity, IntensitySum};
use audited::{Fault, Fixed, Frame, FrameReport, Model, Operation};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    NormalizedOutput { zero_gate: bool },
    ReciprocalTail,
    DiffuseFinish,
    InverseHead,
    InverseTail,
    SquareSum,
    PowerHead,
    SquareSumHead,
}
impl BlockKind {
    fn topology_signature(self) -> u64 {
        match self {
            Self::NormalizedOutput { zero_gate: false } => 0x672c4a124ea86d5b,
            Self::NormalizedOutput { zero_gate: true } => 0x4c67b338db8d936f,
            Self::ReciprocalTail => 0x7a447789bda158de,
            Self::DiffuseFinish => 0x41ff8c8fff40d1ee,
            Self::InverseHead => 0x6613ee65e5746e0b,
            Self::InverseTail => 0x20062abd6a6b9db3,
            Self::SquareSum => 0xd04217cca9e24ce5,
            Self::PowerHead => 0x59683cd2ebb7e36f,
            Self::SquareSumHead => 0xab7ffb5a088facbe,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::NormalizedOutput { zero_gate: true } => "normalized-output-zero",
            Self::NormalizedOutput { zero_gate: false } => "normalized-output",
            Self::ReciprocalTail => "reciprocal-tail",
            Self::DiffuseFinish => "diffuse-finish",
            Self::InverseHead => "inverse-head",
            Self::InverseTail => "inverse-tail",
            Self::SquareSum => "square-sum",
            Self::PowerHead => "power-head",
            Self::SquareSumHead => "square-sum-head",
        }
    }
    /// Reference isolated probe; not a fitted whole-component frequency claim.
    pub fn evidence(self) -> &'static str {
        match self {
            Self::NormalizedOutput { .. } => "lighting-fabric-fusion-20261005/normalized_output; lighting-memory-fusion-20261005/bs_write_normal,ss_write_normal",
            Self::ReciprocalTail => "lighting-fabric-fusion-20261005/rsqrt_interpolate_tail (includes an additional bounded restore shift)",
            Self::DiffuseFinish => "lighting-fabric-fusion-20261005/diffuse_finish",
            Self::InverseHead => "lighting-function-fusion-20261005/inverse_address_bounded; lighting-oc-fusion-20261005/rsqrt-memory",
            Self::InverseTail => "lighting-function-fusion-20261005/inverse_tail_bounded; lighting-oc-fusion-20261005/shared-mux",
            Self::SquareSum => "lighting-function-fusion-20261005/inverse_sum_address_bounded",
            Self::PowerHead => "lighting-retime-20261005/power-control/power_frontend_whole (endpoint clamp excluded)",
            Self::SquareSumHead => "multiplier-retiming/sum-address60 (whole Lighting candidate)",
        }
    }
}

/// Two completed square sums. The public q remains a separately visible result.
pub fn square_sum(
    f: &Frame<'_>,
    values: [Fixed<30, 28, false>; 3],
) -> Result<(Fixed<30, 28, false>, Fixed<30, 28, false>), Fault> {
    let xy = f.add_same(values[0], values[1])?;
    Ok((f.add_same(xy, values[2])?, xy))
}

pub struct InverseHead {
    pub exponent: Fixed<18, 0, true>,
    pub align: Fixed<18, 0, true>,
    pub mantissa: Fixed<30, 28, false>,
    pub segment: Fixed<6, 0, false>,
    pub fraction: Fixed<8, 8, false>,
    pub address: Fixed<7, 0, false>,
    pub restore: Fixed<1, 0, false>,
    pub zeros: Fixed<18, 0, true>,
}
/// One multi-output boundary: square sums, bounded LZD and mantissa alignment.
/// Export the sum with address/fraction/restore, preserving visible stage data.
pub fn square_sum_head(
    f: &Frame<'_>,
    values: [Fixed<30, 28, false>; 3],
) -> Result<(Fixed<30, 28, false>, InverseHead), Fault> {
    let q = square_sum(f, values)?.0;
    Ok((q, inverse_head(f, q)?))
}
/// One measured fabric boundary: q -> address, fraction and restore.
/// Domain 2^26 <= q <= 3*2^28 is established by the normalization producer.
/// All side results stay in the DAG; explicit exports share one ready edge.
pub fn inverse_head(f: &Frame<'_>, q: Fixed<30, 28, false>) -> Result<InverseHead, Fault> {
    let zeros = f.leading_zeros(q)?;
    let z = f.resize_exact::<2, 0, false>(zeros)?;
    let z = f.resize_exact::<6, 0, true>(z)?;
    let exponent: Fixed<18, 0, true> =
        f.resize_exact(f.sub_same(Fixed::<6, 0, true>::constant::<1>(), z)?)?;
    let align = f.resize_exact(f.sub_same(z, Fixed::<6, 0, true>::constant::<15>())?)?;
    let mantissa = f.shift(q, align)?;
    let segment = f.slice::<6, 0, false, 8>(mantissa)?;
    let fraction = f.slice::<8, 8, false, 0>(mantissa)?;
    let parity = f.slice::<1, 0, false, 0>(exponent)?;
    let page = f.shift_left_const::<6, 7, 0, false>(f.resize_exact(parity)?)?;
    let half = f.shift(exponent, Fixed::<18, 0, true>::constant::<-1>())?;
    let e = f.resize_exact::<6, 0, true>(half)?;
    let restore = f.resize_exact(f.sub_same(Fixed::<6, 0, true>::constant::<0>(), e)?)?;
    let address = f.add_same(page, f.resize_exact(segment)?)?;
    Ok(InverseHead {
        exponent,
        align,
        mantissa,
        segment,
        fraction,
        address,
        restore,
        zeros,
    })
}
/// RNE/correction subtraction plus bounded 0/1 exponent restoration, one edge.
pub fn inverse_tail(
    f: &Frame<'_>,
    base: Fixed<16, 15, false>,
    correction: Fixed<16, 23, false>,
    restore: Fixed<18, 0, true>,
) -> Result<(super::super::format::Reciprocal, Fixed<16, 15, false>), Fault> {
    inverse_tail_with_floor(f, base, correction, restore, false)
}
pub fn inverse_tail_with_floor(
    f: &Frame<'_>,
    base: Fixed<16, 15, false>,
    correction: Fixed<16, 23, false>,
    restore: Fixed<18, 0, true>,
    floor: bool,
) -> Result<(super::super::format::Reciprocal, Fixed<16, 15, false>), Fault> {
    let correction = if floor {
        f.floor_to(correction)?
    } else {
        f.round_to(correction)?
    };
    let interpolated = f.sub_same(base, correction)?;
    Ok((
        f.shift(f.resize_exact(interpolated)?, restore)?,
        interpolated,
    ))
}

/// One coordinate boundary: latched table context and safe x -> address/tail.
/// Negated shift is an explicit side result, reused after the interpolation DSP.
pub struct PowerHead {
    pub address: Fixed<10, 0, false>,
    pub tail: Fixed<12, 0, false>,
    pub neg: Fixed<18, 0, true>,
}
pub fn power_head(
    f: &Frame<'_>,
    x: super::super::format::Specular,
    context: Fixed<43, 0, false>,
) -> Result<PowerHead, Fault> {
    let boundary = f.slice::<15, 0, false, 0>(context)?;
    let wide = f.slice::<4, 0, false, 15>(context)?;
    let fine = f.slice::<4, 0, false, 19>(context)?;
    let base_w = f.slice::<10, 0, false, 23>(context)?;
    let base_f = f.slice::<10, 0, false, 33>(context)?;
    let raw = f.binary_scale::<16, 0, false>(x)?;
    let coarse = f.less(raw, boundary)?;
    let shift_code = f.select(coarse, wide, fine)?;
    let shift: Fixed<18, 0, true> = f.resize_exact(shift_code)?;
    let base = f.select(coarse, base_w, base_f)?;
    let neg = f.sub_same(Fixed::<18, 0, true>::constant::<0>(), shift)?;
    let index = f.shift(raw, neg)?;
    let offset = f.add::<17, 0, false>(base, index)?;
    let aligned = f.shift(index, shift)?;
    let tail = f.resize_exact::<12, 0, false>(f.sub_same(raw, aligned)?)?;
    // Keep diagnostics attached to the numerical DAG; names do not imply exports.
    for (name, value) in [
        ("specular.coarse_shift", wide),
        ("specular.fine_shift", fine),
        ("specular.selected_shift", shift_code),
    ] {
        f.name_value(name, value)?;
    }
    f.name_value("specular.split_boundary", boundary)?;
    f.name_value("specular.coarse_region", coarse)?;
    f.name_value("specular.index_shift", shift)?;
    f.name_value("specular.table_page", base)?;
    f.name_value("specular.table_index", index)?;
    let address = f.slice::<10, 0, false, 0>(offset)?;
    Ok(PowerHead { address, tail, neg })
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

/// Post-DSP S33F29 -> RNE S18F14 -> +/-1 clamp -> optional zero gate.
/// Returning the rounded value preserves existing diagnostic names. If a caller
/// actually publishes/consumes it elsewhere, the atomic binding is rejected.
pub fn normalized_output(
    f: &Frame<'_>,
    product: Fixed<33, 29, true>,
    zero: Option<Fixed<1, 0, false>>,
) -> Result<(Direction, Fixed<18, 14, true>), Fault> {
    normalized_output_with_floor(f, product, zero, false)
}
pub fn normalized_output_with_floor(
    f: &Frame<'_>,
    product: Fixed<33, 29, true>,
    zero: Option<Fixed<1, 0, false>>,
    floor: bool,
) -> Result<(Direction, Fixed<18, 14, true>), Fault> {
    let rounded = if floor {
        f.floor_to::<18, 14, true>(product)?
    } else {
        f.round_to::<18, 14, true>(product)?
    };
    let clamped = clamp(
        f,
        rounded,
        Fixed::constant::<-16384>(),
        Fixed::constant::<16384>(),
    )?;
    let value = match zero {
        Some(zero) => f.select(zero, Direction::constant::<0>(), f.resize_exact(clamped)?)?,
        None => f.resize_exact(clamped)?,
    };
    Ok((value, rounded))
}

/// Post-DSP correction U16F23 -> RNE U16F15 -> base subtract U16F15.
/// The round increment has at most nine active bits: the floor is <=255 and
/// its carry is <=256. The existing public U16 format remains unchanged.
pub fn reciprocal_tail(
    f: &Frame<'_>,
    base: Fixed<16, 15, false>,
    correction: Fixed<16, 23, false>,
) -> Result<Fixed<16, 15, false>, Fault> {
    f.sub_same(base, f.round_to(correction)?)
}

/// Post-DSP U18F16 -> RNE intensity -> ambient add -> saturate to U9F8.
/// Product-domain checks remain those of the original counted kernel: RNE must
/// fit U9. Returning intermediates does not silently permit them to escape.
pub fn diffuse_finish(
    f: &Frame<'_>,
    product: Fixed<18, 16, false>,
    ambient: Intensity,
) -> Result<(Intensity, Intensity, IntensitySum), Fault> {
    let diffuse: Intensity = f.round_to(product)?;
    let sum: IntensitySum = f.add(ambient, diffuse)?;
    let clamped = clamp(
        f,
        sum,
        IntensitySum::constant::<0>(),
        IntensitySum::constant::<511>(),
    )?;
    Ok((f.resize_exact(clamped)?, diffuse, sum))
}

struct Pattern {
    kind: BlockKind,
    frame: FrameReport,
    result: usize,
    exports: Vec<usize>,
}
/// Pin the numerical topology independently of runtime samples and source lines.
/// Timing evidence must be reviewed again before accepting a changed signature.
fn fingerprint(frame: &FrameReport, result: usize) -> u64 {
    let mut ancestors = BTreeSet::new();
    let mut pending: Vec<_> = std::iter::once(frame.values[result].producer)
        .chain(frame.outputs.iter().map(|o| frame.values[o.value].producer))
        .collect();
    while let Some(id) = pending.pop() {
        if ancestors.insert(id) {
            pending.extend(
                frame.events[id]
                    .inputs
                    .iter()
                    .map(|&v| frame.values[v].producer),
            );
        }
    }
    let mut signature = String::new();
    for id in ancestors {
        let e = &frame.events[id];
        let v = &frame.values[e.output.unwrap()];
        signature.push_str(&format!(
            "{id}:{:?}:{:?}:{:?}",
            e.operation, v.format, e.inputs
        ));
        if matches!(e.operation, Operation::Literal) {
            signature.push_str(&format!("={}", v.raw));
        }
        signature.push('|');
    }
    signature.bytes().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            BlockKind::SquareSumHead,
            BlockKind::NormalizedOutput { zero_gate: false },
            BlockKind::NormalizedOutput { zero_gate: true },
            BlockKind::ReciprocalTail,
            BlockKind::DiffuseFinish,
            BlockKind::InverseHead,
            BlockKind::InverseTail,
            BlockKind::SquareSum,
            BlockKind::PowerHead,
        ]
        .into_iter()
        .map(|kind| {
            let mut m = Model::numerical();
            let product = m.input::<33, 29, true>("product", &[0]).unwrap();
            let zero = m.input::<1, 0, false>("zero", &[0]).unwrap();
            let base = m.input::<16, 15, false>("base", &[32768]).unwrap();
            let correction = m.input::<16, 23, false>("correction", &[0]).unwrap();
            let intensity_product = m.input::<18, 16, false>("intensity-product", &[0]).unwrap();
            let ambient = m.input::<9, 8, false>("ambient", &[0]).unwrap();
            let q = m.input::<30, 28, false>("q", &[1 << 26]).unwrap();
            let restore = m.input::<18, 0, true>("restore", &[0]).unwrap();
            let ctx = m.input::<43,0,false>("context", &[1 << 19]).unwrap();
            let sx = m.input::<16,15,false>("specular", &[0]).unwrap();
            let f = m.compute("typed pipeline pattern", 128).unwrap();
            match kind {
                BlockKind::SquareSumHead => {
                    let a = f.read(q.at::<0>()).unwrap();
                    let b = f.read(q.at::<0>()).unwrap();
                    let c = f.read(q.at::<0>()).unwrap();
                    let (sum, h) = square_sum_head(&f, [a,b,c]).unwrap();
                    f.publish("result", h.address).unwrap();
                    f.publish("fraction", h.fraction).unwrap();
                    f.publish("restore", h.restore).unwrap();
                    f.publish("q", sum).unwrap();
                }
                BlockKind::NormalizedOutput { zero_gate } => {
                    let p = f.read(product.at::<0>()).unwrap();
                    let z = if zero_gate {
                        Some(f.read(zero.at::<0>()).unwrap())
                    } else {
                        None
                    };
                    f.publish("result", normalized_output(&f, p, z).unwrap().0)
                        .unwrap();
                }
                BlockKind::ReciprocalTail => {
                    let b = f.read(base.at::<0>()).unwrap();
                    let c = f.read(correction.at::<0>()).unwrap();
                    f.publish("result", reciprocal_tail(&f, b, c).unwrap())
                        .unwrap();
                }
                BlockKind::DiffuseFinish => {
                    let p = f.read(intensity_product.at::<0>()).unwrap();
                    let a = f.read(ambient.at::<0>()).unwrap();
                    f.publish("result", diffuse_finish(&f, p, a).unwrap().0)
                        .unwrap();
                }
                BlockKind::InverseHead => {
                    let h = inverse_head(&f, f.read(q.at::<0>()).unwrap()).unwrap();
                    f.publish("result", h.address).unwrap();
                    f.publish("fraction", h.fraction).unwrap();
                    f.publish("restore", h.restore).unwrap();
                }
                BlockKind::PowerHead => {
                    let h=power_head(&f,f.read(sx.at::<0>()).unwrap(),f.read(ctx.at::<0>()).unwrap()).unwrap();
                    f.publish("result",h.address).unwrap();f.publish("tail",h.tail).unwrap();f.publish("negative shift",h.neg).unwrap();
                }
                BlockKind::InverseTail => {
                    let b = f.read(base.at::<0>()).unwrap();
                    let c = f.read(correction.at::<0>()).unwrap();
                    let r = f.read(restore.at::<0>()).unwrap();
                    f.publish("result", inverse_tail(&f,b,c,r).unwrap().0).unwrap();
                }
                BlockKind::SquareSum => {
                    let a = f.read(q.at::<0>()).unwrap();
                    let b = f.read(q.at::<0>()).unwrap();
                    let c = f.read(q.at::<0>()).unwrap();
                    f.publish("result", square_sum(&f,[a,b,c]).unwrap().0).unwrap();
                }
            }
            let frame = f.finish();
            frame.audit().unwrap();
            let result = frame.outputs[0].value;
            assert_eq!(fingerprint(&frame,result),kind.topology_signature(),
                "typed block topology changed: revalidate isolated timing before updating the signature");
            let exports = frame.outputs.iter().skip(1).map(|o| o.value).collect();
            Pattern {
                kind,
                frame,
                result,
                exports,
            }
        })
        .collect()
    })
}

/// Exact opcode/format/literal/alias matcher against typed reference DAGs.
/// Dynamic inputs are boundaries regardless of their actual producers; sample
/// values and presentation names are never used as a physical timing proof.
fn match_value(
    pattern: &FrameReport,
    value: usize,
    frame: &FrameReport,
    actual: usize,
    aliases: &mut BTreeMap<usize, usize>,
    members: &mut BTreeSet<usize>,
) -> bool {
    if let Some(&previous) = aliases.get(&value) {
        return previous == actual;
    }
    let a = &pattern.values[value];
    let b = &frame.values[actual];
    if a.format != b.format {
        return false;
    }
    aliases.insert(value, actual);
    let e = &pattern.events[a.producer];
    let other = &frame.events[b.producer];
    if matches!(e.operation, Operation::Read { .. }) {
        return true;
    }
    if matches!(e.operation, Operation::Literal) {
        return matches!(other.operation, Operation::Literal) && a.raw == b.raw;
    }
    if e.operation != other.operation || e.inputs.len() != other.inputs.len() {
        return false;
    }
    members.insert(other.id);
    e.inputs
        .iter()
        .zip(&other.inputs)
        .all(|(&v, &w)| match_value(pattern, v, frame, w, aliases, members))
}

pub(crate) fn bind(
    frame: &FrameReport,
    functions: bool,
    sum_address: bool,
) -> Result<Vec<(BlockKind, audited::physical::LogicCone)>, String> {
    let mut result = Vec::new();
    let mut used = BTreeSet::new();
    // Prefer the complete gated/output block over an earlier matching sub-tail.
    for event in frame.events.iter().rev() {
        let Some(value) = event.output else {
            continue;
        };
        if used.contains(&event.id)
            || !event
                .source
                .file()
                .replace('\\', "/")
                .ends_with("lighting/sim/pipeline.rs")
        {
            continue;
        }
        for p in patterns() {
            if p.kind == BlockKind::SquareSumHead && (!functions || !sum_address) {
                continue;
            }
            if !functions
                && matches!(
                    p.kind,
                    BlockKind::InverseHead
                        | BlockKind::InverseTail
                        | BlockKind::SquareSum
                        | BlockKind::PowerHead
                )
            {
                continue;
            }
            let mut members = BTreeSet::new();
            let mut aliases = BTreeMap::new();
            if !match_value(&p.frame, p.result, frame, value, &mut aliases, &mut members) {
                continue;
            }
            let mut exports = Vec::new();
            let mut complete = true;
            for &extra in &p.exports {
                let mut choices = Vec::new();
                for (candidate_id, candidate) in frame.values.iter().enumerate() {
                    let event = &frame.events[candidate.producer];
                    if !event
                        .source
                        .file()
                        .replace('\\', "/")
                        .ends_with("lighting/sim/pipeline.rs")
                    {
                        continue;
                    }
                    let mut a = aliases.clone();
                    let mut m = members.clone();
                    if match_value(&p.frame, extra, frame, candidate_id, &mut a, &mut m) {
                        choices.push((candidate.producer, a, m));
                    }
                }
                if choices.len() != 1 {
                    complete = false;
                    break;
                }
                let (id, a, m) = choices.pop().unwrap();
                exports.push(id);
                aliases = a;
                members = m;
            }
            if !complete {
                continue;
            }
            if members.iter().any(|id| used.contains(id)) {
                continue;
            }
            let operands: BTreeSet<_> = members
                .iter()
                .flat_map(|&id| frame.events[id].inputs.iter().copied())
                .filter(|&v| !members.contains(&frame.values[v].producer))
                .collect();
            let max_width = members
                .iter()
                .flat_map(|&id| {
                    frame.events[id]
                        .output
                        .into_iter()
                        .chain(frame.events[id].inputs.iter().copied())
                })
                .map(|v| frame.values[v].format.bits)
                .max()
                .unwrap_or(1);
            let cone = audited::physical::LogicCone {
                result_event: event.id,
                absorbed_events: members
                    .iter()
                    .copied()
                    .filter(|&id| id != event.id)
                    .collect(),
                exported_events: exports,
                operands: operands.into_iter().collect(),
                max_width,
                latency: 1,
            };
            // A diagnostic or another consumer can make this region non-atomic.
            // Preserve its primitive scheduling instead of hiding the escape.
            if cone.audit(frame).is_err() {
                continue;
            }
            used.extend(members);
            result.push((p.kind, cone));
            break;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(escape: bool) -> FrameReport {
        let mut m = Model::numerical();
        let input = m.input::<33, 29, true>("product", &[0]).unwrap();
        let f = m.compute("module escape", 128).unwrap();
        let (result, rounded) =
            normalized_output(&f, f.read(input.at::<0>()).unwrap(), None).unwrap();
        f.publish("result", result).unwrap();
        if escape {
            f.publish("visible rounded", rounded).unwrap();
        }
        f.finish()
    }
    #[test]
    fn block_matching_checks_constants_and_does_not_hide_escaped_intermediates() {
        let original = frame(false);
        original.audit().unwrap();
        assert_eq!(bind(&original, false, false).unwrap().len(), 1);
        let escaped = frame(true);
        escaped.audit().unwrap();
        assert!(bind(&escaped, false, false).unwrap().is_empty());
        let mut altered = frame(false);
        // For a zero product changing the lower clamp bound leaves every sampled
        // numerical result valid. It must still invalidate the typed pattern.
        let bound = altered.values.iter_mut().find(|v| v.raw == -16384).unwrap();
        bound.raw = -16383;
        altered.audit().unwrap();
        assert!(bind(&altered, false, false).unwrap().is_empty());
    }
    #[test]
    fn measured_pattern_signatures() {
        for p in patterns() {
            assert_eq!(fingerprint(&p.frame, p.result), p.kind.topology_signature());
        }
    }
    #[test]
    fn sum_address_exports_q_and_rejects_an_unexported_partial_sum() {
        for escape_xy in [false, true] {
            let mut m = Model::numerical();
            let input = m
                .input::<30, 28, false>("squares", &[1 << 26, 1 << 26, 0])
                .unwrap();
            let f = m.compute("sum address exports", 128).unwrap();
            let squares = [
                f.read(input.at::<0>()).unwrap(),
                f.read(input.at::<1>()).unwrap(),
                f.read(input.at::<2>()).unwrap(),
            ];
            let (q, xy) = square_sum(&f, squares).unwrap();
            let h = inverse_head(&f, q).unwrap();
            f.publish("address", h.address).unwrap();
            f.publish("fraction", h.fraction).unwrap();
            f.publish("restore", h.restore).unwrap();
            f.publish("q", q).unwrap();
            if escape_xy {
                f.publish("partial sum", xy).unwrap();
            }
            let frame = f.finish();
            frame.audit().unwrap();
            let blocks = bind(&frame, true, true).unwrap();
            assert_eq!(
                blocks.iter().any(|(k, _)| *k == BlockKind::SquareSumHead),
                !escape_xy
            );
            if !escape_xy {
                let cone = &blocks
                    .iter()
                    .find(|(k, _)| *k == BlockKind::SquareSumHead)
                    .unwrap()
                    .1;
                assert!(cone
                    .exported_events
                    .contains(&frame.values[frame.outputs[3].value].producer));
            }
        }
    }
}
