//! Public borrowed-resource forwarding used by application UI formatting.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
#[test]
fn borrowed_grid_forwarding_writes_actual_external_bytes() {
    for forwarded in [false, true] {
        let body = if forwarded {
            "write_text(&mut grid);"
        } else {
            "grid.write(CellRect{x:0,y:0,width:8,height:1},b\"OBS \");"
        };
        let source = format!(
            r#"mod gpu_api;use gpu_api::*;
fn write_text(grid: &mut UiGrid){{grid.write(CellRect{{x:0,y:0,width:8,height:1}},b"OBS ");}}
fn main(){{
let absent:Option<CommandMemory>=None;
let a:CommandMemory=CommandMemory{{commands:unsafe{{GpuMemory::from_sectors(0x9800,512)}},push:unsafe{{GpuMemory::from_sectors(0x9a00,512)}}}};
let gpu=Gpu::init(GpuConfig{{object_arena_bytes:8192,command_a:a,command_b:absent}});
let atlas=UiAtlas::from_native(unsafe{{GpuMemory::from_sectors(0xa000,512)}},255);
let mut grid=UiGrid::from_memory(unsafe{{GpuMemory::from_sectors(0xb100,8)}},&atlas);
grid.fill(255);{body}drop(grid);drop(atlas);drop(gpu);halt(0x600d);}}"#
        );
        let parsed =
            super::gpu_api_resources::parse(&source, &format!("forwarded-{forwarded}"), false)
                .unwrap_or_else(|error| panic!("{error}"));
        let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let mut halted = false;
        for _ in 0..1_000_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded UI forwarding timeout");
        assert_eq!(
            &cpu.physical_memory_mut()[0xb100 * 64..0xb100 * 64 + 4],
            &[0x424f, 0x2053, 0xffff, 0xffff],
            "forwarded={forwarded}"
        );
    }
}
