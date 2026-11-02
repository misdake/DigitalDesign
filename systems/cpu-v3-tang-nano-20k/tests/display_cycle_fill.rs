//! Logic-domain differential. The real production pixel domain is a controlled
//! consumer peer; this does not qualify a Rust pixel/TMDS implementation or native MC.
use cpu_v3_tang_nano_20k::hardware::display_cycle::{
    Edges, Fill, FillInput, FillOutput, LineBuffer, LineBufferInput,
};
use cpu_v3_tang_nano_20k::{DisplayLineBuffer, FramebufferHdmi};
use digital_design_hardware::{HardwareIdentity, Module};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn decode(word: u128) -> FillInput {
    let mut shift = 101u32;
    let mut take = |width: u32| {
        shift -= width;
        (word >> shift) & ((1u128 << width) - 1u128)
    };
    FillInput {
        reset: take(1) != 0,
        boot_enable: take(1) != 0,
        boot_failed: take(1) != 0,
        memory_request_ready: take(1) != 0,
        memory_data_valid: take(1) != 0,
        memory_read_data: take(64) as u64,
        memory_last: take(1) != 0,
        memory_error: take(1) != 0,
        device_index: take(3) as u8,
        device_channel: take(4) as u8,
        device_read_enable: take(1) != 0,
        device_write_enable: take(1) != 0,
        device_write_data: take(16) as u16,
        released: take(2) as u8,
        frame_toggle: take(1) != 0,
        pixel_underflow: take(1) != 0,
        pixel_started: take(1) != 0,
    }
}
fn packed(o: FillOutput) -> u128 {
    [
        (u128::from(o.memory_request_valid), 1),
        (u128::from(o.memory_urgent), 1),
        (u128::from(o.memory_address), 22),
        (u128::from(o.line_write), 1),
        (u128::from(o.line_write_address), 9),
        (
            u128::from(if o.line_write { o.line_write_data } else { 0 }),
            64,
        ),
        (u128::from(o.published), 2),
        (u128::from(o.group_valid), 2),
        (u128::from(o.group_linear), 2),
        (u128::from(o.device_read_data), 16),
        (u128::from(o.underflow), 1),
        (u128::from(o.started_ack), 1),
        (u128::from(o.startup_error), 1),
        (u128::from(o.frame_event), 1),
    ]
    .into_iter()
    .fold(0, |value, (part, width)| (value << width) | part)
}
fn compare(path: &Path) -> Result<Vec<u64>, String> {
    let mut fill = Fill::default();
    let mut ram = LineBuffer::default();
    let mut cycle = 0usize;
    let mut saw_final = false;
    let mut final_read_mask = 0u16;
    let mut cells = 0;
    let mut summary = None;
    for row in BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?).lines() {
        let row = row.map_err(|e| e.to_string())?;
        let words: Vec<_> = row.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        match words[0] {
            "E" | "F" => {
                let final_row = words[0] == "F";
                let count = if final_row {
                    1
                } else {
                    words[1].parse::<usize>().map_err(|e| e.to_string())?
                };
                let offset = if final_row { 1 } else { 2 };
                let raw = u128::from_str_radix(words[offset], 16).map_err(|e| e.to_string())?;
                let expected =
                    u128::from_str_radix(words[offset + 1], 16).map_err(|e| e.to_string())?;
                let input = decode(raw);
                for _ in 0..count {
                    let output = fill.outputs(&input);
                    if packed(output) != expected {
                        return Err(format!("FILL_PORT edge={cycle} input={raw:032x} expected={expected:032x} actual={:032x} state={fill:?}",packed(output)));
                    }
                    if !final_row {
                        ram.clock(
                            Edges {
                                logic_rise: true,
                                pixel_rise: false,
                            },
                            &LineBufferInput {
                                write_enable: output.line_write,
                                write_address: output.line_write_address,
                                write_data: output.line_write_data,
                                ..Default::default()
                            },
                        );
                        fill.clock(&input);
                        cycle += 1;
                        if cycle > 4_000_000 {
                            return Err("logic bound".into());
                        }
                    }
                }
                if final_row {
                    final_read_mask |= 1 << input.device_channel;
                }
                saw_final |= final_row;
            }
            "M" => {
                let address = words[1].parse::<u16>().map_err(|e| e.to_string())?;
                for bank in 0..2 {
                    let expected = u32::from_str_radix(words[bank + 2], 16).ok();
                    let actual = ram.peek(bank, address);
                    if expected != actual.known.then_some(actual.value) {
                        return Err(format!(
                            "FILL_RAM cell={address} bank={bank} actual={actual:?} RTL={}",
                            words[bank + 2]
                        ));
                    }
                }
                cells += 1;
            }
            "S" => {
                summary = Some(
                    words[1..]
                        .iter()
                        .map(|s| s.parse::<u64>().unwrap())
                        .collect::<Vec<_>>(),
                )
            }
            _ => return Err(format!("unknown row {row}")),
        }
    }
    let summary = summary.ok_or("missing summary")?;
    if !saw_final || final_read_mask & 31 != 31 || cells != 400 || summary[0] != cycle as u64 {
        return Err("incomplete trace".into());
    }
    Ok(summary)
}
fn run(command: &mut Command, out: &Path, name: &str) -> bool {
    let argv: Vec<_> = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    fs::write(
        out.join(format!("{name}.command.json")),
        serde_json::to_string_pretty(&argv).unwrap(),
    )
    .unwrap();
    command
        .stdout(Stdio::from(
            fs::File::create(out.join(format!("{name}.stdout"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(out.join(format!("{name}.stderr"))).unwrap(),
        ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.success();
        }
        if start.elapsed() > Duration::from_secs(90) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("{name} wall bound");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn black_publication_partial_and_physical_address_limit() {
    let mut fill = Fill::default();
    let mut input = FillInput {
        boot_enable: true,
        device_index: 3,
        device_channel: 3,
        device_read_enable: true,
        ..Default::default()
    };
    for _ in 0..4 {
        fill.clock(&input);
    }
    let out = fill.outputs(&input);
    assert_eq!(out.published, 3);
    assert_eq!(out.group_valid, 0);
    assert!(!out.memory_request_valid && !out.line_write && !out.started_ack);
    for (base, status) in [(0x3e7000, 1), (0x3e7010, 5), (0x200001, 5)] {
        input.device_write_enable = true;
        input.device_channel = 1;
        input.device_write_data = base as u16;
        fill.clock(&input);
        input.device_write_enable = false;
        input.device_channel = 3;
        assert_ne!(fill.outputs(&input).device_read_data & 2, 0);
        input.device_write_enable = true;
        input.device_channel = 2;
        input.device_write_data = (base >> 16) as u16;
        fill.clock(&input);
        input.device_channel = 3;
        input.device_write_data = 1;
        fill.clock(&input);
        input.device_write_enable = false;
        assert_eq!(fill.outputs(&input).device_read_data, status);
    }
}
fn qualification(group: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/display-cycle-fill");
    fs::create_dir_all(&out).unwrap();
    let line_name = DisplayLineBuffer::verilog_identity().module_name();
    let source = FramebufferHdmi::verilog_source()
        .unwrap()
        .replace(&line_name, "DisplayLineBuffer");
    fs::write(
        out.join("line.v"),
        DisplayLineBuffer::verilog_source().unwrap(),
    )
    .unwrap();
    fs::write(out.join("tb.v"), include_str!("display_cycle_fill_tb.v")).unwrap();
    let start = source
        .find("        if (frame_sync != frame_seen) begin")
        .unwrap();
    let end = source
        .find("        // Process device writes after the frame event")
        .unwrap();
    let device_end = source
        .find("        if (memory_request_valid && memory_request_ready) begin")
        .unwrap();
    let swapped = format!(
        "{}{}{}{}",
        &source[..start],
        &source[end..device_end],
        &source[start..end],
        &source[device_end..]
    );
    let variants = [
        ("production", source.clone(), true),
        (
            "short_release_cdc",
            source.replace("release_sync <= release_meta;", "release_sync <= released;"),
            false,
        ),
        ("swap_priority", swapped, false),
        (
            "ignored_error",
            source
                .replace(
                    "if (memory_error) begin memory_error_sticky<=1;",
                    "if (1'b0) begin memory_error_sticky<=1;",
                )
                .replace(
                    "if (memory_error || (memory_last != (beat_index==3)))",
                    "if (memory_last != (beat_index==3))",
                ),
            false,
        ),
    ];
    let mut results = Vec::new();
    for (name, source, positive) in variants {
        if (group == "mutants") == positive {
            continue;
        }
        fs::write(out.join(format!("{name}.v")), source).unwrap();
        let exe = out.join(format!("{name}.vvp"));
        let mut compile =
            Command::new(std::env::var("IVERILOG_EXE").unwrap_or_else(|_| "iverilog".into()));
        compile
            .args(["-g2012", "-s", "tb", "-o"])
            .arg(&exe)
            .arg(out.join("tb.v"))
            .arg(out.join("line.v"))
            .arg(out.join(format!("{name}.v")));
        assert!(run(&mut compile, &out, &format!("{name}-compile")));
        let cases: Vec<_> = if positive {
            vec![
                (0, 0),
                (0, 17),
                (1, 17),
                (2, 17),
                (3, 17),
                (4, 17),
                (5, 17),
                (6, 17),
            ]
        } else if name == "ignored_error" {
            vec![(3, 17)]
        } else {
            vec![(0, 17)]
        };
        for (case, phase) in cases {
            let label = format!("{name}-{case}-{phase}");
            let trace = out.join(format!("{label}.trace"));
            let mut simulate =
                Command::new(std::env::var("VVP_EXE").unwrap_or_else(|_| "vvp".into()));
            simulate
                .arg(&exe)
                .arg(format!("+case={case}"))
                .arg(format!("+phase={phase}"))
                .arg(format!("+trace={}", trace.display()));
            let success = run(&mut simulate, &out, &label);
            let result = compare(&trace);
            if positive {
                assert!(
                    success,
                    "{}",
                    fs::read_to_string(out.join(format!("{label}.stdout"))).unwrap()
                );
                let coverage = result.unwrap_or_else(|error| panic!("{label}: {error}"));
                assert!(coverage[0] > 1_000_000 && coverage[1] > 0);
                if case == 0 {
                    assert!(coverage[1] >= 6000 && coverage[3] >= 2 && coverage[4] > 1000);
                }
                if case == 1 {
                    assert_eq!(coverage[1], 50);
                }
                if case == 2 {
                    assert_eq!(coverage[1], 100);
                }
                results.push(serde_json::json!({"case":label,"coverage":coverage,"pass":true}));
            } else {
                let error = result.expect_err("DUT mutation escaped the per-edge port oracle");
                assert!(error.starts_with("FILL_PORT"), "{error}");
                fs::write(out.join(format!("{label}.diff")), &error).unwrap();
                results.push(serde_json::json!({"case":label,"detected":error}));
            }
        }
    }
    fs::write(out.join(format!("manifest-{group}.json")),serde_json::to_string_pretty(&serde_json::json!({
        "scope":"logic fill/CDC return/Device3 with actual RTL pixel consumer; no Rust pixels or native MC qualification",
        "max_logic_edges":4000000,"max_pixel_frames":4,"max_rle_rows":150000,
        "wall_seconds_per_process":90,"results":results,
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "bounded production fill, faults and two real pixel clock phases"]
fn production_fill_protocol() {
    qualification("normal");
}

#[test]
#[ignore = "real CDC, swap-priority and memory-error DUT mutations"]
fn production_fill_mutants() {
    qualification("mutants");
}
