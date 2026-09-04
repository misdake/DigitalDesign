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

// ---------------------------------------------------------------------------
// FPU types: fix16 scalar and vec2/vec3/vec4 vectors. On the target each value
// occupies exactly one F register (four signed Q8.8 lanes); vec2/vec3 keep
// their tail lanes zero. The host implementations below model the target
// fix16 arithmetic exactly for +, -, *, dot, and the simple unary operations;
// the ROM-based operations (frcp/frsqrt/fsincos) panic on the host.
// ---------------------------------------------------------------------------

fn fix16_saturate(value: i64) -> i16 {
    value.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

fn round_shift_ties_even(value: i64, shift: u32) -> i64 {
    let negative = value < 0;
    let magnitude = value.unsigned_abs() as i64;
    let divisor = 1_i64 << shift;
    let quotient = magnitude >> shift;
    let remainder = magnitude & (divisor - 1);
    let half = divisor >> 1;
    let rounded = if remainder > half || (remainder == half && quotient & 1 == 1) {
        quotient + 1
    } else {
        quotient
    };
    if negative { -rounded } else { rounded }
}

fn fix16_mul(a: i16, b: i16) -> i16 {
    fix16_saturate(round_shift_ties_even(i64::from(a) * i64::from(b), 8))
}

fn fix16_floor(v: i16) -> i16 {
    v & !0xff
}

fn fix16_ceil(v: i16) -> i16 {
    if v & 0xff == 0 {
        v
    } else {
        fix16_saturate(i64::from(v & !0xff) + 256)
    }
}

fn fix16_round(v: i16) -> i16 {
    fix16_saturate(round_shift_ties_even(i64::from(v), 8) << 8)
}

fn fix16_abs(v: i16) -> i16 {
    if v == i16::MIN { i16::MAX } else { v.abs() }
}

fn fix16_neg(v: i16) -> i16 {
    if v == i16::MIN { i16::MAX } else { -v }
}

/// signed Q8.8 fixed-point scalar (one F register on the target)
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct fix16(pub i16);

#[allow(non_camel_case_types)]
impl fix16 {
    /// raw Q8.8 bit pattern (FLOAD bridge)
    pub fn from_bits(bits: u16) -> fix16 {
        fix16(bits as i16)
    }
    /// integer value, shifted into the Q8.8 format
    pub fn from_int(value: i16) -> fix16 {
        fix16(fix16_saturate(i64::from(value) << 8))
    }
    pub fn zero() -> fix16 {
        fix16(0)
    }
    /// the raw Q8.8 bit pattern (FSTORE bridge)
    pub fn to_bits(self) -> u16 {
        self.0 as u16
    }
    /// truncate toward negative infinity (arithmetic shift)
    pub fn to_int(self) -> i16 {
        self.0 >> 8
    }
    pub fn x(self) -> fix16 {
        self
    }
    pub fn abs(self) -> fix16 {
        fix16(fix16_abs(self.0))
    }
    pub fn floor(self) -> fix16 {
        fix16(fix16_floor(self.0))
    }
    pub fn ceil(self) -> fix16 {
        fix16(fix16_ceil(self.0))
    }
    pub fn round(self) -> fix16 {
        fix16(fix16_round(self.0))
    }
    pub fn sat01(self) -> fix16 {
        fix16(self.0.clamp(0, 256))
    }
    pub fn sign(self) -> fix16 {
        fix16(match self.0.cmp(&0) {
            std::cmp::Ordering::Less => -256,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 256,
        })
    }
}

impl std::ops::Add for fix16 {
    type Output = fix16;
    fn add(self, rhs: fix16) -> fix16 {
        fix16(fix16_saturate(i64::from(self.0) + i64::from(rhs.0)))
    }
}
impl std::ops::Sub for fix16 {
    type Output = fix16;
    fn sub(self, rhs: fix16) -> fix16 {
        fix16(fix16_saturate(i64::from(self.0) - i64::from(rhs.0)))
    }
}
impl std::ops::Mul for fix16 {
    type Output = fix16;
    fn mul(self, rhs: fix16) -> fix16 {
        fix16(fix16_mul(self.0, rhs.0))
    }
}
impl std::ops::Neg for fix16 {
    type Output = fix16;
    fn neg(self) -> fix16 {
        fix16(fix16_neg(self.0))
    }
}

macro_rules! fpu_vec {
    ($name:ident, $lanes:expr) => {
        #[allow(non_camel_case_types)]
        #[derive(Copy, Clone, PartialEq, Debug, Default)]
        pub struct $name(pub [fix16; 4]);

        #[allow(non_camel_case_types)]
        impl $name {
            pub fn zero() -> $name {
                $name([fix16(0); 4])
            }
            fn map(self, f: fn(i16) -> i16) -> $name {
                let mut lanes = [fix16(0); 4];
                for (i, lane) in lanes.iter_mut().enumerate().take($lanes) {
                    *lane = fix16(f(self.0[i].0));
                }
                $name(lanes)
            }
            pub fn x(self) -> fix16 {
                self.0[0]
            }
            pub fn abs(self) -> $name {
                self.map(fix16_abs)
            }
            pub fn floor(self) -> $name {
                self.map(fix16_floor)
            }
            pub fn ceil(self) -> $name {
                self.map(fix16_ceil)
            }
            pub fn round(self) -> $name {
                self.map(fix16_round)
            }
            pub fn sat01(self) -> $name {
                self.map(|v| v.clamp(0, 256))
            }
            pub fn sign(self) -> $name {
                self.map(|v| match v.cmp(&0) {
                    std::cmp::Ordering::Less => -256,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 256,
                })
            }
        }

        impl std::ops::Add for $name {
            type Output = $name;
            fn add(self, rhs: $name) -> $name {
                let mut lanes = [fix16(0); 4];
                for i in 0..$lanes {
                    lanes[i] = self.0[i] + rhs.0[i];
                }
                $name(lanes)
            }
        }
        impl std::ops::Sub for $name {
            type Output = $name;
            fn sub(self, rhs: $name) -> $name {
                let mut lanes = [fix16(0); 4];
                for i in 0..$lanes {
                    lanes[i] = self.0[i] - rhs.0[i];
                }
                $name(lanes)
            }
        }
        impl std::ops::Mul for $name {
            type Output = $name;
            fn mul(self, rhs: $name) -> $name {
                let mut lanes = [fix16(0); 4];
                for i in 0..$lanes {
                    lanes[i] = self.0[i] * rhs.0[i];
                }
                $name(lanes)
            }
        }
        impl std::ops::Mul<fix16> for $name {
            type Output = $name;
            fn mul(self, rhs: fix16) -> $name {
                let mut lanes = [fix16(0); 4];
                for i in 0..$lanes {
                    lanes[i] = self.0[i] * rhs;
                }
                $name(lanes)
            }
        }
        impl std::ops::Mul<$name> for fix16 {
            type Output = $name;
            fn mul(self, rhs: $name) -> $name {
                rhs * self
            }
        }
        impl std::ops::Neg for $name {
            type Output = $name;
            fn neg(self) -> $name {
                self.map(fix16_neg)
            }
        }
    };
}

fpu_vec!(vec2, 2);
fpu_vec!(vec3, 3);
fpu_vec!(vec4, 4);

#[allow(non_camel_case_types)]
impl vec2 {
    pub fn new(x: fix16, y: fix16) -> vec2 {
        vec2([x, y, fix16(0), fix16(0)])
    }
    pub fn y(self) -> fix16 {
        self.0[1]
    }
}

#[allow(non_camel_case_types)]
impl vec3 {
    pub fn new(x: fix16, y: fix16, z: fix16) -> vec3 {
        vec3([x, y, z, fix16(0)])
    }
    pub fn y(self) -> fix16 {
        self.0[1]
    }
    pub fn z(self) -> fix16 {
        self.0[2]
    }
}

#[allow(non_camel_case_types)]
impl vec4 {
    pub fn new(x: fix16, y: fix16, z: fix16, w: fix16) -> vec4 {
        vec4([x, y, z, w])
    }
    pub fn y(self) -> fix16 {
        self.0[1]
    }
    pub fn z(self) -> fix16 {
        self.0[2]
    }
    pub fn w(self) -> fix16 {
        self.0[3]
    }
    /// load four aligned words from data memory (FIMPORT4)
    pub fn import(ptr: Ptr) -> vec4 {
        assert_eq!(ptr.addr() & 3, 0, "vec4::import requires a 4-aligned address");
        let mut lanes = [fix16(0); 4];
        for (i, lane) in lanes.iter_mut().enumerate() {
            *lane = fix16::from_bits(ptr.read(i as i16));
        }
        vec4(lanes)
    }
    /// store four aligned words to data memory (FEXPORT4)
    pub fn export(v: vec4, ptr: Ptr) {
        assert_eq!(ptr.addr() & 3, 0, "vec4::export requires a 4-aligned address");
        for (i, lane) in v.0.iter().enumerate() {
            ptr.write(i as i16, lane.to_bits());
        }
    }
}

/// dot product accumulated in the wide ACC format and rounded back to Q8.8
pub fn fdot(a: vec4, b: vec4) -> fix16 {
    let mut acc: i64 = 0;
    for i in 0..4 {
        acc += i64::from(a.0[i].0) * i64::from(b.0[i].0);
    }
    fix16(fix16_saturate(round_shift_ties_even(acc, 8)))
}

/// ROM-based on the target; no bit-exact host model
pub fn frcp(_x: fix16) -> fix16 {
    unimplemented!("frcp is a target FPU ROM operation without a host model")
}

/// ROM-based on the target; no bit-exact host model
pub fn frsqrt(_x: fix16) -> fix16 {
    unimplemented!("frsqrt is a target FPU ROM operation without a host model")
}

/// ROM-based on the target; no bit-exact host model
pub fn fsincos(_x: fix16) -> vec2 {
    unimplemented!("fsincos is a target FPU ROM operation without a host model")
}
