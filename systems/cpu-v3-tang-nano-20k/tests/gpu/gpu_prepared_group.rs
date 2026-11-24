//! Application-owned immutable prepared storage and cross-page command copies.
use cpu_v3::{rcc_backend, CpuV3Sim};
use rcc::frontend::{
    compile_program_named_with_libraries_checked, rustc_check::RustcCheck, ModuleNamespace,
    ModuleSource,
};
use std::path::Path;

const SOURCE: &str = r#"use gpu_bind::*;
static MODEL:Buf<u16,48>=Buf::new([0;48]);
static MESH:Buf<u16,16>=Buf::new([0;16]);
static ROW:Buf<u16,64>=Buf::new([0;64]);
static CAMERA:Buf<u16,36>=Buf::new([0;36]);
static STATE:Buf<u16,16>=Buf::new([0;16]);
static STEP:u16=0;
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn select_camera(slot:u16){let mut data=CAMERA.as_array();let mut i:u16=0;while i<32{data[i]=0;i+=1;}let one=fix32::from_int(1);
 gpu_scene_fix_write(data,0,one);gpu_scene_fix_write(data,10,one);gpu_scene_fix_write(data,20,one);
 gpu_scene_fix_write(data,GPU_CAMERA_FOCAL,fix32::from_int(200));gpu_scene_fix_write(data,GPU_CAMERA_FAR,fix32::from_int(100));
 gpu_scene_fix_write(STATE.as_array(),4,one);gpu_scene_fix_write(STATE.as_array(),6,one);gpu_scene_fix_write(STATE.as_array(),8,one);
 data[GPU_CAMERA_REVISION]+=1;check(gpu_bind_desired(slot,true,data));check(gpu_bind_desired(slot,false,STATE.as_array()));}
pub fn prepared_group_test(){
 check(gpu_arena_init(8192));gpu_record_init();let saved=read_dseg_page(3);
 check(gpu_record_configure(0,0x9800,128,0x9a00,128));check(gpu_record_configure(1,0xfc00,128,0xfe00,128));
 let owner=gpu_arena_alloc(false,GpuObjectKind::Prepared);
 check(gpu_group_new(owner,PreparedMemory::new(unsafe{GpuMemory::from_sectors(0xa800,8)},unsafe{GpuMemory::from_sectors(0xa900,32)})));
 let mut i:u16=0;while i<64{ROW[i]=i+123;i+=1;}
 check(gpu_window_write_words(0xc0ff,48,ROW.as_array(),64));
 check(gpu_window_copy_span(0xc0ff,48,0xd0ff,48,64,ROW.as_array()));
 i=0;while i<64{ROW[i]=0;i+=1;}
 check(gpu_window_read_words(0xd0ff,48,ROW.as_array(),64));i=0;while i<64{check(ROW[i]==i+123);i+=1;}
 check(!gpu_window_copy_span(0xc0ff,1,0xd0ff,48,64,ROW.as_array()));
 check(!gpu_window_copy_span(0xc000,32768,0xd000,0,16,ROW.as_array()));
 let memory=unsafe{GpuMemory::from_sectors(0xa000,32)};
 let desc=MeshDesc::from_memory(memory,0,2);
 ROW[0u16]=0xa004;ROW[1u16]=1;ROW[2u16]=0xa008;ROW[3u16]=2;
 check(gpu_window_write_words(0xa000,0,ROW.as_array(),4));check(gpu_mesh_init(MESH.as_array(),desc));
 check(gpu_group_part_begin(owner));check(!gpu_group_part_begin(owner));check(!gpu_group_free(owner));
 let mut native=gpu_native_scratch();native[31u16]=1;MODEL[44u16]=1;native[5u16]=1;native[120u16]=9;native[65u16]=1;MODEL[36u16]=65535;native[110u16]=1;
 check(gpu_mesh_draw(GPU_PREPARED_RECORD,MESH.as_array(),MODEL.as_array(),native));native[120u16]=11;MODEL[36u16]=0x4567;MODEL[44u16]+=1;
 check(gpu_mesh_draw(GPU_PREPARED_RECORD,MESH.as_array(),MODEL.as_array(),native));
 check(gpu_group_end(GPU_PREPARED_RECORD));check(!gpu_group_end(GPU_PREPARED_RECORD));check(gpu_group_part_ready(owner));
 let pool=gpu_arena_alloc(false,GpuObjectKind::PreparedPool);
 check(gpu_group_pool_init(pool,unsafe{GpuMemory::from_sectors(0xc100,80)}));
 let first=gpu_group_pool_allocate(pool,1024,2048);check(first!=0);let old=gpu_arena_handle(first);
 let second=gpu_group_pool_allocate(pool,1024,2048);check(second!=0);
 check(gpu_group_pool_allocate(pool,8192,8192)==0);
 check(gpu_group_pool_free(pool,first));let replacement=gpu_group_pool_allocate(pool,1024,2048);check(replacement!=0);
 let fresh=gpu_arena_handle(replacement);check(fresh.generation.low!=old.generation.low||fresh.generation.high!=old.generation.high);
 check(gpu_arena_data(replacement)[GPU_PREPARED_COMMAND]==0xc100);
 check(gpu_group_part_begin(second));gpu_group_abandon(GPU_PREPARED_RECORD);check(!gpu_group_part_ready(second));
 check(gpu_group_part_begin(replacement));check(gpu_group_end(GPU_PREPARED_RECORD));
 check(gpu_packet_begin(0,0x8800,0x9000,0,0,0));
 let c=gpu_record_data(0);let used=c[GPU_RECORD_COMMAND_USED];
 check(gpu_group_part_append(0,replacement));check(c[GPU_RECORD_COMMAND_USED]==used);
 check(!gpu_group_part_append(0,owner));select_camera(0);check(gpu_group_part_append(0,owner));check(gpu_group_part_append(0,owner));
 check(gpu_group_part_ready(owner));
 check(gpu_record_close_pass(0));
 check(gpu_packet_begin(1,0x8800,0x9000,0,0,0));select_camera(1);check(gpu_group_part_append(1,owner));check(gpu_record_close_pass(1));
 check(gpu_record_reset(0));check(gpu_group_part_ready(owner));
 check(gpu_record_reset(1));check(gpu_group_invalidate(owner));check(gpu_group_free(owner));
 check(gpu_group_pool_drop(pool));check(gpu_arena_empty());check(read_dseg_page(3)==saved);
 check(dcache_invalidate_all()==0);halt(0x600d);
}
"#;

#[test]
fn prepared_allocations_preserve_snapshots_for_both_frame_consumers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("rcc")
        .canonicalize()
        .unwrap();
    let out = root.join("../../../target/prepared-api/core");
    for enabled in [false, true] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = compile_program_named_with_libraries_checked(
            "group-core",
            "mod gpu_api; fn main() { gpu_api::prepared_group_test(); }",
            &options,
            &[root.join("gpu_api.rs")],
            &RustcCheck::new(out.join(format!("rustc-{enabled}"))),
            &mut |name| {
                let origin = root.join(format!("{name}.rs"));
                let mut source = std::fs::read_to_string(&origin).map_err(|e| e.to_string())?;
                if name == "gpu_api" {
                    source.push('\n');
                    source.push_str(SOURCE);
                }
                Ok(ModuleSource {
                    source,
                    namespace: ModuleNamespace::Flat,
                    origin: Some(origin),
                })
            },
        )
        .unwrap();

        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let outcome = cpu.run(2_000_000).unwrap();
        assert!(
            matches!(outcome, cpu_v3::RunOutcome::Halted { signal: 0x600d, .. }),
            "{outcome:?}"
        );
        let memory = cpu.physical_memory_mut();
        let code = memory[0xa800 * 64..0xa800 * 64 + 512]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|w| u32::from(w[0]) | u32::from(w[1]) << 16)
            .collect::<Vec<_>>();
        assert!(code.iter().all(|v| v >> 28 != 5 && *v != 0xf0000000));
        let worlds = code
            .iter()
            .filter(|v| **v >> 28 == 1 && (**v >> 3) & 127 == 80)
            .map(|v| ((v >> 10) & 0x3ffff) * 16)
            .collect::<Vec<_>>();
        assert_eq!(worlds.len(), 2);
        assert_ne!(worlds[0], worlds[1]);
        assert_eq!(memory[worlds[0] as usize + 44], 65535);
        assert_eq!(memory[worlds[1] as usize + 44], 0x4567);
    }
}
