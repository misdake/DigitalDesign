fn main() {
    let mut sum: u16 = 0;
    let mut i: u16 = 5;
    while i != 0 {
        sum = sum + i;
        i = i - 1;
    }
    halt(sum);
}
