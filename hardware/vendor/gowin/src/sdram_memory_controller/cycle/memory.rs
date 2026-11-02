//! Gear/controller/pin composition; the system supplies its sole backing slice.
use super::{
    controller::Controller,
    gear::{Gear, Input, LogicOutput},
    pins::{self, Pins},
    Edges,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Pins(pins::Error),
    UnknownRead { native_cycle: u64, known: u32 },
}
#[derive(Clone, Debug)]
pub struct Memory {
    gear: Gear,
    controller: Controller,
    pins: Pins,
    phase: u8,
    native_cycle: u64,
}
impl Default for Memory {
    fn default() -> Self {
        Self::new(true, 21600)
    }
}
impl Memory {
    pub fn new(cold_start_only: bool, init_cycles: u16) -> Self {
        Self {
            gear: Gear::new(cold_start_only),
            controller: Controller::new(init_cycles),
            pins: Pins::default(),
            phase: 0,
            native_cycle: 0,
        }
    }
    pub fn output(&self, input: &Input) -> LogicOutput {
        self.gear.logic_output(input)
    }
    pub fn native_cycle(&self) -> u64 {
        self.native_cycle
    }
    pub fn clock_half(&mut self, input: &Input, backing: &mut [u16]) -> Result<(), Error> {
        let edges = Edges {
            logic_rise: self.phase == 0,
            native_rise: self.phase == 0 || self.phase == 2,
            native_fall: self.phase == 1 || self.phase == 3,
        };
        let drive = self.gear.controller_input(input);
        let controller = self.controller.output(&drive);
        if edges.native_rise && controller.feedback.read_valid && controller.read_known != u32::MAX
        {
            return Err(Error::UnknownRead {
                native_cycle: self.native_cycle,
                known: controller.read_known,
            });
        }
        self.gear.clock(edges, input, &controller.feedback);
        if edges.native_rise {
            self.controller
                .clock(&drive, false, self.pins.sample(&controller));
            self.native_cycle += 1;
        } else {
            self.pins
                .clock(input.reset, &controller, backing)
                .map_err(Error::Pins)?;
        }
        self.phase = (self.phase + 1) & 3;
        Ok(())
    }
    /// Callers sample output and clock their logic from that same old snapshot.
    /// Physical writes commit during these four events, not on request accept.
    pub fn clock_logic(&mut self, input: &Input, backing: &mut [u16]) -> Result<(), Error> {
        assert_eq!(self.phase, 0, "logic cycle must start at its rising edge");
        for _ in 0..4 {
            self.clock_half(input, backing)?;
        }
        Ok(())
    }
}
