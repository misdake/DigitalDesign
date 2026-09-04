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
use std::ops::{Index, IndexMut};
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
    pub fn as_u16_array(self) -> Array<u16> {
        unimplemented!("Ptr::as_u16_array is a target intrinsic")
    }
    pub fn as_i16_array(self) -> Array<i16> {
        unimplemented!("Ptr::as_i16_array is a target intrinsic")
    }
}

/// Typed, one-word array view used by rcc's indexing syntax. On the target it
/// has exactly the same representation as Ptr and performs unchecked access.
pub struct Array<T>(*mut T);

impl<T> Copy for Array<T> {}
impl<T> Clone for Array<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Array<T> {
    fn from_host_ptr(ptr: *mut T) -> Self {
        Self(ptr)
    }
    pub fn as_ptr(self) -> Ptr {
        unimplemented!("Array::as_ptr is a target intrinsic")
    }
}

macro_rules! impl_array_index {
    ($index:ty) => {
        impl<T> Index<$index> for Array<T> {
            type Output = T;
            fn index(&self, index: $index) -> &Self::Output {
                unsafe { &*self.0.offset(index as isize) }
            }
        }
        impl<T> IndexMut<$index> for Array<T> {
            fn index_mut(&mut self, index: $index) -> &mut Self::Output {
                unsafe { &mut *self.0.offset(index as isize) }
            }
        }
    };
}

impl_array_index!(u16);
impl_array_index!(i16);

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

/// number of set bits in a 16-bit word
pub fn cnt1(x: u16) -> u16 {
    x.count_ones() as u16
}

/// integer base-2 logarithm; returns zero for an input of zero
pub fn log2(x: u16) -> u16 {
    if x == 0 {
        0
    } else {
        x.ilog2() as u16
    }
}

/// Receive a word from a device register (not available on the Rust host).
pub fn dev_recv(dev: u8, ch: u8) -> u16 {
    let _ = (dev, ch);
    unimplemented!("devices are not available on the host")
}

/// Send a word to a device register (not available on the Rust host).
pub fn dev_send(dev: u8, ch: u8, v: u16) {
    let _ = (dev, ch, v);
    unimplemented!("devices are not available on the host")
}

/// Invalidate the complete data cache (CpuV3-only; not available on the Rust
/// host). This is a compiler memory and control barrier.
pub fn dcache_invalidate_all() -> u16 {
    unimplemented!("cache maintenance is not available on the host")
}

/// Clean the complete data cache (CpuV3-only; not available on the Rust host).
/// The CPU is held until completion and the final maintenance status is returned.
pub fn dcache_clean_all() -> u16 {
    unimplemented!("cache maintenance is not available on the host")
}

/// Invalidate the complete instruction cache on the registered delayed path,
/// then immediately switch CSEG and jump (CpuV3-only). Never returns.
pub fn icache_invalidate_delayed_and_jump(cseg: u16, target: u16) -> ! {
    let _ = (cseg, target);
    unimplemented!("cache maintenance is not available on the host")
}

/// Write the DSEG special register (CpuV3-only; not available on the Rust host).
pub fn mtsr_dseg(v: u16) {
    let _ = v;
    unimplemented!("segment registers are not available on the host")
}

/// Atomically switch CSEG to `cseg` and jump to `target` (CpuV3-only; not
/// available on the Rust host). Never returns.
pub fn jseg(cseg: u16, target: u16) -> ! {
    let _ = (cseg, target);
    unimplemented!("segment registers are not available on the host")
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
    type Item;
    fn read(&self, i: u16) -> u16;
    fn write(&mut self, i: u16, v: u16);
    fn as_ptr(&self) -> Ptr;
    fn as_array(&self) -> Array<Self::Item>;
    fn len(&self) -> u16;
}

impl<const N: usize> Slice2 for [u16; N] {
    type Item = u16;
    fn read(&self, i: u16) -> u16 {
        self[i as usize]
    }
    fn write(&mut self, i: u16, v: u16) {
        self[i as usize] = v;
    }
    fn as_ptr(&self) -> Ptr {
        unimplemented!("as_ptr is a target intrinsic")
    }
    fn as_array(&self) -> Array<u16> {
        Array::from_host_ptr(self.as_slice().as_ptr() as *mut u16)
    }
    fn len(&self) -> u16 {
        N as u16
    }
}

impl<const N: usize> Slice2 for [i16; N] {
    type Item = i16;
    fn read(&self, i: u16) -> u16 {
        self[i as usize] as u16
    }
    fn write(&mut self, i: u16, v: u16) {
        self[i as usize] = v as i16;
    }
    fn as_ptr(&self) -> Ptr {
        unimplemented!("as_ptr is a target intrinsic")
    }
    fn as_array(&self) -> Array<i16> {
        Array::from_host_ptr(self.as_slice().as_ptr() as *mut i16)
    }
    fn len(&self) -> u16 {
        N as u16
    }
}

#[cfg(test)]
mod tests {
    use super::Slice2;

    #[test]
    fn host_array_view_indexes_real_arrays() {
        let words = [1u16, 2, 3];
        let mut view = words.as_array();
        view[1u16] = 7;
        view[2u16] += 4;
        assert_eq!(words, [1, 7, 7]);

        let signed = [-3i16, 5];
        let view = signed.as_array();
        assert_eq!(view[0i16], -3);
    }
}
