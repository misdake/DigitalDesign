use crate::programmer::*;
use crate::FuncName;

#[derive(Copy, Clone, Debug)]
pub struct Relocation {
    pub func_name: FuncName,
    pub slots: [InstructionSlot; 2], // load_lo(tmp), load_hi(tmp)
}

impl RegisterOperation {
    /// convert register operation to instructions, write to assembler, return relocation info
    pub fn write_function_assembly(
        ops: &Vec<RegisterOperation>,
        assembler: &mut Assembler,
        start_address: usize,
    ) -> Vec<Relocation> {
        assembler.set_cursor(start_address);

        let mut r = vec![];
        for op in ops {
            Self::write_asm_inner(op, assembler, &mut r);
        }

        r
    }
    fn write_asm_inner(op: &RegisterOperation, asm: &mut Assembler, r: &mut Vec<Relocation>) {
        use crate::Instruction::*;
        match op {
            RegisterOperation::Result(op, dst) => {
                match *op {
                    ResultOp::Inv(v) => asm.inst(inv(v.0, dst.0)),
                    ResultOp::Neg(v) => asm.inst(neg(v.0, dst.0)),
                    ResultOp::Not0(v) => asm.inst(not0(v.0, dst.0)),
                    ResultOp::Cnt1(v) => asm.inst(cnt1(v.0, dst.0)),
                    ResultOp::Log2(v) => asm.inst(log2(v.0, dst.0)),
                    ResultOp::Add(v2, v1) => asm.inst(add(v2.0, v1.0, dst.0)),
                    ResultOp::Addi(v2, v) => asm.inst(addi(v2.0, v, dst.0)),
                    ResultOp::LoadMem(base, offset) => asm.inst(load_mem(base.0, offset, dst.0)),
                };
            }
            RegisterOperation::Update(op) => {
                match *op {
                    UpdateOp::Inv(dst) => asm.inst(inv(dst.0, dst.0)),
                    UpdateOp::Neg(dst) => asm.inst(neg(dst.0, dst.0)),
                    UpdateOp::Not0(dst) => asm.inst(not0(dst.0, dst.0)),
                    UpdateOp::Cnt1(dst) => asm.inst(cnt1(dst.0, dst.0)),
                    UpdateOp::Log2(dst) => asm.inst(log2(dst.0, dst.0)),
                    UpdateOp::LoadImmLo(dst, v) => {
                        let (hi, lo) = u8_to_hi_lo(v);
                        asm.inst(load_lo(hi, lo, dst.0))
                    }
                    UpdateOp::LoadImmHi(dst, v) => {
                        let (hi, lo) = u8_to_hi_lo(v);
                        asm.inst(load_hi(hi, lo, dst.0))
                    }
                    UpdateOp::Mov(dst, v) => asm.inst(mov(v.0, dst.0)),
                    UpdateOp::AddAssign(dst, v) => asm.inst(add(v.0, dst.0, dst.0)),
                    UpdateOp::AddiAssign(dst, v) => asm.inst(addi(dst.0, v, dst.0)),
                    UpdateOp::SubiAssign(dst, v) => asm.inst(subi(dst.0, v, dst.0)),
                    UpdateOp::StoreMem(base, offset, v) => asm.inst(store_mem(base.0, offset, v.0)),
                    UpdateOp::Halt() => asm.inst(halt()),
                };
            }
            RegisterOperation::List(list) => {
                for op in list {
                    Self::write_asm_inner(op, asm, r);
                }
            }
            RegisterOperation::If(cond, then_block, else_block) => {
                let cond = *cond;
                let mut relocation1 = vec![];
                let mut relocation2 = vec![];

                if else_block.is_some() {
                    match cond {
                        CondOp::Cmp(r0, r1, cond) => asm.if_else_reg(
                            r0.0,
                            r1.0,
                            cond,
                            |asm| {
                                Self::write_asm_inner(then_block, asm, &mut relocation1);
                            },
                            |asm| {
                                Self::write_asm_inner(
                                    else_block.as_ref().unwrap(),
                                    asm,
                                    &mut relocation2,
                                );
                            },
                        ),
                        CondOp::CmpI(r0, u4, cond) => asm.if_else_u4(
                            r0.0,
                            u4,
                            cond,
                            |asm| {
                                Self::write_asm_inner(then_block, asm, &mut relocation1);
                            },
                            |asm| {
                                Self::write_asm_inner(
                                    else_block.as_ref().unwrap(),
                                    asm,
                                    &mut relocation2,
                                );
                            },
                        ),
                    }
                } else {
                    match cond {
                        CondOp::Cmp(r0, r1, cond) => asm.if_reg(r0.0, r1.0, cond, |asm| {
                            Self::write_asm_inner(then_block, asm, &mut relocation1);
                        }),
                        CondOp::CmpI(r0, u4, cond) => asm.if_u4(r0.0, u4, cond, |asm| {
                            Self::write_asm_inner(then_block, asm, &mut relocation1);
                        }),
                    }
                }

                r.extend(relocation1);
                r.extend(relocation2);
            }
            RegisterOperation::Loop(cond, loop_block) => {
                let cond = *cond;
                match cond {
                    CondOp::Cmp(r0, r1, cond) => asm.loop_reg(r0.0, r1.0, cond, |asm| {
                        Self::write_asm_inner(loop_block, asm, r);
                    }),
                    CondOp::CmpI(r0, u4, cond) => asm.loop_u4(r0.0, u4, cond, |asm| {
                        Self::write_asm_inner(loop_block, asm, r);
                    }),
                }
            }
            RegisterOperation::Func(name, _ra, _param) => {
                let slot = InstructionSlot::new(asm.get_cursor());
                asm.comment_at(slot, format!("fn {name}")); //TODO write param+return?
            }
            RegisterOperation::Call(name, _param, _rv) => {
                r.push(Relocation {
                    func_name: name,
                    slots: [asm.skip(), asm.skip()], // load_lo(tmp), load_hi(tmp)
                });
                asm.inst(call_reg(TMP_REG, RETURN_ADDR_REG)); // call_reg(tmp, r13)
            }
            RegisterOperation::Return(ra, _rv) => {
                asm.inst(jmp_reg(ra.0));
            }
        }
    }
}

pub fn u16_to_hi_lo(v: u16) -> (u8, u8) {
    ((v >> 8 & 0xff) as u8, (v & 0xff) as u8)
}
pub fn u8_to_hi_lo(v: u8) -> (u8, u8) {
    (v >> 4 & 0xf, v & 0xf)
}

#[cfg(test)]
fn test_program((vo1, decl): (VariableOperation1, FuncDecl)) {
    let vo2s = VariableOperation2Scope::from(vo1);
    let vo3 = VariableOperation3::from(vo2s);

    use std::rc::Rc;
    let mut allocator = RegisterAllocator::new(Rc::new(default_reg_usages()), decl);
    let ops = allocator.run(&vo3);

    let mut asm = Assembler::default();
    let relocations = RegisterOperation::write_function_assembly(&ops, &mut asm, 0);
    let end = asm.get_cursor();

    let instructions = asm.slice_ref();
    let instructions = &instructions[0..end];

    for (addr, inst) in instructions.iter().enumerate() {
        println!("inst {addr:04x}: {inst}");
    }

    println!("relocations: {relocations:#?}");
}

#[test]
fn test_basic() {
    test_program(vo1_basic_program());
}
