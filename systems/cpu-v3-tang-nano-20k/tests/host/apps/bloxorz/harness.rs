use super::*;

pub(super) trait SourcePatch {
    fn required_replace(&self, from: &str, to: &str) -> String;
}
impl SourcePatch for str {
    fn required_replace(&self, from: &str, to: &str) -> String {
        assert!(
            self.contains(from),
            "required source patch was not found: {from}"
        );
        self.replace(from, to)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CaptureProbe {
    Objects,
    Mesh,
}

pub(super) struct ApplicationCase {
    pub entry: &'static str,
    pub edit: Box<dyn FnOnce(String) -> String>,
    pub atlas: Option<(u16, u16)>,
    pub assets_from_sources: bool,
    pub capture: Option<CaptureProbe>,
    pub hook: bool,
    pub edit_modules: Option<fn(&Path, &Path)>,
}
impl Default for ApplicationCase {
    fn default() -> Self {
        Self {
            entry: "renderer_game_frames",
            edit: Box::new(|source| source),
            atlas: None,
            assets_from_sources: false,
            capture: None,
            hook: false,
            edit_modules: None,
        }
    }
}

pub(super) fn prepare_source(
    mut source: String,
    probe: Option<&str>,
    entry: &str,
    edit: Box<dyn FnOnce(String) -> String>,
) -> String {
    source = edit(source);
    if let Ok(alpha) = std::env::var("BLOXORZ_HINT_ALPHA") {
        assert!(["23", "60", "64", "96"].contains(&alpha.as_str()));
        assert_eq!(source.matches("color.a = 60;").count(), 1);
        source = source.replace("color.a = 60;", &format!("color.a = {alpha};"));
    }
    let entry = if let Some(body) = probe {
        source += &format!(
            "\nfn check(ok:bool){{if !ok{{halt(0xbad);}}}}\npub fn probe(){{{body}halt(0x600d);}}"
        );
        "probe"
    } else {
        entry
    };
    // Test-only readonly instrumentation uses the public getter, never arena slot layout.
    // Original marker12 is preserved for the independent input-transition trials.
    // Independent geometry fixtures may replace the entire pose function.
    // Observe retained native pose calls without requiring that implementation.
    source = source.replace(
        "block_pose(model);",
        "block_pose(model);world_probe(model);",
    );
    source += "\nfn world_probe(model:&Model){let world=model.world();let words:Buf<u16,24>=Buf::new([world.row0.x.lo_bits(),world.row0.x.hi_bits(),world.row0.y.lo_bits(),world.row0.y.hi_bits(),world.row0.z.lo_bits(),world.row0.z.hi_bits(),world.row0.w.lo_bits(),world.row0.w.hi_bits(),world.row1.x.lo_bits(),world.row1.x.hi_bits(),world.row1.y.lo_bits(),world.row1.y.hi_bits(),world.row1.z.lo_bits(),world.row1.z.hi_bits(),world.row1.w.lo_bits(),world.row1.w.hi_bits(),world.row2.x.lo_bits(),world.row2.x.hi_bits(),world.row2.y.lo_bits(),world.row2.y.hi_bits(),world.row2.z.lo_bits(),world.row2.z.hi_bits(),world.row2.w.lo_bits(),world.row2.w.hi_bits()]);signal(13,words.as_ptr().addr());}\n";
    crate::shared_application_entry::select(&source, entry).unwrap()
}

pub(super) fn application(
    name: &str,
    probe: Option<&str>,
) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>, PathBuf) {
    application_case(name, probe, None, ApplicationCase::default())
}

pub(super) fn application_case(
    name: &str,
    probe: Option<&str>,
    frozen_sources: Option<&Path>,
    case: ApplicationCase,
) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = frozen_sources.unwrap_or(root);
    let source_example = sources.join("examples/gpu_renderer_bloxorz");
    let example = if case.assets_from_sources {
        source_example.clone()
    } else {
        root.join("examples/gpu_renderer_bloxorz")
    };
    let out = std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT")
        .map_or_else(|| root.join("../../target/bloxorz-rtt"), PathBuf::from)
        .join(name);
    let mut producer = std::process::Command::new("python");
    producer
        .arg("-B")
        .arg(example.join("tools/assets.py"))
        .arg("--output")
        .arg(&out);
    if let Some((main, marker)) = case.atlas {
        producer.args([
            "--main-atlas-log2",
            &main.to_string(),
            "--marker-atlas-log2",
            &marker.to_string(),
        ]);
    }
    assert!(producer.status().unwrap().success());
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    assert_eq!(catalog["profile"], "production");
    let ui = std::fs::read(out.join("atlas.bin")).unwrap();
    for glyph in b"LEVEL DRAW MS".iter().copied().filter(|g| *g != b' ') {
        assert!(
            ui[usize::from(glyph) * 256..(usize::from(glyph) + 1) * 256]
                .iter()
                .any(|v| *v != 0),
            "HUD glyph{} is invisible",
            char::from(glyph)
        );
    }

    let mut resources = vec![];
    let mut config = String::from("format 2\ngpu-api\napplication main.rs\n");
    for r in catalog["resources"].as_array().unwrap() {
        let file = r["file"].as_str().unwrap();
        let base = r["address"].as_u64().unwrap() as usize;
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {base} {file}\n",
            file.trim_end_matches(".bin").to_uppercase()
        ));
        resources.push((base, std::fs::read(out.join(file)).unwrap()));
    }
    let constants = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config)
        .unwrap()
        .constants()
        + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    let source = std::fs::read_to_string(source_example.join("rcc/main.rs"))
        .unwrap()
        .replace("\r\n", "\n");
    let source = prepare_source(source, probe, case.entry, case.edit);
    std::fs::write(out.join("compiled-main.rs"), &source).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let mut dependencies = vec![];
    let profile = resolver::capture_profile::ColumnLayout;
    let capture = if case.capture.is_some() {
        let gate = out.join("capture-gate.rs");
        std::fs::write(
            &gate,
            r#"use crate::dsl_rt::*;mod gpu_api;use gpu_api::*;
pub fn profiler_gate(d:Array<u16>)->u16 {
 signal(5,d.as_ptr().addr());let mut r:u16=0;
 while r<d[36u16] {
  if gpu_capture_range_word(r,1)==4 && gpu_capture_range_word(r,3)<2 {
   let sector=(gpu_capture_range_word(r,2)>>7)|(gpu_capture_range_word(r,3)<<9);
   signal(6,sector);let mut i:u16=0;
   while i<40 {signal(7,gpu_capture_read_word(r,if i<16 {i}else{i+48},0));i+=1;}
  }
  r+=1;
 }
 if gpu_capture_error()!=0 {halt(0xca01);}0
}"#,
        )
        .unwrap();
        if case.capture == Some(CaptureProbe::Objects) {
            let text = std::fs::read_to_string(&gate).unwrap().replace(
                "gpu_capture_range_word(r,1)==4 && gpu_capture_range_word(r,3)<2",
                "gpu_capture_range_word(r,1)==2",
            );
            std::fs::write(&gate, text).unwrap();
        }
        let manifest = out.join("capture-manifest.json");
        std::fs::write(&manifest,serde_json::to_vec_pretty(&serde_json::json!({"version":3,"schema":"digitaldesign.gpu-capture-build","command_abi":8,"manifest_id":"0000000000000000000000000000000000000000000000000000000000000000","layout":profile.recorded_description(),"resources":[],"options":{"capture_allocation_guards":false}})).unwrap()).unwrap();
        resolver::RendererCaptureConfiguration::from_manifest(gate, &manifest).unwrap()
    } else if case.hook {
        resolver::RendererCaptureConfiguration::with_default_hook()
    } else {
        resolver::RendererCaptureConfiguration::without_capture()
    };
    let mut application_rcc = source_example.join("rcc");
    let mut ordinary_sources = vec![];
    if let Some(edit) = case.edit_modules {
        let local = out.join("instrumented-rcc");
        std::fs::create_dir_all(&local).unwrap();
        edit(&application_rcc, &local);
        ordinary_sources.push(application_rcc);
        application_rcc = local;
    }
    // Immutable accepted fixtures retain their original shared-source home.
    let rule_sources = if sources.join("applications/bloxorz/rcc").is_dir() {
        sources.join("applications/bloxorz/rcc")
    } else {
        sources.join("examples/bloxorz/rcc")
    };
    ordinary_sources.insert(0, rule_sources.clone());
    let ir = resolver::compile_explicit_gpu_program_checked_with_sources(
        "Bloxorz RTT",
        &source,
        &options,
        &application_rcc,
        &sources.join("rcc"),
        &constants,
        &ordinary_sources,
        &capture,
        &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
        &mut |p| dependencies.push(p.to_owned()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(dependencies.contains(&rule_sources.join("bloxorz_core.rs")));
    let frontend_static_words = ir.debug.static_data_end;
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    std::fs::write(
        out.join("budget.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"static_words":frontend_static_words}))
            .unwrap(),
    )
    .unwrap();
    (program, resources, out)
}

pub(super) fn check_swap_bridge_failure(level: u16, route: &[u16]) {
    let moves = route
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        r#"
session_init();level_reset({level});SESSION[0u16]=PLAY;TRANSITION[0u16]=0;SESSION[3u16]=0;
let route:Buf<u16,{count}>=Buf::new([{moves}]);
let mut saved:Buf<u16,10>=Buf::new([0;10]);let mut n:u16=0;
while n+1<{count} {{
    let mut j:u16=0;while j<10 {{saved[j]=GAME[j];j+=1;}}
    check(session_move(route[n]));
    if route[n]==6 {{
        check(GAME[4u16]==0 && SESSION[0u16]==PLAY && GAME[8u16]==(saved[8u16]^1));
        session_tick(32768,0,0);j=0;while j<10 {{check(GAME[j]==saved[j]);j+=1;}}
        check(session_move(6));
    }}
    let mut ticks:u16=0;while SESSION[0u16]!=PLAY && ticks<40 {{session_tick(0,0,50);ticks+=1;}}
    check(SESSION[0u16]==PLAY && GAME[4u16]==0);SESSION[3u16]=0;n+=1;
}}
let mut j:u16=0;while j<10 {{saved[j]=GAME[j];j+=1;}}
let before=block_transform();let supported=idle_cube_transform();
let generation=LEVEL_GENERATION[0u16];signal(6,1);
check(route[n]==6 && session_move(6));
check(GAME[4u16]==2 && GAME[9u16]==1 && SESSION[0u16]==FALL && !fading());signal(6,2);
let at=if saved[8u16]==0 {{GAME[1u16]*LEVEL[0u16]+GAME[0u16]}}else{{GAME[7u16]*LEVEL[0u16]+GAME[6u16]}};
check(level_field(at,0)==5 && GAME[3u16]&level_field(at,1)==0);
check(GAME[8u16]==(saved[8u16]^1) && MOTION[12u16]==saved[8u16]);
let falling=block_transform();let idle=idle_cube_transform();
check(falling.position.x==before.position.x && falling.position.y==before.position.y && falling.position.z==before.position.z);
check(falling.scale.x==before.scale.x && falling.scale.y==before.scale.y && falling.scale.z==before.scale.z);
check(idle.position.x==supported.position.x && idle.position.y==supported.position.y && idle.position.z==supported.position.z);
session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,50);
let dropped=block_transform();let idle=idle_cube_transform();
check(dropped.position.y<before.position.y && dropped.position.x==before.position.x && dropped.position.z==before.position.z);
check(idle.position.x==supported.position.x && idle.position.y==supported.position.y && idle.position.z==supported.position.z);signal(6,3);
// Undo before the fade restores both identities, mask and move count exactly.
session_tick(32768,0,0);j=0;while j<10 {{check(GAME[j]==saved[j]);j+=1;}}
check(SESSION[0u16]==PLAY && !fading());check(session_move(6));
n=0;while n<12 {{session_tick(0,0,50);n+=1;}}
check(SESSION[0u16]==FALL && fading() && GAME[4u16]==2);signal(6,4);
n=0;while TRANSITION[1u16]<320 && n<8 {{session_tick(0,0,50);n+=1;}}
check(TRANSITION[1u16]==320 && LEVEL_GENERATION[0u16]==generation && GAME[4u16]==2);
session_presented();check(TRANSITION[0u16]==HOLD && SESSION[0u16]==ENTRY_OPEN);
check(LEVEL_GENERATION[0u16]==generation+1 && MOTION[6u16]=={level} && GAME[4u16]==0 && GAME[5u16]==0);
n=0;while n<10 {{session_tick(0,0,50);n+=1;}}check(TRANSITION[1u16]==380);session_presented();
n=0;while n<12 {{session_tick(0,0,50);n+=1;}}
check(!fading() && SESSION[0u16]==PLAY && session_move(route[0u16]));signal(6,5);
"#,
        count = route.len()
    );
    let (program, resources, out) = application(
        &format!("swap-bridge-failure-level{}", level + 1),
        Some(&body),
    );
    std::fs::write(out.join("probe-body.rs"), &body).unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    let mut stage = 0;
    let mut steps = 0;
    let mut halted = false;
    for _ in 0..2_000_000 {
        steps += 1;
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 6 => stage = e.value,
            StepOutcome::Halted { signal } => {
                assert_eq!(
                    signal,
                    0x600d,
                    "level{} swap terminal stage{stage}",
                    level + 1
                );
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded swap terminal timeout");
    assert_eq!(stage, 5);
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"level":level+1,"route":route,"steps":steps,"max_steps":2000000,"correct_unsupported_cube_falls_supported_cube_stays":true,"ordinary_swap_and_full_undo_checked":true,"failure_undo_before_fade_checked":true,"reset_only_after_actual_presentation_callback":true,"new_entry_and_move_checked":true,"gpu_executed":false})).unwrap()).unwrap();
}

pub(super) fn write_display_ppm(path: &Path, pixels: &[u32]) {
    let mut ppm = b"P6\n400 240\n255\n".to_vec();
    for &p in pixels {
        ppm.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(path, ppm).unwrap();
}
