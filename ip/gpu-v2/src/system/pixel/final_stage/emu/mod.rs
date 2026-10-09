//! Independent old-state register model of the final-color pipeline.
//!
//! The nine stages are the fixed implementation calendar certified by
//! [`super::sim::timed::StageCalendar`]:
//!
//! ```text
//! S1 (tint*texture, specular*h, g)  -> S2 t = m1 + 128
//! S3 b = t + (t >> 8)               -> S4 bg = (b >> 8) * g
//! S5 sum = bg + m2 (and bit)        -> S6 r1 = sum + 127
//! S7 r2 = r1 + bit                  -> S8 rounded = r2 >> 8, over = rounded > 255
//! S9 color = over ? 255 : rounded[7:0]
//! ```
//!
//! Edge convention (matches [`super::sim::credit`] and the emitted RTL):
//!
//! - All registers, both FIFO pointers and the credit counter advance on an
//!   enabled edge (`ce=1`); `ce=0` freezes every one of them.
//! - Pre-edge combinational handshake: `in_ready = ce && credits < CAPACITY`
//!   and a pixel is accepted iff `in_ready && in_valid`. `out_valid` is the
//!   non-empty FIFO. A result transfers iff `ce && out_ready && out_valid`.
//! - Acceptance and publication are computed from the old state, so a credit
//!   returned by a transfer on an edge can never fund an acceptance on that
//!   same edge; it funds the next edge.
//! - A result becomes visible at the FIFO head on the enabled edge after the
//!   publishing stage advances, so with a continuously ready consumer the
//!   first result transfers on the tenth enabled edge after acceptance.
//!
//! `out_ready=0` (backpressure) holds the head result stable; acceptance then
//! stalls once all four credits are reserved. `initiation_interval` here is the
//! arithmetic lane capacity (one pixel can be accepted per enabled edge); the
//! finite credit-aware completion is `sim::credit::CreditCalendar`, not this
//! number. No counted or oracle call occurs on `tick`.

use super::{
    Input, Output, Snapshot, Step, Tick, PIPELINE_LATENCY, PIPELINE_STAGES, RESULT_CAPACITY,
};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
struct S1 {
    key: u8,
    m1: [u32; 3],
    m2: [u32; 3],
    g: u16,
}
#[derive(Clone, Copy)]
struct S2 {
    key: u8,
    t: [u32; 3],
    m2: [u32; 3],
    g: u16,
}
#[derive(Clone, Copy)]
struct S3 {
    key: u8,
    b: [u32; 3],
    m2: [u32; 3],
    g: u16,
}
#[derive(Clone, Copy)]
struct S4 {
    key: u8,
    bg: [u32; 3],
    m2: [u32; 3],
}
#[derive(Clone, Copy)]
struct S5 {
    key: u8,
    sum: [u32; 3],
    bit: [u32; 3],
}
#[derive(Clone, Copy)]
struct S6 {
    key: u8,
    r1: [u32; 3],
    bit: [u32; 3],
}
#[derive(Clone, Copy)]
struct S7 {
    key: u8,
    r2: [u32; 3],
}
#[derive(Clone, Copy)]
struct S8 {
    key: u8,
    rounded: [u32; 3],
    over: [bool; 3],
}
#[derive(Clone, Copy)]
struct S9 {
    key: u8,
    color: [u8; 3],
}
#[derive(Clone, Copy)]
enum Stage {
    S1(S1),
    S2(S2),
    S3(S3),
    S4(S4),
    S5(S5),
    S6(S6),
    S7(S7),
    S8(S8),
    S9(S9),
}
impl Stage {
    fn key(self) -> u8 {
        match self {
            Stage::S1(s) => s.key,
            Stage::S2(s) => s.key,
            Stage::S3(s) => s.key,
            Stage::S4(s) => s.key,
            Stage::S5(s) => s.key,
            Stage::S6(s) => s.key,
            Stage::S7(s) => s.key,
            Stage::S8(s) => s.key,
            Stage::S9(s) => s.key,
        }
    }
}

fn accept(input: &Input) -> S1 {
    let m1 = std::array::from_fn(|c| u32::from(input.tint[c]) * u32::from(input.texture[c]));
    let m2 = std::array::from_fn(|c| u32::from(input.specular[c]) * u32::from(input.h));
    S1 {
        key: input.key,
        m1,
        m2,
        g: input.g,
    }
}

fn step(stage: Stage) -> Stage {
    match stage {
        Stage::S1(s) => Stage::S2(S2 {
            key: s.key,
            t: std::array::from_fn(|c| s.m1[c] + 128),
            m2: s.m2,
            g: s.g,
        }),
        Stage::S2(s) => Stage::S3(S3 {
            key: s.key,
            b: std::array::from_fn(|c| s.t[c] + (s.t[c] >> 8)),
            m2: s.m2,
            g: s.g,
        }),
        Stage::S3(s) => Stage::S4(S4 {
            key: s.key,
            bg: std::array::from_fn(|c| ((s.b[c] >> 8) * u32::from(s.g)) & 0x1_ffff),
            m2: s.m2,
        }),
        Stage::S4(s) => Stage::S5(S5 {
            key: s.key,
            sum: std::array::from_fn(|c| s.bg[c] + s.m2[c]),
            bit: std::array::from_fn(|c| ((s.bg[c] + s.m2[c]) >> 8) & 1),
        }),
        Stage::S5(s) => Stage::S6(S6 {
            key: s.key,
            r1: std::array::from_fn(|c| s.sum[c] + 127),
            bit: s.bit,
        }),
        Stage::S6(s) => Stage::S7(S7 {
            key: s.key,
            r2: std::array::from_fn(|c| s.r1[c] + s.bit[c]),
        }),
        Stage::S7(s) => Stage::S8(S8 {
            key: s.key,
            rounded: std::array::from_fn(|c| s.r2[c] >> 8),
            over: std::array::from_fn(|c| (s.r2[c] >> 8) > 255),
        }),
        Stage::S8(s) => Stage::S9(S9 {
            key: s.key,
            color: std::array::from_fn(|c| {
                if s.over[c] {
                    255
                } else {
                    (s.rounded[c] & 0xff) as u8
                }
            }),
        }),
        Stage::S9(s) => Stage::S9(s),
    }
}

pub struct FinalEmu {
    stages: [Option<Stage>; PIPELINE_STAGES],
    fifo: VecDeque<Output>,
    credits: usize,
    wall: u64,
    enabled: u64,
    max_wall: u64,
    faulted: bool,
}

impl FinalEmu {
    pub fn new(max_wall: u64) -> Result<Self, String> {
        if max_wall == 0 || max_wall > 4_000_000 {
            return Err("final watchdog bound".into());
        }
        Ok(Self {
            stages: [None; PIPELINE_STAGES],
            fifo: VecDeque::new(),
            credits: 0,
            wall: 0,
            enabled: 0,
            max_wall,
            faulted: false,
        })
    }
    pub fn latency(&self) -> usize {
        PIPELINE_LATENCY
    }
    pub fn initiation_interval(&self) -> usize {
        1
    }
    pub fn in_flight(&self) -> usize {
        self.stages.iter().filter(|s| s.is_some()).count()
    }
    pub fn queued(&self) -> usize {
        self.fifo.len()
    }
    pub fn head(&self) -> Option<Output> {
        self.fifo.front().copied()
    }
    pub fn credits(&self) -> usize {
        self.credits
    }
    pub fn idle(&self) -> bool {
        self.in_flight() == 0 && self.fifo.is_empty() && self.credits == 0
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            wall: self.wall,
            enabled: self.enabled,
            pipeline_keys: std::array::from_fn(|i| self.stages[i].map(Stage::key)),
            credits: self.credits,
            queued: self.fifo.len(),
        }
    }
    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        if self.faulted {
            return Err("final terminal fault; recreate before reuse".into());
        }
        if self.wall >= self.max_wall {
            self.faulted = true;
            return Err("final wall watchdog".into());
        }
        let result = self.advance(tick);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }

    fn advance(&mut self, tick: Tick) -> Result<Step, String> {
        self.wall += 1;
        let output = self.fifo.front().copied();
        let input_ready = tick.ce && self.credits < RESULT_CAPACITY;
        let accepted = input_ready && tick.input.is_some();
        let consumed = tick.ce && tick.output_ready && !self.fifo.is_empty();
        let accepted_input = tick.input.filter(|_| accepted);
        // Validate only the accepted request; a held-offered input is ignored.
        if let Some(input) = accepted_input {
            input.validate()?;
        }
        // Compute the next pipeline from old state.
        let mut next: [Option<Stage>; PIPELINE_STAGES] = [None; PIPELINE_STAGES];
        for i in 0..PIPELINE_STAGES - 1 {
            next[i + 1] = self.stages[i].map(step);
        }
        if let Some(input) = accepted_input {
            next[0] = Some(Stage::S1(accept(&input)));
        }
        let push = match self.stages[PIPELINE_STAGES - 1] {
            Some(Stage::S9(s)) => Some(Output::new(s.key, s.color)),
            _ => None,
        };
        if tick.ce {
            self.enabled += 1;
            if consumed {
                self.fifo.pop_front();
                self.credits = self
                    .credits
                    .checked_sub(1)
                    .ok_or("final credit underflow")?;
            }
            if let Some(result) = push {
                if self.fifo.len() >= RESULT_CAPACITY {
                    return Err("final reserved result overflow".into());
                }
                self.fifo.push_back(result);
            }
            if accepted {
                self.credits += 1;
            }
            self.stages = next;
        }
        let snapshot = self.snapshot();
        if self.credits > RESULT_CAPACITY || self.credits != self.fifo.len() + self.in_flight() {
            return Err("final credit ownership".into());
        }
        Ok(Step {
            input_ready,
            accepted,
            consumed,
            output,
            snapshot,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(seed: u32) -> Input {
        let mut x = seed.wrapping_mul(2654435761).wrapping_add(1013904223);
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        Input {
            key: (next() & 0x3f) as u8,
            tint: std::array::from_fn(|_| (next() & 0xff) as u8),
            texture: std::array::from_fn(|_| (next() & 0xff) as u8),
            g: (next() % 512) as u16,
            h: (next() % 257) as u16,
            specular: std::array::from_fn(|_| (next() & 0xff) as u8),
        }
    }

    #[test]
    fn stages_follow_the_fixed_stage_calendar() {
        let input = Input {
            key: 5,
            tint: [200, 0, 0],
            texture: [200, 0, 0],
            g: 2,
            h: 256,
            specular: [100, 0, 0],
        };
        let s1 = accept(&input);
        assert_eq!(s1.m1, [40_000, 0, 0], "stage 1 tint*texture");
        assert_eq!(s1.m2, [25_600, 0, 0], "stage 1 specular*h");
        assert_eq!(s1.g, 2);
        let Stage::S2(s2) = step(Stage::S1(s1)) else {
            panic!("stage 2")
        };
        assert_eq!(s2.t, [40_128, 128, 128], "stage 2 +128");
        let Stage::S3(s3) = step(Stage::S2(s2)) else {
            panic!("stage 3")
        };
        assert_eq!(s3.b, [40_284, 128, 128], "stage 3 +(t >> 8)");
        let Stage::S4(s4) = step(Stage::S3(s3)) else {
            panic!("stage 4")
        };
        assert_eq!(s4.bg, [314, 0, 0], "stage 4 base*g");
        let Stage::S5(s5) = step(Stage::S4(s4)) else {
            panic!("stage 5")
        };
        assert_eq!(s5.sum, [25_914, 0, 0], "stage 5 sum");
        assert_eq!(s5.bit, [1, 0, 0]);
        let Stage::S6(s6) = step(Stage::S5(s5)) else {
            panic!("stage 6")
        };
        assert_eq!(s6.r1, [26_041, 127, 127], "stage 6 +127");
        let Stage::S7(s7) = step(Stage::S6(s6)) else {
            panic!("stage 7")
        };
        assert_eq!(s7.r2, [26_042, 127, 127], "stage 7 +bit");
        let Stage::S8(s8) = step(Stage::S7(s7)) else {
            panic!("stage 8")
        };
        assert_eq!(s8.rounded, [101, 0, 0], "stage 8 r2 >> 8");
        assert_eq!(s8.over, [false, false, false], "stage 8 compare");
        let Stage::S9(s9) = step(Stage::S8(s8)) else {
            panic!("stage 9")
        };
        assert_eq!(s9.color, [101, 0, 0], "stage 9 select");
    }

    #[test]
    fn latency_is_exactly_nine_enabled_edges() {
        let mut emu = FinalEmu::new(1000).unwrap();
        let pixel = pixel(7);
        let step = emu
            .tick(Tick {
                ce: true,
                input: Some(pixel),
                output_ready: true,
            })
            .unwrap();
        assert!(step.accepted);
        let mut cycles = 0;
        while emu.queued() == 0 {
            emu.tick(Tick {
                ce: true,
                input: None,
                output_ready: false,
            })
            .unwrap();
            cycles += 1;
            assert!(cycles < 40, "pipeline watchdog");
        }
        assert_eq!(cycles, PIPELINE_LATENCY);
        let step = emu
            .tick(Tick {
                ce: true,
                input: None,
                output_ready: true,
            })
            .unwrap();
        assert_eq!(
            step.output,
            Some(Output::new(pixel.key, pixel.reference_rgb()))
        );
        assert!(step.consumed);
        assert!(emu.idle());
    }
}
