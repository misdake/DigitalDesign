//! Latest-only autonomous Device5 input. No transport timer or wire waits.
use crate::dsl_rt::*;

// State[10]: connected, ID, active-high buttons, RX,RY,LX,LY,
// previous buttons, pressed, released. Invalid/digital axes are neutral128.
pub fn ds2_disconnect(mut s: Array<u16>) {
    s[0u16] = 0;
    s[1u16] = 0;
    s[2u16] = 0;
    s[7u16] = 0;
    s[8u16] = 0;
    s[9u16] = 0;
    let mut i: u16 = 3;
    while i < 7 {
        s[i] = 128;
        i += 1;
    }
}

pub fn ds2_init(mut s: Array<u16>) {
    ds2_disconnect(s);
}

// At most one read attempt per call. A concurrent publication requests a retry
// on the next call; it never blocks acquisition or publishes a partial sample.
// rx[9] contains the FF prefix followed by the eight retained wire bytes.
pub fn ds2_service(mut s: Array<u16>, mut rx: Array<u16>) -> bool {
    s[8u16] = 0;
    s[9u16] = 0;
    let status = dev_recv(5, 8);
    if (status & 1) == 0 {
        ds2_disconnect(s);
        return false;
    }
    if (status & 2) == 0 && s[0u16] != 0 {
        return false;
    }
    dev_send(5, 8, 0);
    rx[0u16] = 255;
    // Physical RAM addresses follow the serial slot, with no hardware reorder.
    rx[1u16] = dev_recv(5, 2);
    rx[2u16] = dev_recv(5, 3);
    rx[3u16] = dev_recv(5, 4);
    rx[4u16] = dev_recv(5, 5);
    rx[5u16] = dev_recv(5, 6);
    rx[6u16] = dev_recv(5, 7);
    rx[7u16] = dev_recv(5, 0);
    rx[8u16] = dev_recv(5, 1);
    let after = dev_recv(5, 8);
    if (after & 1) == 0 {
        ds2_disconnect(s);
        return false;
    }
    if (after & 2) != 0 {
        return false;
    }
    let id = rx[1u16];
    let buttons = (rx[3u16] | (rx[4u16] << 8)) ^ 65535;
    if s[0u16] != 0 {
        s[8u16] = buttons & (s[7u16] ^ 65535);
        s[9u16] = s[7u16] & (buttons ^ 65535);
    }
    s[0u16] = 1;
    s[1u16] = id;
    s[2u16] = buttons;
    s[7u16] = buttons;
    let mut i: u16 = 3;
    while i < 7 {
        if id == 65 {
            s[i] = 128;
        } else {
            s[i] = rx[i + 2];
        }
        i += 1;
    }
    return true;
}

pub fn ds2_axis(raw: u16, deadzone: u16) -> i16 {
    let value = (raw as i16) - 128;
    if value >= -(deadzone as i16) && value <= deadzone as i16 {
        return 0;
    }
    return value;
}
