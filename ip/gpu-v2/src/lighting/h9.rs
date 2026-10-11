//! Single generated source for the explicitly selected direct-output power ROM.
pub type H9Entry = audited::Fixed<16, 0, false>;
include!(concat!(env!("OUT_DIR"), "/lighting_h9.rs"));
pub fn grid_shift(code: u8) -> i128 {
    i128::from(H9_SHIFTS_RAW[usize::from(code.min(16))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lighting::sim::pipeline;
    use audited::Model;
    #[test]
    fn h9_context_steps_endpoints_and_bin_edges() {
        for code in 0..17usize {
            let shift = H9_SHIFTS_RAW[code] as u32 + 6;
            let width = 1_i128 << shift;
            let mut cosines = vec![0, 32768, 1, 32767];
            for bin in [1, 15, 31, 32] {
                for offset in [-1, 0, 1] {
                    let cosine = 32768 - (bin * width + offset);
                    if (0..=32768).contains(&cosine) {
                        cosines.push(cosine);
                    }
                }
            }
            for cosine in cosines {
                for positive in [0, 1] {
                    let mut model = Model::numerical();
                    let x = model.input::<16, 15, false>("cosine", &[cosine]).unwrap();
                    let c = model.input::<5, 0, false>("code", &[code as i128]).unwrap();
                    let s = model
                        .input::<3, 0, false>("step", &[i128::from(shift - 6)])
                        .unwrap();
                    let p = model.input::<1, 0, false>("positive", &[positive]).unwrap();
                    let table = model.table("h9", &H9).unwrap();
                    let f = model.compute("bounded h9 endpoints", 256).unwrap();
                    let head = pipeline::h9_head(
                        &f,
                        f.read(x.at::<0>()).unwrap(),
                        f.read(c.at::<0>()).unwrap(),
                        f.read(s.at::<0>()).unwrap(),
                    )
                    .unwrap();
                    let entry = f.read(table.indexed(head.address)).unwrap();
                    let value = pipeline::h9_tail(
                        &f,
                        entry,
                        head.fraction,
                        head.far,
                        f.read(p.at::<0>()).unwrap(),
                    )
                    .unwrap();
                    f.publish("h9", value).unwrap();
                    let report = f.finish();
                    report.audit().unwrap();
                    let actual = report.values[report.outputs[0].value].raw;
                    let distance = 32768 - cosine;
                    let index = distance >> shift;
                    let word = H9_RAW[code * 32 + (index as usize & 31)];
                    let base = word & 511;
                    let delta = word >> 9;
                    assert!(delta <= 71 && delta <= base);
                    let fraction = ((distance << 6) >> shift) & 63;
                    let expected = if positive == 0 || index >= 32 {
                        0
                    } else {
                        i128::from(base) - ((i128::from(delta) * fraction + 32) >> 6)
                    };
                    assert_eq!(
                        actual, expected,
                        "code{code} cosine{cosine} positive{positive}"
                    );
                }
            }
        }
    }
}
