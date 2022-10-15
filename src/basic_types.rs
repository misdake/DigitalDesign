// pub type WireSingleValue = bool;
//
// pub enum Assert<const CHECK: bool> {}
//
// pub trait IsTrue {}
//
// impl IsTrue for Assert<true> {}
//
// pub struct Wire<const W: usize> where Assert<{ W - 1 > 0 }>: IsTrue {
//     values: [bool; W - 1],
// }
//
// pub struct Wire<const W: usize> {
//     values: [bool; W],
// }

#[derive(Copy, Clone)]
pub struct WireRef(usize);

static mut WIRES: Vec<bool> = Vec::new();
static mut GATES: Vec<Gate> = Vec::new();

pub fn input() -> WireRef {
    unsafe {
        let index = WIRES.len();
        WIRES.push(false);
        WireRef(index)
    }
}

pub fn nand(a: WireRef, b: WireRef) -> WireRef {
    unsafe {
        let out = input();
        GATES.push(Gate {
            wire_a: a,
            wire_b: b,
            wire_out: out,
        });
        out
    }
}

impl WireRef {
    pub fn get(self) -> bool {
        unsafe {
            WIRES[self.0]
        }
    }
    pub fn set(self, value: bool) {
        unsafe {
            WIRES[self.0] = value;
        }
    }
}

pub struct Gate {
    pub wire_a: WireRef,
    pub wire_b: WireRef,
    pub wire_out: WireRef,
}

impl Gate {
    pub fn execute(&self) {
        let a = self.wire_a.get();
        let b = self.wire_b.get();
        self.wire_out.set(!(a & b));
    }
}

pub fn execute_all_gates() {
    unsafe {
        for gate in &GATES {
            gate.execute();
        }
    }
}
