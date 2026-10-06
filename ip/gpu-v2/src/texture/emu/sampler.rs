//! Conservative independent whole sampler: actual preparation registers,
//! demand cache, RAW565 color products/feedback, and one held quad result.
//! No Program/counting frame/average latency is used on the stepping path.
use crate::{
    memory::ports::MemoryPort,
    texture::{
        emu::{
            cache::{CacheEmu, CacheTick},
            color::{self, ColorEmu},
            derivative,
        },
        ports::Slot,
        sim::staged::bound::serial::{self, PreparationEmu},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuadResult {
    pub quad: u8,
    pub mask: u8,
    pub colors: [[u8; 3]; 4],
}
#[derive(Clone, Copy, Debug)]
struct Active {
    value: QuadResult,
    done: u8,
}
#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub ce: bool,
    pub input: Option<derivative::Input>,
    pub output_ready: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub input_ready: bool,
    pub accepted: bool,
    pub output: Option<QuadResult>,
    pub transferred: bool,
    pub packet_accepted: bool,
    pub color_accepted: bool,
    pub phase: serial::Phase,
}
/// Additional one-quad result/status bank, outside the accepted leaf inventories.
pub const RESULT_DATA_BITS: usize = 4 + 4 + 4 + 4 * 24;
pub const RESULT_CONTROL_BITS: usize = 2; // active valid + terminal wrapper fault
pub struct SamplerEmu<M: MemoryPort> {
    preparation: PreparationEmu,
    cache: CacheEmu<M>,
    color: ColorEmu,
    active: Option<Active>,
    fault: bool,
}
impl<M: MemoryPort> SamplerEmu<M> {
    pub fn new(slots: Vec<Slot>, memory: M, max_wall: u64) -> Result<Self, String> {
        Self::with_config(slots, memory, max_wall, serial::Config::default())
    }
    pub fn with_config(
        slots: Vec<Slot>,
        memory: M,
        max_wall: u64,
        config: serial::Config,
    ) -> Result<Self, String> {
        Ok(Self {
            preparation: PreparationEmu::with_config(max_wall, config)?,
            cache: CacheEmu::new(slots, memory, max_wall)?,
            color: ColorEmu::new(max_wall)?,
            active: None,
            fault: false,
        })
    }
    pub fn cache(&self) -> &CacheEmu<M> {
        &self.cache
    }
    pub fn cache_mut(&mut self) -> &mut CacheEmu<M> {
        &mut self.cache
    }
    pub fn idle(&self) -> bool {
        !self.fault
            && self.active.is_none()
            && self.preparation.idle()
            && self.cache.idle()
            && self.color.idle()
    }
    pub fn faulted(&self) -> bool {
        self.fault || self.cache.faulted() || self.color.faulted() || self.preparation.faulted()
    }
    pub fn output(&self) -> Option<QuadResult> {
        if self.faulted() {
            None
        } else {
            self.active
                .filter(|a| a.done == a.value.mask)
                .map(|a| a.value)
        }
    }
    pub fn abort(&mut self) {
        self.fault = true;
        self.active = None;
        self.cache.abort();
    }
    pub fn drain_tick(&mut self) -> Result<bool, String> {
        self.cache.drain_tick(false).map(|s| s.drained)
    }
    pub fn tick(&mut self, t: Tick) -> Result<Step, String> {
        if self.faulted() {
            return Err("sampler terminal; drain transport explicitly".into());
        }
        let r = self.advance(t);
        if r.is_err() {
            self.fault = true;
            self.cache.abort();
        }
        r
    }
    fn advance(&mut self, t: Tick) -> Result<Step, String> {
        let old = self.active;
        let output = old.filter(|a| a.done == a.value.mask).map(|a| a.value);
        let transferred = t.ce && t.output_ready && output.is_some();
        let input_ready = old.is_none() && self.preparation.input_ready(t.ce);
        let input = t.input.filter(|_| input_ready);
        let packet = self.preparation.output();
        let captured = self.cache.output();
        let color_input = captured.map(|v| color::Input {
            payload: v.payload,
            texels: v.texels,
        });
        // The color tick exposes PRE-edge ready/result. Advancing its owned state
        // first does not create a hardware clock ordering or a credit fallthrough.
        let color = self.color.tick(color::Tick {
            ce: t.ce,
            input: color_input,
            output_ready: true,
        })?;
        let cache = self.cache.tick(CacheTick {
            ce: t.ce,
            input: packet,
            output_ready: color.input_ready,
        })?;
        if cache.transferred != color.accepted {
            return Err("sampler cache/color old-edge disagreement".into());
        }
        let prep = self.preparation.tick(serial::Tick {
            ce: t.ce,
            input,
            output_ready: cache.input_ready,
        })?;
        if prep.transferred != cache.accepted {
            return Err("sampler preparation/cache old-edge disagreement".into());
        }
        if prep.accepted {
            let v = input.ok_or("sampler input reservation")?;
            if old.is_some() {
                return Err("sampler reused live quad bank".into());
            }
            self.active = Some(Active {
                value: QuadResult {
                    quad: v.header.quad,
                    mask: v.header.mask,
                    colors: [[255; 3]; 4],
                },
                done: 0,
            });
        }
        if t.ce {
            if let Some(v) = color.output {
                let a = self.active.as_mut().ok_or("sampler color owner absent")?;
                let lane = v.key % 4;
                let bit = 1 << lane;
                if v.key / 4 != a.value.quad || a.value.mask & bit == 0 || a.done & bit != 0 {
                    return Err("sampler color identity/coverage/duplicate".into());
                }
                a.value.colors[lane as usize] = v.rgb;
                a.done |= bit;
            }
        }
        if transferred {
            self.active = None;
        }
        Ok(Step {
            input_ready,
            accepted: prep.accepted,
            output,
            transferred,
            packet_accepted: cache.accepted,
            color_accepted: color.accepted,
            phase: prep.phase,
        })
    }
}
