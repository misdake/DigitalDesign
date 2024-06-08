use crate::{FuncDecl, FuncName, RegisterOperation};
use std::collections::HashMap;

pub struct Linker {
    functions: HashMap<FuncName, (FuncDecl, Vec<RegisterOperation>)>,
}
