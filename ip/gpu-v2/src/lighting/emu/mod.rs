//! Cycle-stepped numerical pipeline. No oracle/counting call occurs on tick.
use super::{datapath::Program, ports::*, LightingProfile};
use std::collections::VecDeque;

struct Token {
    request: LightingRequest,
    context: LightingContext,
    age: usize,
    values: Vec<Option<i128>>,
    pending: Vec<(usize, Vec<(usize, i128)>)>,
}

pub struct LightingEmu {
    programs: [Program; 2],
    context: Option<LightingContext>,
    tokens: VecDeque<Token>,
    phase: usize,
    wall_ticks: u64,
    max_wall_ticks: u64,
}
impl LightingEmu {
    pub fn new(max_wall_ticks: u64) -> Result<Self, String> {
        Self::with_profile(LightingProfile::Fast, max_wall_ticks)
    }
    pub fn with_profile(profile: LightingProfile, max_wall_ticks: u64) -> Result<Self, String> {
        Self::with_dedicated_dsp(profile, false, max_wall_ticks)
    }
    /// Selected resource alternative; g matches baseline, h uses scalar H normalization.
    pub fn with_resource_profile(
        profile: LightingProfile,
        max_wall_ticks: u64,
    ) -> Result<Self, String> {
        Self::with_kernel(
            profile,
            false,
            super::sim::counted::Config::resource_profile(profile),
            true,
            max_wall_ticks,
        )
    }
    pub fn with_system_profile(
        profile: LightingProfile,
        max_wall_ticks: u64,
    ) -> Result<Self, String> {
        if !profile.system() {
            return Err("system profile requires SystemFast/SystemCompact".into());
        }
        Self::with_kernel(
            profile,
            false,
            super::sim::counted::Config::system_profile(),
            true,
            max_wall_ticks,
        )
    }
    /// Bounded area experiment with one permanent DSP lane per operation.
    pub fn with_dedicated_dsp(
        profile: LightingProfile,
        dedicated: bool,
        max_wall_ticks: u64,
    ) -> Result<Self, String> {
        Self::with_scalar(profile, dedicated, false, max_wall_ticks)
    }
    /// Experimental scalar normalization; published internal stages differ.
    pub fn with_scalar(
        profile: LightingProfile,
        dedicated: bool,
        scalar: bool,
        max_wall_ticks: u64,
    ) -> Result<Self, String> {
        Self::with_kernel(
            profile,
            dedicated,
            if scalar {
                super::sim::counted::Config::scalar()
            } else {
                super::sim::counted::Config::architecture()
            },
            false,
            max_wall_ticks,
        )
    }
    pub fn with_kernel(
        profile: LightingProfile,
        dedicated: bool,
        kernel: super::sim::counted::Config,
        roles: bool,
        max_wall_ticks: u64,
    ) -> Result<Self, String> {
        if max_wall_ticks == 0 {
            return Err("zero clock budget".into());
        }
        Ok(Self {
            programs: [
                Program::with_kernel(profile, true, dedicated, kernel, roles)?,
                Program::with_kernel(profile, false, dedicated, kernel, roles)?,
            ],
            context: None,
            tokens: VecDeque::new(),
            phase: 0,
            wall_ticks: 0,
            max_wall_ticks,
        })
    }
    /// Accept-to-valid latency in advancing CE edges, excluding stalls.
    pub fn latency(&self) -> usize {
        self.active_program().latency
    }
    fn program_index(c: LightingContext) -> usize {
        usize::from(c.mode() != 3)
    }
    fn active_program(&self) -> &Program {
        &self.programs[self.context.map_or(0, Self::program_index)]
    }
    pub fn initiation_interval(&self) -> usize {
        self.active_program().ii
    }
    pub fn advancing_phase(&self) -> usize {
        self.phase
    }
    pub fn in_flight(&self) -> usize {
        self.tokens.len()
    }

    fn result(&self, t: &Token) -> LightingResult {
        let program = &self.programs[Self::program_index(t.context)];
        let value =
            |id: usize| t.values[program.output_values[id]].expect("committed result") as u16;
        let output = match t.context.mode() {
            0 => LightingOutput { g: 256, h: 0 },
            1 => LightingOutput {
                g: t.context.light.ambient,
                h: 0,
            },
            2 => LightingOutput { g: value(0), h: 0 },
            _ => LightingOutput {
                g: value(0),
                h: value(1),
            },
        };
        LightingResult {
            output,
            id: t.request.id,
            epoch: t.context.epoch,
        }
    }
    pub fn signals(&self, tick: LightingTick) -> LightingSignals {
        let output = self
            .tokens
            .front()
            .filter(|t| t.age == self.programs[Self::program_index(t.context)].latency)
            .map(|t| self.result(t));
        let advancing = !tick.reset && tick.ce && !(output.is_some() && !tick.output_ready);
        let context_ready = !tick.reset && tick.ce && self.tokens.is_empty();
        LightingSignals {
            context_ready,
            input_ready: advancing
                && self.context.is_some()
                && self.phase == 0
                && tick.context.is_none(),
            output,
        }
    }
    /// Diagnostic golden boundaries of the currently held output token.
    pub fn output_stages(&self) -> Option<Vec<(String, i128)>> {
        let t = self
            .tokens
            .front()
            .filter(|t| t.age == self.programs[Self::program_index(t.context)].latency)?;
        Some(
            self.programs[Self::program_index(t.context)]
                .frame
                .outputs
                .iter()
                .map(|o| (o.name.clone(), t.values[o.value].unwrap()))
                .collect(),
        )
    }
    /// Boundaries produced at the current advancing age, for independent HDL
    /// stage comparison. Call once per edge; CE pauses repeat the same values.
    pub fn stage_values(&self) -> Vec<(u32, bool, String, i128)> {
        let mut stages = Vec::new();
        for token in &self.tokens {
            let program = &self.programs[Self::program_index(token.context)];
            for o in &program.frame.outputs {
                let producer = program.frame.values[o.value].producer;
                let ready = program
                    .instructions
                    .iter()
                    .find(|i| i.root == producer)
                    .unwrap()
                    .ready;
                if token.age == ready {
                    stages.push((
                        token.request.id,
                        program.full,
                        o.name.clone(),
                        token.values[o.value].unwrap(),
                    ));
                }
            }
        }
        stages
    }
    fn execute_age(program: &Program, t: &mut Token) -> Result<(), String> {
        let mut keep = Vec::new();
        for (ready, results) in t.pending.drain(..) {
            if ready == t.age {
                for (value, raw) in results {
                    t.values[value] = Some(raw);
                }
            } else {
                keep.push((ready, results));
            }
        }
        t.pending = keep;
        for &id in &program.order {
            let ins = &program.instructions[id];
            if ins.issue != t.age {
                continue;
            }
            let results = program.execute(ins, &t.values, t.request.pixel, t.context)?;
            if ins.ready == t.age {
                for (value, raw) in results {
                    t.values[value] = Some(raw);
                }
            } else {
                t.pending.push((ins.ready, results));
            }
        }
        Ok(())
    }
    /// Returns the pre-edge signals and then applies the edge atomically.
    /// Only accepted data is validated; invalid parked inputs do not fault.
    pub fn tick(&mut self, tick: LightingTick) -> Result<LightingSignals, String> {
        if self.wall_ticks >= self.max_wall_ticks {
            return Err("lighting clock budget exhausted".into());
        }
        self.wall_ticks += 1;
        let signals = self.signals(tick);
        if tick.reset {
            self.tokens.clear();
            self.context = None;
            self.phase = 0;
            return Ok(signals);
        }
        if !tick.ce {
            return Ok(signals);
        }
        if let Some(context) = tick.context.filter(|_| signals.context_ready) {
            context.validate().map_err(|e| format!("context: {e:?}"))?;
            self.context = Some(context);
            self.phase = 0;
            return Ok(signals);
        }
        if signals.output.is_some() && !tick.output_ready {
            return Ok(signals);
        }
        // Validate before changing any numerical state.
        let accepted = tick.input.filter(|_| signals.input_ready);
        if let Some(request) = accepted {
            let context = self.context.unwrap();
            validate(
                request.pixel,
                context.material,
                context.light,
                context.projection,
            )
            .map_err(|e| format!("pixel: {e:?}"))?;
        }
        if signals.output.is_some() {
            self.tokens.pop_front();
        }
        if self.context.is_none() {
            return Ok(signals);
        }
        for t in &mut self.tokens {
            t.age += 1;
            Self::execute_age(&self.programs[Self::program_index(t.context)], t)?;
        }
        if let Some(request) = accepted {
            let mut t = Token {
                request,
                context: self.context.unwrap(),
                age: 0,
                values: vec![None; self.active_program().frame.values.len()],
                pending: Vec::new(),
            };
            Self::execute_age(self.active_program(), &mut t)?;
            self.tokens.push_back(t);
        }
        self.phase = (self.phase + 1) % self.active_program().ii;
        Ok(signals)
    }
}
