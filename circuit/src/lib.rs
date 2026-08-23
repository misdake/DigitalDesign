#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![allow(clippy::manual_memcpy)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::bool_to_int_with_if)]
#![allow(clippy::manual_range_contains)]

mod basic;
mod component;
mod component_lib;
mod export;
mod external;
mod reg;
mod wires;

pub use basic::*;
pub use component::*;
pub use component_lib::*;
pub use export::*;
pub use external::*;
pub use reg::*;
pub use wires::*;

mod tests;
pub use tests::*;

pub fn select<T>(b: bool, t: T, f: T) -> T {
    if b {
        t
    } else {
        f
    }
}
