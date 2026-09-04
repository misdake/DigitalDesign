// bench-max-cycles: 10000
// bench-expected-halt: 1
fn main() {
    let mut x: u16 = 0x1357;
    let mut y: u16 = 0x2468;
    let mut i: u16 = 0;
    while i < 24 {
        x = (x + y) ^ (x << 1);
        y = (y + 3) ^ (x >> 2);
        i = i + 1;
    }
    halt(1);
}

