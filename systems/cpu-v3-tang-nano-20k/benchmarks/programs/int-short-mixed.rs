// bench-max-cycles: 10000
// bench-expected-halt: 1
fn mix(x0: u16, n: u16) -> u16 {
    let mut x: u16 = x0;
    let mut i: u16 = 0;
    while i < n {
        x = ((x << 3) ^ (x >> 2)) + i + 0x1234;
        if (x & 7) == 3 { x = x ^ 0xa5a5; }
        i = i + 1;
    }
    x
}
fn main() { let x = mix(7, 24); if x != 0 { halt(1); } else { halt(0); } }

