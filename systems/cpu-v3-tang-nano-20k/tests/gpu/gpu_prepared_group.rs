//! Native draw-group replay, fixed indirect parameters and same-page LCOPY.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::Path;

const SOURCE: &str = r#"mod gpu_group;mod gpu_mesh;mod gpu_arena;mod gpu_execute;mod gpu_present;mod gpu_api_fault;
static STEP:u16=0;static ROW:Buf<u16,64>=Buf::new([0;64]);static MESH:Buf<u16,16>=Buf::new([0;16]);
static FACTS:Buf<u16,8>=Buf::new([0xa000,32,1,1,0xa100,4,2,1]);
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){let fact_plan:Buf<u16,2>=Buf::new([0;2]);gpu_record_init();let saved=read_dseg_page(3);
 check(gpu_record_configure(0,0x9801,8,0x9a00,16));check(!gpu_group_enable(0));
 check(gpu_record_configure(0,0x9800,512,0x9a00,512));check(gpu_group_enable(0));check(!gpu_group_enable(0));
 check(gpu_record_configure(1,0xfc00,512,0xfe00,512));check(!gpu_group_ready(1));
 let mut c=gpu_record_data(0);check(c[GPU_RECORD_COMMAND_BASE]==0x9880&&c[GPU_RECORD_COMMAND_CAPACITY]==8192);
 check(!gpu_group_begin(0)&&!gpu_group_append(0));
 // Dirty source must win; dirty destination must not later overwrite the copy.
 unsafe{write_dseg_page(3,0x98);let src=Ptr::from_addr(0xc000);let dst=Ptr::from_addr(0xe000);
 src.write(0,0x1234);dst.write(0,0xdead);write_dseg_page(3,saved);}
 check(gpu_window_copy_lines(0x9800,0,0x9880,0,16));
 check(!gpu_window_copy_lines(0x9800,1,0x9880,0,16));
 check(!gpu_window_copy_lines(0x9800,0,0xfc00,0,16));
 check(!gpu_window_copy_lines(0x9800,0,0x9800,0,16));
 check(!gpu_window_copy_lines(0x9800,0,0x98ff,64,16));
 check(gpu_window_copy_lines(65535,65535,0,0,0));
 check(gpu_window_read_words(0x9880,0,ROW.as_array(),1));check(ROW[0u16]==0x1234);
 let memory:GpuMemory=GpuMemory{first_sector:0xa000,sector_count:32};
 let desc:MeshDesc=MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:2,storage:memory};
 check(gpu_mesh_init(MESH.as_array(),desc));ROW[0u16]=0xa004;ROW[1u16]=1;ROW[2u16]=0xa008;ROW[3u16]=2;
 check(gpu_window_write_words(0xa000,0,ROW.as_array(),4));
 let mut native=gpu_native_scratch();native[1u16]=199;native[3u16]=16384;native[36u16]=9;native[65u16]=1;native[108u16]=65535;
 check(gpu_packet_begin(0,0x8000,0x9000,0,0,0));c[GPU_RECORD_VALID]|=24;
 check(gpu_group_begin(0));check(!gpu_group_begin(0));check(!gpu_record_close_pass(0));
 check(gpu_mesh_draw(0,MESH.as_array(),native));native[36u16]=11;native[108u16]=0x4567;
 check(gpu_mesh_draw(0,MESH.as_array(),native));
 check(gpu_record_resource_room(0,FACTS.as_array(),2, fact_plan.as_array()));gpu_record_resource_commit(0,FACTS.as_array(),2, fact_plan.as_array());
 check(gpu_group_end(0));check(!gpu_group_end(0));check(gpu_group_ready(0));
 check(c[GPU_RECORD_VALID]&31==3&&c[GPU_GROUP_FACT_COUNT]==2);
 let prefix=c[GPU_GROUP_PUSH_END];let words=c[GPU_GROUP_WORDS];check(words!=0&&words&15==0);
 gpu_bind_copy(native,0,c,GPU_CB_GLOBAL,48);
 c[GPU_RECORD_VALID]|=24;check(gpu_group_part_begin(0,1));native[108u16]=0x9876;
 check(gpu_mesh_draw(0,MESH.as_array(),native));check(gpu_group_end(0));let second=c[GPU_GROUP_WORDS];
 check(gpu_group_ready(0)&&gpu_group_part_ready(0,1)&&!gpu_group_part_ready(1,1));
 check(gpu_record_close_pass(0));check(gpu_record_seal(0));check(gpu_record_reset(0));
 check(c[GPU_RECORD_PUSH_USED]==prefix&&c[GPU_RECORD_FACT_COUNT]==0&&gpu_group_ready(0));
 check(gpu_packet_begin(0,0x8800,0x9000,0,0,0));
 c[GPU_RECORD_COMMAND_USED]=8180;check(!gpu_group_append(0));check(c[GPU_RECORD_COMMAND_USED]==8180&&c[GPU_RECORD_FACT_COUNT]==0);
 c[GPU_RECORD_COMMAND_USED]=10;check(gpu_group_append(0));check(c[GPU_RECORD_COMMAND_USED]==16+words);
 check(c[GPU_RECORD_PUSH_USED]==prefix+16&&c[GPU_RECORD_FACT_COUNT]==2);
 // Matching complete entry state omits one32-byte Global/Render prefix.
 check(gpu_group_part_append(0,1));check(c[GPU_RECORD_COMMAND_USED]==words+second);
 c[GPU_RECORD_VALID]|=24;check(!gpu_group_part_begin(0,1));check(!gpu_group_part_begin(0,16));
 check(gpu_record_close_pass(0));check(gpu_record_seal(0));
 check(gpu_record_reset(0));check(gpu_packet_begin(0,0x8800,0x9000,0,0,0));c[GPU_RECORD_VALID]|=24;
 check(gpu_group_part_begin(0,1));native[108u16]=0xabcd;
 check(gpu_mesh_draw(0,MESH.as_array(),native));check(gpu_group_end(0));
 check(gpu_record_close_pass(0));check(gpu_record_seal(0));
 check(gpu_group_clear(0));check(!gpu_group_ready(0)&&c[GPU_RECORD_PUSH_USED]==prefix&&c[GPU_RECORD_COMMAND_USED]==0);
 check(read_dseg_page(3)==saved);halt(0x600d);}"#;

#[test]
fn prepared_group_preserves_snapshots_facts_and_rejects_invalid_line_copies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = root.join("../../../target/bloxorz-command-assembly/group-core");
    for enabled in [false, true] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = compile_program_named_checked(
            "group-core",
            SOURCE,
            &options,
            &RustcCheck::new(out.join(format!("rustc-{enabled}"))),
            &mut |name| {
                std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let mut halted = false;
        for _ in 0..2_000_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d, "check {}", signal.wrapping_sub(0xd000));
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded group-core timeout");
        // Reset is metadata-only: template and replay bytes remain inspectable.
        let m = cpu.physical_memory_mut();
        let template = 0x9800 * 64;
        let active = 0x9880 * 64;
        let native = |base: usize| {
            m[base..base + 256]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16))
                .collect::<Vec<_>>()
        };
        let t = native(template);
        let count = t.iter().position(|v| *v == 0xf0000000).unwrap_or(128);
        assert_eq!(count, 128, "templates must contain no END");
        let draws = t.iter().filter(|v| **v >> 28 == 3).count();
        assert_eq!(draws, 4);
        let bytes = t.iter().rposition(|v| *v != 0).unwrap() + 1;
        // Re-recording part1 has not changed part0's four-draw template.
        assert!(bytes * 2 <= 512);
        // Both distinct Instance DMA sources survive, so a repeated Model cannot
        // collapse earlier draws onto the final pose.
        let instances = t
            .iter()
            .filter(|v| **v >> 28 == 1 && ((**v >> 3) & 127) == 80)
            .map(|v| ((v >> 10) & 0x3ffff) * 16)
            .collect::<Vec<_>>();
        assert_eq!(instances.len(), 2);
        assert_ne!(instances[0], instances[1]);
        assert_eq!(m[instances[0] as usize + 44], 65535);
        assert_eq!(m[instances[1] as usize + 44], 0x4567);
        let second = template + 512;
        let part = native(second);
        let instance = part
            .iter()
            .find(|v| **v >> 28 == 1 && ((**v >> 3) & 127) == 80)
            .unwrap();
        let source = ((instance >> 10) & 0x3ffff) * 16;
        assert!(source >= 0x9a00 * 64 + 8192);
        assert_eq!(m[source as usize + 44], 0xabcd);
        assert!(m[active..active + 16].iter().any(|v| *v != 0));
    }
}
