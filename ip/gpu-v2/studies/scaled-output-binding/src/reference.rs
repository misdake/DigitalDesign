//! Frozen helper from geometry counted.rs blob dacf7da0b72caa561ba89440b975a8e1f06b9647.
//! This is a provenance-labelled test adapter, not a second production oracle.
use crate::window::Spec;
use audited::{Fault, Fixed, Frame, FrameReport, Model};
type E = Fixed<18, 0, true>;
type P = Fixed<1, 0, false>;

// Exact function body at intake; only its visibility/name were adapted.
fn scaled_output<const F: u32, const B: u32, const G: u32>(
    f: &Frame<'_>,
    v: Fixed<72, F, true>,
    e: E,
) -> Result<Fixed<B, G, true>, Fault> {
    if F <= G {
        return Err(Fault::Format);
    }
    f.require::<true>(f.less(e, E::constant::<32>())?)?;
    let shifted = f.shift(v, f.sub_same(E::constant::<0>(), e)?)?;
    let recovered = f.shift(shifted, e)?;
    let lost = f.sub_same(v, recovered)?;
    let sticky = f.less(Fixed::<72, F, true>::constant::<0>(), lost)?;
    let low = f.slice::<1, 0, false, 0>(shifted)?;
    let jam = f.select(low, P::constant::<0>(), sticky)?;
    let incremented = f.add_same(shifted, Fixed::<72, F, true>::constant::<1>())?;
    f.round_to(f.select(jam, incremented, shifted)?)
}

fn typed<const F: u32, const B: u32, const G: u32>(
    v: i128,
    e: i128,
) -> Result<(Result<i128, Fault>, FrameReport), Fault> {
    let mut m = Model::numerical();
    let input = m.input::<72, F, true>("value", &[v])?;
    let exponent = m.input::<18, 0, true>("exponent", &[e])?;
    let f = m.compute("reference-scaling", 256)?;
    let result = (|| {
        let x = f.read(input.at::<0>())?;
        let e = f.read(exponent.at::<0>())?;
        f.publish("result", scaled_output::<F, B, G>(&f, x, e)?)
    })();
    let report = f.finish();
    let result = result.map(|()| report.outputs[0].raw);
    Ok((result, report))
}

pub fn run(spec: Spec, v: i128, e: i128) -> Result<(Result<i128, Fault>, FrameReport), Fault> {
    match (spec.fraction, spec.bits, spec.out_fraction) {
        (46, 36, 28) => typed::<46, 36, 28>(v, e),
        (60, 36, 28) => typed::<60, 36, 28>(v, e),
        (28, 36, 24) => typed::<28, 36, 24>(v, e),
        (60, 36, 24) => typed::<60, 36, 24>(v, e),
        (28, 18, 17) => typed::<28, 18, 17>(v, e),
        (60, 18, 17) => typed::<60, 18, 17>(v, e),
        (28, 10, 9) => typed::<28, 10, 9>(v, e),
        (60, 10, 9) => typed::<60, 10, 9>(v, e),
        _ => Err(Fault::Format),
    }
}
