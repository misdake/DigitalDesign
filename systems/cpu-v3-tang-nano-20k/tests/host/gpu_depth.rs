//! Bounded independent encoding oracle and CPU-only content lifetime contracts.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::Path;

fn run(source: &str, label: &str, debug: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = std::path::PathBuf::from(std::env::var_os("RCC_RUSTC_TEST_DIR").unwrap())
        .join(format!("depth-{label}-{debug}"));
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = debug;
    let parsed = compile_program_named_checked(
        "depth-contract",
        source,
        &options,
        &RustcCheck::new(out),
        &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for _ in 0..1_000_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(
                signal,
                0x600d,
                "{label}, check {}",
                signal.wrapping_sub(0xd000)
            );
            return;
        }
    }
    panic!("depth contract exceeded one million instructions: {label}");
}

const CHECK: &str = "static STEP:u16=0;fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}";

#[test]
fn actual_public_triangle_keeps_color_and_recovers_depth_bits_in_functional_gpu() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let mut outputs = Vec::new();
    for shift in [0, 2] {
        let prefix = super::gpu_api_scene::SOURCE
            .split("fn main(){")
            .next()
            .unwrap();
        let prefix = prefix.replace(
            "let projection=Projection::from_horizontal_fov(fix32::from_int(1),fix32::from_int(100));camera.set_projection(projection);",
            &format!("let mut projection=Projection::from_horizontal_fov(fix32::from_int(1),fix32::from_int(31));check(projection.set_depth_shift({shift}));camera.set_projection(projection);"));
        let source = format!(
            r#"{prefix}
fn main(){{
 let a:CommandMemory=CommandMemory{{commands:unsafe{{GpuMemory::from_sectors(0x9800,512)}},push:unsafe{{GpuMemory::from_sectors(0x9a00,512)}}}};
 let absent:Option<CommandMemory>=None;let mut gpu=Gpu::init(GpuConfig{{object_arena_bytes:8192,command_a:a,command_b:absent}});
 let mut cb=gpu.command_buffer();let camera=make_camera();let model=make_model();let mut material=make_material();material.set_unlit(true);let state=make_state();
 let color=ColorBuffer::from_memory(unsafe{{GpuMemory::from_sectors(0x8000,2048)}});
 let depth=DepthBuffer::from_memory(unsafe{{GpuMemory::from_sectors(0x9000,2048)}});
 let mesh=Mesh::from_native(MeshDesc{{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:unsafe{{GpuMemory::from_sectors(0xa000,16)}}}});
 {{let mut pass=cb.pass(&color,&depth,ColorLoad::clear(Rgb565::from_raw(0x1234)),DepthLoad::clear(65535));pass.set_camera(&camera);pass.set_state(&state);pass.draw_colored(&mesh,&model,&material);}}
 gpu.submit(cb);cb=gpu.wait();halt(0x600d);
}}"#
        );
        let parsed =
            super::gpu_api_resources::parse(&source, &format!("depth-render-{shift}"), false)
                .unwrap();
        let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        let memory = sim.cpu_mut().physical_memory_mut();
        for first in [0x8000, 0x9000] {
            memory[first * 64..(first + 2048) * 64].fill(0xa55a);
        }
        memory[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
        let header = [
            3u32 | (1 << 8) | (20 << 16) | (5 << 24),
            0,
            16,
            0,
            (-65536i32) as u32,
            (-65536i32) as u32,
            (-67i32) as u32,
            8,
        ];
        for (i, word) in header.into_iter().enumerate() {
            memory[0xa001 * 64 + i * 2..0xa001 * 64 + i * 2 + 2]
                .copy_from_slice(&[word as u16, (word >> 16) as u16]);
        }
        let payload = [
            0u32,
            511 << 20,
            0xffffff,
            512,
            511 << 20,
            0xffffff,
            256 | (512 << 10),
            511 << 20,
            0xffffff,
            0,
            (1 << 6) | (2 << 12),
            0,
        ];
        for (i, word) in payload.into_iter().enumerate() {
            memory[0xa002 * 64 + i * 2..0xa002 * 64 + i * 2 + 2]
                .copy_from_slice(&[word as u16, (word >> 16) as u16]);
        }
        let mut halted = false;
        for _ in 0..1_000_000 {
            if let StepOutcome::Halted { signal } = sim.step().unwrap() {
                assert_eq!(
                    signal,
                    0x600d,
                    "functional GPU: {:?}",
                    sim.gpu().last_error()
                );
                halted = true;
                break;
            }
        }
        assert!(halted, "one-triangle CPU/functional-GPU bound exceeded");
        assert!(sim.gpu().last_error().is_none());
        let memory = sim.cpu_mut().physical_memory_mut();
        for first in [0x8000, 0x9000] {
            assert!(memory[first * 64 + 102400..(first + 2048) * 64]
                .iter()
                .all(|&word| word == 0xa55a));
        }
        outputs.push((
            memory[0x8000 * 64..0x8000 * 64 + 102400].to_vec(),
            memory[0x9000 * 64..0x9000 * 64 + 102400].to_vec(),
        ));
    }
    assert_eq!(
        outputs[0].0, outputs[1].0,
        "shift must preserve the color plane"
    );
    let mut covered = 0;
    let mut recovered = 0;
    for (&coarse, &fine) in outputs[0].1.iter().zip(&outputs[1].1) {
        if coarse == 65535 {
            assert_eq!(fine, 65535);
        } else {
            covered += 1;
            assert_eq!(fine >> 2, coarse);
            recovered += usize::from(fine > (coarse << 2));
        }
    }
    assert!(
        covered > 0 && recovered > 0,
        "triangle must render with recovered fractional depth"
    );
}

#[test]
fn projection_shift_reaches_the_production_recorded_global_in_both_profiles() {
    let original = "let projection=Projection::from_horizontal_fov(fix32::from_int(1),fix32::from_int(100));camera.set_projection(projection);";
    assert_eq!(super::gpu_api_scene::SOURCE.matches(original).count(), 1);
    for debug in [false, true] {
        for shift in 1..=3 {
            let replacement = format!("let mut projection=Projection::from_horizontal_fov(fix32::from_int(1),fix32::from_int(31));check(projection.depth_shift()==0);check(projection.set_depth_shift({shift}));check(!projection.set_depth_shift(4));check(projection.depth_shift()=={shift});camera.set_projection(projection);");
            let source = super::gpu_api_scene::SOURCE.replace(original, &replacement);
            let parsed =
                super::gpu_api_resources::parse(&source, &format!("depth-scene-{shift}"), debug)
                    .unwrap();
            let record = match parsed
                .debug
                .globals
                .iter()
                .find(|g| g.name == "GPU_CB_RECORDS")
                .unwrap()
                .loc
            {
                rcc::VarLoc::Global(address) => usize::from(address),
                _ => unreachable!(),
            };
            let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
            options.gpu_debug = debug;
            let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&program.words);
            cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
            let mut halted = false;
            for _ in 0..1_000_000 {
                if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                    assert_eq!(signal, 0x600d, "profile {debug}, shift {shift}");
                    halted = true;
                    break;
                }
            }
            assert!(
                halted,
                "production depth scene exceeded one million instructions"
            );
            let memory = cpu.physical_memory_mut();
            let encoded = ((31u32 << 16) << shift) | (shift << 24);
            assert_eq!(
                &memory[record + 80..record + 82],
                &[encoded as u16, (encoded >> 16) as u16]
            );
            let push_at = 0x9a00 * 64 + usize::from(memory[record + 23] - 1);
            assert_eq!(
                &memory[push_at..push_at + 2],
                &[encoded as u16, (encoded >> 16) as u16]
            );
            assert_ne!(
                memory[record + 8] & 64,
                0,
                "shift capability requirement was lost"
            );
        }
    }
}

#[test]
fn far_encoding_matches_full_width_boundary_oracle() {
    let mut source = format!("mod gpu_depth;{CHECK}fn main(){{");
    for shift in 0..=4u32 {
        let threshold = 0xffff00u64 >> shift;
        for raw in [
            0,
            1,
            255,
            65535,
            63 * 65536,
            threshold - 1,
            threshold,
            threshold + 1,
            200 * 65536 - 1,
            200 * 65536,
        ] {
            let valid = shift <= 3 && raw > 0 && raw < 200 * 65536 && (raw << shift) < 0xffff00;
            source += &format!(
                "check({}gpu_depth_valid({},{},{shift}));",
                if valid { "" } else { "!" },
                raw & 65535,
                raw >> 16
            );
            if valid {
                let encoded = (raw << shift) | (u64::from(shift) << 24);
                source += &format!("{{let e=gpu_depth_encode({},{},{shift});check(e.low=={}&&e.high=={});check(gpu_depth_native_valid(e.low,e.high));}}",
                    raw & 65535, raw >> 16, encoded & 65535, encoded >> 16);
            }
        }
    }
    source += "check(!gpu_depth_native_valid(0,0));check(!gpu_depth_native_valid(1,512));check(!gpu_depth_native_valid(0,1024));halt(0x600d);}";
    run(&source, "encoding", false);
}

#[test]
fn content_requires_completed_clear_or_matching_key_and_survives_descriptor_reuse() {
    let source = format!(
        r#"mod gpu_record;mod gpu_arena;mod gpu_schema;const GPU_KIND_DEPTH:u16=9;{CHECK}
fn surface(base:u16)->u16{{let h=gpu_arena_alloc(false,GPU_KIND_DEPTH);check(h!=0);let mut d=gpu_arena_data(h);d[0u16]=base;d[1u16]=2048;h}}
fn main(){{
 check(gpu_arena_init(8192));gpu_record_init();
 let a=surface(0x9000);check(gpu_depth_surface_register(a));
 let alias=surface(0x9001);check(!gpu_depth_surface_register(alias));check(gpu_arena_free(alias,false,GPU_KIND_DEPTH));
 let b=surface(0xa000);check(gpu_depth_surface_register(b));
 let reference=GPU_DEPTH_REF;
 let load2=reference|3|16|32|8;let load1=reference|3|16|32|4;
 check(!gpu_depth_input_compatible(load2,0x9000,2048));
 let clear=reference|3|64;gpu_depth_content_start(clear,0x9000,2048);
 check(!gpu_depth_input_compatible(load2,0x9000,2048));
 gpu_depth_content_complete(clear,0x9000,2048);
 check(gpu_depth_input_compatible(load2,0x9000,2048));check(gpu_depth_input_compatible(load1,0x9000,2048));
 gpu_depth_content_start(load2,0x9000,2048);check(!gpu_depth_input_compatible(load2,0x9000,2048));
 gpu_depth_content_complete(load2,0x9000,2048);
 check(gpu_depth_input_compatible(load2,0x9000,2048));check(!gpu_depth_input_compatible(load1,0x9000,2048));
 let exact=surface(0x9000);check(gpu_depth_surface_register(exact));check(gpu_arena_data(exact)[2u16]==3);
 check(gpu_depth_capture_shift(load2,0x9000,2048)==2);
 // Two independently recorded CBs share only committed physical contents.
 let mut first=gpu_record_data(0);first[GPU_RECORD_FACT_COUNT]=1;first[GPU_RECORD_DEPTH]=0x9000;
 let mut facts=gpu_record_capture_facts(0);facts[0u16]=0x9000;facts[1u16]=2048;facts[2u16]=GPU_FACT_DEPTH;facts[3u16]=reference|3;
 check(gpu_record_depth_format(0,2));check(!gpu_record_depth_format(0,1));check(gpu_record_depth_prepare(0));
 let mut second=gpu_record_data(1);second[GPU_RECORD_FACT_COUNT]=1;second[GPU_RECORD_DEPTH]=0x9000;
 let mut other=gpu_record_capture_facts(1);other[0u16]=0x9000;other[1u16]=2048;other[2u16]=GPU_FACT_DEPTH;other[3u16]=reference|3;
 check(gpu_record_depth_format(1,1));check(!gpu_record_depth_prepare(1));
 // Clearing before geometry permits a new key after successful completion.
 other[3u16]=reference|3;gpu_record_depth_clear(1);check(gpu_record_depth_format(1,1));check(gpu_record_depth_prepare(1));
 gpu_record_depth_start(1);check(!gpu_record_depth_prepare(0));gpu_record_depth_complete(1);
 check(!gpu_record_depth_prepare(0));check(gpu_record_depth_prepare(1));
 // Restore k2 for the failure/retirement checks below.
 gpu_depth_content_start(load2,0x9000,2048);gpu_depth_content_complete(load2,0x9000,2048);
 // A descriptor created after START cannot inherit that old writer's proof.
 gpu_depth_content_start(load2,0x9000,2048);let late=surface(0x9000);check(gpu_depth_surface_register(late));
 gpu_depth_content_complete(load2,0x9000,2048);check(!gpu_depth_input_compatible(load2,0x9000,2048));
 check(gpu_depth_surface_unregister(late));check(gpu_arena_free(late,false,GPU_KIND_DEPTH));
 // A failed writer has no completion call and leaves contents unknown.
 gpu_depth_content_start(load2,0x9000,2048);check(!gpu_depth_input_compatible(load2,0x9000,2048));
 check(gpu_depth_surface_unregister(a));check(gpu_arena_free(a,false,GPU_KIND_DEPTH));
 let reused=surface(0xb000);check(reused==a);check(gpu_depth_surface_register(reused));
 gpu_depth_content_complete(load2,0x9000,2048);
 check(!gpu_depth_input_compatible(load2,0xb000,2048));
 check(gpu_depth_input_compatible(load2,0x9000,2048));check(gpu_arena_data(exact)[2u16]==3);
 check(gpu_depth_surface_unregister(b));check(gpu_depth_surface_unregister(reused));
 check(gpu_depth_surface_unregister(exact));
 let fresh=surface(0x9000);check(gpu_depth_surface_register(fresh));gpu_depth_content_complete(load2,0x9000,2048);
 check(!gpu_depth_input_compatible(load2,0x9000,2048));check(gpu_depth_surface_unregister(fresh));
 check(!gpu_depth_surface_unregister(reused));
 halt(0x600d);
}}"#
    );
    for debug in [false, true] {
        run(&source, "lifetime", debug);
    }
}
