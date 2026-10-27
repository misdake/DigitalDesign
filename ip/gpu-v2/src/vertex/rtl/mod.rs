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
pub fn reference_do_loan_portable() -> String {
    DO_LOAN_SOURCE.to_owned()
}
pub fn reference_do_loan_native() -> String {
    format!(
        "`define GPU_VERTEX_DO_LOAN_NATIVE\n{DO_LOAN_SOURCE}\n`undef GPU_VERTEX_DO_LOAN_NATIVE\n"
    )
}

const SINGLE_EDGE_SOURCE: &str = include_str!("single_edge.v");
pub fn single_edge_portable() -> String {
    SINGLE_EDGE_SOURCE.to_owned()
}
pub fn single_edge_native() -> String {
    format!("`define GPU_VERTEX_SINGLE_EDGE_NATIVE\n{SINGLE_EDGE_SOURCE}\n`undef GPU_VERTEX_SINGLE_EDGE_NATIVE\n")
}

pub const SELECTED_MODULE: &str = "gpu_v2_vertex_do_loan";
pub fn selected_portable() -> String {
    do_loan_portable()
}
pub fn selected_native() -> String {
    do_loan_native()
}

/// Keep the existing default leaf module name; only its qualified engine changes.
fn default_module(source: String) -> String {
    let old = "module gpu_v2_vertex_single_edge(";
    assert_eq!(source.matches(old).count(), 1);
    source.replacen(old, "module gpu_v2_vertex_do_loan(", 1)
}
pub fn do_loan_portable() -> String {
    default_module(single_edge_portable())
}
pub fn do_loan_native() -> String {
    default_module(single_edge_native())
}
