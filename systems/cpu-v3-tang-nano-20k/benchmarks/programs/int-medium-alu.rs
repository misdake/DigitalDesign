// bench-max-cycles: 400000
// bench-expected-halt: 1
fn main() {
    let mut x: u16 = 1;
    let mut y: u16 = 0x9e37;
    let mut i: u16 = 0;
    while i < 1536 {
        x = (x + y) ^ (x << 5) ^ (x >> 3);
        y = y + x + i;
        i = i + 1;
    }
    halt(1);
}

