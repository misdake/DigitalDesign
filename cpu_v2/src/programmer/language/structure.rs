#[allow(unused_imports)] // used in define_struct
use crate::dsl::DslPtr;

pub trait DslStruct {
    const SIZE: usize;
}

#[allow(unused_macros)] // used in define_struct
macro_rules! count {
    () => (0usize);
    ( $x:tt $($xs:tt)* ) => (1usize + count!($($xs)*));
}

#[macro_export]
macro_rules! define_struct {
    ($struct_name:ident { $($field_name:ident),+ }) => {
        #[allow(unused)]
        pub struct $struct_name {
            $($field_name: DslPtr,)+
        }
        impl $struct_name {
            #[allow(unused)]
            pub fn new(mut ptr: DslPtr) -> Self {
                $( let $field_name = ptr ; ptr += 1; )+
                Self {
                    $($field_name,)+
                }
            }
        }
        impl DslStruct for $struct_name {
            const SIZE: usize = count!($($field_name)+);
        }
    };
}

#[test]
fn test_struct() {
    define_struct!(Vec2 { x, y });

    use crate::programmer::language::dsl::*;
    use crate::programmer::language::*;
    let func = DslFunction::new("test_struct", [], []);

    let mut compiler = Compiler::default();
    compiler.func_op(
        &func.func_decl,
        func.define(|[], _ret| {
            let base = DslArray::<{ Vec2::SIZE }>::new(DslPtr::new(v(555)));

            let vec2 = Vec2::new(base.index_imm(1));
            vec2.x.write(v(123));
            vec2.y.write(v(456));

            let x = vec2.x.read();

            halt_with_signal(x);
        }),
    );

    let instructions = compiler.finish("test_struct");
    let (state, signal) = simulate(&instructions, 1000);
    assert_eq!(signal, Some(123));
    assert_eq!(state.mem[555], 0);
    assert_eq!(state.mem[556], 0);
    assert_eq!(state.mem[557], 123);
    assert_eq!(state.mem[558], 456);
    assert_eq!(state.mem[559], 0);
}
