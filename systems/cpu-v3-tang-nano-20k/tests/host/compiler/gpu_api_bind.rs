//! Independent native-byte oracle for canonical binding and immutable push reuse.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;

const SOURCE: &str = r#"mod gpu_bind;mod gpu_arena;mod gpu_execute;mod gpu_present;mod gpu_api_fault;
static CAPTURE_RESERVATION:Buf<u16,656>=Buf::new([0;656]);
static DEPENDENCY_RESERVATION:Buf<u16,1024>=Buf::new([0;1024]);
static STEP:u16=0;static PLAN:Buf<u16,6>=Buf::new([0;6]);
static DESIRED:Buf<u16,48>=Buf::new([0;48]);static COPY:Buf<u16,256>=Buf::new([0;256]);
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){gpu_record_init();check(gpu_arena_init(8192));check(gpu_record_configure(0,0x9800,8,0x9a00,16));
 check(gpu_record_configure(1,0xfc00,4,0xfe00,16));
 let mut native=gpu_native_scratch();native[1u16]=199;native[3u16]=16384;
 native[5u16]=57344;native[6u16]=60621;native[7u16]=8192;native[8u16]=128;native[9u16]=128;native[10u16]=33;
 native[36u16]=9;native[65u16]=1;native[75u16]=1;native[85u16]=1;native[89u16]=1;native[97u16]=1;
 native[105u16]=1;native[107u16]=1;native[108u16]=65535;native[109u16]=65535;
 DESIRED[0u16]=0x1111;DESIRED[32u16]=0x2222;
 check(!gpu_bind_desired(0,true,DESIRED.as_array()));check(gpu_packet_begin(0,0x8000,0x9000,0,0,0));
 check(gpu_bind_desired(0,true,DESIRED.as_array()));
 let state=unsafe{DESIRED.as_ptr().add(32).as_u16_array()};check(gpu_bind_desired(0,false,state));
 DESIRED[0u16]=0x3333;DESIRED[32u16]=0x4444;
 let mut c=gpu_record_data(0);check(c[GPU_CB_CAMERA]==0x1111&&c[GPU_CB_STATE]==0x2222);
 check(gpu_bind_plan(0,native,PLAN.as_array()));check(PLAN[0u16]==7&&PLAN[1u16]==64&&PLAN[2u16]==96&&PLAN[3u16]==128&&PLAN[4u16]==192&&PLAN[5u16]==18);
 check(gpu_record_preflight(0,PLAN[5u16],PLAN[4u16]));check(gpu_bind_emit(0,native,PLAN.as_array()));check(c[GPU_RECORD_COMMAND_USED]==28&&c[GPU_RECORD_PUSH_USED]==192&&c[GPU_RECORD_VALID]==31);
 check(gpu_bind_install(0,native));check(c[GPU_RECORD_COMMAND_USED]==28&&c[GPU_RECORD_PUSH_USED]==192);
 native[108u16]=0x1234;check(gpu_bind_install(0,native));check(c[GPU_RECORD_COMMAND_USED]==32&&c[GPU_RECORD_PUSH_USED]==256);
 native[8u16]=64;native[36u16]=11;check(gpu_bind_plan(0,native,PLAN.as_array()));
 check(PLAN[0u16]==3&&PLAN[1u16]==256&&PLAN[2u16]==288&&PLAN[3u16]==192&&PLAN[4u16]==304&&PLAN[5u16]==14);
 check(gpu_record_preflight(0,PLAN[5u16],PLAN[4u16]));check(gpu_bind_emit(0,native,PLAN.as_array()));check(c[GPU_RECORD_COMMAND_USED]==46&&c[GPU_RECORD_PUSH_USED]==304);
 check(gpu_packet_begin(1,0x8800,0x9000,0,0,0));check(gpu_bind_install(1,native));
 let b=gpu_record_data(1);check(b[GPU_RECORD_COMMAND_USED]==28&&b[GPU_RECORD_PUSH_USED]==192);
 native[36u16]=9;check(gpu_bind_plan(1,native,PLAN.as_array()));check(PLAN[0u16]==2&&PLAN[5u16]==8);
 check(gpu_record_preflight(1,PLAN[5u16],PLAN[4u16]));check(gpu_bind_emit(1,native,PLAN.as_array()));check(b[GPU_RECORD_COMMAND_USED]==36&&b[GPU_RECORD_PUSH_USED]==208);
 native[8u16]=96;check(gpu_bind_plan(1,native,PLAN.as_array()));check(PLAN[0u16]==1&&PLAN[5u16]==8);
 check(gpu_record_preflight(1,PLAN[5u16],PLAN[4u16]));check(gpu_bind_emit(1,native,PLAN.as_array()));check(b[GPU_RECORD_COMMAND_USED]==44&&b[GPU_RECORD_PUSH_USED]==256);
 native[8u16]=64;native[36u16]=11;let mut i:u16=0;while i<256{COPY[i]=native[i];i+=1;}
 check(gpu_bind_install(0,COPY.as_array()));check(c[GPU_RECORD_COMMAND_USED]==46&&c[GPU_RECORD_PUSH_USED]==304);
 // Synthetic capacity pressure: no cache, command, or push writes may commit.
 c[GPU_RECORD_COMMAND_USED]=500;native[8u16]=32;check(!gpu_bind_install(0,native));
 check(c[GPU_RECORD_COMMAND_USED]==500&&c[GPU_RECORD_PUSH_USED]==304&&c[GPU_CB_GLOBAL+8]==64);
 check(gpu_window_read_words(0x9800,500,PLAN.as_array(),1));check(PLAN[0u16]==0);
 c[GPU_RECORD_COMMAND_USED]=46;c[GPU_RECORD_PUSH_USED]=1000;check(!gpu_bind_install(0,native));
 check(c[GPU_RECORD_COMMAND_USED]==46&&c[GPU_RECORD_PUSH_USED]==1000&&c[GPU_CB_GLOBAL+8]==64);
 c[GPU_RECORD_PUSH_USED]=304;native[8u16]=64;
 check(gpu_record_close_pass(0));check(gpu_record_seal(0));check(c[GPU_RECORD_COMMAND_USED]==52);
 check(gpu_record_close_pass(1));check(gpu_record_seal(1));check(b[GPU_RECORD_COMMAND_USED]==50);
 halt(0x600d);}"#;

fn dma(base: u32, word: u32, local: u32, size: u32) -> u32 {
    0x10000000 | (((base * 128 + word * 2) / 32) << 10) | (local << 3) | (size << 1)
}
fn prefix(base: u32) -> Vec<u32> {
    vec![
        0x40000003,
        0x70000000,
        dma(base, 0, 69, 0),
        0x40000001,
        0xa0000045,
    ]
}
fn first_binding(base: u32) -> Vec<u32> {
    vec![
        0x40000003,
        dma(base, 64, 66, 1),
        0x40000001,
        0x50000042,
        dma(base, 96, 68, 0),
        0x40000001,
        0x60000044,
        dma(base, 128, 80, 2),
        0x40000001,
    ]
}
#[test]
fn canonical_bindings_share_drains_and_reuse_immutable_instance_payloads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    for enabled in [false, true] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = crate::checked_flat_fixture::compile(
            "canonical-binding",
            SOURCE,
            &options,
            &RustcCheck::new(output.join(format!("bind-{enabled}"))),
            &mut |name| {
                std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let static_words = parsed.debug.static_data_end;
        assert!(static_words <= 8192);
        for name in ["GPU_OBJECT_BACKING", "GPU_CB_RECORDS", "GPU_NATIVE_SCRATCH"] {
            assert!(parsed.debug.globals.iter().any(|g| g.name == name));
        }
        for name in ["CAPTURE_RESERVATION", "DEPENDENCY_RESERVATION"] {
            assert!(!parsed.debug.globals.iter().any(|g| g.name == name));
        }
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let mut minimum_sp = 0xc000;
        let mut halted = false;
        let mut steps = 0;
        while steps < 1_000_000 {
            steps += 1;
            let outcome = cpu.step().unwrap();
            let sp = cpu.registers()[13];
            if sp != 0 {
                minimum_sp = minimum_sp.min(sp);
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded canonical bind timeout");
        assert!(minimum_sp >= 0xa000);
        let mut a = prefix(0x9a00);
        a.extend(first_binding(0x9a00));
        a.extend([dma(0x9a00, 192, 80, 2), 0x40000001]);
        a.extend([
            0x40000003,
            dma(0x9a00, 256, 66, 1),
            0x40000001,
            0x50000042,
            dma(0x9a00, 288, 68, 0),
            0x40000001,
            0x60000044,
        ]);
        a.extend([0x40000003, 0x70000000, 0xf0000000]);
        let mut b = prefix(0xfe00);
        b.extend(first_binding(0xfe00));
        b.extend([
            0x40000003,
            dma(0xfe00, 192, 68, 0),
            0x40000001,
            0x60000044,
            0x40000003,
            dma(0xfe00, 224, 66, 1),
            0x40000001,
            0x50000042,
            0x40000003,
            0x70000000,
            0xf0000000,
        ]);
        let memory = cpu.physical_memory_mut();
        for (base, code) in [(0x9800, a), (0xfc00, b)] {
            let mut expected = code
                .iter()
                .flat_map(|w| [*w as u16, (*w >> 16) as u16])
                .collect::<Vec<_>>();
            expected.resize(64, 0);
            assert_eq!(
                &memory[base * 64..base * 64 + 64],
                expected,
                "slot {base:x}/{enabled}"
            );
        }
        let push = 0x9a00 * 64;
        assert_eq!(memory[push + 64 + 8], 128);
        assert_eq!(memory[push + 256 + 8], 64);
        assert_eq!(memory[push + 128 + 44], 65535);
        assert_eq!(memory[push + 192 + 44], 0x1234);
        assert_eq!(memory[push + 96 + 4], 9);
        assert_eq!(memory[push + 288 + 4], 11);
        let directory = output.join(format!("bind-{enabled}"));
        let result = serde_json::json!({"gpu_debug":enabled,"static_bytes":static_words*2,
            "observed_stack_bytes":(0xc000-minimum_sp)*2,"steps":steps,"code_words":program.words.len(),
            "max_frame_words":program.debug.functions.iter().map(|f|f.frame_size).max().unwrap_or(0),
            "scope":"actual A arena/record/bind/execute/present/fault storage plus capture/dependency reservations and fixture buffers; not integrated A+B+C or G1"});
        std::fs::write(
            directory.join("binding.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        println!("BINDING {result}");
    }
}
