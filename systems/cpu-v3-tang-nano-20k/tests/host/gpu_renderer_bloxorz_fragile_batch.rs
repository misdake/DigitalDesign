//! Bounded actual RCC heap builder and reversible native-index publication.
use super::{application, preload};
use cpu_v3::StepOutcome;

fn packed(words: &[u16], at: usize) -> u64 {
    (0..4).fold(0, |a, i| a | (u64::from(words[at + i]) << (i * 16)))
}
fn triangle(words: &[u16], slot: usize, t: usize) -> [usize; 3] {
    let offset = 64 + usize::from(words[1] >> 8) * 4;
    let t = slot * 10 + t;
    let bits = packed(words, offset + t / 3 * 4) >> (18 * (t % 3));
    [
        (bits & 63) as usize,
        ((bits >> 6) & 63) as usize,
        ((bits >> 12) & 63) as usize,
    ]
}

#[test]
fn all_stage_fragile_vertices_and_every_cell_index_hide_restore_match_native_oracle() {
    let body = r#"
let owner=fragile_begin();let mut level:u16=0;
while level<33 {
 level_reset(level);fragile_rebuild(owner);signal(1,level);signal(2,owner.addr());
 let mut cell:u16=0;
 while cell<LEVEL[6u16] {
  if level_field(cell,0)==2 {
   FRACTURE[0u16]=cell;SESSION[0u16]=CRACK;fragile_sync(owner);signal(3,cell);
   FRACTURE[0u16]=65535;SESSION[0u16]=PLAY;fragile_sync(owner);signal(4,cell);
  }
  cell+=1;
 }
 level+=1;
}

fragile_end(owner);
"#;
    let (program, resources, out) = application("fragile-all-stage-builder", Some(body));
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let archive = std::fs::read(out.join("blox_levels.bin")).unwrap();
    let archive: Vec<u16> = archive
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect();
    let mut cpu = cpu_v3::CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    let mut level = 0;
    let mut context = [0u16; 8];
    let mut original = Vec::new();
    let mut map = Vec::new();
    let mut stages = Vec::new();
    let mut hidden = 0;
    let mut restored = 0;
    let mut halted = false;
    for _ in 0..40_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 1 => level = usize::from(e.value),
            StepOutcome::Signaled(e) if e.signal_type == 2 => {
                let m = cpu.physical_memory_slice();
                context.copy_from_slice(&m[usize::from(e.value)..usize::from(e.value) + 8]);
                let offset = usize::from(archive[4 + level * 16]);
                let width = usize::from(archive[offset]);
                let depth = usize::from(archive[offset + 1]);
                let cells = usize::from(archive[offset + 6]);
                let expected: Vec<_> = (0..cells)
                    .filter(|i| archive[offset + 8 + i * 2] & 7 == 2)
                    .collect();
                assert_eq!(usize::from(context[6]), expected.len());
                if expected.is_empty() {
                    assert_eq!(
                        (context[0], context[1], context[2], context[3], context[7]),
                        (0, 0, 0, 0, 0)
                    );
                    continue;
                }
                let begin = usize::from(context[1]);
                original = m[begin..begin + usize::from(context[2]) * 64].to_vec();
                map = m[usize::from(context[7])..usize::from(context[7]) + expected.len() * 2]
                    .to_vec();
                let mut mapped = map
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| usize::from(p[0]))
                    .collect::<Vec<_>>();
                mapped.sort_unstable();
                assert_eq!(mapped, expected);
                for pair in map.as_chunks::<2>().0.iter() {
                    let cell = usize::from(pair[0]);
                    let record = usize::from(pair[1] >> 2) * 64;
                    let slot = usize::from(pair[1] & 3);
                    let words = &m[record..];
                    let v = usize::from(words[0] & 127);
                    let t = usize::from(words[0] >> 8);
                    assert!((20..=60).contains(&v) && v % 20 == 0 && t == v / 2);
                    assert!(slot < v / 20);
                    let index = 64 + usize::from(words[1] >> 8) * 4;
                    for qword in 0..t.div_ceil(3) {
                        let live = (t - qword * 3).min(3) * 18;
                        assert_eq!(
                            packed(words, index + qword * 4) >> live,
                            0,
                            "padding/unused triangle fields"
                        );
                    }
                    let x = cell % width;
                    let z = cell / width;
                    let variant = (x * 3 + z * 5) % 4;
                    let first = catalog["catalog"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|entry| entry["name"] == "MARKED_FRAGILE")
                        .unwrap()["first"]
                        .as_u64()
                        .unwrap() as usize;
                    let reference = &catalog["meshes"][first + variant];
                    let u32at = |i| u32::from(words[i]) | (u32::from(words[i + 1]) << 16);
                    for vertex in 0..20 {
                        let at = 64 + (slot * 20 + vertex) * 6;
                        let p = u32at(at);
                        let old = reference["vertices"][vertex].as_array().unwrap();
                        let q = old[0].as_u64().unwrap() as u32;
                        for axis in 0..3 {
                            let actual = i64::from(u32at(8 + axis * 2) as i32)
                                + i64::from((p >> (10 * axis)) & 1023) * 512;
                            let translation = if axis == 0 {
                                (2 * x as i64 - width as i64 + 1) * 65536
                            } else if axis == 2 {
                                (2 * z as i64 - depth as i64 + 1) * 65536
                            } else {
                                0
                            };
                            let wanted = reference["config"][axis].as_i64().unwrap()
                                + i64::from((q >> (10 * axis)) & 1023) * 512
                                + translation;
                            assert_eq!(
                                actual,
                                wanted,
                                "stage{} cell{cell} vertex{vertex} axis{axis}",
                                level + 1
                            );
                        }
                        assert_eq!(u64::from(u32at(at + 2)), old[1].as_u64().unwrap());
                        assert_eq!(u64::from(u32at(at + 4)), old[2].as_u64().unwrap());
                    }
                    for tri in 0..10 {
                        let want = reference["triangles"][tri]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|i| i.as_u64().unwrap() as usize + slot * 20)
                            .collect::<Vec<_>>();
                        assert_eq!(triangle(words, slot, tri).as_slice(), want);
                    }
                }
                stages.push(serde_json::json!({"level":level+1,"cells":expected.len(),"meshlets":context[3],"gpu_storage_sectors":context[2],"owner_word":context[0],"directory_word":context[1]}));
                if level == 3 {
                    assert_eq!((context[6], context[3]), (33, 13));
                }
            }
            StepOutcome::Signaled(e) if e.signal_type == 3 => {
                let m = cpu.physical_memory_slice();
                let begin = usize::from(context[1]);
                let pair = map
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .find(|p| p[0] == e.value)
                    .unwrap();
                let at = usize::from(pair[1] >> 2) * 64;
                let slot = usize::from(pair[1] & 3);
                let mut restored_copy = m[begin..begin + original.len()].to_vec();
                let words = &m[at..];
                for tri in 0..10 {
                    assert_eq!(triangle(words, slot, tri), [0; 3]);
                }
                // Restore only the target triangle bits in an independent u64 decoder.
                let index = at - begin + 64 + usize::from(words[1] >> 8) * 4;
                for tri in slot * 10..slot * 10 + 10 {
                    let q = index + tri / 3 * 4;
                    let shift = tri % 3 * 18;
                    let mask = ((1u64 << 18) - 1) << shift;
                    let value = (packed(&restored_copy, q) & !mask) | (packed(&original, q) & mask);
                    for i in 0..4 {
                        restored_copy[q + i] = (value >> (16 * i)) as u16;
                    }
                }
                assert_eq!(
                    restored_copy, original,
                    "non-target geometry/directory/padding must remain byte-exact"
                );
                hidden += 1;
            }
            StepOutcome::Signaled(e) if e.signal_type == 4 => {
                let m = cpu.physical_memory_slice();
                let begin = usize::from(context[1]);
                assert_eq!(&m[begin..begin + original.len()], original);
                restored += 1;
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded40M all-stage fragile builder timeout");
    assert_eq!((hidden, restored), (153, 153));
    std::fs::write(out.join("builder-qualification.json"), serde_json::to_vec_pretty(&serde_json::json!({"stages":stages,"all_cells_hidden_and_restored":hidden,"max_steps":40000000,"mode":"instruction CPU/native wire decode, not GPU raster or CPU timing"})).unwrap()).unwrap();
}

#[test]
fn retained_cb_refetches_changed_mesh_indices_and_degenerates_cover_no_pixels() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let body = r#"
session_init();level_reset(3);LEVEL[0u16]=1;LEVEL[1u16]=1;LEVEL[6u16]=1;LEVEL[7u16]=2;LEVEL[8u16]=2;
let owner=fragile_begin();fragile_rebuild(owner);
let mut gpu=Gpu::init(demo_config());let mut commands=gpu.command_buffer();
let color=demo_color_b();let depth=demo_depth();let camera=game_camera();let state=game_solid_state();
let model=game_floor_model();let material=demo_material(demo_white(),true);let texture=marker_texture();let view=texture.view();let mesh=fragile_mesh(owner);
{let mut pass=commands.pass(&color,&depth,ColorLoad::clear(Rgb565::from_raw(0)),DepthLoad::clear(65535));pass.set_camera(&camera);pass.set_state(&state);pass.draw(&mesh,&model,&material,&view);}
let mut n:u16=0;while n<3 {
 FRACTURE[0u16]=if n==1 {0u16}else{65535u16};SESSION[0u16]=if n==1 {CRACK}else{PLAY};
 fragile_sync(owner);gpu.submit(commands);commands=gpu.wait();signal(2,n);n+=1;
}
drop(commands);fragile_end(owner);
"#;
    let (program, resources, out) = application("fragile-mesh-cache-refetch", Some(body));
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut frames = Vec::new();
    let mut payloads = Vec::new();
    let mut count = 0usize;
    let mut halted = false;
    for _ in 0..8_000_000 {
        let result = sim
            .step_observing(|w, m| {
                if w.device == 4 && w.channel == 0 {
                    count = usize::from(w.value);
                }
                if w.device == 4 && w.channel == 1 {
                    let base = usize::from(w.value) * 64;
                    let code = m[base..base + (count + 1) * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|w| u32::from(w[0]) | (u32::from(w[1]) << 16))
                        .collect::<Vec<_>>();
                    let records = code
                        .iter()
                        .filter(|c| **c >> 28 == 2)
                        .map(|c| usize::try_from((c >> 12) & 65535).unwrap() * 64)
                        .collect::<Vec<_>>();
                    assert_eq!(records.len(), 1);
                    payloads.push((code, m[records[0]..records[0] + 256].to_vec()));
                }
                true
            })
            .unwrap()
            .unwrap();
        match result {
            StepOutcome::Signaled(e) if e.signal_type == 2 => {
                assert_eq!(usize::from(e.value), frames.len());
                frames.push(
                    sim.cpu().physical_memory_slice()[0x440000 / 2..0x440000 / 2 + 102400].to_vec(),
                );
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded8M retained mesh replay timeout");
    assert!(sim.gpu().last_error().is_none());
    assert_eq!((frames.len(), payloads.len()), (3, 3));
    assert!(frames[0].iter().filter(|p| **p != 0).count() > 10);
    assert!(
        frames[1].iter().all(|p| *p == 0),
        "all ten degenerate triangles must cover zero pixels"
    );
    assert_eq!(frames[0], frames[2]);
    assert_eq!(
        payloads[0].0, payloads[1].0,
        "retained command bytes unchanged"
    );
    assert_eq!(payloads[1].0, payloads[2].0);
    assert_ne!(payloads[0].1, payloads[1].1);
    assert_eq!(payloads[0].1, payloads[2].1);
    for (i, frame) in frames.iter().enumerate() {
        std::fs::write(
            out.join(format!("replay-{i}-color.bin")),
            frame
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    std::fs::write(out.join("refetch-qualification.json"), serde_json::to_vec_pretty(&serde_json::json!({"mode":"instruction CPU + functional GPU, retained actual DMA_MESH command; no RTL/board claim","draws":3,"mesh_address_unchanged":true,"command_bytes_unchanged":true,"hidden_pixels":0,"restore_pixels_exact":true,"max_steps":8000000})).unwrap()).unwrap();
}

fn published(memory: &[u16], base: u16, count: u16) -> Vec<u16> {
    let begin = usize::from(base) * 64;
    let code = &memory[begin..begin + (usize::from(count) + 1) * 2];
    let mut result = code.to_vec();
    for pair in code.as_chunks::<2>().0.iter() {
        let c = u32::from(pair[0]) | (u32::from(pair[1]) << 16);
        if c >> 28 == 1 {
            let at = ((c >> 10) & 0x3ffff) as usize * 16;
            let words = 16usize << ((c >> 1) & 3);
            result.extend_from_slice(&memory[at..at + words]);
        } else if c >> 28 == 2 {
            let at = ((c >> 12) & 65535) as usize * 64;
            let words = (((c >> 1) & 7) as usize + 2) * 64;
            result.extend_from_slice(&memory[at..at + words]);
        }
    }
    result
}

#[test]
#[ignore = "bounded accepted-vs-candidate level4 PLAY native CPU cost, functional GPU; no GPU cycle/FPS claim"]
fn level4_play_native_cpu_cost_and_instruction_oracles() {
    use super::{application_from_sources, global, write_display_ppm};
    use cpu_v3_tang_nano_20k::{
        boot,
        input_device::{ControllerSnapshot, InputMode},
        system_emu::CpuV3SystemEmu,
        system_sim::{CpuV3SystemSim, VBlankMode},
    };
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
        process::Command,
    };
    const BASE: &str = "de42d82d6437743664e14187c8b8edb796cb1080";
    let root =
        PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit output root"));
    std::fs::create_dir_all(&root).unwrap();
    let accepted = root.join("accepted-de42-source");
    if !accepted.exists() {
        let zip = root.join("accepted-de42-source.zip");
        assert!(Command::new("git")
            .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .args(["archive", "--format=zip", "--output"])
            .arg(&zip)
            .arg(BASE)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("python").args(["-B", "-c", "import pathlib,sys,zipfile; d=pathlib.Path(sys.argv[2]); d.mkdir(); zipfile.ZipFile(sys.argv[1]).extractall(d)"]).arg(&zip).arg(&accepted).status().unwrap().success());
    }
    let accepted = accepted.join("systems/cpu-v3-tang-nano-20k");
    let input = ControllerSnapshot {
        mode: InputMode::Analog,
        buttons_active_low: 65535,
        left: [128; 2],
        right: [128; 2],
    };
    let mut arm_frames = Vec::new();
    let mut costs = Vec::new();
    for (arm, sources) in [("accepted", Some(accepted.as_path())), ("candidate", None)] {
        if arm == "accepted" {
            if let Some(previous) = std::env::var_os("BLOXORZ_REUSE_ACCEPTED_COST") {
                let previous = PathBuf::from(previous);
                let report: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(previous.join("level4-comparison.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(report["accepted_source"], BASE);
                assert_eq!(
                    report["arms"][0]["native_publication_and_full_images_match_instruction"],
                    true
                );
                let folder = previous.join("fragile-accepted-play-cpu-cost-six-frames");
                let frames = (0..6)
                    .map(|i| {
                        let ppm = std::fs::read(folder.join(format!("frame-{i}.ppm"))).unwrap();
                        assert!(ppm.starts_with(b"P6\n400 240\n255\n"));
                        let rgb = ppm[15..]
                            .as_chunks::<3>()
                            .0
                            .iter()
                            .map(|c| {
                                (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])
                            })
                            .collect::<Vec<_>>();
                        let read = |label| {
                            std::fs::read(folder.join(format!("frame-{i}-{label}.bin")))
                                .unwrap()
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|w| u16::from_le_bytes([w[0], w[1]]))
                                .collect::<Vec<_>>()
                        };
                        assert_eq!(rgb.len(), 96000);
                        (rgb, read("color"), read("depth"))
                    })
                    .collect::<Vec<_>>();
                let mut cost = report["arms"][0].clone();
                cost["reused_from"] = serde_json::json!(previous);
                costs.push(cost);
                arm_frames.push(frames);
                continue;
            }
        }
        let (program, resources, out) = application_from_sources(
            &format!("fragile-{arm}-play-cpu-cost-six-frames"),
            Some("run(6,false,128);"),
            sources,
        );
        let code: Vec<u8> = program.words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let budget: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("budget.json")).unwrap()).unwrap();
        let copies = resources
            .iter()
            .map(|(a, b)| boot::BootResource::copy(*a, b.clone()))
            .collect::<Vec<_>>();
        let package = boot::pack_application_resources(
            &code,
            budget["static_words"].as_u64().unwrap() as usize,
            &copies,
        )
        .unwrap();
        std::fs::write(out.join("cpu-v3-boot.bin"), &package).unwrap();
        let mut sim =
            CpuV3SystemSim::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
                .unwrap()
                .with_functional_gpu();
        sim.publish_input(Some(input));
        let starts = program
            .debug
            .functions
            .iter()
            .filter(|f| {
                [
                    "gpu_api_draw",
                    "gpu_scene_model_init",
                    "gpu_scene_matrix_prepare",
                    "fragile_sync",
                ]
                .contains(&f.name.as_str())
            })
            .map(|f| (f.addr.0 as u16, f.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut calls = BTreeMap::<String, usize>::new();
        let mut frames = Vec::new();
        let mut publications = Vec::new();
        let mut count = 0;
        let mut frame_draws = Vec::new();
        let mut draws = 0;
        let mut in_frame = false;
        let mut halted = false;
        for _ in 0..24_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            if in_frame {
                if let Some(name) = starts.get(&sim.cpu().pc()) {
                    *calls.entry(name.clone()).or_default() += 1;
                }
            }
            let step = sim
                .step_observing(|w, m| {
                    if w.device == 4 && w.channel == 0 {
                        count = w.value;
                    }
                    if w.device == 4 && w.channel == 1 {
                        let at = usize::from(w.value) * 64;
                        draws += m[at..at + (usize::from(count) + 1) * 2]
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .filter(|c| c[1] >> 12 == 3)
                            .count();
                        publications.push(published(m, w.value, count));
                    }
                    true
                })
                .unwrap()
                .unwrap();
            match step {
                StepOutcome::Signaled(e) if e.signal_type == 4 => {
                    calls.clear();
                    draws = 0;
                    in_frame = true;
                }
                StepOutcome::Signaled(e) if e.signal_type == 12 => {
                    let m = sim.cpu().physical_memory_slice();
                    let at = sim.display_state().active_base as usize;
                    let rgb = sim.render_active_framebuffer();
                    write_display_ppm(&out.join(format!("frame-{}.ppm", frames.len())), &rgb);
                    let color = m[at..at + 102400].to_vec();
                    let depth = m[0x480000 / 2..0x480000 / 2 + 102400].to_vec();
                    for (label, words) in [("color", &color), ("depth", &depth)] {
                        std::fs::write(
                            out.join(format!("frame-{}-{label}.bin", frames.len())),
                            words
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                    }
                    frames.push((rgb, color, depth));
                    frame_draws.push(serde_json::json!({"calls":calls,"native_draws":draws}));
                    in_frame = false;
                }
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d, "{arm}");
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(halted, "bounded24M instruction {arm}");
        assert_eq!(frames.len(), 6);
        assert!(sim.gpu().last_error().is_none());
        let instruction_memory = sim.cpu().physical_memory_slice();
        let heap_begin = usize::from(instruction_memory[global(&program, "HEAP_BEGIN")]);
        let heap_end = usize::from(instruction_memory[global(&program, "HEAP_END")]);
        assert_eq!(
            instruction_memory[heap_begin],
            0x8000 | ((heap_end - heap_begin) as u16)
        );
        let mut native =
            CpuV3SystemEmu::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
                .unwrap()
                .with_functional_gpu();
        native.publish_input(Some(input));
        let wall = std::time::Instant::now();
        let mut last = None;
        let mut begin = 0;
        let mut first = None;
        let mut cycles = Vec::new();
        let mut submission = 0;
        let mut min_sp = 0xc000u16;
        let mut done = false;
        for _ in 0..32_000_000 {
            assert!(wall.elapsed().as_secs() < 240, "bounded240s native {arm}");
            if native.waiting_for_vblank() {
                native.advance_vblank();
            }
            let step = native
                .step_observing(|w, m| {
                    if w.device == 4 && w.channel == 0 {
                        count = w.value;
                    }
                    if w.device == 4 && w.channel == 1 {
                        assert_eq!(
                            published(m, w.value, count),
                            publications[submission],
                            "native publication {arm}/{submission}"
                        );
                        submission += 1;
                    }
                    true
                })
                .unwrap()
                .unwrap();
            assert!(!step.fault);
            let core = native.core();
            let sp = core.architectural_gprs()[13];
            if sp != 0 {
                min_sp = min_sp.min(sp);
            }
            let event = core.signal_event();
            drop(core);
            if event != last {
                if let Some(e) = event {
                    if e.signal_type == 4 {
                        begin = native.cycles();
                        first.get_or_insert(begin);
                    }
                    if e.signal_type == 12 {
                        let i = cycles.len();
                        assert_eq!(
                            native.render_active_framebuffer(),
                            frames[i].0,
                            "native RGB {arm}/{i}"
                        );
                        let m = native.physical_memory();
                        let at = native.display_state().active_base as usize;
                        assert_eq!(&m[at..at + 102400], frames[i].1);
                        assert_eq!(&m[0x480000 / 2..0x480000 / 2 + 102400], frames[i].2);
                        cycles.push(native.cycles() - begin);
                        std::fs::write(out.join("native-progress.json"), serde_json::to_vec_pretty(&serde_json::json!({"arm":arm,"completed_frame_cycles":cycles,"cold_setup_cycles":first,"source":if arm=="accepted"{BASE}else{"candidate"}})).unwrap()).unwrap();
                    }
                }
                last = event;
            }
            if step.halted {
                assert_eq!(step.halt_signal, 0x600d);
                done = true;
                break;
            }
        }
        assert!(done, "bounded32M native {arm}");
        assert_eq!(cycles.len(), 6);
        assert_eq!(submission, publications.len());
        assert!(usize::from(min_sp) >= heap_end);
        costs.push(serde_json::json!({"arm":arm,"source":if arm=="accepted"{BASE}else{"candidate worktree snapshot, see compiled-main and source manifest"},"cold_setup_cycles":first,"frame_cycles":cycles,"function_calls_and_native_draws":frame_draws,"minimum_SP_word":min_sp,"heap_end_exclusive_word":heap_end,"whole_heap_released":true,"native_publication_and_full_images_match_instruction":true,"code_bytes":code.len(),"static_words":budget["static_words"]}));
        arm_frames.push(frames);
    }
    let differences=arm_frames[0].iter().zip(&arm_frames[1]).enumerate().map(|(i,(a,b))|serde_json::json!({"frame":i,"display_different_pixels":a.0.iter().zip(&b.0).filter(|(x,y)|x!=y).count(),"color_different_words":a.1.iter().zip(&b.1).filter(|(x,y)|x!=y).count(),"depth_different_words":a.2.iter().zip(&b.2).filter(|(x,y)|x!=y).count(),"semantic":if i==3||i==4{"candidate removes fractured tile immediately; original CRACK/FALL retained"}else{"same pose/source attributes; new matrix grouping may change raster rounding"}})).collect::<Vec<_>>();
    std::fs::write(root.join("level4-comparison.json"),serde_json::to_vec_pretty(&serde_json::json!({"accepted_source":BASE,"mode":"CPU native/cycle emu with synchronous functional GPU; no physical GPU/scanout/contention timing","workload":"6 actual ordinary frames, first3 stationary PLAY with visible shadow, then CRACK/FALL/undo; neutral analog, stats off, zero elapsed except fixed real session actions","max_instruction_steps":24000000,"max_native_steps":32000000,"arms":costs,"differences":differences,"per_optimization_net_attribution":false})).unwrap()).unwrap();
    for i in [1, 2] {
        assert!(
            costs[1]["frame_cycles"][i].as_u64().unwrap()
                < costs[0]["frame_cycles"][i].as_u64().unwrap(),
            "warm combined CPU cost must decrease"
        );
    }
}

#[test]
fn inflight_mesh_patch_rebuild_and_free_stop_before_mutation() {
    use cpu_v3::Device;
    use std::{any::Any, cell::Cell, rc::Rc};
    struct Busy(Rc<Cell<bool>>);
    impl Device for Busy {
        fn read(&mut self, _: &mut [u16], _: u8) -> u16 {
            u16::from(self.0.get())
        }
        fn write(&mut self, _: &mut [u16], _: u8, _: u16) {}
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    for action in [
        "fragile_sync(owner);",
        "fragile_rebuild(owner);",
        "fragile_end(owner);",
    ] {
        let body=format!("level_reset(3);let owner=fragile_begin();fragile_rebuild(owner);signal(2,owner.addr());FRACTURE[0u16]=5;SESSION[0u16]=CRACK;{action}");
        let (program, resources, _) = application(
            &format!("fragile-busy-{}", action.split('(').next().unwrap()),
            Some(&body),
        );
        let busy = Rc::new(Cell::new(false));
        let mut cpu = cpu_v3::CpuV3Sim::default();
        cpu.attach_device(4, Box::new(Busy(busy.clone())));
        cpu.set_boot_window(&program.words);
        preload(cpu.physical_memory_mut(), &resources);
        let mut before = Vec::new();
        let mut owner = 0;
        let mut halted = false;
        for _ in 0..4_000_000 {
            match cpu.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 2 => {
                    owner = usize::from(e.value);
                    before = cpu.physical_memory_slice()[8192..40959].to_vec();
                    busy.set(true);
                }
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0xfb01);
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(halted, "bounded4M {action}");
        let m = cpu.physical_memory_slice();
        // Stack-free heap state, including allocator and both owners, is unchanged.
        assert_eq!(
            &m[8192..40959],
            before,
            "inflight {action} must not write/free geometry or heap"
        );
        assert_ne!(m[owner], 0);
    }
}

#[test]
fn representative_mixed_material_goal_and_level_rebuilds_render_against_accepted() {
    use super::{application_from_sources, write_display_ppm};
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };
    let root = PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit root"));
    std::fs::create_dir_all(&root).unwrap();
    let accepted = root.join("accepted-de42-source");
    if !accepted.exists() {
        let zip = root.join("accepted.zip");
        assert!(Command::new("git")
            .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .args(["archive", "--format=zip", "--output"])
            .arg(&zip)
            .arg("de42d82d6437743664e14187c8b8edb796cb1080")
            .status()
            .unwrap()
            .success());
        assert!(Command::new("python").args(["-B","-c","import pathlib,sys,zipfile; d=pathlib.Path(sys.argv[2]);d.mkdir();zipfile.ZipFile(sys.argv[1]).extractall(d)"]).arg(&zip).arg(&accepted).status().unwrap().success());
    }
    let accepted = accepted.join("systems/cpu-v3-tang-nano-20k");
    let mut arms = Vec::new();
    for (arm, sources) in [("accepted", Some(accepted.as_path())), ("candidate", None)] {
        let (program, resources, out) = application_from_sources(
            &format!("fragile-{arm}-representative-cpu-cost-scenes"),
            Some("run(5,false,128);"),
            sources,
        );
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut frames = Vec::new();
        let mut halted = false;
        for _ in 0..40_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            match sim.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 12 => {
                    let rgb = sim.render_active_framebuffer();
                    assert!(rgb.iter().filter(|p| **p != rgb[0]).count() > 1000);
                    write_display_ppm(&out.join(format!("frame-{}.ppm", frames.len())), &rgb);
                    let at = sim.display_state().active_base as usize;
                    let m = sim.cpu().physical_memory_slice();
                    let color = m[at..at + 102400].to_vec();
                    let depth = m[0x480000 / 2..0x480000 / 2 + 102400].to_vec();
                    for (label, words) in [("color", &color), ("depth", &depth)] {
                        std::fs::write(
                            out.join(format!("frame-{}-{label}.bin", frames.len())),
                            words
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                    }
                    frames.push((rgb, color, depth));
                }
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d, "{arm}");
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(halted, "bounded40M representative {arm}");
        assert_eq!(frames.len(), 5);
        assert!(sim.gpu().last_error().is_none());
        arms.push(frames);
    }
    let stages = [8, 15, 17, 30, 1];
    let differences=arms[0].iter().zip(&arms[1]).enumerate().map(|(i,(a,b))|serde_json::json!({"level":stages[i],"RGB_different_pixels":a.0.iter().zip(&b.0).filter(|(x,y)|x!=y).count(),"color_different_words":a.1.iter().zip(&b.1).filter(|(x,y)|x!=y).count(),"depth_different_words":a.2.iter().zip(&b.2).filter(|(x,y)|x!=y).count()})).collect::<Vec<_>>();
    std::fs::write(root.join("representative-differences.json"),serde_json::to_vec_pretty(&serde_json::json!({"stages":stages,"mode":"CPU instruction + functional GPU, full images retained; no exact pixel equivalence assumed","differences":differences,"max_steps_per_arm":40000000})).unwrap()).unwrap();
}

#[test]
fn ordinary_restart_next_stage_shadow_and_finite_heap_lifetimes_close() {
    use super::global;
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let (program, resources, out) =
        application("fragile-flow-restart-next", Some("run(8,false,128);"));
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let fracture = global(&program, "FRACTURE");
    let shadow = global(&program, "SHADOW_BATCH_BASE");
    let mut context = 0;
    let mut frames = Vec::new();
    let mut starts = 0;
    let mut halted = false;
    for _ in 0..24_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        let step = sim
            .step_observing(|w, _| {
                if w.device == 4 && w.channel == 1 {
                    starts += 1;
                }
                true
            })
            .unwrap()
            .unwrap();
        match step {
            StepOutcome::Signaled(e) if e.signal_type == 14 => context = usize::from(e.value),
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let m = sim.cpu().physical_memory_slice();
                let c = &m[context..context + 8];
                let n = frames.len();
                assert_eq!(c[3], if n < 6 { 13 } else { 0 });
                assert_eq!(c[5], if n == 1 || n == 2 { 5 } else { 65535 });
                if n == 1 {
                    let at = usize::from(m[shadow]) + 64;
                    assert_eq!(m[at + 31] & 4, 4);
                    assert_eq!(m[at + 31] >> 3, 5);
                }
                if n >= 3 {
                    assert_eq!(m[fracture], 65535);
                }
                frames.push(serde_json::json!({"frame":n,"context":c,"fracture":m[fracture],"session":e.value}));
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded24M ordinary restart/next timeout");
    assert_eq!(frames.len(), 8);
    // Initial level, crack hide, undo restore and next level each rebuild once.
    assert_eq!(starts, 20);
    assert!(sim.gpu().last_error().is_none());
    let m = sim.cpu().physical_memory_slice();
    let begin = usize::from(m[global(&program, "HEAP_BEGIN")]);
    let end = usize::from(m[global(&program, "HEAP_END")]);
    assert_eq!(m[begin], 0x8000 | ((end - begin) as u16));
    assert_eq!(m[end - 1], m[begin]);
    std::fs::write(out.join("lifetime-qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"mode":"actual ordinary caller with fixed input and fast-forwarded veil timer endpoints; CPU instruction + functional GPU","frames":frames,"gpu_starts":starts,"whole_heap_released":true,"max_steps":24000000,"restart_and_next_use_real_opaque_present_reset":true})).unwrap()).unwrap();
}

#[test]
fn compact_ground_normal_basis_matches_original_axial_inverse_transpose() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let body = r#"
session_init();level_reset(3);let owner=ground_begin();ground_rebuild();
let mut gpu=Gpu::init(demo_config());let mut commands=gpu.command_buffer();let color=demo_color_b();let depth=demo_depth();let camera=game_camera();let state=game_solid_state();let mut model=game_floor_model();let mut material=demo_material(demo_white(),false);let texture=demo_texture();let view=texture.view();
{let mut pass=commands.pass(&color,&depth,ColorLoad::clear(Rgb565::from_raw(0)),DepthLoad::clear(65535));pass.set_camera(&camera);pass.set_state(&state);draw_ground_batches(&mut pass,&mut model,&mut material);let half=fix32::from_words(32768,0);model.set_pose(demo_pose(demo_zero(),demo_zero(),demo_point(half,half,half)));let mesh=ground_mesh(0);pass.draw(&mesh,&model,&material,&view);}
gpu.submit(commands);commands=gpu.wait();drop(commands);ground_end(owner);
"#;
    let (program, resources, out) = application("fragile-ground-normal-basis", Some(body));
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut count = 0;
    let mut instances = Vec::new();
    let mut halted = false;
    for _ in 0..8_000_000 {
        if let StepOutcome::Halted { signal } = sim
            .step_observing(|w, m| {
                if w.device == 4 && w.channel == 0 {
                    count = w.value;
                }
                if w.device == 4 && w.channel == 1 {
                    let a = usize::from(w.value) * 64;
                    for pair in m[a..a + (usize::from(count) + 1) * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                    {
                        let c = u32::from(pair[0]) | (u32::from(pair[1]) << 16);
                        if c >> 28 == 1 && (c >> 3) & 127 == 80 {
                            let a = ((c >> 10) & 0x3ffff) as usize * 16;
                            instances.push(m[a..a + 64].to_vec());
                        }
                    }
                }
                true
            })
            .unwrap()
            .unwrap()
        {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted, "bounded8M normal-basis timeout");
    assert!(sim.gpu().last_error().is_none());
    assert_eq!(instances.len(), 2);
    assert_eq!(
        &instances[0][24..42],
        &instances[1][24..42],
        "pre-normalized axial inverse transpose must match accepted half-scale normal matrix"
    );
    let n = instances[0][24..42]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| (u32::from(v[0]) | (u32::from(v[1]) << 16)) as i32)
        .collect::<Vec<_>>();
    for a in &n {
        assert!(a.abs() <= 131072);
        for sign in [-1i64, 1] {
            let v = (i64::from(*a) * 511 * 128 * sign) >> 16;
            assert!(
                (-131072..131072).contains(&v),
                "normal18 axial MAC result must not wrap"
            );
        }
    }
    std::fs::write(out.join("normal-basis-qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"mode":"actual CPU-produced native Instance128, functional GPU","compact_normal_matrix_F16":n,"original_half_scale_matrix_exact":true,"all_axial_normal18_inputs_in_range":true,"max_steps":8000000})).unwrap()).unwrap();
}
