//! Standalone audited artifact consumer for the fitted Tang Nano 20K target.
fn main() {
    if let Err(error) = digital_design_hardware_gowin::run_gowin_artifact_cli::<
        digital_design_hardware_gowin::TangNano20K,
    >() {
        eprintln!("gowin-artifact: {error}");
        std::process::exit(1);
    }
}
