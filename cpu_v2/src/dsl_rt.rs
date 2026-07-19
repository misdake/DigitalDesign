//! host-side runtime for rcc subset programs (`dsl_progs/*_dsl.rs`).
//!
//! These are the **real Rust** declarations that make subset programs valid
//! Rust for rust-analyzer/rustc. The rcc compiler frontend recognizes the
//! same names as intrinsics and lowers them to machine instructions instead.
//!
//! On the host they simulate a tiny machine: a 64K-word data memory, `halt`
//! panicking with the signal value. This lets subset programs also run on
//! the host for debugging.

use once_cell::sync::Lazy;
use std::sync::Mutex;

/// data memory shared by all subset programs running on the host
pub static MEM: Lazy<Mutex<Box<[u16; 65536]>>> = Lazy::new(|| Mutex::new(Box::new([0; 65536])));

/// data pointer (address in data memory). In the rcc subset this is a
/// distinct type from function pointers (Harvard architecture).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Ptr(pub u16);

impl Ptr {
    pub fn from_addr(addr: u16) -> Ptr {
        Ptr(addr)
    }
    pub fn addr(self) -> u16 {
        self.0
    }
    pub fn read(self, off: i16) -> u16 {
        MEM.lock().unwrap()[(self + off).0 as usize]
    }
    pub fn write(self, off: i16, v: u16) {
        MEM.lock().unwrap()[(self + off).0 as usize] = v;
    }
}

impl std::ops::Add<i16> for Ptr {
    type Output = Ptr;
    fn add(self, off: i16) -> Ptr {
        Ptr(self.0.wrapping_add(off as u16))
    }
}

/// halt the machine with a signal value
pub fn halt(x: u16) -> ! {
    panic!("halt with signal {x} ({x:#06x})")
}

/// halt with `sig` when `cond` does not hold
pub fn assert(cond: bool, sig: u16) {
    if !cond {
        halt(sig);
    }
}

/// receive a word from a device (not available on the host)
pub fn dev_recv(dev: u8, ch: u8) -> u16 {
    let _ = (dev, ch);
    unimplemented!("devices are not available on the host")
}

/// send a word to a device (not available on the host)
pub fn dev_send(dev: u8, ch: u8, v: u16) {
    let _ = (dev, ch, v);
    unimplemented!("devices are not available on the host")
}
