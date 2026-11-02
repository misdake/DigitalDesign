//! One execution owner, explicit capture eligibility and real-START accounting.
use super::gpu_api_scene_patch::{compile, global};
use super::shared_build_support_rcc_modules::capture_profile::abi::GPU_CAPTURE_H_SUBMISSION;
use cpu_v3::{CpuV3Sim, Device, StepOutcome};
use cpu_v3_tang_nano_20k::boot::{SYSCTL_MILLISECONDS, SYSCTL_UART};
use std::{any::Any, cell::RefCell, rc::Rc};

const SOURCE: &str = r#"mod gpu_api;use gpu_api::*;
fn main(){
 let memory:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:memory,command_b:absent});
 let mut a=gpu.command_buffer();
 let color=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 {let p=a.pass(&color,&depth,ColorLoad::clear(Rgb565::from_raw(0x1234)),DepthLoad::clear(65535));}
 dev_send(7,1,0);gpu.submit_capturable(a);a=gpu.wait();
 dev_send(7,1,1);gpu.poll_snapshot();gpu.poll_snapshot();
 dev_send(7,1,2);gpu.request_snapshot();
 gpu.submit(a);a=gpu.wait();
 dev_send(7,1,3);gpu.submit_capturable(a);a=gpu.wait();
 gpu.submit_capturable(a);a=gpu.wait();dev_send(7,1,4);
 drop(depth);drop(color);drop(a);drop(gpu);halt(0x600d);
}"#;
#[derive(Default)]
struct Seen {
    starts: usize,
    gates: Vec<(usize, u16)>,
    uart: Vec<u8>,
    buttons: u16,
    milliseconds: u16,
    paused_reads: usize,
    physical: bool,
    runtime: bool,
    paused: bool,
    carry: bool,
    fail_start: bool,
    gpu_reads: usize,
}
struct Peer {
    kind: u8,
    control: usize,
    header: usize,
    submission: usize,
    seen: Rc<RefCell<Seen>>,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.seen.borrow_mut();
        if self.kind == 4 {
            s.gpu_reads += 1;
            if s.fail_start && s.gpu_reads == 4 {
                2
            } else {
                0
            }
        } else if self.kind == 0 {
            match channel {
                SYSCTL_MILLISECONDS => {
                    s.milliseconds = s.milliseconds.wrapping_add(25);
                    s.milliseconds
                }
                8 => {
                    if s.runtime && s.paused {
                        s.paused_reads += 1;
                        // Keep the original pause held, then release and make
                        // a fresh press. The held samples must not start work.
                        if s.paused_reads <= 5 {
                            assert_eq!(s.starts, 2);
                        }
                        if (6..=8).contains(&s.paused_reads) {
                            0
                        } else {
                            2
                        }
                    } else {
                        s.buttons
                    }
                }
                _ => 0,
            }
        } else {
            0
        }
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut s = self.seen.borrow_mut();
        if self.kind == 4 && channel == 1 {
            s.starts += 1;
        } else if self.kind == 7 {
            match channel {
                1 => {
                    if value == 0 {
                        memory[self.header + self.submission] = 0xdead;
                        memory[self.header + self.submission + 1] = 0xbeef;
                        s.buttons = if s.physical || s.runtime { 2 } else { 0 };
                        if s.carry {
                            memory[self.control + 4] = 65533;
                            memory[self.control + 5] = 0x42;
                        }
                    }
                    if value == 1 {
                        s.buttons = 0;
                    }
                    if value == 2 {
                        s.buttons = if s.physical || s.runtime { 2 } else { 0 };
                    }
                    if value == 3 {
                        assert_eq!(
                            &memory
                                [self.header + self.submission..self.header + self.submission + 2],
                            &[0xdead, 0xbeef],
                            "ordinary/unarmed paths must not build a descriptor"
                        );
                        assert_eq!(
                            &memory[self.control + 2..self.control + 4],
                            &[0, 0],
                            "no earlier capture begin"
                        );
                        assert_eq!(
                            memory[self.control + 4],
                            if s.carry { 65535 } else { 2 },
                            "ordinary START preserves pending arm and ignores stale header"
                        );
                    }
                    if value == 4 {
                        assert_eq!(
                            &memory[self.control + 2..self.control + 4],
                            &[1, 0],
                            "exactly one eligible begin"
                        );
                        assert_eq!(memory[self.control + 4], if s.carry { 1 } else { 4 });
                        assert_eq!(memory[self.control + 5], if s.carry { 0x43 } else { 0 });
                    }
                }
                2 => {
                    let starts = s.starts;
                    s.gates.push((starts, value));
                }
                3 => assert_eq!(value, if s.carry { 0x43 } else { 0 }),
                _ => panic!("unexpected marker {channel}"),
            }
        } else if self.kind == 0 && channel == SYSCTL_UART {
            s.uart.push(value as u8);
            if s.uart.ends_with(b"GPUPROF:P\n") {
                s.paused = true;
            }
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(debug: bool, physical: bool, runtime: bool) -> Seen {
    run_options(debug, physical, runtime, false, false)
}
fn run_options(debug: bool, physical: bool, runtime: bool, carry: bool, fail_start: bool) -> Seen {
    let source = if physical {
        SOURCE.replace(
            "gpu.request_snapshot();",
            "gpu.poll_snapshot();gpu.poll_snapshot();",
        )
    } else {
        SOURCE.to_owned()
    };
    let code = compile(
        &source,
        debug,
        if runtime {
            "submit-runtime"
        } else {
            "submit-modes"
        },
    );
    let seen = Rc::new(RefCell::new(Seen {
        physical,
        runtime,
        carry,
        fail_start,
        ..Seen::default()
    }));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    let abi = usize::from(GPU_CAPTURE_H_SUBMISSION);
    for kind in [0, 4, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                control: global(&code, "GPU_CAPTURE_CONTROL"),
                header: global(&code, "GPU_CAPTURE_HEADER"),
                submission: abi,
                seen: seen.clone(),
            }),
        );
    }
    let mut halted = false;
    for _ in 0..1_000_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(
                signal,
                if fail_start { 0xe001 } else { 0x600d },
                "{debug}/{physical}/{runtime} UART={:?}",
                seen.borrow().uart
            );
            if fail_start {
                let at = global(&code, "GPU_CAPTURE_CONTROL");
                assert_eq!(&cpu.physical_memory_mut()[at + 4..at + 6], &[0, 0]);
            }
            halted = true;
            break;
        }
    }
    assert!(halted, "submit fixture reached 1M instruction bound");
    drop(cpu);
    Rc::try_unwrap(seen).ok().unwrap().into_inner()
}
#[test]
fn ordinary_and_unarmed_submissions_bypass_gate_and_do_not_consume_pending_request() {
    for debug in [false, true] {
        let state = run(debug, false, false);
        assert_eq!(state.starts, 4);
        assert_eq!(state.gates, [(2, 3)]);
    }
}
#[test]
fn held_baseline_and_debounced_poll_arm_only_a_fresh_pause_edge() {
    for debug in [false, true] {
        let state = run(debug, true, false);
        assert_eq!(state.starts, 4);
        assert_eq!(state.gates, [(2, 3)]);
    }
}
#[test]
fn requested_runtime_lease_requires_release_and_a_fresh_resume_edge() {
    for debug in [false, true] {
        let state = run(debug, false, true);
        assert_eq!(state.starts, 4);
        assert!(state.paused);
        assert!(state.paused_reads >= 10);
        assert!(state.uart.windows(10).any(|b| b == b"GPUPROF:R\n"));
    }
}

#[test]
fn common_counter_carries_and_a_rejected_start_never_advances_it() {
    for debug in [false, true] {
        let state = run_options(debug, false, false, true, false);
        assert_eq!(state.starts, 4);
        assert_eq!(state.gates, [(2, 0)]);
        let state = run_options(debug, false, false, false, true);
        assert_eq!(state.starts, 0);
        assert!(state.gates.is_empty());
    }
}
