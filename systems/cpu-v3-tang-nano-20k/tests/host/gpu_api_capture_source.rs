//! Actual A-side source/fact storage and read-only health exports; no C shim.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

#[derive(Default)]
struct State {
    events: Vec<(u8, u8, u16)>,
    display_reads: usize,
    pending_reads: usize,
    torn: bool,
}
struct Peer(u8, Rc<RefCell<State>>);
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.1.borrow_mut();
        if self.0 == 3 {
            s.display_reads += 1;
            return match channel {
                1 => 0,
                2 => 0x20,
                3 => {
                    s.pending_reads += 1;
                    if s.torn {
                        (s.pending_reads & 1) as u16
                    } else {
                        0
                    }
                }
                4 => 0,
                _ => panic!("unexpected Display channel {channel}"),
            };
        }
        0
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        self.1.borrow_mut().events.push((self.0, channel, value));
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(label: &str, body: &str, enabled: bool, torn: bool) -> State {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    let source = format!(
        r#"mod gpu_execute;mod gpu_present;
fn check(ok:bool){{if !ok{{halt(0xbad);}}}}
fn main(){{gpu_record_init();{body}halt(0x600d);}}"#
    );
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = enabled;
    let parsed = compile_program_named_checked(
        "capture-source",
        &source,
        &options,
        &RustcCheck::new(output.join(format!("capture-source-{label}-{enabled}"))),
        &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let state = Rc::new(RefCell::new(State {
        torn,
        ..State::default()
    }));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for kind in [0, 3, 4] {
        cpu.attach_device(kind, Box::new(Peer(kind, state.clone())));
    }
    let mut halted = false;
    for _ in 0..3_000_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d, "{label}/{enabled}");
            halted = true;
            break;
        }
    }
    assert!(halted, "bounded capture-source timeout {label}/{enabled}");
    // Devices still hold the shared observer until CPU is dropped.
    drop(cpu);
    Rc::try_unwrap(state)
        .unwrap_or_else(|_| panic!("observer retained"))
        .into_inner()
}

#[test]
fn allocation_facts_group_preflight_deduplicates_and_survives_until_reset() {
    let body = r#"
 check(gpu_record_configure(0,0x9800,1,0x9a00,1));check(gpu_record_configure(1,0xfc00,1,0xfe00,1));
 let mut input:Buf<u16,8>=Buf::new([0x8000,2048,5,1,0x8000,2048,5,2]);
 check(gpu_record_capture_room(0,input.as_array(),2));gpu_record_capture_commit(0,input.as_array(),2);
 check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==1);check(gpu_record_capture_facts(0)[3u16]==3);
 input[0u16]=0x9000;input[1u16]=1600;input[2u16]=6;input[3u16]=3;
 check(gpu_record_capture_room(0,input.as_array(),1));gpu_record_capture_commit(0,input.as_array(),1);
 check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==2&&gpu_record_data(1)[GPU_RECORD_FACT_COUNT]==0);
 let mut i:u16=0;while i<62{input[0u16]=100+i;input[1u16]=1;input[2u16]=1;input[3u16]=1;
 check(gpu_record_capture_room(0,input.as_array(),1));gpu_record_capture_commit(0,input.as_array(),1);i+=1;}
 input[0u16]=0x8000;input[1u16]=2048;input[2u16]=5;input[3u16]=1;
 input[4u16]=300;input[5u16]=1;input[6u16]=1;input[7u16]=1;
 check(!gpu_record_capture_room(0,input.as_array(),2));check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==64);
 check(gpu_record_capture_facts(0)[0u16]==0x8000&&gpu_record_capture_facts(0)[3u16]==3);
 check(gpu_record_capture_room(0,input.as_array(),1));gpu_record_capture_commit(0,input.as_array(),1);
 check(gpu_record_capture_room(1,input.as_array(),2));gpu_record_capture_commit(1,input.as_array(),2);
 check(gpu_record_reset(1));check(gpu_record_data(1)[GPU_RECORD_FACT_COUNT]==0&&gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==64);
 check(gpu_record_reset(0));check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==0);
 input[1u16]=0;check(!gpu_record_capture_room(0,input.as_array(),1));
 check(gpu_record_data(0)[GPU_RECORD_FACT_COUNT]==0);
 "#;
    for enabled in [false, true] {
        let state = run("facts", body, enabled, false);
        assert!(state.events.is_empty());
        assert_eq!(state.display_reads, 0);
    }
}

#[test]
fn source12_and_prepared_health_are_read_only_and_replay_stable() {
    let body = r#"
 let out:Buf<u16,12>=Buf::new([0xa55a;12]);check(!gpu_execute_capture_source(0,out.as_array()));check(out[0u16]==0xa55a);
 check(gpu_record_configure(0,0x9800,1,0x9a00,1));let code:Buf<u16,2>=Buf::new([3,0x4000]);
 check(gpu_record_begin_pass(0));check(gpu_record_append(0,code.as_array(),2,code.as_array(),0,1));check(gpu_record_close_pass(0));
 check(gpu_execute_init());check(gpu_execute_prepare(0));
 check(gpu_execute_capture_source(0,out.as_array()));check(out[0u16]==1&&out[1u16]==0&&out[2u16]==0x9800&&out[3u16]==64);
 check(out[4u16]==8&&out[5u16]==64&&out[6u16]==0x9a00&&out[7u16]==64&&out[8u16]==0&&out[9u16]==0&&out[10u16]==4&&out[11u16]==2);
 check(gpu_execute_capture_health(0)==0&&GPU_EXECUTION[0u16]==GPU_EXEC_PREPARED&&GPU_EXECUTION[1u16]==1);
 check(!gpu_execute_capture_source(1,out.as_array())&&out[2u16]==0x9800);
 let saved=read_dseg_page(3);unsafe{write_dseg_page(3,saved+1);}check(gpu_execute_capture_health(0)!=0);
 check(GPU_EXECUTION[0u16]==GPU_EXEC_PREPARED&&GPU_EXECUTION[5u16]==0);unsafe{write_dseg_page(3,saved);}
 gpu_record_data(0)[GPU_RECORD_PUSH_USED]=1;check(gpu_execute_capture_health(0)!=0);gpu_record_data(0)[GPU_RECORD_PUSH_USED]=0;
 check(gpu_execute_start(0,true));check(gpu_execute_poll()==1);check(gpu_execute_prepare(0));
 check(gpu_execute_capture_source(0,out.as_array())&&out[4u16]==8&&out[5u16]==64);
 check(gpu_execute_start(0,false));check(gpu_execute_poll()==1);
 "#;
    for enabled in [false, true] {
        let state = run("prepared", body, enabled, false);
        assert_eq!(
            state
                .events
                .iter()
                .filter(|e| e.0 == 4)
                .copied()
                .collect::<Vec<_>>(),
            vec![(4, 0, 3), (4, 1, 0x9800), (4, 0, 3), (4, 1, 0x9800)]
        );
        assert_eq!(
            state.events.iter().filter(|e| e.0 == 0 && e.1 == 1).count(),
            3
        );
        assert_eq!(state.display_reads, 0);
    }
}

#[test]
fn display_capture_export_does_not_open_wait_or_clear_errors() {
    for enabled in [false, true] {
        let absent = run(
            "display-absent",
            r#"let out:Buf<u16,4>=Buf::new([0xa55a;4]);check(gpu_present_capture_state(out.as_array())==0&&out[0u16]==0xa55a);
GPU_PRESENT[3u16]=7;check(gpu_present_capture_state(out.as_array())==65535&&GPU_PRESENT[3u16]==7);"#,
            enabled,
            false,
        );
        assert_eq!(absent.display_reads, 0);
        assert!(absent.events.is_empty());
        let present = run(
            "display-open",
            r#"let out:Buf<u16,4>=Buf::new([0xa55a;4]);GPU_PRESENT[0u16]=1;
check(gpu_present_capture_state(out.as_array())==2&&out[0u16]==0&&out[1u16]==0&&out[2u16]==0x20&&out[3u16]==0);
check(GPU_PRESENT[0u16]==1&&GPU_PRESENT[1u16]==0&&GPU_PRESENT[3u16]==0);"#,
            enabled,
            false,
        );
        assert_eq!(present.display_reads, 5);
        assert!(present.events.is_empty());
        let torn = run(
            "display-torn",
            r#"let out:Buf<u16,4>=Buf::new([0xa55a;4]);GPU_PRESENT[0u16]=1;
check(gpu_present_capture_state(out.as_array())==1&&out[0u16]==0xa55a&&GPU_PRESENT[3u16]==0);"#,
            enabled,
            true,
        );
        assert_eq!(torn.display_reads, 5);
        assert!(torn.events.is_empty());
    }
}
