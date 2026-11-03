//! Separate controlled-core and real private preparation RAM/DSP fixtures.
//! Raster and runtime context/compute are qualified by their own compositions.
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "support/vliw_triangle/mod.rs"]
mod triangle_support;
#[path = "support/vliw_prepare_unit.rs"]
mod unit_fixture;
#[allow(dead_code)]
#[path = "support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    prepare::{Control, Input, Output, Registers},
    vertex::Bits,
};
use vertex_support::rtl::{self, Pin};

fn signed(v: u64, n: u32) -> i64 {
    ((v << (64 - n)) as i64) >> (64 - n)
}
use gpu_v2::system::vliw_cycle::prepare::{Params, Prepare, PrepareInput, PrepareOutput};
struct UnitEdge {
    i: PrepareInput,
    o: PrepareOutput,
    p: Prepare,
}
fn unit_run(shape: u8, rgb: bool) -> (u32, [u64; 16], Vec<UnitEdge>) {
    unit_run_depth(shape, rgb, 0)
}
fn unit_run_depth(shape: u8, rgb: bool, depth_shift: u8) -> (u32, [u64; 16], Vec<UnitEdge>) {
    let data = triangle_support::data(triangle_support::Case {
        shape,
        rgb,
        ..Default::default()
    });
    let (descriptor, words) = data.packets[0];
    let mut p = Prepare::default();
    let mut cursor = 0;
    let mut rows = Vec::new();
    for wall in 0..100000 {
        let i = PrepareInput {
            reset: wall < 2,
            ce: wall % 7 > 1,
            start: wall == 2,
            execute_grant: wall > 500,
            descriptor,
            raw_valid: wall > 2 && cursor < 16 && wall % 5 != 0,
            raw_index: cursor as u8,
            raw_data: Bits::new(words[cursor.min(15)], 54),
            params: Params {
                far_z: (31 << 16) << depth_shift,
                depth_shift,
                tex_u: 6,
                tex_v: 6,
                max_mip: 6,
                bias: -3,
            },
            ..Default::default()
        };
        let o = p.outputs(i);
        rows.push(UnitEdge { i, o, p: p.clone() });
        if i.ce && i.raw_valid && o.raw_ready {
            cursor += 1;
        }
        p.clock(i)
            .unwrap_or_else(|e| panic!("shape{shape}/rgb{rgb}/wall{wall}/op{}: {e}", o.operation));
        if o.done && i.ce {
            assert_eq!(cursor, 16);
            return (descriptor, words, rows);
        }
    }
    panic!("private prepare exceeded100000 edges: shape{shape}/rgb{rgb}");
}

#[test]
fn depth_shift_preserves_prepare_calendar_storage_and_far_completion() {
    let (_, _, baseline) = unit_run_depth(0, false, 0);
    for shift in 1..=3 {
        let (_, _, candidate) = unit_run_depth(0, false, shift);
        assert_eq!(baseline.len(), candidate.len());
        for (a, b) in baseline.iter().zip(&candidate) {
            assert_eq!(
                (a.o.done, a.o.cfg_apply, a.o.param_valid, a.o.bound_valid),
                (b.o.done, b.o.cfg_apply, b.o.param_valid, b.o.bound_valid)
            );
            if a.o.cfg_apply {
                assert_eq!(b.o.z_bias, a.o.z_bias - shift as i8);
                assert_eq!(b.o.far_z, a.o.far_z << shift);
            }
            assert_eq!(a.o.param_data, b.o.param_data);
        }
    }
}
#[test]
fn private_prepare_reduces_t16_and_publishes_packed_parameters() {
    for shape in [0, 3, 9, 10, 11] {
        for rgb in [false, true] {
            let (descriptor, words, rows) = unit_run(shape, rgb);
            let p = &rows.last().unwrap().p;
            let params: Vec<_> = rows
                .iter()
                .filter(|e| e.i.ce && e.o.param_valid)
                .map(|e| {
                    (
                        usize::from(e.o.param_index) * 2 + usize::from(e.o.param_bank),
                        e.o.param_data.get(51, "parameter").unwrap(),
                    )
                })
                .collect();
            for edge in 0..3 {
                let raw = [
                    signed(words[3 * edge], 27),
                    signed(words[3 * edge + 1], 27),
                    signed(words[3 * edge + 2], 38),
                ];
                let exp = raw
                    .iter()
                    .enumerate()
                    .map(|(k, n)| {
                        (64 - n.unsigned_abs().leading_zeros()).saturating_sub(if k == 2 {
                            27
                        } else {
                            17
                        })
                    })
                    .max()
                    .unwrap();
                for (k, n) in raw.into_iter().enumerate() {
                    let mut canonical = n / (1i64 << exp);
                    if descriptor & (1 << 18) != 0 {
                        canonical = -canonical;
                    }
                    let got = p.storage.b[6 + 3 * k + edge]
                        .get(36, "canonical PB")
                        .unwrap();
                    assert_eq!(
                        signed(got, 36),
                        canonical,
                        "shape{shape}/edge{edge}/field{k}"
                    );
                }
            }
            if p.control.regs.empty {
                let covered = (0..240).any(|y| {
                    (0..400).any(|x| {
                        (0..3).all(|edge| {
                            let nx = signed(p.storage.b[6 + edge].value, 36);
                            let ny = signed(p.storage.b[9 + edge].value, 36);
                            let c = signed(p.storage.b[12 + edge].value, 36);
                            let h = 2 * nx * x + 2 * ny * y + c;
                            h > 0 || (h == 0 && (nx > 0 || (nx == 0 && ny > 0)))
                        })
                    })
                });
                assert!(
                    !covered,
                    "empty bounds discarded a covered pixel: shape{shape}"
                );
                assert!(rows.iter().all(|e| !e.o.cfg_apply));
                assert_eq!(
                    params.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
                    (0..params.len()).collect::<Vec<_>>()
                );
            } else {
                assert_eq!(
                    params.len(),
                    6,
                    "shape{shape}/rgb{rgb}: incomplete nonempty context"
                );
                assert_eq!(rows.iter().filter(|e| e.i.ce && e.o.cfg_apply).count(), 1);
            }
            let source: Vec<[i64; 6]> = (0..3)
                .map(|v| {
                    let n = words[10 + v * 2];
                    let a = words[11 + v * 2];
                    [
                        signed(n, 12),
                        signed(n >> 12, 12),
                        signed(n >> 24, 12),
                        if rgb {
                            (a & 255) as i64
                        } else {
                            (a & 65535) as i64
                        },
                        if rgb {
                            ((a >> 8) & 255) as i64
                        } else {
                            ((a >> 16) & 65535) as i64
                        },
                        if rgb { ((a >> 16) & 255) as i64 } else { 0 },
                    ]
                })
                .collect();
            for (index, value) in params {
                let base = source[2][index];
                let d0 = source[0][index] - base;
                let d1 = source[1][index] - base;
                let expected = ((base as u64 & 0x1ffff) << 34)
                    | ((d1 as u64 & 0x1ffff) << 17)
                    | (d0 as u64 & 0x1ffff);
                assert_eq!(value, expected, "shape{shape}/rgb{rgb}/component{index}");
            }
        }
    }
}

#[test]
#[ignore = "bounded actual private preparation with real RAM and live-C DSP"]
fn private_prepare_storage_and_dsp_match_production_edges() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "prepare-unit-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    for (shape, rgb, shift) in [
        (0, false, 0),
        (0, true, 1),
        (3, false, 2),
        (9, false, 3),
        (10, true, 0),
    ] {
        let (_, _, rows) = unit_run_depth(shape, rgb, shift);
        let pins = unit_fixture::fields(&rows[0]);
        rtl::replay(
            &root,
            &format!("shape{shape}-rgb{rgb}-depth{shift}"),
            rows.iter().map(unit_fixture::fields).collect(),
            unit_fixture::production(&pins),
            false,
            None,
        );
    }
    println!("PREPARE_UNIT_CHECKPOINT {}", root.display());
}

#[test]
#[ignore = "compiled private preparation DSP/storage/control mutations"]
fn private_prepare_negative_controls_are_sensitive() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "prepare-unit-mutants-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    for (name, shape, old, new) in [
        ("registered_C", 9, "r<=av*br+c;", "r<=av*br+cr;"),
        (
            "signed_PA",
            0,
            "memory[{3'd0,wa}]<={1'b0,wd}",
            "memory[{3'd0,wa}]<={wd[16],wd}",
        ),
        (
            "early_context",
            0,
            "assign cfg_apply=context_ready&&execute_grant&&!bound_empty;",
            "assign cfg_apply=context_ready&&!bound_empty;",
        ),
    ] {
        let (_, _, rows) = unit_run(shape, false);
        let pins = unit_fixture::fields(&rows[0]);
        let source = unit_fixture::production(&pins);
        assert_eq!(source.matches(old).count(), 1);
        rtl::replay(
            &root,
            name,
            rows.iter().map(unit_fixture::fields).collect(),
            source.replacen(old, new, 1),
            true,
            None,
        );
    }
    println!("PREPARE_UNIT_MUTANTS {}", root.display());
}

struct Edge {
    i: Input,
    o: Output,
    r: Registers,
}
fn run(rgb: bool, search: bool) -> Vec<Edge> {
    let mut core = Control::default();
    let mut rows = Vec::new();
    let mut commit_age = 0;
    let mut was_active = false;
    for wall in 0..100000 {
        let probe = core.outputs(Input::default());
        let result = if search
            && ((matches!(probe.operation, 16 | 17))
                || (probe.operation == 20 && core.regs.search_bit & 1 == 0))
        {
            -1i64
        } else {
            4
        };
        let b = if search && probe.operation == 1 {
            -3i64
        } else {
            4
        };
        let i = Input {
            reset: wall < 2,
            ce: wall % 11 > 2,
            start: wall == 3,
            input_take: wall % 5 != 0,
            normalize_wait: commit_age < 9,
            rgb,
            seed_sign: 0b101010,
            shifts: [0, 7, 31],
            b: Bits::new(b as u64, 36),
            result: Bits::new(result as u64, 54),
        };
        rows.push(Edge {
            i,
            o: core.outputs(i),
            r: core.regs.clone(),
        });
        core.clock(i).unwrap();
        if i.ce && probe.operation == 23 {
            commit_age += 1;
        }
        was_active |= core.regs.active;
        if was_active && !core.regs.active {
            return rows;
        }
    }
    panic!("prepare control exceeded100000 edges: {:?}", core.regs);
}
fn fields(e: &Edge) -> Vec<Pin> {
    let i = e.i;
    let o = e.o;
    let r = &e.r;
    let mut p = Vec::new();
    macro_rules! pin {
        ($n:literal,$w:expr,$v:expr,$out:expr) => {
            p.push(Pin {
                name: $n.into(),
                width: $w,
                value: u128::from($v),
                known: (1u128 << $w) - 1,
                output: $out,
            });
        };
    }
    pin!("reset", 1, i.reset, false);
    pin!("ce", 1, i.ce, false);
    pin!("start", 1, i.start, false);
    pin!("input_take", 1, i.input_take, false);
    pin!("normalize_wait", 1, i.normalize_wait, false);
    pin!("rgb", 1, i.rgb, false);
    pin!("seed_sign", 6, i.seed_sign, false);
    pin!("shift0", 5, i.shifts[0], false);
    pin!("shift1", 5, i.shifts[1], false);
    pin!("shift2", 5, i.shifts[2], false);
    pin!("bq", 36, i.b.value, false);
    pin!("result", 54, i.result.value, false);
    pin!("operation", 6, o.operation, true);
    pin!("import_row", 4, o.import_row, true);
    pin!("import_kind", 3, o.import_kind, true);
    pin!("a_re", 1, o.a_read, true);
    pin!("a_ra", 7, o.a_address, true);
    pin!("b_re", 1, o.b_read, true);
    pin!("b_ra", 4, o.b_address, true);
    pin!("b_we", 1, o.b_write, true);
    pin!("b_wa", 4, o.b_write_address, true);
    pin!("b_wd", 36, o.b_data.value, true);
    pin!("c_re", 1, o.c_read, true);
    pin!("c_ra", 4, o.c_address, true);
    pin!("c_we", 1, o.c_write, true);
    pin!("c_wa", 4, o.c_write_address, true);
    pin!("c_wd", 54, o.c_data.value, true);
    pin!("wide_valid", 1, o.wide_valid, true);
    pin!("wide_asign", 1, o.wide_signed, true);
    pin!("active", 1, o.active, true);
    pin!("done", 1, o.done, true);
    pin!("bound_valid", 1, o.bound_valid, true);
    pin!("bound_empty", 1, o.bound_empty, true);
    pin!("xlo", 5, o.xlo, true);
    pin!("xhi", 5, o.xhi, true);
    pin!("ylo", 4, o.ylo, true);
    pin!("yhi", 4, o.yhi, true);
    pin!("attr_send", 1, o.attr_send, true);
    pin!("attr_index", 2, o.attr_index, true);
    pin!("attr_bank", 1, o.attr_bank, true);
    pin!("attr0_take", 1, o.attr0_take, true);
    pin!("attr1_take", 1, o.attr1_take, true);
    pin!("obs_u", 36, r.u, true);
    pin!("obs_issued_op", 6, r.issued_op, true);
    pin!("obs_axis", 1, r.axis, true);
    pin!("obs_index", 2, r.index, true);
    pin!("obs_d_kind", 2, r.d_kind, true);
    pin!("obs_aux_kind", 2, r.aux_kind, true);
    pin!("obs_sign0", 1, r.sign0, true);
    pin!("obs_nonzero0", 1, r.nonzero0, true);
    pin!("obs_empty", 1, r.empty, true);
    pin!("obs_eligible", 2, r.eligible, true);
    pin!("obs_search_main", 1, r.search_main, true);
    pin!("obs_dual_search", 1, r.dual_search, true);
    pin!("obs_aux_pending", 1, r.aux_pending, true);
    pin!("obs_search_bit", 3, r.search_bit, true);
    pin!("obs_prefix", 5, r.prefix, true);
    pin!("obs_aux_prefix", 4, r.aux_prefix, true);
    pin!("obs_rom_address", 9, o.rom_address, true);
    pin!("obs_rom_read", 1, o.rom_read, true);
    p
}
fn production(p: &[Pin]) -> String {
    let mut s = String::from("`define PORTABLE\n");
    s.push_str(include_str!("../src/frontend/vliw/prepare_program.v"));
    s.push_str(include_str!("../src/frontend/vliw/prepare_core.v"));
    s.push_str("\nmodule g2a(input clk");
    use std::fmt::Write as _;
    for f in p {
        write!(
            s,
            ",{}[{}:0]{}",
            if f.output { "output" } else { "input" },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    s.push_str(");rp_prepare_core dut(.clk(clk),.cq(54'd0)");
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for f in p.iter().filter(|f| f.name.starts_with("obs_")) {
        let name = f.name.strip_prefix("obs_").unwrap();
        let net = match name {
            "rom_address" => "program_address",
            "rom_read" => "program_read",
            _ => name,
        };
        writeln!(s, "assign {}=dut.{net};", f.name).unwrap();
    }
    s.push_str("endmodule\n");
    s
}
#[test]
fn real_prepare_rom_control_finishes_with_ce_and_input_stalls() {
    for rgb in [false, true] {
        for search in [false, true] {
            let rows = run(rgb, search);
            assert!(rows.iter().any(|e| e.o.operation >= 48 && !e.i.input_take));
            assert!(rows.iter().any(|e| !e.i.ce && e.o.active));
            assert!(rows.iter().any(|e| e.o.bound_valid));
            assert!(rows.iter().any(|e| e.o.operation == 23));
            assert!(rows
                .iter()
                .any(|e| e.o.operation == 23 && e.i.normalize_wait && e.i.ce));
            if search {
                assert!(rows.iter().any(|e| e.o.operation == 20));
            }
        }
    }
}
#[test]
#[ignore = "bounded actual preparation control/ROM RTL; controlled PB/DSP peers"]
fn preparation_control_matches_every_production_edge() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "prepare-control-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    for rgb in [false, true] {
        for search in [false, true] {
            let run = run(rgb, search);
            let pins = fields(&run[0]);
            rtl::replay(
                &root,
                &format!("rgb{rgb}-search{search}"),
                run.iter().map(fields).collect(),
                production(&pins),
                false,
                None,
            );
        }
    }
    let run = run(false, true);
    let pins = fields(&run[0]);
    for (name, old, new) in [
        (
            "rom_owner",
            "issued_op<=program_address[5:0]",
            "issued_op<=next_pc",
        ),
        (
            "early_commit",
            "23:next_pc=normalize_wait?6'd23:6'd24",
            "23:next_pc=6'd24",
        ),
        (
            "search_credit",
            "if(search_bit!=0)search_bit<=search_bit-1'b1",
            "if(search_bit>1)search_bit<=search_bit-1'b1",
        ),
    ] {
        let source = production(&pins);
        assert_eq!(source.matches(old).count(), 1);
        rtl::replay(
            &root,
            name,
            run.iter().map(fields).collect(),
            source.replacen(old, new, 1),
            true,
            None,
        );
    }
    println!("PREPARE_CONTROL_CHECKPOINT {}", root.display());
}
