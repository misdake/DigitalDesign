//! Actual bound preparation -> packet pool -> cache/data -> color connection.
//! Warmth is qualified by real refill submissions in the measured interval.
#[path = "texture_bound_probe.rs"]
#[allow(dead_code)]
mod baseline;
use baseline::{physical, support};
use gpu_v2::texture::{
    ports::*,
    sim::{oracle, staged::bound, timed},
};
use std::{fs, io::Write, path::Path};
fn edge(
    trace: &mut fs::File,
    wall: u64,
    enabled: u64,
    ce: bool,
    domain: &str,
    event: &impl std::fmt::Debug,
) {
    let event = format!("{event:?}").replace('"', "\"\"");
    writeln!(trace, "{wall},{enabled},{ce},{domain},\"{event}\"").unwrap();
}
fn inputs(case: &str) -> Vec<QuadInput> {
    (0..48)
        .map(|i| {
            let mut q = support::input(
                5,
                if case == "bilinear" {
                    Filter::Bilinear
                } else {
                    Filter::Trilinear
                },
                [0.13, 0.07],
            );
            q.quad_id = (i % 16) as u8;
            q.uv[1][0] += 1.0 / 32.0;
            q.uv[2][1] += 1.0 / 32.0;
            q.uv[3][0] += 1.0 / 32.0;
            q.uv[3][1] += 1.0 / 32.0;
            if case == "two-plane" {
                q.lod_bias = 0.5;
            }
            if case == "seams" {
                q.uv = [[0.0; 2]; 4];
                q.uv[3][0] = 1.0 / 262144.0;
                q.lod_bias = 13.5;
            }
            if case == "recovery" && i >= 32 {
                for uv in &mut q.uv {
                    uv[0] += 0.5;
                    uv[1] += 0.5;
                }
            }
            q
        })
        .collect()
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map_or("target/gpu-v2-texture-connected", String::as_str),
    );
    fs::create_dir_all(root).unwrap();
    let mut summary = fs::File::create(root.join("connected.csv")).unwrap();
    let mut blockers = fs::File::create(root.join("hot_blockers.csv")).unwrap();
    writeln!(
        blockers,
        "case,enabled,cache_reads,no_old_head,ready_head_wait,producer_credit_full,packet_issues"
    )
    .unwrap();
    writeln!(summary,"case,path,wall,enabled,packets,pixels,refills,hot_packets,hot_pixels,hot_enabled,hot_refills,hot_packet_interval,hot_CPP,hot_quad_interval,peak_P,peak_G,peak_rows,peak_heads_pending,CE_refill_beats,result_credit_stalls,hot_start_wall,hot_end_wall").unwrap();
    let s = support::slot(5, true);
    let bytes = support::asset(s, support::pattern);
    for case in ["bilinear", "two-plane", "seams", "recovery"] {
        let qs = inputs(case);
        let mut image = support::Image {
            bytes: bytes.clone(),
            requests: vec![],
        };
        let mut oc = oracle::Cache::new(vec![s]).unwrap();
        let expected: Vec<_> = qs
            .iter()
            .flat_map(|q| {
                oracle::sample(q, &mut oc, &mut image, Config::counted())
                    .unwrap()
                    .pixels
                    .into_iter()
                    .map(|p| timed::PixelResult {
                        quad_id: q.quad_id,
                        lane: p.lane,
                        rgb: p.rgb,
                    })
            })
            .collect();
        for pooled in [false, true] {
            let mut memory =
                physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, false);
            let hw = bound::control::Hardware {
                storage: bound::control::Storage::Packed,
                ..Default::default()
            };
            let c = timed::Hardware {
                prefetch: false,
                max_cycles: 100_000,
                ..Default::default()
            };
            let ctl = |t| timed::Control {
                ce: case != "recovery" || t % 23 > 7,
                result_ready: case != "recovery" || t > 900 && t % 31 > 7,
            };
            let r = if pooled {
                bound::system::run_pooled(&qs, &[s], &mut memory, hw, c, ctl)
            } else {
                bound::system::run(&qs, &[s], &mut memory, hw, c, ctl)
            }
            .unwrap();
            assert_eq!(r.cache.pixels, expected);
            if case == "bilinear" {
                let inventory = bound::inventory::describe(
                    &r.binding,
                    r.preparation_hardware,
                    &r.cache.hardware,
                )
                .unwrap();
                let mut file = fs::File::create(root.join(if pooled {
                    "allocation_pool.csv"
                } else {
                    "allocation_native.csv"
                }))
                .unwrap();
                writeln!(file, "field,FF,RAM16SDP4,BSRAM,hard_DSP_bits,ports").unwrap();
                for row in &inventory.rows {
                    writeln!(
                        file,
                        "\"{}\",{},{},{},{},\"{}\"",
                        row.name,
                        row.ff_bits,
                        row.sdp4_cells,
                        row.bsram,
                        row.hard_pipeline_bits,
                        row.ports
                    )
                    .unwrap();
                }
                writeln!(
                    file,
                    "TOTAL,{},{},{},{},declared retained native stages plus actual packet boundary",
                    inventory.ff_bits,
                    inventory.sdp4_cells,
                    inventory.bsram,
                    inventory.hard_pipeline_bits
                )
                .unwrap();
            }
            let commits: Vec<_> = r
                .cache
                .steps
                .iter()
                .filter(|st| {
                    st.events
                        .iter()
                        .any(|e| matches!(e, timed::Event::Commit { .. }))
                })
                .collect();
            let start = commits[63].cycle;
            let end = commits[127].cycle;
            let hot: Vec<_> = r
                .cache
                .steps
                .iter()
                .filter(|st| st.cycle > start && st.cycle <= end)
                .collect();
            let enabled = hot.iter().filter(|st| st.control.ce).count();
            let hot_packets = hot
                .iter()
                .flat_map(|st| &st.events)
                .filter(|e| matches!(e, timed::Event::Read { .. }))
                .count();
            let refills = hot
                .iter()
                .flat_map(|st| &st.events)
                .filter(|e| matches!(e, timed::Event::Submitted { .. }))
                .count();
            if case != "recovery" {
                assert_eq!(refills, 0, "hot interval must use normally warmed lines");
            }
            if pooled {
                let mut no_head = 0;
                let mut head_wait = 0;
                let mut producer_full = 0;
                let mut issues = 0;
                for (i, st) in r
                    .cache
                    .steps
                    .iter()
                    .enumerate()
                    .filter(|(_, st)| st.cycle > start && st.cycle <= end && st.control.ce)
                {
                    let previous = r.cache.steps[i - 1].snapshot.packet_pool.as_ref().unwrap();
                    if !st
                        .events
                        .iter()
                        .any(|e| matches!(e, timed::Event::Read { .. }))
                    {
                        if previous.heads == 0 {
                            no_head += 1;
                        } else {
                            head_wait += 1;
                        }
                    }
                    producer_full += usize::from(previous.producer == 16);
                    issues += st.packet_issues.len();
                }
                assert_eq!(enabled, hot_packets + no_head + head_wait);
                writeln!(
                    blockers,
                    "{case},{enabled},{hot_packets},{no_head},{head_wait},{producer_full},{issues}"
                )
                .unwrap();
            }
            let mut peaks = [0; 4];
            for st in &r.cache.steps {
                if let Some(p) = &st.snapshot.packet_pool {
                    for (v, n) in peaks.iter_mut().zip([
                        p.producer,
                        p.groups,
                        p.rows,
                        p.heads + usize::from(p.pending),
                    ]) {
                        *v = (*v).max(n);
                    }
                }
            }
            let ce_beats = r
                .cache
                .steps
                .iter()
                .filter(|st| !st.control.ce)
                .flat_map(|st| &st.events)
                .filter(|e| matches!(e, timed::Event::Beat { .. }))
                .count();
            if pooled && case == "recovery" {
                assert!(ce_beats > 0 && r.cache.stats.result_credit_stalls > 0);
            }
            let packet_interval = enabled as f64 / hot_packets as f64;
            let cpp = enabled as f64 / 64.0;
            let path = if pooled { "pool" } else { "native" };
            writeln!(summary,"{case},{path},{},{},{},{},{},{hot_packets},64,{enabled},{refills},{packet_interval},{cpp},{},{},{},{},{},{ce_beats},{},{start},{end}",r.cache.stats.wall_cycles,r.cache.stats.enabled_cycles,r.cache.stats.reads,r.cache.pixels.len(),memory.requests,cpp*4.0,peaks[0],peaks[1],peaks[2],peaks[3],r.cache.stats.result_credit_stalls).unwrap();
            {
                let name = if pooled {
                    format!("{case}_edges.csv")
                } else {
                    format!("{case}_native_edges.csv")
                };
                let mut trace = fs::File::create(root.join(name)).unwrap();
                writeln!(trace, "wall,enabled,ce,domain,event").unwrap();
                let mut t = 0;
                for (prep, st) in r.preparation.iter().zip(&r.cache.steps) {
                    if st.control.ce {
                        t += 1;
                    }
                    for e in &prep.events {
                        edge(&mut trace, st.cycle, t, st.control.ce, "prep", e);
                    }
                    for e in &st.packet_events {
                        edge(&mut trace, st.cycle, t, st.control.ce, "pool", e);
                    }
                    for e in &st.events {
                        if matches!(
                            e,
                            timed::Event::Read { .. }
                                | timed::Event::Commit { .. }
                                | timed::Event::Submitted { .. }
                                | timed::Event::Produced { .. }
                                | timed::Event::Accepted { .. }
                        ) {
                            edge(&mut trace, st.cycle, t, st.control.ce, "cache", e);
                        }
                    }
                }
            }
            println!("{case}/{path}: hot {packet_interval} clocks/packet, {cpp} CPP; P{} G{} rows{} heads{}",peaks[0],peaks[1],peaks[2],peaks[3]);
        }
    }
    let mut topology = fs::File::create(root.join("packet_selectors.csv")).unwrap();
    writeln!(topology, "path,bit_mux_nodes,scope").unwrap();
    for (name, n) in [
        ("producer payload-W descriptor selection", 13 * 15),
        ("producer index-transfer descriptor selection", 13 * 15),
        ("Group index front selection", 6 * 31),
        ("two head payload and valid selection", 74),
    ] {
        writeln!(
            topology,
            "{name},{n},conservative FF descriptors; topology nodes are not fitted LUTs"
        )
        .unwrap();
    }
    writeln!(topology,"TOTAL,650,ring compare/decode/CE/data enables/arithmetic and pipeline selection are additional").unwrap();
}
