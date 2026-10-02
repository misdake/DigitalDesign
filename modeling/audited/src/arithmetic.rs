//! The only implementation of numerical operations. No division operation exists.
//! Primitive carriers are private here; every result enters the event ledger.
use super::sealed::Sealed;
use super::*;

impl Frame<'_> {
    /// Same-format helpers infer the result format without adding an event layer.
    pub fn add_same<const B: u32, const F: u32, const S: bool>(
        &self,
        a: Fixed<B, F, S>,
        b: Fixed<B, F, S>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.add(a, b)
    }
    pub fn sub_same<const B: u32, const F: u32, const S: bool>(
        &self,
        a: Fixed<B, F, S>,
        b: Fixed<B, F, S>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.sub(a, b)
    }
    fn typed<const B: u32, const F: u32, const S: bool>(
        &self,
        v: Operand,
    ) -> Result<Fixed<B, F, S>, Fault> {
        if v.format != Fixed::<B, F, S>::FORMAT {
            return self.fail(Fault::Format);
        }
        Ok(Fixed {
            bits: v.bits,
            origin: v.origin,
        })
    }
    fn binary(
        &self,
        a: Operand,
        b: Operand,
        out: Format,
        subtract: bool,
    ) -> Result<Operand, Fault> {
        if a.format.fraction != b.format.fraction || out.fraction != a.format.fraction {
            return self.fail(Fault::Format);
        }
        let width = a.format.bits.max(b.format.bits).max(out.bits);
        let resource = self.unit_for(Resource::Adder, width)?;
        let raw = if subtract {
            a.bits.checked_sub(b.bits)
        } else {
            a.bits.checked_add(b.bits)
        };
        let Some(raw) = raw else {
            return self.fail(Fault::Range);
        };
        self.dynamic(
            if subtract {
                Operation::Sub
            } else {
                Operation::Add
            },
            Some(resource),
            &[a, b],
            out,
            raw,
        )
    }
    pub fn add<const B: u32, const F: u32, const S: bool>(
        &self,
        a: impl FixedValue,
        b: impl FixedValue,
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.typed(self.binary(a.operand(), b.operand(), Fixed::<B, F, S>::FORMAT, false)?)
    }
    pub fn sub<const B: u32, const F: u32, const S: bool>(
        &self,
        a: impl FixedValue,
        b: impl FixedValue,
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.typed(self.binary(a.operand(), b.operand(), Fixed::<B, F, S>::FORMAT, true)?)
    }
    fn resize(&self, a: Operand, format: Format) -> Result<Operand, Fault> {
        if a.format.fraction != format.fraction {
            return self.fail(Fault::Format);
        }
        // Checked narrowing is a model contract check, not a saturation circuit.
        // Legal values use wiring. A failed check invalidates the entire frame.
        self.dynamic(Operation::Resize, None, &[a], format, a.bits)
    }
    pub fn resize_exact<const B: u32, const F: u32, const S: bool>(
        &self,
        a: impl FixedValue,
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.typed(self.resize(a.operand(), Fixed::<B, F, S>::FORMAT)?)
    }
    /// A statically chosen power-of-two scale, represented by the binary point.
    pub fn binary_scale<const B: u32, const F: u32, const S: bool>(
        &self,
        a: impl FixedValue,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let a = a.operand();
        let out = Fixed::<B, F, S>::FORMAT;
        if out.bits != a.format.bits || out.signed != a.format.signed {
            return self.fail(Fault::Format);
        }
        self.typed(self.dynamic(Operation::BinaryScale, None, &[a], out, a.bits)?)
    }
    pub fn shift_left_const<const SHIFT: u32, const B: u32, const F: u32, const S: bool>(
        &self,
        a: Fixed<B, F, S>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.typed(self.left_shift(a.operand(), SHIFT)?)
    }
    fn left_shift(&self, a: Operand, shift: u32) -> Result<Operand, Fault> {
        if shift >= 127 {
            return self.fail(Fault::Format);
        }
        let Some(raw) = a.bits.checked_mul(1_i128 << shift) else {
            return self.fail(Fault::Range);
        };
        self.dynamic(Operation::ShiftLeft(shift), None, &[a], a.format, raw)
    }
    /// Positive amount shifts left; negative shifts right (arithmetic for signed data).
    /// This is a clocked variable shifter, never classified as static wiring.
    pub fn shift<const B: u32, const F: u32, const S: bool>(
        &self,
        a: Fixed<B, F, S>,
        amount: Fixed<18, 0, true>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        if !(-126..=126).contains(&amount.bits) {
            return self.fail(Fault::Range);
        }
        let raw = if amount.bits >= 0 {
            a.bits.checked_mul(1_i128 << amount.bits as u32)
        } else {
            Some(a.bits >> (-amount.bits) as u32)
        };
        let Some(raw) = raw else {
            return self.fail(Fault::Range);
        };
        let resource = self.unit_for(Resource::Shift, B)?;
        self.typed(self.dynamic(
            Operation::Shift,
            Some(resource),
            &[a.operand(), amount.operand()],
            Fixed::<B, F, S>::FORMAT,
            raw,
        )?)
    }
    /// Unsigned leading-zero detector. Zero has B leading zeros; a caller may guard it.
    pub fn leading_zeros<const B: u32, const F: u32>(
        &self,
        a: Fixed<B, F, false>,
    ) -> Result<Fixed<18, 0, true>, Fault> {
        if !Fixed::<B, F, false>::FORMAT.valid() {
            return self.fail(Fault::Format);
        }
        let resource = self.unit_for(Resource::LeadingZeros, B)?;
        let raw = (a.bits as u128).leading_zeros() - (128 - B);
        self.typed(self.dynamic(
            Operation::LeadingZeros,
            Some(resource),
            &[a.operand()],
            Fixed::<18, 0, true>::FORMAT,
            i128::from(raw),
        )?)
    }
    /// Normalize positive U18 data to a Q16 mantissa and a signed binary exponent.
    /// The top-bit case discards one low bit; all other cases are exact wiring shifts.
    /// Every check, leading-zero count, exponent add/sub and dynamic shift is audited.
    pub fn normalize_positive<const F: u32>(
        &self,
        a: Fixed<18, F, false>,
    ) -> Result<(Fixed<18, 16, false>, Fixed<18, 0, true>), Fault> {
        self.require::<true>(self.less(Fixed::<18, F, false>::constant::<0>(), a)?)?;
        let zeros = self.leading_zeros(a)?;
        let amount = self.sub_same(zeros, Fixed::<18, 0, true>::constant::<1>())?;
        let mantissa = self.binary_scale(self.shift(a, amount)?)?;
        let exponent = self.sub_same(Fixed::<18, F, false>::MSB_EXPONENT, zeros)?;
        Ok((mantissa, exponent))
    }
    pub fn slice<const B: u32, const F: u32, const S: bool, const LOW: u32>(
        &self,
        a: impl FixedValue,
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.typed(self.slice_value(a.operand(), Fixed::<B, F, S>::FORMAT, LOW)?)
    }
    fn slice_value(&self, a: Operand, out: Format, low: u32) -> Result<Operand, Fault> {
        if !out.valid() || low + out.bits > a.format.bits {
            return self.fail(Fault::Format);
        }
        let bits = (a.bits >> low) & ((1_i128 << out.bits) - 1);
        let raw = if out.signed && bits & (1_i128 << (out.bits - 1)) != 0 {
            bits - (1_i128 << out.bits)
        } else {
            bits
        };
        self.dynamic(Operation::Slice(low), None, &[a], out, raw)
    }
    fn physical_product(
        &self,
        a: Operand,
        b: Operand,
        resource: Resource,
    ) -> Result<Operand, Fault> {
        let cap = match resource {
            Resource::Dsp18 => 18,
            Resource::Dsp36 => 36,
            _ => return self.fail(Fault::MissingResource),
        };
        if a.format.bits > cap || b.format.bits > cap {
            return self.fail(Fault::Format);
        }
        let format = Format {
            bits: a.format.bits + b.format.bits,
            fraction: a.format.fraction + b.format.fraction,
            signed: a.format.signed || b.format.signed,
        };
        let Some(raw) = a.bits.checked_mul(b.bits) else {
            return self.fail(Fault::Range);
        };
        self.dynamic(Operation::Multiply, Some(resource), &[a, b], format, raw)
    }
    /// A logical product always records its actual DSP lowering.
    /// Native18Pair includes two DSP returns, shift wiring and a counted wide add.
    pub fn mul<const B: u32, const F: u32, const S: bool>(
        &self,
        a: impl FixedValue,
        b: impl FixedValue,
        route: ProductRoute,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let (_, a) = self.resolve(a)?;
        let (_, b) = self.resolve(b)?;
        let out = Fixed::<B, F, S>::FORMAT;
        if out.bits != a.format.bits + b.format.bits
            || out.fraction != a.format.fraction + b.format.fraction
            || out.signed != (a.format.signed || b.format.signed)
        {
            return self.fail(Fault::Format);
        }
        let result = match route {
            ProductRoute::Native18 => self.physical_product(a, b, Resource::Dsp18)?,
            ProductRoute::Wide36 => {
                if a.format.bits > 36 || b.format.bits > 36 {
                    return self.fail(Fault::Format);
                }
                let x = self.resize(
                    a,
                    Format {
                        bits: 36,
                        ..a.format
                    },
                )?;
                let y = self.resize(
                    b,
                    Format {
                        bits: 36,
                        ..b.format
                    },
                )?;
                let p = self.physical_product(x, y, Resource::Dsp36)?;
                self.resize(p, out)?
            }
            ProductRoute::Native18Pair => {
                let (wide, narrow) = if (19..=36).contains(&a.format.bits) && b.format.bits <= 18 {
                    (a, b)
                } else if (19..=36).contains(&b.format.bits) && a.format.bits <= 18 {
                    (b, a)
                } else {
                    return self.fail(Fault::Format);
                };
                let hi = self.slice_value(
                    wide,
                    Format {
                        bits: wide.format.bits - 18,
                        ..wide.format
                    },
                    18,
                )?;
                let lo = self.slice_value(
                    wide,
                    Format {
                        bits: 18,
                        signed: false,
                        ..wide.format
                    },
                    0,
                )?;
                let high = self.physical_product(hi, narrow, Resource::Dsp18)?;
                let low = self.physical_product(lo, narrow, Resource::Dsp18)?;
                let high = self.resize(high, out)?;
                let high = self.left_shift(high, 18)?;
                let low = self.resize(low, out)?;
                self.binary(high, low, out, false)?
            }
        };
        let inputs = [
            self.resolve(a)?.0,
            self.resolve(b)?.0,
            self.resolve(result)?.0,
        ];
        self.emit(
            Operation::ProductMapping {
                a_bits: a.format.bits,
                b_bits: b.format.bits,
                route,
            },
            None,
            &inputs,
            None,
            0,
        )?;
        self.typed(result)
    }
    /// Infer output type from a typed destination; choose a fully counted lowering.
    /// Narrow x narrow uses one DSP18, wide x narrow uses two DSP18 returns,
    /// and wide x wide uses one DSP36. Explicit `mul` still overrides the route.
    pub fn product<const B: u32, const F: u32, const S: bool>(
        &self,
        a: impl FixedValue,
        b: impl FixedValue,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let a = a.operand();
        let b = b.operand();
        let route = if a.format.bits <= 18 && b.format.bits <= 18 {
            ProductRoute::Native18
        } else if a.format.bits <= 36
            && b.format.bits <= 36
            && (a.format.bits <= 18 || b.format.bits <= 18)
        {
            ProductRoute::Native18Pair
        } else {
            ProductRoute::Wide36
        };
        self.mul(a, b, route)
    }
    pub fn less(
        &self,
        a: impl FixedValue,
        b: impl FixedValue,
    ) -> Result<Fixed<1, 0, false>, Fault> {
        let a = a.operand();
        let b = b.operand();
        if a.format.fraction != b.format.fraction {
            return self.fail(Fault::Format);
        }
        let resource = self.unit_for(Resource::Compare, a.format.bits.max(b.format.bits))?;
        self.typed(self.dynamic(
            Operation::Less,
            Some(resource),
            &[a, b],
            Fixed::<1, 0, false>::FORMAT,
            i128::from(a.bits < b.bits),
        )?)
    }
    pub fn select<const B: u32, const F: u32, const S: bool>(
        &self,
        p: Fixed<1, 0, false>,
        a: Fixed<B, F, S>,
        b: Fixed<B, F, S>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let raw = if p.bits != 0 { a.bits } else { b.bits };
        self.typed(self.dynamic(
            Operation::Select,
            Some(self.unit_for(Resource::Select, B)?),
            &[p.operand(), a.operand(), b.operand()],
            Fixed::<B, F, S>::FORMAT,
            raw,
        )?)
    }
    /// Fixed-format ties-even rounding: explicit bit wiring and round control,
    /// followed by a counted adder (including +0) and checked final narrowing.
    /// A guard bit is retained whenever the source domain can cross an output edge.
    pub fn round_to<const B: u32, const F: u32, const S: bool>(
        &self,
        a: impl FixedValue,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let a = a.operand();
        let out = Fixed::<B, F, S>::FORMAT;
        if a.format.fraction < F {
            return self.fail(Fault::Format);
        }
        let shift = a.format.fraction - F;
        if shift == 0 {
            return self.resize_exact(a);
        }
        if shift >= 127 {
            return self.fail(Fault::Format);
        }
        let base = a.bits >> shift;
        let rem = a.bits & ((1_i128 << shift) - 1);
        let half = 1_i128 << (shift - 1);
        let signed = a.format.signed || out.signed;
        let needed =
            a.format.bits.saturating_sub(shift).max(1) + u32::from(!a.format.signed && signed);
        let intermediate = Format {
            bits: B + u32::from(needed > B),
            fraction: F,
            signed,
        };
        let floor = self.dynamic(
            Operation::RescaleFloor(shift),
            None,
            &[a],
            intermediate,
            base,
        )?;
        let resource = self.unit_for(Resource::RoundControl, a.format.bits)?;
        let increment = self.dynamic(
            Operation::RoundIncrement(shift),
            Some(resource),
            &[a],
            Format {
                bits: 1,
                fraction: F,
                signed: false,
            },
            i128::from(rem > half || rem == half && base & 1 != 0),
        )?;
        let rounded = self.binary(floor, increment, intermediate, false)?;
        self.typed(self.resize(rounded, out)?)
    }
}
