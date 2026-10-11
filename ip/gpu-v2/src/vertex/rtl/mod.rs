//! Opt-in standalone vertex leaf. Existing frontend factories are unchanged.
pub const MODULE: &str = "gpu_v2_vertex";
const SOURCE: &str = include_str!("vertex.v");
pub fn portable() -> String {
    SOURCE.to_owned()
}
pub fn native() -> String {
    format!("`define GPU_VERTEX_NATIVE\n{SOURCE}\n`undef GPU_VERTEX_NATIVE\n")
}

const DO_LOAN_SOURCE: &str = include_str!("do_loan.v");
pub fn do_loan_portable() -> String {
    DO_LOAN_SOURCE.to_owned()
}
pub fn do_loan_native() -> String {
    format!(
        "`define GPU_VERTEX_DO_LOAN_NATIVE\n{DO_LOAN_SOURCE}\n`undef GPU_VERTEX_DO_LOAN_NATIVE\n"
    )
}
