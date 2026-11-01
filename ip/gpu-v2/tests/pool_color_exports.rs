//! Dependency-closed factory inputs for Pool/Color policy qualification.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::{
        self,
        pixel::{quad_ingress::rtl::Backend, vliw_backend, vliw_quad_pool},
    },
    texture::{ports::Slot, rtl::runtime_color},
};
#[path = "../src/system/simulation_checks.rs"]
mod simulation_checks;

#[test]
#[ignore = "writes dependency-closed RTL to POOL_COLOR_EXPORT_DIR"]
fn export_pool_color_policy_factories() {
    let out = std::path::PathBuf::from(
        std::env::var_os("POOL_COLOR_EXPORT_DIR").expect("POOL_COLOR_EXPORT_DIR"),
    );
    assert!(!out.exists(), "preserve previous exports");
    std::fs::create_dir_all(&out).unwrap();
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 400,
        height: 240,
    };
    let slots = [Slot {
        base_address: 0,
        max_size_log2: 0,
        has_full_mip: false,
        valid: true,
    }];
    for (name, backend) in [("native", Backend::Native), ("portable", Backend::Portable)] {
        let native = matches!(backend, Backend::Native);
        let checked = vliw_backend::verilog_selected(&slots, surface, backend).unwrap();
        // Exercise the selected direct component before the physical Group35
        // adapter removes its level field. The full command export below also
        // traverses that adapter, so these are distinct interface contracts.
        let raw_command=simulation_checks::lower(if checked.contains("`ifdef GPU_V2_VLIW_COLOR_COMMAND") {
            format!("`define GPU_V2_VLIW_COLOR_COMMAND\n{checked}\n`undef GPU_V2_VLIW_COLOR_COMMAND\n")
        } else { checked.clone() }).unwrap();
        for (kind, source) in [
            (
                "command",
                system::vliw_command::verilog(surface, backend).unwrap(),
            ),
            (
                "full",
                system::vliw::verilog(&slots, surface, backend).unwrap(),
            ),
            (
                "sdram",
                system::vliw_sdram::verilog(&slots, surface, backend).unwrap(),
            ),
            ("backend", checked),
            ("raw-command", raw_command),
            ("pool", vliw_quad_pool::verilog(!native)),
            ("public-color", runtime_color::source(native)),
            ("reserved-color", runtime_color::source_reserved(native)),
        ] {
            std::fs::write(out.join(format!("{kind}-{name}.v")), source).unwrap();
        }
    }
}
