//! Selected Final/ROP share one physical multiplier Q, with independent integer checks.
#[path = "../support/vliw_pixel_math.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{pixel_math::*, vertex::Bits};
use std::collections::VecDeque;

struct Edge {
    fi: FinalInput,
    fo: FinalOutput,
    ri: RopInput,
    ro: RopOutput,
    shared: SharedBank,
    final_state: FinalState,
}
fn b(v: u64, width: u32) -> Bits {
    Bits::new(v, width)
}
#[derive(Clone, Copy)]
struct Loan {
    tint: u32,
    texture: u32,
    specular: u32,
    g: u16,
    h: u16,
}
impl Loan {
    fn new(n: usize) -> Self {
        Self {
            tint: (n as u32).wrapping_mul(0x1739b7) & 0xffffff,
            texture: (n as u32).wrapping_mul(0x29e57d) ^ 0xffffff,
            specular: (n as u32).wrapping_mul(0x57c31b) & 0xffffff,
            g: [0, 1, 127, 128, 255, 256, 257, 511][n & 7],
            h: [0, 1, 127, 128, 255, 256][n % 6],
        }
    }
    fn reference(self) -> u64 {
        let mut rgb = 0;
        for c in 0..3 {
            let a = (self.tint >> (8 * c)) & 255;
            let t = (self.texture >> (8 * c)) & 255;
            let spec = (self.specular >> (8 * c)) & 255;
            let modulated = (a * t * 2 + 255) / 510;
            let diffuse = modulated * u32::from(self.g) / 256;
            let specular = spec * u32::from(self.h) / 256;
            rgb |= u64::from((diffuse + specular).min(255)) << (8 * c);
        }
        rgb
    }
}
#[derive(Clone, Copy)]
struct Pixel {
    rgb: u32,
    alpha: u8,
    z: u16,
    old_z: u16,
    old_color: u16,
    function: u8,
    covered: bool,
    blend: bool,
    dw: bool,
}
impl Pixel {
    fn new(n: usize) -> Self {
        Self {
            rgb: (n as u32).wrapping_mul(0x6193a7) & 0xffffff,
            alpha: [0, 1, 2, 5, 6, 127, 128, 129, 254, 255][n % 10],
            z: (n as u16).wrapping_mul(7919),
            old_z: if n.is_multiple_of(3) {
                (n as u16).wrapping_mul(7919)
            } else {
                (n as u16).wrapping_mul(5347)
            },
            old_color: (n as u16).wrapping_mul(18283),
            function: (n & 7) as u8,
            covered: !n.is_multiple_of(13),
            blend: n % 3 != 1,
            dw: !n.is_multiple_of(5),
        }
    }
    fn reference(self, texture_alpha: u8) -> (u64, u64, u64, u64) {
        let depth_pass = match self.function {
            0 => false,
            1 => self.z < self.old_z,
            2 => self.z == self.old_z,
            3 => self.z <= self.old_z,
            4 => self.z > self.old_z,
            5 => self.z != self.old_z,
            6 => self.z >= self.old_z,
            _ => true,
        } && self.covered;
        let r = u32::from(self.old_color >> 11);
        let g = u32::from((self.old_color >> 5) & 63);
        let bl = u32::from(self.old_color & 31);
        let destination = [
            (r << 3) | (r >> 2),
            (g << 2) | (g >> 4),
            (bl << 3) | (bl >> 2),
        ];
        let alpha = if self.blend {
            ((u32::from(self.alpha) * u32::from(texture_alpha) + 127) / 255 + 2) / 4
        } else {
            64
        };
        let mut rgb565 = 0;
        for (c, old) in destination.into_iter().enumerate() {
            let source = (self.rgb >> (c * 8)) & 255;
            let channel = (source * alpha + old * (64 - alpha) + 32) / 64;
            let max = if c == 1 { 63 } else { 31 };
            let quantized = (channel * max * 2 + 255) / 510;
            rgb565 |= u64::from(quantized) << [11, 5, 0][c];
        }
        (
            rgb565,
            self.z.into(),
            u64::from(depth_pass),
            u64::from(depth_pass && self.dw),
        )
    }
}
fn run(case: usize) -> Vec<Edge> {
    let mut final_stage = Final::default();
    let mut rop = Rop::default();
    let mut shared = SharedBank::default();
    let mut phase = false;
    let mut loan = Loan::new(1);
    let mut offered = 0;
    let mut pixels = 0;
    let mut live_old_depth = 0;
    let mut final_queue = VecDeque::new();
    let mut rop_queue = VecDeque::new();
    let mut enabled = 0;
    let mut rows = Vec::new();
    let mut old_full = false;
    let mut direct = false;
    let mut queued = false;
    let mut saw_abort_ce0 = false;
    for wall in 0..5000usize {
        let reset = wall == 0 || wall == 1900 || case == 1 && wall == 3100;
        let abort = case == 1 && wall == 3000;
        let ce = wall % 17 != 8 && wall % 17 != 9 && !(case == 1 && wall == 3000);
        let allow = wall % 29 != 21;
        let valid = wall < 4400 && wall % 7 != 3;
        if !phase && valid {
            loan = Loan::new(offered + 1);
        }
        let fi = FinalInput {
            reset,
            ce,
            proposal_ce: true,
            commit_allow: allow,
            abort,
            shade_phase: phase,
            in_valid: valid,
            tint: b(loan.tint.into(), 24),
            texture: b(loan.texture.into(), 24),
            texture_alpha: b(
                if case >= 2 {
                    (offered as u64 * 71) & 255
                } else {
                    255
                },
                8,
            ),
            specular: b(loan.specular.into(), 24),
            g: b(loan.g.into(), 9),
            h: b(loan.h.into(), 9),
            shared_spec_product: shared.spec_product(),
            out_ready: wall % 71 >= 32 && wall % 11 != 4,
        };
        let fo = final_stage.outputs(fi);
        let p = Pixel::new(pixels + 1);
        let texture_alpha = if case >= 2 {
            (pixels as u64 * 193) & 255
        } else {
            255
        };
        let ri = RopInput {
            reset,
            ce: fo.advance,
            shade_phase: phase,
            valid: phase && wall < 4400 && wall % 9 != 5,
            blend: p.blend,
            depth_func: p.function,
            depth_write: p.dw,
            covered: b(u64::from(p.covered), 1),
            old_color: b(p.old_color.into(), 16),
            // D supplies a deliberately wrong value. The correct live RAM Q is
            // supplied on its following C edge, when the comparison is made.
            old_depth: b(
                if phase {
                    u64::from(!p.old_z)
                } else {
                    live_old_depth
                },
                16,
            ),
            rgb: b(p.rgb.into(), 24),
            alpha: b(p.alpha.into(), 8),
            // Only C owns this operand. Poison D to verify the DSP input hold.
            texture_alpha: b(
                if phase {
                    texture_alpha ^ 0x5a
                } else {
                    texture_alpha
                },
                8,
            ),
            texture_capture: !phase,
            depth: b(p.z.into(), 16),
            shared_rop_product: shared.rop_product(),
        };
        let ro = rop.outputs(ri);
        rows.push(Edge {
            fi,
            fo,
            ri,
            ro,
            shared: shared.clone(),
            final_state: final_stage.state.clone(),
        });
        if reset {
            final_queue.clear();
            rop_queue.clear();
            enabled = 0;
        } else {
            if fo.out_valid {
                let &(value, alpha, ready_edge) =
                    final_queue.front().expect("unowned Final result");
                assert!(enabled >= ready_edge);
                assert_eq!(
                    fo.texture_alpha.get(8, "Final alpha").unwrap(),
                    alpha,
                    "alpha slot wall{wall}"
                );
                assert_eq!(fo.rgb.get(24, "Final result").unwrap(), value, "wall{wall}");
            }
            if fo.advance && fo.out_fire {
                final_queue.pop_front();
            }
            if fo.in_accept {
                final_queue.push_back((loan.reference(), fi.texture_alpha.value, enabled + 2));
                offered += 1;
            }
            if ro.valid {
                let &(value, ready_edge) = rop_queue.front().expect("unowned ROP result");
                assert_eq!(enabled, ready_edge);
                assert_eq!(
                    (
                        ro.color.get(16, "ROP color").unwrap(),
                        ro.depth.get(16, "ROP depth").unwrap(),
                        ro.color_written.get(1, "ROP color gate").unwrap(),
                        ro.depth_written.get(1, "ROP depth gate").unwrap()
                    ),
                    value,
                    "wall{wall}"
                );
                if ri.ce {
                    rop_queue.pop_front();
                }
            }
            if ri.ce && ri.valid {
                rop_queue.push_back((p.reference(texture_alpha as u8), enabled + 4));
                pixels += 1;
                live_old_depth = p.old_z.into();
            }
            old_full |= fo.credits == 2 && fi.out_ready && !phase && !fo.in_accept;
            direct |= fo.out_fire && final_stage.state.q_count == 0;
            queued |= fo.out_fire && final_stage.state.q_count != 0;
            saw_abort_ce0 |= abort && !ce && fo.fault;
        }
        let si = SharedInput {
            rop_ce: ro.shared_rop_ce,
            rop_a: ro.shared_rop_a,
            rop_b: ro.shared_rop_b,
            spec_ce: fo.shared_spec_ce,
            spec_a: fo.shared_spec_a,
            spec_b: fo.shared_spec_b,
        };
        assert!(!(si.rop_ce && si.spec_ce), "shared owner conflict");
        final_stage.clock(fi).unwrap();
        rop.clock(ri);
        shared.clock(si);
        if reset {
            phase = false;
        } else if fo.advance {
            phase = !phase;
            enabled += 1;
        }
    }
    assert!(offered > 500 && pixels > 500 && final_queue.is_empty() && rop_queue.is_empty());
    assert!(old_full && direct && queued);
    assert!(case != 1 || saw_abort_ce0);
    rows
}
#[test]
fn final_and_live_depth_rop_match_independent_integer_math() {
    for case in 0..3 {
        run(case);
    }
}

fn root(label: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    root
}
#[test]
#[ignore = "two bounded selected Final/ROP/shared-DSP production profiles"]
fn every_shared_final_rop_edge_matches_production() {
    let root = root("pixel-math");
    for case in 0..3 {
        let rows = run(case);
        let pins = fixture::fields(&rows[0]);
        vertex_support::rtl::replay(
            &root,
            &format!("case{case}"),
            rows.iter().map(fixture::fields).collect(),
            fixture::production(&pins),
            false,
            None,
        );
    }
    println!("PIXEL_MATH_CHECKPOINT {}", root.display());
}
#[test]
#[ignore = "three actual arithmetic/credit/age mutations and the original H monitor"]
fn shared_final_rop_negative_controls() {
    let root = root("pixel-math-negative");
    let mut rows = run(0);
    let pins = fixture::fields(&rows[0]);
    for (name,old,new) in [
        ("spec_truncation","if(phase==SPEC)sh<=spec_product[15:8];","if(phase==SPEC)sh<=spec_product[16:9];"),
        ("alpha_q6","(alpha_tail>=11'd383)","(alpha_tail>=11'd511)"),
        ("texture_alpha_live_D","alpha_result<=alpha_texture*src_a;","alpha_result<=src_texture_alpha*src_a;"),
        ("live_depth","wire[15:0]compared_old_depth=old_depth;","reg[15:0]old_depth_delayed;always@(posedge clk)if(ce)old_depth_delayed<=old_depth;wire[15:0]compared_old_depth=old_depth_delayed;"),
    ] {
        let source=fixture::production(&pins); assert_eq!(source.matches(old).count(),1,"{name}");
        vertex_support::rtl::replay(&root,name,rows.iter().map(fixture::fields).collect(),source.replacen(old,new,1),true,None);
    }
    let at = rows
        .iter()
        .position(|e| !e.fi.reset && e.fo.proposal_ready && e.fi.in_valid)
        .unwrap();
    rows.truncate(at + 1);
    rows[at].fi.h = b(257, 9);
    let mut model = Final::default();
    for row in &rows[..at] {
        model.clock(row.fi).unwrap();
    }
    rows[at].fo = model.outputs(rows[at].fi);
    assert_eq!(
        model.clock(rows[at].fi).unwrap_err().rule,
        "GPU contract: preflight_bad"
    );
    vertex_support::rtl::replay(
        &root,
        "bad_h",
        rows.iter().map(fixture::fields).collect(),
        fixture::production(&pins),
        false,
        Some("GPU contract: preflight_bad"),
    );
    println!("PIXEL_MATH_NEGATIVE_CHECKPOINT {}", root.display());
}
