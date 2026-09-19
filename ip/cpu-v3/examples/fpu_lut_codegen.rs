//! Regenerates the FPU register-file BSRAM initialization region from the
//! `cpu_v3::lut` reference model.
//!
//! The register-file Verilog is edited in place between the `LUT_INIT_BEGIN`
//! and `LUT_INIT_END` markers; everything outside the markers is preserved.
//! Running it repeatedly is idempotent. Pass `--check` to audit the checked-in
//! file without writing instead.

use std::path::PathBuf;

use cpu_v3::lut::{extract_lut_init_region, render_lut_init_region, replace_lut_init_region};

fn main() {
    let destination = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("hardware")
        .join("fpu")
        .join("cpu_v3_fpu_register_ram.v");
    let checked_in = std::fs::read_to_string(&destination).expect("read FPU register-file Verilog");

    if std::env::args()
        .skip(1)
        .any(|argument| argument == "--check")
    {
        assert_eq!(
            extract_lut_init_region(&checked_in),
            render_lut_init_region(),
            "checked-in FPU LUT init region is stale"
        );
        println!("FPU LUT init region is current: {}", destination.display());
    } else {
        let rendered = replace_lut_init_region(&checked_in);
        std::fs::write(&destination, rendered).expect("write FPU register-file Verilog");
        println!("wrote {}", destination.display());
    }
}
