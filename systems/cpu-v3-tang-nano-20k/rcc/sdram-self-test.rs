static VALUE: u16 = 0;

fn main() {
    let mut words = addr_of(&VALUE).as_u16_array();
    words[0u16] = 0x1234;
    halt(words[0u16] + 1);
}
