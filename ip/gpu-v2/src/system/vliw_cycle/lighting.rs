//! Selected live-global II4 Lighting and its simultaneous tuple adapter.
//! Arithmetic executes the selected schedule one instruction at its issue age;
//! only the instruction's own result waits for its declared resource latency.
use super::{postquad::LightResult, vertex::Bits};
use crate::lighting::{datapath::Program, dsp_sharing, ports::*, LightingProfile};
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Globals {
    pub epoch: u16,
    pub mode: u8,
    pub code: u8,
    pub light: [i16; 3],
    pub ambient: u16,
    pub directional: u16,
    pub ray: [i16; 3],
}
impl Globals {
    fn context(self) -> LightingContext {
        LightingContext {
            material: Material {
                unlit: false,
                specular_color: [255; 3],
                shininess_code: self.code & 31,
            },
            light: Light {
                direction: self.light,
                ambient: self.ambient & 511,
                directional: self.directional & 511,
            },
            projection: Projection {
                ray_scale: [self.ray[0], self.ray[1]],
                k: self.ray[2],
            },
            epoch: self.epoch,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub compute_tick: bool,
    pub source_valid: bool,
    pub source_slot: u8,
    pub source_lane: u8,
    pub source_mask: u8,
    pub normal: Bits,
    pub position: Bits,
    pub globals: Globals,
    pub out_ready: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub source_ready: bool,
    pub pixel_accepted: bool,
    pub in_ready: bool,
    pub result: LightResult,
    pub epoch: u16,
    pub phase: u8,
    pub valid_pipe: u64,
    pub id: Bits,
}
#[derive(Clone, Debug)]
struct Token {
    pixel: PixelInput,
    age: usize,
    values: Vec<Option<i128>>,
    pending: Vec<(usize, Vec<(usize, i128)>)>,
}
#[derive(Clone)]
pub struct Lighting {
    program: Arc<Program>,
    tokens: VecDeque<Token>,
    phase: u8,
    valid_pipe: u64,
    ids: [Bits; 10],
}
impl std::fmt::Debug for Lighting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lighting")
            .field("phase", &self.phase)
            .field("valid_pipe", &self.valid_pipe)
            .field("tokens", &self.tokens)
            .finish_non_exhaustive()
    }
}
impl Default for Lighting {
    fn default() -> Self {
        Self::new().expect("selected II4 Lighting calendar")
    }
}
impl Lighting {
    pub fn new() -> Result<Self, String> {
        static PROGRAM: std::sync::OnceLock<Arc<Program>> = std::sync::OnceLock::new();
        let program = if let Some(p) = PROGRAM.get() {
            p.clone()
        } else {
            let (o, plans) = dsp_sharing::four_cycle_shared_roms(9)?;
            let mut p = Program::with_retiming(
                LightingProfile::Fast,
                true,
                o.dedicated_dsp,
                o.kernel(),
                o.role_schedule,
                o.logic_depth,
                o.retiming,
            )?;
            p.apply_plan(&plans[0])?;
            if (p.ii, p.latency) != (4, 34) {
                return Err("selected Lighting calendar changed".into());
            }
            let p = Arc::new(p);
            let _ = PROGRAM.set(p.clone());
            p
        };
        Ok(Self {
            program,
            tokens: VecDeque::new(),
            phase: 0,
            valid_pipe: 0,
            ids: [Bits::default(); 10],
        })
    }
    pub fn idle(&self) -> bool {
        self.valid_pipe == 0
    }
    pub fn in_flight(&self) -> usize {
        self.tokens.len()
    }
    pub fn outputs(&self, i: Input) -> Output {
        let covered = i.source_mask & (1 << (i.source_lane & 3)) != 0;
        let ready = i.compute_tick && !i.reset && self.phase == 0;
        let token = self.tokens.front().filter(|t| t.age == 34);
        let scalar = |n: usize| {
            token
                .and_then(|t| t.values[self.program.output_values[n]])
                .map_or(Bits::default(), |v| Bits::new(v as u64, 9))
        };
        let (g, h) = match i.globals.mode & 3 {
            1 => (Bits::new(u64::from(i.globals.ambient), 9), Bits::new(0, 9)),
            2 => (scalar(0), Bits::new(0, 9)),
            _ => (scalar(0), scalar(1)),
        };
        let id = self.ids[9];
        Output {
            source_ready: i.ce && !i.reset && i.source_valid && (!covered || ready),
            pixel_accepted: i.ce && i.source_valid && covered && ready,
            in_ready: ready,
            result: LightResult {
                valid: self.valid_pipe & (1 << 34) != 0,
                key: id.value as u8 & 127,
                last: id.value & 128 != 0,
                g,
                h,
            },
            epoch: i.globals.epoch,
            phase: self.phase,
            valid_pipe: self.valid_pipe,
            id,
        }
    }
    fn execute_age(program: &Program, t: &mut Token, c: LightingContext) -> Result<(), String> {
        let mut keep = Vec::new();
        for (ready, values) in t.pending.drain(..) {
            if ready == t.age {
                for (v, raw) in values {
                    t.values[v] = Some(raw);
                }
            } else {
                keep.push((ready, values));
            }
        }
        t.pending = keep;
        // These sources/cones are wires in the live-global factory, not a
        // per-pixel context snapshot or a context-install transaction.
        for &n in &program.order {
            let ins = &program.instructions[n];
            let output = program.frame.events[ins.root].output.unwrap();
            if program.stable[output] {
                for (v, raw) in program.execute(ins, &t.values, t.pixel, c)? {
                    t.values[v] = Some(raw);
                }
            }
        }
        for &n in &program.order {
            let ins = &program.instructions[n];
            let output = program.frame.events[ins.root].output.unwrap();
            if ins.issue != t.age || program.stable[output] {
                continue;
            }
            let results = program.execute(ins, &t.values, t.pixel, c)?;
            if ins.ready == t.age {
                for (v, raw) in results {
                    t.values[v] = Some(raw);
                }
            } else {
                t.pending.push((ins.ready, results));
            }
        }
        Ok(())
    }
    pub fn clock(&mut self, i: Input) -> Result<(), String> {
        if i.reset {
            self.tokens.clear();
            self.phase = 0;
            self.valid_pipe = 0;
            return Ok(());
        }
        if !i.compute_tick {
            return Ok(());
        }
        let accept =
            i.source_valid && i.source_mask & (1 << (i.source_lane & 3)) != 0 && self.phase == 0;
        let pixel = if accept {
            if !i.normal.is_known(36) || !i.position.is_known(32) {
                return Err("Lighting accepted an unknown source tuple".into());
            }
            Some(PixelInput {
                normal: std::array::from_fn(|c| {
                    ((i.normal.value >> (12 * c)) as u16).wrapping_shl(4) as i16
                }),
                ndc: [
                    i.position.value as u16 as i16 as i32,
                    (i.position.value >> 16) as u16 as i16 as i32,
                ],
            })
        } else {
            None
        };
        if self.tokens.front().is_some_and(|t| t.age == 34) {
            self.tokens.pop_front();
        }
        for t in &mut self.tokens {
            t.age += 1;
            Self::execute_age(&self.program, t, i.globals.context())?;
        }
        if let Some(pixel) = pixel {
            let mut t = Token {
                pixel,
                age: 0,
                values: vec![None; self.program.frame.values.len()],
                pending: Vec::new(),
            };
            Self::execute_age(&self.program, &mut t, i.globals.context())?;
            self.tokens.push_back(t);
        }
        let last = (i.source_mask >> ((i.source_lane & 3) + 1)) == 0;
        let id = u64::from((i.source_slot & 31) * 4 + (i.source_lane & 3)) | (u64::from(last) << 7);
        let old_ids = self.ids;
        self.ids[0] = Bits::new(id, 32);
        if self.phase == 1 {
            self.ids[1..].copy_from_slice(&old_ids[..9]);
        }
        self.valid_pipe = ((self.valid_pipe << 1) | u64::from(accept)) & ((1 << 35) - 1);
        self.phase = (self.phase + 1) & 3;
        Ok(())
    }
}
