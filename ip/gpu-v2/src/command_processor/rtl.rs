//! Same synthesizable controller on native/portable targets; no arithmetic DSP.
pub fn verilog() -> &'static str {
    include_str!("control_kernel.v")
}
