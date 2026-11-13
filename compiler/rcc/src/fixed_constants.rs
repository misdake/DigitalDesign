//! Shared host/constant-construction rules for signed Q16.16 values.
const fn rounded_divide(numerator: i64, denominator: i64) -> i32 {
    let bias = denominator / 2;
    let adjusted = if numerator < 0 {
        numerator - bias
    } else {
        numerator + bias
    };
    (adjusted / denominator) as i32
}

/// Nearest Q16.16 value; halfway values round away from zero.
pub const fn ratio(numerator: i16, denominator: u16) -> Option<i32> {
    if denominator == 0 {
        None
    } else {
        Some(rounded_divide((numerator as i64) << 16, denominator as i64))
    }
}

/// Integer degrees to radians, quantized once from a Q32 approximation of pi.
pub const fn degrees(value: i16) -> i32 {
    rounded_divide(value as i64 * 13_493_037_705, 180 * 65_536)
}

/// Exact decimal quantization without a floating-point intermediate.
pub const fn decimal(text: &str) -> Result<i32, &'static str> {
    let bytes = text.as_bytes();
    let negative = !bytes.is_empty() && bytes[0] == b'-';
    let start = if negative { 1 } else { 0 };
    let mut end = start;
    let mut digits: i64 = 0;
    let mut before_point: i64 = 0;
    let mut first_nonzero: i64 = -1;
    let mut point_seen = false;
    while end < bytes.len() && bytes[end] != b'e' && bytes[end] != b'E' {
        let byte = bytes[end];
        if byte >= b'0' && byte <= b'9' {
            if byte != b'0' && first_nonzero == -1 {
                first_nonzero = digits;
            }
            digits += 1;
            if !point_seen {
                before_point += 1;
            }
        } else if byte == b'.' && !point_seen {
            point_seen = true;
        } else if byte != b'_' {
            return Err("fix! requires an unsuffixed decimal literal");
        }
        end += 1;
    }
    if digits == 0 {
        return Err("fix! requires a decimal literal");
    }
    let mut exponent: i64 = 0;
    if end < bytes.len() {
        let mut index = end + 1;
        let exponent_negative = index < bytes.len() && bytes[index] == b'-';
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }
        let mut has_digit = false;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte >= b'0' && byte <= b'9' {
                has_digit = true;
                exponent = exponent
                    .saturating_mul(10)
                    .saturating_add((byte - b'0') as i64);
            } else if byte != b'_' {
                return Err("fix! requires an unsuffixed decimal literal");
            }
            index += 1;
        }
        if !has_digit {
            return Err("fix! exponent requires digits");
        }
        if exponent_negative {
            exponent = -exponent;
        }
    }
    if first_nonzero == -1 {
        return Ok(0);
    }
    let point = before_point.saturating_add(exponent);
    if point.saturating_sub(first_nonzero) > 5 {
        return Err("fix! value is outside the Q16.16 range");
    }
    // Every half-step is an integer multiple of 2^-17, whose decimal expansion
    // terminates after 17 places. Later digits cannot change ties-away rounding.
    const DECIMAL_SCALE: u128 = 100_000_000_000_000_000;
    let mut scaled: u128 = 0;
    let mut index = start;
    let mut digit_index: i64 = 0;
    while index < end {
        let byte = bytes[index];
        if byte >= b'0' && byte <= b'9' {
            let relative = point.saturating_sub(digit_index);
            if relative >= -16 && relative <= 5 {
                let mut power = 16 + relative;
                let mut place: u128 = 1;
                while power > 0 {
                    place *= 10;
                    power -= 1;
                }
                scaled += (byte - b'0') as u128 * place;
            }
            digit_index += 1;
        }
        index += 1;
    }
    let raw = (scaled * 65_536 + DECIMAL_SCALE / 2) / DECIMAL_SCALE;
    let limit = if negative {
        2_147_483_648
    } else {
        2_147_483_647
    };
    if raw > limit {
        return Err("fix! value is outside the Q16.16 range");
    }
    let signed = if negative { -(raw as i64) } else { raw as i64 };
    Ok(signed as i32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn decimal_literals_cover_sign_exponent_precision_and_boundaries() {
        for (text, expected) in [
            ("0", 0),
            ("-0.0", 0),
            ("1.", 65536),
            ("-1.25", -81920),
            ("0.001", 66),
            ("1e-3", 66),
            ("2.5E+1", 1638400),
            ("1_234.5", 80904192),
            ("-32768", i32::MIN),
            ("32767.9999847412109375", i32::MAX),
            ("0.00000762939453125", 1),
            ("-0.00000762939453125", -1),
            ("0.000007629394531249999999999999999999999", 0),
            ("0.000007629394531250000000000000000000001", 1),
            ("32767.9999923706054687499999", i32::MAX),
            ("1e-9999999999999999999999999", 0),
            ("0e9999999999999999999999999", 0),
        ] {
            assert_eq!(super::decimal(text), Ok(expected), "{text}");
        }
        let long = format!("1{}e-100", "0".repeat(100));
        assert_eq!(super::decimal(&long), Ok(65536));
        for text in [
            "32768",
            "-32769",
            "32767.99999237060546875",
            "-32768.00000762939453125",
            "1e999999999999999999999999",
        ] {
            assert!(
                super::decimal(text).unwrap_err().contains("range"),
                "{text}"
            );
        }
        for text in [
            "0x10", "1f32", "1u16", "", ".", "--1", "1e", "1e+", "12.3.4",
        ] {
            assert!(super::decimal(text).is_err(), "{text}");
        }
        const HOST: crate::dsl_rt::fix32 = crate::fix!(-1.25);
        assert_eq!(HOST.0, -81920);
    }

    #[test]
    fn decimal_half_steps_round_identically_across_the_fraction() {
        // Independent exact decimal half-step oracle: 10^17 / 2^17.
        for odd in (1..131072u64).step_by(127).filter(|n| n % 2 == 1) {
            let boundary = odd * 762_939_453_125;
            let below = format!("0.{:017}", boundary - 1);
            let exact = format!("0.{boundary:017}");
            let above = format!("0.{:017}", boundary + 1);
            let rounded = odd.div_ceil(2) as i32;
            assert_eq!(super::decimal(&below), Ok(rounded - 1));
            assert_eq!(super::decimal(&exact), Ok(rounded));
            assert_eq!(super::decimal(&above), Ok(rounded));
            assert_eq!(super::decimal(&format!("-{exact}")), Ok(-rounded));
        }
    }

    #[test]
    fn quantization_is_signed_and_uses_nearest_ties_away_from_zero() {
        assert_eq!(super::ratio(1, 1000), Some(66));
        assert_eq!(super::ratio(-1, 1000), Some(-66));
        assert_eq!(super::ratio(32767, 1), Some(0x7fff0000));
        assert_eq!(super::ratio(-32768, 1), Some(i32::MIN));
        assert_eq!(super::ratio(1, 0), None);
        assert_eq!(super::degrees(180), 205887);
        assert_eq!(super::degrees(-180), -205887);
        assert_eq!(super::degrees(90), 102944);
    }
}
