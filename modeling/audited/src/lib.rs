//! Closed fixed-point data path with automatic resource and dependency accounting.
//!
//! Runtime constructors, raw getters, primitive operators and division are absent.
//! Only compile-time literals, audited operations and typed memory reads produce values.
//! External data enters through a checked input store, never a Fixed constructor.
//! Host observations become available only after consuming the frame.
//! Numerical mode counts work without imposing hardware or port capacities.
//! Optional timing is an ASAP dependency/resource schedule with declared latencies,
//! not an executed microcode machine or an FPGA timing claim.
//!
//! ```
//! use audited::{Fixed, Model};
//! let mut model = Model::numerical();
//! let input = model.input::<18, 4, true>("x", &[16, 32]).unwrap();
//! let frame = model.compute("square", 64).unwrap();
//! let x = frame.read(input.at::<0>()).unwrap();
//! let squared: Fixed<36, 8, true> = frame.product(x, x).unwrap();
//! frame.publish("square", squared).unwrap();
//! let report = frame.finish();
//! report.audit().unwrap();
//! assert_eq!(report.scheduled_cycles(), None);
//! assert_eq!(report.outputs[0].raw, 256);
//! ```
//! ```compile_fail
//! use audited::Model;
//! let mut model = Model::numerical();
//! let frame = model.compute("closed_input_boundary", 64).unwrap();
//! let late_input = model.input::<18,0,true>("late", &[37]);
//! frame.finish();
//! ```
//! ```compile_fail
//! use audited::{Fixed, Model};
//! let mut model = Model::numerical();
//! let frame = model.compute("no_host_predicate", 64).unwrap();
//! let primitive = frame.branch_value(Fixed::<1,0,false>::constant::<1>(),
//!     |_| Ok(37_i128), |_| Ok(42_i128));
//! ```
//! ```compile_fail
//! use audited::Model;
//! let mut model = Model::numerical();
//! let input = model.input::<18,4,true>("no_float_input", &[1.0_f64]);
//! ```
//!
//! ```compile_fail
//! use audited::Fixed;
//! let runtime = std::env::args().count() as i128;
//! let value = Fixed::<18, 4, true>::constant::<runtime>();
//! ```
//! ```compile_fail
//! use audited::Fixed;
//! let value = Fixed::<18, 4, true>::from_raw(123);
//! ```
//! ```compile_fail
//! use audited::Fixed;
//! let a = Fixed::<18, 4, true>::constant::<16>();
//! let quotient = a / a;
//! ```
//! ```compile_fail
//! use audited::Fixed;
//! let a = Fixed::<18, 4, true>::constant::<16>();
//! let sum = a + a;
//! ```
//! ```compile_fail
//! use audited::Fixed;
//! let a = Fixed::<18, 4, true>::constant::<16>();
//! let raw = a.raw();
//! ```
//! ```compile_fail
//! use audited::{Model,Hardware,Limits};
//! let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
//! let frame = model.begin_frame("bad",Limits { max_cycle: 32, max_events: 128 });
//! let output = frame.add::<18,0,true>(123_i128,456_i128);
//! ```
//! ```compile_fail
//! use audited::Fixed;
//! let forged = Fixed::<18,0,true> { bits: 37, origin: todo!() };
//! ```
//! ```compile_fail
//! use audited::Fixed;
//! let a = Fixed::<18,0,true>::constant::<37>();
//! let primitive_comparison = a < a;
//! ```
//! ```compile_fail,E0277
//! use audited::FixedValue;
//! struct Forged;
//! impl FixedValue for Forged {}
//! ```
//! ```compile_fail
//! use audited::{Fixed, Frame, Memory};
//! fn wrong_format(frame: &Frame<'_>, memory: Memory<18,4,true>) {
//!     frame.write(memory.at::<0>(), Fixed::<18,8,true>::constant::<16>());
//! }
//! ```
//! ```compile_fail
//! use audited::Memory;
//! fn runtime_host_address(memory: Memory<18,4,true>, row: usize) {
//!     let address = memory.at::<row>();
//! }
//! ```
//! ```compile_fail
//! use audited::Memory;
//! fn primitive_address(memory: Memory<18,4,true>, row: usize) {
//!     let address = memory.indexed(row);
//! }
//! ```
//! ```compile_fail
//! use audited::{Fixed, Memory};
//! fn fractional_address(memory: Memory<18,4,true>) {
//!     let address = memory.indexed(Fixed::<18,1,false>::constant::<1>());
//! }
//! ```
#![forbid(unsafe_code)]

mod arithmetic;
mod model;
#[cfg(test)]
extern crate self as audited;
#[cfg(test)]
#[path = "../examples/support/triangle.rs"]
mod triangle;

use std::collections::BTreeMap;
use std::fmt;

pub mod flow;
pub mod lifecycle;
pub mod physical;

pub use model::{
    Address, Event, Frame, FrameReport, Memory, MemoryKind, Model, Observation, Operation, ValueId,
};

/// Numerical work has no cycle meaning. Scheduled mode additionally binds hardware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionMode {
    Numerical,
    Scheduled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Format {
    pub bits: u32,
    pub fraction: u32,
    pub signed: bool,
}
impl Format {
    const fn valid(self) -> bool {
        self.bits > 0 && self.bits <= 126 && self.fraction <= 126
    }
    const fn fits(self, raw: i128) -> bool {
        if !self.valid() {
            return false;
        }
        if self.signed {
            raw >= -(1_i128 << (self.bits - 1)) && raw < 1_i128 << (self.bits - 1)
        } else {
            raw >= 0 && raw < 1_i128 << self.bits
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Origin {
    Constant,
    Event {
        frame: u64,
        value: ValueId,
        ready: u64,
    },
}

/// A data value, never a host integer. Copying preserves its provenance.
#[derive(Clone, Copy)]
pub struct Fixed<const BITS: u32, const FRAC: u32, const SIGNED: bool> {
    bits: i128,
    origin: Origin,
}
impl<const B: u32, const F: u32, const S: bool> Fixed<B, F, S> {
    pub const FORMAT: Format = Format {
        bits: B,
        fraction: F,
        signed: S,
    };
    /// Compile-time format metadata as a literal, never a runtime data constructor.
    pub const MSB_EXPONENT: Fixed<18, 0, true> = {
        assert!(Self::FORMAT.valid(), "invalid fixed-point format");
        Fixed {
            bits: B as i128 - 1 - F as i128,
            origin: Origin::Constant,
        }
    };
    pub const fn constant<const RAW: i128>() -> Self {
        const {
            assert!(Self::FORMAT.fits(RAW), "invalid fixed-point literal");
        }
        Self {
            bits: RAW,
            origin: Origin::Constant,
        }
    }
}
impl<const B: u32, const F: u32, const S: bool> fmt::Debug for Fixed<B, F, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fixed")
            .field("format", &Self::FORMAT)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct Operand {
    bits: i128,
    format: Format,
    origin: Origin,
}
impl Operand {
    /// Type metadata only: does not expose the runtime value or provenance.
    pub const fn format(self) -> Format {
        self.format
    }
}
mod sealed {
    pub trait Sealed {
        fn operand(&self) -> super::Operand;
    }
}
/// Sealed: callers cannot implement another data type or forge an operand.
#[allow(private_bounds)]
pub trait FixedValue: sealed::Sealed {}
impl<const B: u32, const F: u32, const S: bool> sealed::Sealed for Fixed<B, F, S> {
    fn operand(&self) -> Operand {
        Operand {
            bits: self.bits,
            format: Self::FORMAT,
            origin: self.origin,
        }
    }
}
impl<const B: u32, const F: u32, const S: bool> FixedValue for Fixed<B, F, S> {}
impl sealed::Sealed for Operand {
    fn operand(&self) -> Operand {
        *self
    }
}
impl FixedValue for Operand {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Resource {
    Dsp18,
    Dsp36,
    Adder(u32),
    Compare(u32),
    RoundControl(u32),
    Select(u32),
    LeadingZeros(u32),
    Shift(u32),
    Read(usize),
    Write(usize),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unit {
    pub lanes: usize,
    pub latency: u64,
    pub initiation: u64,
}
impl Unit {
    pub const fn pipelined(lanes: usize, latency: u64) -> Self {
        Self {
            lanes,
            latency,
            initiation: 1,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Hardware {
    pub units: BTreeMap<Resource, Unit>,
}
impl Hardware {
    /// User DSP18 accounting: 36x36=4; two true 18x18 lanes=1+1.
    /// The adders are separate fixed widths, not one universal shared ALU.
    pub fn one_wide_two_narrow() -> Self {
        Self {
            units: [
                (Resource::Dsp36, Unit::pipelined(1, 3)),
                (Resource::Dsp18, Unit::pipelined(2, 2)),
                (Resource::Adder(18), Unit::pipelined(1, 1)),
                (Resource::Adder(36), Unit::pipelined(1, 1)),
                (Resource::Adder(54), Unit::pipelined(1, 1)),
                (Resource::Compare(18), Unit::pipelined(1, 1)),
                (Resource::RoundControl(36), Unit::pipelined(1, 1)),
                (Resource::RoundControl(54), Unit::pipelined(1, 1)),
                (Resource::Select(18), Unit::pipelined(1, 1)),
                (Resource::Select(36), Unit::pipelined(1, 1)),
                (Resource::Select(54), Unit::pipelined(1, 1)),
                (Resource::LeadingZeros(18), Unit::pipelined(1, 1)),
                (Resource::Shift(18), Unit::pipelined(1, 1)),
                (Resource::Shift(36), Unit::pipelined(1, 1)),
                (Resource::Shift(54), Unit::pipelined(1, 1)),
            ]
            .into_iter()
            .collect(),
        }
    }
    pub fn dsp18_units(&self) -> usize {
        self.units.get(&Resource::Dsp36).map_or(0, |u| 4 * u.lanes)
            + self.units.get(&Resource::Dsp18).map_or(0, |u| u.lanes)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct PortShape {
    pub read_ports: usize,
    pub write_ports: usize,
    pub read_latency: u64,
    pub max_reads_per_frame: u64,
    pub max_writes_per_frame: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_cycle: u64,
    pub max_events: usize,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fault {
    Format,
    Range,
    ForeignValue,
    ForeignMemory,
    Uninitialized,
    Address,
    ReadOnly,
    MissingResource,
    PortFrameLimit,
    Deadline,
    EventLimit,
    Audit(String),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductRoute {
    Native18,
    Wide36,
    Native18Pair,
}

#[cfg(test)]
mod tests;
