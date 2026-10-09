use digital_design_hardware_gowin::{
    run_gowin_project_cli, sdram_memory_controller::probe::SdramChainedGroupProbe, GowinCliError,
};
fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        SdramChainedGroupProbe::project(),
        "target/sdram_chained_group_probe_gowin",
    )
}
