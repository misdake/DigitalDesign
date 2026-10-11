use std::{env, fs, path::PathBuf};
fn main() {
    let output = PathBuf::from(env::args().nth(1).expect("output directory"));
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("vertex-portable.v"),
        gpu_v2::vertex::rtl::portable(),
    )
    .unwrap();
    fs::write(
        output.join("vertex-native.v"),
        gpu_v2::vertex::rtl::native(),
    )
    .unwrap();
    println!("{}", output.display());
}
