//! G0 admission and language probe against real arena/schema sources, not a GPU executor.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;

const PROBE: &str = r#"
mod gpu_arena;
mod gpu_schema;
mod gpu_api_fault;
// Conservative reservations for the A/C boundary; these are not capture hooks.
static G0_CB_RECORDS:Buf<u16,416>=Buf::new([0;416]);
static G0_NATIVE_SCRATCH:Buf<u16,256>=Buf::new([0;256]);
static G0_CAPTURE_BRIDGE:Buf<u16,656>=Buf::new([0;656]);
static G0_REPLAY_DEPENDENCIES:Buf<u16,1024>=Buf::new([0;1024]);
static G0_EXECUTION:Buf<u16,16>=Buf::new([0;16]);
static G0_DISPLAY:Buf<u16,8>=Buf::new([0;8]);
static G0_STEP:u16=0;
fn check(ok:bool){if !ok{halt(0xb000+G0_STEP);}unsafe{addr_of(&G0_STEP).write(0,G0_STEP+1);}}
#[derive(Copy,Clone)]struct CommandMemory{commands:GpuMemory,push:GpuMemory}
struct GpuConfig{bytes:u16,first:CommandMemory,second:Option<CommandMemory>}
#[rcc_resource]struct Gpu{word:u16}
#[rcc_resource]struct CommandBuffer{word:u16}
#[rcc_resource(parent=CommandBuffer)]struct Pass{word:u16}
static ACTIVE:u16=0;static PASS_ENDS:u16=0;static CB_DROPS:u16=0;
impl Drop for Gpu{fn drop(&mut self){check(ACTIVE==0);}}
impl Drop for CommandBuffer{fn drop(&mut self){unsafe{addr_of(&CB_DROPS).write(0,CB_DROPS+1);}}}
impl Drop for Pass{fn drop(&mut self){unsafe{addr_of(&PASS_ENDS).write(0,PASS_ENDS+1);}}}
fn admission_gpu_init(config:GpuConfig)->Gpu{check(gpu_arena_init(config.bytes));check(config.first.commands.first_sector()==0x9800);
  let optional:Option<CommandMemory>=config.second;if let Some(memory)=optional{check(memory.push.first_sector()==0xfe00);}else{halt(0xbad);}
  Gpu{word:1}}
impl Gpu{
 fn buffer(&mut self)->CommandBuffer{CommandBuffer{word:0}}
 fn submit(&mut self,buffer:CommandBuffer){check(ACTIVE==0);unsafe{addr_of(&ACTIVE).write(0,buffer.word+1);}resource_disarm(buffer);}
 fn try_wait(&mut self)->Option<CommandBuffer>{if ACTIVE==0{return None;}let word=ACTIVE-1;unsafe{addr_of(&ACTIVE).write(0,0);}let buffer=CommandBuffer{word:word};Some(buffer)}
}
impl CommandBuffer{fn id(&self)->u16{self.word}fn pass(&mut self)->Pass{Pass{word:self.word}}}
impl Pass{fn touch(&mut self,value:CommandMemory){check(value.commands.sector_count()==512);}}
fn main(){
 check(!gpu_arena_init(0));check(!gpu_arena_init(31));check(!gpu_arena_init(8193));
 check(gpu_arena_init(128));let small=gpu_arena_alloc(false,1);let large=gpu_arena_alloc(true,2);
 check(small==1&&large==17);check(gpu_arena_alloc(false,1)==0);
 gpu_arena_data(small)[15u16]=0x1234;gpu_arena_data(large)[47u16]=0xabcd;
 check(gpu_arena_data(small).as_ptr().addr()&15==0);check(gpu_arena_data(large).as_ptr().addr()&15==0);
 check(gpu_arena_data(small)[15u16]==0x1234&&gpu_arena_data(large)[47u16]==0xabcd);
 #[cfg(gpu_debug)]{check(!gpu_arena_free(large,true,3));}
 check(gpu_arena_free(large,true,2));
 #[cfg(gpu_debug)]{check(!gpu_arena_free(large,true,2));}
 check(gpu_arena_alloc(true,2)==large);check(gpu_arena_free(small,false,1));check(gpu_arena_alloc(false,1)==small);
 check(gpu_arena_init(512));let anchor=gpu_arena_alloc(true,2);let mut ids:Buf<u16,8>=Buf::new([0;8]);
 let mut n:u16=0;while n<8{ids[n]=gpu_arena_alloc(false,1);check(ids[n]!=0);n+=1;}
 check(anchor==209);check(gpu_arena_data(anchor).as_ptr().addr()==GPU_OBJECT_CONTROL[0u16]+208);
 check(gpu_arena_free(ids[3u16],false,1));check(gpu_arena_alloc(false,1)==ids[3u16]);
 check(gpu_arena_init(192));let l=gpu_arena_alloc(true,2);let s=gpu_arena_alloc(false,1);let s1=gpu_arena_alloc(false,1);let s2=gpu_arena_alloc(false,1);
 check(l!=0&&s!=0&&s1!=0&&s2!=0);check(gpu_arena_free(s,false,1));check(gpu_arena_alloc(true,2)==0);check(gpu_arena_alloc(false,1)==s);
 let memory:CommandMemory=CommandMemory{commands:unsafe{admission_memory(0x9800,512)},push:unsafe{admission_memory(0x9a00,512)}};
 let second:CommandMemory=CommandMemory{commands:unsafe{admission_memory(0xfc00,512)},push:unsafe{admission_memory(0xfe00,512)}};
 let config:GpuConfig=GpuConfig{bytes:8192,first:memory,second:Some(second)};
 let mut gpu:Gpu=admission_gpu_init(config);let mut buffer:CommandBuffer=gpu.buffer();let mut cycle:u16=0;
 while cycle<3{{let mut pass=buffer.pass();pass.touch(memory);}check(buffer.id()==0);gpu.submit(buffer);
 let completed:Option<CommandBuffer>=gpu.try_wait();if let Some(next)=completed{buffer=next;}else{halt(0xbad);}cycle+=1;}
 check(PASS_ENDS==3&&CB_DROPS==0);drop(buffer);check(CB_DROPS==1);let none:Option<CommandBuffer>=gpu.try_wait();if let Some(unexpected)=none{drop(unexpected);halt(0xbad);}
 halt(0x600d);
}
"#;

#[test]
fn actual_rcc_g0_admission_and_affine_probe_both_profiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/graphics-api-g0")
        });
    let mut results = Vec::new();
    for enabled in [false, true] {
        let dir = out.join(format!("admission-{enabled}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = crate::checked_flat_fixture::compile(
            "graphics-api-g0",
            PROBE,
            &options,
            &RustcCheck::new(dir.join("rustc")),
            &mut |n| {
                let mut source = std::fs::read_to_string(root.join(format!("{n}.rs")))
                    .map_err(|error| error.to_string())?;
                if n == "gpu_schema" {
                    // The isolated language fixture constructs values legally in
                    // their defining file. Public registration is tested separately.
                    source.push_str("\npub unsafe fn admission_memory(first:u16,count:u16)->GpuMemory{GpuMemory{first_sector:first,sector_count:count}}\n");
                }
                Ok(source)
            },
        )
        .unwrap_or_else(|e| panic!("profile{enabled}: {e}"));
        let static_words = parsed.debug.static_data_end;
        assert!(
            static_words <= 8192,
            "static prefix is {static_words} words"
        );
        assert_eq!(
            parsed
                .debug
                .globals
                .iter()
                .any(|v| v.name == "GPU_OBJECT_TAGS"),
            enabled
        );
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        std::fs::write(dir.join("program.lst"), &program.listing).unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let mut minimum_sp = 0xc000;
        let mut steps = 0;
        let mut halted = false;
        while steps < 1_000_000 {
            steps += 1;
            let outcome = cpu.step().unwrap();
            let sp = cpu.registers()[13];
            if sp > 0 {
                minimum_sp = minimum_sp.min(sp);
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded G0 timeout");
        assert!(minimum_sp >= 0xa000, "stack escaped its16-KiB budget");
        let result = serde_json::json!({"gpu_debug":enabled,"static_words":static_words,"static_bytes":static_words*2,
            "frame_max_words":program.debug.functions.iter().map(|f|f.frame_size).max().unwrap_or(0),
            "observed_stack_words":0xc000-minimum_sp,"steps":steps,"code_words":program.words.len(),
            "scope":"Real arena/schema plus conservative CB/capture reservations and compiler ownership probe; no actual GPU submission or rendering"});
        std::fs::write(
            dir.join("admission.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        results.push(result);
    }
    println!("G0_ADMISSION {}", serde_json::to_string(&results).unwrap());
}

#[test]
fn required_resource_associated_factory_compiles_on_target() {
    let source = r#"#[rcc_resource]struct Gpu{word:u16}
impl Drop for Gpu{fn drop(&mut self){}}
impl Gpu{fn init()->Gpu{Gpu{word:0}}}
fn main(){let gpu:Gpu=Gpu::init();halt(0x600d);}"#;
    let root = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/graphics-api-g0")
        });
    let directory = root.join("required-associated-factory");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let program = crate::checked_flat_fixture::compile(
        "required-resource-factory",
        source,
        &options,
        &RustcCheck::new(directory),
        &mut |n| Err(format!("unexpected module {n}")),
    );
    assert!(
        program.is_ok(),
        "Required public Gpu::init/scene constructors remain blocked: {}",
        program.err().unwrap()
    );
}

#[test]
fn required_unsafe_extent_constructor_compiles_on_target() {
    let source = r#"struct GpuMemory{first_sector:u16,sector_count:u16}
impl GpuMemory{pub unsafe fn from_sectors(first_sector:u16,sector_count:u16)->Self{
GpuMemory{first_sector:first_sector,sector_count:sector_count}}}
fn main(){let memory:GpuMemory=unsafe{GpuMemory::from_sectors(0x9800,512)};halt(0x600d);}"#;
    let directory = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/graphics-api-g0")
        })
        .join("required-unsafe-constructor");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let program = crate::checked_flat_fixture::compile(
        "required-unsafe-constructor",
        source,
        &options,
        &RustcCheck::new(directory),
        &mut |n| Err(format!("unexpected module {n}")),
    );
    assert!(
        program.is_ok(),
        "Required unsafe GpuMemory constructor remains blocked: {}",
        program.err().unwrap()
    );
}

#[test]
fn cpu_owned_storage_chunks_preserve_mapping_and_physical_boundaries() {
    let source = r#"mod gpu_storage;
static ROW:Buf<u16,96>=Buf::new([0;96]);static STEP:u16=0;
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){let saved=read_dseg_page(3);let mut n:u16=0;
 while n<32{ROW[n]=0x7000+n;n+=1;}
 check(gpu_window_write_words(0x9800,16376,ROW.as_array(),32));check(read_dseg_page(3)==saved);
 n=0;while n<32{ROW[n]=0;n+=1;}
 check(gpu_window_read_words(0x9800,16376,ROW.as_array(),32));n=0;while n<32{check(ROW[n]==0x7000+n);n+=1;}
 check(read_dseg_page(3)==saved);check(gpu_window_write_words(65535,48,ROW.as_array(),16));
 check(!gpu_window_write_words(65535,48,ROW.as_array(),17));check(!gpu_window_read_words(65535,64,ROW.as_array(),1));
 let invalid=unsafe{Ptr::from_addr(0xc000).as_u16_array()};check(!gpu_window_write_words(0,0,invalid,1));
 check(!gpu_window_write_words(0,0,ROW.as_array(),GPU_API_PUSH_CHUNK_WORDS+1));
 n=0;while n<64{ROW[n]=0x6000+n;n+=1;}
 check(gpu_window_write_words(0x9000,0,ROW.as_array(),64));
 n=0;while n<64{ROW[n]=0;n+=1;}
 check(gpu_window_read_words(0x9000,0,ROW.as_array(),64));
 n=0;while n<64{check(ROW[n]==0x6000+n);n+=1;}
 check(gpu_window_write_words(65535,65535,invalid,0));check(read_dseg_page(3)==saved);
 let mut aligned=unsafe{Ptr::from_addr((ROW.as_ptr().addr()+15)&65520).as_u16_array()};
 n=0;while n<64{aligned[n]=0x4000+n;n+=1;}
 check(gpu_window_write_words(0x9100,0,aligned,64));
 n=0;while n<64{aligned[n]=0;n+=1;}
 check(gpu_window_read_words(0x9100,0,aligned,64));
 n=0;while n<64{check(aligned[n]==0x4000+n);n+=1;}
 let middle=unsafe{aligned.as_ptr().add(1).as_u16_array()};
 check(gpu_window_write_words(0x9200,1,middle,31));
 n=1;while n<32{aligned[n]=0;n+=1;}
 check(gpu_window_read_words(0x9200,1,middle,31));
 check(aligned[0u16]==0x4000&&aligned[32u16]==0x4020);
 n=1;while n<32{check(aligned[n]==0x4000+n);n+=1;}
 check(read_dseg_page(3)==saved);halt(0x600d);}"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/graphics-api-g0")
        });
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = crate::checked_flat_fixture::compile(
        "cpu-owned-storage",
        source,
        &options,
        &RustcCheck::new(out.join("storage")),
        &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let code = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    let mut copied_lines = 0;
    let mut halted = false;
    for _ in 0..500_000 {
        let pc = usize::from(cpu.pc());
        if code.words[pc] >> 8 == 0x67 {
            copied_lines += 1;
        }
        if let cpu_v3::StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted, "storage copy exceeded its step bound");
    assert!(
        copied_lines >= 10,
        "aligned and partial spans did not execute LCOPY"
    );
    // These are physical word addresses: verify both sides of the DSEG page and the8-MiB tail.
    let start = 0x9800usize * 64 + 16376;
    assert_eq!(
        &cpu.physical_memory_mut()[start..start + 32],
        &(0..32).map(|n| 0x7000 + n).collect::<Vec<u16>>()
    );
    assert_eq!(
        &cpu.physical_memory_mut()[0x9000 * 64..0x9000 * 64 + 64],
        &(0..64).map(|n| 0x6000 + n).collect::<Vec<u16>>()
    );
    assert_eq!(
        &cpu.physical_memory_mut()[0x3ffff0..0x400000],
        &(0..16).map(|n| 0x7000 + n).collect::<Vec<u16>>()
    );
}
