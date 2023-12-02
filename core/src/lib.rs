#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![allow(clippy::manual_memcpy)]
#![allow(clippy::needless_range_loop)]

mod basic;
mod component_lib;
mod export;
mod external;
mod reg;
mod wires;

pub use basic::*;
pub use component_lib::*;
pub use export::*;
pub use external::*;
pub use reg::*;
pub use wires::*;

mod tests;
pub use tests::*;

use std::sync::{LockResult, Mutex, MutexGuard};
static GLOBAL_LOCK: Mutex<()> = Mutex::new(());
pub fn global_lock() -> LockResult<MutexGuard<'static, ()>> {
    GLOBAL_LOCK.lock()
}

pub fn select<T>(b: bool, t: T, f: T) -> T {
    if b {
        t
    } else {
        f
    }
}
