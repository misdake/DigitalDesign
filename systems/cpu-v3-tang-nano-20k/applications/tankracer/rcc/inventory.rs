//! Original weapon selection, signed-u8 ammunition cap and collection cooldown.
use crate::dsl_rt::*;
use crate::{attitude, integer};

// State: original u32 weapon followed by one u8 ammo word.
pub fn collect(mut state: Array<u16>, kind: fix32, amounts: Array<u16>) -> fix32 {
    if kind.hi_bits() != 0 || kind.lo_bits() >= 11 {
        return integer::from_i16(-1);
    }
    let amount = amounts[kind.lo_bits()];
    if attitude::get(state, 0) != kind {
        attitude::put(state, 0, kind);
        state[2u16] = amount;
    } else if kind == integer::from_i16(9) {
        state[2u16] = amount;
    } else {
        let mut ammo = (state[2u16] + amount) & 255;
        // Values 128..255 retain their original signed-byte comparison.
        if ammo > 3 && ammo < 128 {
            ammo = 3;
        }
        state[2u16] = ammo;
    }
    fix32::zero()
}
pub fn collected(state: Array<u16>, cooldown: Array<u16>) {
    let weapon = attitude::get(state, 0);
    if (weapon == integer::from_i16(1)
        || weapon == integer::from_i16(2)
        || weapon == integer::from_i16(7)
        || weapon == integer::from_i16(8))
        && attitude::get(cooldown, 0) > integer::from_i16(300)
    {
        attitude::put(cooldown, 0, integer::from_i16(300));
    }
}
