//! Explicit complete-suite qualification; inputs are frozen by the caller.
use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::{
    boot::SystemControlDevice,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use gpu_v2::system::{functional::Executor, vliw_command::DeviceSubmission};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn root() -> PathBuf {
    PathBuf::from(
        std::env::var_os("GPU_BENCH_QUALIFICATION_ROOT")
            .expect("explicit frozen qualification inputs"),
    )
}
fn json_file(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn memory(path: &Path) -> (Value, Vec<u16>) {
    let catalog = json_file(&path.join("assets.json"));
    let mut memory = vec![0xa55a; 0x400000];
    for resource in catalog["resources"].as_array().unwrap() {
        let data = std::fs::read(path.join(resource["file"].as_str().unwrap())).unwrap();
        let first = resource["address"].as_u64().unwrap() as usize / 2;
        for (out, pair) in memory[first..].iter_mut().zip(data.as_chunks::<2>().0) {
            *out = u16::from_le_bytes(*pair);
        }
    }
    (catalog, memory)
}
fn execute(memory: &mut [u16], case: &Value) {
    for phase in ["setup", "work"] {
        let base = case[format!("{phase}_base")].as_u64().unwrap() as u32;
        let bytes = case[format!("{phase}_instructions")].as_u64().unwrap() as u32 * 4;
        Executor::new(64_000_000)
            .execute(memory, DeviceSubmission::new(base, bytes).unwrap())
            .unwrap();
    }
}
fn planes(memory: &[u16]) -> &[u16] {
    &memory[0x440000 / 2..0x4c0000 / 2]
}

#[test]
fn ui_reads_low_and_high_atlases_with_identical_alpha_and_untouched_depth() {
    use gpu_v2::{framebuffer::ports::MaterializedSurface, system::vliw_command as abi};
    fn put(memory: &mut [u16], base: usize, data: &[u8]) {
        for (out, pair) in memory[base / 2..].iter_mut().zip(data.as_chunks::<2>().0) {
            *out = u16::from_le_bytes(*pair);
        }
    }
    let surface = MaterializedSurface {
        color_base_bytes: 0x440000,
        depth_base_bytes: 0x480000,
        width: 400,
        height: 240,
    };
    let mut results = vec![];
    for atlas in [0x100000, 0x4e0000] {
        let grid = atlas + 0x10000;
        let mut memory = vec![0xa55a; 0x400000];
        put(&mut memory, 0x4d0000, &abi::target_params(surface).unwrap());
        put(
            &mut memory,
            0x4d0020,
            &abi::ui_params(grid as u32, atlas as u32).unwrap(),
        );
        memory[grid / 2] = 0xff00;
        let mut glyph = [[0; 8]; 16];
        for (y, row) in glyph.iter_mut().enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                *pixel = 0xf800 | ((x + y) as u16 & 31) << 6 | u16::from((x + y) % 3 != 0);
            }
        }
        put(&mut memory, atlas, &abi::ui_char(&glyph));
        let commands = [
            abi::Command::wait(false, true),
            abi::Command::FLUSH,
            abi::Command::dma_local(0x4d0000, 264, 64).unwrap(),
            abi::Command::wait(true, false),
            abi::Command::set_target(264).unwrap(),
            abi::Command::FLUSH,
            abi::Command::ui_grid_range(268, 0, 1).unwrap(),
            abi::Command::END,
        ];
        let data = commands.iter().flat_map(|c| c.encode()).collect::<Vec<_>>();
        put(&mut memory, 0x4c0000, &data);
        Executor::new(4096)
            .execute(
                &mut memory,
                DeviceSubmission::new(0x4c0000, data.len() as u32).unwrap(),
            )
            .unwrap();
        assert!(memory[0x480000 / 2..0x4c0000 / 2]
            .iter()
            .all(|&p| p == 0xa55a));
        assert!(planes(&memory).iter().any(|&p| p != 0xa55a));
        results.push(planes(&memory).to_vec());
    }
    assert_eq!(results[0], results[1]);
}

#[test]
#[ignore = "bounded full GPU suite; requires explicit frozen raw reference and bank fixtures"]
fn complete_workloads_match_and_full_scene_reads_all_inputs_from_low_bank() {
    let root = root();
    let start = Instant::now();
    let mut results = vec![];
    for (left, right, count) in [
        ("micro-reference", "micro", 56),
        ("full-high", "full-low", 6),
    ] {
        let (before, before_memory) = memory(&root.join(left));
        let (after, after_memory) = memory(&root.join(right));
        assert_eq!(before["cases"].as_array().unwrap().len(), count);
        assert_eq!(after["cases"].as_array().unwrap().len(), count);
        for i in 0..count {
            assert!(
                start.elapsed() < Duration::from_secs(600),
                "full suite host bound"
            );
            let mut a = before_memory.clone();
            let mut b = after_memory.clone();
            let ca = &before["cases"][i];
            let cb = &after["cases"][i];
            assert_eq!(ca["name"], cb["name"]);
            execute(&mut a, ca);
            execute(&mut b, cb);
            assert_eq!(planes(&a), planes(&b), "{left} case {}", ca["name"]);
            assert_eq!(
                &b[..0x440000 / 2],
                &after_memory[..0x440000 / 2],
                "immutable preimage"
            );
            assert_eq!(
                &b[0x4c0000 / 2..],
                &after_memory[0x4c0000 / 2..],
                "immutable tail"
            );
            results.push(
                json!({"input":right,"case":ca["name"],"color_depth_and_padding_equal":true}),
            );
        }
    }
    std::fs::write(
        root.join("gpu-equivalence.json"),
        serde_json::to_vec_pretty(&json!({
        "mode":"functional GPU; no timing claim", "maximum_work_per_submission":64000000,
        "reference_scope":"original micro raw native workload, never an accepted boot package",
        "cases":results}))
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "bounded complete CB06 CPU + functional GPU epoch; requires explicit application artifact"]
fn complete_micro_epoch_keeps_all_warmups_samples_and_crc_packets() {
    let root = root();
    let catalog = json_file(&root.join("micro/assets.json"));
    let image = std::fs::read(root.join("application/cpu-v3-boot.bin")).unwrap();
    let mut sim = CpuV3SystemSim::from_single_flash(&image, 0xa55a, VBlankMode::Manual)
        .unwrap()
        .with_functional_gpu();
    let start = Instant::now();
    let mut submissions = 0;
    let mut completed = false;
    let mut steps = 0;
    for step in 0..40_000_000 {
        steps = step + 1;
        if step == 10_000 {
            sim.advance_host_time(Duration::from_secs(8));
        }
        let outcome = sim
            .step_observing(|write, _| {
                if write.device == 4 && write.channel == 1 {
                    submissions += 1;
                }
                true
            })
            .unwrap()
            .unwrap();
        assert!(
            !matches!(outcome, StepOutcome::Halted { .. }),
            "{outcome:?}"
        );
        if step % 1000 == 0 {
            assert!(
                start.elapsed() < Duration::from_secs(600),
                "CPU/GPU epoch host bound"
            );
            assert!(
                sim.gpu().last_error().is_none(),
                "{:?}",
                sim.gpu().last_error()
            );
            let uart = &sim.cpu().device::<SystemControlDevice>(0).unwrap().uart;
            if uart.as_chunks::<20>().0.last().is_some_and(|p| p[5] == 3) {
                completed = true;
                break;
            }
        }
    }
    assert!(completed, "40 million CPU step bound");
    assert_eq!(submissions, 56 * (3 + 31) * 2);
    let uart = &sim.cpu().device::<SystemControlDevice>(0).unwrap().uart;
    let mut seen = vec![false; 56 * 31 * 2];
    let packets = uart.as_chunks::<20>().0;
    assert_eq!(packets.len(), 56 * 31 * 2 + 2);
    assert_eq!(packets.first().unwrap()[5], 0);
    assert_eq!(packets.last().unwrap()[5], 3);
    for p in packets {
        assert_eq!(&p[..5], b"GBEN\x01");
        assert_eq!(u16::from_le_bytes([p[18], p[19]]), super::crc16(&p[..18]));
        assert_eq!(
            u16::from_le_bytes([p[16], p[17]]) as u64,
            catalog["token"].as_u64().unwrap()
        );
        if p[5] == 1 || p[5] == 5 {
            let case = u16::from_le_bytes([p[6], p[7]]) as usize;
            let sample = u16::from_le_bytes([p[8], p[9]]) as usize;
            assert!(case < 56 && sample < 31);
            let at = (case * 31 + sample) * 2 + usize::from(p[5] == 5);
            assert!(!seen[at]);
            seen[at] = true;
        }
    }
    assert!(seen.into_iter().all(|v| v));
    std::fs::write(root.join("functional-epoch-uart.bin"), uart).unwrap();
    std::fs::write(root.join("functional-epoch.json"),serde_json::to_vec_pretty(&json!({
        "mode":"CPU instruction sim + functional GPU; host clock skips initial attach delay; no performance claim",
        "steps":steps,"maximum_steps":40000000,"submissions":submissions,
        "cases":56,"warmups":3,"samples":31,"packets":packets.len(),"complete":true})).unwrap()).unwrap();
}
