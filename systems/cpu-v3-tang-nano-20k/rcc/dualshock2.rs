//! Nonblocking Device5 packet driver. Include time alongside it.
use crate::dsl_rt::*;

pub fn ds2_abort() {
    dev_send(5, 0, 3);
}
// The stable 16-bit port presents two consecutive wire bytes, first in high8.
// SEND5,0,1 advances one whole word; final consumption releases the snapshot.
fn ds2_collect(mut rx: Array<u16>, header_good: bool) -> u16 {
    let first = dev_recv(5, 1);
    let id = first >> 8;
    let mut length: u16 = 0;
    if id == 65 {
        length = 5;
    }
    if id == 115 || id == 243 {
        length = 9;
    }
    if id == 121 {
        length = 21;
    }
    if !header_good || (first & 255) != 90 || length == 0 {
        ds2_abort();
        return 0;
    }
    rx[0u16] = 255;
    rx[1u16] = id;
    rx[2u16] = first & 255;
    dev_send(5, 0, 1);
    let mut at: u16 = 3;
    while at < length {
        let word = dev_recv(5, 1);
        rx[at] = word >> 8;
        rx[at + 1] = word & 255;
        dev_send(5, 0, 1);
        at += 2;
    }
    return id;
}

// State[16]: connected, ID, active-high buttons, RX,RY,LX,LY,
// previous buttons, pressed, released, config phase, failed attempts,
// last completion/start ms, delay ms, started, in-flight. Invalid axes are neutral 128.
pub fn ds2_disconnect(mut s: Array<u16>) {
    s[0u16] = 0;
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
    let mut i: u16 = 0;
    while i < 16 {
        s[i] = 0;
        i += 1;
    }
    ds2_disconnect(s);
    ds2_abort();
}

// Calls never wait; collection can immediately rearm the following packet.
// Independent millisecond timeout permits shared microsecond timer clears.
pub fn ds2_service(mut s: Array<u16>, mut rx: Array<u16>) -> bool {
    s[8u16] = 0;
    s[9u16] = 0;
    let now = time_milliseconds();
    if s[15u16] == 0 {
        if s[14u16] != 0 && time_elapsed_ms(s[12u16], now) < s[13u16] {
            return false;
        }
        let phase = s[10u16];
        let mut descriptor: u16 = 0;
        if phase == 1 {
            descriptor = 257;
        }
        if phase == 2 {
            descriptor = 258;
        }
        if phase == 3 {
            descriptor = 1;
        }
        dev_send(5, 1, descriptor);
        s[14u16] = 1;
        s[15u16] = 1;
        s[12u16] = now;
        return false;
    }
    let status = dev_recv(5, 0);
    let mut id: u16 = 0;
    if time_elapsed_ms(s[12u16], now) >= 20 || (status & 8) != 0 {
        ds2_abort();
    } else {
        if (status & 1) != 0 {
            return false;
        }
        if (status & 2) != 0 {
            id = ds2_collect(rx, (status & 16) != 0);
        } else {
            ds2_abort();
        }
    }
    let started = s[12u16];
    s[15u16] = 0;
    let phase = s[10u16];
    s[12u16] = time_milliseconds();
    s[1u16] = id;
    // Two integer ticks guarantee at least 1 ms after packet completion.
    s[13u16] = 2;
    if id == 0 || (phase == 2 && id != 243) || (phase == 3 && id != 243) {
        ds2_disconnect(s);
        s[10u16] = 0;
        if s[11u16] < 3 {
            s[11u16] += 1;
        }
        s[13u16] = 10;
        if s[11u16] >= 3 {
            s[13u16] = 250;
        }
        return false;
    }
    if phase != 0 {
        s[10u16] = phase + 1;
        if phase == 3 {
            s[10u16] = 0;
        }
        return false;
    }
    if id == 243 {
        ds2_disconnect(s);
        s[10u16] = 3;
        return false;
    }
    if id == 65 {
        ds2_disconnect(s);
        // Three complete configuration attempts, then a low-frequency probe.
        if s[11u16] >= 3 {
            s[11u16] = 0;
            s[13u16] = 250;
        } else {
            s[11u16] += 1;
            s[10u16] = 1;
        }
        return false;
    }
    let buttons = (rx[3u16] | (rx[4u16] << 8)) ^ 65535;
    if s[0u16] != 0 {
        s[8u16] = buttons & (s[7u16] ^ 65535);
        s[9u16] = s[7u16] & (buttons ^ 65535);
    }
    s[0u16] = 1;
    s[2u16] = buttons;
    s[7u16] = buttons;
    s[3u16] = rx[5u16];
    s[4u16] = rx[6u16];
    s[5u16] = rx[7u16];
    s[6u16] = rx[8u16];
    s[10u16] = 0;
    s[11u16] = 0;
    // Connected cadence is start-to-start. A once-per-frame caller may collect
    // the preceding packet and immediately launch the next, retaining overlap.
    s[12u16] = started;
    s[13u16] = 16;
    let next = time_milliseconds();
    if time_elapsed_ms(started, next) >= 16 {
        dev_send(5, 1, 0);
        s[12u16] = next;
        s[15u16] = 1;
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
