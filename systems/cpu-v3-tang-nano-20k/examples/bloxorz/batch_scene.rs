//! Bounded compiled-CPU recording and native checks for imported level batches.
use super::static_scene::{bounded, native_replay, Peer, State};
use super::{compile, CpuV3Sim, RunOutcome};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

fn tool(out: &Path, args: &[&str]) {
    let mut c = Command::new("python");
    c.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/bloxorz/tools/batch-scene.py"));
    c.arg(args[0]).arg(out).args(&args[1..]);
    bounded(&mut c, &out.join(format!("{}.log", args[0])));
}
fn words(path: &Path) -> Vec<u16> {
    std::fs::read(path)
        .unwrap()
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
        .collect()
}
fn hex(path: &Path, values: &[u16]) {
    std::fs::write(
        path,
        values
            .iter()
            .map(|w| format!("{w:04x}\n"))
            .collect::<String>(),
    )
    .unwrap();
}
#[derive(Debug)]
struct Stats {
    steps: usize,
    commands: usize,
    objects: usize,
    draw_calls: usize,
    native_draws: usize,
}
fn record(level: &str, mode: &str, code: &[u16]) -> (PathBuf, Stats) {
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/bloxorz-batch/{level}-{mode}"));
    std::fs::create_dir_all(&out).unwrap();
    tool(&out, &["assets", level, mode]);
    let params = words(&out.join("params.bin"));
    let assets = words(&out.join("assets.bin"));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(code);
    cpu.physical_memory_mut()[0x7000..0x7000 + params.len()].copy_from_slice(&params);
    cpu.physical_memory_mut()[0x4b0000 / 2..0x4b0000 / 2 + assets.len()].copy_from_slice(&assets);
    cpu.physical_memory_mut()[0x4a0000 / 2..0x4b0000 / 2].fill(0xa55a);
    let state = Rc::new(RefCell::new(State::default()));
    for device in [0, 3, 4, 7] {
        cpu.attach_device(
            device,
            Box::new(Peer {
                device,
                state: state.clone(),
            }),
        );
    }
    let result = cpu.run(20_000_000).unwrap();
    let RunOutcome::Halted {
        signal: 0x600d,
        steps,
    } = result
    else {
        panic!("{level}/{mode}: {result:?}")
    };
    let s = state.borrow();
    assert_eq!((s.starts, s.completions, s.counts.len()), (1, 1, 1));
    assert_eq!(s.matrices.len(), usize::from(params[5]));
    assert!(s.maintenance > 0);
    let command_bytes = (usize::from(s.counts[0]) + 1) * 4;
    assert!(s.commands[0][command_bytes / 2..].iter().all(|w| *w == 0));
    let object_bytes = (s.objects[0].iter().rposition(|w| *w != 0xa55a).unwrap() + 1) * 2;
    assert_eq!(
        object_bytes % 128,
        0,
        "last instance padding written in full"
    );
    assert_eq!(object_bytes, 256 + usize::from(params[5]) * 128);
    hex(&out.join("program.hex"), code);
    hex(&out.join("cpu-commandbuffer.hex"), &s.commands[0]);
    std::fs::write(
        out.join("cpu-commandbuffer.bin"),
        s.commands[0]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(out.join("submit-count.txt"), s.counts[0].to_string()).unwrap();
    let mut seed = String::new();
    for (address, values) in [(0x490000, &s.commands[0]), (0x4a0000, &s.objects[0])] {
        seed += &format!("@{:x}\n", address / 8);
        for q in values.chunks_exact(4) {
            let v = q
                .iter()
                .enumerate()
                .map(|(i, w)| u64::from(*w) << (16 * i))
                .sum::<u64>();
            seed += &format!("{v:016x}\n");
        }
    }
    std::fs::write(out.join("cpu-gpu-input.hex"), seed).unwrap();
    std::fs::write(
        out.join("matrices.txt"),
        s.matrices
            .iter()
            .map(|row| {
                row.iter()
                    .map(|w| format!("{w:04x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
                    + "\n"
            })
            .collect::<String>(),
    )
    .unwrap();
    let native_draws = params[8..].chunks_exact(8).map(|p| usize::from(p[2])).sum();
    let stats = Stats {
        steps,
        commands: command_bytes,
        objects: object_bytes,
        draw_calls: s.matrices.len(),
        native_draws,
    };
    std::fs::write(
        out.join("cpu-recording.txt"),
        format!("{stats:?}\ndefault optimizer; separate control peer\n"),
    )
    .unwrap();
    (out, stats)
}

#[test]
fn batched_levels_record_less_cpu_work() {
    let code = compile(include_str!("rcc/bloxorz_batch.rs"));
    let mut summary =
        String::from("level,mode,steps,command_bytes,object_bytes,draw_calls,native_draws\n");
    for level in ["b1", "largest"] {
        let (tiled_out, tiled) = record(level, "tiled", &code);
        tool(&tiled_out, &["golden"]);
        let (out, batch) = record(level, "batch", &code);
        assert!(
            batch.steps < tiled.steps
                && batch.commands < tiled.commands
                && batch.objects < tiled.objects,
            "{level}: {batch:?} vs {tiled:?}"
        );
        super::static_scene::prepare_native(&out, 1, batch.native_draws, true);
        for (mode, s) in [("tiled", tiled), ("batch", batch)] {
            summary += &format!(
                "{level},{mode},{},{},{},{},{}\n",
                s.steps, s.commands, s.objects, s.draw_calls, s.native_draws
            );
        }
        tool(&out, &["golden"]);
        tool(&out, &["compare"]);
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bloxorz-batch");
    std::fs::write(out.join("comparison.csv"), &summary).unwrap();
    println!("{summary}");
}

#[test]
#[ignore = "bounded imported-level native GPU cases; CPU recording precedes separate GPU replay"]
fn batched_levels_native_gpu() {
    let code = compile(include_str!("rcc/bloxorz_batch.rs"));
    for level in ["b1", "largest"] {
        let (out, stats) = record(level, "batch", &code);
        tool(&out, &["golden"]);
        native_replay(&out, 1, stats.native_draws, true);
    }
}

#[test]
fn all_level_shared_instances_fit_native_range() {
    const PROBE: &str = r#"
mod bloxorz_scene;mod gpu_transform;
static CAMERA:Buf<u16,18>=Buf::new([0;18]);static MODEL:Buf<u16,18>=Buf::new([0;18]);static VIEW:Buf<u16,24>=Buf::new([0;24]);
fn main(){
 let input=unsafe{Ptr::from_addr(0x5000).as_u16_array()};let out=unsafe{Ptr::from_addr(0x6000).as_u16_array()};
 if !blox_camera(input[0u16],input[1u16],CAMERA.as_array(),VIEW.as_array()){halt(1);}
 let half=fix32::from_words(32768,0);
 gpu_pose_set(MODEL.as_array(),vec3::new(fix32::zero(),-fix32::from_words(6554,0),fix32::zero()),vec3::zero(),vec3::new(half,half,half));
 if !blox_instance(out,MODEL.as_array(),CAMERA.as_array()){halt(2);}
 let mut bounds:Buf<u16,13>=Buf::new([0;13]);let x=fix32::from_int(input[0u16] as i16);let z=fix32::from_int(input[1u16] as i16);let y=fix32::from_words(13056,0);
 gpu_bounds_set(bounds.as_array(),vec3::new(-x,-y,-z),vec3::new(x,y,z),5);
 if !gpu_instance_bounds(out,bounds.as_array()){halt(3);}halt(0x600d);
}"#;
    let code = compile(PROBE);
    let mut matrices = String::new();
    for case in super::cases() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code);
        cpu.physical_memory_mut()[0x5000..0x5002].copy_from_slice(&case.level[..2]);
        let r = cpu.run(100_000).unwrap();
        assert!(
            matches!(r, RunOutcome::Halted { signal: 0x600d, .. }),
            "{} {r:?}",
            case.name
        );
        matrices += &case.name;
        for w in &cpu.physical_memory_mut()[0x6000..0x602a] {
            matrices += &format!(" {w:04x}");
        }
        matrices.push('\n');
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bloxorz-batch");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("range-matrices.txt"), matrices).unwrap();
    tool(&out, &["range"]);
}
