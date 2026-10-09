use gpu_v2::lighting::{rtl, LightingProfile};
fn main() {
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/gpu-v2-lighting/rtl".into());
    let profile = match std::env::args().nth(3).as_deref() {
        Some("compact") => LightingProfile::Compact,
        Some("system-fast") => LightingProfile::SystemFast,
        Some("system-compact") => LightingProfile::SystemCompact,
        None | Some("fast") => LightingProfile::Fast,
        Some(s) => panic!("unknown profile {s}"),
    };
    let options = match std::env::args().nth(4).as_deref() {
        None => rtl::LightingRtlOptions::default(),
        Some("factor-window-ff") => rtl::LightingRtlOptions {
            shallow_normal_ff: true,
            q_windows: true,
            ..rtl::LightingRtlOptions::factor_profile()
        },
        Some("factor-window") => rtl::LightingRtlOptions {
            q_windows: true,
            ..rtl::LightingRtlOptions::factor_profile()
        },
        Some("factor-cut") => rtl::LightingRtlOptions {
            cost_cut: true,
            ..rtl::LightingRtlOptions::factor_profile()
        },
        Some("factor-coarse") => rtl::LightingRtlOptions::factor_profile(),
        Some("factor-hierarchy") => rtl::LightingRtlOptions {
            hierarchy: true,
            ..rtl::LightingRtlOptions::factor_profile()
        },
        Some("resource-coarse") => rtl::LightingRtlOptions {
            logic_depth: 8,
            ..rtl::LightingRtlOptions::resource_profile(profile)
        },
        Some("resource") => rtl::LightingRtlOptions::resource_profile(profile),
        Some("system") => rtl::LightingRtlOptions::system_profile(),
        Some("stationary") => rtl::LightingRtlOptions {
            stationary_logic: true,
            exact_normal_gate: false,
            ..rtl::LightingRtlOptions::system_profile()
        },
        Some("coarse") => rtl::LightingRtlOptions {
            logic_depth: 8,
            exact_normal_gate: false,
            ..rtl::LightingRtlOptions::system_profile()
        },
        Some("system-scaled-gate") | Some("system-square18") => rtl::LightingRtlOptions {
            exact_normal_gate: false,
            ..rtl::LightingRtlOptions::system_profile()
        },
        Some("system-mixed9") => rtl::LightingRtlOptions {
            square9: true,
            exact_normal_gate: false,
            ..rtl::LightingRtlOptions::system_profile()
        },
        Some("system-terminal") => rtl::LightingRtlOptions {
            split_cones: false,
            exact_normal_gate: false,
            ..rtl::LightingRtlOptions::system_profile()
        },
        Some("system-hierarchy") => rtl::LightingRtlOptions {
            hierarchy: true,
            ..rtl::LightingRtlOptions::system_profile()
        },
        Some("system-reference") => rtl::LightingRtlOptions {
            scalar_norm: true,
            block_prescale: true,
            role_schedule: true,
            shared_ids: true,
            id_ring: true,
            ..Default::default()
        },
        Some("resource-hierarchy") => rtl::LightingRtlOptions {
            hierarchy: true,
            ..rtl::LightingRtlOptions::resource_profile(profile)
        },
        Some("hierarchy") => rtl::LightingRtlOptions {
            hierarchy: true,
            ..Default::default()
        },
        Some("shift-share") => rtl::LightingRtlOptions {
            share_scalars: true,
            range_shifts: false,
            ..Default::default()
        },
        Some("shift-bound") => rtl::LightingRtlOptions {
            share_scalars: false,
            range_shifts: true,
            ..Default::default()
        },
        Some("shift-both") => rtl::LightingRtlOptions {
            share_scalars: true,
            range_shifts: true,
            ..Default::default()
        },
        Some("shift-both-hierarchy") => rtl::LightingRtlOptions {
            share_scalars: true,
            range_shifts: true,
            hierarchy: true,
            ..Default::default()
        },
        Some("unmask") => rtl::LightingRtlOptions {
            hierarchy: false,
            free_slots: true,
            split_cones: false,
            ..Default::default()
        },
        Some("unmask-hierarchy") => rtl::LightingRtlOptions {
            hierarchy: true,
            free_slots: true,
            split_cones: false,
            ..Default::default()
        },
        Some("unmask-split") => rtl::LightingRtlOptions {
            free_slots: true,
            split_cones: true,
            ..Default::default()
        },
        Some("dedicated") => rtl::LightingRtlOptions {
            free_slots: true,
            split_cones: false,
            dedicated_dsp: true,
            ..Default::default()
        },
        Some("dedicated-split") => rtl::LightingRtlOptions {
            free_slots: true,
            split_cones: true,
            dedicated_dsp: true,
            ..Default::default()
        },
        Some("baseline") => rtl::LightingRtlOptions {
            free_slots: false,
            split_cones: false,
            share_scalars: false,
            range_shifts: false,
            ..Default::default()
        },
        Some(s) => panic!("unknown lowering {s}"),
    };
    let result = rtl::generate_with_options(profile, options).unwrap();
    for full in [true, false]
        .into_iter()
        .filter(|_| !options.stationary_logic)
    {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            format!(
                "{directory}/calendar-{}.csv",
                if full { "full" } else { "diffuse" }
            ),
            rtl::calendar_study_with_options(profile, full, options).unwrap(),
        )
        .unwrap();
    }

    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        format!("{directory}/operations.csv"),
        rtl::operation_calendar_with_options(profile, options).unwrap(),
    )
    .unwrap();
    let mut lanes = String::from("lane,kind,width,latency,full_operations,diffuse_operations\n");
    for l in &result.lanes {
        use std::fmt::Write;
        writeln!(
            lanes,
            "{},\"{}\",{},{},{},{}",
            l.lane,
            l.kind.replace('"', "\"\""),
            l.width,
            l.latency,
            l.full_operations,
            l.diffuse_operations
        )
        .unwrap();
    }
    let gowin = std::env::args().nth(2).as_deref() == Some("gowin");
    std::fs::create_dir_all(&directory).unwrap();
    // Independent conservative coexistence certificate, not inferred from the
    // fitter's nominal DSP percentage and not a claim about exact placed sites.
    use audited::physical::{DspInventory, DspMode};
    let latency = |kind: &str| {
        result
            .lanes
            .iter()
            .find(|l| l.kind == kind)
            .unwrap()
            .latency as u64
    };
    let required_macros = result.small_multipliers.div_ceil(4)
        + result.large_multipliers.div_ceil(2)
        + result.pair_macros;
    let packing = DspInventory::pack(
        required_macros.div_ceil(2),
        &[
            (
                DspMode::Multiply9,
                result.small_multipliers,
                latency("SmallMultiply"),
                1,
            ),
            (
                DspMode::Multiply18,
                result.large_multipliers,
                latency("LargeMultiply"),
                1,
            ),
            (
                DspMode::PairMultiplyAdd,
                result.pair_macros,
                latency("PairMultiplyAdd"),
                1,
            ),
        ],
    )
    .unwrap();
    let usage = packing.audit().unwrap();
    let mut certificate = String::from("name,mode,tile,macro,slot,latency,II\n");
    use std::fmt::Write;
    for instance in packing.instances {
        writeln!(
            certificate,
            "{},{:?},{},{},{},{},{}",
            instance.name,
            instance.mode,
            instance.tile,
            instance.macro_index,
            instance.slot,
            instance.latency,
            instance.initiation_interval
        )
        .unwrap();
    }
    std::fs::write(format!("{directory}/dsp-packing.csv"), certificate).unwrap();
    std::fs::write(format!("{directory}/dsp-packing.txt"),format!("certified_macros={},certified_tiles={},DSP9_half_slots={},whole_macro_DSP18_charge={},whole_tile_DSP18_charge={}\n",usage.macros,usage.tiles,usage.multiplier_half_slots,2*usage.macros,4*usage.tiles)).unwrap();
    std::fs::write(format!("{directory}/lanes.csv"), lanes).unwrap();
    std::fs::write(format!("{directory}/storage.csv"), &result.storage_csv).unwrap();
    std::fs::write(format!("{directory}/cuts.csv"), &result.cuts_csv).unwrap();
    let source = if gowin {
        format!("`define GPU_V2_GOWIN_DSP\n{}", result.source)
    } else {
        result.source
    };
    std::fs::write(format!("{directory}/lighting.v"), source).unwrap();
    if gowin {
        std::fs::write(
            format!("{directory}/probe.v"),
            include_str!("../src/lighting/rtl/probe.v"),
        )
        .unwrap();
        std::fs::write(
            format!("{directory}/build.tcl"),
            include_str!("../src/lighting/rtl/probe.tcl"),
        )
        .unwrap();
        std::fs::write(
            format!("{directory}/lighting.sdc"),
            "create_clock -name clk -period 18.518 [get_ports {clk}]\n",
        )
        .unwrap();
    }
    let report=format!("specular_latency={},diffuse_latency={},specular_ii={},diffuse_ii={},register_bits={},small_multiply={},large_multiply={},pair_macros={},normalization_roms={},id_slots={}\n",
        result.latency,result.diffuse_latency,result.specular_ii,result.diffuse_ii,result.register_bits,result.small_multipliers,result.large_multipliers,result.pair_macros,result.normalization_roms,result.id_slots);
    std::fs::write(format!("{directory}/structure.txt"), &report).unwrap();
    print!("{report}");
}
