//! Demo application for the G16 two-stage flash boot: repeatedly transmits
//! the 8-byte DDHT status frame (magic "DDHT", protocol version 1, test ID
//! 0x07 "G16 two-stage flash boot", status 0 = success, XOR checksum of
//! bytes 0..6) through the device 0 UART. Never halts.

use crate::dsl_rt::*;

/// Transmits one byte through the device 0 UART (channel 3), polling the
/// busy bit first.
fn uart_byte(b: u16) {
    while dev_recv(0, 3) & 1 != 0 { }
    dev_send(0, 3, b);
}

#[allow(clippy::eq_op)] // `while 1 == 1` is the rcc spelling of an endless loop
fn main() {
    // XOR of the frame bytes 'D' 'D' 'H' 'T' 1 0x07 0 (the two 'D' cancel)
    let checksum: u16 = 0x48 ^ 0x54 ^ 1 ^ 0x07;
    while 1 == 1 {
        uart_byte(0x44); // 'D'
        uart_byte(0x44); // 'D'
        uart_byte(0x48); // 'H'
        uart_byte(0x54); // 'T'
        uart_byte(1);    // protocol version
        uart_byte(0x07); // test ID: G16 two-stage flash boot
        uart_byte(0);    // status: success
        uart_byte(checksum);
    }
}
