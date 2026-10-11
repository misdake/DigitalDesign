//! Export the exact bounded post-quad RTL for matched simulation and fitting.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::pixel::{postquad, quad_ingress},
    texture::ports::Slot,
};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() >= 3,
        "output-path reference|folded|threshold [closed-depth] [fused-join]"
    );
    let final_stage = match args[2].as_str() {
        "reference" => postquad::FinalProfile::Reference,
        "folded" => postquad::FinalProfile::Folded,
        "threshold" => postquad::FinalProfile::Threshold,
        "shared" => postquad::FinalProfile::Shared,
        "single" => postquad::FinalProfile::SingleCaptured,
        "borrowed" => postquad::FinalProfile::SingleBorrowed,
        "combined-group48-pool64"
        | "combined-closed-basic-carry"
        | "combined-closed-basic"
        | "combined-stream-onehot"
        | "combined-stream-context"
        | "combined-unique-group34"
        | "combined-unique-weights"
        | "combined-weight-order"
        | "combined-maintenance-planes"
        | "combined-prehead-exact"
        | "combined-prehead"
        | "combined"
        | "forwarded"
        | "selected"
        | "cache-pipeline"
        | "sh9"
        | "cadence"
        | "cadence-h9"
        | "cadence-h9-control"
        | "cadence-h9-local-fault" => postquad::FinalProfile::Forwarded,
        _ => panic!("unknown Final profile"),
    };
    let profile = postquad::CompositionProfile {
        final_stage,
        closed_depth: args.iter().any(|x| x == "closed-depth"),
        fused_join: args.iter().any(|x| x == "fused-join"),
        completion_frontiers: args.iter().any(|x| x == "frontiers"),
        independent_sink: args.iter().any(|x| x == "independent-sink"),
        raw_proposals: args.iter().any(|x| x == "raw-proposals"),
        closed_bank: args.iter().any(|x| x == "closed-bank"),
        reuse_temporaries: args.iter().any(|x| x == "reuse-temporaries"),
        closed_sink_ingress: args.iter().any(|x| x == "closed-sink-ingress"),
        block_output: args.iter().any(|x| x == "block-output"),
        closed_color: args.iter().any(|x| x == "closed-color"),
        narrow_quantization: args.iter().any(|x| x == "narrow-quantization"),
        hard_descriptor: args.iter().any(|x| x == "hard-descriptor"),
        descriptor_control: args.iter().any(|x| x == "descriptor-control"),
        stream_output: args.iter().any(|x| x == "stream-output"),
        cache_pipeline: false,
        join_overlap: false,
        rop_overlap: false,
        final_reissue: false,
        final_two_cycle: false,
        join_pipeline: false,
        rop_prefix: false,
        final_fraction_bits: None,
        color_precision: postquad::ColorPrecision::Reference,
        rop_precision: postquad::RopPrecision::Reference,
        color_contract: false,
        color_normalize_511: false,
        framebuffer_write_address: false,
        framebuffer_write_data: false,
        framebuffer_held_draw: false,
    };
    let mut profile = if args[2] == "sh9"
        || args[2] == "cadence"
        || (args[2] == "cadence-h9"
            || (args[2] == "cadence-h9-control" || args[2] == "cadence-h9-local-fault"))
    {
        postquad::CompositionProfile::sh9_candidate()
    } else if args[2] == "selected" || args[2] == "cache-pipeline" {
        postquad::CompositionProfile::selected()
    } else {
        profile
    };
    profile.cache_pipeline |= args.iter().any(|x| x == "cache-pipeline");
    profile.join_overlap |= args.iter().any(|x| x == "join-overlap");
    profile.rop_overlap |= args.iter().any(|x| x == "rop-overlap");
    profile.final_reissue |= args.iter().any(|x| x == "final-reissue");
    profile.final_two_cycle |= args.iter().any(|x| x == "final-two-cycle");
    profile.join_pipeline |= args.iter().any(|x| x == "join-pipeline");
    profile.rop_prefix |= args.iter().any(|x| x == "rop-prefix");
    if let Some(f) = (0..=2).find(|f| args.iter().any(|x| x == &format!("final-q{f}"))) {
        profile.final_fraction_bits = Some(f);
    }
    profile.color_precision = if args.iter().any(|x| x == "color-q9-pairs") {
        postquad::ColorPrecision::NearestPairs9
    } else if args.iter().any(|x| x == "color-q9-rne") {
        postquad::ColorPrecision::Nearest9
    } else if args.iter().any(|x| x == "color-q9") {
        postquad::ColorPrecision::Floor9
    } else {
        profile.color_precision
    };
    profile.rop_precision = if args.iter().any(|x| x == "rop-alpha6") {
        postquad::RopPrecision::Alpha6
    } else if args.iter().any(|x| x == "rop-opaque") {
        postquad::RopPrecision::OpaqueReplace
    } else {
        profile.rop_precision
    };
    profile.color_contract |= args.iter().any(|x| x == "color-contract");
    profile.color_normalize_511 |= args.iter().any(|x| x == "color-normalize-511");
    profile.framebuffer_write_address |= args.iter().any(|x| x == "framebuffer-write-address");
    profile.framebuffer_write_data |= args.iter().any(|x| x == "framebuffer-write-data");
    profile.framebuffer_held_draw |= args.iter().any(|x| x == "framebuffer-held-draw");
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 512,
        height: 256,
    };
    let slots = [Slot {
        base_address: 0x1000,
        max_size_log2: 6,
        has_full_mip: true,
        valid: true,
    }];
    let native = !args.iter().any(|x| x == "portable");
    let backend = if native {
        quad_ingress::rtl::Backend::Native
    } else {
        quad_ingress::rtl::Backend::Portable
    };
    let rtl = if args[2] == "combined-group48-pool64" {
        postquad::verilog_combined_group48_pool64_candidate(&slots, surface, backend)
            .expect("Group48")
    } else if args[2] == "combined-closed-basic-carry" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_closed_basic_carry_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-closed-basic" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_closed_basic_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-stream-onehot" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_stream_onehot_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-stream-context" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_stream_context_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-unique-group34" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_unique_group34_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-unique-weights" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_unique_weights_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-weight-order" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_weight_order_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-maintenance-planes" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_maintenance_planes_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-prehead-exact" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_prehead_exact_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined-prehead" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_prehead_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "combined" {
        assert!(args.iter().skip(3).all(|arg| arg == "portable"));
        postquad::verilog_combined_candidate(&slots, surface, backend).unwrap()
    } else if args[2] == "cadence"
        || (args[2] == "cadence-h9"
            || (args[2] == "cadence-h9-control" || args[2] == "cadence-h9-local-fault"))
    {
        assert!(
            args.iter().skip(3).all(|arg| arg == "portable"),
            "cadence is a fixed qualified configuration; only portable may be added"
        );
        if args[2] == "cadence-h9"
            || (args[2] == "cadence-h9-control" || args[2] == "cadence-h9-local-fault")
        {
            if args[2] == "cadence-h9-control" || args[2] == "cadence-h9-local-fault" {
                if args[2] == "cadence-h9-local-fault" {
                    postquad::verilog_cadence_h9_local_fault_candidate(&slots, surface, backend)
                        .unwrap()
                } else {
                    postquad::verilog_cadence_h9_control_candidate(&slots, surface, backend)
                        .unwrap()
                }
            } else {
                postquad::verilog_cadence_h9_candidate(&slots, surface, backend).unwrap()
            }
        } else {
            postquad::verilog_cadence_candidate(&slots, surface, backend).unwrap()
        }
    } else {
        postquad::verilog_profile(&slots, surface, backend, profile).unwrap()
    };
    let source = (if native {
        "`define GPU_V2_GOWIN_DSP\n"
    } else {
        ""
    })
    .to_owned()
        + &rtl;
    let path = std::path::Path::new(&args[1]);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, source).unwrap();
    if args[2] == "cadence"
        || (args[2] == "cadence-h9"
            || (args[2] == "cadence-h9-control" || args[2] == "cadence-h9-local-fault"))
    {
        println!(
            "Exported fixed Final8/ROP two-phase cadence to {}",
            path.display()
        );
    } else {
        println!("Exported {:?} to {}", profile, path.display());
    }
}
