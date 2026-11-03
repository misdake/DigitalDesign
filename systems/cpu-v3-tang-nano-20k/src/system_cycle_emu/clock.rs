//! Exact integer calendar; visit edges, never all quanta or rounded frame periods.
use digital_design_hardware_gowin::sdram_memory_controller::profile::{CONTROLLER_HZ, LOGIC_HZ};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Instant {
    pub ticks: u64,
    pub logic_rise: bool,
    pub pixel_rise: bool,
    pub native_half: bool,
    pub native_phase: u8,
}
#[derive(Clone, Debug)]
pub struct Clock {
    pub quantum_hz: u64,
    periods: [u64; 3],
    next: [u64; 3],
    native_phase: u8,
    pub logic_edges: u64,
    pub pixel_edges: u64,
    pub native_halves: u64,
}
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
fn lcm(a: u64, b: u64) -> u64 {
    a / gcd(a, b) * b
}
impl Default for Clock {
    fn default() -> Self {
        Self::new(crate::display::ACTIVE_DISPLAY_CONFIG.pixel_clock_hz, 0)
    }
}
impl Clock {
    /// `pixel_phase` is an exact offset in this calendar's integer quantum.
    pub fn new(pixel_hz: u64, pixel_phase: u64) -> Self {
        assert!(pixel_hz > 0);
        let rates = [u64::from(LOGIC_HZ), u64::from(CONTROLLER_HZ) * 2, pixel_hz];
        let quantum_hz = rates.into_iter().fold(1, lcm);
        let periods = rates.map(|r| quantum_hz / r);
        assert!(pixel_phase < periods[2]);
        Self {
            quantum_hz,
            periods,
            next: [0, 0, pixel_phase],
            native_phase: 0,
            logic_edges: 0,
            pixel_edges: 0,
            native_halves: 0,
        }
    }
    pub fn peek(&self) -> Instant {
        let ticks = *self.next.iter().min().unwrap();
        Instant {
            ticks,
            logic_rise: self.next[0] == ticks,
            native_half: self.next[1] == ticks,
            pixel_rise: self.next[2] == ticks,
            native_phase: self.native_phase,
        }
    }
    pub fn advance(&mut self) -> Instant {
        let e = self.peek();
        for (n, take) in [e.logic_rise, e.native_half, e.pixel_rise]
            .into_iter()
            .enumerate()
        {
            if take {
                self.next[n] += self.periods[n];
            }
        }
        self.logic_edges += u64::from(e.logic_rise);
        self.pixel_edges += u64::from(e.pixel_rise);
        if e.native_half {
            self.native_halves += 1;
            self.native_phase = (self.native_phase + 1) & 3;
        }
        assert!(
            !e.logic_rise || e.native_half && e.native_phase == 0,
            "logic/native phase disagreement"
        );
        e
    }
}
