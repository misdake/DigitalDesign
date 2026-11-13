//! Public text formatting: boundaries, sign and packed-cell preservation.
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};

#[test]
fn decimal_signed_minimum_and_hex_are_real_ascii_in_both_profiles() {
    let source = r#"
mod gpu_api;use gpu_api::*;mod gpu_board;use gpu_board::*;mod ui_text;use ui_text::*;
fn main(){
 let gpu=Gpu::init(board_gpu_config());
 let atlas=UiAtlas::from_native(unsafe{GpuMemory::from_sectors(0xb000,512)},255);
 let mut grid=UiGrid::from_memory(unsafe{GpuMemory::from_sectors(0xb300,6)},&atlas);
 grid.fill(255);grid.write_glyph(0,0,65);
 text_u16(&mut grid,1,0,0);text_u16(&mut grid,3,0,65535);
 text_i16(&mut grid,0,1,-32767i16-1);text_i16(&mut grid,8,1,32767);
 text_hex16(&mut grid,46,14,0xabcd);halt(0x600d);
}"#;
    for debug in [false, true] {
        let parsed = super::gpu_api_resources::parse(source, "ui-text", debug).unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        assert!(matches!(
            cpu.run(1_000_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        let grid = &cpu.physical_memory_slice()[0xb300 * 64..0xb300 * 64 + 375];
        let text = |x: usize, y: usize, count: usize| -> Vec<u8> {
            (0..count)
                .map(|i| {
                    let cell = y * 50 + x + i;
                    (grid[cell / 2] >> ((cell & 1) * 8)) as u8
                })
                .collect()
        };
        assert_eq!(text(0, 0, 2), b"A0");
        assert_eq!(text(3, 0, 5), b"65535");
        assert_eq!(text(0, 1, 6), b"-32768");
        assert_eq!(text(8, 1, 5), b"32767");
        assert_eq!(text(46, 14, 4), b"ABCD");
        assert_eq!(text(2, 0, 1), [255]);
    }
}
