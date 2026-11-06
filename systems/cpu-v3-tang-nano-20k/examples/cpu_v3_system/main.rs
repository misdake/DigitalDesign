//! Stable export/build/program CLI for the production board.
#[path = "../../board/mod.rs"]
mod board;

fn main() -> Result<(), digital_design_hardware_gowin::GowinCliError> {
    digital_design_hardware_gowin::run_gowin_project_cli(
        board::gowin_project(),
        "target/cpu_v3_system_gowin",
    )
}
