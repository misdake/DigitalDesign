//! Demo application for the CpuV3 two-stage flash boot: moves one lit LED back
//! and forth across all six logical LEDs through device 0 channel 2, while
//! repeatedly transmitting the 8-byte DDHT status frame (magic "DDHT",
//! protocol version 1, test ID 0x07 "CpuV3 two-stage flash boot", status 0 =
//! success, XOR checksum of bytes 0..6) through the device 0 UART. Never halts.

use crate::dsl_rt::*;

/// Transmits one byte through the device 0 UART (channel 3), polling the
/// busy bit first.
fn uart_byte(b: u16) {
    while dev_recv(0, 3) & 1 != 0 { }
    dev_send(0, 3, b);
}

/// Transmits one successful CpuV3 boot status frame.
fn uart_success() {
    uart_byte(0x44); // 'D'
    uart_byte(0x44); // 'D'
    uart_byte(0x48); // 'H'
    uart_byte(0x54); // 'T'
    uart_byte(1);    // protocol version
    uart_byte(0x07); // test ID: CpuV3 two-stage flash boot
    uart_byte(0);    // status: success
    // XOR of 'D' 'D' 'H' 'T' 1 0x07 0 (the two 'D' bytes cancel).
    uart_byte(0x48 ^ 0x54 ^ 1 ^ 0x07);
}

/// Busy-waits long enough for each LED position to be visible at 54 MHz.
fn visible_delay() {
    let mut outer: u16 = 0;
    while outer < 8 {
        let mut inner: u16 = 0;
        while inner < 60000 {
            inner += 1;
        }
        outer += 1;
    }
}

#[allow(clippy::eq_op)] // `while 1 == 1` is the rcc spelling of an endless loop
fn main() {
    let mut led: u16 = 0b00_0001;
    let mut moving_left: u16 = 1;
    while 1 == 1 {
        dev_send(0, 2, led);
        // Keep two adjacent frames at every position so bounded host and model
        // validation does not depend on the deliberately long visual delay.
        uart_success();
        uart_success();
        visible_delay();

        if led == 0b10_0000 {
            moving_left = 0;
        } else if led == 0b00_0001 {
            moving_left = 1;
        }
        if moving_left == 1 {
            led = led << 1;
        } else {
            led = led >> 1;
        }
    }
}
