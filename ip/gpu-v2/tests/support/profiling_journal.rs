//! Finite passive replay of captured real GPU inputs; no additional MC steps.
use gpu_v2::system::vliw_cycle::draw::{Gpu, Input, Output};
use std::{collections::BTreeMap, io::Write, path::Path};
pub fn record(
    path: &Path,
    initial: Gpu,
    rows: impl IntoIterator<Item = (Input, Output)>,
) -> BTreeMap<&'static str, usize> {
    record_inner(path, initial, rows, false, &[])
}
#[allow(dead_code)]
pub fn record_common(
    path: &Path,
    initial: Gpu,
    rows: impl IntoIterator<Item = (Input, Output)>,
) -> BTreeMap<&'static str, usize> {
    record_inner(path, initial, rows, true, &[])
}
type NativeWrite = (
    u64,
    digital_design_hardware_gowin::sdram_memory_controller::cycle::pins::Write,
);
#[allow(dead_code)]
pub fn record_common_native(
    path: &Path,
    initial: Gpu,
    rows: impl IntoIterator<Item = (Input, Output)>,
    writes: &[NativeWrite],
) -> BTreeMap<&'static str, usize> {
    record_inner(path, initial, rows, true, writes)
}
fn record_inner(
    path: &Path,
    initial: Gpu,
    rows: impl IntoIterator<Item = (Input, Output)>,
    common: bool,
    writes: &[NativeWrite],
) -> BTreeMap<&'static str, usize> {
    if !common {
        std::fs::write(
            path.with_file_name("gpu-load-columns.json"),
            format!("{:?}\n", gpu_v2::system::vliw_cycle::trace::LOAD_COLUMNS),
        )
        .unwrap();
    }
    let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut plain = initial;
    let mut observed = plain.clone();
    let mut sid = 0;
    let mut executed_pc = None;
    let mut latched_command = None;
    let mut old_busy = false;
    let mut stopped = false;
    let mut kinds = std::collections::BTreeMap::<&str, usize>::new();
    let mut end_tick = 0;
    let mut native = writes.iter().peekable();
    for (wall, reference) in rows.into_iter().enumerate() {
        assert!(wall < 200000);
        let (i, o) = reference;
        for _ in 0..2 {
            assert_eq!(format!("{:?}", observed.outputs(i)), format!("{:?}", o));
            assert_eq!(format!("{:?}", plain.outputs(i)), format!("{:?}", o));
        }
        let tick = wall * 148;
        end_tick = tick as u64 + 148;
        for port in 0..if common { 0 } else { 3 } {
            let request = o.physical[port];
            let physical = i.physical[port];
            let p = port + 4;
            if request.valid && physical.request_ready {
                writeln!(
                    file,
                    "[{tick},\"accept\",[{p},{},{},{}]]",
                    request.address_halfword,
                    port == 2,
                    (u16::from(request.count) + 1) * 4
                )
                .unwrap();
            }
            if port < 2 && physical.response.valid {
                writeln!(file, "[{tick},\"payload\",[{p},8,0]]").unwrap();
            }
            if port == 2 && physical.write_ready {
                writeln!(file, "[{tick},\"payload\",[{p},0,8]]").unwrap();
            }
            if physical.response.valid && physical.response.last {
                writeln!(
                    file,
                    "[{tick},\"terminal\",[{p},{}]]",
                    physical.response.error
                )
                .unwrap();
            }
        }
        if old_busy && !o.control.processor.busy && !stopped {
            writeln!(file, "[{tick},\"gpu_done\",{sid}]").unwrap();
        }
        old_busy = o.control.processor.busy;
        let edge = observed.clock_observing(i).unwrap();
        plain.clock(i).unwrap();
        let c = edge.command;
        if let Some((base, bytes)) = c.submit {
            stopped = false;
            sid += 1;
            executed_pc = None;
            latched_command = None;
            writeln!(
                file,
                "[{tick},\"submit\",{sid}]\n[{tick},\"cb_submit\",[{sid},{base},{bytes}]]"
            )
            .unwrap();
        }
        if let Some(command) = c.latched {
            latched_command = Some(command);
        }
        for (name, value) in [
            ("cb_latch", c.latched),
            ("cb_execute", c.execute),
            ("cb_issue", c.issued),
        ] {
            // The production Recorder deduplicates enabled Exec retries.
            if name == "cb_execute" {
                if let Some((pc, _)) = value {
                    if executed_pc == Some(pc) {
                        continue;
                    }
                    executed_pc = Some(pc);
                }
            }
            if let Some((pc, word)) = value {
                writeln!(file, "[{tick},\"{name}\",[{sid},{pc},{word}]]").unwrap();
            }
        }
        if let Some(pc) = c.released {
            let (latched, word) = latched_command.unwrap();
            assert_eq!(pc, latched);
            writeln!(file, "[{tick},\"cb_release\",[{sid},{pc},{word}]]").unwrap();
        }
        for (name, active) in [("cb_abort", c.aborted), ("cb_reset", c.reset)] {
            if active {
                writeln!(file, "[{tick},\"{name}\",[{sid}]]").unwrap();
                if common {
                    stopped = true;
                    writeln!(
                        file,
                        "[{tick},\"gpu_stopped\",[{sid},\"{}\"]]",
                        if name == "cb_abort" { "abort" } else { "reset" }
                    )
                    .unwrap();
                }
            }
        }
        if c.drained {
            writeln!(file, "[{tick},\"cb_drain\",[{sid}]]").unwrap();
        }
        writeln!(file, "[{tick},\"gpu_load\",{:?}]", edge.trace.load).unwrap();
        if let Some(control) = edge.trace.control {
            writeln!(
                file,
                "[{tick},\"gpu_control\",[{sid},{control:?},{}]]",
                edge.trace.control_word.unwrap()
            )
            .unwrap();
        }
        if let Some(cache) = edge.trace.cache {
            let tags = cache.valid_tags();
            writeln!(
                file,
                "[{tick},\"gpu_cache\",[{:?},{:?},{},{}]]",
                cache.valid, tags, cache.color_base, cache.depth_base
            )
            .unwrap();
        }
        for event in edge.trace.events {
            *kinds.entry(event.kind).or_default() += 1;
            writeln!(
                file,
                "[{tick},\"gpu_event\",[{sid},\"{}\",{:?}]]",
                event.kind, event.values
            )
            .unwrap();
        }
        while let Some((time, write)) = native.peek() {
            if *time >= (tick as u64) + 148 {
                break;
            }
            assert!(*time >= tick as u64);
            writeln!(
                file,
                "[[{time},1],\"physical_write\",[{},{},{}]]",
                write.address_bytes, write.data, write.unmasked
            )
            .unwrap();
            native.next();
        }
    }
    file.flush().unwrap();
    assert!(native.next().is_none());
    if common {
        common_metadata(
            path.parent().unwrap(),
            end_tick,
            "gpu-cycle-emu",
            !old_busy && !stopped,
        );
    }
    assert_eq!(format!("{plain:?}"), format!("{observed:?}"));
    // Separate bounded reset qualification after the captured reference. It
    // neither appends fabricated capture edges nor clocks the memory service.
    let reset = Input {
        reset: true,
        ce: true,
        ..Default::default()
    };
    if observed.control.processor.draw_status().unwrap().2 {
        let edge = observed.clock_observing(reset).unwrap();
        plain.clock(reset).unwrap();
        assert!(edge
            .trace
            .events
            .iter()
            .any(|e| e.kind == "target_unbind" && e.values == [1]));
        assert!(!observed.control.processor.draw_status().unwrap().2);
        assert_eq!(format!("{plain:?}"), format!("{observed:?}"));
        let held = observed.clock_observing(reset).unwrap();
        plain.clock(reset).unwrap();
        assert!(!held.trace.events.iter().any(|e| e.kind == "target_unbind"));
        assert_eq!(format!("{plain:?}"), format!("{observed:?}"));
    }
    kinds
}

#[allow(dead_code)]
pub fn common_metadata(folder: &Path, end_tick: u64, source: &str, reached: bool) {
    let path = folder.join("events.jsonl");
    let count = std::fs::read_to_string(&path).unwrap().lines().count();
    let native = folder.join("initial-memory.bin").is_file();
    let image = if native {
        r#"{"available":true,"preimage":"initial-memory.bin","boundary":"native32-write","completed_target_budget":2}"#
    } else {
        r#"{"available":false}"#
    };
    let evidence = if native {
        r#"["source-executable.bin","initial-memory.bin"]"#
    } else {
        r#"["source-executable.bin"]"#
    };
    std::fs::copy(
        std::env::current_exe().unwrap(),
        folder.join("source-executable.bin"),
    )
    .unwrap();
    let catalog = gpu_v2::system::vliw_cycle::trace::CAPTURE_CONTRACT
        .trim()
        .strip_suffix('}')
        .unwrap();
    let meta = format!(
        r#"{catalog},"clock_hz":54000000,"quantum_hz":7992000000,
      "source_kind":"{source}","origin_tick":0,"end_tick":{end_tick},"coverage_end_tick":{end_tick},
      "events":{count},"event_limit":10000000,"reached":{reached},"truncated":{},
      "ports":["display","instruction","data","dma","gpu_ro","gpu_fb_r","gpu_fb_w"],
      "gpu_load_columns":{:?},"evidence_files":{evidence},"scanouts":[],"image_journal":{image},
      "domains":{{"gpu":true,"cpu":false,"display":false,"shared_service64":false,"native_model32":false,"physical_write":{native}}},
      "capabilities":{{"client_payload64":false,"native_model32":false,"shared_service64":false,"pin_electrical":false,
        "command_control_lineage":true,"gpu_signal_journal":true,"gpu_master_requests":true,"gpu_master_payloads":true,"instruction_owner_lineage":true}}}}
    "#,
        !reached,
        gpu_v2::system::vliw_cycle::trace::LOAD_COLUMNS
    );
    std::fs::write(folder.join("capture.json"), meta).unwrap();
}

#[allow(dead_code)]
pub fn observer(path: &Path, submissions: usize, max_cycle: usize) -> String {
    let template = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/observer.vh"),
    )
    .unwrap();
    template
        .replace("@GPU@", "top.dut")
        .replace("@CLOCK@", "clk")
        .replace("@RESET@", "reset")
        .replace("@ENABLE@", "ce")
        .replace("@CYCLE@", "n")
        .replace("@EVENTS@", &path.to_string_lossy().replace('\\', "/"))
        .replace(
            "pr_max_submissions=1",
            &format!("pr_max_submissions={submissions}"),
        )
        .replace("pr_limit=12000000", &format!("pr_limit={max_cycle}"))
}

pub fn memory(
    root: &Path,
    initial: &[u16],
    writes: &[(
        u64,
        digital_design_hardware_gowin::sdram_memory_controller::cycle::pins::Write,
    )],
    oracles: &[(usize, Vec<u8>, Vec<u8>)],
) {
    std::fs::write(
        root.join("initial-memory.bin"),
        initial
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut physical =
        std::io::BufWriter::new(std::fs::File::create(root.join("physical-writes.jsonl")).unwrap());
    for (tick, write) in writes {
        writeln!(
            physical,
            "[{tick},\"physical_write\",[{},{},{}]]",
            write.address_bytes, write.data, write.unmasked
        )
        .unwrap();
    }
    physical.flush().unwrap();
    for (index, (wall, color, depth)) in oracles.iter().enumerate() {
        std::fs::write(root.join(format!("oracle-{}-color.bin", index + 1)), color).unwrap();
        std::fs::write(root.join(format!("oracle-{}-depth.bin", index + 1)), depth).unwrap();
        assert!(*wall < 200000);
    }
}
