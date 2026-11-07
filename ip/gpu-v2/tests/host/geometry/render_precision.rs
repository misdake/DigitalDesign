//! Independent boundary checks for the selected render precision contract.
use gpu_v2::{
    lighting::ports::{CompactPixelInput, PixelInput, PixelRows},
    texture::{
        ports::*,
        sim::{counted, oracle, staged::bound},
    },
    vertex::ports::Transformed,
};

fn quad() -> QuadInput {
    QuadInput {
        quad_id: 0,
        mask: 1,
        uv: [[0.25, 0.5]; 4],
        force_coarsest: false,
        slot: 0,
        material_size_log2: 10,
        filter: Filter::Trilinear,
        lod_bias: -32.0,
    }
}
fn slot() -> Slot {
    Slot {
        base_address: 4096,
        max_size_log2: 10,
        has_full_mip: true,
        valid: true,
    }
}

#[test]
fn transformed_normal_is_twelve_bits_and_rejects_aliases() {
    let v = Transformed {
        clip: [i32::MIN, i32::MAX, 0, 65536],
        normal: [-2048, 2047, -1],
        uv: [0, 4095],
        rgb565: 0xabcd,
    };
    v.validate().unwrap();
    let rows = v.rows();
    assert_eq!(rows[4], 2048 | (2047 << 12) | (4095 << 24));
    assert_eq!(rows[5], 4095 << 12);
    assert_eq!(Transformed::from_rows(rows).unwrap(), v);
    for (i, spare) in [(4, 36), (5, 24), (6, 16)] {
        let mut corrupt = rows;
        corrupt[i] |= 1 << spare;
        assert!(Transformed::from_rows(corrupt).is_err());
    }
    assert!(Transformed {
        normal: [2048, 0, 0],
        ..v.clone()
    }
    .validate()
    .is_err());
    assert!(Transformed {
        normal: [-2049, 0, 0],
        ..v
    }
    .validate()
    .is_err());
}

#[test]
fn ndc_uses_two_sixteen_bit_codes_in_one_row() {
    let v = CompactPixelInput {
        normal: [-2048, 2047, -1],
        ndc: [-16384, 16384],
    };
    let rows = v.rows().unwrap();
    assert_eq!(rows[1], 0x4000_c000);
    assert_eq!(CompactPixelInput::from_rows(rows).unwrap(), v);
    assert!(CompactPixelInput::from_rows([rows[0], rows[1] | (1 << 32)]).is_err());
    let p = v.expanded().unwrap();
    let got = PixelRows::encode(p).unwrap().decode().unwrap();
    assert_eq!(got.normal, p.normal);
    assert_eq!(got.ndc, p.ndc);
    assert!(PixelRows::encode(PixelInput {
        ndc: [16385, 0],
        ..p
    })
    .is_err());
}

#[test]
fn every_400_by_240_pixel_center_has_one_rounding_boundary() {
    use gpu_v2::lighting::ports::pixel_center_ndc;
    for y in 0..240 {
        for x in 0..400 {
            let expected = [
                (((f64::from(x) + 0.5) * 2.0 / 400.0 - 1.0) * 16384.0).round_ties_even() as i32,
                ((1.0 - (f64::from(y) + 0.5) * 2.0 / 240.0) * 16384.0).round_ties_even() as i32,
            ];
            assert_eq!(pixel_center_ndc(x, y, 400, 240).unwrap(), expected);
        }
    }
    assert!(pixel_center_ndc(400, 0, 400, 240).is_err());
    assert!(pixel_center_ndc(0, 0, 0, 240).is_err());
}

#[test]
fn uv_capture_preserves_unwrapped_domain_and_rne_ties() {
    let mut q = quad();
    q.mask = 15;
    q.uv = [
        [-2.0, 131071.0 / 65536.0],
        [1.0, -1.0],
        [2.5 / 65536.0, 3.5 / 65536.0],
        [-2.5 / 65536.0, -3.5 / 65536.0],
    ];
    let (raw, forced) = capture_uv(&q).unwrap();
    assert_eq!(raw, [[-131072, 131071], [65536, -65536], [2, 4], [-2, -4]]);
    assert!(!forced);
    q.uv[0][0] = 2.0;
    assert!(capture_uv(&q).is_err());
}

#[test]
fn invalid_uncovered_helper_forces_coarsest_despite_negative_bias() {
    let mut q = quad();
    // A thin projected triangle can extrapolate a helper far outside [-2,2).
    q.uv[3] = [8.5, -9.0];
    let (raw, forced) = capture_uv(&q).unwrap();
    assert!(forced);
    assert_eq!(raw[0], [16384, 32768]);
    assert_eq!(raw[3], [0, 0]);
    let s = slot();
    let ideal = oracle::prepare(&q, &[s], Config::counted()).unwrap();
    assert_eq!(ideal.lod.selected, 10.0);
    assert!(ideal
        .pixels
        .iter()
        .all(|p| p.layers.iter().all(|l| l.n == 0)));
    let monolithic = counted::prepare(&q, &[s]).unwrap();
    let staged = bound::prepare(&q, &[s]).unwrap();
    assert_eq!(
        monolithic
            .groups
            .iter()
            .map(|g| g.pack72().unwrap() as i128)
            .collect::<Vec<_>>(),
        staged.payloads
    );
    q.uv = [[0.25, 0.5]; 4];
    q.force_coarsest = true;
    assert_eq!(
        oracle::prepare(&q, &[s], Config::counted())
            .unwrap()
            .lod
            .selected,
        10.0
    );
    let no_mips = Slot {
        has_full_mip: false,
        ..s
    };
    let only_base = oracle::prepare(&q, &[no_mips], Config::counted()).unwrap();
    assert_eq!(only_base.lod.selected, 0.0);
    assert!(only_base
        .pixels
        .iter()
        .all(|p| p.layers.iter().all(|l| l.n == 10)));
    q.mask = 9;
    q.uv[3][0] = 8.5;
    assert!(capture_uv(&q).is_err());
}
