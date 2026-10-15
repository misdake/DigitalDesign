use gpu_v2::command_processor::rtl;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(std::env::args().nth(1).ok_or("expected output directory")?);
    for (backend, source) in [("native", rtl::native()), ("portable", rtl::portable())] {
        let dir = output.join(backend);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("command-control.v"), source)?;
    }
    fs::write(
        output.join("reference-control.v"),
        rtl::reference_control_verilog(),
    )?;
    Ok(())
}
