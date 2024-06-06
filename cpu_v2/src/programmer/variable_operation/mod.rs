pub mod op;
mod variable;
mod variable_operation1;
mod variable_operation2;
mod variable_operation3;

pub use op::*;
pub use variable::*;
pub use variable_operation1::*;
pub use variable_operation2::*;
pub use variable_operation3::*;

#[derive(Clone, Debug)]
pub struct FuncDecl {
    pub func_name: FuncName,
    pub param_names: Vec<&'static str>,
    pub return_value_names: Vec<&'static str>,
}

impl FuncDecl {
    pub fn new(
        func_name: FuncName,
        param_names: &[&'static str],
        return_value_names: &[&'static str],
    ) -> Self {
        Self {
            func_name,
            param_names: Vec::from(param_names),
            return_value_names: Vec::from(return_value_names),
        }
    }
}
