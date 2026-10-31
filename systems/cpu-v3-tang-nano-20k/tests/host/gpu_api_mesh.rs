//! Directory-only fixtures: metadata/payload bytes are not rendered by this test.
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::Path;

const SOURCE: &str = r#"mod gpu_mesh;
static STEP:u16=0;static MESH:Buf<u16,16>=Buf::new([0;16]);
static SUB:Buf<u16,16>=Buf::new([0;16]);static ROW:Buf<u16,64>=Buf::new([0;64]);
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){gpu_record_init();let saved=read_dseg_page(3);
 let memory:GpuMemory=GpuMemory{first_sector:0xa000,sector_count:512};
 let desc:MeshDesc=MeshDesc{directory_sector:0xa0ff,directory_word:63,meshlet_count:9,storage:memory};
 check(gpu_mesh_init(MESH.as_array(),desc));
 check(!gpu_mesh_subset(SUB.as_array(),MESH.as_array(),10,0));
 check(!gpu_mesh_subset(SUB.as_array(),MESH.as_array(),8,2));
 let mut i:u16=0;while i<9{ROW[i*2]=0xa010+i*4;ROW[i*2+1]=i%3+1;i+=1;}
 check(gpu_window_write_words(0xa0ff,63,ROW.as_array(),18));
 let mut native=GPU_NATIVE_SCRATCH.as_array();native[1u16]=199;native[3u16]=16384;
 native[5u16]=57344;native[6u16]=60621;native[7u16]=8192;native[8u16]=128;native[9u16]=128;native[10u16]=33;
 native[36u16]=9;native[65u16]=1;native[75u16]=1;native[85u16]=1;native[89u16]=1;native[97u16]=1;
 native[105u16]=1;native[107u16]=1;native[108u16]=65535;native[109u16]=65535;
 let mut case:u16=0;while case<4{
 check(gpu_record_configure(0,0x9800+case*8,8,0x9a00+case*16,16));
 check(gpu_packet_begin(0,0x8000,0x9000,0,0,0));
 let mut first:u16=0;let mut count:u16=9;
 if case==0{count=1;}if case==1{first=3;count=2;}if case==3{first=9;count=0;}
 check(gpu_mesh_subset(SUB.as_array(),MESH.as_array(),first,count));
 check(gpu_mesh_draw(0,SUB.as_array(),native));
 check(gpu_record_close_pass(0));check(gpu_record_seal(0));case+=1;
 }
 // The ninth entry fails only after the first batch; no bind bytes may be written.
 ROW[17u16]=0;check(gpu_window_write_words(0xa0ff,63,ROW.as_array(),18));
 check(gpu_record_configure(0,0x9900,8,0x9b00,16));check(gpu_packet_begin(0,0x8000,0x9000,0,0,0));
 check(!gpu_mesh_draw(0,MESH.as_array(),native));let c=gpu_record_data(0);
 check(c[GPU_RECORD_COMMAND_USED]==10&&c[GPU_RECORD_PUSH_USED]==16&&c[GPU_RECORD_VALID]==0);
 check(gpu_window_read_words(0x9900,10,ROW.as_array(),1));check(ROW[0u16]==0);
 check(gpu_window_read_words(0x9b00,32,ROW.as_array(),1));check(ROW[0u16]==0);
 // Complete-draw capacity is rejected before even a valid first binding can commit.
 check(gpu_record_configure(0,0x9910,1,0x9b10,16));check(gpu_packet_begin(0,0x8000,0x9000,0,0,0));
 check(!gpu_mesh_draw(0,MESH.as_array(),native));check(c[GPU_RECORD_COMMAND_USED]==10&&c[GPU_RECORD_PUSH_USED]==16);
 // An empty subset at the physical endpoint retains a representable directory pointer.
 let edge:GpuMemory=GpuMemory{first_sector:65535,sector_count:1};
 let empty:MeshDesc=MeshDesc{directory_sector:65535,directory_word:63,meshlet_count:0,storage:edge};
 check(gpu_mesh_init(MESH.as_array(),empty));check(gpu_mesh_subset(SUB.as_array(),MESH.as_array(),0,0));
 check(SUB[GPU_MESH_DIRECTORY]==65535&&SUB[GPU_MESH_COUNT]==0);
 check(read_dseg_page(3)==saved);halt(0x600d);}"#;

fn dma(base: u32, word: u32, local: u32, size: u32) -> u32 {
    0x10000000 | (((base * 128 + word * 2) / 32) << 10) | (local << 3) | (size << 1)
}
#[test]
fn mesh_directory_expands_with_pingpong_prefetch_and_whole_draw_preflight() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    for enabled in [false, true] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = compile_program_named_checked(
            "mesh-directory",
            SOURCE,
            &options,
            &RustcCheck::new(output.join(format!("mesh-{enabled}"))),
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
        let memory = cpu.physical_memory_mut();
        for (case, first, count) in [(0, 0, 1), (1, 3, 2), (2, 0, 9), (3, 9, 0)] {
            let push = 0x9a00 + case * 16;
            let mut code = vec![
                0x40000003u32,
                0x70000000,
                dma(push, 0, 69, 0),
                0x40000001,
                0xa0000045,
            ];
            if count != 0 {
                code.extend([
                    0x40000003,
                    dma(push, 32, 66, 1),
                    0x40000001,
                    0x50000042,
                    dma(push, 64, 68, 0),
                    0x40000001,
                    0x60000044,
                    dma(push, 128, 80, 2),
                    0x40000001,
                ]);
                for i in 0..count {
                    let record = 0xa010 + (first + i) * 4;
                    let payload = (first + i) % 3 + 1;
                    code.push(0x20000000 | (record << 12) | ((payload - 1) << 1) | (i & 1));
                    if i != 0 {
                        code.push(0x30000000 | ((i - 1) & 1));
                    }
                    code.push(0x40000001);
                }
                code.push(0x30000000 | ((count - 1) & 1));
            }
            code.extend([0x40000003, 0x70000000, 0xf0000000]);
            let mut expected = code
                .iter()
                .flat_map(|w| [*w as u16, (*w >> 16) as u16])
                .collect::<Vec<_>>();
            expected.resize(expected.len().div_ceil(64) * 64, 0);
            let base = (0x9800 + case as usize * 8) * 64;
            assert_eq!(
                &memory[base..base + expected.len()],
                expected,
                "case{case}/{enabled}"
            );
        }
    }
}
