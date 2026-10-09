//! Persistent sampling connection unit: actual preparation -> pooled packet ->
//! cache -> independently verified ColorEmu, one cycle per call. Actual MC.
//! Hot windows are reported explicitly; no average-time substitution is used.
#[path = "texture_bound_probe.rs"]
#[allow(dead_code)]
mod baseline;
use baseline::{physical, support};
use gpu_v2::texture::{
    emu::color::{self, Output as ColorOutput},
    ports::*,
    sim::{oracle, staged::bound, timed},
};
use std::{fs, io::Write, path::Path};

fn quads(case: &str) -> Vec<QuadInput> {
    (0..40)
        .map(|i| {
            let filter = match case {
                "nearest" => Filter::Nearest,
                "bilinear" => Filter::Bilinear,
                _ => Filter::Trilinear,
            };
            let mut q = support::input(5, filter, [0.13, 0.07]);
            q.quad_id = (i % 16) as u8;
            q.mask = [15, 1, 6, 15][i % 4];
            q.uv[1][0] += 1.0 / 32.0;
            q.uv[2][1] += 1.0 / 32.0;
            q.uv[3][0] += 1.0 / 32.0;
            q.uv[3][1] += 1.0 / 32.0;
            match case {
                "mip" => q.lod_bias = 0.5,
                "seam" => {
                    q.uv = [[0.0; 2]; 4];
                    q.uv[3][0] = 1.0 / 262144.0;
                    q.lod_bias = 13.5;
                }
                _ => {}
            }
            q
        })
        .collect()
}

fn expected(inputs: &[QuadInput], slot: Slot, bytes: &[u8]) -> Vec<ColorOutput> {
    let mut cache = oracle::Cache::new(vec![slot]).unwrap();
    let mut image = support::Image {
        bytes: bytes.to_vec(),
        requests: vec![],
    };
    inputs
        .iter()
        .flat_map(|q| {
            oracle::sample(q, &mut cache, &mut image, Config::counted())
                .unwrap()
                .pixels
                .into_iter()
                .map(|p| ColorOutput {
                    key: q.quad_id * 4 + p.lane,
                    rgb: p.rgb,
                })
        })
        .collect()
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map_or("target/gpu-v2-sampling-step", String::as_str),
    );
    fs::create_dir_all(root).unwrap();
    let mut summary = fs::File::create(root.join("sampling_step.csv")).unwrap();
    writeln!(
        summary,
        "case,quads,pixels,packets,wall,cache_enabled,hot_pixels,hot_packets,hot_quads,hot_cache_enabled,packet_interval_wall,pixel_interval_wall,quad_interval_wall,color_credits_peak,color_stalls,gated_edges,cache_reads,refills,mc_requests,hot_wall,hot_caller_enabled,hot_refills"
    )
    .unwrap();
    let slot = support::slot(5, true);
    let bytes = support::asset(slot, support::pattern);
    for case in ["nearest", "bilinear", "mip", "seam"] {
        let qs = quads(case);
        let want = expected(&qs, slot, &bytes);
        let mut memory =
            physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, false);
        let mut session = bound::session::Session::new(
            &qs,
            &[slot],
            bound::control::Hardware {
                storage: bound::control::Storage::Packed,
                ..Default::default()
            },
            timed::Hardware {
                prefetch: false,
                max_cycles: 100_000,
                max_quads: 64,
                ..Default::default()
            },
            100_000,
        )
        .unwrap();
        let mut results = vec![];
        let mut edges = vec![];
        let mut t = 0_u64;
        let mut pixels = 0_u64;
        let mut packets = 0_u64;
        let mut accepted = 0_u64;
        let mut enabled = 0_u64;
        let mut caller_enabled = 0_u64;
        let mut peak_credits = 0;
        while !session.idle() {
            t += 1;
            assert!(t < 100_000, "sampling step did not drain");
            let st = session
                .step(
                    &mut memory,
                    timed::Control {
                        ce: t % 17 > 3,
                        result_ready: t > 400 && t % 13 > 2,
                    },
                )
                .unwrap();
            pixels += st.results.len() as u64;
            packets += st
                .cache
                .events
                .iter()
                .filter(|e| matches!(e, timed::Event::Produced { .. }))
                .count() as u64;
            accepted += u64::from(st.accepted);
            enabled += u64::from(st.effective_ce);
            caller_enabled += u64::from(st.control.ce);
            peak_credits = peak_credits.max(st.snapshot.color.result_credits);
            edges.push((
                t,
                enabled,
                pixels,
                packets,
                accepted,
                caller_enabled,
                session.cache_stats().refills,
            ));
            results.extend(st.results.iter().copied());
        }
        assert_eq!(results, want, "{case}: results must match the oracle");
        assert!(pixels > 0 && packets > 0, "{case}: link must run");
        // Hot window excludes the first and last eight committed pixels.
        let total = edges.last().unwrap().2;
        let skip = 8.min(total / 3);
        let lo = edges.iter().find(|e| e.2 >= skip).copied().unwrap();
        let hi = edges
            .iter()
            .rev()
            .find(|e| e.2 <= total - skip)
            .copied()
            .unwrap();
        let hot_enabled = hi.1 - lo.1;
        let hot_wall = hi.0 - lo.0;
        let hot_caller_enabled = hi.5 - lo.5;
        let hot_refills = hi.6 - lo.6;
        let hot_pixels = hi.2 - lo.2;
        let hot_packets = hi.3 - lo.3;
        let hot_quads = hi.4 - lo.4;
        let iv = |n: u64| {
            if n == 0 {
                f64::NAN
            } else {
                hot_wall as f64 / n as f64
            }
        };
        writeln!(
            summary,
            "{case},{accepted},{pixels},{packets},{t},{enabled},{hot_pixels},{hot_packets},{hot_quads},{hot_enabled},{},{},{},{peak_credits},{},{},{},{},{},{hot_wall},{hot_caller_enabled},{hot_refills}",
            iv(hot_packets),
            iv(hot_pixels),
            iv(hot_quads),
            session.stats.color_stalls,
            session.stats.color_gated_edges,
            session.cache_stats().reads,
            session.cache_stats().refills,
            memory.requests,
        )
        .unwrap();
        println!(
            "{case}: {pixels} px, {packets} packets, window {} wall-clk/packet, {} wall-clk/px; refills={hot_refills}",
            iv(hot_packets),
            iv(hot_pixels)
        );
    }
    let mut cost = fs::File::create(root.join("sampling_step_cost.csv")).unwrap();
    writeln!(cost, "field,value,note").unwrap();
    let a = color::ALLOCATION;
    for (name, value, note) in [
        (
            "color_dsp9_lanes",
            a.dsp9_lanes as u64,
            "declared Multiply9 lanes (existing color target)",
        ),
        (
            "color_dsp18_equivalents",
            a.dsp18_equivalents as u64,
            "declared packing",
        ),
        (
            "color_hard_product_bits",
            a.hard_product_bits as u64,
            "existing color macro",
        ),
        (
            "color_datapath_ff_bits",
            a.datapath_ff_bits as u64,
            "existing color datapath",
        ),
        (
            "color_result_control_ff_bits",
            a.result_and_control_ff_bits as u64,
            "existing result queue/credits",
        ),
        (
            "color_bsram",
            a.bsram as u64,
            "no new result BSRAM in this unit",
        ),
        (
            "link_captured_input_bits",
            72 + 64 + 1,
            "one captured packet/texel register and valid; logical bits only",
        ),
        (
            "link_result_lane_bits",
            16 * 4,
            "public ID ownership until actual consumption",
        ),
        (
            "link_terminal_fault_bits",
            1,
            "recreation required after error",
        ),
        (
            "packet_storage",
            0,
            "existing 64x72 pool unchanged (P16/G32, two heads)",
        ),
        (
            "mc_owner",
            1,
            "single texture RefillPort owner; no second MC adapter",
        ),
    ] {
        writeln!(cost, "{name},{value},{note}").unwrap();
    }
}
