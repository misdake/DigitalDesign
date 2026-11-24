//! Opt-in same-source compiler comparison; no production timing hooks.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn words_hash(words: &[u16]) -> String {
    hash(
        &words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}
fn rgb_hash(words: &[u32]) -> String {
    hash(
        &words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}
fn held(level: u16, frame: usize) -> u16 {
    if frame == 7 {
        if level == 1 {
            0xfffe
        } else {
            0xffdf
        }
    } else {
        0xffff
    }
}

fn compile(arm: &str, level: u16) {
    let count = if level == 1 { 7 } else { 8 };
    let automatic_inline = match std::env::var("BLOXORZ_CPU_AUTO_INLINE").as_deref() {
        Ok("0") => false,
        Ok("1") | Err(_) => true,
        value => panic!("BLOXORZ_CPU_AUTO_INLINE must be 0 or 1: {value:?}"),
    };
    let local_memory = match std::env::var("BLOXORZ_CPU_LOCAL_MEMORY").as_deref() {
        Ok("0") => false,
        Ok("1") | Err(_) => true,
        value => panic!("BLOXORZ_CPU_LOCAL_MEMORY must be 0 or 1: {value:?}"),
    };
    let case = ApplicationCase {
        observe_pose: false,
        automatic_inline: Some(automatic_inline),
        local_memory: Some(local_memory),
        edit: Box::new(|source| {
            // Gameplay now has its own function; capture the caller's frame
            // before entering it instead of referring to an out-of-scope local.
            source
                .required_replace(
                    "session_tick(buttons, held, elapsed);",
                    "session_tick(buttons, held, CPU_MATRIX_DT[0u16]);",
                )
                .required_replace(
                    "signal(4, number);",
                    "CPU_MATRIX_DT[0u16]=if number==7 {17u16}else{0u16};signal(4, number);",
                )
                + "\nstatic CPU_MATRIX_DT: Buf<u16,1> = Buf::new([0;1]);\n"
        }),
        ..ApplicationCase::default()
    };
    let (program, resources, out) = application_case(
        &format!("matrix-{arm}-level{level}"),
        Some(&format!("run({count},false,128);")),
        None,
        case,
    );
    let budget: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("budget.json")).unwrap()).unwrap();
    let code = program
        .words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect::<Vec<_>>();
    let image = boot::pack_application_resources(
        &code,
        budget["static_words"].as_u64().unwrap() as usize,
        &resources
            .iter()
            .map(|(p, b)| boot::BootResource::copy(*p, b.clone()))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(out.join("application.cb06"), &image).unwrap();
    std::fs::write(out.join("application.dbg"), program.debug.render()).unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let paths: Vec<PathBuf> =
        serde_json::from_slice(&std::fs::read(out.join("source-paths.json")).unwrap()).unwrap();
    let sources: BTreeMap<_, _> = paths
        .iter()
        .filter(|path| path.is_file())
        .map(|path| {
            (
                path.canonicalize()
                    .unwrap()
                    .strip_prefix(&repository)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                hash(&std::fs::read(path).unwrap()),
            )
        })
        .collect();
    let mut code_segments = vec![];
    for segment in &program.code_segments {
        let name = format!("application.cseg{}.dbg", segment.cseg);
        std::fs::write(out.join(&name), segment.debug.render()).unwrap();
        std::fs::write(
            out.join(format!("application.cseg{}.asm", segment.cseg)),
            &segment.listing,
        )
        .unwrap();
        code_segments.push(json!({"cseg":segment.cseg,"debug":name,"bytes":segment.words.len()*2}));
    }
    let functions: BTreeMap<_, _> = ["record_static_scene", "draw_shadow", "draw_shadow_hull"]
        .into_iter()
        .map(|name| {
            let f = program
                .debug
                .functions
                .iter()
                .find(|f| f.name == name)
                .expect("measured function must remain linked");
            (name, json!({"entry":f.addr.0,"frame_words":f.frame_size}))
        })
        .collect();
    let globals: BTreeMap<_, _> = [
        "GAME",
        "SESSION",
        "MOTION",
        "SHADOW_INFO",
        "BACKGROUND",
        "HUD",
    ]
    .into_iter()
    .map(|name| (name, global(&program, name)))
    .collect();
    let compiler: BTreeMap<_, _> = [
        "compiler/rcc/src/frontend/mod.rs",
        "compiler/rcc/src/compiler/builder.rs",
        "compiler/rcc/src/compiler/inline.rs",
        "compiler/rcc/src/compiler/passes.rs",
        "compiler/rcc/src/compiler/passes/local_memory.rs",
        "compiler/rcc/src/frontend/liveness.rs",
        "ip/cpu-v3/src/rcc_backend/inline.rs",
    ]
    .into_iter()
    .map(|name| (name, hash(&std::fs::read(repository.join(name)).unwrap())))
    .collect();
    let resource_hashes: Vec<_> = resources
        .iter()
        .map(|(address, bytes)| json!({"address":address,"bytes":bytes.len(),"sha256":hash(bytes)}))
        .collect();
    let metadata = json!({"arm":arm,"level":level,"frames":count,"package_sha256":hash(&image),"code_sha256":hash(&code),
        "main_sha256":hash(&std::fs::read(out.join("compiled-main.rs")).unwrap()),
        "producer_sha256":hash(&std::fs::read(out.join("renderer_assets.rs")).unwrap()),
        "sources":sources,"compiler":compiler,"functions":functions,"globals":globals,"resources":resource_hashes,
        "static_words":budget["static_words"],"pose_probe":false,"automatic_inline":automatic_inline,"local_memory":local_memory,"code_segments":code_segments,"total_code_bytes":program.total_code_words()*2,
        "time_protocol":"PLAY from producer, dt=0 except frame8=17ms; neutral through frame6; frame7 Select(level1) or Right(level29), release at frame8"});
    std::fs::write(
        out.join("profile-input.json"),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();
    eprintln!("Profile input: {}", out.display());
}

fn oracle(image: &[u8], level: u16, count: usize) -> Vec<serde_json::Value> {
    let mut cpu = CpuV3SystemSim::from_single_flash(image, 0, VBlankMode::PauseOnFrameIndexWait)
        .unwrap()
        .with_functional_gpu();
    cpu.publish_input(input(0xffff));
    let mut frames = vec![];
    for _ in 0..30_000_000 {
        if cpu.waiting_for_vblank() {
            cpu.advance_vblank();
        }
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 4 => {
                cpu.publish_input(input(held(level, usize::from(e.value) + 1)))
            }
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let active = cpu.display_state().active_base as usize;
                let memory = cpu.cpu().physical_memory_slice();
                frames.push(json!({"color_sha256":words_hash(&memory[active..active+102400]),
                    "depth_sha256":words_hash(&memory[0x480000/2..0x480000/2+102400]),"rgb_sha256":rgb_hash(&cpu.render_active_framebuffer())}));
                assert!(frames.len() <= count);
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                assert_eq!(frames.len(), count);
                assert!(cpu.gpu().last_error().is_none());
                return frames;
            }
            _ => {}
        }
    }
    panic!("bounded matrix instruction oracle");
}

struct Watch {
    name: String,
    entry: u16,
    active: Option<(u64, u16, usize)>,
}

// CPU V3's target ABI reserves r14 as LINK (r10 is callee-saved data).
const LINK_REGISTER: usize = 14;

fn run(out: &Path) {
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("profile-input.json")).unwrap()).unwrap();
    let level = metadata["level"].as_u64().unwrap() as u16;
    let count = metadata["frames"].as_u64().unwrap() as usize;
    let image = std::fs::read(out.join("application.cb06")).unwrap();
    assert_eq!(hash(&image), metadata["package_sha256"]);
    let expected = oracle(&image, level, count);
    let mut native =
        CpuV3SystemEmu::from_single_flash(&image, 0, VBlankMode::PauseOnFrameIndexWait)
            .unwrap()
            .with_functional_gpu();
    native.publish_input(input(0xffff));
    let mut watches: Vec<_> = metadata["functions"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, f)| Watch {
            name: name.clone(),
            entry: f["entry"].as_u64().unwrap() as u16,
            active: None,
        })
        .collect();
    let mut frame = 0;
    let mut begin = 0;
    let mut last = None;
    let mut spans = vec![];
    let mut frames = vec![];
    let mut submits = vec![];
    let debug =
        rcc::parse_debug(&std::fs::read_to_string(out.join("application.dbg")).unwrap()).unwrap();
    let mut bank_debug = BTreeMap::from([(7u16, debug)]);
    if let Some(segments) = metadata["code_segments"].as_array() {
        for s in segments {
            bank_debug.insert(
                s["cseg"].as_u64().unwrap() as u16,
                rcc::parse_debug(
                    &std::fs::read_to_string(out.join(s["debug"].as_str().unwrap())).unwrap(),
                )
                .unwrap(),
            );
        }
    }
    // Passive instruction observations do not alter the emulated workload.
    // Columns: DSEG3 writes, changed-page writes, DSEG3 reads, cache waits.
    let mut windows: BTreeMap<usize, BTreeMap<String, [u64; 4]>> = BTreeMap::new();
    let mut last_window_instruction = None;
    let wall = std::time::Instant::now();
    let mut minimum_sp = 0xc000u16;
    for _ in 0..60_000_000 {
        assert!(
            wall.elapsed().as_secs() < 600,
            "bounded matrix CPU emu host timeout"
        );
        if native.waiting_for_vblank() {
            native.advance_vblank();
        }
        let cycle = native.cycles();
        let mut window_write = None;
        {
            let core = native.core();
            let gprs = core.architectural_gprs();
            if gprs[13] != 0 {
                minimum_sp = minimum_sp.min(gprs[13]);
            }
            if let Some((segment, pc)) = core.executing_instruction_address() {
                let key = (core.retired_words(), segment, pc);
                let column = match cpu_v3::decode(
                    native.physical_memory()[(usize::from(segment) << 16) + usize::from(pc)],
                ) {
                    cpu_v3::Instruction::WriteDataSegment { index: Some(3), .. } => Some(0),
                    cpu_v3::Instruction::ReadSpecial {
                        special: cpu_v3::SpecialRegister::DataSegment3,
                        ..
                    } => Some(2),
                    cpu_v3::Instruction::DataCacheWait => Some(3),
                    _ => None,
                };
                if let Some(column) = column.filter(|_| last_window_instruction != Some(key)) {
                    last_window_instruction = Some(key);
                    let name = bank_debug
                        .get(&segment)
                        .expect("instruction CSEG debug")
                        .functions
                        .iter()
                        .find(|f| f.addr.0 <= usize::from(pc) && usize::from(pc) <= f.addr.1)
                        .expect("observed instruction must have a function")
                        .name
                        .clone();
                    windows
                        .entry(frame)
                        .or_default()
                        .entry(name.clone())
                        .or_default()[column] += 1;
                    if column == 0 {
                        window_write = Some((name, core.data_segments()[3]));
                    }
                }
                for watch in &mut watches {
                    if let Some((start, ret, ordinal)) = watch.active {
                        if segment == 7 && pc == ret {
                            spans.push(json!({"function":watch.name,"frame":ordinal,"start":start,"end":cycle,"cpu_cycles":cycle-start,"cpu_ms":(cycle-start) as f64/54000.0}));
                            watch.active = None;
                        }
                    }
                    if segment == 7 && pc == watch.entry && watch.active.is_none() {
                        watch.active = Some((cycle, gprs[LINK_REGISTER], frame));
                    }
                }
            }
        }
        let outcome = native
            .step_observing(|write, _| {
                if write.device == 4 && write.channel == 1 {
                    submits.push(json!({"frame":frame,"cycle":cycle,"sector":write.value}));
                }
                true
            })
            .unwrap()
            .unwrap();
        if let Some((name, previous)) = window_write {
            if native.core().data_segments()[3] != previous {
                windows.get_mut(&frame).unwrap().get_mut(&name).unwrap()[1] += 1;
            }
        }
        assert!(
            !outcome.fault && !outcome.halted,
            "native profile must reach presents before its finite exit"
        );
        let event = native.core().signal_event();
        if event != last {
            if let Some(e) = event {
                if e.signal_type == 4 {
                    frame = usize::from(e.value) + 1;
                    begin = native.cycles();
                    native.publish_input(input(held(level, frame)));
                }
                if e.signal_type == 12 {
                    let end = native.cycles();
                    let active = native.display_state().active_base as usize;
                    let memory = native.physical_memory();
                    let color = words_hash(&memory[active..active + 102400]);
                    let depth = words_hash(&memory[0x480000 / 2..0x480000 / 2 + 102400]);
                    drop(memory);
                    let rgb = native.render_active_framebuffer();
                    let rgb_hash = rgb_hash(&rgb);
                    assert_eq!(color, expected[frames.len()]["color_sha256"]);
                    assert_eq!(depth, expected[frames.len()]["depth_sha256"]);
                    assert_eq!(rgb_hash, expected[frames.len()]["rgb_sha256"]);
                    let state: BTreeMap<_, _> = [
                        ("GAME", 10),
                        ("SESSION", 9),
                        ("MOTION", 17),
                        ("SHADOW_INFO", 3),
                        ("BACKGROUND", 1),
                        ("HUD", 3),
                    ]
                    .into_iter()
                    .map(|(name, words)| {
                        let address = metadata["globals"][name].as_u64().unwrap() as usize;
                        (name, native.inspect_cpu_words(address, words).unwrap())
                    })
                    .collect();
                    if [1, 3, 7, 8].contains(&frame) {
                        write_display_ppm(&out.join(format!("frame-{frame}.ppm")), &rgb);
                    }
                    frames.push(json!({"frame":frame,"start":begin,"end":end,"cpu_cycles":end-begin,"cpu_ms":(end-begin) as f64/54000.0,
                        "color_sha256":color,"depth_sha256":depth,"rgb_sha256":rgb_hash,"state":state}));
                    std::fs::write(out.join("progress.json"),serde_json::to_vec_pretty(&json!({"frame":frame,"cycles":end,"host_seconds":wall.elapsed().as_secs_f64()})).unwrap()).unwrap();
                    eprintln!(
                        "matrix {} level{level} frame{frame}: {} cycles",
                        metadata["arm"],
                        end - begin
                    );
                    if frames.len() == count {
                        assert!(native.gpu().last_error().is_none());
                        assert!(watches.iter().all(|watch| watch.active.is_none()));
                        assert!(minimum_sp >= 0xa000);
                        for ordinal in 1..=count {
                            assert_eq!(
                                spans
                                    .iter()
                                    .filter(|span| span["function"] == "draw_shadow"
                                        && span["frame"] == ordinal)
                                    .count(),
                                1,
                                "one complete shadow observation per frame"
                            );
                        }
                        assert_eq!(
                            spans
                                .iter()
                                .filter(|span| span["function"] == "record_static_scene")
                                .count(),
                            if level == 1 { 2 } else { 1 }
                        );
                        assert!(spans
                            .iter()
                            .all(|span| span["function"] != "draw_shadow_hull"
                                || !(3..=6).contains(&(span["frame"].as_u64().unwrap() as usize))));
                        if level == 29 {
                            assert_eq!(frames.last().unwrap()["state"]["SESSION"][0], 1);
                            assert_eq!(frames.last().unwrap()["state"]["SESSION"][1], 17);
                            assert_eq!(frames.last().unwrap()["state"]["GAME"][5], 1);
                            assert_eq!(
                                spans
                                    .iter()
                                    .filter(|span| span["function"] == "draw_shadow_hull"
                                        && span["frame"] == 8)
                                    .count(),
                                1
                            );
                        } else {
                            assert_eq!(frames.last().unwrap()["state"]["BACKGROUND"][0], 0);
                        }
                        let result = json!({"input":metadata,"frames":frames,"functions":spans,"submissions":submits,"minimum_sp":minimum_sp,
                            "host_seconds":wall.elapsed().as_secs_f64(),"cycles":native.cycles(),"oracle_equal":true,
                            "scope":"54MHz CPU/cache/native memory emu + synchronous functional GPU; preloaded CB06, cold caches; frame signal4 to signal12, init reset to first signal12; excludes physical GPU, shared contention, SPI installation and scanout; function spans first Execute at entry to first Execute at saved r14 return"});
                        std::fs::write(
                            out.join("cpu-profile.json"),
                            serde_json::to_vec_pretty(&result).unwrap(),
                        )
                        .unwrap();
                        std::fs::write(out.join("window-counts.json"), serde_json::to_vec_pretty(
                            &json!({"columns":["writes","page_changes","reads","cache_waits"],"frames":windows})
                        ).unwrap()).unwrap();
                        return;
                    }
                }
            }
            last = event;
        }
    }
    panic!("bounded matrix CPU emu cycle limit");
}

#[test]
#[ignore = "explicit same-migrated-source CPU emu/functional GPU compiler comparison"]
fn compiler_cpu_matrix() {
    let arm = std::env::var("BLOXORZ_CPU_ARM").expect("explicit compiler arm");
    let level = std::env::var("BLOXORZ_START_LEVEL")
        .unwrap()
        .parse::<u16>()
        .unwrap();
    assert!([1, 29].contains(&level));
    assert_eq!(std::env::var("BLOXORZ_START_PHASE").unwrap(), "play");
    let root =
        PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit output root"));
    match std::env::var("BLOXORZ_CPU_MATRIX_MODE").unwrap().as_str() {
        "compile" => compile(&arm, level),
        "run" => run(&root.join(format!("matrix-{arm}-level{level}"))),
        mode => panic!("unknown matrix mode {mode}"),
    }
}
