use crate::programmer::*;

#[derive(Copy, Clone, Debug)]
pub enum RegisterOperation1 {
    Result(ResultOp<Reg>, Reg),
    Update(UpdateOp<Reg>),
}
