//! Two separately checked storage increments over texture_storage_probe.
//! Arithmetic remains closed-frame golden data; no native controller is replaced.
#[path = "texture_storage_probe/lane.rs"]
mod lane;
#[path = "texture_storage_probe.rs"]
#[allow(dead_code)]
mod previous;
use gpu_v2::texture::sim::staged::bound;
use previous::{baseline, raw};
use std::{collections::VecDeque, fs, io::Write, path::Path, sync::Arc};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map_or("target/gpu-v2-texture-storage-increment", String::as_str),
    );
    fs::create_dir_all(root).unwrap();
    if !args.iter().any(|a| a == "--packet-only") {
        let b = bound::Binding::build().unwrap();
        let d = previous::stream_d(root, &b);
        lane::probe(root, &d);
    }
    previous::packet::probe_increment(root);
}
