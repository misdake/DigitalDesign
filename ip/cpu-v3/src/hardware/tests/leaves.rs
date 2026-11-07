use super::*;

#[test]
#[ignore = "explicit external simulation of the scalar register file"]
fn verify_gpr_ram_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3GprRam>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 register file"]
fn verify_fpu_register_ram_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuRegisterRam>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 instruction front-end"]
fn verify_fpu_frontend_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuFrontend>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 scalar ALU"]
fn verify_fpu_scalar_alu_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuScalarAlu>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 scalar path"]
fn verify_fpu_scalar_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuScalarPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 special-function path"]
fn verify_fpu_special_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuSpecialPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 unit top"]
fn verify_fpu_unit_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Fpu>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 vector path"]
fn verify_fpu_vector_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuVectorPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 multiply path"]
fn verify_fpu_multiply_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuMultiplyPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 dot path"]
fn verify_fpu_dot_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuDotPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the shared FPU v2 multiply pipe"]
fn verify_fpu_mul_pipe_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuMulPipe>().unwrap();
}
