//! External SDRAM load for GPU-only measurements.

const VIDEO_GROUP_PERIOD: usize = 6850;
const VIDEO_FRAME_PERIOD: usize = 899_027;
const VIDEO_GROUP_COUNT: usize = 120;
const BURSTS_PER_GROUP: usize = 50;
const GPU_SUBMIT_EDGE: usize = 16_385;

#[derive(Default, Clone, Copy)]
pub struct Drive {
    pub instruction: Option<u64>,
    pub data: Option<(u64, bool)>,
    pub display: Option<u64>,
}

#[derive(Default, Clone, Copy)]
pub struct Grant {
    pub instruction: bool,
    pub data: bool,
    pub display: bool,
    pub display_last: bool,
}

#[derive(Default)]
pub struct Traffic {
    display: bool,
    display_base: usize,
    phase: usize,
    group: usize,
    frame: usize,
    burst: usize,
    display_waiting: bool,
    instruction_period: usize,
    data_period: usize,
    next_instruction: usize,
    next_data: usize,
    pub grants: [usize; 3],
}

impl Traffic {
    pub fn new(profile: &str, color_byte_address: usize, phase: usize) -> Self {
        let (display, instruction_period, data_period) = match profile {
            "idle" => (false, 0, 0),
            "display" => (true, 0, 0),
            "display_cpu" => (true, 256, 96),
            "display_cpu_heavy" => (true, 96, 24),
            _ => panic!("unsupported contention profile"),
        };
        let front = match color_byte_address {
            0x400000 => 0x440000,
            0x440000 => 0x400000,
            _ if !display => 0,
            _ => panic!("expected a physical color slot"),
        };
        Self {
            display,
            display_base: front / 2,
            phase,
            instruction_period,
            data_period,
            next_instruction: GPU_SUBMIT_EDGE + 31,
            next_data: GPU_SUBMIT_EDGE + 53,
            ..Self::default()
        }
    }

    pub fn drive(&self, edge: usize) -> Drive {
        let instruction = (self.instruction_period != 0 && edge >= self.next_instruction)
            .then(|| (0x2000 + (self.grants[0] % 257) * 16) as u64);
        let data = (self.data_period != 0 && edge >= self.next_data).then(|| {
            (
                (0x14000 + (self.grants[1] % 1021) * 16) as u64,
                self.grants[1] % 2 == 1,
            )
        });
        let display = (self.display
            && !self.display_waiting
            && edge
                >= GPU_SUBMIT_EDGE
                    + self.phase
                    + self.frame * VIDEO_FRAME_PERIOD
                    + self.group * VIDEO_GROUP_PERIOD)
            .then(|| {
                let line = self.group * 2 + self.burst % 2;
                let column = self.burst / 2;
                (self.display_base + column * 4096 + line * 16) as u64
            });
        Drive {
            instruction,
            data,
            display,
        }
    }

    pub fn clock(&mut self, edge: usize, grant: Grant) {
        if grant.instruction {
            self.grants[0] += 1;
            self.next_instruction = edge + self.instruction_period;
        }
        if grant.data {
            self.grants[1] += 1;
            self.next_data = edge + self.data_period;
        }
        if grant.display {
            assert!(!self.display_waiting);
            self.display_waiting = true;
            self.grants[2] += 1;
        }
        if grant.display_last {
            assert!(self.display_waiting);
            self.display_waiting = false;
            self.burst += 1;
            if self.burst == BURSTS_PER_GROUP {
                self.burst = 0;
                self.group += 1;
                if self.group == VIDEO_GROUP_COUNT {
                    self.group = 0;
                    self.frame += 1;
                }
            }
        }
    }
}
