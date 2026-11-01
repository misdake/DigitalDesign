//! Control-only peers shared by the builder and UI fixtures; never render.
use cpu_v3::{CpuV3Sim, Device};
use std::{any::Any, cell::RefCell, rc::Rc};

pub fn attach(cpu: &mut CpuV3Sim) -> Rc<RefCell<(usize, usize)>> {
    let state = Rc::new(RefCell::new((0, 0)));
    for kind in [0, 3, 4] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                state: state.clone(),
            }),
        );
    }
    state
}

struct Peer {
    kind: u8,
    state: Rc<RefCell<(usize, usize)>>,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        if self.kind == 3 && channel == 2 {
            0x20
        } else {
            0
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, _: u16) {
        if self.kind == 0 && channel == 1 {
            self.state.borrow_mut().0 += 1;
        }
        if self.kind == 4 && channel == 1 {
            self.state.borrow_mut().1 += 1;
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn status_publication_and_start_are_control_only() {
    let mut cpu = CpuV3Sim::default();
    let counts = attach(&mut cpu);
    let mut memory = [0xa55a; 16];
    for kind in [0, 3, 4] {
        let mut peer = Peer {
            kind,
            state: counts.clone(),
        };
        for channel in 0..16 {
            assert_eq!(
                peer.read(&mut memory, channel),
                if (kind, channel) == (3, 2) { 0x20 } else { 0 }
            );
            peer.write(&mut memory, channel, 0xffff);
        }
    }
    assert_eq!(*counts.borrow(), (1, 1));
    assert_eq!(memory, [0xa55a; 16]);
}
