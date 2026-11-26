use super::*;

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
                            if (x / cell + y / cell) % 2 == 0 {
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
                        let mut report = Report::default();
                        let sampled = texture.sample(&memory, uv, lod, &mut report, 16).unwrap();
                        assert!(sampled[0] >= 250);
                        assert_eq!(&sampled[1..3], &[0, 0]);
                    }
                }
            }
        }
    }
    assert!(Texture::decode((2 << 16) | (1 << 20) | (3 << 26)).is_err());
    assert!(Texture::decode((5 << 16) | (1 << 26)).is_err());
    assert!(Texture::decode((5 << 16) | (1 << 28)).is_err());
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
