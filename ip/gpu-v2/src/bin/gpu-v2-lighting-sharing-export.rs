//! Export a genuinely scheduled candidate into the exact frozen complete-chain shell.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    lighting::{
        calendars::UnifiedCalendar, dsp_sharing, rtl, LightingProfile, LightingQuantization,
    },
    system::pixel::{postquad, quad_ingress},
    texture::ports::Slot,
};
use std::{fs, path::Path};
fn body(source: &str) -> &str {
    let start = source
        .find("module gpu_v2_lighting(")
        .expect("Lighting module");
    let end = start + source[start..].find("endmodule").expect("Lighting end") + 9;
    &source[start..end]
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        4,
        "variant output-directory frozen-complete-source"
    );
    let out = Path::new(&args[2]);
    fs::create_dir_all(out).unwrap();
    let q = LightingQuantization::CompensatedFloor;
    let (options, plans) = match args[1].as_str() {
        "selected" => (
            UnifiedCalendar::SmoothH9.options(q),
            UnifiedCalendar::SmoothH9.plans(q).unwrap(),
        ),
        "baseline" => (
            UnifiedCalendar::SmoothH9Reference.options(q),
            UnifiedCalendar::SmoothH9Reference.plans(q).unwrap(),
        ),
        "A" => dsp_sharing::shared_dots().unwrap(),
        "B" => dsp_sharing::shared_scalar_pairs(false, false).unwrap(),
        "Bsmall" => dsp_sharing::shared_scalar_pairs(true, false).unwrap(),
        "C" => dsp_sharing::shared_scalar_pairs(false, true).unwrap(),
        "D" => dsp_sharing::shared_scalar_pairs(true, true).unwrap(),
        _ => panic!("unknown variant"),
    };
    let core = rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &plans).unwrap();
    let slots = [Slot {
        base_address: 0x1000,
        max_size_log2: 6,
        has_full_mip: true,
        valid: true,
    }];
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 512,
        height: 256,
    };
    let native = postquad::verilog_combined_group48_pool64_candidate(
        &slots,
        surface,
        quad_ingress::rtl::Backend::Native,
    )
    .unwrap();
    let frozen = fs::read_to_string(&args[3]).unwrap();
    fs::write(out.join("capture-current-native.v"), &native).unwrap();
    // The root owns the frozen complete-chain fit shell. Candidate substitution
    // is restricted to the core emitted from the actual checked program.
    assert_eq!(
        body(
            &rtl::generate_with_schedule_plans(
                LightingProfile::Fast,
                UnifiedCalendar::SmoothH9Reference.options(q),
                &UnifiedCalendar::SmoothH9Reference.plans(q).unwrap(),
            )
            .unwrap()
            .source
        ),
        body(&frozen),
        "source-matched frozen Lighting reference"
    );
    let portable = postquad::verilog_combined_group48_pool64_candidate(
        &slots,
        surface,
        quad_ingress::rtl::Backend::Portable,
    )
    .unwrap();
    let baseline = rtl::generate_with_schedule_plans(
        LightingProfile::Fast,
        UnifiedCalendar::SmoothH9Reference.options(q),
        &UnifiedCalendar::SmoothH9Reference.plans(q).unwrap(),
    )
    .unwrap();
    assert_eq!(body(&frozen), body(&baseline.source));
    let selected = rtl::generate_with_schedule_plans(
        LightingProfile::Fast,
        UnifiedCalendar::SmoothH9.options(q),
        &UnifiedCalendar::SmoothH9.plans(q).unwrap(),
    )
    .unwrap();
    assert_eq!(body(&native), body(&selected.source));
    for (name, full) in [
        ("complete-native.v", frozen),
        ("complete-portable.v", portable),
    ] {
        fs::write(
            out.join(name),
            full.replacen(body(&full), body(&core.source), 1),
        )
        .unwrap();
    }
    fs::write(out.join("lighting.v"), &core.source).unwrap();
    fs::write(out.join("storage.csv"), &core.storage_csv).unwrap();
    fs::write(out.join("dsp-inputs.csv"), &core.dsp_input_csv).unwrap();
    let calendar =
        rtl::physical_calendar_with_schedule_plans(LightingProfile::Fast, options, &plans).unwrap();
    let mut csv = String::from("event,lane,kind,issue,ready\n");
    use std::fmt::Write;
    for i in calendar {
        writeln!(
            csv,
            "{},{},{},{},{}",
            i.event,
            i.lane.map_or(String::new(), |n| n.to_string()),
            i.kind.replace(',', ";"),
            i.issue,
            i.ready
        )
        .unwrap();
    }
    fs::write(out.join("physical-calendar.csv"), csv).unwrap();
    let mut plan = format!(
        "2\n{}\nid,issue,lane\n",
        plans[0]
            .capacities
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    );
    for slot in &plans[0].slots {
        writeln!(plan, "{},{},{}", slot.id, slot.issue, slot.lane).unwrap();
    }
    fs::write(out.join("calendar.plan"), plan).unwrap();
    fs::write(
        out.join("graph.csv"),
        dsp_sharing::graph_receipt(options, &plans).unwrap(),
    )
    .unwrap();
    fs::write(out.join("inventory.json"),format!("{{\"latency\":{},\"ii\":{},\"ordinary18\":{},\"small9_including_h9\":{},\"paired_mac\":{},\"behavioral_register_bits\":{}}}\n",core.latency,core.specular_ii,core.large_multipliers,core.small_multipliers,core.pair_macros,core.register_bits)).unwrap();
    println!(
        "{} latency={} II={} DSP18={} DSP9={} MAC={}",
        args[1],
        core.latency,
        core.specular_ii,
        core.large_multipliers,
        core.small_multipliers,
        core.pair_macros
    );
}
