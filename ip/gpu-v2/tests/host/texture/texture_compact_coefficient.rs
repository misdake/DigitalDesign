//! Selected compact executor checked against independent generic execution.
use gpu_v2::texture::emu::{
    coefficient::{
        self as c, compact_stage::CompactCoefficientEmu, fixed_stage::FixedCoefficientEmu,
    },
    compact, coordinate,
};

fn fixture(n: u8, uv: [u16; 2], nearest: bool, p1: u16, key: u8) -> c::Input {
    let p1 = if nearest { 0 } else { p1 };
    let physical = n.max(1);
    let mask = (1i32 << physical) - 1;
    let axes = uv.map(|v| {
        let fine = ((i32::from(v) << 2) >> (10 - physical)) - if nearest { 0 } else { 128 };
        let coarse = if n > 1 { (fine - 128) >> 1 } else { fine };
        [
            (fine, mask),
            (coarse, if n > 1 { (1 << (n - 1)) - 1 } else { mask }),
        ]
    });
    c::Input {
        parents: [511 - p1, p1],
        nearest,
        fractions: std::array::from_fn(|p| std::array::from_fn(|a| axes[a][p].0 as u8)),
        metadata: c::Metadata {
            coordinates: std::array::from_fn(|p| {
                let (x, m) = axes[0][p];
                let (y, _) = axes[1][p];
                [
                    (x >> 8) & m,
                    ((x >> 8) + 1) & m,
                    (y >> 8) & m,
                    ((y >> 8) + 1) & m,
                ]
                .map(|v| v as u16)
            }),
            levels: [n, n.saturating_sub(1)],
            slot: key & 15,
            key,
            last_fine: p1 == 0,
        },
    }
}

#[test]
fn compact_cohorts_match_generic_old_state_and_nearest_raw_borrow() {
    let mut vectors = vec![
        fixture(2, [12288, 40960], true, 0, 63),
        fixture(8, [255, 512], false, 256, 1),
        fixture(8, [639, 128], false, 383, 2),
    ];
    for n in 0..=10 {
        for nearest in [false, true] {
            for p1 in 0_u16..=511 {
                vectors.push(fixture(
                    n,
                    [(p1 * 127).wrapping_add(255), (p1 * 113).wrapping_add(128)],
                    nearest,
                    p1,
                    (p1 % 64) as u8,
                ));
            }
        }
    }
    let mut a = CompactCoefficientEmu::new(200_000).unwrap();
    let mut b = FixedCoefficientEmu::new(200_000).unwrap();
    let mut sent = 0;
    let mut got = 0;
    let mut frozen = 0;
    for wall in 0..200_000 {
        let t = c::Tick {
            ce: wall % 19 != 3 && wall % 19 != 4,
            input: vectors.get(sent).copied(),
            output_ready: wall % 41 < 30,
            work_available: if wall % 31 == 0 { 0 } else { 16 },
        };
        let sa = a.tick(t).unwrap();
        let sb = b.tick(t).unwrap();
        assert_eq!(
            (
                sa.input_ready,
                sa.accepted,
                sa.consumed,
                sa.output,
                sa.events
            ),
            (
                sb.input_ready,
                sb.accepted,
                sb.consumed,
                sb.output,
                sb.events
            ),
            "wall{wall}"
        );
        assert_eq!(
            (
                sa.ready_access.write,
                sa.ready_access.publish,
                sa.ready_access.read_address
            ),
            (
                sb.ready_access.write,
                sb.ready_access.publish,
                sb.ready_access.read_address
            )
        );
        assert_eq!(
            (
                sa.snapshot.valid,
                sa.snapshot.phase,
                sa.snapshot.queued,
                sa.snapshot.cohorts
            ),
            (
                sb.snapshot.valid,
                sb.snapshot.phase,
                sb.snapshot.queued,
                sb.snapshot.cohorts
            )
        );
        sent += usize::from(sa.accepted);
        got += usize::from(sa.consumed);
        frozen += usize::from(!t.ce);
        if sent == vectors.len() && a.idle() {
            break;
        }
    }
    assert_eq!((sent, got), (vectors.len(), vectors.len()));
    assert!(frozen > 0);
    let i = vectors[0];
    let m = compact::Metadata::capture(i).unwrap();
    assert_eq!(m.base(1), [0, 1]);
    assert_eq!((m.0 >> 20) & 3, 0);
    println!("COMPACT coefficient jobs={sent} CE0={frozen} generic output/DSP/event agreement");
}

#[test]
fn compact_checked_domain_preserves_independent_generic_acceptance() {
    let mut i = fixture(8, [255, 512], false, 256, 3);
    i.metadata.coordinates[0][1] = 91;
    let mut generic = FixedCoefficientEmu::new(100).unwrap();
    assert!(
        generic
            .tick(c::Tick {
                input: Some(i),
                ..Default::default()
            })
            .unwrap()
            .accepted
    );
    assert_eq!(compact::Metadata::capture(i), Err(c::Fault::Input));
    let mut selected = CompactCoefficientEmu::new(100).unwrap();
    assert_eq!(
        selected
            .tick(c::Tick {
                input: Some(i),
                ..Default::default()
            })
            .unwrap_err(),
        c::Fault::Input
    );
    // Compact Coordinate result reconstructs exact checked-domain generic outputs.
    let i = fixture(3, [65535, 1], false, 1, 0);
    let o = coordinate::Output {
        coordinates: i.metadata.coordinates,
        fractions: i.fractions,
    };
    assert_eq!(compact::expand_result(compact::pack_result(o), 3), o);
}

#[test]
fn command_direct_weights_use_the_owned_producer_and_original_bank_binding() {
    use gpu_v2::{
        framebuffer::ports::MaterializedSurface,
        system::{pixel::quad_ingress::rtl::Backend, vliw_command},
        texture::rtl::compact_coefficient,
    };
    let generic = compact_coefficient::verilog();
    assert!(generic.contains("reg [71:0] ready_ff0,ready_ff1;"));
    assert!(!generic.contains("direct_weight_write"));
    let source = vliw_command::verilog(
        MaterializedSurface {
            color_base_bytes: 0x100000,
            depth_base_bytes: 0x140000,
            width: 400,
            height: 240,
        },
        Backend::Portable,
    )
    .unwrap();
    let producer = source
        .split_once("module gpu_v2_runtime_coefficient_directweight(")
        .unwrap()
        .1
        .split_once("endmodule")
        .unwrap()
        .0;
    assert!(producer.contains("reg [7:0] ready_ff0,ready_ff1;"));
    assert!(producer.contains("assign direct_weight_value=returning_corners;"));
    assert!(producer.contains("coefficient metadata cross-port collision"));
    assert!(producer.contains("always @(posedge clk)fault<=0;"));
    assert!(producer.contains("assign preflight_bad=1'b0;"));
    let wrapper = source
        .split_once("module gpu_v2_texture_runtime_backend_unique(")
        .unwrap()
        .1
        .split_once("endmodule")
        .unwrap()
        .0;
    for pin in ["write", "address", "value"] {
        assert_eq!(
            wrapper
                .matches(&format!(".direct_weight_{pin}(weight_{pin})"))
                .count(),
            1
        );
    }
    assert!(!wrapper.contains("assign weight_write="));
}
