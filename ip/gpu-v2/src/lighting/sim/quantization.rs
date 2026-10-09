//! Local arithmetic adapter. Every operation is still emitted by audited Frame;
//! this adapter selects the numerical contract before the DAG is constructed.
use crate::lighting::LightingQuantization;
use audited::{Fault, Fixed, FixedValue, Frame};
use std::ops::Deref;

pub(super) struct Arithmetic<'f, 'm> {
    pub frame: &'f Frame<'m>,
    pub policy: LightingQuantization,
}
impl<'m> Deref for Arithmetic<'_, 'm> {
    type Target = Frame<'m>;
    fn deref(&self) -> &Self::Target {
        self.frame
    }
}
impl Arithmetic<'_, '_> {
    #[track_caller]
    pub fn round_to<const B: u32, const F: u32, const S: bool>(
        &self,
        a: impl FixedValue,
    ) -> Result<Fixed<B, F, S>, Fault> {
        if self.policy == LightingQuantization::NearestEven {
            return self.frame.round_to(a);
        }
        let source = a.operand().format();
        if B == 9 && F == 8 && !S && source.bits == 18 && source.fraction == 16 && !source.signed {
            self.frame.round_to(a)
        } else if F == 8 && !(source.bits == 16 && source.fraction == 15 && !source.signed) {
            self.frame.half_up_to(a)
        } else {
            self.frame.floor_to(a)
        }
    }
    pub fn branch(
        &self,
        p: Fixed<1, 0, false>,
        yes: impl FnOnce(&Self) -> Result<(), Fault>,
        no: impl FnOnce(&Self) -> Result<(), Fault>,
    ) -> Result<(), Fault> {
        self.frame.branch(p, |_| yes(self), |_| no(self))
    }
    pub fn branch_value<const B: u32, const F: u32, const S: bool>(
        &self,
        p: Fixed<1, 0, false>,
        yes: impl FnOnce(&Self) -> Result<Fixed<B, F, S>, Fault>,
        no: impl FnOnce(&Self) -> Result<Fixed<B, F, S>, Fault>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.frame.branch_value(p, |_| yes(self), |_| no(self))
    }
    pub fn floor_intermediates(&self) -> bool {
        self.policy == LightingQuantization::CompensatedFloor
    }
}
