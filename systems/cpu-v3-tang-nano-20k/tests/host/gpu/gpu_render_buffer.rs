//! Borrowed-plane ownership on actual RCC callers, with bounded control peers.
use super::*;

#[test]
fn texture_views_own_their_description_and_are_consumed_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let prefix = r#"mod gpu_api;use gpu_api::*;
fn consume_color(value:ColorBuffer){} fn consume_view(value:TextureView){}
fn main(){let b=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0xc000,2048)});
let view=b.texture_view();"#;
    let parse = |body: &str| {
        resolver::compile_explicit_gpu_program_checked(
            "RTT resource ownership",
            &format!("{prefix}{body}}}"),
            &options,
            &root
                .parent()
                .unwrap()
                .join("../../target/rtt/ownership-app"),
            &root,
            RENDERER_METADATA,
            &rcc::frontend::rustc_check::RustcCheck::new(
                root.parent().unwrap().join("../../target/rtt/ownership"),
            ),
            &mut |_| {},
        )
    };
    // The descriptor owns a copy; the caller still owns the physical allocation.
    for body in [
        "consume_color(b);consume_view(view);",
        "consume_view(view);consume_color(b);",
    ] {
        let positive = parse(body);
        assert!(positive.is_ok(), "{}", positive.err().unwrap_or_default());
    }
    for body in [
        "consume_view(view);consume_view(view);",
        "let fabricated=TextureView{slot:1};consume_view(fabricated);",
    ] {
        assert!(parse(body).is_err(), "accepted {body}");
    }
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
    let example = root.join("examples/demo_render_to_texture");
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
        let mut config = String::from("format 2\ngpu-api\napplication main.rs\n");
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
        let source = crate::shared_application_entry::select(&source, entry).unwrap();
        let ir = resolver::compile_explicit_gpu_program_checked(
            "RTT demo",
            &source,
            &options,
            &example.join("rcc"),
            &root.join("rcc"),
            &constants,
            &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
            &mut |_| {},
        )
        .unwrap();
        let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
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
