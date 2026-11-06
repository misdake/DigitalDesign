//! Logical special/multiplier ports shared by selected and standalone leaves.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuSpecialPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
    pub mul_out_valid: Wire,
    pub mul_out_product: Wires<64>,
    pub mul_out_tag: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuSpecialPathOutput {
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub busy: Wire,
    pub r_wait: Wires<4>,
    pub w_wait: Wires<4>,
    pub x_wait: Wires<4>,
    pub mul_in_valid: Wire,
    pub mul_in_a: Wires<36>,
    pub mul_in_b: Wires<36>,
    pub mul_in_tag: Wires<9>,
}
