//! Finite control/port experiment; fixture cycles are not measured MC performance.
use gpu_v2::framebuffer::{
    ports::*,
    sim::fixture::{replay, replay_forwarding, replay_pipelined, Fixture},
};
use std::{fmt::Write as _, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pipelined = std::env::args().any(|a| a == "--pipelined");
    let forwarding = std::env::args().any(|a| a == "--forwarding");
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/framebuffer".into()),
    );
    fs::create_dir_all(&dir)?;
    let s = MaterializedSurface {
        color_base_bytes: 0,
        depth_base_bytes: 16384,
        width: 160,
        height: 32,
    };
    let c = Context {
        depth: DepthFunc::Always,
        depth_write: true,
        blend: Blend::SrcOver,
    };
    let mut csv=String::from("scenario,quads,pixels,wall_cycles,hits,misses,read_bytes,write_bytes,bytes_per_pixel,maintenance_cycles,input_stalls,serialized_wait,raw_wait,queue_peak,min_commit_interval,forwarded_lanes,forward_fallbacks\n");
    for name in ["independent", "same_address", "sparse", "dirty_rotation"] {
        let qs: Vec<_> = (0..48)
            .map(|i| {
                let (x, y) = if name == "dirty_rotation" {
                    ((i % 20 % 10) * 16, (i % 20 / 10) * 16)
                } else if name == "same_address" {
                    (0, 0)
                } else {
                    ((i % 8) * 2, (i / 8 % 8) * 2)
                };
                Quad {
                    header: Header {
                        x: x as u16,
                        y: y as u8,
                        mask: if name == "sparse" { 1 } else { 15 },
                    },
                    pixels: std::array::from_fn(|lane| Fragment {
                        rgba: [(i * 5) as u8, 77 + lane as u8 * 31, 201, 93],
                        depth: 1000 + i as u16,
                    }),
                }
            })
            .collect();
        let mut bytes = vec![0u8; 32768];
        for pair in bytes[16384..].as_chunks_mut::<2>().0 {
            pair.copy_from_slice(&65535u16.to_le_bytes());
        }
        let runner = if forwarding {
            replay_forwarding
        } else if pipelined {
            replay_pipelined
        } else {
            replay
        };
        let (m, _, trace) = runner(s, c, &qs, Fixture::new(bytes), 0, 100_000)?;
        if m.fault || !m.flush_complete {
            return Err("probe did not flush".into());
        }
        let commits: Vec<_> = trace
            .iter()
            .filter(|t| t.committed.is_some())
            .map(|t| t.cycle)
            .collect();
        let interval = commits.windows(2).map(|w| w[1] - w[0]).min().unwrap_or(0);
        let st = &m.stats;
        writeln!(
            csv,
            "{name},{},{},{},{},{},{},{},{:.3},{},{},{},{},{},{interval},{},{}",
            st.quads,
            st.pixels,
            st.cycles,
            st.hits,
            st.misses,
            st.read_bytes,
            st.write_bytes,
            (st.read_bytes + st.write_bytes) as f64 / st.pixels as f64,
            st.maintenance_stalls,
            st.input_stalls,
            st.serialized_wait,
            st.raw_wait,
            st.queue_peak,
            st.forwarded_lanes,
            st.forward_fallbacks
        )?;
        let mut calendar = String::from(
            "cycle,ce,input,output_row,A,B,request,read,write_accept,completion,commit,stall,published,captured,arithmetic_issue,arithmetic_return,forwarded\n",
        );
        for t in trace {
            let accesses = |port| {
                t.accesses
                    .iter()
                    .filter(|a| a.port == port)
                    .map(|a| {
                        format!(
                            "{}:{}:{}",
                            a.bank,
                            a.address,
                            if a.write { "W" } else { "R" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(";")
            };
            writeln!(
                calendar,
                "{},{},{},{:?},{},{},{:?},{:?},{},{:?},{:?},{},{:?},{:?},{:?},{:?},{:?}",
                t.cycle,
                t.ce,
                t.input_accepted,
                t.output_read,
                accesses('A'),
                accesses('B'),
                t.request
                    .map(|r| format!("{}:{}", r.address_bytes, r.write)),
                t.response.read.map(|r| r.0),
                t.response.write_accepted,
                t.response.complete,
                t.committed.map(|h| format!("{}:{}:{}", h.x, h.y, h.mask)),
                t.stall,
                t.output_published
                    .map(|h| format!("{}:{}:{}", h.x, h.y, h.mask)),
                t.payload_captured
                    .map(|h| format!("{}:{}:{}", h.x, h.y, h.mask)),
                t.arithmetic_issue
                    .map(|(h, l)| format!("{}:{}:{}", h.x, h.y, l)),
                t.arithmetic_return
                    .map(|(h, l)| format!("{}:{}:{}", h.x, h.y, l)),
                t.forwarded.map(|(h, l)| format!("{}:{}:{}", h.x, h.y, l))
            )?;
        }
        fs::write(dir.join(format!("{name}-calendar.csv")), calendar)?;
    }
    fs::write(dir.join("summary.csv"), &csv)?;
    print!("{csv}");
    Ok(())
}
