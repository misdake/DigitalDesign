//! Bounded live quad stimulus, real MC and actual ColorEmu results.
//! Host compilation is counted preparation replay, not cycle arithmetic emu.
#[path = "../tests/support/sdram/physical_texture.rs"]
#[allow(dead_code)]
mod physical;
#[path = "../tests/support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    emu::color::Output,
    ports::*,
    sim::{oracle, staged::bound, timed},
};
use std::{fs, io::Write, path::Path};

fn quad(case: &str, i: usize) -> QuadInput {
    let mut q = support::input(
        5,
        match case {
            "nearest" => Filter::Nearest,
            "bilinear" => Filter::Bilinear,
            _ => Filter::Trilinear,
        },
        [0.13, 0.07],
    );
    q.quad_id = (i % 16) as u8;
    q.uv[1][0] += 1.0 / 32.0;
    q.uv[2][1] += 1.0 / 32.0;
    q.uv[3][0] += 1.0 / 32.0;
    q.uv[3][1] += 1.0 / 32.0;
    if case == "mip" {
        q.lod_bias = 0.5;
    } else if case == "seam" {
        q.uv = [[0.0; 2]; 4];
        q.uv[3][0] += 1.0 / 262144.0;
        q.lod_bias = 13.5;
    }
    q
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map_or("target/gpu-sampling-runtime", String::as_str),
    );
    fs::create_dir_all(root).unwrap();
    let mut summary = fs::File::create(root.join("runtime_calendar.csv")).unwrap();
    writeln!(summary, "case,mode,warm_wall,warm_refills,quads,pixels,packets,wall,caller_enabled,cache_enabled,refills,compiled_quads,rejected,P_peak,G_peak,context_peak,source_peak,color_credit_peak,gated,wall_per_pixel,enabled_per_pixel").unwrap();
    for case in ["nearest", "bilinear", "mip", "seam"] {
        for paused in [false, true] {
            let slot = support::slot(5, true);
            let bytes = support::asset(slot, support::pattern);
            let mut memory =
                physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, false);
            let mut r = bound::runtime::Runtime::new(
                &[slot],
                bound::control::Hardware {
                    storage: bound::control::Storage::Packed,
                    ..Default::default()
                },
                timed::Hardware {
                    prefetch: false,
                    max_quads: 1,
                    ..Default::default()
                },
                100_000,
            )
            .unwrap();
            // Warm the actual same-instance cache, then start a drained window.
            let warm = quad(case, 0);
            assert!(
                r.step(&mut memory, Some(&warm), timed::Control::default())
                    .unwrap()
                    .accepted
            );
            for _ in 0..10_000 {
                r.step(&mut memory, None, timed::Control::default())
                    .unwrap();
                if r.idle() {
                    break;
                }
            }
            assert!(r.idle(), "warm-up watchdog");
            let warm_wall = r.stats.link.wall;
            let warm_refills = r.cache_stats().refills;
            let before = r.stats.clone();
            let mode = if paused { "paused" } else { "hot" };
            let mut trace = fs::File::create(root.join(format!("{case}-{mode}.csv"))).unwrap();
            let mut events =
                fs::File::create(root.join(format!("{case}-{mode}-events.tsv"))).unwrap();
            writeln!(trace, "wall,window_wall,caller_ce,cache_ce,ready,offered,accepted,results,prep_live,public_lanes,color_credits,capture,P,G,rows,heads,pending,beats").unwrap();
            writeln!(
                events,
                "wall\tcache_enabled\tpreparation_events\tcache_events\tcolor_events"
            )
            .unwrap();
            let mut offered = quad(case, 0);
            let mut accepted = vec![];
            let mut got = vec![];
            let mut caller_enabled = 0;
            let mut peak_p = 0;
            let mut peak_g = 0;
            for t in 1..=80_000 {
                let st = r
                    .step(
                        &mut memory,
                        (accepted.len() < 48).then_some(&offered),
                        timed::Control {
                            ce: !paused || t % 17 > 3,
                            result_ready: !paused || (t > 1_200 && t % 29 > 5),
                        },
                    )
                    .unwrap();
                assert_eq!(
                    st.accepted,
                    st.offered.is_some() && st.input_ready && st.control.ce
                );
                let pool = st.snapshot.cache.packet_pool.as_ref().unwrap();
                assert!(pool.producer <= 16 && pool.groups <= 32 && pool.rows <= 64);
                assert!(pool.heads + usize::from(pool.pending) <= 2);
                peak_p = peak_p.max(pool.producer);
                peak_g = peak_g.max(pool.groups);
                let beats = st
                    .cache
                    .events
                    .iter()
                    .filter(|e| matches!(e, timed::Event::Beat { .. }))
                    .count();
                let public_lanes: u32 = st
                    .snapshot
                    .result_lanes
                    .iter()
                    .map(|m| m.count_ones())
                    .sum();
                writeln!(
                    trace,
                    "{},{t},{},{},{},{},{},{},{},{public_lanes},{},{},{},{},{},{},{},{beats}",
                    st.cycle,
                    u8::from(st.control.ce),
                    u8::from(st.effective_ce),
                    u8::from(st.input_ready),
                    st.offered.map_or(-1, i32::from),
                    u8::from(st.accepted),
                    st.results.len(),
                    st.snapshot.preparation_live.count_ones(),
                    st.snapshot.color.result_credits,
                    u8::from(st.snapshot.color_input),
                    pool.producer,
                    pool.groups,
                    pool.rows,
                    pool.heads,
                    u8::from(pool.pending)
                )
                .unwrap();
                if !st.preparation.events.is_empty()
                    || !st.cache.events.is_empty()
                    || !st.color.events.is_empty()
                {
                    writeln!(
                        events,
                        "{}\t{}\t{:?}\t{:?}\t{:?}",
                        st.cycle,
                        r.stats.link.enabled,
                        st.preparation.events,
                        st.cache.events,
                        st.color.events
                    )
                    .unwrap();
                }
                caller_enabled += u64::from(st.control.ce);
                got.extend(st.results);
                if st.accepted {
                    accepted.push(offered);
                    offered = quad(case, accepted.len());
                }
                if accepted.len() == 48 && r.idle() {
                    break;
                }
            }
            assert!(r.idle() && accepted.len() == 48, "runtime window watchdog");
            // Goldens are computed afterward and never drive runtime decisions.
            let mut oracle_cache = oracle::Cache::new(vec![slot]).unwrap();
            let mut image = support::Image {
                bytes,
                requests: vec![],
            };
            let want: Vec<_> = accepted
                .iter()
                .flat_map(|q| {
                    oracle::sample(q, &mut oracle_cache, &mut image, Config::counted())
                        .unwrap()
                        .pixels
                        .into_iter()
                        .map(|p| Output {
                            key: q.quad_id * 4 + p.lane,
                            rgb: p.rgb,
                        })
                })
                .collect();
            assert_eq!(got, want);
            let wall = r.stats.link.wall - warm_wall;
            let enabled = r.stats.link.enabled - before.link.enabled;
            let packets = r.stats.link.packets - before.link.packets;
            let refills = r.cache_stats().refills - warm_refills;
            assert_eq!(refills, 0, "measurement window must actually be warm");
            let interval_wall = wall as f64 / got.len() as f64;
            let interval_enabled = enabled as f64 / got.len() as f64;
            writeln!(summary, "{case},{mode},{warm_wall},{warm_refills},48,{},{packets},{wall},{caller_enabled},{enabled},{refills},{},{},{peak_p},{peak_g},{},{},{},{},{interval_wall},{interval_enabled}",
                got.len(), r.stats.compilations - before.compilations, r.stats.rejected - before.rejected,
                r.preparation_stats().peak_contexts, r.stats.peak_preparation_programs,
                r.stats.link.peak_color_credits, r.stats.link.color_gated_edges - before.link.color_gated_edges).unwrap();
            println!("{case}/{mode}: {} pixels, {packets} packets, {wall} wall/{enabled} enabled, {interval_wall:.3} wall per pixel, warm refills={refills}", got.len());
        }
    }
}
