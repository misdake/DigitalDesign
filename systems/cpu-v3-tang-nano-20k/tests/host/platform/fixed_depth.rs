//! Real public RCC submission on CPU instruction sim and functional GPU.
use cpu_v3::{rcc_backend, StepOutcome};

#[test]
fn public_triangle_keeps_fixed_d16_and_guards_in_both_profiles() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let mut outputs = Vec::new();
    for debug in [false, true] {
        let prefix = super::gpu_api_scene::SOURCE
            .split("fn main(){")
            .next()
            .unwrap();
        let prefix = prefix.replace(
            "let projection=Projection::from_horizontal_fov(fix32::from_int(1),fix32::from_int(100));camera.set_projection(projection);",
            "let projection=Projection::from_horizontal_fov(fix32::from_int(1),fix32::from_int(31));camera.set_projection(projection);");
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
            super::gpu_api_resources::parse(&source, &format!("fixed-depth-render-{debug}"), debug)
                .unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
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
        outputs[0], outputs[1],
        "debug profiles must keep fixed D16 outputs"
    );
    let (color, depth) = &outputs[0];
    assert!(color.iter().any(|&v| v != 0x1234));
    assert!(depth.iter().any(|&v| v != 65535));
    assert!(depth.iter().all(|&v| v == 65535 || v <= 31 * 256));
}
