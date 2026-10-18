//! Cold-boot fixture: zero statics depend on Boot, nonzero statics on RCC.
use crate::dsl_rt::*;
static ZEROS: Buf<u16, 4> = Buf::new([0; 4]);
static MIXED: Buf<u16, 4> = Buf::new([0, 0x1234, 0, 0xabcd]);
fn main() {
    let zeros = ZEROS.as_array();
    let mixed = MIXED.as_array();
    if zeros[0u16] != 0 || zeros[1u16] != 0 || zeros[2u16] != 0 || zeros[3u16] != 0 {
        halt(1);
    }
    if mixed[0u16] != 0 || mixed[1u16] != 0x1234 || mixed[2u16] != 0 || mixed[3u16] != 0xabcd {
        halt(2);
    }
    let value = fix32::from_int(mixed[1u16] as i16);
    let doubled = value * fix32::from_int(2);
    if doubled.to_int() != 0x2468 {
        halt(3);
    }
    halt(0x5a35);
}
