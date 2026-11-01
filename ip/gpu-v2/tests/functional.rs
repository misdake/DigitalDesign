use gpu_v2::system::{
    functional::Executor,
    vliw_command::{self as abi, Command, DeviceSubmission},
};

fn put(memory: &mut [u16], base: usize, bytes: &[u8]) {
    for (out, pair) in memory[base / 2..].iter_mut().zip(bytes.as_chunks::<2>().0) {
        *out = u16::from_le_bytes(*pair);
    }
}
fn image(memory: &mut [u16], commands: &[Command]) -> DeviceSubmission {
    let bytes: Vec<_> = commands
        .iter()
        .flat_map(|command| command.encode())
        .collect();
    put(memory, 0x40000, &bytes);
    DeviceSubmission::new(0x40000, bytes.len() as u32).unwrap()
}

#[test]
fn native_memcpy_moves_raw_words_with_guards_and_real_completion() {
    let mut memory = vec![0xa55a; 0x400000];
    for (i, word) in memory[0x50000 / 2..0x50200 / 2].iter_mut().enumerate() {
        *word = (i as u16).wrapping_mul(173);
    }
    put(
        &mut memory,
        0x48000,
        &abi::memcpy_params(0x50000, 0x60000, 512).unwrap(),
    );
    let submission = image(
        &mut memory,
        &[
            Command::wait(false, true),
            Command::FLUSH,
            Command::dma_local(0x48000, 264, 32).unwrap(),
            Command::wait(true, false),
            Command::memcpy(264).unwrap(),
            Command::END,
        ],
    );
    let report = Executor::new(4096)
        .execute(&mut memory, submission)
        .unwrap();
    assert_eq!(report.effects.written_bytes, Some((0x60000, 0x60200)));
    assert_eq!(report.effects.target, None);
    assert_eq!(
        memory[0x50000 / 2..0x50200 / 2],
        memory[0x60000 / 2..0x60200 / 2]
    );
    assert_eq!(memory[0x60000 / 2 - 1], 0xa55a);
    assert_eq!(memory[0x60200 / 2], 0xa55a);
}

#[test]
fn unsupported_or_invalid_commands_and_missing_fences_are_errors() {
    for (commands, pc) in [
        (vec![Command::draw_meshlet(0).unwrap(), Command::END], 0),
        (vec![Command::NOP, Command(0xd0000000), Command::END], 4),
        (vec![Command::END], 0),
        (
            vec![
                Command::wait(false, true),
                Command::FLUSH,
                Command::END,
                Command::NOP,
            ],
            8,
        ),
    ] {
        let mut memory = vec![0; 0x400000];
        let submission = image(&mut memory, &commands);
        let error = Executor::new(64)
            .execute(&mut memory, submission)
            .unwrap_err();
        assert_eq!(error.byte_pc, pc);
        assert!(!error.reason.is_empty());
    }
}

#[test]
fn command_work_and_physical_memory_are_bounded() {
    let mut memory = vec![0; 0x400000];
    let submission = image(&mut memory, &[Command::NOP, Command::NOP]);
    let error = Executor::new(1)
        .execute(&mut memory, submission)
        .unwrap_err();
    assert_eq!(error.byte_pc, 4);
    assert!(error.reason.contains("work limit"));
    let error = Executor::new(64)
        .execute(&mut memory[..4], submission)
        .unwrap_err();
    assert!(error.reason.contains("physical memory"));
}

#[test]
fn ui_uses_native_atlas_permutation_alpha_and_column_target() {
    let mut memory = vec![0xa55a; 0x400000];
    let surface = gpu_v2::framebuffer::ports::MaterializedSurface {
        color_base_bytes: 0x400000,
        depth_base_bytes: 0x480000,
        width: 400,
        height: 240,
    };
    put(&mut memory, 0x48000, &abi::target_params(surface).unwrap());
    put(
        &mut memory,
        0x48020,
        &abi::ui_params(0x50000, 0x60000).unwrap(),
    );
    // Cell one is drawn, cell zero is outside the selected range, cell two skips.
    memory[0x50000 / 2] = 0x0001;
    memory[0x50000 / 2 + 1] = 0xffff;
    let mut glyph = [[0; 8]; 16];
    for (y, row) in glyph.iter_mut().enumerate() {
        for (x, pixel) in row.iter_mut().enumerate() {
            *pixel = (((x + 1) as u16) << 11)
                | (((y + 1) as u16) << 6)
                | (((x + y) as u16 & 31) << 1)
                | u16::from((x + y) % 3 != 0);
        }
    }
    put(&mut memory, 0x60000, &abi::ui_char(&glyph));
    let submission = image(
        &mut memory,
        &[
            Command::wait(false, true),
            Command::FLUSH,
            Command::dma_local(0x48000, 264, 64).unwrap(),
            Command::wait(true, false),
            Command::set_target(264).unwrap(),
            Command::FLUSH,
            Command::ui_grid_range(268, 1, 3).unwrap(),
            Command::END,
        ],
    );
    // A capture may contain only the used glyph and even one grid byte. It must
    // not make the entire atlas or adjacent bytes readable implicitly.
    let mut access = vec![0; memory.len() * 2];
    access[0x40000..0x40080].fill(1);
    access[0x48000..0x48040].fill(1);
    access[0x50001..0x50003].fill(1);
    access[0x60000..0x60100].fill(1);
    access[0x400000..0x400000 + 204800].fill(3);
    Executor::new(4096)
        .execute_captured(&mut memory, &mut access, submission)
        .unwrap();
    for (y, row) in glyph.iter().enumerate() {
        for (x, &rgba) in row.iter().enumerate() {
            let r = rgba >> 11;
            let g = (rgba >> 6) & 31;
            let b = (rgba >> 1) & 31;
            let expected = if rgba & 1 == 0 {
                0xa55a
            } else {
                (r << 11) | ((g * 2 + g / 16) << 5) | b
            };
            // First macro column, row zero: row-major 16x16 texels.
            assert_eq!(memory[0x400000 / 2 + y * 16 + x + 8], expected, "{x},{y}");
        }
    }
    assert_eq!(memory[0x400000 / 2], 0xa55a);
    assert_eq!(memory[0x400000 / 2 + 4096], 0xa55a, "skipped cell two");
    assert!(memory[0x480000 / 2..0x480000 / 2 + 102400]
        .iter()
        .all(|&v| v == 0xa55a));
}

#[test]
fn captured_copy_dependencies_initialize_bytes_and_reject_unknown_reads_or_forbidden_writes() {
    let mut memory = vec![0xa55a; 0x400000];
    put(
        &mut memory,
        0x48000,
        &abi::memcpy_params(0x50000, 0x60000, 512).unwrap(),
    );
    put(
        &mut memory,
        0x48020,
        &abi::memcpy_params(0x60000, 0x70000, 512).unwrap(),
    );
    memory[0x50000 / 2..0x50200 / 2].fill(0x1234);
    let submission = image(
        &mut memory,
        &[
            Command::wait(false, true),
            Command::FLUSH,
            Command::dma_local(0x48000, 264, 32).unwrap(),
            Command::wait(true, false),
            Command::memcpy(264).unwrap(),
            Command::dma_local(0x48020, 264, 32).unwrap(),
            Command::wait(true, false),
            Command::memcpy(264).unwrap(),
            Command::END,
        ],
    );
    let mut access = vec![0; memory.len() * 2];
    access[0x40000..0x40080].fill(1);
    access[0x48000..0x48040].fill(1);
    access[0x50000..0x50200].fill(1);
    access[0x60000..0x60200].fill(2);
    access[0x70000..0x70200].fill(2);
    let mut unknown = access.clone();
    unknown[0x50001] = 0;
    let error = Executor::new(4096)
        .execute_captured(&mut memory, &mut unknown, submission)
        .unwrap_err();
    assert_eq!(error.byte_pc, 16);
    assert!(error.reason.contains("uncaptured byte 0x50001"));
    assert!(memory[0x60000 / 2..0x60200 / 2]
        .iter()
        .all(|&value| value == 0xa55a));
    let mut forbidden = access.clone();
    forbidden[0x60001] = 0;
    let error = Executor::new(4096)
        .execute_captured(&mut memory, &mut forbidden, submission)
        .unwrap_err();
    assert_eq!(error.byte_pc, 16);
    assert!(error.reason.contains("writable bytes at 0x60001"));
    assert!(memory[0x60000 / 2..0x60200 / 2]
        .iter()
        .all(|&value| value == 0xa55a));
    Executor::new(4096)
        .execute_captured(&mut memory, &mut access, submission)
        .unwrap();
    assert!(memory[0x70000 / 2..0x70200 / 2]
        .iter()
        .all(|&value| value == 0x1234));
    assert!(access[0x60000..0x60200].iter().all(|&flags| flags == 3));
    assert!(access[0x70000..0x70200].iter().all(|&flags| flags == 3));
}
