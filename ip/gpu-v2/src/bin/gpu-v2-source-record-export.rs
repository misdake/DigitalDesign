use gpu_v2::frontend::source_record::{native_rtl, portable_rtl};
fn main() {
    let dir = std::path::PathBuf::from(std::env::args_os().nth(1).expect("export directory"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("native.v"), native_rtl()).unwrap();
    std::fs::write(dir.join("portable.v"), portable_rtl()).unwrap();
}
