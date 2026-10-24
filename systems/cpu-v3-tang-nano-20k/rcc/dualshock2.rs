//! Device5 wire-protocol driver. Include device_abi and time alongside it.
use crate::dsl_rt::*;

// Reads never consume a byte. Only SEND5,2 acknowledges a completed result.
pub fn ds2_abort() {
    dev_send(5, 0, 3);
}

pub fn ds2_expired(start: u16) -> bool {
    let now = time_microseconds();
    return now == 65535 || now < start || now - start >= 20000;
}

pub fn ds2_wait_idle(start: u16) -> bool {
    while (dev_recv(5, 0) & 1) != 0 {
        if ds2_expired(start) {
            ds2_abort();
            return false;
        }
    }
    if ds2_expired(start) || (dev_recv(5, 0) & 8) != 0 {
        ds2_abort();
        return false;
    }
    return true;
}

// 0..255 on success, 256 on timeout/rejection. Never clears the shared timer.
pub fn ds2_byte(tx: u16, start: u16) -> u16 {
    dev_send(5, 1, tx);
    if !ds2_wait_idle(start) {
        return 256;
    }
    if (dev_recv(5, 0) & 2) == 0 {
        ds2_abort();
        return 256;
    }
    let value = dev_recv(5, 2) & 255;
    dev_send(5, 2, 1);
    return value;
}

// Complete one bounded packet. command=42/43/44; arg is its fourth byte.
// Only recognized IDs determine length; F3 is configuration state, never input.
pub fn ds2_packet(mut rx: Array<u16>, command: u16, arg: u16) -> u16 {
    let start = time_microseconds();
    if start == 65535 {
        ds2_abort();
        return 0;
    }
    ds2_abort();
    dev_send(5, 0, 1);
    if !ds2_wait_idle(start) {
        return 0;
    }
    let mut length: u16 = 3;
    let mut i: u16 = 0;
    while i < length {
        let mut tx: u16 = 0;
        if i == 0 {
            tx = 1;
        }
        if i == 1 {
            tx = command;
        }
        if i == 3 {
            tx = arg;
        }
        if command == 68 && i == 4 {
            tx = 3;
        }
        if command == 67 && arg == 0 && i >= 4 {
            tx = 90;
        }
        let value = ds2_byte(tx, start);
        if value > 255 {
            return 0;
        }
        rx[i] = value;
        if i == 2 {
            if rx[0u16] != 255 || value != 90 {
                ds2_abort();
                return 0;
            }
            let id = rx[1u16];
            if id == 65 {
                length = 5;
            } else if id == 115 || id == 243 {
                length = 9;
            } else if id == 121 {
                length = 21;
            } else {
                ds2_abort();
                return 0;
            }
        }
        i += 1;
    }
    dev_send(5, 0, 2);
    if !ds2_wait_idle(start) {
        return 0;
    }
    return rx[1u16];
}

// State[16]: connected, ID, active-high buttons, RX,RY,LX,LY,
// previous buttons, pressed, released, config phase, failed attempts,
// last service ms, delay ms, started, reserved. Invalid axes are neutral 128.
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

// One packet per service, including configuration. Caller services frequently
// and owns timer clear. Return true only when a new analog snapshot is ready.
pub fn ds2_service(mut s: Array<u16>, mut rx: Array<u16>) -> bool {
    s[8u16] = 0;
    s[9u16] = 0;
    let now = time_milliseconds();
    if s[14u16] != 0 && time_elapsed_ms(s[12u16], now) < s[13u16] {
        return false;
    }
    s[14u16] = 1;
    s[12u16] = now;
    let phase = s[10u16];
    let mut command: u16 = 66;
    let mut arg: u16 = 0;
    if phase == 1 {
        command = 67;
        arg = 1;
    }
    if phase == 2 {
        command = 68;
        arg = 1;
    }
    if phase == 3 {
        command = 67;
    }
    let id = ds2_packet(rx, command, arg);
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
    s[13u16] = 16;
    return true;
}

pub fn ds2_axis(raw: u16, deadzone: u16) -> i16 {
    let value = (raw as i16) - 128;
    if value >= -(deadzone as i16) && value <= deadzone as i16 {
        return 0;
    }
    return value;
}
