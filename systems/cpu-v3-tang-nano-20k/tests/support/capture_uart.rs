//! Offline real RCC UART runtime with functional GPIO/time/TX peers.
//! Debounce is explicitly primed at the first Prepared gate; this is not board
//! timing or a physical button qualification. No GPU consumes the frozen input.
use cpu_v3::{rcc_backend::CpuV3Program, Device, StepOutcome};
use cpu_v3_tang_nano_20k::{
    profiling_capture::{CapturePoint, FrozenInputs},
    system_sim::{CpuV3SystemSim, VBlankMode},
};
#[allow(dead_code)]
#[path = "../../rcc/device_abi.rs"]
mod abi;
use abi::*;

pub fn reject_dependency(package: &[u8], program: &CpuV3Program, point: CapturePoint, out: &Path) {
    let mut sim =
        CpuV3SystemSim::from_single_flash(package, 0, VBlankMode::PauseOnFrameIndexWait).unwrap();
    let start = Instant::now();
    for step in 0..5_000_000usize {
        if step % 100_000 == 0 {
            assert!(start.elapsed() < Duration::from_secs(30));
        }
        assert!(!sim.gpu().busy(), "invalid dependency issued GPU SEND");
        assert_ne!(
            sim.cpu().pc(),
            point.gate_pc,
            "invalid dependency reached the consumer"
        );
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        if matches!(sim.step().unwrap(), StepOutcome::Halted { .. }) {
            let g = program
                .debug
                .globals
                .iter()
                .find(|g| g.name == "GPU_CAPTURE_CONTROL")
                .unwrap();
            let rcc::VarLoc::Global(address) = g.loc else {
                panic!("capture state");
            };
            assert_eq!(
                sim.cpu_mut().physical_memory_mut()[usize::from(address) + 1],
                9
            );
            std::fs::write(
                out.join("dependency-rejection.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "capture_error":9,"gpu_consumed":false,"gate_reached":false,"steps":step,
                    "maximum_steps":5000000,"maximum_wall_seconds":30,"physical_board":false
                }))
                .unwrap(),
            )
            .unwrap();
            return;
        }
    }
    panic!("dependency rejection step bound; incomplete");
}
use std::{
    any::Any,
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Control {
    buttons: u16,
    now: u16,
    led: u16,
    pauses: usize,
    step: usize,
    pause_step: usize,
    uart: Vec<u8>,
}
struct Peer(Rc<RefCell<Control>>);
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let s = self.0.borrow();
        match u16::from(channel) {
            SYSCTL_BUTTONS => s.buttons,
            SYSCTL_MILLISECONDS => s.now,
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.0.borrow_mut();
        match u16::from(channel) {
            SYSCTL_UART_TX_DATA => s.uart.push(value as u8),
            SYSCTL_LED => {
                s.led = value;
                if value == 2 {
                    s.pauses += 1;
                    s.pause_step = s.step;
                }
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn exercise(
    package: &[u8],
    program: &CpuV3Program,
    point: CapturePoint,
    frozen: &FrozenInputs,
    out: &Path,
) {
    let mut sim =
        CpuV3SystemSim::from_single_flash(package, 0, VBlankMode::PauseOnFrameIndexWait).unwrap();
    let control = Rc::new(RefCell::new(Control::default()));
    sim.cpu_mut()
        .attach_device(0, Box::new(Peer(control.clone())));
    let start = Instant::now();
    let mut reached = false;
    let mut resumed = false;
    let mut steps = 0;
    for step in 0..2_000_000_000usize {
        steps = step;
        if step % 100_000 == 0 {
            assert!(
                start.elapsed() < Duration::from_secs(180),
                "UART wall bound; incomplete"
            );
        }
        if !reached && sim.cpu().pc() == point.gate_pc {
            reached = true;
            assert_eq!(
                &sim.cpu_mut().physical_memory_mut()[point.header_word..point.header_word + 64],
                frozen.header
            );
            let global = program
                .debug
                .globals
                .iter()
                .find(|g| g.name == "PROFILER_BUTTONS")
                .unwrap();
            let rcc::VarLoc::Global(address) = global.loc else {
                panic!("button state");
            };
            sim.cpu_mut().physical_memory_mut()[usize::from(address)..usize::from(address) + 4]
                .copy_from_slice(&[1, 2, 0, 0]);
            control.borrow_mut().buttons = 2;
        }
        {
            let mut s = control.borrow_mut();
            s.step = step;
            s.now = (step / 100) as u16;
            if s.pauses != 0 {
                let elapsed = step - s.pause_step;
                if elapsed >= 5000 {
                    s.buttons = 0;
                }
                if elapsed >= 10000 {
                    s.buttons = if s.pauses < 3 { 1 } else { 2 };
                }
            }
        }
        if sim.gpu().busy() {
            assert!(reached);
            assert_eq!(
                control.borrow().pauses,
                3,
                "two complete transfers must retain the same lease"
            );
            resumed = true;
            break;
        }
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        assert!(
            !matches!(sim.step().unwrap(), StepOutcome::Halted { .. }),
            "unexpected halt"
        );
    }
    assert!(
        reached && resumed,
        "UART step bound; incomplete: reached={reached} pc={} led={} pauses={} bytes={}",
        sim.cpu().pc(),
        control.borrow().led,
        control.borrow().pauses,
        control.borrow().uart.len()
    );
    let (rows, tail) = frozen.ranges.as_chunks::<8>();
    assert!(tail.is_empty());
    for (row, expected) in rows.iter().zip(&frozen.payloads) {
        let base = (usize::from(row[2]) | (usize::from(row[3]) << 16)) / 2;
        let actual: Vec<u8> = sim.cpu_mut().physical_memory_mut()[base..base + expected.len() / 2]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        assert_eq!(&actual, expected, "dump/resume changed a frozen resource");
    }
    std::fs::write(out.join("uart-offline.bin"), &control.borrow().uart).unwrap();
    std::fs::write(
        out.join("uart-peer.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "source":"actual RCC UART runtime; controlled functional peer", "physical_board":false,
            "debounce":"primed settled pause edge at first Prepared gate", "complete_dumps":2,
            "resume":"same prepared CB first GPU SEND only after explicit Button 2", "steps":steps,
            "maximum_steps":2000000000,"maximum_wall_seconds":180
        }))
        .unwrap(),
    )
    .unwrap();
}
