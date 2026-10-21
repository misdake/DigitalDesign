//! Immutable reciprocal/log constants embedded in the existing work BSRAM.
use std::fmt::Write;

fn round_ratio(n: u64, d: u64) -> u64 {
    let (q, r) = (n / d, n % d);
    q + u64::from(2 * r > d || (2 * r == d && q & 1 != 0))
}

fn words() -> [u64; 256] {
    std::array::from_fn(|j| {
        let seed = round_ratio(1 << 25, 256 + j as u64);
        let next = round_ratio(1 << 25, 257 + j as u64);
        let log = ((1.0 + (j / 4) as f64 / 64.0).log2() * 256.0).round() as u64;
        (seed - next) | (seed << 9) | (log << 27)
    })
}

pub(super) fn verilog() -> String {
    let mut values = String::new();
    for (address, word) in words().iter().enumerate() {
        writeln!(values, "    {address}: constant_value=36'h{word:09x};").unwrap();
    }
    let blocks: Vec<_> = (0..32)
        .map(|i| format!("  .INIT_RAM_{i:02X}(constant_block({}))", i * 8))
        .collect();
    include_str!("vliw/primitives/numeric_ram.v.in")
        .replace("@VALUES@", values.trim_end())
        .replace("@BLOCKS@", &blocks.join(",\n"))
}
