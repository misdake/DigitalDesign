use std::{env, fs, path::PathBuf};
fn main() {
    let out = PathBuf::from(env::args().nth(1).expect("output directory"));
    fs::create_dir_all(&out).unwrap();
    fs::write(
        out.join("vertex-single-edge-portable.v"),
        gpu_v2::vertex::rtl::single_edge_portable(),
    )
    .unwrap();
    fs::write(
        out.join("vertex-single-edge-native.v"),
        gpu_v2::vertex::rtl::single_edge_native(),
    )
    .unwrap();
    fs::write(
        out.join("old-position32-portable.v"),
        gpu_v2::vertex::rtl::portable(),
    )
    .unwrap();
    fs::write(
        out.join("old-position32-native.v"),
        gpu_v2::vertex::rtl::native(),
    )
    .unwrap();
    fs::write(
        out.join("old-do-loan-portable.v"),
        gpu_v2::vertex::rtl::do_loan_portable(),
    )
    .unwrap();
    fs::write(
        out.join("old-do-loan-native.v"),
        gpu_v2::vertex::rtl::do_loan_native(),
    )
    .unwrap();
}
