//! Default independent command component owns 90 payload bits and 25 control bits.
pub const MODULE: &str = "gpu_v2_command_processor";
pub fn control_verilog() -> &'static str {
    include_str!("control_kernel.v")
}
pub fn reference_control_verilog() -> &'static str {
    include_str!("control_reference.v")
}
pub fn verilog() -> String {
    format!("{}\n{}", control_verilog(), include_str!("payload_owner.v"))
}
pub fn native() -> String {
    verilog()
}
pub fn portable() -> String {
    verilog()
}
