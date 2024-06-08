use crate::programmer::*;
use crate::FuncName;

pub struct Relocation {
    func_name: FuncName,
    slots: [InstructionSlot; 3],
}

impl RegisterOperation {
    /// convert register operation to instructions, write to assembler, return relocation info
    pub fn write_assembly(
        ops: &Vec<RegisterOperation>,
        assembler1: &mut Assembler1,
        start_address: usize,
    ) -> Vec<Relocation> {
        assembler1.set_cursor(start_address);

        let mut r = vec![];
        for op in ops {
            Self::write_asm_inner(op, assembler1, &mut r);
        }

        r
    }
    fn write_asm_inner(op: &RegisterOperation, asm: &mut Assembler1, r: &mut Vec<Relocation>) {
        use crate::Instruction::*;
        match op {
            RegisterOperation::Result(op, dst) => {
                match *op {
                    ResultOp::Add(v2, v1) => asm.inst(add(v2.0, v1.0, dst.0)),
                    ResultOp::Addi(v2, v) => asm.inst(addi(v2.0, v, dst.0)),
                    ResultOp::LoadMem(base, offset) => asm.inst(load_mem(base.0, offset, dst.0)),
                };
            }
            RegisterOperation::Update(op) => {
                match *op {
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
            RegisterOperation::Func(_name, _ra, _param) => {}
            RegisterOperation::Call(name, _param, _rv) => {
                r.push(Relocation {
                    func_name: name,
                    slots: [asm.skip(), asm.skip(), asm.skip()], // load_lo(tmp), load_hi(tmp), call_reg(tmp, r13)
                });
            }
            RegisterOperation::Return(ra, _rv) => {
                asm.inst(jmp_reg(ra.0));
            }
        }
    }
}

fn u8_to_hi_lo(v: u8) -> (u8, u8) {
    (v >> 4 & 0xf, v & 0xf)
}
