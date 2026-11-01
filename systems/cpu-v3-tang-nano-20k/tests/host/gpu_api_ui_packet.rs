//! UI recording bytes and staging lifetimes; no glyph or pixel rendering here.
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::Path;

const SOURCE: &str = r#"mod gpu_ui_packet;mod gpu_mesh;
static STEP:u16=0;static DESC:Buf<u16,16>=Buf::new([0;16]);
static MESH:Buf<u16,16>=Buf::new([0;16]);static WORD:Buf<u16,4>=Buf::new([0;4]);
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){gpu_record_init();check(gpu_record_configure(0,0x9800,4,0x9a00,16));
 check(gpu_packet_begin(0,0x8000,0x9000,0,0,0));DESC[0u16]=0xb000;DESC[1u16]=0x70;
 check(gpu_ui_packet_draw(0,DESC.as_array(),2,1,3,2));let mut c=gpu_record_data(0);
 check(c[GPU_RECORD_COMMAND_USED]==22&&c[GPU_RECORD_PUSH_USED]==32&&c[GPU_RECORD_UI_WORD]==17);
 check(gpu_ui_packet_draw(0,DESC.as_array(),10,4,5,1));check(c[GPU_RECORD_COMMAND_USED]==24);
 let invalid=unsafe{Ptr::from_addr(0xc000).as_u16_array()};check(gpu_ui_packet_draw(0,invalid,50,15,0,0));
 check(!gpu_ui_packet_draw(0,DESC.as_array(),49,14,2,1));check(c[GPU_RECORD_COMMAND_USED]==24);
 let mut native=gpu_native_scratch();native[1u16]=199;native[3u16]=16384;
 native[5u16]=57344;native[6u16]=60621;native[7u16]=8192;native[8u16]=128;native[9u16]=128;native[10u16]=33;
 native[36u16]=9;native[65u16]=1;native[75u16]=1;native[85u16]=1;native[89u16]=1;native[97u16]=1;
 native[105u16]=1;native[107u16]=1;native[108u16]=65535;native[109u16]=65535;
 check(gpu_bind_install(0,native));check(c[GPU_RECORD_VALID]&32!=0&&c[GPU_RECORD_UI_CLEAN]==1);
 check(gpu_ui_packet_draw(0,DESC.as_array(),0,6,2,1));check(c[GPU_RECORD_COMMAND_USED]==44);
 let memory:GpuMemory=GpuMemory{first_sector:0xa000,sector_count:16};
 let desc:MeshDesc=MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:memory};
 check(gpu_mesh_init(MESH.as_array(),desc));WORD[0u16]=0xa002;WORD[1u16]=1;
 check(gpu_window_write_words(0xa000,0,WORD.as_array(),2));check(gpu_mesh_draw(0,MESH.as_array(),native));
 check(c[GPU_RECORD_COMMAND_USED]==54&&c[GPU_RECORD_UI_CLEAN]==0&&c[GPU_RECORD_VALID]&32!=0);
 check(gpu_ui_packet_draw(0,DESC.as_array(),5,11,2,1));check(c[GPU_RECORD_COMMAND_USED]==60&&c[GPU_RECORD_PUSH_USED]==192);
 check(gpu_record_close_pass(0));check(gpu_packet_begin(0,0x8800,0x9000,0,0,0));
 check(c[GPU_RECORD_VALID]&32==0&&c[GPU_RECORD_UI_WORD]==17);
 check(gpu_ui_packet_draw(0,DESC.as_array(),0,0,50,15));check(c[GPU_RECORD_COMMAND_USED]==84&&c[GPU_RECORD_PUSH_USED]==208);
 DESC[0u16]=0xb006;check(gpu_ui_packet_draw(0,DESC.as_array(),49,14,1,1));
 check(c[GPU_RECORD_COMMAND_USED]==90&&c[GPU_RECORD_PUSH_USED]==224&&c[GPU_RECORD_UI_WORD]==209);
 c[GPU_RECORD_COMMAND_USED]=250;DESC[0u16]=0xb00c;check(!gpu_ui_packet_draw(0,DESC.as_array(),0,0,50,15));
 check(c[GPU_RECORD_COMMAND_USED]==250&&c[GPU_RECORD_PUSH_USED]==224&&c[GPU_CB_UI]==0xb006);
 check(gpu_window_read_words(0x9800,250,WORD.as_array(),1));check(WORD[0u16]==0);
 c[GPU_RECORD_COMMAND_USED]=90;check(gpu_record_close_pass(0));check(gpu_record_seal(0));
 check(c[GPU_RECORD_COMMAND_USED]==96&&c[GPU_RECORD_PADDED]==128);halt(0x600d);}"#;
fn dma(word: u32, local: u32, size: u32) -> u32 {
    0x10000000 | (((0x9a00u32 * 128 + word * 2) / 32) << 10) | (local << 3) | (size << 1)
}
fn range(first: u32, end: u32) -> u32 {
    if first == 0 && end == 750 {
        0xb0000045
    } else {
        0xb8000045 | (first << 7) | ((end - 1) << 17)
    }
}
#[test]
fn ui_ranges_reuse_descriptor_and_preserve_binding_staging_lifetimes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    for enabled in [false, true] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = compile_program_named_checked(
            "ui-packet",
            SOURCE,
            &options,
            &RustcCheck::new(output.join(format!("ui-packet-{enabled}"))),
            &mut |name| {
                std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        assert!(matches!(
            cpu.run(1_000_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        let code = [
            0x40000003,
            0x70000000,
            dma(0, 69, 0),
            0x40000001,
            0xa0000045,
            0x40000003,
            0x70000000,
            dma(16, 69, 0),
            0x40000001,
            range(52, 55),
            range(102, 105),
            range(210, 215),
            0x40000003,
            dma(32, 66, 1),
            0x40000001,
            0x50000042,
            dma(64, 68, 0),
            0x40000001,
            0x60000044,
            dma(128, 80, 2),
            0x40000001,
            range(300, 302),
            dma(128, 80, 2),
            0x40000001,
            0x2a002000,
            0x40000001,
            0x30000000,
            0x40000003,
            0x70000000,
            range(555, 557),
            0x40000003,
            0x70000000,
            0x40000003,
            0x70000000,
            dma(192, 69, 0),
            0x40000001,
            0xa0000045,
            0x40000003,
            0x70000000,
            dma(16, 69, 0),
            0x40000001,
            range(0, 750),
            dma(208, 69, 0),
            0x40000001,
            range(749, 750),
            0x40000003,
            0x70000000,
            0xf0000000,
        ];
        let mut expected = code
            .iter()
            .flat_map(|w| [*w as u16, (*w >> 16) as u16])
            .collect::<Vec<_>>();
        assert_eq!(expected.len(), 96);
        expected.resize(128, 0);
        let memory = cpu.physical_memory_mut();
        let base = 0x9800 * 64;
        assert_eq!(&memory[base..base + 128], expected, "UI packet/{enabled}");
        let push = 0x9a00 * 64;
        assert_eq!(&memory[push + 16..push + 18], &[0xb000, 0x70]);
        assert_eq!(&memory[push + 192..push + 194], &[0x2200, 0x0900]);
        assert_eq!(&memory[push + 208..push + 210], &[0xb006, 0x70]);
    }
}
