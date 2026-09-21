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

/// Owned word storage — **the** array type (spec §10). The compiler recognizes
/// `Buf<T, N>` in a type position and lowers it to N consecutive words, where T is
/// `u16`, `i16` or a struct; the methods below are target intrinsics. On the host
/// they touch real Rust storage, so a host run keeps the bounds check for free.
#[repr(transparent)]
pub struct Buf<T, const N: usize>([T; N]);

impl<T, const N: usize> Buf<T, N> {
    /// initialize from `[v; N]` or `[e0, e1, ...]`
    pub const fn new(words: [T; N]) -> Self {
        Self(words)
    }
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u16 {
        N as u16
    }
    pub fn as_ptr(&self) -> Ptr {
        unimplemented!("as_ptr is a target intrinsic")
    }
    /// the first-element address as a one-word typed view
    pub fn as_array(&self) -> Array<T> {
        Array::from_host_ptr(self.0.as_slice().as_ptr() as *mut T)
    }
}

impl<const N: usize> Buf<u16, N> {
    pub fn read(&self, i: u16) -> u16 {
        self[i]
    }
    pub fn write(&mut self, i: u16, v: u16) {
        self[i] = v;
    }
}

impl<const N: usize> Buf<i16, N> {
    pub fn read(&self, i: u16) -> u16 {
        self[i] as u16
    }
    pub fn write(&mut self, i: u16, v: u16) {
        self[i] = v as i16;
    }
}

macro_rules! impl_buf_index {
    ($index:ty) => {
        impl<T, const N: usize> Index<$index> for Buf<T, N> {
            type Output = T;
            fn index(&self, index: $index) -> &Self::Output {
                &self.0[index as usize]
            }
        }
        impl<T, const N: usize> IndexMut<$index> for Buf<T, N> {
            fn index_mut(&mut self, index: $index) -> &mut Self::Output {
                &mut self.0[index as usize]
            }
        }
    };
}

impl_buf_index!(u16);
impl_buf_index!(i16);

/// The address of one value as a typed view. The target has no separate
/// representation: a struct (or addressable scalar) value *is* its address.
pub fn view_of<T>(value: &T) -> Array<T> {
    Array::from_host_ptr(value as *const T as *mut T)
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

/// unsigned `/` with a defined zero divisor (`x / 0 == 0`), matching the target
/// routine. The raw `/` operator keeps Rust's panic on the host, so a program
/// that must run both ways calls this when the divisor may be zero.
pub fn div_u16(a: u16, b: u16) -> u16 {
    a.checked_div(b).unwrap_or(0)
}

/// unsigned `%` with a defined zero divisor (`x % 0 == x`)
pub fn rem_u16(a: u16, b: u16) -> u16 {
    a.checked_rem(b).unwrap_or(a)
}

/// signed `div_u16`: the quotient truncates toward zero, and `x / 0 == 0`
pub fn div_i16(a: i16, b: i16) -> i16 {
    if b == 0 {
        0
    } else {
        // wrapping rather than checked: `i16::MIN / -1` wraps like the target
        // routine instead of saturating or panicking
        a.wrapping_div(b)
    }
}

/// signed `rem_u16`: the remainder follows the dividend's sign, and `x % 0 == x`
pub fn rem_i16(a: i16, b: i16) -> i16 {
    if b == 0 {
        a
    } else {
        a.wrapping_rem(b)
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

#[cfg(test)]
mod tests {
    use super::Buf;

    #[test]
    fn host_buf_indexes_real_storage() {
        let mut words: Buf<u16, 3> = Buf::new([1, 2, 3]);
        words[1u16] = 7;
        words[2u16] += 4;
        assert_eq!((words[0u16], words[1u16], words[2u16]), (1, 7, 7));
        assert_eq!(words.len(), 3);
        words.write(0, 9);
        assert_eq!(words.read(0), 9);

        let signed: Buf<i16, 2> = Buf::new([-3, 5]);
        assert_eq!(signed[0i16], -3);
        assert_eq!(signed[1i16], 5);
    }

    #[test]
    fn buf_view_indexes_the_same_storage() {
        let words: Buf<u16, 3> = Buf::new([1, 2, 3]);
        let mut view = words.as_array();
        view[1u16] = 7;
        view[2u16] += 4;
        assert_eq!(words[1u16], 7);
        assert_eq!(words[2u16], 7);
    }

    #[test]
    #[should_panic]
    fn host_buf_keeps_the_bounds_check() {
        let words: Buf<u16, 2> = Buf::new([1, 2]);
        let _ = words[2u16];
    }
}

// ---------------------------------------------------------------------------
// FPU types: fix32 scalar and vec2/vec3/vec4 vectors. All architectural F
// registers are **signed Q16.16**: 16 fractional bits, numeric range about
// [-32768, +32767.99998], and every operation wraps (no saturation, no
// rounding flag). The name `fix32` denotes the 32-bit Q16.16 storage of the
// source scalar. A vecN value
// is a consecutive range of N scalar F registers (2/3/4 for vec2/3/4); the
// host representation keeps four lanes with a zero tail for uniformity.
//
// The host implementations model the target Q16.16 arithmetic for +, -, *,
// dot and the simple unary operations. The special functions
// (frcp/frsqrt/fsincos) still panic on the host; their pure reference model
// lives in the CPU V3 architecture crate.
// ---------------------------------------------------------------------------

/// Fractional bits of an architectural F register.
pub const FIX32_FRACTION_BITS: u32 = 16;

fn fix32_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> FIX32_FRACTION_BITS) as i32
}

fn fix32_floor(v: i32) -> i32 {
    v & !0xffff
}

fn fix32_ceil(v: i32) -> i32 {
    if v & 0xffff == 0 {
        v
    } else {
        fix32_floor(v).wrapping_add(1 << FIX32_FRACTION_BITS)
    }
}

fn fix32_round(v: i32) -> i32 {
    v.wrapping_add(1 << (FIX32_FRACTION_BITS - 1)) & !0xffff
}

fn fix32_trunc(v: i32) -> i32 {
    if v < 0 {
        fix32_ceil(v)
    } else {
        fix32_floor(v)
    }
}

fn fix32_abs(v: i32) -> i32 {
    v.wrapping_abs()
}

fn fix32_neg(v: i32) -> i32 {
    v.wrapping_neg()
}

/// signed Q16.16 fixed-point scalar (one F register on the target)
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct fix32(pub i32);

#[allow(non_camel_case_types)]
impl fix32 {
    /// integer value, shifted into the Q16.16 format
    pub fn from_int(value: i16) -> fix32 {
        fix32(i32::from(value) << FIX32_FRACTION_BITS)
    }
    pub fn zero() -> fix32 {
        fix32(0)
    }
    /// Builds a Q16.16 value from its two raw halves, low half first. This is
    /// the raw-move contract of `ILO2F`/`IHI2F`/`FLD`; it is deliberately
    /// distinct from the numeric `from_int`/`to_int` conversions.
    pub fn from_words(lo: u16, hi: u16) -> fix32 {
        fix32(((u32::from(hi) << 16) | u32::from(lo)) as i32)
    }
    /// Low 16 bits as an unsigned word (`FLO2I`/`FST` half).
    pub fn lo_bits(self) -> u16 {
        self.0 as u16
    }
    /// High 16 bits as an unsigned word (`FHI2I`/`FST` half).
    pub fn hi_bits(self) -> u16 {
        (self.0 >> 16) as u16
    }
    /// Truncate toward zero (`FTOI16`); wraps on overflow.
    pub fn to_int(self) -> i16 {
        (fix32_trunc(self.0) >> FIX32_FRACTION_BITS) as i16
    }
    pub fn x(self) -> fix32 {
        self
    }
    pub fn abs(self) -> fix32 {
        fix32(fix32_abs(self.0))
    }
    pub fn floor(self) -> fix32 {
        fix32(fix32_floor(self.0))
    }
    pub fn ceil(self) -> fix32 {
        fix32(fix32_ceil(self.0))
    }
    pub fn round(self) -> fix32 {
        fix32(fix32_round(self.0))
    }
    /// Truncate toward zero.
    pub fn trunc(self) -> fix32 {
        fix32(fix32_trunc(self.0))
    }
    /// Clamp to `[0.0, 1.0]`; a host helper only (v2 has no SAT01 subop).
    pub fn sat01(self) -> fix32 {
        fix32(self.0.clamp(0, 1 << FIX32_FRACTION_BITS))
    }
    /// `-1.0`, `0.0` or `1.0`; a host helper only (v2 has no SIGN subop).
    pub fn sign(self) -> fix32 {
        fix32(match self.0.cmp(&0) {
            std::cmp::Ordering::Less => -(1 << FIX32_FRACTION_BITS),
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1 << FIX32_FRACTION_BITS,
        })
    }
}

impl std::ops::Add for fix32 {
    type Output = fix32;
    fn add(self, rhs: fix32) -> fix32 {
        fix32(self.0.wrapping_add(rhs.0))
    }
}
impl std::ops::Sub for fix32 {
    type Output = fix32;
    fn sub(self, rhs: fix32) -> fix32 {
        fix32(self.0.wrapping_sub(rhs.0))
    }
}
impl std::ops::Mul for fix32 {
    type Output = fix32;
    fn mul(self, rhs: fix32) -> fix32 {
        fix32(fix32_mul(self.0, rhs.0))
    }
}
impl std::ops::Neg for fix32 {
    type Output = fix32;
    fn neg(self) -> fix32 {
        fix32(fix32_neg(self.0))
    }
}

macro_rules! fpu_vec {
    ($name:ident, $lanes:expr) => {
        #[allow(non_camel_case_types)]
        #[derive(Copy, Clone, PartialEq, Debug, Default)]
        pub struct $name(pub [fix32; 4]);

        #[allow(non_camel_case_types)]
        impl $name {
            pub fn zero() -> $name {
                $name([fix32(0); 4])
            }
            fn map(self, f: fn(i32) -> i32) -> $name {
                let mut lanes = [fix32(0); 4];
                for (i, lane) in lanes.iter_mut().enumerate().take($lanes) {
                    *lane = fix32(f(self.0[i].0));
                }
                $name(lanes)
            }
            pub fn x(self) -> fix32 {
                self.0[0]
            }
            pub fn abs(self) -> $name {
                self.map(fix32_abs)
            }
            pub fn floor(self) -> $name {
                self.map(fix32_floor)
            }
            pub fn ceil(self) -> $name {
                self.map(fix32_ceil)
            }
            pub fn round(self) -> $name {
                self.map(fix32_round)
            }
            pub fn trunc(self) -> $name {
                self.map(fix32_trunc)
            }
            pub fn sat01(self) -> $name {
                self.map(|v| v.clamp(0, 1 << FIX32_FRACTION_BITS))
            }
            pub fn sign(self) -> $name {
                self.map(|v| match v.cmp(&0) {
                    std::cmp::Ordering::Less => -(1 << FIX32_FRACTION_BITS),
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1 << FIX32_FRACTION_BITS,
                })
            }
        }

        impl std::ops::Add for $name {
            type Output = $name;
            fn add(self, rhs: $name) -> $name {
                let mut lanes = [fix32(0); 4];
                for i in 0..$lanes {
                    lanes[i] = self.0[i] + rhs.0[i];
                }
                $name(lanes)
            }
        }
        impl std::ops::Sub for $name {
            type Output = $name;
            fn sub(self, rhs: $name) -> $name {
                let mut lanes = [fix32(0); 4];
                for i in 0..$lanes {
                    lanes[i] = self.0[i] - rhs.0[i];
                }
                $name(lanes)
            }
        }
        impl std::ops::Mul for $name {
            type Output = $name;
            fn mul(self, rhs: $name) -> $name {
                let mut lanes = [fix32(0); 4];
                for i in 0..$lanes {
                    lanes[i] = self.0[i] * rhs.0[i];
                }
                $name(lanes)
            }
        }
        impl std::ops::Mul<fix32> for $name {
            type Output = $name;
            fn mul(self, rhs: fix32) -> $name {
                let mut lanes = [fix32(0); 4];
                for i in 0..$lanes {
                    lanes[i] = self.0[i] * rhs;
                }
                $name(lanes)
            }
        }
        impl std::ops::Mul<$name> for fix32 {
            type Output = $name;
            fn mul(self, rhs: $name) -> $name {
                rhs * self
            }
        }
        impl std::ops::Neg for $name {
            type Output = $name;
            fn neg(self) -> $name {
                self.map(fix32_neg)
            }
        }
    };
}

fpu_vec!(vec2, 2);
fpu_vec!(vec3, 3);
fpu_vec!(vec4, 4);

#[allow(non_camel_case_types)]
impl vec2 {
    pub fn new(x: fix32, y: fix32) -> vec2 {
        vec2([x, y, fix32(0), fix32(0)])
    }
    pub fn y(self) -> fix32 {
        self.0[1]
    }
}

#[allow(non_camel_case_types)]
impl vec3 {
    pub fn new(x: fix32, y: fix32, z: fix32) -> vec3 {
        vec3([x, y, z, fix32(0)])
    }
    pub fn y(self) -> fix32 {
        self.0[1]
    }
    pub fn z(self) -> fix32 {
        self.0[2]
    }
}

#[allow(non_camel_case_types)]
impl vec4 {
    pub fn new(x: fix32, y: fix32, z: fix32, w: fix32) -> vec4 {
        vec4([x, y, z, w])
    }
    pub fn y(self) -> fix32 {
        self.0[1]
    }
    pub fn z(self) -> fix32 {
        self.0[2]
    }
    pub fn w(self) -> fix32 {
        self.0[3]
    }
    /// Load four consecutive Q16.16 values, two little-endian words each,
    /// low half first (`FLDV4`).
    pub fn import(ptr: Ptr) -> vec4 {
        let mut lanes = [fix32(0); 4];
        for (i, lane) in lanes.iter_mut().enumerate() {
            *lane = fix32::from_words(ptr.read(2 * i as i16), ptr.read(2 * i as i16 + 1));
        }
        vec4(lanes)
    }
    /// Store four consecutive Q16.16 values, two little-endian words each,
    /// low half first (`FSTV4`).
    pub fn export(v: vec4, ptr: Ptr) {
        for (i, lane) in v.0.iter().enumerate() {
            ptr.write(2 * i as i16, lane.lo_bits());
            ptr.write(2 * i as i16 + 1, lane.hi_bits());
        }
    }
}

/// A vector type usable with [`fdot`] (host model): each implementation sums
/// its real lanes in the wide accumulator.
pub trait Fdot: Copy {
    fn dot_terms(self, other: Self) -> i64;
}

macro_rules! impl_fdot {
    ($name:ident, $lanes:expr) => {
        impl Fdot for $name {
            fn dot_terms(self, other: Self) -> i64 {
                let mut acc: i64 = 0;
                for i in 0..$lanes {
                    acc = acc
                        .wrapping_add(i64::from(self.0[i].0).wrapping_mul(i64::from(other.0[i].0)));
                }
                acc
            }
        }
    };
}

impl_fdot!(vec2, 2);
impl_fdot!(vec3, 3);
impl_fdot!(vec4, 4);

/// Dot product through the wide Q32.32 accumulator, narrowed once to Q16.16
/// (`DOTSTORE`). Each product is exact; accumulation keeps the low 64 bits.
pub fn fdot<T: Fdot>(a: T, b: T) -> fix32 {
    fix32((a.dot_terms(b) >> FIX32_FRACTION_BITS) as i32)
}

// The special-function declarations below are target intrinsics lowered to
// the two-word FPU v2 special subops (`RCP`, `RSQRT`, and `SINCOS`
// modes 00/01/10), so `fsin`/`fcos` reuse the same `SINCOS` encoding as
// `fsincos`. The bit-exact reference model (and its hidden BSRAM tables) lives
// in the CPU V3 architecture crate. The host cannot reproduce it without
// duplicating that table, so these shims panic on the Rust host exactly like
// the other target-only intrinsics.

/// Target special function `Fd = rcp(Fa)` (`SCALAR` subop `0x0C`).
pub fn frcp(_x: fix32) -> fix32 {
    unimplemented!("frcp is a target FPU special function without a host model")
}

/// Target special function `Fd = rsqrt(Fa)` (`SCALAR` subop `0x0D`).
pub fn frsqrt(_x: fix32) -> fix32 {
    unimplemented!("frsqrt is a target FPU special function without a host model")
}

/// Target special function `Fd = sin(Fa)` (`SINCOS` mode `01`).
pub fn fsin(_x: fix32) -> fix32 {
    unimplemented!("fsin is a target FPU special function without a host model")
}

/// Target special function `Fd = cos(Fa)` (`SINCOS` mode `10`).
pub fn fcos(_x: fix32) -> fix32 {
    unimplemented!("fcos is a target FPU special function without a host model")
}

/// Target special function `Fd = sin(Fa)`, `Fd+1 = cos(Fa)` (`SINCOS` mode
/// `00`); the pair is a contiguous `vec2` over two adjacent F registers.
pub fn fsincos(_x: fix32) -> vec2 {
    unimplemented!("fsincos is a target FPU special function without a host model")
}

// ---------------------------------------------------------------------------
// v3 geometry library contract (target lowering; no new opcodes). The functions
// are ordinary rcc library calls built from existing FPU v2 scalar/vector
// instructions: `DOTSTORE`, `RSQRT`, `VMULS`, `VSUB` and the scalar `CMP`. The
// pure reference model and its tests live in the CPU V3 architecture crate
// (`cpu_v3::v3_length2` etc.). The release helpers assume a small-range Q16.16
// contract and emit no checks; the `_checked` debug helpers validate the same
// contract and halt with a fixed nonzero signal instead of silently clamping.
// ---------------------------------------------------------------------------

/// Halt signal of `v3_length2_checked` on a violated component range.
pub const V3_LENGTH2_CHECKED_HALT: u16 = 1;
/// Halt signal of `v3_normalize_checked` on a violated component range.
pub const V3_NORMALIZE_CHECKED_HALT: u16 = 2;
/// Halt signal of `v3_distance_gt_checked` on a violated input range.
pub const V3_DISTANCE_GT_CHECKED_INPUT_HALT: u16 = 3;
/// Halt signal of `v3_distance_gt_checked` on a violated difference range.
pub const V3_DISTANCE_GT_CHECKED_DIFFERENCE_HALT: u16 = 4;

/// Raw Q16.16 inclusive component bound `[-104, +104]` of the checked
/// `v3_length2`/`v3_normalize` contract.
pub const V3_GEOMETRY_COMPONENT_LIMIT_Q16: i32 = 104 << FIX32_FRACTION_BITS;
/// Raw Q16.16 inclusive lower input bound `-16384` of the checked distance
/// contract.
pub const V3_GEOMETRY_DISTANCE_INPUT_MIN_Q16: i32 = -16384 << FIX32_FRACTION_BITS;
/// Raw Q16.16 inclusive upper input bound `+16383` of the checked distance
/// contract.
pub const V3_GEOMETRY_DISTANCE_INPUT_MAX_Q16: i32 = 16383 << FIX32_FRACTION_BITS;

/// The squared length `|v|^2`: one `DOTSTORE` of `v` with itself.
pub fn v3_length2(_v: vec3) -> fix32 {
    unimplemented!("v3_length2 is a target FPU v2 library lowering")
}

/// `v / |v|`: `DOTSTORE`, `RSQRT`, then `VMULS`; a zero vector stays zero
/// because `RSQRT(0) == 0`.
pub fn v3_normalize(_v: vec3) -> vec3 {
    unimplemented!("v3_normalize is a target FPU v2 library lowering")
}

/// The approximate ordinary distance `|a - b|`: `VSUB`, one `DOTSTORE` of the
/// difference, `s * RSQRT(s)`, then the scalar `CMP` against the ordinary
/// Q16.16 `threshold`.
pub fn v3_distance_gt(_a: vec3, _b: vec3, _threshold: fix32) -> bool {
    unimplemented!("v3_distance_gt is a target FPU v2 library lowering")
}

/// [`v3_length2`] with the small-range precondition checked: every component
/// must be inclusively within `[-104, +104]` Q16.16, otherwise it halts with
/// [`V3_LENGTH2_CHECKED_HALT`]. It never silently wraps a length.
pub fn v3_length2_checked(_v: vec3) -> fix32 {
    unimplemented!("v3_length2_checked is a target FPU v2 library lowering")
}

/// [`v3_normalize`] with the same `[-104, +104]` component precondition,
/// otherwise it halts with [`V3_NORMALIZE_CHECKED_HALT`].
pub fn v3_normalize_checked(_v: vec3) -> vec3 {
    unimplemented!("v3_normalize_checked is a target FPU v2 library lowering")
}

/// [`v3_distance_gt`] with both preconditions checked: every input component
/// within `[-16384, +16383]` Q16.16 (otherwise it halts with
/// [`V3_DISTANCE_GT_CHECKED_INPUT_HALT`]), then every difference component
/// within `[-104, +104]` (otherwise
/// [`V3_DISTANCE_GT_CHECKED_DIFFERENCE_HALT`]).
pub fn v3_distance_gt_checked(_a: vec3, _b: vec3, _threshold: fix32) -> bool {
    unimplemented!("v3_distance_gt_checked is a target FPU v2 library lowering")
}
