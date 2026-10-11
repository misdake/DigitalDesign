//! Light2 row consumer for the frozen selected Lighting core.
//! The queue owns the descriptor and odd row until acceptance. Only the preceding
//! normal row is retained here; PixelRows are combinational wiring. Context
//! switches drain the core locally while results continue to leave.

use crate::lighting::{
    calendars::UnifiedCalendar, emu::LightingEmu, ports::*, LightingProfile, LightingQuantization,
};

/// Parent-held queue head. All eight rows of an entry share its descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    pub row: u8,
    pub data: u64,
    pub slot: u8,
    pub mask: u8,
    pub draw_bank: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub source: Option<Source>,
    /// Immutable bank selected by source.draw_bank; held while source is stalled.
    pub context: LightingContext,
    pub output_ready: bool,
    /// Actual draw retirement, after every source/result/global reference drains.
    pub retire_bank: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultRow {
    pub key: u8,
    pub quad_last: bool,
    pub epoch: u16,
    pub g: u16,
    pub h: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signals {
    pub source_ready: bool,
    pub context_wait: bool,
    pub context_accepted: bool,
    pub pixel_accepted: bool,
    pub output: Option<ResultRow>,
}

pub struct Consumer {
    core: LightingEmu,
    normal: u64,
    loaded_bank: Option<u8>,
}

impl Consumer {
    pub fn new(max_wall: u64) -> Result<Self, String> {
        Self::with_calendar(
            max_wall,
            UnifiedCalendar::selected(LightingQuantization::CompensatedFloor),
        )
    }
    /// Explicit reviewed program; default selection is preserved.
    pub fn with_calendar(max_wall: u64, calendar: UnifiedCalendar) -> Result<Self, String> {
        let q = LightingQuantization::CompensatedFloor;
        Ok(Self {
            core: LightingEmu::with_schedule_plans(
                LightingProfile::Fast,
                calendar.options(q),
                &calendar.plans(q)?,
                max_wall,
            )?,
            normal: 0,
            loaded_bank: None,
        })
    }

    pub fn in_flight(&self) -> usize {
        self.core.in_flight()
    }

    /// Return old-state signals, then advance exactly one physical clock.
    pub fn tick(&mut self, t: Tick) -> Result<Signals, String> {
        if t.retire_bank.is_some_and(|b| b > 1) {
            return Err("Lighting retirement bank outside two draw banks".into());
        }
        if let Some(s) = t.source {
            if s.row > 7
                || s.slot > 31
                || s.mask > 15
                || s.draw_bank > 1
                || s.data >> if s.row & 1 == 0 { 36 } else { 32 } != 0
            {
                return Err("Lighting source outside compact row contract".into());
            }
        }
        let covered = t.source.is_some_and(|s| s.mask & (1 << (s.row / 2)) != 0);
        let switching = t
            .source
            .is_some_and(|s| covered && self.loaded_bank != Some(s.draw_bank));
        let request = t
            .source
            .filter(|s| covered && s.row & 1 != 0 && !switching)
            .map(|s| {
                let lane = s.row / 2;
                let last = s.mask >> (lane + 1) == 0;
                Ok::<_, String>(LightingRequest {
                    pixel: CompactPixelInput::from_rows([self.normal, s.data])
                        .and_then(CompactPixelInput::expanded)
                        .map_err(|e| format!("Lighting compact row: {e:?}"))?,
                    id: u32::from(s.slot * 4 + lane) | (u32::from(last) << 7),
                })
            })
            .transpose()?;
        let ct = LightingTick {
            reset: t.reset,
            ce: t.ce,
            context: switching.then_some(t.context),
            input: request,
            output_ready: t.output_ready,
        };
        let core = self.core.tick(ct)?;
        let source_ready = t.ce
            && !t.reset
            && t.source
                .is_some_and(|s| s.row & 1 == 0 || !covered || (!switching && core.input_ready));
        let context_accepted = switching && core.context_ready;
        if t.reset {
            self.normal = 0;
            self.loaded_bank = None;
        } else if t.ce {
            if source_ready && covered && t.source.is_some_and(|s| s.row & 1 == 0) {
                self.normal = t.source.unwrap().data;
            }
            if t.retire_bank == self.loaded_bank && t.retire_bank.is_some() {
                self.loaded_bank = None;
            }
            if context_accepted {
                self.loaded_bank = t.source.map(|s| s.draw_bank);
            }
        }
        let output = core.output.map(|o| ResultRow {
            key: (o.id & 127) as u8,
            quad_last: o.id & 128 != 0,
            epoch: o.epoch,
            g: o.output.g,
            h: o.output.h,
        });
        Ok(Signals {
            source_ready,
            context_wait: switching,
            context_accepted,
            pixel_accepted: source_ready && covered && t.source.is_some_and(|s| s.row & 1 != 0),
            output,
        })
    }
}

/// Adapter only, instantiating the unmodified gpu_v2_lighting module.
pub fn adapter_verilog() -> &'static str {
    include_str!("lighting_consumer.v")
}

/// Actual selected core and its row consumer, ready for composition or replay.
pub fn verilog() -> Result<String, String> {
    verilog_with_calendar(UnifiedCalendar::selected(
        LightingQuantization::CompensatedFloor,
    ))
}
/// Factory for an explicit arithmetic program/calendar pair.
pub fn verilog_with_calendar(c: UnifiedCalendar) -> Result<String, String> {
    let q = LightingQuantization::CompensatedFloor;
    let core = crate::lighting::rtl::generate_with_schedule_plans(
        LightingProfile::Fast,
        c.options(q),
        &c.plans(q)?,
    )?;
    Ok(format!("{}\n{}", adapter_verilog(), core.source))
}
