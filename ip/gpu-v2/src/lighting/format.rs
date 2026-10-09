//! Generated static signal formats and ROMs. Edit spec/*.csv, not generated code.
#![allow(dead_code)]
include!(concat!(env!("OUT_DIR"), "/lighting.rs"));

#[cfg(test)]
mod tests {
    use super::{CONTEXT_RAW, POWER_PARAMS, POWER_RAW, RSQRT_RAW, SQUARE_SIGNED_RAW};

    #[test]
    fn normalized_square_sum_has_only_four_leading_zero_counts() {
        // Independent integer RNE, including negative half ties.
        let scaled = |x: i64, shift: i32| {
            if shift >= 0 {
                x << shift
            } else {
                let divisor = 1_i64 << -shift;
                let floor = x.div_euclid(divisor);
                let rem = x.rem_euclid(divisor);
                floor + i64::from(2 * rem > divisor || 2 * rem == divisor && floor & 1 != 0)
            }
        };
        for (threshold, maximum, adjust) in [(4, 32768, true), (64, 16384, false)] {
            for magnitude in 0_i64..=maximum {
                let safe = if magnitude < threshold {
                    8192
                } else {
                    magnitude
                };
                let z = 16 - (63 - safe.leading_zeros() as i32);
                let mut shift = z - 3;
                if adjust && scaled(safe, shift) >= 16384 {
                    shift -= 1;
                }
                if adjust {
                    let optimized_shift = if safe == 32767 { -2 } else { z - 3 };
                    assert_eq!(shift, optimized_shift, "guard m={magnitude}");
                }
                assert!(shift >= -2 && shift <= if adjust { 11 } else { 7 });
                let p = scaled(safe, shift);
                let n = scaled(-safe, shift);
                assert!((8192..=16384).contains(&p), "m={magnitude},k={shift},p={p}");
                assert_eq!(n, -p);
                // RNE is monotone; endpoints bound every component in [-safe,safe].
                assert!(n >= -16384 && p <= 16384);
            }
        }
        for x in -16384_i64..=16384 {
            let index = x.div_euclid(128);
            let tail = x.rem_euclid(128);
            // The positive +16384 endpoint aliases the negative SQ endpoint;
            // tail=0 makes their squares identical.
            let signed_index = (index as i8) as i64;
            let entry = SQUARE_SIGNED_RAW[(index & 255) as usize] as i64;
            let chord = (entry << 14) + (((2 * signed_index + 1) * tail) << 7);
            assert!(chord >= x * x && chord <= (1 << 28), "x={x},q={chord}");
            if x.abs() >= 8192 {
                assert!(chord >= (1 << 26));
            }
        }
        assert_eq!((1_u32 << 26).leading_zeros() - 2, 3);
        assert_eq!((3_u32 << 28).leading_zeros() - 2, 0);
        // Projection ports require 8192<=Vz<=16384 and |Vx|,|Vy|<=16384,
        // giving the same chord bounds without a pre-scale stage.
    }

    #[test]
    fn documented_power_contexts_address_only_their_own_segments() {
        let exponents = [
            4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64,
        ];
        let offsets = [
            0, 23, 57, 91, 125, 159, 209, 259, 303, 347, 414, 475, 535, 595, 673, 744, 815, 886,
        ];
        assert_eq!(POWER_RAW.len(), 886);
        assert_eq!(CONTEXT_RAW.len(), 17);
        assert_eq!(POWER_RAW.iter().map(|entry| entry >> 16).max(), Some(3908));
        for code in 0..17 {
            let (s, boundary, wide, fine, offset) = POWER_PARAMS[code];
            assert_eq!(s, exponents[code]);
            assert_eq!(offset, offsets[code]);
            let context = CONTEXT_RAW[code];
            assert_eq!(context >> 43, 0);
            assert_eq!(context & 32767, u64::from(boundary));
            assert_eq!((context >> 15) & 15, u64::from(wide));
            assert_eq!((context >> 19) & 15, u64::from(fine));
            for x in 0..32768_u32 {
                let shift = if x < boundary { wide } else { fine };
                let base = if x < boundary {
                    (context >> 23) & 1023
                } else {
                    (context >> 33) & 1023
                } as u32;
                let address = (base + (x >> shift)) & 1023;
                // Independent piecewise indexing, including negative fine bases.
                let expected = if x < boundary {
                    offset + x / (1 << wide)
                } else {
                    offset + boundary / (1 << wide) + (x - boundary) / (1 << fine)
                };
                assert_eq!(address, expected, "code={code},x={x}");
                assert!((offsets[code]..offsets[code + 1]).contains(&address));
                assert_eq!(x & ((1 << shift) - 1), x - ((x >> shift) << shift));
            }
        }
    }

    #[test]
    fn documented_rsqrt_pages_are_monotone_and_meet_interpolation_error() {
        assert_eq!(RSQRT_RAW.len(), 128);
        for parity in 0..2 {
            let mut previous = u64::MAX;
            for segment in 0..64 {
                let entry = RSQRT_RAW[parity * 64 + segment];
                assert_eq!(entry >> 24, 0);
                let base = entry & 65535;
                let delta = entry >> 16;
                for fraction in 0..256_u64 {
                    let product = delta * fraction;
                    let q = product / 256;
                    let remainder = product % 256;
                    let correction =
                        q + u64::from(remainder > 128 || remainder == 128 && q & 1 != 0);
                    let actual = base - correction;
                    assert!(actual <= previous);
                    previous = actual;
                    let t = 1.0 + (segment as f64 + fraction as f64 / 256.0) / 64.0;
                    let ideal = 32768.0 / (t * (1_u32 << parity) as f64).sqrt();
                    // Chord curvature plus endpoint/interpolation RNE: <2 Q15 codes.
                    assert!((actual as f64 - ideal).abs() < 2.0);
                }
            }
        }
    }
    #[test]
    fn signed_square_chords_equal_magnitude_chords_over_the_complete_domain() {
        for x in -16384_i64..=16383 {
            let a = x.div_euclid(128);
            let b = x.rem_euclid(128);
            let entry = SQUARE_SIGNED_RAW[a.rem_euclid(256) as usize];
            let base = entry as i64;
            let slope = 2 * a + 1;
            assert_eq!(base, a * a);
            let actual = (base << 14) + ((slope * b) << 7);
            let u = x.abs();
            let high = u / 128;
            let tail = u % 128;
            let expected = ((high * high) << 14) + (((2 * high + 1) * tail) << 7);
            assert_eq!(actual, expected, "x={x}");
        }
    }
}

#[cfg(test)]
mod compensation_tests {
    #[test]
    fn midpoint_rom_keeps_delta_capacity_and_endpoint_encoding() {
        use super::*;
        assert_eq!(POWER_RAW.len(), POWER_MIDPOINT_RAW.len());
        for (&raw, &biased) in POWER_RAW.iter().zip(POWER_MIDPOINT_RAW.iter()) {
            assert_eq!(raw >> 16, biased >> 16);
            assert_eq!((raw & 65535) + 64, biased & 65535);
            assert!(biased < 1 << 28);
        }
        for p in 0_i128..=32768 {
            assert_eq!((p + 64) / 128, p / 128 + i128::from(p % 128 >= 64));
        }
    }
}
