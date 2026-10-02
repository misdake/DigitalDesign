use digital_design_hardware_gowin::{
    run_gowin_project_cli, sdram_memory_controller::probe::SdramTrafficProbe, GowinCliError,
};
fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        SdramTrafficProbe::project(),
        "target/sdram_traffic_probe_gowin",
    )
}
