//! Stable export/build/program CLI for the production board.
// Qualification registration belongs to this entry, not every CLI using the board.
mod board {
    include!("../../board/mod.rs");
    #[cfg(test)]
    mod tests {
        include!("../../board/tests.rs");
    }
}

fn main() -> Result<(), digital_design_hardware_gowin::GowinCliError> {
    digital_design_hardware_gowin::run_gowin_project_cli(
        board::gowin_project(),
        "target/cpu_v3_system_gowin",
    )
}
