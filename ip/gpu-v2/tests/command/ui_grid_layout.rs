use gpu_v2::system::vliw_command::{ui_char, ui_params, Command};

#[test]
fn ui_assets_match_native_bank_order_without_losing_alpha_or_color() {
    let pixels = std::array::from_fn(|y| std::array::from_fn(|x| (y * 8 + x) as u16 * 509));
    let encoded = ui_char(&pixels);
    for (y, row) in pixels.iter().enumerate() {
        for (x, expected) in row.iter().enumerate() {
            let physical_lane = (x & 3) ^ ((y & 1) << 1);
            let offset = (y * 8 + (x & !3) + physical_lane) * 2;
            assert_eq!(
                u16::from_le_bytes([encoded[offset], encoded[offset + 1]]),
                *expected
            );
        }
    }
}

#[test]
fn ui_descriptor_uses_checked_aligned_physical_ranges() {
    assert_eq!(Command::ui_grid(276).unwrap().0, 0xb0000045);
    assert!(Command::ui_grid(277).is_err());
    assert_eq!(
        &ui_params(0x50000, 0x60000).unwrap()[..4],
        &0x00060a00u32.to_le_bytes()
    );
    assert!(ui_params(0x50001, 0x60000).is_err());
    assert!(ui_params(0x50000, 0x60080).is_err());
    assert!(ui_params(0x7fff80, 0x60000).is_err());
}

#[test]
fn ui_range_is_self_contained_in_the_existing_instruction() {
    for (first, end) in [(0, 1), (1, 2), (127, 130), (0, 750), (749, 750)] {
        let command = Command::ui_grid_range(276, first, end).unwrap().0;
        assert_eq!(command >> 28, 11);
        assert_eq!((command >> 27) & 1, 1);
        assert_eq!((command >> 7) & 1023, first as u32);
        assert_eq!((command >> 17) & 1023, end as u32 - 1);
        assert_eq!(command & 127, 69);
    }
    for (first, end) in [(0, 0), (3, 2), (749, 751)] {
        assert!(Command::ui_grid_range(276, first, end).is_err());
    }
    assert!(Command::ui_grid_range(277, 0, 1).is_err());
}
