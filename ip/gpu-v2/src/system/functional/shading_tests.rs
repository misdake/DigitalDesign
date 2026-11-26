use super::*;

#[test]
fn final_alpha_gates_only_depth_and_preserves_blended_color() {
    let mut state = DrawState {
        global: Global {
            far: 100 * 65536,
            light: light::Light {
                direction: [0, 0, 16384],
                ambient: 256,
                directional: 0,
            },
            projection: light::Projection {
                ray_scale: [1, 1],
                k: 1,
            },
            mode: 0,
            code: 0,
        },
        render: RenderState {
            texture: Texture::decode(1 << 24).unwrap(),
            lod_bias: 0,
            specular: [0; 3],
            control: 3 | 8,
        },
        target: [0, 128],
        color: u32::MAX,
        rgb: false,
    };
    for blend in [false, true] {
        state.render.control = if blend { 3 | 8 | 16 } else { 3 | 8 };
        for alpha in [0, 1, 127, 128, 253, 254, 255] {
            let mut words = vec![0; 256];
            words[0] = 0x07e0;
            words[128] = 100;
            let mut memory = super::super::Memory {
                words: &mut words,
                access: None,
                effects: Default::default(),
            };
            let result = rop(&mut memory, [0, 0], 50, [255, 0, 0, alpha], &state).unwrap();
            let weight = if blend {
                (u32::from(alpha) + 2) / 4
            } else {
                64
            };
            assert_eq!(result, Some(weight as u8));
            let red = (255 * weight + 32) / 64;
            let green = (255 * (64 - weight) + 32) / 64;
            let expected = (((red * 62 + 255) / 510) << 11) | (((green * 126 + 255) / 510) << 5);
            assert_eq!(memory.load(0).unwrap(), expected as u16);
            assert_eq!(
                memory.load(128).unwrap(),
                if weight == 64 { 50 } else { 100 }
            );
        }
    }
}

#[test]
fn atlas_grid_clamps_each_mip_and_never_samples_a_neighboring_cell() {
    for grid in 1..=3 {
        for filter in 0..=2 {
            let word = (5 << 16) | (1 << 20) | (filter << 21) | (1 << 25) | (grid << 26);
            let texture = Texture::decode(word).unwrap();
            let mut words = vec![0; texture.word_count()];
            for level in grid..=5 {
                let side = 1usize << level;
                let cell = side >> grid;
                for y in 0..side {
                    for x in 0..side {
                        let offset = (((x >> 4) << level.saturating_sub(4)) + (y >> 4)) * 256
                            + (y & 15) * 16
                            + (x & 15);
                        // Uniform cell colours at every permitted mip make any
                        // neighbor tap or forbidden coarse mip observable.
                        words[texture.prefix(level as u8) + offset] =
                            if (x / cell + y / cell).is_multiple_of(2) {
                                0xf800
                            } else {
                                0x07e0
                            };
                    }
                }
            }
            let memory = super::super::Memory {
                words: &mut words,
                access: None,
                effects: Default::default(),
            };
            for cell in 0..(1 << grid) {
                let first = cell * (65536 >> grid);
                let last = first + (65536 >> grid) - 1;
                for uv in [[first, first], [last, last], [first, last], [last, first]] {
                    for lod in [0, 1, 7, 8, 15, 16, 23, 24, 39, 255] {
                        let mut report = Report {
                            textures: Some(Default::default()),
                            ..Default::default()
                        };
                        let sampled = texture.sample(&memory, uv, lod, &mut report, 16).unwrap();
                        assert!(sampled[0] >= 250);
                        assert_eq!(&sampled[1..3], &[0, 0]);
                        for p in report.textures.unwrap().values() {
                            for level in 0..=5 {
                                if p.taps[level] == 0 {
                                    continue;
                                }
                                assert!(level >= grid as usize);
                                let size = 1i128 << (level - grid as usize);
                                let lo = cell * size;
                                assert!(p.tap_min[level].iter().all(|&v| v >= lo));
                                assert!(p.tap_max[level].iter().all(|&v| v < lo + size));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(Texture::decode((2 << 16) | (1 << 20) | (3 << 26)).is_err());
    assert!(Texture::decode((5 << 16) | (1 << 26)).is_err());
    assert!(Texture::decode((5 << 16) | (1 << 28)).is_err());
}

#[test]
fn uv_probe_observes_before_clamp_and_only_nonzero_weight_taps() {
    let mut data = vec![0x7bef; 256];
    let memory = super::super::Memory {
        words: &mut data,
        access: None,
        effects: Default::default(),
    };
    let texture = Texture::decode((1 << 16) | (1 << 21) | (1 << 25)).unwrap();
    let uv = [-1, 70000];
    let mut ordinary = Report::default();
    let expected = texture.sample(&memory, uv, 0, &mut ordinary, 16).unwrap();
    let mut observed = Report {
        textures: Some(Default::default()),
        ..Default::default()
    };
    assert_eq!(
        texture.sample(&memory, uv, 0, &mut observed, 16).unwrap(),
        expected
    );
    assert_eq!(ordinary.work, observed.work);
    let p = observed.textures.as_ref().unwrap().values().next().unwrap();
    assert_eq!(p.samples, 1);
    assert_eq!(p.outside_normalized, 1);
    assert_eq!(p.uv_min, uv);
    assert_eq!(p.uv_max, uv);
    assert_eq!(p.taps[1], 1);
    assert_eq!(p.tap_min[1], [0, 1]);
    assert_eq!(p.tap_max[1], [0, 1]);
}

fn sample_alpha(alphas: [u16; 3], filter: u8, lod: u8, uv: [i128; 2]) -> u8 {
    let mut words: Vec<u16> = alphas
        .into_iter()
        .flat_map(|a| std::iter::repeat_n(0xa5c0 | a, 256))
        .collect();
    let memory = super::super::Memory {
        words: &mut words,
        access: None,
        effects: Default::default(),
    };
    let texture =
        Texture::decode((2 << 16) | (1 << 20) | (u32::from(filter) << 21) | (1 << 24)).unwrap();
    let mut report = Report::default();
    texture.sample(&memory, uv, lod, &mut report, 8).unwrap()[3]
}

#[test]
fn filtered_constant_alpha_keeps_every_unorm4_value_and_mip_endpoint() {
    for alpha in 0..16 {
        for filter in 0..3 {
            for lod in 0..=16 {
                for uv in [[0, 0], [64, 35200], [16384, 32768], [65535, 65407]] {
                    assert_eq!(
                        sample_alpha([alpha; 3], filter, lod, uv),
                        (alpha * 17) as u8,
                        "alpha={alpha} filter={filter} lod={lod} uv={uv:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn trilinear_alpha_uses_the_same_conserved_mip_weights_as_color() {
    let coarse = [0, 64, 128, 192, 256, 319, 383, 447];
    for lod in 0..16u8 {
        let fine_level = 2 - usize::from(lod / 8);
        let alphas = [0, 5, 15];
        let parent = coarse[usize::from(lod % 8)];
        let raw = alphas[fine_level] * (511 - parent) + alphas[fine_level - 1] * parent;
        assert_eq!(
            u16::from(sample_alpha(alphas, 2, lod, [12345, 54321])),
            ((u32::from(raw) * 17 + 255) / 511) as u16
        );
    }
}

#[test]
fn rgba_format_and_mip_are_independent_and_fixed_views_stay_rgb565() {
    for mip in [false, true] {
        for filter in 0..3u32 {
            let descriptor = (4 << 16) | (u32::from(mip) << 20) | (filter << 21) | (1 << 24);
            let decoded = Texture::decode(descriptor).unwrap();
            assert_eq!(decoded.full_mip, mip);
            assert!(decoded.rgba4444);
            let clamped = Texture::decode(descriptor | (1 << 25)).unwrap();
            assert!(clamped.clamp && clamped.rgba4444);
            assert_eq!(clamped.full_mip, mip);
        }
    }
    assert!(Texture::decode((9 << 16) | (1 << 23) | (1 << 24)).is_err());
    assert!(Texture::decode((9 << 16) | (1 << 23) | (1 << 25)).is_err());
    assert!(Texture::decode(1 << 26).is_err());
}
