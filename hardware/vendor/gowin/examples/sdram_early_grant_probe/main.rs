use digital_design_hardware_gowin::{
    run_gowin_project_cli, sdram_memory_controller::probe::SdramEarlyGrantProbe, GowinCliError,
};
fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        SdramEarlyGrantProbe::project(),
        "target/sdram_early_grant_probe_gowin",
    )
}
