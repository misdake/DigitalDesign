//! Instruction selection and symbolic function bodies.
use super::abi::*;
use crate as cpu_v3;
use crate::{
    AluOp, FpuAuxKind, FpuAuxSubop, FpuScalarSubop, FpuSinCosMode, FpuVectorLength, FpuVectorSubop,
    ImmediateOp, SpecialRegister, TestCondition, Word,
};
use crate::{CACHE_MAINTENANCE_DEVICE, D_INVALIDATE_ALL, ICACHE_INVALIDATE_ALL_DELAYED};
use rcc::*;

#[derive(Clone)]
pub(super) enum Line {
    Word {
        word: Word,
        line: Option<u32>,
    },
    Label(usize),
    Branch {
        condition: TestCondition,
        target: usize,
        line: Option<u32>,
        /// one-word i8 relative form selected by relaxation
        short: bool,
    },
    Jump {
        target: usize,
        line: Option<u32>,
        short: bool,
    },
    Call {
        function: FuncName,
        line: Option<u32>,
        short: bool,
    },
    LoadFunctionAddress {
        function: FuncName,
        dst: u8,
        line: Option<u32>,
    },
}

impl Line {
    pub(super) fn size(&self) -> usize {
        match self {
            Self::Word { .. } => 1,
            Self::Label(_) => 0,
            Self::Branch { short, .. } | Self::Jump { short, .. } | Self::Call { short, .. } => {
                if *short {
                    1
                } else {
                    2
                }
            }
            Self::LoadFunctionAddress { .. } => 2,
        }
    }
}

/// Lowers instructions into `Line`s while stamping each with the source line
/// of the instruction that produced it (for the debug line map). `None` marks
/// compiler-generated code with no source ownership.
struct Lines {
    items: Vec<Line>,
    cur_line: Option<u32>,
}

impl Lines {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            cur_line: None,
        }
    }

    fn set_line(&mut self, line: Option<u32>) {
        self.cur_line = line;
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    fn label(&mut self, block: usize) {
        self.items.push(Line::Label(block));
    }

    fn word(&mut self, word: Word) {
        self.items.push(Line::Word {
            word,
            line: self.cur_line,
        });
    }

    fn branch(&mut self, condition: TestCondition, target: usize) {
        self.items.push(Line::Branch {
            condition,
            target,
            line: self.cur_line,
            short: false,
        });
    }

    fn jump(&mut self, target: usize) {
        self.items.push(Line::Jump {
            target,
            line: self.cur_line,
            short: false,
        });
    }

    fn call(&mut self, function: FuncName) {
        self.items.push(Line::Call {
            function,
            line: self.cur_line,
            short: false,
        });
    }

    fn load_function_address(&mut self, function: FuncName, dst: u8) {
        self.items.push(Line::LoadFunctionAddress {
            function,
            dst,
            line: self.cur_line,
        });
    }
}

pub(super) struct LoweredFunction {
    pub(super) name: FuncName,
    pub(super) lines: Vec<Line>,
    pub(super) static_addresses: Vec<u16>,
    pub(super) frame_size: usize,
    pub(super) callee_saved: usize,
}

pub(super) fn lower_function(
    function: &IrFunc,
    allocation: &Allocation,
    is_main: bool,
    stack_init: u16,
) -> LoweredFunction {
    let mut lines = Lines::new();
    let mut static_addresses = vec![];
    let register = |vreg: VReg| allocation.reg[&vreg];

    // prologue is compiler-generated (no source line)
    lines.set_line(None);
    if is_main && stack_init != 0 {
        emit_load_immediate(&mut lines, REG_SP, stack_init);
    }
    if allocation.frame_size() != 0 {
        emit_immediate(
            &mut lines,
            ImmediateOp::Sub,
            REG_SP,
            allocation.frame_size() as u16,
            true,
        );
        for (slot, &register) in allocation.callee_saved.iter().enumerate() {
            emit_store(&mut lines, register, REG_SP, slot as i16);
        }
    }

    let layout = function.rpo();
    let mut return_tail = Lines::new();
    for (slot, &register) in allocation.callee_saved.iter().enumerate() {
        emit_load(&mut return_tail, register, REG_SP, slot as i16);
    }
    if allocation.frame_size() != 0 {
        emit_immediate(
            &mut return_tail,
            ImmediateOp::Add,
            REG_SP,
            allocation.frame_size() as u16,
            true,
        );
    }
    return_tail.word(cpu_v3::jump_register(REG_LINK));
    let returns = layout
        .iter()
        .copied()
        .filter(|&b| matches!(function.blocks[b].term, Some(Terminator::Ret { .. })))
        .collect::<Vec<_>>();
    // A relaxed jump costs at most two words. Share only longer tails, keeping
    // the last return in place so that its ordinary path needs no extra jump.
    let shared_return =
        (returns.len() > 1 && return_tail.items.len() > 2).then(|| *returns.last().unwrap());
    let return_label = function.blocks.len();
    for (layout_index, &block_id) in layout.iter().enumerate() {
        lines.label(block_id);
        let block = &function.blocks[block_id];
        if block.preds.len() == 1 && !block.phis.is_empty() {
            let predecessor = block.preds[0];
            let moves = block
                .phis
                .iter()
                .map(|phi| {
                    let value = phi
                        .args
                        .iter()
                        .find(|(pred, _)| *pred == predecessor)
                        .expect("missing phi predecessor")
                        .1;
                    (value, phi.dst)
                })
                .collect::<Vec<_>>();
            lines.set_line(None);
            emit_phi_moves(function, &moves, &register, &mut lines);
        }

        for (index, instruction) in block.insts.iter().enumerate() {
            lines.set_line(block.lines.get(index).copied().flatten());
            lower_instruction(
                function,
                instruction,
                &register,
                allocation,
                &mut lines,
                &mut static_addresses,
            );
        }

        lines.set_line(block.term_line);
        match block
            .term
            .as_ref()
            .expect("reachable block is unterminated")
        {
            Terminator::Jmp { target } => {
                emit_edge_moves(function, block_id, *target, &register, &mut lines);
                if layout.get(layout_index + 1) != Some(target) {
                    lines.jump(*target);
                }
            }
            Terminator::Br {
                cmp,
                if_true,
                if_false,
            } => {
                let next = layout.get(layout_index + 1).copied();
                if if_true == if_false {
                    emit_edge_moves(function, block_id, *if_true, &register, &mut lines);
                    if next != Some(*if_true) {
                        lines.jump(*if_true);
                    }
                } else {
                    // Critical edges were split by allocation, so neither
                    // conditional successor needs edge-local phi moves here.
                    let condition = lower_comparison(function, cmp, &register, &mut lines);
                    if next == Some(*if_false) {
                        lines.branch(condition, *if_true);
                    } else if next == Some(*if_true) {
                        lines.branch(condition.invert(), *if_false);
                    } else {
                        lines.branch(condition, *if_true);
                        lines.jump(*if_false);
                    }
                }
            }
            Terminator::Ret { .. } => {
                if shared_return.is_some_and(|b| b != block_id) {
                    lines.jump(return_label);
                } else {
                    if shared_return.is_some() {
                        lines.label(return_label);
                    }
                    lines.items.extend(return_tail.items.iter().cloned());
                }
            }
            Terminator::Halt { signal } => {
                let signal = register(*signal);
                if signal != 0 {
                    lines.word(cpu_v3::move_register(0, signal));
                }
                lines.word(cpu_v3::halt());
            }
            Terminator::IcacheInvalidateDelayedAndJump { cseg, target } => {
                let cseg = register(*cseg);
                let target = register(*target);
                lines.word(cpu_v3::device_send(
                    cseg,
                    CACHE_MAINTENANCE_DEVICE,
                    ICACHE_INVALIDATE_ALL_DELAYED,
                ));
                lines.word(cpu_v3::jump_segment(cseg, target));
            }
        }
    }

    LoweredFunction {
        name: function.name,
        lines: lines.items,
        static_addresses,
        frame_size: allocation.frame_size(),
        callee_saved: allocation.callee_saved.len(),
    }
}

fn lower_instruction(
    function: &IrFunc,
    instruction: &Instr,
    register: &dyn Fn(VReg) -> u8,
    allocation: &Allocation,
    lines: &mut Lines,
    static_addresses: &mut Vec<u16>,
) {
    match instruction {
        Instr::Bin { dst, op, lhs, rhs } => {
            let dst = register(*dst);
            let lhs = register(*lhs);
            match rhs {
                IntOperand::Reg(rhs) => {
                    let rhs = register(*rhs);
                    let operation = match op {
                        BinOp::Add => AluOp::Add,
                        BinOp::Sub => AluOp::Sub,
                        BinOp::And => AluOp::And,
                        BinOp::Or => AluOp::Or,
                        BinOp::Xor => AluOp::Xor,
                    };
                    lines.word(cpu_v3::alu(operation, dst, lhs, rhs));
                }
                IntOperand::Imm(value) => {
                    // destructive immediate op on rd: lhs moves into dst first.
                    // ADDI/SUBI read an unsigned u4 (`emit_immediate` flips a
                    // negative adjustment to the opposite operation); SEQI/
                    // SLTI keep a signed i4 short form, and ANDI/ORI/XORI read
                    // a zero-extended u4 mask.
                    let (operation, signed_short) = match op {
                        BinOp::Add => (ImmediateOp::Add, true),
                        BinOp::Sub => (ImmediateOp::Sub, true),
                        BinOp::And => (ImmediateOp::And, false),
                        BinOp::Or => (ImmediateOp::Or, false),
                        BinOp::Xor => (ImmediateOp::Xor, false),
                    };
                    if dst != lhs {
                        lines.word(cpu_v3::move_register(dst, lhs));
                    }
                    emit_immediate(lines, operation, dst, *value, signed_short);
                }
            }
        }
        Instr::Mul {
            dst,
            window,
            lhs,
            rhs,
        } => {
            let dst = register(*dst);
            let lhs = register(*lhs);
            let window = match window {
                MulWindow::Low => cpu_v3::MultiplyWindow::Low,
                MulWindow::Shift8 => cpu_v3::MultiplyWindow::Shift8,
                MulWindow::Shift16 => cpu_v3::MultiplyWindow::Shift16,
            };
            match rhs {
                IntOperand::Reg(rhs) => {
                    let rhs = register(*rhs);
                    // destructive multiply is commutative, so a dst == rhs
                    // collision swaps the operands
                    if dst == lhs {
                        lines.word(cpu_v3::multiply(window, dst, rhs));
                    } else if dst == rhs {
                        lines.word(cpu_v3::multiply(window, dst, lhs));
                    } else {
                        lines.word(cpu_v3::move_register(dst, lhs));
                        lines.word(cpu_v3::multiply(window, dst, rhs));
                    }
                }
                IntOperand::Imm(value) => {
                    if matches!(window, cpu_v3::MultiplyWindow::Low) {
                        // MULI: destructive multiply by the unsigned immediate
                        if dst != lhs {
                            lines.word(cpu_v3::move_register(dst, lhs));
                        }
                        emit_muli(lines, dst, *value);
                    } else {
                        // MUL8/MUL16 have no immediate form: materialize first
                        emit_load_immediate(lines, REG_TMP, *value);
                        if dst != lhs {
                            lines.word(cpu_v3::move_register(dst, lhs));
                        }
                        lines.word(cpu_v3::multiply(window, dst, REG_TMP));
                    }
                }
            }
        }
        Instr::Un { dst, op, src } => lower_unary(register(*dst), *op, register(*src), lines),
        Instr::Shift {
            dst,
            op,
            src,
            amount,
        } => {
            let dst = register(*dst);
            let src = register(*src);
            let operation = match op {
                ShiftOp::Lsl => cpu_v3::ShiftOp::Left,
                ShiftOp::Lsr => cpu_v3::ShiftOp::RightLogical,
                ShiftOp::Asr => cpu_v3::ShiftOp::RightArithmetic,
            };
            match amount {
                IntOperand::Imm(amount) => {
                    if dst != src {
                        lines.word(cpu_v3::move_register(dst, src));
                    }
                    lines.word(cpu_v3::shift_immediate(operation, dst, *amount as u8))
                }
                IntOperand::Reg(amount) => {
                    let amount = register(*amount);
                    // when dst aliases the amount register, the MOV into dst
                    // would clobber the amount before the shift reads it
                    if dst == amount {
                        lines.word(cpu_v3::move_register(REG_TMP, amount));
                        if dst != src {
                            lines.word(cpu_v3::move_register(dst, src));
                        }
                        lines.word(cpu_v3::shift_register(operation, dst, REG_TMP));
                    } else {
                        if dst != src {
                            lines.word(cpu_v3::move_register(dst, src));
                        }
                        lines.word(cpu_v3::shift_register(operation, dst, amount));
                    }
                }
            }
        }
        Instr::Signal { signal_type, value } => {
            lines.word(cpu_v3::signal(register(*value), *signal_type));
        }
        Instr::Mfsr { dst, sr } => {
            let sr = match sr {
                SpecialReg::Cseg => SpecialRegister::CodeSegment,
                SpecialReg::Dseg => SpecialRegister::DataSegment,
                SpecialReg::DsegPage(index) => match index {
                    0 => SpecialRegister::DataSegment0,
                    1 => SpecialRegister::DataSegment1,
                    2 => SpecialRegister::DataSegment2,
                    3 => SpecialRegister::DataSegment3,
                    _ => unreachable!("validated DSEG page index"),
                },
            };
            lines.word(cpu_v3::read_special(register(*dst), sr));
        }
        Instr::Bool { dst, cmp } => lower_bool(register(*dst), cmp, function, register, lines),
        Instr::CMov {
            dst,
            cmp,
            src,
            fallback,
        } => {
            if register(*dst) != register(*fallback) {
                lines.word(cpu_v3::move_register(register(*dst), register(*fallback)));
            }
            let condition = lower_comparison(function, cmp, register, lines);
            lines.word(cpu_v3::conditional_move(
                condition,
                register(*dst),
                register(*src),
            ));
        }
        Instr::Mov { dst, src } => {
            let dst = register(*dst);
            let src = register(*src);
            if dst != src {
                lines.word(cpu_v3::move_register(dst, src));
            }
        }
        Instr::LoadImm { dst, value } => emit_load_immediate(lines, register(*dst), *value),
        Instr::LoadMem { dst, base, offset } => {
            emit_load(lines, register(*dst), register(*base), *offset)
        }
        Instr::StoreMem { base, offset, src } => {
            emit_store(lines, register(*src), register(*base), *offset)
        }
        Instr::StoreStatic { addr, value } => {
            static_addresses.push(*addr);
            // __data_init runs at main entry, where the link register is
            // still dead; borrow it as the second scratch next to REG_TMP.
            emit_load_immediate(lines, REG_TMP, *addr);
            emit_load_immediate(lines, REG_LINK, *value);
            emit_store(lines, REG_LINK, REG_TMP, 0);
        }
        Instr::Call { func, .. } => lines.call(func),
        Instr::LoadFuncAddr { dst, func } => lines.load_function_address(func, register(*dst)),
        Instr::CallPtr { .. } => lines.word(cpu_v3::jump_and_link_register(REG_TMP)),
        Instr::DevRecv {
            dst,
            device,
            channel,
        } => {
            check_device(*device);
            lines.word(cpu_v3::device_receive(register(*dst), *device, *channel));
        }
        Instr::DevSend {
            device,
            channel,
            src,
        } => {
            check_device(*device);
            lines.word(cpu_v3::device_send(register(*src), *device, *channel));
        }
        Instr::DcacheInvalidateAll => lines.word(cpu_v3::device_send(
            REG_TMP,
            CACHE_MAINTENANCE_DEVICE,
            D_INVALIDATE_ALL,
        )),
        Instr::DcacheLineCopy {
            source,
            destination,
        } => lines.word(cpu_v3::line_copy(register(*source), register(*destination))),
        Instr::DcacheCleanLine { address } => {
            lines.word(cpu_v3::clean_data_line(register(*address)))
        }
        Instr::DcacheWait => lines.word(cpu_v3::wait_data_cache()),
        Instr::MtsrDseg { src, index } => lines.word(match index {
            Some(index) => cpu_v3::write_data_segment_page(*index, register(*src)),
            None => cpu_v3::write_data_segment(register(*src)),
        }),
        Instr::Jseg { cseg, target } => {
            lines.word(cpu_v3::jump_segment(register(*cseg), register(*target)))
        }
        Instr::LoadSp { dst, slot } => emit_load(
            lines,
            register(*dst),
            REG_SP,
            (allocation.callee_saved.len() as u8 + allocation.local_slots + *slot) as i16,
        ),
        Instr::StoreSp { slot, src } => emit_store(
            lines,
            register(*src),
            REG_SP,
            (allocation.callee_saved.len() as u8 + allocation.local_slots + *slot) as i16,
        ),
        Instr::LoadLocal { dst, slot } => emit_load(
            lines,
            register(*dst),
            REG_SP,
            (allocation.callee_saved.len() as u8 + *slot) as i16,
        ),
        Instr::StoreLocal { slot, src } => emit_store(
            lines,
            register(*src),
            REG_SP,
            (allocation.callee_saved.len() as u8 + *slot) as i16,
        ),
        Instr::AddrOfLocal { dst, slot } => {
            let dst = register(*dst);
            lines.word(cpu_v3::move_register(dst, REG_SP));
            emit_immediate(
                lines,
                ImmediateOp::Add,
                dst,
                u16::from(allocation.callee_saved.len() as u8 + *slot),
                true,
            );
        }
        Instr::FpuBin { dst, op, lhs, rhs } => {
            let subop = match op {
                FpuBinOp::Add => FpuScalarSubop::Add,
                FpuBinOp::Sub => FpuScalarSubop::Sub,
                FpuBinOp::Mul => FpuScalarSubop::Mul,
            };
            emit_fpu_scalar(
                lines,
                register(*lhs),
                register(*rhs),
                register(*dst),
                subop,
                0,
            );
        }
        Instr::FpuUn { dst, op, src } => {
            let subop = match op {
                FpuUnOp::Abs => FpuScalarSubop::Abs,
                FpuUnOp::Neg => FpuScalarSubop::Neg,
                FpuUnOp::Floor => FpuScalarSubop::Floor,
                FpuUnOp::Ceil => FpuScalarSubop::Ceil,
                FpuUnOp::Round => FpuScalarSubop::Round,
                FpuUnOp::Trunc => FpuScalarSubop::Trunc,
            };
            emit_fpu_scalar(lines, register(*src), 0, register(*dst), subop, 0);
        }
        Instr::FpuSpecial { dst, op, src } => {
            let (subop, mode) = match op {
                FpuSpecialOp::Rcp => (FpuScalarSubop::Rcp, 0),
                FpuSpecialOp::Rsqrt => (FpuScalarSubop::Rsqrt, 0),
                FpuSpecialOp::Sin => (FpuScalarSubop::SinCos, FpuSinCosMode::Sin as u8),
                FpuSpecialOp::Cos => (FpuScalarSubop::SinCos, FpuSinCosMode::Cos as u8),
                FpuSpecialOp::SinCos => (FpuScalarSubop::SinCos, FpuSinCosMode::SinCos as u8),
            };
            emit_fpu_scalar(lines, register(*src), 0, register(*dst), subop, mode);
        }
        Instr::FpuMov { dst, src } => {
            let dst = register(*dst);
            let src = register(*src);
            if dst != src {
                emit_fpu_scalar(lines, src, 0, dst, FpuScalarSubop::Mov, 0);
            }
        }
        Instr::FpuFromInt { dst, src_gpr } => {
            emit_fpu_aux(
                lines,
                register(*src_gpr),
                0,
                register(*dst),
                FpuAuxSubop::I16tof,
            );
        }
        Instr::FpuToInt { dst_gpr, src } => {
            emit_fpu_aux(
                lines,
                register(*dst_gpr),
                register(*src),
                0,
                FpuAuxSubop::Ftoi16,
            );
        }
        Instr::FpuFromLo { dst, src_gpr } => {
            emit_fpu_aux(
                lines,
                register(*src_gpr),
                0,
                register(*dst),
                FpuAuxSubop::Ilo2f,
            );
        }
        Instr::FpuFromHi { dst, src, src_gpr } => {
            let dst = register(*dst);
            let src = register(*src);
            // IHI2F reads and writes the same F register, so materialize the
            // low half into dst first.
            if dst != src {
                emit_fpu_scalar(lines, src, 0, dst, FpuScalarSubop::Mov, 0);
            }
            emit_fpu_aux(lines, register(*src_gpr), 0, dst, FpuAuxSubop::Ihi2f);
        }
        Instr::FpuToLo { dst_gpr, src } => {
            emit_fpu_aux(
                lines,
                register(*dst_gpr),
                register(*src),
                0,
                FpuAuxSubop::Flo2i,
            );
        }
        Instr::FpuToHi { dst_gpr, src } => {
            emit_fpu_aux(
                lines,
                register(*dst_gpr),
                register(*src),
                0,
                FpuAuxSubop::Fhi2i,
            );
        }
        Instr::FpuLoad {
            dst,
            base_gpr,
            offset,
        } => {
            let addr = emit_fpu_address(lines, register(*base_gpr), *offset);
            emit_fpu_aux(lines, addr, 0, register(*dst), FpuAuxSubop::Fld);
        }
        Instr::FpuStore {
            base_gpr,
            offset,
            src,
        } => {
            let addr = emit_fpu_address(lines, register(*base_gpr), *offset);
            emit_fpu_aux(lines, addr, register(*src), 0, FpuAuxSubop::Fst);
        }
        Instr::AddrOfFpuSpill { dst, slot } => {
            // dst = align4(sp + fpu_area_offset) + 4 * slot; alignment is
            // computed at run time because nothing guarantees sp mod 4 == 0.
            let dst = register(*dst);
            lines.word(cpu_v3::move_register(dst, REG_SP));
            emit_immediate(
                lines,
                ImmediateOp::Add,
                dst,
                u16::from(allocation.fpu_area_offset()) + 3,
                true,
            );
            emit_load_immediate(lines, REG_TMP, 0xfffc);
            lines.word(cpu_v3::alu(AluOp::And, dst, dst, REG_TMP));
            if *slot != 0 {
                emit_immediate(lines, ImmediateOp::Add, dst, 4 * u16::from(*slot), true);
            }
        }
        Instr::FpuVecBin { dst, op, lhs, rhs } => {
            let subop = match op {
                FpuBinOp::Add => FpuVectorSubop::VAdd,
                FpuBinOp::Sub => FpuVectorSubop::VSub,
                FpuBinOp::Mul => FpuVectorSubop::VMul,
            };
            emit_fpu_vector(
                lines,
                register(*lhs),
                register(*rhs),
                register(*dst),
                vector_length(function.lanes(*dst)),
                subop,
                0,
            );
        }
        Instr::FpuVecMulS { dst, lhs, scalar } => emit_fpu_vector(
            lines,
            register(*lhs),
            register(*scalar),
            register(*dst),
            vector_length(function.lanes(*dst)),
            FpuVectorSubop::VMulS,
            0,
        ),
        Instr::FpuVecUn { dst, op, src } => {
            let subop = match op {
                FpuVecUnOp::Abs => FpuVectorSubop::VAbs,
                FpuVecUnOp::Neg => FpuVectorSubop::VNeg,
            };
            emit_fpu_vector(
                lines,
                register(*src),
                0,
                register(*dst),
                vector_length(function.lanes(*dst)),
                subop,
                0,
            );
        }
        Instr::FpuVecMove { dst, src } => {
            // cycle-safe parallel move: any source/destination overlap is
            // handled with the reserved F63 scratch
            let lanes = function.lanes(*dst);
            assert_eq!(
                lanes,
                function.lanes(*src),
                "FPU vector move width mismatch"
            );
            let from_base = register(*src);
            let to_base = register(*dst);
            let moves = (0..lanes)
                .map(|lane| (from_base + lane, to_base + lane, RegClass::Fpu))
                .collect::<Vec<_>>();
            emit_parallel_moves(lines, &moves);
        }
        Instr::FpuVecConstruct { dst, lanes } => {
            let to_base = register(*dst);
            let moves = lanes
                .iter()
                .enumerate()
                .map(|(lane, &value)| (register(value), to_base + lane as u8, RegClass::Fpu))
                .collect::<Vec<_>>();
            emit_parallel_moves(lines, &moves);
        }
        Instr::FpuVecLane { dst, src, lane } => {
            let src_base = register(*src);
            emit_fpu_scalar(
                lines,
                src_base + *lane,
                0,
                register(*dst),
                FpuScalarSubop::Mov,
                0,
            );
        }
        Instr::FpuDotStore { dst, lhs, rhs } => emit_fpu_vector(
            lines,
            register(*lhs),
            register(*rhs),
            register(*dst),
            vector_length(function.lanes(*lhs)),
            FpuVectorSubop::DotStore,
            0,
        ),
        Instr::FpuVecLoad {
            dst,
            base_gpr,
            offset,
        } => {
            let addr = emit_fpu_address(lines, register(*base_gpr), *offset);
            emit_fpu_aux_vec(
                lines,
                addr,
                0,
                register(*dst),
                FpuAuxSubop::Fld,
                function.lanes(*dst) - 1,
            );
        }
        Instr::FpuVecStore {
            base_gpr,
            offset,
            src,
        } => {
            let addr = emit_fpu_address(lines, register(*base_gpr), *offset);
            emit_fpu_aux_vec(
                lines,
                addr,
                register(*src),
                0,
                FpuAuxSubop::Fst,
                function.lanes(*src) - 1,
            );
        }
    }
}

/// Maps a contiguous lane count (2/3/4) to the VECTOR length field.
fn vector_length(lanes: u8) -> FpuVectorLength {
    match lanes {
        2 => FpuVectorLength::Vec2,
        3 => FpuVectorLength::Vec3,
        4 => FpuVectorLength::Vec4,
        _ => unreachable!("FPU vector lane count {lanes} is not 2/3/4"),
    }
}

/// Emits one two-word FPU VECTOR instruction.
fn emit_fpu_vector(
    lines: &mut Lines,
    fa: u8,
    fb: u8,
    fd: u8,
    len: FpuVectorLength,
    subop: FpuVectorSubop,
    mode: u8,
) {
    for word in cpu_v3::fpu_vector(fa, fb, fd, len, subop, mode) {
        lines.word(word);
    }
}

/// Emits one two-word FPU AUX FLD/FST vector instruction (`mode` is lanes-1).
fn emit_fpu_aux_vec(lines: &mut Lines, x: u8, fa: u8, fd: u8, subop: FpuAuxSubop, mode: u8) {
    for word in cpu_v3::fpu_aux(FpuAuxKind::IntegerRegister, x, fa, fd, subop, mode) {
        lines.word(word);
    }
}

/// Emits one two-word FPU SCALAR instruction. `mode` is zero for every subop
/// except `SINCOS`, where it selects sin, cos, or the dual output.
fn emit_fpu_scalar(lines: &mut Lines, fa: u8, fb: u8, fd: u8, subop: FpuScalarSubop, mode: u8) {
    for word in cpu_v3::fpu_scalar(fa, fb, fd, subop, mode) {
        lines.word(word);
    }
}

/// Emits one two-word FPU AUX instruction (kind `00`, integer register X).
fn emit_fpu_aux(lines: &mut Lines, x: u8, fa: u8, fd: u8, subop: FpuAuxSubop) {
    for word in cpu_v3::fpu_aux(FpuAuxKind::IntegerRegister, x, fa, fd, subop, 0) {
        lines.word(word);
    }
}

/// Materializes `base + offset` into REG_TMP when an FPU load/store needs an
/// address register; `base` itself is returned when the offset is zero.
fn emit_fpu_address(lines: &mut Lines, base: u8, offset: i16) -> u8 {
    if offset == 0 {
        return base;
    }
    lines.word(cpu_v3::move_register(REG_TMP, base));
    emit_immediate(lines, ImmediateOp::Add, REG_TMP, offset as u16, true);
    REG_TMP
}

fn lower_unary(dst: u8, operation: UnOp, src: u8, lines: &mut Lines) {
    match operation {
        UnOp::Inv => lines.word(cpu_v3::not(dst, src)),
        UnOp::Neg => lines.word(cpu_v3::negate(dst, src)),
        UnOp::Cnt1 => lines.word(cpu_v3::population_count(dst, src)),
        UnOp::Sextb => lines.word(cpu_v3::sign_extend_byte(dst, src)),
        UnOp::Clz => lines.word(cpu_v3::leading_zeros(dst, src)),
        UnOp::Log2 => {
            // log2(0) is defined by rcc as zero.
            lines.word(cpu_v3::leading_zeros(dst, src));
            emit_load_immediate(lines, REG_TMP, 15);
            lines.word(cpu_v3::alu(AluOp::Sub, dst, REG_TMP, dst));
            let done = usize::MAX - lines.len();
            emit_test_nonzero(lines, src);
            lines.branch(TestCondition::NotEqual, done);
            emit_load_immediate(lines, dst, 0);
            lines.label(done);
        }
        UnOp::Not0 => {
            if dst != src {
                lines.word(cpu_v3::move_register(dst, src));
            }
            lines.word(cpu_v3::immediate_signed(ImmediateOp::SetEqual, dst, 0));
            lines.word(cpu_v3::immediate_unsigned(ImmediateOp::Xor, dst, 1));
        }
    }
}

/// the hardware condition code for one of the six real predicates
fn test_condition(cond: CompareOp) -> TestCondition {
    match cond {
        CompareOp::Equal => TestCondition::Equal,
        CompareOp::NotEqual => TestCondition::NotEqual,
        CompareOp::Less => TestCondition::LessThan,
        CompareOp::GreaterEqual => TestCondition::GreaterOrEqual,
        CompareOp::Greater => TestCondition::GreaterThan,
        CompareOp::LessEqual => TestCondition::LessOrEqual,
        CompareOp::Never | CompareOp::Always => {
            unreachable!("diamond conversion never emits degenerate conditions")
        }
    }
}

/// Lowers a Boolean-producing comparison (`dst = lhs cond rhs` as 0/1) to the
/// destructive S* instructions; inverted conditions use an operand swap or a
/// trailing XORI 1.
/// kind of a destructive S* operation
#[derive(Clone, Copy)]
enum SetKind {
    Equal,
    Less { signed: bool },
}

impl SetKind {
    fn reg_op(self, dst: u8, rhs: u8) -> Word {
        match self {
            SetKind::Equal => cpu_v3::set_equal(dst, rhs),
            SetKind::Less { signed: true } => cpu_v3::set_less_than_signed(dst, rhs),
            SetKind::Less { signed: false } => cpu_v3::set_less_than_unsigned(dst, rhs),
        }
    }
    fn imm_op(self) -> ImmediateOp {
        match self {
            SetKind::Equal => ImmediateOp::SetEqual,
            SetKind::Less { signed: true } => ImmediateOp::SetLessThanSigned,
            SetKind::Less { signed: false } => ImmediateOp::SetLessThanUnsigned,
        }
    }
    fn signed(self) -> bool {
        match self {
            SetKind::Equal => true,
            SetKind::Less { signed } => signed,
        }
    }
}

/// Emits `dst = left SET_OP right` through the destructive S* forms. The
/// scratch register preserves the right operand when `dst` aliases it.
fn set_op(
    lines: &mut Lines,
    dst: u8,
    left: u8,
    kind: SetKind,
    right: &CmpRhs,
    register: &dyn Fn(VReg) -> u8,
) {
    match right {
        CmpRhs::Reg(r) => {
            let r = register(*r);
            if dst == r {
                // preserve the right operand before dst is overwritten
                lines.word(cpu_v3::move_register(REG_TMP, r));
                if dst != left {
                    lines.word(cpu_v3::move_register(dst, left));
                }
                lines.word(kind.reg_op(dst, REG_TMP));
            } else {
                if dst != left {
                    lines.word(cpu_v3::move_register(dst, left));
                }
                lines.word(kind.reg_op(dst, r));
            }
        }
        CmpRhs::Imm(value) => {
            if dst != left {
                lines.word(cpu_v3::move_register(dst, left));
            }
            emit_immediate(lines, kind.imm_op(), dst, *value, kind.signed());
        }
    }
}

/// Lowers a Boolean-producing comparison (`dst = lhs cond rhs` as 0/1). Integer
/// comparisons use the destructive S* instructions (inverted conditions swap
/// operands or append XORI 1); an FPU comparison sets the pending test with the
/// scalar `CMP` and materializes 0/1 through one conditional move.
fn lower_bool(
    dst: u8,
    cmp: &Cmp,
    function: &IrFunc,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) {
    let lhs = register(cmp.lhs);
    if function.class_of(cmp.lhs) == RegClass::Fpu {
        // `CMP` sets the pending test and the following conditional move
        // consumes it immediately, so the two constants are prepared first.
        let CmpRhs::Reg(rhs) = cmp.rhs else {
            unreachable!("FPU comparisons always have a register operand");
        };
        let condition = test_condition(cmp.cond);
        emit_load_immediate(lines, REG_TMP, 1);
        emit_load_immediate(lines, dst, 0);
        emit_fpu_scalar(lines, lhs, register(rhs), 0, FpuScalarSubop::Cmp, 0);
        lines.word(cpu_v3::conditional_move(condition, dst, REG_TMP));
        return;
    }
    let xor1 = |lines: &mut Lines| {
        lines.word(cpu_v3::immediate_unsigned(ImmediateOp::Xor, dst, 1));
    };
    let less = SetKind::Less { signed: cmp.signed };
    match cmp.cond {
        CompareOp::Equal => set_op(lines, dst, lhs, SetKind::Equal, &cmp.rhs, register),
        CompareOp::NotEqual => {
            set_op(lines, dst, lhs, SetKind::Equal, &cmp.rhs, register);
            xor1(lines);
        }
        CompareOp::Less => set_op(lines, dst, lhs, less, &cmp.rhs, register),
        CompareOp::GreaterEqual => {
            set_op(lines, dst, lhs, less, &cmp.rhs, register);
            xor1(lines);
        }
        // swapped: lhs > rhs is rhs < lhs; lhs <= rhs is !(rhs < lhs)
        CompareOp::Greater | CompareOp::LessEqual => {
            match &cmp.rhs {
                CmpRhs::Reg(r) => {
                    let r = register(*r);
                    set_op(lines, dst, r, less, &CmpRhs::Reg(cmp.lhs), register);
                }
                CmpRhs::Imm(value) => {
                    // dst must start as the immediate; when it aliases lhs the
                    // sequence routes through the scratch register
                    if dst == lhs {
                        emit_load_immediate(lines, REG_TMP, *value);
                        lines.word(less.reg_op(REG_TMP, dst));
                        lines.word(cpu_v3::move_register(dst, REG_TMP));
                    } else {
                        emit_load_immediate(lines, dst, *value);
                        lines.word(less.reg_op(dst, lhs));
                    }
                }
            }
            if matches!(cmp.cond, CompareOp::LessEqual) {
                xor1(lines);
            }
        }
        CompareOp::Never | CompareOp::Always => {
            unreachable!("diamond conversion never emits degenerate conditions")
        }
    }
}

fn check_device(device: u8) {
    assert!(
        device < 8,
        "CpuV3 device index {device} exceeds the ISA v0.6 limit of 8 devices"
    );
}

/// Emits the generic "test a value against zero" comparison used when a
/// branch tests a plain value rather than a comparison outcome.
fn emit_test_nonzero(lines: &mut Lines, test: u8) {
    lines.word(cpu_v3::immediate_signed(
        ImmediateOp::CompareSigned,
        test,
        0,
    ));
}

/// Lowers a branch comparison directly to a CMP-class instruction feeding a
/// conditional branch; no 0/1 value is materialized. Returns the condition
/// that branches when `cmp` holds.
fn lower_comparison(
    function: &IrFunc,
    comparison: &Cmp,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) -> TestCondition {
    let lhs = register(comparison.lhs);
    let condition = match comparison.cond {
        CompareOp::Equal => TestCondition::Equal,
        CompareOp::NotEqual => TestCondition::NotEqual,
        CompareOp::Less => TestCondition::LessThan,
        CompareOp::GreaterEqual => TestCondition::GreaterOrEqual,
        CompareOp::Greater => TestCondition::GreaterThan,
        CompareOp::LessEqual => TestCondition::LessOrEqual,
        // A value always compares Equal to itself, so these degenerate
        // conditions do not depend on the compared values at all.
        CompareOp::Always => {
            emit_self_compare(function, comparison, register, lines);
            return TestCondition::Equal;
        }
        CompareOp::Never => {
            emit_self_compare(function, comparison, register, lines);
            return TestCondition::NotEqual;
        }
    };
    if function.class_of(comparison.lhs) == RegClass::Fpu {
        // The FPU scalar `CMP` sets the pending test from the signed Q16.16
        // ordering; the following conditional branch consumes it exactly like
        // CMPS/CMPU, and nothing may be emitted between the compare and branch.
        let CmpRhs::Reg(rhs) = comparison.rhs else {
            unreachable!("FPU comparisons always have a register operand");
        };
        emit_fpu_scalar(lines, lhs, register(rhs), 0, FpuScalarSubop::Cmp, 0);
        return condition;
    }
    match comparison.rhs {
        CmpRhs::Reg(rhs) => {
            let rhs = register(rhs);
            lines.word(if comparison.signed {
                cpu_v3::compare_signed(lhs, rhs)
            } else {
                cpu_v3::compare_unsigned(lhs, rhs)
            });
        }
        CmpRhs::Imm(rhs) => emit_immediate(
            lines,
            if comparison.signed {
                ImmediateOp::CompareSigned
            } else {
                ImmediateOp::CompareUnsigned
            },
            lhs,
            rhs,
            comparison.signed,
        ),
    }
    condition
}

/// the degenerate Always/Never conditions compare a value with itself
fn emit_self_compare(
    function: &IrFunc,
    comparison: &Cmp,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) {
    let lhs = register(comparison.lhs);
    if function.class_of(comparison.lhs) == RegClass::Fpu {
        emit_fpu_scalar(lines, lhs, lhs, 0, FpuScalarSubop::Cmp, 0);
        return;
    }
    lines.word(cpu_v3::compare_signed(lhs, lhs));
}

fn emit_edge_moves(
    function: &IrFunc,
    predecessor: BlockId,
    target: BlockId,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) {
    let block = &function.blocks[target];
    if block.phis.is_empty() || block.preds.len() == 1 {
        return;
    }
    let moves = block
        .phis
        .iter()
        .map(|phi| {
            let value = phi
                .args
                .iter()
                .find(|(pred, _)| *pred == predecessor)
                .expect("missing phi edge")
                .1;
            (value, phi.dst)
        })
        .collect::<Vec<_>>();
    emit_phi_moves(function, &moves, register, lines);
}

/// Expands phi moves to per-lane register pairs and emits a cycle-safe
/// parallel move. A multi-lane FPU value contributes one move per lane, so a
/// cycle through any lane is broken with the reserved F63 scratch.
fn emit_phi_moves(
    function: &IrFunc,
    moves: &[(VReg, VReg)],
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) {
    let mut expanded: Vec<(u8, u8, RegClass)> = Vec::new();
    for &(from, to) in moves {
        let class = function.class_of(to);
        let lanes = function.lanes(to);
        assert_eq!(
            lanes,
            function.lanes(from),
            "phi input and destination widths differ"
        );
        if class == RegClass::Fpu && lanes > 1 {
            let from_base = register(from);
            let to_base = register(to);
            for lane in 0..lanes {
                expanded.push((from_base + lane, to_base + lane, class));
            }
        } else {
            expanded.push((register(from), register(to), class));
        }
    }
    emit_parallel_moves(lines, &expanded);
}

/// parallel phi moves, split by register class: GPR moves use MOV with
/// REG_TMP as the cycle-breaking scratch, FPU moves use FMOV with the
/// reserved FPU_SCRATCH register
fn emit_parallel_moves(lines: &mut Lines, moves: &[(u8, u8, RegClass)]) {
    for class in [RegClass::Gpr, RegClass::Fpu] {
        let class_moves = moves
            .iter()
            .filter(|&&(_, _, c)| c == class)
            .map(|&(from, to, _)| (from, to))
            .collect::<Vec<_>>();
        emit_parallel_moves_in_file(lines, &class_moves, class);
    }
}

fn emit_parallel_moves_in_file(lines: &mut Lines, moves: &[(u8, u8)], class: RegClass) {
    let emit_move = |lines: &mut Lines, to: u8, from: u8| match class {
        RegClass::Gpr => lines.word(cpu_v3::move_register(to, from)),
        RegClass::Fpu => emit_fpu_scalar(lines, from, 0, to, FpuScalarSubop::Mov, 0),
    };
    let scratch = match class {
        RegClass::Gpr => REG_TMP,
        RegClass::Fpu => FPU_SCRATCH,
    };
    let mut pending = moves
        .iter()
        .copied()
        .filter(|(from, to)| from != to)
        .collect::<Vec<_>>();
    while !pending.is_empty() {
        if let Some(index) = pending.iter().enumerate().find_map(|(index, &(_, to))| {
            (!pending
                .iter()
                .enumerate()
                .any(|(other, &(from, _))| other != index && from == to))
            .then_some(index)
        }) {
            let (from, to) = pending.remove(index);
            emit_move(lines, to, from);
        } else {
            let target = pending[0].1;
            emit_move(lines, scratch, target);
            for (from, _) in &mut pending {
                if *from == target {
                    *from = scratch;
                }
            }
        }
    }
}

fn emit_load_immediate(lines: &mut Lines, dst: u8, value: u16) {
    if value <= 15 {
        lines.word(cpu_v3::immediate_unsigned(
            ImmediateOp::LoadUnsigned,
            dst,
            value as u8,
        ));
    } else if value >= 0xfff8 {
        lines.word(cpu_v3::immediate_signed(
            ImmediateOp::LoadSigned,
            dst,
            value as i16,
        ));
    } else {
        for word in cpu_v3::load_immediate16(dst, value) {
            lines.word(word);
        }
    }
}

/// Emits a MULI (short u4, or PFX12 + full unsigned u16 bit pattern).
fn emit_muli(lines: &mut Lines, dst: u8, value: u16) {
    if value <= 15 {
        lines.word(cpu_v3::multiply_immediate(dst, value as u8));
    } else {
        for word in cpu_v3::prefixed(cpu_v3::multiply_immediate(dst, 0), value) {
            lines.word(word);
        }
    }
}

fn emit_immediate(
    lines: &mut Lines,
    operation: ImmediateOp,
    dst: u8,
    value: u16,
    signed_short: bool,
) {
    // ADDI/SUBI take an unsigned u4 unprefixed, so a negative adjustment is
    // expressed with the opposite operation of its magnitude (equivalent
    // under wrapping arithmetic, also for the prefixed 16-bit form).
    if matches!(operation, ImmediateOp::Add | ImmediateOp::Sub) {
        let (operation, value) = if (value as i16) < 0 {
            let flipped = match operation {
                ImmediateOp::Add => ImmediateOp::Sub,
                ImmediateOp::Sub => ImmediateOp::Add,
                _ => unreachable!(),
            };
            (flipped, (value as i16).unsigned_abs())
        } else {
            (operation, value)
        };
        if value <= 15 {
            lines.word(cpu_v3::immediate_unsigned(operation, dst, value as u8));
        } else {
            let consumer = 0xa000 | ((operation as u16) << 8) | (u16::from(dst) << 4);
            for word in cpu_v3::prefixed(consumer, value) {
                lines.word(word);
            }
        }
        return;
    }
    if signed_short && (-8..=7).contains(&(value as i16)) {
        lines.word(cpu_v3::immediate_signed(operation, dst, value as i16));
    } else if !signed_short && value <= 15 {
        lines.word(cpu_v3::immediate_unsigned(operation, dst, value as u8));
    } else {
        let consumer = 0xa000 | ((operation as u16) << 8) | (u16::from(dst) << 4);
        for word in cpu_v3::prefixed(consumer, value) {
            lines.word(word);
        }
    }
}

fn emit_load(lines: &mut Lines, dst: u8, base: u8, offset: i16) {
    if (0..=15).contains(&offset) {
        lines.word(cpu_v3::load(dst, base, offset));
    } else {
        for word in cpu_v3::prefixed(cpu_v3::load(dst, base, 0), offset as u16) {
            lines.word(word);
        }
    }
}

fn emit_store(lines: &mut Lines, src: u8, base: u8, offset: i16) {
    if (0..=15).contains(&offset) {
        lines.word(cpu_v3::store(src, base, offset));
    } else {
        for word in cpu_v3::prefixed(cpu_v3::store(src, base, 0), offset as u16) {
            lines.word(word);
        }
    }
}
