fn main() {
    let path = std::env::args().nth(1).expect("fixture path");
    gpu_v2::texture::emu::runtime_frontend::ordered::certify(std::path::Path::new(&path)).unwrap();
}
