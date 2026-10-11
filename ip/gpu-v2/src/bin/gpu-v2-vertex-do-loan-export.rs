use std::{env, fs, path::PathBuf};
fn main() {
    let out = PathBuf::from(env::args().nth(1).expect("output directory"));
    fs::create_dir_all(&out).unwrap();
    fs::write(
        out.join("vertex-do-loan-portable.v"),
        gpu_v2::vertex::rtl::do_loan_portable(),
    )
    .unwrap();
    fs::write(
        out.join("vertex-do-loan-native.v"),
        gpu_v2::vertex::rtl::do_loan_native(),
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
}
