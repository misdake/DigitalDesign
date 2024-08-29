pub mod op;
mod variable;
mod variable_operation1;
mod variable_operation2;
mod variable_operation3;

use arrayvec::ArrayVec;
pub use op::*;
pub use variable::*;
pub use variable_operation1::*;
pub use variable_operation2::*;
pub use variable_operation3::*;

#[derive(Clone, Debug)]
pub struct FuncDecl {
    pub func_name: FuncName,
    pub param_names: ArrayVec<&'static str, 4>,
    pub return_value_names: ArrayVec<&'static str, 4>,
}

impl FuncDecl {
    pub fn new(
        func_name: FuncName,
        param_names: &[&'static str],
        return_value_names: &[&'static str],
    ) -> Self {
        Self {
            func_name,
            param_names: ArrayVec::from_iter(param_names.iter().cloned()),
            return_value_names: ArrayVec::from_iter(return_value_names.iter().cloned()),
        }
    }
}
