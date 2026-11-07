//! Actual approved A facade and fixed C implementation. Device protocol only;
//! production GPU pixel execution has a separate bounded qualification.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use std::{any::Any, cell::RefCell, rc::Rc};

pub(super) const APPLICATION: &str = r#"mod gpu_api;use gpu_api::*;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let b:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0xfc00,512)},push:unsafe{GpuMemory::from_sectors(0xfe00,512)}};
 let mut gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:Some(b)});
 let mut first:CommandBuffer=gpu.command_buffer();let mut second:CommandBuffer=gpu.command_buffer();
 let color:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth:DepthBuffer=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 {let pass=first.pass(&color,&depth,ColorLoad::clear(Rgb565::from_raw(0x1357)),DepthLoad::load());}
 {let pass=second.pass(&color,&depth,ColorLoad::load(),DepthLoad::clear(0x2468));}
 gpu.submit(first);first=gpu.wait();let sealed=first.command_words();
 let absent:Option<CommandBuffer>=gpu.try_wait();if let Some(unexpected)=absent{drop(unexpected);halt(0xbad);}
 gpu.submit(second);second=gpu.wait();if dev_recv(1,0)!=0{gpu.request_snapshot();}gpu.submit_capturable(first);first=gpu.wait();
 check(first.command_words()==sealed);first.reset();check(first.command_words()==0&&first.push_words()==0);
 drop(first);drop(second);drop(color);drop(depth);drop(gpu);halt(0x600d);
}"#;

#[derive(Default)]
pub(super) struct Trace {
    events: Vec<(u8, u8, u16)>,
    busy: u8,
    count: u16,
    images: Vec<Vec<u16>>,
    pub(super) rtl: Option<std::path::PathBuf>,
    resume_test: bool,
    fault_while_paused: bool,
    paused: bool,
    button_reads: usize,
    milliseconds: u16,
}
pub(super) struct Peer(pub(super) u8, pub(super) Rc<RefCell<Trace>>);
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        if self.0 == 1 && channel == 0 {
            return u16::from(self.1.borrow().resume_test);
        }
        if self.0 == 0 {
            let mut state = self.1.borrow_mut();
            if channel == 10 {
                state.milliseconds = state.milliseconds.wrapping_add(25);
                return state.milliseconds;
            }
            if channel == 8 && state.resume_test {
                if state.paused {
                    let index = state.button_reads;
                    state.button_reads += 1;
                    return [2, 0, 0, 2, 2][index.min(4)];
                }
                return if state.images.is_empty() { 0 } else { 2 };
            }
        }
        if self.0 == 4 {
            assert_eq!(channel, 2);
            let mut state = self.1.borrow_mut();
            if state.fault_while_paused && state.paused {
                return 2;
            }
            if state.busy != 0 {
                state.busy -= 1;
                return 1;
            }
        }
        0
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut state = self.1.borrow_mut();
        if self.0 == 0 && channel == 2 && value == 2 && state.resume_test {
            state.paused = true;
        }
        if (self.0 == 0 && channel == 1) || self.0 == 4 {
            state.events.push((self.0, channel, value));
        }
        if self.0 == 4 && channel == 0 {
            assert_eq!(state.busy, 0);
            state.count = value;
        }
        if self.0 == 4 && channel == 1 {
            assert_eq!(state.busy, 0);
            state.busy = 3;
            let base = usize::from(value) * 64;
            let words = (usize::from(state.count) + 1) * 2;
            let padded = (words + 63) & !63;
            state.images.push(memory[base..base + padded].to_vec());
            if let Some(directory) = &state.rtl {
                super::gpu_api_pixels::execute(directory, memory, value, state.count);
            }
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn actual_a_c_public_submit_wait_and_interleaved_replay() {
    for debug in [false, true] {
        let parsed = super::gpu_api_resources::parse(APPLICATION, "actual-a-c-replay", debug)
            .unwrap_or_else(|error| panic!("{error}"));
        let global = |name| match parsed
            .debug
            .globals
            .iter()
            .find(|item| item.name == name)
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(address) => usize::from(address),
            _ => panic!("non-global capture metadata"),
        };
        let execution = global("GPU_EXECUTION");
        let capture = global("GPU_CAPTURE_CONTROL");
        assert!(
            parsed.debug.static_data_end <= 8192,
            "static prefix exceeds 16 KiB"
        );
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        for mode in 0..3 {
            let trace = Rc::new(RefCell::new(Trace {
                resume_test: mode != 0,
                fault_while_paused: mode == 2,
                ..Trace::default()
            }));
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&program.words);
            for kind in [0, 1, 4] {
                cpu.attach_device(kind, Box::new(Peer(kind, trace.clone())));
            }
            let mut halted = false;
            for _ in 0..3_000_000 {
                if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                    assert_eq!(signal, if mode == 2 { 0xe001 } else { 0x600d });
                    halted = true;
                    break;
                }
            }
            assert!(halted, "bounded public A/C replay timeout");
            let trace = trace.borrow();
            if mode == 2 {
                assert_eq!(trace.images.len(), 2, "fatal gate issued START");
                let memory = cpu.physical_memory_mut();
                assert_eq!(memory[execution], 5);
                assert_eq!(memory[execution + 1], 1, "fault released driver ownership");
                assert_eq!(memory[execution + 5], 10);
                assert_eq!(memory[capture], 1, "fatal capture lease was lost");
                assert_ne!(memory[capture + 1], 0);
                assert_eq!(memory[capture + 4], 2, "fatal lease was marked submitted");
                continue;
            }
            assert_eq!(trace.images.len(), 3);
            assert_eq!(
                trace.images[0], trace.images[2],
                "replay changed native stream"
            );
            assert_ne!(trace.images[0], trace.images[1]);
            let starts: Vec<_> = trace
                .events
                .iter()
                .filter(|event| event.0 == 4 && event.1 == 1)
                .map(|event| event.2)
                .collect();
            assert_eq!(starts, [0x9800, 0xfc00, 0x9800]);
            let order: Vec<_> = trace
                .events
                .iter()
                .map(|event| {
                    if event.0 == 0 {
                        'P'
                    } else if event.1 == 0 {
                        'C'
                    } else {
                        'S'
                    }
                })
                .collect();
            let expected = if mode == 1 {
                vec!['P', 'C', 'S', 'P', 'C', 'S', 'P', 'P', 'C', 'S']
            } else {
                vec!['P', 'C', 'S', 'P', 'C', 'S', 'P', 'C', 'S']
            };
            assert_eq!(order, expected);
        }
    }
}
