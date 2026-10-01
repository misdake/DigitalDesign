//! Generate static formats and ROM literals from the component's single source.
use std::{fmt::Write as _, fs, path::PathBuf};

fn rows(path: &str) -> Vec<Vec<String>> {
    println!("cargo:rerun-if-changed={path}");
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|s| !s.trim().is_empty() && !s.starts_with('#'))
        .map(|s| s.split(',').map(str::to_owned).collect())
        .collect()
}

// x^s / 2^(15*(s-1)), RNE, using integer limbs. No floating power endpoints.
fn power_endpoint(x: u32, s: u32) -> u32 {
    let mut limbs = [0_u64; 17];
    limbs[0] = 1;
    for _ in 0..s {
        let mut carry = 0_u128;
        for limb in &mut limbs {
            let product = u128::from(*limb) * u128::from(x) + carry;
            *limb = product as u64;
            carry = product >> 64;
        }
        assert_eq!(carry, 0);
    }
    let shift = 15 * (s - 1);
    let bit = |i: u32| (limbs[(i / 64) as usize] >> (i % 64)) & 1;
    let floor = (0..16).fold(0, |v, i| v | ((bit(shift + i) as u32) << i));
    let guard = bit(shift - 1) != 0;
    let sticky = (0..shift - 1).any(|i| bit(i) != 0);
    floor + u32::from(guard && (sticky || floor & 1 != 0))
}

fn emit_table(out: &mut String, name: &str, ty: &str, values: &[u64]) {
    writeln!(
        out,
        "pub const {name}_RAW: [u64; {}] = {values:?};",
        values.len()
    )
    .unwrap();
    writeln!(out, "pub static {name}: [{ty}; {}] = [", values.len()).unwrap();
    for value in values {
        writeln!(out, "{ty}::constant::<{value}>(),").unwrap();
    }
    writeln!(out, "]; ").unwrap();
}

fn main() {
    let mut out = String::new();
    for row in rows("spec/lighting-formats.csv") {
        assert_eq!(row.len(), 4);
        let name = &row[0];
        let bits: u32 = row[1].parse().unwrap();
        let fraction: u32 = row[2].parse().unwrap();
        let signed: bool = row[3].parse().unwrap();
        assert!(bits > 0 && bits <= 126 && fraction <= 126);
        writeln!(
            out,
            "pub type {name} = audited::Fixed<{bits},{fraction},{signed}>;"
        )
        .unwrap();
    }
    emit_table(
        &mut out,
        "SQUARE",
        "SquareEntry",
        &(0..128).map(|a| a * a).collect::<Vec<_>>(),
    );
    let mut rsqrt = Vec::new();
    for parity in 0..2 {
        for segment in 0..64 {
            let endpoint = |i: u32| {
                (32768.0 / ((1.0 + f64::from(i) / 64.0) * f64::from(1_u32 << parity)).sqrt())
                    .round_ties_even() as u64
            };
            let base = endpoint(segment);
            let delta = base - endpoint(segment + 1);
            assert!(delta < 256);
            rsqrt.push(base | (delta << 16));
        }
    }
    emit_table(&mut out, "RSQRT", "ReciprocalEntry", &rsqrt);
    let mut power = Vec::new();
    let mut contexts = Vec::new();
    let mut params = Vec::new();
    for row in rows("spec/power-segments.csv") {
        let r: Vec<u32> = row.iter().map(|v| v.parse().unwrap()).collect();
        let [s, boundary, wide, fine] = r[..] else {
            panic!("power row")
        };
        let offset = power.len() as u32;
        params.push((s, boundary, wide, fine, offset));
        let base_f = (offset + (boundary >> wide) + 1024 - (boundary >> fine)) & 1023;
        contexts.push(
            u64::from(boundary)
                | (u64::from(wide) << 15)
                | (u64::from(fine) << 19)
                | (u64::from(offset) << 23)
                | (u64::from(base_f) << 33),
        );
        for (begin, end, shift) in [(0, boundary, wide), (boundary, 32768, fine)] {
            assert_eq!(begin % (1 << shift), 0);
            assert_eq!(end % (1 << shift), 0);
            for x in (begin..end).step_by(1 << shift) {
                let l = power_endpoint(x, s);
                let r = power_endpoint(x + (1 << shift), s);
                assert!(r >= l && r - l < 4096);
                power.push(u64::from(l) | (u64::from(r - l) << 16));
            }
        }
    }
    assert_eq!(power.len(), 886);
    writeln!(
        out,
        "pub const POWER_PARAMS: [(u32,u32,u32,u32,u32);17] = {params:?};"
    )
    .unwrap();
    emit_table(&mut out, "POWER", "PowerEntry", &power);
    emit_table(&mut out, "CONTEXT", "PowerContext", &contexts);
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("lighting.rs"),
        out,
    )
    .unwrap();
}
