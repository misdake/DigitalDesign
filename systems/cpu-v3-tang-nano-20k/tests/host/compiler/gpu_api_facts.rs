//! Resource provenance planning survives successful recording and rejects overflow.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;
fn run(label: &str, body: &str, enabled: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    let source=format!("mod gpu_record;fn check(ok:bool){{if !ok{{halt(0xbad);}}}}fn main(){{gpu_record_init();{body}halt(0x600d);}}");
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = enabled;
    let parsed = crate::checked_flat_fixture::compile(
        "resource-facts",
        &source,
        &options,
        &RustcCheck::new(out.join(format!("facts-{label}-{enabled}"))),
        &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for _ in 0..3_000_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d);
            return;
        }
    }
    panic!("bounded resource-fact timeout");
}
#[test]
fn allocation_facts_group_preflight_deduplicates_and_survives_until_reset() {
    let body = r#"
 check(gpu_record_configure(0,0x9800,1,0x9a00,1));check(gpu_record_configure(1,0xfc00,1,0xfe00,1));
 let fact_plan:Buf<u16,2>=Buf::new([0;2]);
 let mut input:Buf<u16,8>=Buf::new([0x8000,2048,5,1,0x8000,2048,5,2]);
 check(gpu_record_resource_room(0,input.as_array(),2, fact_plan.as_array()));gpu_record_resource_commit(0,input.as_array(),2, fact_plan.as_array());
 check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==1);check(gpu_record_resource_facts(0)[3u16]==3);
 input[0u16]=0x9000;input[1u16]=1600;input[2u16]=6;input[3u16]=3;
 check(gpu_record_resource_room(0,input.as_array(),1, fact_plan.as_array()));gpu_record_resource_commit(0,input.as_array(),1, fact_plan.as_array());
 check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==2&&gpu_record_data(1)[GPU_RECORD_FACT_COUNT]==0);
 let mut i:u16=0;while i<62{input[0u16]=100+i;input[1u16]=1;input[2u16]=1;input[3u16]=1;
 check(gpu_record_resource_room(0,input.as_array(),1, fact_plan.as_array()));gpu_record_resource_commit(0,input.as_array(),1, fact_plan.as_array());i+=1;}
 input[0u16]=0x8000;input[1u16]=2048;input[2u16]=5;input[3u16]=1;
 input[4u16]=300;input[5u16]=1;input[6u16]=1;input[7u16]=1;
 check(!gpu_record_resource_room(0,input.as_array(),2, fact_plan.as_array()));check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==64);
 check(gpu_record_resource_facts(0)[0u16]==0x8000&&gpu_record_resource_facts(0)[3u16]==3);
 check(gpu_record_resource_room(0,input.as_array(),1, fact_plan.as_array()));gpu_record_resource_commit(0,input.as_array(),1, fact_plan.as_array());
 check(gpu_record_resource_room(1,input.as_array(),2, fact_plan.as_array()));gpu_record_resource_commit(1,input.as_array(),2, fact_plan.as_array());
 check(gpu_record_reset(1));check(gpu_record_data(1)[GPU_RECORD_FACT_COUNT]==0&&gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==64);
 check(gpu_record_reset(0));check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==0);
 input[1u16]=0;check(!gpu_record_resource_room(0,input.as_array(),1, fact_plan.as_array()));
 check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==0);
 "#;
    for enabled in [false, true] {
        run("facts", body, enabled);
    }
}
