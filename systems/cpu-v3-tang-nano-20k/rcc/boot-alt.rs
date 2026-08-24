//! Alternate CpuV3 boot application selected by reset-time button value 10:
//! alternates the odd and even logical LEDs while emitting the same DDHT 0x07
//! success frame as the primary boot demo. Never halts.

use crate::dsl_rt::*;

fn uart_byte(b: u16) {
    while dev_recv(0, 3) & 1 != 0 { }
    dev_send(0, 3, b);
}

fn uart_success() {
    uart_byte(0x44);
    uart_byte(0x44);
    uart_byte(0x48);
    uart_byte(0x54);
    uart_byte(1);
    uart_byte(0x07);
    uart_byte(0);
    uart_byte(0x48 ^ 0x54 ^ 1 ^ 0x07);
}

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

#[allow(clippy::eq_op)]
fn main() {
    let mut pattern: u16 = 0b01_0101;
    while 1 == 1 {
        dev_send(0, 2, pattern);
        uart_success();
        uart_success();
        visible_delay();
        if pattern == 0b01_0101 {
            pattern = 0b10_1010;
        } else {
            pattern = 0b01_0101;
        }
    }
}
