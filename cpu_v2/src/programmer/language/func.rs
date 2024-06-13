use crate::programmer::language::push_op;
use crate::*;

pub struct ProgramFunction<const PARAM: usize, const RETURN: usize> {
    pub name: &'static str,
    pub param_names: [&'static str; PARAM],
    pub return_names: [&'static str; RETURN],
    pub func_decl: FuncDecl,
}

impl<const PARAM: usize, const RETURN: usize> ProgramFunction<PARAM, RETURN> {
    pub fn new(
        name: &'static str,
        param_names: [&'static str; PARAM],
        return_names: [&'static str; RETURN],
    ) -> Self {
        let func_decl = FuncDecl::new(name, &param_names, &return_names);

        Self {
            name,
            param_names,
            return_names,
            func_decl,
        }
    }

    pub fn define(
        &self,
        f: impl FnOnce([Variable; PARAM], &dyn Fn([Variable; RETURN])),
    ) -> VariableOperation1 {
        let params = [0; PARAM].map(|_| Variable::new());
        let return_addr = Variable::new();

        compose_variable_operations(|| {
            push_op(VariableOperation1::Func(
                self.name,
                return_addr,
                func_params(params),
            ));

            f(params, &|rv| {
                push_op(VariableOperation1::Return(return_addr, return_values(rv)));
            });
        })
    }

    pub fn call(&self, param: [Variable; PARAM]) -> [Variable; RETURN] {
        let rv = [0; RETURN].map(|_| Variable::new());
        push_op(VariableOperation1::Call(
            self.name,
            func_params(param),
            return_values(rv),
        ));
        rv
    }
}
