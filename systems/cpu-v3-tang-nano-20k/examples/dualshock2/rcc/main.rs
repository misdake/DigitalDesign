mod device_abi;
mod dualshock2;
mod time;
use crate::dsl_rt::*;

fn put(value: u16) {
    let start = time_milliseconds();
    while (dev_recv(0, 3) & 1) != 0 {
        if time_elapsed_ms(start, time_milliseconds()) >= 10 {
            return;
        }
    }
    dev_send(0, 3, value);
}
fn hex(value: u16) {
    let mut shift: u16 = 12;
    let mut i: u16 = 0;
    while i < 4 {
        let mut digit = (value >> shift) & 15;
        if digit < 10 {
            digit += 48;
        } else {
            digit += 55;
        }
        put(digit);
        shift -= 4;
        i += 1;
    }
}
fn main() {
    let mut state: Buf<u16, 10> = Buf::new([0; 10]);
    let mut packet: Buf<u16, 9> = Buf::new([0; 9]);
    ds2_init(state.as_array());
    put(68);
    put(83);
    put(50);
    put(13);
    put(10);
    let mut previous: u16 = 65535;
    let mut report = time_milliseconds();
    loop {
        ds2_service(state.as_array(), packet.as_array());
        let connected = state[0u16];
        dev_send(0, 2, 1 | (connected << 1));
        let now = time_milliseconds();
        if connected != previous || time_elapsed_ms(report, now) >= 250 {
            // DS2 C=.... ID=.... B=.... RX=.... RY=.... LX=.... LY=.... (hex).
            put(67);
            put(61);
            hex(connected);
            put(32);
            put(73);
            put(68);
            put(61);
            hex(state[1u16]);
            put(32);
            put(66);
            put(61);
            hex(state[2u16]);
            put(32);
            put(82);
            put(88);
            put(61);
            hex(state[3u16]);
            put(32);
            put(82);
            put(89);
            put(61);
            hex(state[4u16]);
            put(32);
            put(76);
            put(88);
            put(61);
            hex(state[5u16]);
            put(32);
            put(76);
            put(89);
            put(61);
            hex(state[6u16]);
            put(13);
            put(10);
            report = now;
            previous = connected;
        }
    }
}
