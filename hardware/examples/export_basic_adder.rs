use digital_design_hardware::examples::basic_adder::{
    basic_adder_gowin_project, BasicAdderTangNano20K,
};
use digital_design_hardware::GowinToolchain;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = PathBuf::from("target/basic_adder_gowin");
    let mut build = false;
    let mut program = false;
    for argument in std::env::args_os().skip(1) {
        match argument.to_str() {
            Some("--build") => build = true,
            Some("--program") => {
                build = true;
                program = true;
            }
            Some(value) if value.starts_with('-') => {
                return Err(format!("unknown option `{value}`").into());
            }
            _ => output = PathBuf::from(argument),
        }
    }
    let project = basic_adder_gowin_project().export::<BasicAdderTangNano20K>(&output)?;
    println!("Exported Gowin project to {}", output.display());
    if build {
        let toolchain = GowinToolchain::default();
        let result = toolchain.build(&output, &project)?;
        println!("Built {}", result.bitstream.display());
        for warning in &result.warnings {
            println!("Gowin warning: {warning}");
        }
        if program {
            println!("Programming volatile FPGA SRAM; the board must be connected.");
            toolchain.program_sram(&result, 4)?;
        }
    }
    Ok(())
}
