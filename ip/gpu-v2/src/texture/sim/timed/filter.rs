//! Closed payload kernels executed at the fixed color pipeline boundaries.
use super::super::super::format::*;
use super::super::counted;
use audited::Model;
pub fn partial(payload: i128, words: [u16; 4]) -> Result<[u32; 3], super::Error> {
    let mut m = Model::numerical();
    let packet: GroupWordStore = m.input("Group4", &[payload])?;
    let texels: TexelStore = m.input("cache_words", &words.map(i128::from))?;
    let f = m.compute("timed_group_partial", 512)?;
    let word = f.read(packet.at::<0>())?;
    let weights = [
        f.slice::<9, 0, false, 28>(word)?,
        f.slice::<9, 0, false, 37>(word)?,
        f.slice::<9, 0, false, 46>(word)?,
        f.slice::<9, 0, false, 55>(word)?,
    ];
    let mut products = [[Accumulator::constant::<0>(); 3]; 4];
    for (tap, product) in products.iter_mut().enumerate() {
        let b = match tap {
            0 => texels.at::<0>(),
            1 => texels.at::<1>(),
            2 => texels.at::<2>(),
            _ => texels.at::<3>(),
        };
        let weight = weights[tap];
        let texel = f.read(b)?;
        let red = f.slice::<5, 0, false, 11>(texel)?;
        let green = f.slice::<6, 0, false, 5>(texel)?;
        let blue = f.slice::<5, 0, false, 0>(texel)?;
        let rgb: [Color; 3] = [
            f.add(
                f.shift_left_const::<3, 8, 0, false>(f.resize_exact(red)?)?,
                f.slice::<3, 0, false, 2>(red)?,
            )?,
            f.add(
                f.shift_left_const::<2, 8, 0, false>(f.resize_exact(green)?)?,
                f.slice::<2, 0, false, 4>(green)?,
            )?,
            f.add(
                f.shift_left_const::<3, 8, 0, false>(f.resize_exact(blue)?)?,
                f.slice::<3, 0, false, 2>(blue)?,
            )?,
        ];
        for (out, color) in product.iter_mut().zip(rgb) {
            *out = f.product(weight, color)?;
        }
    }
    for (i, _) in products[0].iter().enumerate() {
        let a = f.add_same(products[0][i], products[1][i])?;
        let b = f.add_same(products[2][i], products[3][i])?;
        f.publish(&format!("partial{i}"), f.add_same(a, b)?)?;
    }
    let report = f.finish();
    report.audit()?;
    Ok(std::array::from_fn(|i| report.outputs[i].raw as u32))
}
pub fn accumulate(
    previous: [u32; 3],
    partial: [u32; 3],
    payload: i128,
) -> Result<[u32; 3], super::Error> {
    let mut m = Model::numerical();
    let values: AccumulatorStore = m.input("accumulator", &previous.map(i128::from))?;
    let incoming: AccumulatorStore = m.input("partial", &partial.map(i128::from))?;
    let packet: GroupWordStore = m.input("Group4", &[payload])?;
    let f = m.compute("timed_accumulator", 96)?;
    let first = f.slice::<1, 0, false, 64>(f.read(packet.at::<0>())?)?;
    for c in 0..3 {
        let (a, b) = match c {
            0 => (values.at::<0>(), incoming.at::<0>()),
            1 => (values.at::<1>(), incoming.at::<1>()),
            _ => (values.at::<2>(), incoming.at::<2>()),
        };
        let old = f.select(first, Accumulator::constant::<0>(), f.read(a)?)?;
        f.publish(&format!("acc{c}"), f.add_same(old, f.read(b)?)?)?;
    }
    let report = f.finish();
    report.audit()?;
    Ok(std::array::from_fn(|i| report.outputs[i].raw as u32))
}
pub fn normalize(value: [u32; 3]) -> Result<[u8; 3], super::Error> {
    let mut m = Model::numerical();
    let input: AccumulatorStore = m.input("sum", &value.map(i128::from))?;
    let f = m.compute("timed_normalize", 96)?;
    for c in 0..3 {
        let address = match c {
            0 => input.at::<0>(),
            1 => input.at::<1>(),
            _ => input.at::<2>(),
        };
        f.publish(
            &format!("rgb{c}"),
            counted::normalize_color(&f, f.read(address)?)?,
        )?;
    }
    let report = f.finish();
    report.audit()?;
    Ok(std::array::from_fn(|i| report.outputs[i].raw as u8))
}
// Fault conversion stays at the IP boundary; no primitive runtime value enters Fixed.
impl From<audited::Fault> for super::Error {
    fn from(f: audited::Fault) -> Self {
        Self(format!("closed payload: {f:?}"))
    }
}
