//! Clock advances only at explicit host events, never as a consequence of RECV.
use super::*;
use cpu_v3::StepOutcome;

#[derive(Default)]
struct Clock {
    now: u64,
    base: u64,
    clears: usize,
}
impl Clock {
    fn set(&mut self, now: u64) {
        assert!(now >= self.now);
        self.now = now;
    }
    fn ms(&self) -> u16 {
        (65530 + self.now / 1000) as u16
    }
    fn us(&self) -> u16 {
        (self.now - self.base).min(65535) as u16
    }
}
struct ClockPeer {
    clock: Rc<RefCell<Clock>>,
    inner: Peer,
}
impl cpu_v3::Device for ClockPeer {
    fn read(&mut self, memory: &mut [u16], channel: u8) -> u16 {
        match channel {
            10 => self.clock.borrow().ms(),
            11 => self.clock.borrow().us(),
            _ => self.inner.read(memory, channel),
        }
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        if channel == 11 {
            assert_eq!(value, 1);
            let mut c = self.clock.borrow_mut();
            c.base = c.now;
            c.clears += 1;
        } else {
            self.inner.write(memory, channel, value);
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn drain(s: &mut State) {
    assert!(s.pending);
    s.active = s.next;
    s.format = s.staged_format;
    s.pending = false;
}

#[test]
fn observed_completion_includes_delay_and_slot_samples_keep_their_own_epochs() {
    for late in [20_000u64, 100_000] {
        let expected_observed = if late == 20_000 { 17000 } else { 62535 };
        let observed_valid = if late == 20_000 { 1 } else { 0 };
        let prep2 = if late == 20_000 { 16000 } else { 61535 };
        let body = format!(
            r#"{{let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);signal(1,1);
let mut a=renderer_frame(&mut r,0x1111);let before=frame_previous(&mut a);check(!before.ready);signal(1,2);
frame_number(&mut a,&mut field,0);let mut p=frame_ui_pass(&mut a);pass_text(&mut p,&mut field);pass_complete(p);frame_complete(a);signal(1,3);
let mut b=renderer_frame(&mut r,0x1112);let prior=frame_previous(&mut b);check(!prior.ready);signal(1,4);
frame_number(&mut b,&mut field,1);let mut q=frame_ui_pass(&mut b);pass_text(&mut q,&mut field);pass_complete(q);signal(1,5);
frame_complete(b);signal(1,6);
let mut c=renderer_frame(&mut r,0x1113);let previous=frame_previous(&mut c);
check(previous.ready && previous.sequence==1 && previous.prepare_valid && previous.prepare_us==2000);
check(previous.submit_to_observed_done_us=={expected_observed});
check(frame_elapsed_ms(&mut c)=={});signal(1,7);
let mut z=frame_ui_pass(&mut c);pass_observed_time(&mut z,&mut field);pass_complete(z);signal(1,8);
frame_complete(c);signal(1,9);renderer_close(r);signal(1,10);}}
"#,
            late / 1000 - 3
        );
        let program = compile("", &body);
        let address = |name: &str| match program
            .debug
            .globals
            .iter()
            .find(|g| g.name == name)
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(a) => usize::from(a),
            _ => panic!("not global"),
        };
        let last = address("GPU_RENDERER_LAST");
        let times = address("GPU_RENDERER_TIMES");
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let state = attach(&mut cpu, Some(vec![vec![(0, 8)]; 3]));
        state.borrow_mut().expected_depth = Some(65535);
        let clock = Rc::new(RefCell::new(Clock::default()));
        cpu.attach_device(
            0,
            Box::new(ClockPeer {
                clock: clock.clone(),
                inner: Peer {
                    kind: 0,
                    state: state.clone(),
                },
            }),
        );
        let mut snapshots = std::collections::BTreeMap::new();
        let mut third_done = false;
        let mut third_drained = false;
        let mut halted = false;
        for _ in 0..5_000_000 {
            match cpu.step().unwrap() {
                StepOutcome::Signaled(event) if event.signal_type == 1 => {
                    let memory = cpu.physical_memory_mut();
                    snapshots.insert(
                        event.value,
                        (
                            memory[last..last + 8].to_vec(),
                            memory[times..times + 24].to_vec(),
                        ),
                    );
                    match event.value {
                        1 => clock.borrow_mut().set(1000),
                        2 => clock.borrow_mut().set(3000),
                        3 => clock.borrow_mut().set(4000),
                        4 => {
                            clock.borrow_mut().set(5000);
                            state.borrow_mut().gpu = 0;
                        }
                        5 => {
                            assert_eq!(
                                clock.borrow().clears,
                                1,
                                "timer reset/poll during recording"
                            );
                            assert_eq!(snapshots[&5].0[0], 0);
                            clock.borrow_mut().set(late);
                        }
                        6 => {
                            clock.borrow_mut().set(late + 1000);
                            drain(&mut state.borrow_mut());
                        }
                        7 => {
                            clock.borrow_mut().set(late + 3000);
                            state.borrow_mut().gpu = 0;
                        }
                        8 => clock.borrow_mut().set(late + 4000),
                        9 => {
                            clock.borrow_mut().set(late + 5000);
                            drain(&mut state.borrow_mut());
                        }
                        10 => {}
                        _ => panic!("marker"),
                    }
                }
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d, "markers {:?}", snapshots.keys());
                    halted = true;
                    break;
                }
                _ => {}
            }
            if state.borrow().starts.len() == 3 && !third_done {
                clock.borrow_mut().set(late + 8000);
                state.borrow_mut().gpu = 0;
                third_done = true;
            }
            if third_done && !third_drained && state.borrow().pending {
                clock.borrow_mut().set(late + 9000);
                drain(&mut state.borrow_mut());
                third_drained = true;
            }
        }
        assert!(halted);
        assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
        assert_eq!(
            snapshots[&6].0,
            [
                1,
                1,
                1,
                2000,
                observed_valid,
                expected_observed,
                0,
                (late / 1000 - 1) as u16
            ]
        );
        assert_eq!(
            snapshots[&9].0,
            [
                1,
                2,
                observed_valid,
                prep2,
                1,
                3000,
                1,
                (late / 1000) as u16
            ]
        );
        assert_eq!(snapshots[&10].0, [1, 3, 1, 3000, 1, 3000, 1, 7]);
        // Frame2 prepared before Frame1 cleared the timer; its own execution
        // started afterwards. Each field retained its correct sequence/epoch.
        assert_eq!(snapshots[&9].1[12 + 2], 1);
        assert_eq!(snapshots[&9].1[12 + 8], 2);
        let expected_text = if late == 20_000 {
            b"OBS 17.0"
        } else {
            b"OBS --.-"
        };
        let page = &state.borrow().ui_snapshots[2];
        let actual: Vec<u8> = page[..4].iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(&actual, expected_text);
        assert_eq!(clock.borrow().clears, 4);
        assert_eq!(state.borrow().maintenance, 3);
    }
}

#[derive(Default)]
struct InputWire {
    changed: bool,
    buttons: u16,
    left_x: u16,
    reads: usize,
}
struct FrameInputPeer(Rc<RefCell<InputWire>>);
impl cpu_v3::Device for FrameInputPeer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.0.borrow_mut();
        s.reads += 1;
        match channel {
            8 => 1 | (u16::from(s.changed) * 2),
            0 => s.left_x,
            1 | 6 | 7 => 128,
            2 => 115,
            3 => 90,
            4 => (s.buttons ^ 65535) & 255,
            5 => (s.buttons ^ 65535) >> 8,
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, _: u16) {
        assert_eq!(channel, 8);
        self.0.borrow_mut().changed = false;
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[test]
fn frame_input_and_time_are_snapshots_and_held_motion_integrates_without_a_new_packet() {
    let body = r#"{let mut r=renderer_open();let mut movement=fix32::zero();
{let mut f=renderer_frame(&mut r,0x1111);let initial=frame_input(&mut f);
check(initial.connected==1 && initial.buttons==0 && initial.left_x==fix32::zero());
check(frame_elapsed_ms(&mut f)==0);let tick=frame_tick_ms(&mut f);signal(1,1);
let again=frame_input(&mut f);check(again.buttons==0 && again.left_x==fix32::zero());
check(frame_tick_ms(&mut f)==tick && frame_elapsed_ms(&mut f)==0);signal(1,2);}
{let mut f=renderer_frame(&mut r,0x1111);let fresh=frame_input(&mut f);
check(fresh.buttons==1 && fresh.pressed==1 && fresh.left_x>fix32::from_words(65000,0));
check(frame_elapsed_ms(&mut f)==16);movement+=fresh.left_x*frame_elapsed_seconds(&mut f);signal(1,3);}
{let mut f=renderer_frame(&mut r,0x1111);let held=frame_input(&mut f);
check(held.buttons==1 && held.pressed==0 && held.left_x>fix32::from_words(65000,0));
check(frame_elapsed_ms(&mut f)==16);movement+=held.left_x*frame_elapsed_seconds(&mut f);
check(movement>fix32::from_words(2080,0) && movement<fix32::from_words(2110,0));signal(1,4);}
renderer_close(r);}"#;
    let program = compile("", body);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    let state = attach(&mut cpu, Some(vec![]));
    let clock = Rc::new(RefCell::new(Clock::default()));
    cpu.attach_device(
        0,
        Box::new(ClockPeer {
            clock: clock.clone(),
            inner: Peer {
                kind: 0,
                state: state.clone(),
            },
        }),
    );
    let input = Rc::new(RefCell::new(InputWire {
        changed: true,
        left_x: 128,
        ..Default::default()
    }));
    cpu.attach_device(5, Box::new(FrameInputPeer(input.clone())));
    let mut marker_reads = Vec::new();
    let mut halted = false;
    for _ in 0..2_000_000 {
        match cpu.step().unwrap() {
            cpu_v3::StepOutcome::Signaled(e) => {
                assert_eq!(e.signal_type, 1);
                marker_reads.push(input.borrow().reads);
                match e.value {
                    1 => {
                        clock.borrow_mut().set(16_000);
                        let mut s = input.borrow_mut();
                        s.buttons = 1;
                        s.left_x = 255;
                        s.changed = true;
                    }
                    2 => assert_eq!(
                        marker_reads[0], marker_reads[1],
                        "Frame query read live hardware"
                    ),
                    3 => clock.borrow_mut().set(32_000),
                    4 => {}
                    _ => panic!("marker"),
                }
            }
            cpu_v3::StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted);
    assert_eq!(marker_reads.len(), 4);
    assert_eq!(
        clock.borrow().clears,
        1,
        "cancelled Frames cleared a shared timer"
    );
    assert!(state.borrow().starts.is_empty());
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
}
