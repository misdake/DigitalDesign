mod design;

use design::basic_adder_gowin_project;
use digital_design_hardware::GowinToolchain;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = PathBuf::from("target/basic_adder_gowin");
    let mut build = false;
    let mut program = false;
    let mut gowin_home = None;
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--build") => build = true,
            Some("--program") => {
                build = true;
                program = true;
            }
            Some("--gowin-home") => {
                gowin_home = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or("`--gowin-home` requires a directory")?,
                ));
            }
            Some(value) if value.starts_with("--gowin-home=") => {
                gowin_home = Some(PathBuf::from(&value["--gowin-home=".len()..]));
            }
            Some(value) if value.starts_with('-') => {
                return Err(format!("unknown option `{value}`").into());
            }
            _ => output = PathBuf::from(argument),
        }
    }
    let project = basic_adder_gowin_project().export(&output)?;
    println!("Exported Gowin project to {}", output.display());
    if build {
        let toolchain = gowin_home
            .map(GowinToolchain::from_home)
            .map(Ok)
            .unwrap_or_else(GowinToolchain::discover)?;
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
