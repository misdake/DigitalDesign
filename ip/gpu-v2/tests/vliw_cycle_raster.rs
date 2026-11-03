//! Production Raster scheduling against an independent integer coverage set.
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    raster::{Input, Output, Raster, State},
    vertex::Bits,
};
use std::collections::BTreeSet;
#[path = "support/vliw_raster.rs"]
mod fixture;
#[derive(Clone)]
struct Geometry {
    n: [[i64; 3]; 3],
    bounds: [u8; 4],
    empty: bool,
}
impl Geometry {
    fn triangle(v: [[i64; 2]; 3]) -> Self {
        let n = std::array::from_fn(|e| {
            let a = v[(e + 1) % 3];
            let b = v[(e + 2) % 3];
            let nx = a[1] - b[1];
            let ny = b[0] - a[0];
            [2 * nx, 2 * ny, 2 * (a[0] * b[1] - a[1] * b[0]) + nx + ny]
        });
        let axis = |k: usize| {
            let lo = v
                .iter()
                .map(|p| p[k])
                .min()
                .unwrap()
                .clamp(0, if k == 0 { 399 } else { 239 });
            let hi = v
                .iter()
                .map(|p| p[k])
                .max()
                .unwrap()
                .clamp(0, if k == 0 { 399 } else { 239 });
            [(lo / 16) as u8, (hi / 16) as u8]
        };
        let x = axis(0);
        let y = axis(1);
        Self {
            n,
            bounds: [x[0], x[1], y[0], y[1]],
            empty: false,
        }
    }
    fn h(&self, x: i64, y: i64) -> [u64; 3] {
        self.n
            .map(|n| (n[0] * x + n[1] * y + n[2]) as u64 & 0x3fffffff)
    }
    fn covered(&self, x: i64, y: i64) -> bool {
        self.n.into_iter().all(|n| {
            let h = n[0] * x + n[1] * y + n[2];
            h > 0 || (h == 0 && (n[0] > 0 || (n[0] == 0 && n[1] > 0)))
        })
    }
}
struct Edge {
    i: Input,
    o: Output,
    s: State,
    u: u64,
    q: [Bits; 8],
}
struct Run {
    rows: Vec<Edge>,
    covered: usize,
    full: bool,
    partial: bool,
    mailbox_full: bool,
    epochs: usize,
}
fn run(case: usize) -> Run {
    let mut jobs = match case {
        0 => vec![Geometry::triangle([[2, 2], [13, 2], [2, 13]])],
        1 => vec![Geometry::triangle([[-16, -16], [80, -16], [-16, 80]])],
        _ => vec![
            Geometry::triangle([[382, 222], [410, 222], [382, 250]]),
            Geometry::triangle([[500, 500], [510, 500], [500, 510]]),
        ],
    };
    if case == 2 {
        jobs[1].empty = true;
    }
    let mut p = Raster::default();
    let mut rows = Vec::new();
    let mut total = 0;
    let mut full = false;
    let mut partial = false;
    let mut mailbox_full = false;
    let mut epochs = 0;
    for (job, g) in jobs.iter().enumerate() {
        let expected: BTreeSet<_> = (0..240)
            .flat_map(|y| (0..400).map(move |x| (x, y)))
            .filter(|&(x, y)| g.covered(x, y))
            .collect();
        if g.empty {
            assert!(expected.is_empty());
        }
        let mut got = BTreeSet::new();
        let mut tick = 0;
        let mut started = false;
        let mut cursor = 0;
        let mut bound_sent = false;
        let mut imported_at = 0;
        let mut pixel = false;
        let mut lane = 0;
        let mut xy = (0, 0);
        let mut accepted_mask = 0;
        for wall in 0..100000 {
            assert!(
                rows.len() < 100000,
                "combined Raster case exceeded100000 wall edges"
            );
            let reset = wall < 2;
            let ce = wall % 7 > 1;
            let phase = (tick & 3) as u8;
            let start = wall >= 2 && !started;
            let geo_write = started && cursor < 9 && wall % 5 != 0;
            let addr = [0, 4, 8, 1, 5, 9, 2, 6, 10][cursor.min(8)];
            let e = cursor.min(8) / 3;
            let field = cursor.min(8) % 3;
            let i = Input {
                reset,
                ce,
                phase,
                start,
                geo_write,
                geo_address: addr,
                geo_data: Bits::new(g.n[e][field] as u64, 30),
                bbox_done: cursor == 9 && !bound_sent && tick >= imported_at + [0, 40, 120][case],
                bbox_empty: g.empty,
                bbox_xlo: g.bounds[0],
                bbox_xhi: g.bounds[1],
                bbox_ylo: g.bounds[2],
                bbox_yhi: g.bounds[3],
                run_enable: wall % 31 >= 4,
                quad_ready: wall % 13 != 0 && (case != 1 || wall > 2500),
            };
            let o = p.outputs(i);
            rows.push(Edge {
                i,
                o,
                s: p.state.clone(),
                u: p.uop,
                q: [
                    p.work.q,
                    p.mailbox.q,
                    p.scan.q,
                    p.nx.q,
                    p.ny.q,
                    p.steps.wq,
                    p.steps.sq,
                    p.origin.result,
                ],
            });
            let epoch = p.state.w_epoch;
            p.clock(i).unwrap_or_else(|error| {
                panic!(
                    "case{case}/job{job}/wall{wall}/life{}/pc{}/mode{}: {error}",
                    rows.last().unwrap().s.life,
                    rows.last().unwrap().s.w_pc,
                    rows.last().unwrap().s.scan_mode
                )
            });
            if !reset && ce {
                tick += 1;
                if start {
                    started = true;
                }
                if geo_write {
                    cursor += 1;
                    if cursor == 9 {
                        imported_at = tick;
                    }
                }
                if i.bbox_done {
                    bound_sent = true;
                }
                if o.quad_take {
                    accepted_mask = 0;
                    full |= o.full_scan;
                    partial |= !o.full_scan;
                }
                if o.sample_valid {
                    assert_eq!(phase, 0);
                    assert!(!pixel);
                    pixel = true;
                    lane = o.sample_lane;
                    xy = (
                        i64::from(o.quad_x) * 2 + i64::from(lane & 1),
                        i64::from(o.quad_y) * 2 + i64::from(lane >> 1),
                    );
                }
                if pixel && phase < 3 {
                    assert_eq!(
                        o.sample_h.get(30, "Raster sample").unwrap(),
                        g.h(xy.0, xy.1)[usize::from(phase)],
                        "case{case}/job{job}/wall{wall}/xy{xy:?}/edge{phase}"
                    );
                }
                if pixel && phase == 2 {
                    let cover = g.covered(xy.0, xy.1);
                    assert_eq!(o.sample_cover, cover, "case{case}/xy{xy:?}");
                    assert_eq!(o.sample_anchor, cover && accepted_mask == 0);
                    if cover {
                        assert!(got.insert(xy), "duplicate sample {xy:?}");
                        accepted_mask |= 1 << lane;
                    }
                    if lane == 3 {
                        assert!(o.quad_end_valid);
                        assert_eq!(o.quad_mask, accepted_mask);
                    }
                    pixel = false;
                }
                mailbox_full |= p.state.tile_tail.wrapping_sub(p.state.tile_head) & 7 == 2;
                if epoch != p.state.w_epoch {
                    epochs += 1;
                }
                if o.done {
                    assert_eq!(cursor, 9);
                    assert!(bound_sent);
                    assert_eq!(got, expected, "case{case}/job{job}");
                    total += got.len();
                    break;
                }
            }
            assert!(wall != 99999, "Raster did not retire within bound");
        }
    }
    Run {
        rows,
        covered: total,
        full,
        partial,
        mailbox_full,
        epochs,
    }
}
#[test]
fn physical_walker_scanner_emits_exact_coverage_and_h_words() {
    for case in 0..3 {
        let run = run(case);
        println!(
            "RASTER_CASE {case} edges={} covered={} epochs={} full={} partial={} mailbox_full={}",
            run.rows.len(),
            run.covered,
            run.epochs,
            run.full,
            run.partial,
            run.mailbox_full
        );
        assert!(run.covered > 0);
        assert!(run.epochs > 0);
        assert!(run.partial);
        if case == 1 {
            assert!(run.full);
            assert!(run.mailbox_full);
        }
        assert!(run.rows.iter().any(|r| !r.i.ce && r.o.busy));
    }
}

#[test]
#[ignore = "three bounded actual recursive walker/scanner RAM/DSP RTL profiles"]
fn every_raster_edge_matches_production() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "raster-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    for case in 0..3 {
        let run = run(case);
        let pins = fixture::fields(&run.rows[0]);
        vertex_support::rtl::replay(
            &root,
            &format!("case{case}"),
            run.rows.iter().map(fixture::fields).collect(),
            fixture::production(&pins),
            false,
            None,
        );
    }
    println!("RASTER_CHECKPOINT {}", root.display());
}

#[test]
#[ignore = "compiled RF age/branch epoch/mailbox publication mutations and geometry monitor"]
fn raster_negative_controls_and_geometry_order_are_sensitive() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "raster-negative-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    let mut run = run(0);
    let pins = fixture::fields(&run.rows[0]);
    for (name,module,old,new) in [
        ("extra_RF_Q","rp_raster_rf ","always @(posedge clk)if(reset)q<=0;else if(ce&&re)q<=raw;","reg[WIDTH-1:0]delayed_q;always @(posedge clk)if(reset)begin q<=0;delayed_q<=0;end else if(ce&&re)begin delayed_q<=raw;q<=delayed_q;end"),
        ("ignore_epoch","rc_raster #","wire cancelled=r_valid&&r_epoch!=w_epoch;","wire cancelled=1'b0;"),
        ("upper_publishes_tail","rc_raster #","wire tile_publish=w_decide&&r_uop[27]&&terminal_tile;","wire tile_publish=w_decide&&!r_uop[27]&&terminal_tile;")
    ] {let mut source=fixture::production(&pins);let start=source.find(&format!("module {module}")).unwrap();
        let end=start+source[start..].find("endmodule").unwrap();let body=&source[start..end];assert_eq!(body.matches(old).count(),1);
        let body=body.replacen(old,new,1);source.replace_range(start..end,&body);
        vertex_support::rtl::replay(&root,name,run.rows.iter().map(fixture::fields).collect(),source,true,None);
    }
    let n = run
        .rows
        .iter()
        .position(|r| r.i.ce && r.i.geo_write)
        .unwrap();
    run.rows.truncate(n + 1);
    run.rows[n].i.geo_address = 4;
    let mut invalid = Raster::default();
    for edge in &run.rows[..n] {
        invalid.clock(edge.i).unwrap();
    }
    let error = invalid.clock(run.rows[n].i).unwrap_err();
    assert_eq!(error.module, "raster");
    assert_eq!(error.rule, "final geometry write order");
    vertex_support::rtl::replay(
        &root,
        "bad_geometry_order",
        run.rows.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        false,
        Some("最终行几何写入次序错误"),
    );
    println!("RASTER_NEGATIVE {}", root.display());
}
