//! Shared architectural/LUT register-file state and standalone RAM peer.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuRegisterRamInput {
    pub write_enable: Wire,
    pub write_address: Wires<9>,
    pub write_data: Wires<32>,
    pub read_a_address: Wires<9>,
    pub read_b_address: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuRegisterRamOutput {
    pub read_a_data: Wires<32>,
    pub read_b_data: Wires<32>,
}

/// FPU v2 register file: two mirrored 512x32 BSRAMs (SDPB) giving one
/// broadcast write port plus two independent synchronous read ports.
/// Physical addresses 0..63 alias architectural F0..F63 (the parent forces
/// the top three address bits of architectural accesses to zero); 64..511
/// are the hidden LUT region for RCP/RSQRT/SINCOS. Same-cycle write/read on
/// one address returns the old word on both read ports.
pub struct CpuV3FpuRegisterRam;

impl HardwareIdentity for CpuV3FpuRegisterRam {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuRegisterRam").namespace(["components", "cpu", "cpu_v3"])
    }
}

/// Builds the two register-file BSRAM mirrors from the frozen LUT tables:
/// mirror A carries RCP and SINCOS, mirror B carries the even- and odd-exponent
/// RSQRT tables. The architectural region 0..63 stays zero in both; writes
/// broadcast to both arrays at runtime.
pub(super) fn rf_mirrors_from_lut() -> (Box<[u32; 512]>, Box<[u32; 512]>) {
    let mut mirror_a = Box::new([0u32; 512]);
    let mut mirror_b = Box::new([0u32; 512]);
    let rcp = lut::packed_rcp_lut();
    let sincos = lut::packed_sincos_lut();
    let rsqrt_even = lut::packed_rsqrt_even_lut();
    let rsqrt_odd = lut::packed_rsqrt_odd_lut();
    mirror_a[lut::MIRROR_A_RCP_BASE..lut::MIRROR_A_RCP_BASE + rcp.len()].copy_from_slice(&rcp);
    mirror_a[lut::MIRROR_A_SINCOS_BASE..lut::MIRROR_A_SINCOS_BASE + sincos.len()]
        .copy_from_slice(&sincos);
    mirror_b[lut::MIRROR_B_RSQRT_EVEN_BASE..lut::MIRROR_B_RSQRT_EVEN_BASE + rsqrt_even.len()]
        .copy_from_slice(&rsqrt_even);
    mirror_b[lut::MIRROR_B_RSQRT_ODD_BASE..lut::MIRROR_B_RSQRT_ODD_BASE + rsqrt_odd.len()]
        .copy_from_slice(&rsqrt_odd);
    (mirror_a, mirror_b)
}

#[derive(Clone)]
pub struct CpuV3FpuRegisterRamState {
    /// Mirror A (read port A): architectural F0..F63 plus the RCP table at
    /// 128..255 and the SINCOS intervals at 256..511 (design section 9.4).
    pub(crate) mirror_a: Box<[u32; 512]>,
    /// Mirror B (read port B): architectural F0..F63 plus the RSQRT even table
    /// at 128..255 and the RSQRT odd table at 256..383.
    pub(super) mirror_b: Box<[u32; 512]>,
    pub(super) read_a_data: u32,
    pub(super) read_b_data: u32,
}

impl Default for CpuV3FpuRegisterRamState {
    fn default() -> Self {
        let (mirror_a, mirror_b) = rf_mirrors_from_lut();
        CpuV3FpuRegisterRamState {
            mirror_a,
            mirror_b,
            read_a_data: 0,
            read_b_data: 0,
        }
    }
}

impl Module for CpuV3FpuRegisterRam {
    type Input = CpuV3FpuRegisterRamInput;
    type Output = CpuV3FpuRegisterRamOutput;
    type EmuState = CpuV3FpuRegisterRamState;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(2))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Self::EmuState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &CpuV3FpuRegisterRamOutputValue {
                read_a_data: u64::from(state.read_a_data),
                read_b_data: u64::from(state.read_b_data),
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        // Both reads observe the pre-write contents (read-first semantics); the
        // write broadcasts the same word into both mirrors.
        state.read_a_data = state.mirror_a[input.read_a_address as usize];
        state.read_b_data = state.mirror_b[input.read_b_address as usize];
        if input.write_enable {
            state.mirror_a[input.write_address as usize] = input.write_data as u32;
            state.mirror_b[input.write_address as usize] = input.write_data as u32;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_register_ram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_register_ram_tb.v").to_string())
    }
}
