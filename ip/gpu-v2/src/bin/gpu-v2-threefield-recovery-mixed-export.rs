use gpu_v2::triangle::recovery_mixed::{native_rtl, portable_rtl, Params};
fn main() {
    let out = std::path::PathBuf::from(std::env::args_os().nth(1).expect("export directory"));
    std::fs::create_dir_all(&out).unwrap();
    let p = Params::default();
    std::fs::write(out.join("mixed-native.v"), native_rtl(p)).unwrap();
    std::fs::write(out.join("mixed-portable.v"), portable_rtl(p)).unwrap();
}
