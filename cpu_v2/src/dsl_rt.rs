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
    /// pointer + offset (inherent method so subset programs need no trait imports)
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, off: i16) -> Ptr {
        Ptr(self.0.wrapping_add(off as u16))
    }
    pub fn read(self, off: i16) -> u16 {
        MEM.lock().unwrap()[self.add(off).0 as usize]
    }
    pub fn write(self, off: i16, v: u16) {
        MEM.lock().unwrap()[self.add(off).0 as usize] = v;
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

/// take the address of a variable (compiler intrinsic; globals become
/// compile-time constants, locals become sp+slot at run time)
pub fn addr_of<T>(_r: &T) -> Ptr {
    unimplemented!("addr_of is a target intrinsic")
}

/// array access extension trait (spec §10): the rcc compiler recognizes these
/// methods as intrinsics with no bounds checks; on the host they index real
/// Rust arrays, so the host run keeps the bounds check for free.
#[allow(clippy::len_without_is_empty)]
pub trait Slice2 {
    fn read(&self, i: u16) -> u16;
    fn write(&mut self, i: u16, v: u16);
    fn as_ptr(&self) -> Ptr;
    fn len(&self) -> u16;
}

impl<const N: usize> Slice2 for [u16; N] {
    fn read(&self, i: u16) -> u16 {
        self[i as usize]
    }
    fn write(&mut self, i: u16, v: u16) {
        self[i as usize] = v;
    }
    fn as_ptr(&self) -> Ptr {
        unimplemented!("as_ptr is a target intrinsic")
    }
    fn len(&self) -> u16 {
        N as u16
    }
}

impl<const N: usize> Slice2 for [i16; N] {
    fn read(&self, i: u16) -> u16 {
        self[i as usize] as u16
    }
    fn write(&mut self, i: u16, v: u16) {
        self[i as usize] = v as i16;
    }
    fn as_ptr(&self) -> Ptr {
        unimplemented!("as_ptr is a target intrinsic")
    }
    fn len(&self) -> u16 {
        N as u16
    }
}
