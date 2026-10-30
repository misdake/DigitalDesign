//! Borrowed-plane ownership on actual RCC callers, with bounded control peers.
use super::*;

#[test]
fn texture_views_keep_the_buffer_parent_and_are_consumed_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let prefix = r#"mod gpu_pipeline;fn main(){let mut r=renderer_open();
let mut memory:RenderBufferMemory=RenderBufferMemory{
color_base_low:0,color_base_high:0x60,color_capacity_low:0,color_capacity_high:4,
depth_base_low:0,depth_base_high:0x64,depth_capacity_low:0,depth_capacity_high:4};
let mut b=renderer_render_buffer(&mut r,view_of(&mut memory));
let mut view=render_buffer_texture(&mut b,false);"#;
    let parse = |body: &str| {
        resolver::compile_renderer_program_checked(
            "RTT resource ownership",
            &format!("{prefix}{body}}}"),
            &options,
            &root
                .parent()
                .unwrap()
                .join("../../target/rtt/ownership-app"),
            &root,
            RENDERER_METADATA,
            &resolver::RendererCaptureConfiguration::without_capture(
                resolver::RendererLayoutProfile::Column16,
            ),
            &rcc::frontend::rustc_check::RustcCheck::new(
                root.parent().unwrap().join("../../target/rtt/ownership"),
            ),
            &mut |_| {},
        )
    };
    let positive = parse("texture_view_close(view);render_buffer_close(b);renderer_close(r);");
    assert!(positive.is_ok(), "{}", positive.err().unwrap_or_default());
    for body in [
        "render_buffer_close(b);texture_view_close(view);",
        "let f=renderer_frame_to(&mut r,&mut b,0);texture_view_close(view);",
        "texture_view_close(view);texture_view_close(view);",
        "let fabricated=TextureView{word:1};texture_view_close(view);",
    ] {
        assert!(parse(body).is_err(), "accepted {body}");
    }
}

#[test]
fn actual_two_stage_application_uses_typed_views_and_captures_mutable_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/gpu_renderer_rtt");
    let out = root.join("../../target/gpu-renderer-rtt-validation");
    assert!(std::process::Command::new("python")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(example.join("tools/assets.py"))
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap()
        .success());
    let constants = std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    assert!(constants.contains("GPU_RENDERER_TEXTURE_SECTORS:u16=4;"));
    assert_eq!(
        std::fs::read(out.join("texture.bin")).unwrap(),
        vec![255; 512]
    );
    let gate = out.join("profiler_gate.rs");
    std::fs::write(&gate, r#"use crate::dsl_rt::*;mod gpu_pipeline;use gpu_pipeline::*;
mod gpu_capture_abi;use gpu_capture_abi::*;
static RTT_GATE_CALLS:u16=0;
static RTT_GATE_CHECKS:u16=0;
fn require(ok:bool){if !ok{halt(0xcb00+RTT_GATE_CHECKS);}unsafe{addr_of(&RTT_GATE_CHECKS).write(0,RTT_GATE_CHECKS+1);}}
pub fn profiler_gate(d:Array<u16>)->u16{
require(d[GPU_CAPTURE_H_LAYOUT_ABI]==2 && d[GPU_CAPTURE_H_COMMAND_ABI]==GPU_CAPTURE_RTT400_COMMAND_ABI);
let consumer=(RTT_GATE_CALLS&1)!=0;let mut sources:u16=0;let mut tails:u16=0;let mut i:u16=0;
while i<d[GPU_CAPTURE_H_RANGE_COUNT]{
 let tag=gpu_capture_range_word(i,GPU_CAPTURE_R_TAG);
 let flags=gpu_capture_range_word(i,GPU_CAPTURE_R_FLAGS);
 if tag==GPU_CAPTURE_TAG_TEXTURE && flags&GPU_CAPTURE_IMMUTABLE==0 {
   require(consumer && flags==(GPU_CAPTURE_READ|GPU_CAPTURE_PREIMAGE));
   require(gpu_capture_range_word(i,GPU_CAPTURE_R_BASE)==0 && gpu_capture_range_word(i,GPU_CAPTURE_R_BASE+1)==0x60);
   require(gpu_capture_range_word(i,GPU_CAPTURE_R_BYTES)==0 && gpu_capture_range_word(i,GPU_CAPTURE_R_BYTES+1)==4);
   require(gpu_capture_range_word(i,GPU_CAPTURE_R_GENERATION)==RTT_GATE_CALLS/2+1);
   require(gpu_capture_read_word(i,65535,1)==0xa55a);sources+=1;
 }
 if (tag==GPU_CAPTURE_TAG_COLOR || tag==GPU_CAPTURE_TAG_DEPTH) && flags&GPU_CAPTURE_FULL_CLEAR==0 {
   require(!consumer && flags==(GPU_CAPTURE_READ|GPU_CAPTURE_PREIMAGE));
   require(gpu_capture_range_word(i,GPU_CAPTURE_R_BYTES)==57344);tails+=1;
 }
 i+=1;
}
require(sources==if consumer{1}else{0});require(tails==if consumer{0}else{2});
require(gpu_capture_error()==0);signal(4,RTT_GATE_CALLS);
unsafe{addr_of(&RTT_GATE_CALLS).write(0,RTT_GATE_CALLS+1);}0
}"#).unwrap();
    let capture_gate = gate;
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let source = std::fs::read_to_string(example.join("rcc/main.rs")).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let mut boot =
        String::from("format 2\nrenderer\napplication examples/gpu_renderer_rtt/rcc/main.rs\n");
    for resource in manifest["resources"].as_array().unwrap() {
        let name = resource["file"].as_str().unwrap();
        boot.push_str(&format!(
            "copy GPU_RENDERER_{} {} {name}\n",
            name.trim_end_matches(".bin").to_uppercase(),
            resource["address"]
        ));
    }
    let project = cpu_v3_tang_nano_20k::boot::parse_boot_project(&boot).unwrap();
    let constants = project.constants() + &constants;
    let capture_manifest = out.join("capture-manifest.json");
    std::fs::write(
        &capture_manifest,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema":"digitaldesign.gpu-capture-build", "version":2, "command_abi":4,
            "manifest_id":"5254".repeat(16),
            "layout":resolver::RendererLayoutProfile::Column16.description(),
            "resources":manifest["resources"], "options":{}
        }))
        .unwrap(),
    )
    .unwrap();
    let configuration =
        resolver::RendererCaptureConfiguration::from_manifest(capture_gate, &capture_manifest)
            .unwrap();
    let ir = resolver::compile_renderer_program_checked(
        "actual RTT application",
        &source,
        &options,
        &example.join("rcc"),
        &root.join("rcc"),
        &constants,
        &configuration,
        &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let program = rcc_backend::try_compile(ir, &options, "renderer_rtt_two_updates").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for resource in manifest["resources"].as_array().unwrap() {
        let data = std::fs::read(out.join(resource["file"].as_str().unwrap())).unwrap();
        let base = resource["address"].as_u64().unwrap() as usize;
        for (index, byte) in data.iter().enumerate() {
            let address = base + index;
            let word = &mut cpu.physical_memory_mut()[address / 2];
            *word =
                (*word & !(255 << ((address & 1) * 8))) | (u16::from(*byte) << ((address & 1) * 8));
        }
    }
    cpu.physical_memory_mut()[0x600000 / 2..0x680000 / 2].fill(0xa55a);
    let state = attach(&mut cpu, Some(vec![vec![]; 4]));
    {
        let mut s = state.borrow_mut();
        s.automatic = true;
        s.capture_dynamic = true;
        s.targets = Some(vec![
            (0x600000, 0x640000),
            (0x440000, 0x480000),
            (0x600000, 0x640000),
            (0x400000, 0x480000),
        ]);
    }
    let fault_address = program
        .debug
        .globals
        .iter()
        .find_map(|g| match g.loc {
            rcc::VarLoc::Global(a) if g.name == "GPU_PIPE_FAULT" => Some(usize::from(a)),
            _ => None,
        })
        .unwrap();
    let mut gates = Vec::new();
    let mut halted = false;
    for _ in 0..12_000_000 {
        match cpu.step().unwrap() {
            cpu_v3::StepOutcome::Signaled(e) if e.signal_type == 4 => gates.push(e.value),
            cpu_v3::StepOutcome::Halted { signal } => {
                assert_eq!(
                    signal,
                    0x600d,
                    "pipeline fault {:?}",
                    &cpu.physical_memory_mut()[fault_address..fault_address + 5]
                );
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted);
    assert_eq!(gates, [0, 1, 2, 3]);
    // A control peer performs no raster writes: prove the CPU did not fake them.
    assert!(cpu.physical_memory_mut()[0x600000 / 2..0x680000 / 2]
        .iter()
        .all(|w| *w == 0xa55a));
    let s = state.borrow();
    assert_eq!((s.starts.len(), s.swaps), (4, 2));
    for (index, (_, words, _)) in s.starts.iter().enumerate() {
        let commands: Vec<u32> = words
            .as_chunks::<2>()
            .0
            .iter()
            .map(|w| u32::from(w[0]) | (u32::from(w[1]) << 16))
            .collect();
        assert_eq!(
            &commands[..3],
            &[Command::wait(true, true).0, Command::FLUSH.0, 0x90000000]
        );
        let mut local = vec![0u16; 1536];
        let mut render_words = Vec::new();
        for &command in &commands {
            match command >> 28 {
                1 => {
                    let object_base = if index % 2 == 0 { 0x4d0000 } else { 0x7f0000 };
                    let at = ((((command >> 10) & 0x3ffff) * 32) as usize - object_base) / 2;
                    let destination = (((command >> 3) & 127) * 16) as usize;
                    let count = ((32 << ((command >> 1) & 3)) / 2) as usize;
                    local[destination..destination + count]
                        .copy_from_slice(&s.object_snapshots[index][at..at + count]);
                }
                6 => {
                    let at = ((command & 127) * 16) as usize;
                    render_words.push(u32::from(local[at]) | (u32::from(local[at + 1]) << 16));
                }
                15 => break,
                _ => {}
            }
        }
        assert_eq!(
            render_words,
            if index % 2 == 0 {
                vec![0x00409c20]
            } else {
                vec![0x0089c000 | (((index / 2) as u32) << 21)]
            }
        );
    }
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    save_rtt_inputs(&out, &s, &program);
}

fn save_rtt_inputs(out: &Path, s: &State, program: &rcc_backend::CpuV3Program) {
    // Export actual recorded inputs for the RTL owner. Later jobs reload only
    // these input arenas; rendered surfaces must remain in retained GPU memory.
    std::fs::write(
        out.join("capture-caller.v3bin"),
        program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut submissions = Vec::new();
    for (index, (base, words, _)) in s.starts.iter().enumerate() {
        let command_file = format!("submission-{index}-commands.bin");
        let object_file = format!("submission-{index}-objects.bin");
        std::fs::write(
            out.join(&command_file),
            words
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(
            out.join(&object_file),
            s.object_snapshots[index]
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        submissions.push(serde_json::json!({
            "command_file":command_file,"object_file":object_file,
            "command_base_bytes":u32::from(*base)*128,
            "object_base_bytes":if index%2==0 {0x4d0000}else{0x7f0000},
            "valid_instructions":s.valid_command_lengths[index],
            "color_base_bytes":s.targets.as_ref().unwrap()[index].0,
            "depth_base_bytes":s.targets.as_ref().unwrap()[index].1,
        }));
    }
    std::fs::write(out.join("rtl-inputs.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "schema":1,"profile":"production","surface":"column-major-16x16-pad16",
        "source":"actual compiled public application under controlled GPU/Display peers",
        "caller_program":"capture-caller.v3bin",
        "status":"input-fixture-only-not-rtl-qualified",
        "initial_memory":"zero except immutable assets and 0xa55a words at 0x600000..0x680000",
        "reload_policy":"reload command/object arenas only between jobs; retain GPU-produced surfaces",
        "submissions":submissions,"assets":"assets.json",
    })).unwrap()).unwrap();
}

#[test]
fn borrowed_planes_preserve_display_and_hold_through_cancel_and_real_terminal() {
    let body = r#"
let mut r=renderer_open();
let mut memory:RenderBufferMemory=RenderBufferMemory{
 color_base_low:0,color_base_high:0x60,color_capacity_low:0,color_capacity_high:4,
 depth_base_low:0,depth_base_high:0x64,depth_capacity_low:0,depth_capacity_high:4};
let mut b=renderer_render_buffer(&mut r,view_of(&mut memory));let id=b.word;
check(id==1 && !gpu_rtt_busy(id) && gpu_rtt_buffer(id)[6u16]==0);
check(!render_buffer_wait(&mut r,&mut b,0));
let mut bad:RenderBufferMemory=memory;bad.color_base_high=0x40;
check(gpu_rtt_register(view_of(&mut bad))==0);
bad=memory;bad.color_base_high=0x4e;check(gpu_rtt_register(view_of(&mut bad))==0);
bad=memory;bad.color_base_high=0x4f;check(gpu_rtt_register(view_of(&mut bad))==0);
bad=memory;bad.color_base_high=0x7e;check(gpu_rtt_register(view_of(&mut bad))==0);
bad=memory;bad.color_capacity_low=65535;bad.color_capacity_high=2;
check(gpu_rtt_register(view_of(&mut bad))==0);
bad=memory;bad.color_capacity_high=2;check(gpu_rtt_register(view_of(&mut bad))==0);
bad=memory;bad.depth_base_high=0x60;check(gpu_rtt_register(view_of(&mut bad))==0);
check(gpu_rtt_register(view_of(&mut memory))==0);
{
let mut f=renderer_frame_to(&mut r,&mut b,0x1111);
let p=frame_ui_pass(&mut f);pass_complete(p);frame_complete(f);
}
check(!render_buffer_wait(&mut r,&mut b,0));
check(render_buffer_wait(&mut r,&mut b,60000));
check(gpu_rtt_buffer(id)[10u16]==1 && GPU_DISPLAY[2u16]==0);
check(dev_recv(7,1)==0);
check(gpu_rtt_view_begin(id));check(!gpu_rtt_write(id,2));
check(gpu_rtt_read(id,2));gpu_rtt_view_end(id);
check(gpu_rtt_busy(id) && !gpu_rtt_write(id,1));
check(gpu_rtt_retire(2,false));check(!gpu_rtt_busy(id));
{
let mut abandoned=renderer_frame_to(&mut r,&mut b,0);
let p=frame_ui_pass(&mut abandoned);pass_complete(p);
}
check(gpu_rtt_buffer(id)[6u16]==1 && gpu_rtt_buffer(id)[10u16]==1);
{
let mut f=renderer_frame(&mut r,0x1112);
let p=frame_ui_pass(&mut f);pass_complete(p);frame_complete(f);
}
{
let mut f=renderer_frame_to(&mut r,&mut b,0x1113);
let p=frame_ui_pass(&mut f);pass_complete(p);frame_complete(f);
}
check(!render_buffer_close(b));
check(gpu_rtt_buffer(id)[0u16]==1 && gpu_rtt_buffer(id)[1u16]==0);
check(gpu_rtt_register(view_of(&mut memory))==0);
renderer_close(r);
check(gpu_rtt_buffer(id)[0u16]==0 && gpu_rtt_buffer(id)[10u16]==2);
check(dev_recv(7,0)==3 && dev_recv(7,1)==1);
"#;
    let program = compile("", body);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    let state = attach(&mut cpu, Some(vec![vec![]; 3]));
    {
        let mut s = state.borrow_mut();
        s.automatic = true;
        s.capture_dynamic = true;
        s.targets = Some(vec![
            (0x600000, 0x640000),
            (0x440000, 0x480000),
            (0x600000, 0x640000),
        ]);
    }
    let outcome = cpu.run(8_000_000).unwrap();
    assert!(
        matches!(outcome, RunOutcome::Halted { signal: 0x600d, .. }),
        "{outcome:?}"
    );
    let s = state.borrow();
    assert_eq!((s.starts.len(), s.swaps, s.active), (3, 1, 0x440000 / 2));
    assert_eq!(s.offscreen_starts_pending, 1);
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
}

#[test]
fn actual_nearest_and_ordinary_bilinear_demo_inputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/gpu_renderer_rtt");
    for (label, entry) in [
        ("nearest", "renderer_rtt_nearest"),
        ("bilinear", "renderer_rtt_bilinear"),
    ] {
        let out = root.join("../../target/rtt/demos").join(label);
        assert!(std::process::Command::new("python")
            .args(["-B"])
            .arg(example.join("tools/assets.py"))
            .arg("--output")
            .arg(&out)
            .status()
            .unwrap()
            .success());
        let assets: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
        let mut config = String::from("format 2\nrenderer\napplication main.rs\n");
        for r in assets["resources"].as_array().unwrap() {
            let file = r["file"].as_str().unwrap();
            config.push_str(&format!(
                "copy GPU_RENDERER_{} {} {}\n",
                file.trim_end_matches(".bin").to_uppercase(),
                r["address"],
                file
            ));
        }
        let constants = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config)
            .unwrap()
            .constants()
            + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
        let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        let source = std::fs::read_to_string(example.join("rcc/main.rs")).unwrap();
        let ir = resolver::compile_renderer_program_checked(
            "RTT demo",
            &source,
            &options,
            &example.join("rcc"),
            &root.join("rcc"),
            &constants,
            &resolver::RendererCaptureConfiguration::without_capture(
                resolver::RendererLayoutProfile::Column16,
            ),
            &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
            &mut |_| {},
        )
        .unwrap();
        let program = rcc_backend::try_compile(ir, &options, entry).unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        for r in assets["resources"].as_array().unwrap() {
            let data = std::fs::read(out.join(r["file"].as_str().unwrap())).unwrap();
            let base = r["address"].as_u64().unwrap() as usize / 2;
            for (i, bytes) in data.as_chunks::<2>().0.iter().enumerate() {
                cpu.physical_memory_mut()[base + i] = u16::from_le_bytes(*bytes);
            }
        }
        cpu.physical_memory_mut()[0x600000 / 2..0x680000 / 2].fill(0xa55a);
        let state = attach(&mut cpu, Some(vec![vec![]; 2]));
        {
            let mut s = state.borrow_mut();
            s.automatic = true;
            s.capture_dynamic = true;
            s.targets = Some(vec![(0x600000, 0x640000), (0x440000, 0x480000)]);
        }
        let result = cpu.run(4_000_000).unwrap();
        assert!(
            matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
            "{label}: {result:?}"
        );
        let s = state.borrow();
        assert_eq!((s.starts.len(), s.swaps), (2, 1));
        assert!(cpu.physical_memory_mut()[0x600000 / 2..0x680000 / 2]
            .iter()
            .all(|v| *v == 0xa55a));
        save_rtt_inputs(&out, &s, &program);
    }
}

#[test]
fn sealed_buffers_cancel_the_recorded_slot_without_submission() {
    let body = r#"
let mut r=renderer_open();
let mut memory:RenderBufferMemory=RenderBufferMemory{
 color_base_low:0,color_base_high:0x60,color_capacity_low:0,color_capacity_high:4,
 depth_base_low:0,depth_base_high:0x64,depth_capacity_low:0,depth_capacity_high:4};
let mut b=r.render_buffer(view_of(&mut memory));let id=b.word;
{
 let mut f=r.frame_to(&mut b,0x1111);
 let p=frame_ui_pass(&mut f);p.finish();f.finish();
}
let slot=GPU_PIPE[2u16];
check(GPU_RENDERER[0u16]==6 && GPU_PIPE[1u16]==0 && GPU_RTT_WRITERS[slot-1]==id);
drop(r);
check(GPU_RTT_WRITERS[slot-1]==0 && !gpu_rtt_busy(id));
check(gpu_rtt_buffer(id)[6u16]==0);
check(b.close());
"#;
    let (_, state) = run(body);
    let s = state.borrow();
    assert!(s.starts.is_empty());
    assert_eq!((s.counts, s.maintenance, s.swaps), (0, 0, 0));
}
