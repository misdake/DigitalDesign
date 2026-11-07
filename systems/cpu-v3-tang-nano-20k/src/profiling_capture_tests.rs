use super::*;

// Generated through the current ISA/CB06 encoders; these tests exercise the
// host freeze boundary, not the complete snapshot/catalog decoder.
fn encoded(commands: &[gpu_v2::system::vliw_command::Command]) -> Vec<u8> {
    commands.iter().flat_map(|c| c.0.to_le_bytes()).collect()
}

fn clear_cb(color: u16) -> Vec<u8> {
    use gpu_v2::system::vliw_command::Command as C;
    encoded(&[
        C::wait(false, true),
        C::FLUSH,
        C::dma_local(0x48000, 264, 32).unwrap(),
        C::wait(true, false),
        C::set_target(264).unwrap(),
        C::clear(false, color),
        C::FLUSH,
        C::END,
    ])
}

fn prefix_program(producers: &[Vec<u8>]) -> (Vec<u8>, CapturePoint) {
    use crate::boot::{pack_application_resources, BootResource};
    use cpu_v3::{device_send, halt, load_immediate16};
    use gpu_v2::{framebuffer::ports::MaterializedSurface, system::vliw_command as command};
    let mut code = Vec::new();
    let send = |code: &mut Vec<u16>, channel, value| {
        code.extend(load_immediate16(1, value));
        code.push(device_send(1, 4, channel));
    };
    let mut resources = Vec::new();
    for (index, producer) in producers.iter().enumerate() {
        let base = 0x40000 + index * 0x1000;
        send(&mut code, 0, (producer.len() / 4 - 1) as u16);
        send(&mut code, 1, (base / 128) as u16);
        resources.push(BootResource::copy(base, producer.clone()));
    }
    let point = CapturePoint {
        gate_pc: u16::try_from(code.len()).unwrap(),
        header_word: 0x10000,
        ranges_word: 0x11000,
    };
    // Both target SENDs follow the gate; executing them changes color.
    send(&mut code, 0, 7);
    send(&mut code, 1, (0x4f000u32 / 128) as u16);
    code.push(halt());
    let mut header = vec![0; usize::from(abi::GPU_CAPTURE_WORDS)];
    for (field, value) in [
        (
            abi::GPU_CAPTURE_H_VERSION,
            abi::GPU_CAPTURE_EXPLICIT_VERSION,
        ),
        (abi::GPU_CAPTURE_H_WORDS, abi::GPU_CAPTURE_WORDS),
        (abi::GPU_CAPTURE_H_FLAGS, abi::GPU_CAPTURE_PUBLISHED),
        (
            abi::GPU_CAPTURE_H_BINDINGS,
            abi::GPU_CAPTURE_BINDINGS_SELF_CONTAINED,
        ),
        (abi::GPU_CAPTURE_H_RANGE_COUNT, 4),
        (abi::GPU_CAPTURE_H_RANGE_WORDS, abi::GPU_CAPTURE_RANGE_WORDS),
        (
            abi::GPU_CAPTURE_H_RANGE_VERSION,
            abi::GPU_CAPTURE_RANGE_VERSION,
        ),
    ] {
        header[usize::from(field)] = value;
    }
    let bytes = |words: Vec<u16>| {
        words
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    let plane_bytes = (gpu_v2::system::functional::PLANE_WORDS * 2) as u32;
    let ranges = [
        (0x400000u32, plane_bytes),
        (0x480000, plane_bytes),
        (0x50000, 512),
        (0x60000, 512),
    ]
    .into_iter()
    .flat_map(|(base, length)| {
        [
            abi::GPU_CAPTURE_READ,
            abi::GPU_CAPTURE_TAG_BUFFER,
            base as u16,
            (base >> 16) as u16,
            length as u16,
            (length >> 16) as u16,
            0,
            0,
        ]
    })
    .collect();
    let target = command::target_params(MaterializedSurface {
        color_base_bytes: 0x400000,
        depth_base_bytes: 0x480000,
        width: 400,
        height: 240,
    })
    .unwrap();
    resources.extend([
        BootResource::copy(point.header_word * 2, bytes(header)),
        BootResource::copy(point.ranges_word * 2, bytes(ranges)),
        BootResource::copy(0x4f000, clear_cb(0x2468)),
        BootResource::copy(0x48000, target.to_vec()),
        BootResource::copy(
            0x48020,
            command::memcpy_params(0x400000, 0x50000, 512)
                .unwrap()
                .to_vec(),
        ),
        BootResource::copy(
            0x48040,
            command::memcpy_params(0x50000, 0x60000, 512)
                .unwrap()
                .to_vec(),
        ),
    ]);
    (
        pack_application_resources(&bytes(code), 0, &resources).unwrap(),
        point,
    )
}

fn all_words(payload: &[u8], expected: u16) -> bool {
    payload
        .as_chunks::<2>()
        .0
        .iter()
        .all(|v| *v == expected.to_le_bytes())
}

#[test]
fn functional_prefix_writes_reach_preimage_without_executing_target() {
    let (flash, point) = prefix_program(&[clear_cb(0x1357)]);
    if let Some(directory) = std::env::var_os("CPU_PREFIX_CAPTURE_TEST_OUT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("prefix.bin"), &flash).unwrap();
        std::fs::write(directory.join("point.json"), serde_json::to_vec(&serde_json::json!({
                "gate_pc":point.gate_pc,"header_word":point.header_word,"ranges_word":point.ranges_word
            })).unwrap()).unwrap();
    }
    let frozen = capture_flash_with_functional_prefix(&flash, point, 0xa55a, 256).unwrap();
    assert!(all_words(&frozen.payloads[0], 0x1357));
    assert!(all_words(&frozen.payloads[1], 0xa55a));
    assert!(capture_flash(&flash, point, 0xa55a, 256)
        .err()
        .unwrap()
        .contains("submission preceded"));
}

#[test]
fn multiple_prefix_submissions_preserve_write_then_read_dependencies() {
    use gpu_v2::system::vliw_command::Command as C;
    let copy = |source| {
        encoded(&[
            C::wait(false, true),
            C::FLUSH,
            C::dma_local(source, 264, 32).unwrap(),
            C::wait(true, false),
            C::memcpy(264).unwrap(),
            C::END,
        ])
    };
    let (flash, point) = prefix_program(&[clear_cb(0x1357), copy(0x48020), copy(0x48040)]);
    let frozen = capture_flash_with_functional_prefix(&flash, point, 0xa55a, 256).unwrap();
    assert!(all_words(&frozen.payloads[0], 0x1357));
    assert!(all_words(&frozen.payloads[2], 0x1357));
    assert!(all_words(&frozen.payloads[3], 0x1357));
}

#[test]
fn first_gate_remains_identical_and_prefix_execution_is_bounded() {
    let (flash, point) = prefix_program(&[]);
    let old = capture_flash(&flash, point, 0x5a5a, 256).unwrap();
    let new = capture_flash_with_functional_prefix(&flash, point, 0x5a5a, 256).unwrap();
    assert_eq!(
        (
            &old.header,
            &old.ranges,
            &old.payloads,
            old.cpu_steps,
            old.vblank_events
        ),
        (
            &new.header,
            &new.ranges,
            &new.payloads,
            new.cpu_steps,
            new.vblank_events
        )
    );
    let native = capture_native_flash(&flash, point, 0x5a5a, 40000).unwrap();
    assert_eq!(native.data.header, new.header);
    assert_eq!(native.data.ranges, new.ranges);
    assert_eq!(native.data.payloads, new.payloads);
    let (flash, point) = prefix_program(&[clear_cb(0x1357)]);
    assert!(capture_native_flash(&flash, point, 0, 40000)
        .err()
        .unwrap()
        .contains("GPU submission preceded"));
    assert!(
        capture_native_flash_with_functional_prefix(&flash, point, 0, 1)
            .err()
            .unwrap()
            .contains("incomplete")
    );
    assert!(
        capture_native_flash_with_functional_prefix(&flash, point, 0, 0)
            .err()
            .unwrap()
            .contains("positive")
    );
    assert!(capture_flash_with_functional_prefix(&flash, point, 0, 1)
        .err()
        .unwrap()
        .contains("incomplete"));
    assert!(capture_flash_with_functional_prefix(&flash, point, 0, 0)
        .err()
        .unwrap()
        .contains("positive"));
}

#[test]
fn failed_unknown_or_out_of_range_prefix_never_publishes_capture() {
    use gpu_v2::system::vliw_command::Command as C;
    // Unknown instruction after a real write, missing local page, and a
    // malformed DMA extending beyond8MiB must all fail without guessed data.
    for (at, bad, reason) in [
        (24, C(0xd0000000), "unknown opcode"),
        (8, C::NOP, "not been DMA loaded"),
        (
            8,
            C(C::dma_local(0x7fffe0, 264, 32).unwrap().0 | 4),
            "copy range",
        ),
    ] {
        let mut producer = clear_cb(0x1357);
        producer[at..at + 4].copy_from_slice(&bad.encode());
        let (flash, point) = prefix_program(&[producer]);
        let error = capture_flash_with_functional_prefix(&flash, point, 0, 256)
            .err()
            .unwrap();
        assert!(
            error.contains("GPU prefix execution failed") && error.contains(reason),
            "{error}"
        );
        let error = capture_native_flash_with_functional_prefix(&flash, point, 0, 40000)
            .err()
            .unwrap();
        assert!(
            error.contains("GPU prefix execution failed") && error.contains(reason),
            "{error}"
        );
    }
}

fn captures() -> Vec<(Vec<u16>, Vec<Vec<u16>>)> {
    let recorded: serde_json::Value = serde_json::from_str(include_str!(
        "../tests/fixtures/prepared_capture_descriptors.json"
    ))
    .unwrap();
    recorded["captures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|capture| {
            (
                serde_json::from_value(capture["header"].clone()).unwrap(),
                serde_json::from_value(capture["ranges"].clone()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn real_v1_and_v2_prepared_envelopes_and_ranges_are_accepted() {
    for (header, ranges) in captures() {
        assert_eq!(range_count(&header).unwrap(), ranges.len());
        for row in ranges {
            payload_span(&row, 1 << 22).unwrap();
        }
    }
}

#[test]
fn malformed_or_unpublished_envelopes_are_rejected() {
    for (header, _) in captures() {
        assert!(range_count(&header[..63]).err().unwrap().contains("length"));
        let mut extra = header.clone();
        extra.push(0);
        assert!(range_count(&extra).err().unwrap().contains("length"));
        for (field, value, reason) in [
            (abi::GPU_CAPTURE_H_WORDS, 63, "length"),
            (abi::GPU_CAPTURE_H_VERSION, 3, "version"),
            (abi::GPU_CAPTURE_H_FLAGS, 0, "Unpublished"),
            (abi::GPU_CAPTURE_H_FLAGS, 2, "Unpublished"),
            (abi::GPU_CAPTURE_H_FLAGS, 0x8001, "unknown flags"),
            (abi::GPU_CAPTURE_H_BINDINGS, 0, "binding"),
            (abi::GPU_CAPTURE_H_BINDINGS, 2, "binding"),
            (abi::GPU_CAPTURE_H_RANGE_WORDS, 7, "range ABI"),
            (abi::GPU_CAPTURE_H_RANGE_VERSION, 2, "range ABI"),
            (abi::GPU_CAPTURE_H_RANGE_COUNT, 0, "range count"),
            (abi::GPU_CAPTURE_H_RANGE_COUNT, 65, "range count"),
            (abi::GPU_CAPTURE_H_GPU_STATUS, 1, "busy or faulted"),
            (abi::GPU_CAPTURE_H_GPU_STATUS, 2, "busy or faulted"),
        ] {
            let mut invalid = header.clone();
            invalid[usize::from(field)] = value;
            assert!(
                range_count(&invalid).err().unwrap().contains(reason),
                "version{} field{field}={value}",
                header[0]
            );
        }
    }
    let mut v1 = captures().remove(0).0;
    v1[usize::from(abi::GPU_CAPTURE_H_FLAGS)] |= abi::GPU_CAPTURE_HAS_TARGET;
    assert!(range_count(&v1).err().unwrap().contains("unknown flags"));
}

#[test]
fn malformed_range_flags_extents_and_table_rows_are_rejected() {
    for (_, rows) in captures() {
        let row = &rows[0];
        assert!(payload_span(&row[..7], 1 << 22)
            .unwrap_err()
            .contains("length"));
        for (field, value, reason) in [
            (abi::GPU_CAPTURE_R_FLAGS, 0, "flags"),
            (abi::GPU_CAPTURE_R_FLAGS, 33, "flags"),
            (
                abi::GPU_CAPTURE_R_FLAGS,
                abi::GPU_CAPTURE_IMMUTABLE | abi::GPU_CAPTURE_WRITE,
                "flags",
            ),
            (abi::GPU_CAPTURE_R_BASE, 1, "halfword"),
            (abi::GPU_CAPTURE_R_BYTES, 0, "halfword"),
            (abi::GPU_CAPTURE_R_BYTES, 3, "halfword"),
            (abi::GPU_CAPTURE_R_BASE + 1, 128, "physical memory"),
            (abi::GPU_CAPTURE_R_BYTES + 1, 128, "physical memory"),
        ] {
            let mut invalid = row.clone();
            invalid[usize::from(field)] = value;
            assert!(
                payload_span(&invalid, 1 << 22)
                    .unwrap_err()
                    .contains(reason),
                "range field{field}={value}"
            );
        }
    }
}
