// bench-max-cycles: 10000
// bench-expected-halt: 1
fn main() {
    let mut x: u16 = 0;
    let mut i: u16 = 0;
    while i < 48 {
        if (i & 3) == 0 { x = x + 7; }
        else if (i & 1) == 0 { x = x ^ i; }
        else { x = x - 1; }
        i = i + 1;
    }
    halt(1);
}

