//! Board build/export CLI; ordinary dependencies contain no application producer.
#[path = "../../board/mod.rs"]
mod board;
fn main() {
    if let Err(error) = digital_design_hardware_gowin::run_gowin_project_cli(
        board::gowin_project(),
        "target/cpu-v3-system",
    ) {
        eprintln!("cpu-v3-system: {error}");
        std::process::exit(1);
    }
}
