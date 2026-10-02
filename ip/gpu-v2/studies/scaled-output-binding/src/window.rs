use audited::Fault;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub fraction: u32,
    pub bits: u32,
    pub out_fraction: u32,
}

pub const SPECS: [Spec; 8] = [
    Spec {
        fraction: 46,
        bits: 36,
        out_fraction: 28,
    },
    Spec {
        fraction: 60,
        bits: 36,
        out_fraction: 28,
    },
    Spec {
        fraction: 28,
        bits: 36,
        out_fraction: 24,
    },
    Spec {
        fraction: 60,
        bits: 36,
        out_fraction: 24,
    },
    Spec {
        fraction: 28,
        bits: 18,
        out_fraction: 17,
    },
    Spec {
        fraction: 60,
        bits: 18,
        out_fraction: 17,
    },
    Spec {
        fraction: 28,
        bits: 10,
        out_fraction: 9,
    },
    Spec {
        fraction: 60,
        bits: 10,
        out_fraction: 9,
    },
];

fn fits(x: i128, bits: u32) -> bool {
    x >= -(1_i128 << (bits - 1)) && x < 1_i128 << (bits - 1)
}

/// The first registered boundary carries a bounded floor and two rounding bits.
/// Rejected inputs never produce a valid packet. Model flags are not FPGA area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub floor: i128,
    pub guard: bool,
    pub sticky: bool,
    intermediate_bits: u32,
    out_bits: u32,
}

impl Window {
    pub fn payload_bits(self) -> u32 {
        // G/S + valid/range status + three-bit format control (eight formats).
        self.intermediate_bits + 6
    }

    pub fn finish(self) -> Result<i128, Fault> {
        let increment = self.guard && (self.sticky || self.floor & 1 != 0);
        let rounded = self.floor + i128::from(increment);
        if !fits(rounded, self.intermediate_bits) || !fits(rounded, self.out_bits) {
            return Err(Fault::Range);
        }
        Ok(rounded)
    }
}

/// Integer bit-window formulation, independent of the original jam/recover graph.
/// All eager range failures of that graph are retained, including unused +1.
pub fn prepare(spec: Spec, value: i128, exponent: i128) -> Result<Window, Fault> {
    if !SPECS.contains(&spec) {
        return Err(Fault::Format);
    }
    if !fits(value, 72) || !fits(exponent, 18) || exponent >= 32 {
        return Err(Fault::Range);
    }
    let amount = -exponent;
    // Preserve the signed 18-bit negation and both dynamic shift amount guards.
    if !fits(amount, 18) || !(-126..=126).contains(&amount) {
        return Err(Fault::Range);
    }
    let shifted = if amount >= 0 {
        value
            .checked_mul(1_i128 << amount as u32)
            .ok_or(Fault::Range)?
    } else {
        value >> (-amount) as u32
    };
    if !fits(shifted, 72) {
        return Err(Fault::Range);
    }
    // Recovery is exact for left scaling. Right scaling has a nonnegative
    // remainder < 2^31, and its arithmetic-floor recovery still fits signed72.
    // The eagerly computed +1 is checked even when jam/select never uses it.
    if shifted == (1_i128 << 71) - 1 {
        return Err(Fault::Range);
    }
    let fixed_drop = spec.fraction - spec.out_fraction;
    let total_drop = i128::from(fixed_drop) + exponent;
    let (floor, guard, sticky) = if total_drop > 0 {
        let drop = total_drop as u32;
        (
            value >> drop,
            value >> (drop - 1) & 1 != 0,
            value & ((1_i128 << (drop - 1)) - 1) != 0,
        )
    } else {
        (
            value
                .checked_mul(1_i128 << (-total_drop) as u32)
                .ok_or(Fault::Range)?,
            false,
            false,
        )
    };
    let needed = 72_u32.saturating_sub(fixed_drop).max(1);
    let intermediate_bits = spec.bits + u32::from(needed > spec.bits);
    // Original RescaleFloor narrows before the rounding add. Do not merely
    // check the final output; a near-boundary floor can fail earlier.
    if !fits(floor, intermediate_bits) {
        return Err(Fault::Range);
    }
    Ok(Window {
        floor,
        guard,
        sticky,
        intermediate_bits,
        out_bits: spec.bits,
    })
}
