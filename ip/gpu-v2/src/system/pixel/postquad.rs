//! Actual post-quad RTL through ordered Final, ROP and framebuffer cache.
//! Draw pins are immutable until pipeline_idle; memory is one wall-clock owner.
use super::{
    final_stage, framebuffer_native, framebuffer_sink, pixel_sampling_ingress, quad_ingress,
    row_store,
};
use crate::{framebuffer::ports::MaterializedSurface, texture::ports::Slot};
pub const MODULE: &str = "gpu_v2_postquad";
#[path = "postquad_cadence.rs"]
mod cadence;
#[path = "postquad_color_contract.rs"]
mod color_contract;
#[path = "postquad_color_precision.rs"]
mod color_precision;
#[path = "postquad_filter_precision.rs"]
mod filter_precision;
#[path = "postquad_framebuffer_address.rs"]
mod framebuffer_address;
#[path = "postquad_framebuffer_contract.rs"]
mod framebuffer_contract;
#[path = "postquad_framebuffer_data.rs"]
mod framebuffer_data;
#[path = "postquad_h9_control.rs"]
mod h9_control;
#[path = "postquad_proposal.rs"]
mod proposal;
#[path = "postquad_quad_authority.rs"]
pub mod quad_authority;
#[path = "postquad_rop_precision.rs"]
mod rop_precision;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorPrecision {
    Reference,
    Floor9,
    Nearest9,
    NearestPairs9,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RopPrecision {
    Reference,
    OpaqueReplace,
    Alpha6,
}
#[derive(Clone, Copy, Debug)]
pub enum FinalProfile {
    Reference,
    Folded,
    Threshold,
    Shared,
    SingleCaptured,
    SingleBorrowed,
    Forwarded,
}
#[derive(Clone, Copy, Debug)]
pub struct CompositionProfile {
    pub final_stage: FinalProfile,
    pub closed_depth: bool,
    pub fused_join: bool,
    pub completion_frontiers: bool,
    pub independent_sink: bool,
    pub raw_proposals: bool,
    pub closed_bank: bool,
    pub reuse_temporaries: bool,
    pub closed_sink_ingress: bool,
    pub block_output: bool,
    pub closed_color: bool,
    pub narrow_quantization: bool,
    pub hard_descriptor: bool,
    pub descriptor_control: bool,
    pub stream_output: bool,
    pub cache_pipeline: bool,
    pub join_overlap: bool,
    pub rop_overlap: bool,
    pub final_reissue: bool,
    pub final_two_cycle: bool,
    pub join_pipeline: bool,
    pub rop_prefix: bool,
    pub final_fraction_bits: Option<u8>,
    pub color_precision: ColorPrecision,
    pub rop_precision: RopPrecision,
    pub color_contract: bool,
    pub color_normalize_511: bool,
    pub framebuffer_write_address: bool,
    pub framebuffer_write_data: bool,
    pub framebuffer_held_draw: bool,
}
impl CompositionProfile {
    /// Explicit SH9/Color9 candidate, separate from the committed selected top.
    /// Keeps strict pair RNE, conserved /511 projection and alpha6 endpoints.
    /// Complete native timing is qualified with its captured placement receipt.
    pub const fn sh9_candidate() -> Self {
        let mut profile = Self::selected();
        profile.final_two_cycle = true;
        profile.join_pipeline = true;
        profile.rop_prefix = true;
        profile.final_fraction_bits = Some(1);
        profile.color_precision = ColorPrecision::NearestPairs9;
        profile.rop_precision = RopPrecision::Alpha6;
        profile.color_contract = true;
        profile.color_normalize_511 = true;
        profile.framebuffer_write_data = true;
        profile.framebuffer_held_draw = true;
        profile
    }
    /// Closed single-draw composition selected by complete RTL and matched fits.
    /// Experimental lowerings remain available for reproducible comparisons.
    pub const fn selected() -> Self {
        Self {
            final_stage: FinalProfile::Forwarded,
            closed_depth: false,
            fused_join: false,
            completion_frontiers: true,
            independent_sink: true,
            raw_proposals: true,
            closed_bank: true,
            reuse_temporaries: false,
            closed_sink_ingress: false,
            block_output: false,
            closed_color: false,
            narrow_quantization: true,
            hard_descriptor: false,
            descriptor_control: false,
            stream_output: true,
            cache_pipeline: true,
            join_overlap: true,
            rop_overlap: true,
            final_reissue: false,
            final_two_cycle: false,
            join_pipeline: false,
            rop_prefix: false,
            final_fraction_bits: None,
            color_precision: ColorPrecision::Reference,
            rop_precision: RopPrecision::Reference,
            color_contract: false,
            color_normalize_511: false,
            framebuffer_write_address: false,
            framebuffer_write_data: false,
            framebuffer_held_draw: false,
        }
    }
}
/// Emit the selected II4 complete chain; interpolator and memory are external.
pub fn verilog_selected(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    super::postquad_ii4::verilog(slots, surface, backend, 9)
}
pub fn verilog(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    profile: FinalProfile,
) -> Result<String, String> {
    verilog_profile(
        slots,
        surface,
        backend,
        CompositionProfile {
            final_stage: profile,
            closed_depth: false,
            fused_join: false,
            completion_frontiers: false,
            independent_sink: false,
            raw_proposals: false,
            closed_bank: false,
            reuse_temporaries: false,
            closed_sink_ingress: false,
            block_output: false,
            closed_color: false,
            narrow_quantization: false,
            hard_descriptor: false,
            descriptor_control: false,
            stream_output: false,
            cache_pipeline: false,
            join_overlap: false,
            rop_overlap: false,
            final_reissue: false,
            final_two_cycle: false,
            join_pipeline: false,
            rop_prefix: false,
            final_fraction_bits: None,
            color_precision: ColorPrecision::Reference,
            rop_precision: RopPrecision::Reference,
            color_contract: false,
            color_normalize_511: false,
            framebuffer_write_address: false,
            framebuffer_write_data: false,
            framebuffer_held_draw: false,
        },
    )
}
/// Closed immutable-draw specialization with one Final/ROP two-phase calendar.
/// Final specular storage is eight integer bits; Sampling retains Q8.1 and /511.
/// Three MULT9X9 lanes alternate Final specular and ROP signed-delta products.
/// Each multiplication retains one enabled-edge latency; the retained Final
/// result buffer breaks cache/depth dependency cycles under backpressure.
pub fn verilog_cadence_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_cadence_profile(
        slots,
        surface,
        backend,
        crate::texture::rtl::runtime_rows::Profile::default(),
    )
}

fn verilog_cadence_profile(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    preparation: crate::texture::rtl::runtime_rows::Profile,
) -> Result<String, String> {
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    let source = verilog_with_preparation(
        slots,
        surface,
        backend,
        CompositionProfile::sh9_candidate(),
        preparation,
    )?;
    cadence::lower(source, native)
}

pub fn verilog_profile(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    profile: CompositionProfile,
) -> Result<String, String> {
    verilog_with_preparation(
        slots,
        surface,
        backend,
        profile,
        crate::texture::rtl::runtime_rows::Profile::default(),
    )
}

fn verilog_with_preparation(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    mut profile: CompositionProfile,
    preparation: crate::texture::rtl::runtime_rows::Profile,
) -> Result<String, String> {
    if profile.final_fraction_bits.is_some_and(|f| f > 2)
        || (profile.final_fraction_bits.is_some() && !profile.final_two_cycle)
    {
        return Err(
            "Narrow Final requires the two-cycle profile and zero to two fractional bits".into(),
        );
    }
    if profile.final_reissue && profile.final_two_cycle {
        return Err("Final reissue and two-cycle profiles are exclusive".into());
    }
    if profile.rop_prefix && (!profile.final_two_cycle || !profile.independent_sink) {
        return Err("Prefix ROP requires two-cycle Final and independent raw proposals".into());
    }
    if profile.rop_precision != RopPrecision::Reference
        && (!profile.rop_prefix || !profile.narrow_quantization)
    {
        return Err("ROP precision requires the closed prefix and RGB565 quantizer".into());
    }
    if profile.color_contract && profile.color_precision == ColorPrecision::Reference {
        return Err("Closed Color contract requires quantized9-bit tap arithmetic".into());
    }
    if profile.color_normalize_511 && profile.color_precision == ColorPrecision::Reference {
        return Err("Color /511 projection requires quantized9-bit tap arithmetic".into());
    }
    if profile.framebuffer_write_address && !profile.rop_prefix {
        return Err("Framebuffer write-address lowering requires the closed prefix".into());
    }
    if profile.framebuffer_write_data && !profile.rop_prefix {
        return Err("Framebuffer write-data lowering requires the closed prefix".into());
    }
    if profile.framebuffer_held_draw && (!profile.rop_prefix || !profile.join_pipeline) {
        return Err(
            "Framebuffer producer contract requires the held-header join and prefix".into(),
        );
    }
    if profile.join_pipeline {
        if !profile.final_two_cycle
            || !profile.completion_frontiers
            || !profile.raw_proposals
            || !profile.independent_sink
            || profile.hard_descriptor
            || profile.descriptor_control
        {
            return Err("Join pipeline requires two-cycle Final, raw proposals, completed frontiers, independent sink and canonical async descriptor".into());
        }
        profile.join_overlap = true;
    }
    if (profile.final_reissue || profile.final_two_cycle) && !profile.join_overlap {
        return Err("Final reissue requires paid join overlap".into());
    }
    if profile.join_overlap {
        if !matches!(profile.final_stage, FinalProfile::Forwarded) {
            return Err("Join overlap requires forwarded Final".into());
        }
        profile.fused_join = true;
    }
    if profile.closed_sink_ingress && (surface.width != 512 || surface.height != 256) {
        return Err("Closed post-quad sink requires the bounded 512x256 origin domain".into());
    }
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    let mut sink = if profile.rop_prefix {
        if !profile.stream_output || !profile.raw_proposals {
            return Err("Prefix ROP requires ordered stream output and raw proposals".into());
        }
        framebuffer_sink::verilog_closed_bank_prefix(surface)?
    } else if profile.rop_overlap {
        if !profile.stream_output {
            return Err("ROP overlap requires immutable stream Output2".into());
        }
        framebuffer_sink::verilog_closed_bank_overlap(surface)?
    } else if profile.stream_output {
        framebuffer_sink::verilog_closed_bank_stream(surface)?
    } else if profile.closed_color {
        framebuffer_sink::verilog_closed_bank_color(surface)?
    } else if profile.block_output {
        framebuffer_sink::verilog_closed_bank_block(surface)?
    } else if profile.reuse_temporaries {
        framebuffer_sink::verilog_closed_bank_reuse_depth(surface)?
    } else if profile.closed_bank {
        framebuffer_sink::verilog_closed_bank_depth(surface)?
    } else if profile.closed_depth {
        framebuffer_sink::verilog_closed_depth(surface)?
    } else {
        framebuffer_sink::verilog(surface)?
    };
    if profile.block_output && (profile.closed_color || profile.stream_output) {
        let old = "reg [31:0] oram [0:15];";
        assert_eq!(sink.matches(old).count(), 1, "block output row anchor");
        sink = sink.replacen(
            old,
            "(* syn_ramstyle=\"block_ram\" *) reg [31:0] oram [0:15];",
            1,
        );
    }
    if profile.narrow_quantization {
        sink = super::framebuffer_quantization::lower(sink);
    }
    let mut sink = if native {
        framebuffer_native::bind_products(sink)
    } else {
        sink
    };
    if profile.closed_sink_ingress {
        // This producer constructs eight ordered rows from one held header;
        // global publication already discarded mask0 and bounded the origin.
        // Odd rows are explicitly zero extended. Draw pins are immutable.
        let start = sink
            .find("wire ingress_error =")
            .expect("closed ingress start");
        let end = start
            + sink[start..]
                .find("wire extra_read =")
                .expect("closed ingress end");
        sink.replace_range(start..end, "wire ingress_error = 1'b0;\n    ");
    }
    let mut body = include_str!("postquad.v").to_owned();
    if profile.raw_proposals {
        for (old, new) in [
            (
                "assign lr=advance;assign sr=advance;",
                "assign lr=compute_ce;assign sr=compute_ce;",
            ),
            (
                "assign light_result=lv&&lr;",
                "assign light_result=advance&&lv&&lr;",
            ),
            (
                ".rop_row7_accepted(quad_accepted)",
                ".rop_row7_accepted(raw_retire_proposal)",
            ),
            ("wire front_fault,front_bad,advance,public_credit,cache_drained;", "wire front_fault,front_bad,advance,public_credit,cache_drained;wire raw_retire_proposal;"),
            ("wire lane_ready,lane_accept;", "wire lane_ready,lane_accept;assign raw_retire_proposal=compute_ce&&lane_valid&&lane_ready&&out_lane==3;"),
        ] {
            assert_eq!(body.matches(old).count(), 1, "raw proposal anchor");
            body = body.replacen(old, new, 1);
        }
    }
    if profile.completion_frontiers {
        for (old,new) in [
            ("reg[31:0]light_done,sample_done;", "reg light_complete,sample_complete;reg[4:0]light_last,sample_last;wire[22:0]head_descriptor=descriptors[retire_slot];"),
            ("light_done<=0;sample_done<=0;", "light_complete<=0;sample_complete<=0;light_last<=0;sample_last<=0;"),
            ("light_done[published_slot]<=published_begin[5];sample_done[published_slot]<=published_begin[6];", "// Bypasses are read from the canonical head descriptor."),
            ("if(light_result&&ll)light_done[lk[6:2]]<=1;", "if(quad_accepted&&retire_slot==light_last)light_complete<=0;\n    if(light_result&&ll)begin light_complete<=1;light_last<=lk[6:2];end"),
            ("if(sample_result&&sl)sample_done[sample_address[6:2]]<=1;", "if(quad_accepted&&retire_slot==sample_last)sample_complete<=0;\n    if(sample_result&&sl)begin sample_complete<=1;sample_last<=sample_address[6:2];end"),
            ("light_done[retire_slot]&&sample_done[retire_slot]", "(head_descriptor[5]||light_complete)&&(head_descriptor[6]||sample_complete)"),
            ("active<=0;light_done[retire_slot]<=0;sample_done[retire_slot]<=0;", "active<=0;"),
        ] {
            assert_eq!(body.matches(old).count(),1,"completion frontier anchor");
            body=body.replacen(old,new,1);
        }
    }
    if profile.independent_sink {
        let old = ".clk(clk),.reset(reset),.ce(advance),.lane_valid(lane_valid),.lane(out_lane)";
        let new=".clk(clk),.reset(reset),.ce(compute_ce),.lane_valid(lane_valid&&advance),.lane(out_lane)";
        assert_eq!(body.matches(old).count(), 1, "sink compute domain anchor");
        body = body.replacen(old, new, 1);
    }
    if profile.fused_join {
        for (old, new) in [
            (
                "(state==J_READ||state==J_DEPTH)",
                "(state==J_READ||(state==J_ISSUE&&(!covered||final_in_ready)))",
            ),
            ("state==J_DEPTH};", "state==J_ISSUE};"),
            (
                "J_ISSUE:if(!covered||final_in_ready)state<=J_DEPTH;",
                "J_ISSUE:if(!covered||final_in_ready)begin state<=J_DEPTH_WAIT;paid<=0;end",
            ),
        ] {
            assert_eq!(body.matches(old).count(), 1, "fused join anchor");
            body = body.replacen(old, new, 1);
        }
    }
    let front = if profile.cache_pipeline {
        pixel_sampling_ingress::verilog_with_preparation(slots, backend, preparation, true)?
    } else {
        pixel_sampling_ingress::verilog_with_preparation(slots, backend, preparation, false)?
    };
    let mut source = body + "\n" + &front + "\n" + &sink;
    for (name, width) in [("light", 18), ("sample", 24)] {
        let bank = if native {
            row_store::verilog_sdp36(128, width)?
        } else {
            row_store::verilog(128, width)?
        };
        source.push_str(&bank.replace(
            "gpu_v2_pixel_row_store",
            &format!("gpu_v2_postquad_{name}_bank"),
        ));
    }
    let ports = r#"module gpu_v2_postquad_final(input wire clk,reset,ce,abort,in_valid,
 input wire[23:0]in_tint,in_tex,in_spec,input wire[8:0]in_g,in_h,
 output wire in_ready,input wire out_ready,output wire out_valid,
 output wire[23:0]out_rgb,output wire fault);
 "#;
    match profile.final_stage {
        FinalProfile::Reference => {
            source.push_str(ports);
            source.push_str(r#"assign fault=1'b0;
 gpu_v2_final_stage leaf(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_ready(in_ready),.in_key(6'd0),
 .in_tint_r(in_tint[7:0]),.in_tint_g(in_tint[15:8]),.in_tint_b(in_tint[23:16]),
 .in_tex_r(in_tex[7:0]),.in_tex_g(in_tex[15:8]),.in_tex_b(in_tex[23:16]),
 .in_spec_r(in_spec[7:0]),.in_spec_g(in_spec[15:8]),.in_spec_b(in_spec[23:16]),.in_g(in_g),.in_h(in_h),
 .out_ready(out_ready),.out_valid(out_valid),.out_key(),.out_r(out_rgb[7:0]),.out_g(out_rgb[15:8]),.out_b(out_rgb[23:16]));
 endmodule
 "#);
            source.push_str(final_stage::rtl::source());
        }
        FinalProfile::Folded
        | FinalProfile::Threshold
        | FinalProfile::Shared
        | FinalProfile::SingleCaptured
        | FinalProfile::SingleBorrowed
        | FinalProfile::Forwarded => {
            source.push_str(ports);
            let mut binding=r#"gpu_v2_final_ordered_compact leaf(.clk(clk),.reset(reset),.ce(ce),.commit_allow(1'b1),.abort(abort),
 .in_valid(in_valid),.in_ready(in_ready),.in_tint(in_tint),.in_tex(in_tex),.in_spec(in_spec),.in_g(in_g),.in_h(in_h),
 .out_ready(out_ready),.out_valid(out_valid),.out_rgb(out_rgb),.fault(fault));endmodule
 "#.to_owned();
            if profile.final_reissue || profile.final_two_cycle {
                binding = binding.replace(
                    ".in_ready(in_ready)",
                    ".in_ready(),.proposal_ready(in_ready)",
                );
            } else if profile.fused_join {
                // Read proposal depends only on old credit; commit CE is separate.
                binding =
                    binding.replace(".in_ready(in_ready)", ".in_ready(),.credits(old_credit)");
                let capacity = if matches!(profile.final_stage, FinalProfile::Forwarded) {
                    "old_credit==0"
                } else {
                    "old_credit<8"
                };
                binding = format!(
                    "wire[3:0]old_credit;assign in_ready=!reset&&!abort&&!fault&&{capacity};\n"
                ) + &binding;
            }
            if matches!(profile.final_stage, FinalProfile::Shared) {
                binding = binding.replace(
                    "gpu_v2_final_ordered_compact",
                    "gpu_v2_final_ordered_shared",
                );
            }
            if matches!(
                profile.final_stage,
                FinalProfile::SingleCaptured | FinalProfile::SingleBorrowed
            ) {
                assert!(
                    !profile.fused_join,
                    "Single Final uses its own raw capacity contract"
                );
                binding = binding.replace(
                    "gpu_v2_final_ordered_compact",
                    "gpu_v2_final_ordered_single",
                );
            }
            if matches!(profile.final_stage, FinalProfile::Forwarded) {
                binding = binding.replace(
                    "gpu_v2_final_ordered_compact",
                    if profile.final_two_cycle {
                        "gpu_v2_final_ordered_two_cycle"
                    } else if profile.final_reissue {
                        "gpu_v2_final_ordered_single_reissue"
                    } else {
                        "gpu_v2_final_ordered_single_forwarded"
                    },
                );
            }
            source.push_str(&binding);
            let selected = if matches!(profile.final_stage, FinalProfile::Threshold) {
                final_stage::rtl::ordered_compact::Profile::Threshold
            } else {
                final_stage::rtl::ordered_compact::Profile::Folded
            };
            if matches!(profile.final_stage, FinalProfile::Shared) {
                source.push_str(&final_stage::rtl::ordered_shared::source(native));
            } else if matches!(profile.final_stage, FinalProfile::Forwarded) {
                source.push_str(&if profile.final_two_cycle {
                    if let Some(f) = profile.final_fraction_bits {
                        final_stage::rtl::ordered_shared::source_two_cycle_narrow(native, true, f)
                    } else {
                        final_stage::rtl::ordered_shared::source_two_cycle(native, true)
                    }
                } else if profile.final_reissue {
                    final_stage::rtl::ordered_shared::source_single_reissue(native, true)
                } else {
                    final_stage::rtl::ordered_shared::source_single_forwarded(native, true)
                });
            } else if matches!(
                profile.final_stage,
                FinalProfile::SingleCaptured | FinalProfile::SingleBorrowed
            ) {
                source.push_str(&final_stage::rtl::ordered_shared::source_single(
                    native,
                    matches!(profile.final_stage, FinalProfile::SingleBorrowed),
                ));
            } else {
                source.push_str(&final_stage::rtl::ordered_compact::source(native, selected));
            }
        }
    }
    if profile.join_overlap {
        source = lower_join_overlap(source)?;
    }
    if profile.final_reissue || profile.final_two_cycle {
        source = lower_final_reissue(source, profile.rop_prefix)?;
    }
    if profile.join_pipeline {
        source = lower_join_pipeline(source)?;
    }
    if profile.rop_prefix {
        source = proposal::lower_prefix_shared_proposal(source)?;
    }
    source = match profile.rop_precision {
        RopPrecision::Reference => source,
        RopPrecision::OpaqueReplace => rop_precision::opaque_replace(source)?,
        RopPrecision::Alpha6 => rop_precision::alpha6(source)?,
    };
    if profile.color_precision != ColorPrecision::Reference {
        source = color_precision::lower(source, profile.color_precision != ColorPrecision::Floor9);
        if profile.color_precision == ColorPrecision::NearestPairs9 {
            source = color_precision::nearest_pairs(source)?;
        }
    }
    if profile.color_contract {
        source = color_contract::lower(source)?;
    }
    if profile.color_normalize_511 {
        source = color_precision::normalize_511(
            source,
            profile.color_precision != ColorPrecision::Floor9,
        );
    }
    if profile.framebuffer_write_address {
        source = framebuffer_address::lower(source)?;
    }
    if profile.framebuffer_write_data {
        source = framebuffer_data::lower(source)?;
    }
    if profile.framebuffer_held_draw {
        source = framebuffer_contract::lower(source)?;
    }
    if profile.descriptor_control {
        super::postquad_descriptor::lower_control(&source, native)
    } else if profile.hard_descriptor {
        super::postquad_descriptor::lower(&source, native)
    } else {
        Ok(source)
    }
}

/// Closed throughput candidate: Final launches directly on its paid read DO,
/// and RGB releases at the real color-row ACK. Depth retains its paid witness;
/// global ownership still retires only at actual row7. Used by the selected top.
/// Three MULT9X9 products impose a three-edge arithmetic lower bound, but this
/// old-state single-owner implementation can initiate at most every four edges.
pub fn verilog_join_overlap(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    mut profile: CompositionProfile,
) -> Result<String, String> {
    if !matches!(profile.final_stage, FinalProfile::Forwarded) {
        return Err("Join overlap requires the forwarded three-multiplier Final".into());
    }
    profile.fused_join = true;
    profile.join_overlap = true;
    verilog_profile(slots, surface, backend, profile)
}

fn lower_join_overlap(mut source: String) -> Result<String, String> {
    for (old, new) in [
        (
            "wire final_in_valid=active&&state==J_ISSUE&&covered;",
            "wire final_in_valid=active&&state==J_WAIT&&(paid||basic_valid)&&covered;",
        ),
        (
            "assign basic_read=active&&(state==J_READ||(state==J_ISSUE&&(!covered||final_in_ready)));",
            "assign basic_read=active&&(state==J_READ||(state==J_WAIT&&(paid||basic_valid)&&(!covered||final_in_ready)));",
        ),
        (
            "assign basic_address={retire_slot,issue_lane,state==J_ISSUE};",
            "assign basic_address={retire_slot,issue_lane,state==J_WAIT};",
        ),
        (
            "J_WAIT:if(paid||basic_valid)begin state<=J_ISSUE;paid<=0;end",
            "J_WAIT:if((paid||basic_valid)&&(!covered||final_in_ready))begin state<=J_DEPTH_WAIT;paid<=0;end",
        ),
        (
            "wire lane_valid=active&&depth_valid[out_lane]&&(!out_covered||final_out_valid);",
            "reg color_paid;\n wire lane_valid=active&&depth_valid[out_lane]&&(!out_covered||final_out_valid||color_paid);",
        ),
        (
            "wire lane_ready,lane_accept;",
            "wire lane_ready,lane_accept,sink_color_accept;",
        ),
        (
            "wire final_out_ready=lane_accept&&out_covered;",
            "wire final_out_ready=sink_color_accept&&out_covered;",
        ),
        (
            "assign final_result=lane_accept&&out_covered;",
            "assign final_result=sink_color_accept&&out_covered;",
        ),
        (
            ".lane_ready(lane_ready),.lane_accept(lane_accept),.quad_accepted(quad_accepted),",
            ".lane_ready(lane_ready),.lane_accept(lane_accept),.color_accept(sink_color_accept),.quad_accepted(quad_accepted),",
        ),
        (
            "output wire lane_ready,lane_accept,quad_accepted,",
            "output wire lane_ready,lane_accept,color_accept,quad_accepted,",
        ),
        (
            "assign lane_accept=row_accept&&phase&&lane_matches;",
            "assign lane_accept=row_accept&&phase&&lane_matches;\n assign color_accept=row_accept&&!phase&&lane_matches;",
        ),
        (
            "stopped<=0;active<=0;state<=J_READ;paid<=0;depth_valid<=0;",
            "stopped<=0;active<=0;state<=J_READ;paid<=0;depth_valid<=0;color_paid<=0;",
        ),
        (
            "if(lane_accept)out_lane<=out_lane+1'b1;",
            "if(sink_color_accept)color_paid<=1;\n    if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;end",
        ),
    ] {
        if source.matches(old).count() != 1 {
            return Err(format!("join overlap anchor: {old}"));
        }
        source = source.replacen(old, new, 1);
    }
    Ok(source)
}

fn lower_final_reissue(mut source: String, prefix: bool) -> Result<String, String> {
    // The proposed color ACK funds same-edge reissue. Actual leaf/sink writes
    // remain gated by advance, so denied/CE0 edges cannot transfer ownership.
    for (old,new) in [
        ("wire lane_ready,lane_accept,sink_color_accept;", "wire lane_ready,lane_accept,sink_color_accept,sink_color_ready;"),
        ("wire final_out_ready=sink_color_accept&&out_covered;", "wire final_out_ready=lane_valid&&sink_color_ready&&out_covered;"),
        (".color_accept(sink_color_accept)", ".color_accept(sink_color_accept),.color_ready(sink_color_ready)"),
        ("output wire lane_ready,lane_accept,color_accept,quad_accepted,", "output wire lane_ready,lane_accept,color_accept,color_ready,quad_accepted,"),
        ("assign color_accept=row_accept&&!phase&&lane_matches;", "assign color_accept=row_accept&&!phase&&lane_matches;\n assign color_ready=!reset&&!phase&&lane_matches&&row_ready;"),
        ("wire row_accept,cache_idle;", "wire row_accept,row_ready,cache_idle;"),
        (".row_index(row_index),.row_data(row_data),.row_accept(row_accept),", ".row_index(row_index),.row_data(row_data),.row_accept(row_accept),.row_ready(row_ready),"),
        ("output            row_accept,", "output            row_accept,row_ready,"),
    ] {
        if source.matches(old).count()!=1 {return Err(format!("Final release proposal anchor: {old}"));}
        source=source.replacen(old,new,1);
    }
    let (anchor, readiness) = if prefix {
        (
            "assign row_accept = ce && !flushing && !fault && row_credit && source_ready && row_valid && !source_bad;",
            "!flushing && !fault && row_credit && source_ready && !source_bad",
        )
    } else {
        (
            "assign row_accept = ce && !flushing && !fault && !full && row_valid;",
            "!flushing && !fault && !full",
        )
    };
    if source.matches(anchor).count() != 1 {
        return Err(format!("Final release proposal anchor: {anchor}"));
    }
    source = source.replacen(
        anchor,
        &format!("assign row_ready = {readiness};\n    {anchor}"),
        1,
    );
    Ok(source)
}

/// A fixed two-row producer and ordered consumer, with at most one quad lead.
/// Four existing depth rows are a reservation ring, not a new output queue.
fn lower_join_pipeline(mut source: String) -> Result<String, String> {
    fn once(source: &mut String, old: &str, new: &str) -> Result<(), String> {
        if source.matches(old).count() != 1 {
            return Err(format!("join pipeline anchor: {old}"));
        }
        *source = source.replacen(old, new, 1);
        Ok(())
    }
    let start = source
        .find("reg active;reg[22:0]header;")
        .ok_or("join pipeline declaration start")?;
    let end = source[start..]
        .find(" wire light_return,sample_return,")
        .map(|offset| start + offset)
        .ok_or("join pipeline declaration end")?;
    source.replace_range(
        start..end,
        r#"reg active;reg[22:0]header;
 reg[2:0]state;reg[1:0]issue_lane,out_lane;
 reg paid;reg[3:0]depth_valid,depth_reserved;
 reg[15:0]depth_rows[0:3];
 reg[4:0]issue_slot;
 reg depth_pending;reg[1:0]depth_tag,input_bypass;
 localparam J_READ=0,J_WAIT=1,J_ISSUE=2,J_DEPTH=3,J_DEPTH_WAIT=4,J_DRAIN=5;
 wire issue_ahead=(issue_slot!=retire_slot);
 wire[22:0]issue_header=descriptors[issue_slot];
 wire issue_exists=published_count>{5'd0,issue_ahead};
 // For the sole next quad, an ordered completed prefix must extend past
 // the current retirement head. Bypasses need no branch result ownership.
 wire issue_light_ready=issue_header[5]||(light_complete&&(!issue_ahead||light_last!=retire_slot));
 wire issue_sample_ready=issue_header[6]||(sample_complete&&(!issue_ahead||sample_last!=retire_slot));
 wire covered=issue_header[issue_lane];wire out_covered=header[out_lane];
 wire[6:0]read_key={issue_slot,issue_lane};
 wire join_value_read=active&&state==J_READ&&issue_exists&&issue_light_ready&&issue_sample_ready&&!depth_reserved[issue_lane];
 // Only old free rows pay reads. A retirement or publication on this edge
 // cannot fund a reservation. The producer never crosses a second quad.
 wire join_depth_read=active&&state==J_WAIT&&(paid||basic_valid)&&(!covered||final_in_ready)&&(!issue_ahead||issue_lane!=3);
 wire read_values=join_value_read;
 assign basic_read=join_value_read||join_depth_read;
 assign basic_address={issue_slot,issue_lane,state==J_WAIT};
"#,
    );
    for (old, new) in [
        (
            "wire final_in_valid=active&&state==J_WAIT&&(paid||basic_valid)&&covered;",
            "wire final_in_valid=active&&state==J_WAIT&&(paid||basic_valid)&&covered&&(!issue_ahead||issue_lane!=3);",
        ),
        (
            "wire[8:0]final_g=header[5]?9'd256:light_data[8:0];",
            "wire[8:0]final_g=input_bypass[0]?9'd256:light_data[8:0];",
        ),
        (
            "wire[8:0]final_h=header[5]?9'd0:light_data[17:9];",
            "wire[8:0]final_h=input_bypass[0]?9'd0:light_data[17:9];",
        ),
        (
            "wire[23:0]final_texture=header[6]?24'hffffff:sample_data;",
            "wire[23:0]final_texture=input_bypass[1]?24'hffffff:sample_data;",
        ),
        (
            "stopped<=0;active<=0;state<=J_READ;paid<=0;depth_valid<=0;color_paid<=0;",
            "stopped<=0;active<=0;state<=J_READ;paid<=0;depth_valid<=0;color_paid<=0;issue_slot<=0;depth_reserved<=0;depth_pending<=0;depth_tag<=0;input_bypass<=0;",
        ),
        (
            "if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;end",
            "if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;depth_valid[out_lane]<=0;depth_reserved[out_lane]<=0;end",
        ),
        (
            "if(quad_accepted)begin active<=0;end",
            "if(quad_accepted)begin\n     if(published_count>1)begin active<=1;header<=descriptors[retire_slot+5'd1];end\n     else active<=0;\n    end",
        ),
    ] {
        once(&mut source, old, new)?;
    }
    let start = source
        .find("   // Paid synchronous read returns are captured regardless of compute CE.")
        .ok_or("join pipeline paid start")?;
    let end = source[start..]
        .find("   if(public_publish)")
        .map(|offset| start + offset)
        .ok_or("join pipeline paid end")?;
    source.replace_range(
        start..end,
        r#"   // A paid depth return owns its tagged ring row even on CE0.
   depth_pending<=advance&&join_depth_read;
   if(advance&&join_depth_read)depth_tag<=issue_lane;
   if(depth_pending&&basic_valid)begin
    depth_rows[depth_tag]<=basic_data[15:0];depth_valid[depth_tag]<=1;
   end
   if(active&&state==J_WAIT&&basic_valid&&!depth_pending)paid<=1;
"#,
    );
    let start = source
        .find("    if(!active&&published_count!=0&&")
        .ok_or("join pipeline activation start")?;
    let end = source[start..]
        .find("    if(sink_color_accept)")
        .map(|offset| start + offset)
        .ok_or("join pipeline activation end")?;
    source.replace_range(
        start..end,
        r#"    if(!active&&published_count!=0)begin
     active<=1;header<=descriptors[retire_slot];out_lane<=0;
    end
    if(active)case(state)
     J_READ:if(join_value_read)begin
      state<=J_WAIT;paid<=0;depth_reserved[issue_lane]<=1;
      // These two controls survive the last g loan even when issue_slot
      // advances at lane3 acceptance. Full origin/header stays canonical.
      input_bypass<=issue_header[6:5];
     end
     J_WAIT:if(join_depth_read)begin
      state<=J_READ;paid<=0;
      if(issue_lane==3)begin issue_lane<=0;issue_slot<=issue_slot+1'b1;end
      else issue_lane<=issue_lane+1'b1;
     end
    endcase
"#,
    );
    Ok(source)
}

/// Complete cadence with an explicit independently executable Lighting program.
/// Replace one complete generated component; arithmetic is never text-edited.
pub fn verilog_cadence_h9_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_cadence_h9_profile(
        slots,
        surface,
        backend,
        crate::texture::rtl::runtime_rows::Profile::default(),
    )
}

fn verilog_cadence_h9_profile(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    preparation: crate::texture::rtl::runtime_rows::Profile,
) -> Result<String, String> {
    let source = verilog_cadence_profile(slots, surface, backend, preparation)?;
    let reference = super::lighting_consumer::verilog()?;
    if source.matches(&reference).count() != 1 {
        return Err("Lighting factory anchor".into());
    }
    let candidate = super::lighting_consumer::verilog_with_calendar(
        crate::lighting::calendars::UnifiedCalendar::SmoothH9,
    )?;
    Ok(source.replacen(&reference, &candidate, 1))
}

/// Explicit H9 timing experiment: local raw validation precedes terminal commit.
pub fn verilog_cadence_h9_control_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    h9_control::lower(verilog_cadence_h9_candidate(slots, surface, backend)?)
}

/// Explicit second timing candidate; previous cadence and h9 remain references.
pub fn verilog_cadence_h9_local_fault_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: super::quad_ingress::rtl::Backend,
) -> Result<String, String> {
    h9_control::lower_local_fault(verilog_cadence_h9_candidate(slots, surface, backend)?)
}

/// Independent Quad authority experiment over the retained cadence factory.
pub fn verilog_quad_authority_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    authority: quad_authority::Authority,
) -> Result<String, String> {
    quad_authority::lower(
        verilog_cadence_candidate(slots, surface, backend)?,
        authority,
    )
}

/// Actual Program h9, continuous prepare, canonical ingress and integer filter.
pub fn verilog_combined_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    let preparation = crate::texture::rtl::runtime_rows::Profile {
        continuous_prepare: true,
        ..Default::default()
    };
    let source = verilog_cadence_h9_profile(slots, surface, backend, preparation)?;
    let source = quad_authority::lower(source, quad_authority::Authority::Ingress)?;
    let source = h9_control::lower_local_fault(source)?;
    Ok(filter_precision::lower(source))
}

/// Prepare a borrowed head address from the old-state paid retirement offer.
/// Actual header capture/retirement remain gated by the terminal commit fence.
pub fn verilog_combined_prehead_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    let source = verilog_combined_candidate(slots, surface, backend)?;
    let old = ".descriptor_head_slot(quad_accepted?retire_slot+5'd1:retire_slot)";
    let new = ".descriptor_head_slot(raw_retire_proposal?retire_slot+5'd1:retire_slot)";
    if source.matches(old).count() != 1 {
        return Err("Combined head address anchor".into());
    }
    Ok(source.replacen(old, new, 1))
}

/// Speculate only the asynchronous address; exact raw sink readiness precedes
/// the global commit gate, while all writes and ownership still use that gate.
pub fn verilog_combined_prehead_exact_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    let mut source = verilog_combined_candidate(slots, surface, backend)?;
    for (old,new) in [
        ("wire coalition_allow=compute_ce&&!(front_bad||sink_bad||final_bad);", "wire raw_head_retire;\n wire coalition_allow=compute_ce&&!(front_bad||sink_bad||final_bad);"),
        (".descriptor_head_slot(quad_accepted?retire_slot+5'd1:retire_slot)", ".descriptor_head_slot(raw_head_retire?retire_slot+5'd1:retire_slot)"),
        (".lane_ready(lane_ready),.lane_accept(lane_accept),", ".raw_head_retire(raw_head_retire),.lane_ready(lane_ready),.lane_accept(lane_accept),"),
        ("output wire lane_ready,lane_accept,color_accept,color_ready,quad_accepted,", "output wire raw_head_retire,\n output wire lane_ready,lane_accept,color_accept,color_ready,quad_accepted,"),
        ("assign quad_accepted=lane_accept&&(expected_lane==2'd3);", "assign quad_accepted=lane_accept&&(expected_lane==2'd3);\n assign raw_head_retire=ce&&!reset&&!fault&&lane_valid&&phase&&lane_matches&&(phase==shade_phase)&&row_ready&&(expected_lane==2'd3);"),
    ] {
        if source.matches(old).count()!=1 {return Err(format!("Exact head address anchor {old}"));}
        source=source.replacen(old,new,1);
    }
    Ok(source)
}

#[path = "postquad_maintenance_planes.rs"]
pub mod maintenance_planes;
/// Remaining-plane snapshot only; original transaction state and paid flags.
pub fn verilog_combined_maintenance_planes_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    maintenance_planes::lower(verilog_combined_prehead_exact_candidate(
        slots, surface, backend,
    )?)
}

#[path = "postquad_weight_order.rs"]
pub mod weight_order;
pub fn verilog_combined_weight_order_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    weight_order::lower(verilog_combined_maintenance_planes_candidate(
        slots, surface, backend,
    )?)
}

#[path = "postquad_unique_weights.rs"]
pub mod unique_weights;
pub fn verilog_combined_unique_weights_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    unique_weights::lower(verilog_combined_weight_order_candidate(
        slots, surface, backend,
    )?)
}

#[path = "postquad_unique_group34.rs"]
pub mod unique_group34;
/// Closed immutable-draw profile: draw slot may change only after pipeline_idle.
pub fn verilog_combined_unique_group34_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    unique_group34::lower(verilog_combined_unique_weights_candidate(
        slots, surface, backend,
    )?)
}

#[path = "postquad_stream_context.rs"]
pub mod stream_context;
/// Explicit read-to-return context handoff on the fixed four-edge ROP calendar.
pub fn verilog_combined_stream_context_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    stream_context::lower(verilog_combined_unique_group34_candidate(
        slots, surface, backend,
    )?)
}

#[path = "postquad_stream_onehot.rs"]
pub mod stream_onehot;
/// Explicit reset-led stream context with unique valid-tag hit reduction.
pub fn verilog_combined_stream_onehot_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    stream_onehot::lower(verilog_combined_stream_context_candidate(
        slots, surface, backend,
    )?)
}

#[path = "postquad_closed_basic.rs"]
pub mod closed_basic;
pub fn verilog_combined_closed_basic_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    closed_basic::lower(verilog_combined_stream_onehot_candidate(
        slots, surface, backend,
    )?)
}

#[path = "postquad_texture_bank_carry.rs"]
pub mod texture_bank_carry;
pub fn verilog_combined_closed_basic_carry_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    texture_bank_carry::lower(verilog_combined_closed_basic_candidate(
        slots, surface, backend,
    )?)
}

#[path = "postquad_group48.rs"]
pub mod group48;
pub fn verilog_combined_group48_pool64_candidate(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    group48::lower(verilog_combined_closed_basic_carry_candidate(
        slots, surface, backend,
    )?)
}
