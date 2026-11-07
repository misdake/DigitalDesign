//! Actual record-core execution without a public factory or GPU device peer.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::Path;

const RECORD: &str = r#"
mod gpu_execute;mod gpu_present;mod gpu_arena;mod gpu_api_fault;
static CODE:Buf<u16,64>=Buf::new([0;64]);
static DATA:Buf<u16,64>=Buf::new([0;64]);
static STEP:u16=0;
static CAPTURE_RESERVATION:Buf<u16,656>=Buf::new([0;656]);
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){
 let mut scratch=gpu_native_scratch();let raw=GPU_NATIVE_SCRATCH.as_ptr().addr();let aligned=scratch.as_ptr().addr();
 check(aligned&15==0&&aligned>=raw&&aligned+256<=raw+271);
 scratch[0u16]=0xa11e;scratch[255u16]=0xb11e;
 check(gpu_native_scratch()[0u16]==0xa11e&&gpu_native_scratch()[255u16]==0xb11e);
 check(GPU_CB_CAPTURE_FACTS[0u16]==0&&GPU_CB_CAPTURE_FACTS[511u16]==0);
 let saved=read_dseg_page(3);gpu_record_init();check(gpu_arena_init(8192));
 check(!gpu_record_configure(2,0x9800,1,0x9a00,1));
 check(!gpu_record_configure(0,0x9800,0,0x9a00,1));
 check(!gpu_record_configure(0,0x9800,513,0x9a00,1));
 check(!gpu_record_configure(0,65535,2,0x9a00,1));
 check(!gpu_record_configure(0,0x9800,2,0x9801,1));
 check(gpu_record_configure(0,0x9800,1,0x9a00,3));
 check(gpu_record_configure(1,0xfc00,2,0xfe00,2));
 let mut a=gpu_record_data(0);let mut b=gpu_record_data(1);a[GPU_CB_CAMERA]=0x1111;b[GPU_CB_CAMERA]=0x2222;
 check(!gpu_record_seal(0));check(!gpu_record_close_pass(0));
 let mut i:u16=0;while i<64{CODE[i]=0x5000+i;DATA[i]=0xaaaa;i+=1;}
 check(gpu_window_write_words(0x9a00,0,DATA.as_array(),64));
 check(gpu_window_write_words(0x9a00,64,DATA.as_array(),64));
 check(gpu_window_write_words(0x9a00,128,DATA.as_array(),64));
 check(gpu_window_write_words(0xfc00,0,DATA.as_array(),64));
 check(gpu_window_write_words(0xfc00,64,DATA.as_array(),64));
 i=0;while i<64{DATA[i]=0x7000+i;i+=1;}
 check(gpu_record_begin_pass(0));check(!gpu_record_begin_pass(0));check(!gpu_record_reset(0));check(!gpu_record_seal(0));
 check(gpu_record_append(0,CODE.as_array(),4,DATA.as_array(),16,16));
 check(gpu_record_push_offset(0,32,32)==32);
 check(gpu_record_append(0,CODE.as_array(),2,DATA.as_array(),32,32));
 check(gpu_record_push_offset(0,64,64)==64);
 check(gpu_record_append(0,CODE.as_array(),2,DATA.as_array(),64,64));
 check(gpu_record_append(0,CODE.as_array(),2,DATA.as_array(),64,64));
 check(a[GPU_RECORD_PUSH_USED]==192&&a[GPU_RECORD_COMMAND_USED]==10);
 check(!gpu_record_append(0,CODE.as_array(),2,DATA.as_array(),1,16));
 check(!gpu_record_append(0,CODE.as_array(),50,DATA.as_array(),0,1));
 check(!gpu_record_append(0,CODE.as_array(),1,DATA.as_array(),0,1));
 check(a[GPU_RECORD_PUSH_USED]==192&&a[GPU_RECORD_COMMAND_USED]==10);
 check(gpu_window_read_words(0x9800,10,DATA.as_array(),1));check(DATA[0u16]==0);
 check(gpu_window_read_words(0x9a00,192,DATA.as_array(),1));check(DATA[0u16]==0);
 DATA[0u16]=0x7000;
 check(gpu_record_preflight(0,48,192));check(!gpu_record_preflight(0,50,192));
 check(!gpu_record_preflight(0,2,193));check(!gpu_record_preflight(0,2,191));
 check(gpu_record_preflight(1,100,128));
 check(gpu_record_begin_pass(1));check(gpu_record_append(1,CODE.as_array(),2,DATA.as_array(),16,16));
 check(gpu_record_close_pass(1));check(gpu_record_seal(1));
 check(b[GPU_RECORD_COMMAND_USED]==8&&b[GPU_RECORD_PADDED]==64);
 check(a[GPU_CB_CAMERA]==0x1111&&b[GPU_CB_CAMERA]==0x2222&&a[GPU_RECORD_PHASE]==GPU_RECORD_PASS);
 check(gpu_record_append(0,CODE.as_array(),48,DATA.as_array(),0,1));
 check(gpu_record_close_pass(0));check(a[GPU_RECORD_COMMAND_USED]==62);check(gpu_record_seal(0));
 check(a[GPU_RECORD_COMMAND_USED]==64&&a[GPU_RECORD_PADDED]==64);
 check(!gpu_record_append(0,CODE.as_array(),2,DATA.as_array(),0,1));
 // Test-only witness in unused fetch padding: repeated seal and reset must not rewrite it.
 DATA[0u16]=0xbeef;check(gpu_window_write_words(0xfc00,20,DATA.as_array(),1));
 DATA[0u16]=0xcafe;check(gpu_window_write_words(0xfc00,6,DATA.as_array(),1));
 check(gpu_record_seal(1));check(gpu_record_reset(1));
 check(b[GPU_RECORD_COMMAND_USED]==0&&b[GPU_RECORD_PUSH_USED]==0&&b[GPU_CB_CAMERA]==0);
 check(!gpu_record_seal(1));check(read_dseg_page(3)==saved);
 // Synthetic cursor near the full 64-KiB limit tests arithmetic, not a rendered stream.
 check(gpu_record_configure(0,0x9800,512,0x9a00,512));
 a[GPU_RECORD_COMMAND_USED]=32760;a[GPU_RECORD_PUSH_USED]=32704;
 check(gpu_record_begin_pass(0));
 check(gpu_record_append(0,CODE.as_array(),2,DATA.as_array(),64,64));
 check(a[GPU_RECORD_PUSH_USED]==32768);check(gpu_record_close_pass(0));check(gpu_record_seal(0));
 check(a[GPU_RECORD_COMMAND_USED]==32768&&a[GPU_RECORD_PADDED]==32768);
 check(gpu_record_seal(0));
 // A transport failure after payload stores poisons the record rather than promising rollback.
 check(gpu_record_begin_pass(1));let invalid=unsafe{Ptr::from_addr(0xc000).as_u16_array()};
 check(!gpu_record_append(1,invalid,2,DATA.as_array(),16,16));
 check(b[GPU_RECORD_PHASE]==GPU_RECORD_POISON&&b[GPU_RECORD_COMMAND_USED]==0&&b[GPU_RECORD_PUSH_USED]==0);
 check(!gpu_record_seal(1)&&!gpu_record_reset(1));check(read_dseg_page(3)==saved);
 halt(0x600d);
}"#;

#[test]
fn record_core_preflights_seals_and_replays_without_a_public_factory() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    for enabled in [false, true] {
        let directory = output.join(format!("record-{enabled}"));
        std::fs::create_dir_all(&directory).unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = compile_program_named_checked(
            "graphics-api-record-core",
            RECORD,
            &options,
            &RustcCheck::new(directory.join("rustc")),
            &mut |name| {
                std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let static_words = parsed.debug.static_data_end;
        assert!(static_words <= 8192);
        for name in [
            "GPU_CB_RECORDS",
            "GPU_NATIVE_SCRATCH",
            "GPU_OBJECT_BACKING",
            "CAPTURE_RESERVATION",
            "GPU_CB_CAPTURE_FACTS",
            "GPU_API_FAULT",
            "GPU_EXECUTION",
            "GPU_PRESENT",
        ] {
            assert!(
                parsed.debug.globals.iter().any(|g| g.name == name),
                "missing budget {name}"
            );
        }
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        std::fs::write(directory.join("layout.txt"), program.debug.render()).unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let mut minimum_sp = 0xc000;
        let mut halted = false;
        let mut steps = 0;
        while steps < 2_000_000 {
            steps += 1;
            let outcome = cpu.step().unwrap();
            let sp = cpu.registers()[13];
            if sp != 0 {
                minimum_sp = minimum_sp.min(sp);
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(
                    signal,
                    0x600d,
                    "profile {enabled}, check {}",
                    signal.wrapping_sub(0xd000)
                );
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded record-core timeout");
        assert!(minimum_sp >= 0xa000);
        let memory = cpu.physical_memory_mut();
        let command = 0x9800 * 64;
        let push = 0x9a00 * 64;
        assert_eq!(
            &memory[command + 58..command + 64],
            &[3, 0x4000, 0, 0x7000, 0, 0xf000]
        );
        assert_eq!(&memory[push + 16..push + 32], &[0xaaaa; 16]);
        assert_eq!(
            &memory[push + 32..push + 64],
            &(0..32).map(|n| 0x7000 + n).collect::<Vec<u16>>()
        );
        let second = 0xfc00 * 64;
        assert_eq!(memory[second + 20], 0xbeef);
        assert_eq!(memory[second + 6], 0xcafe);
        assert_eq!(&memory[second + 64..second + 128], &[0xaaaa; 64]);
        assert_eq!(
            &memory[command + 32762..command + 32768],
            &[3, 0x4000, 0, 0x7000, 0, 0xf000]
        );
        let result = serde_json::json!({"gpu_debug":enabled,"static_words":static_words,
            "static_bytes":static_words*2,"code_words":program.words.len(),"steps":steps,
            "max_frame_words":program.debug.functions.iter().map(|f|f.frame_size).max().unwrap_or(0),
            "observed_stack_words":0xc000-minimum_sp,
            "scope":"Actual CPU record core, allocation facts and window stores; only C bridge remains reserved; no GPU submission, production scene math or G1"});
        std::fs::write(
            directory.join("record.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        println!("RECORD_CORE {result}");
    }
}
