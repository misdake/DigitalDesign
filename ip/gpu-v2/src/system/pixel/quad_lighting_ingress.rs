//! Actual Quad ingress connected directly to the frozen Lighting row consumer.
//! Descriptor ownership stays in Quad's Light2 queue. Basic and Sample ports are
//! exposed unchanged. This unit does not implement join, result RAM, or ROP.

use super::{lighting_consumer, quad_ingress};
use crate::lighting::ports::LightingContext;

#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub quad: quad_ingress::Tick,
    /// Parent mux of the immutable bank named by the current Light descriptor.
    pub context: LightingContext,
    pub light_output_ready: bool,
    pub retire_bank: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub quad: quad_ingress::Step,
    pub lighting: lighting_consumer::Signals,
}

pub struct Ingress {
    quad: quad_ingress::Ingress,
    lighting: lighting_consumer::Consumer,
}

impl Ingress {
    pub fn new(max_wall: u64) -> Result<Self, String> {
        Ok(Self {
            quad: quad_ingress::Ingress::new(max_wall),
            lighting: lighting_consumer::Consumer::new(max_wall)?,
        })
    }

    pub fn quad_signals(&self, tick: quad_ingress::Tick) -> quad_ingress::Signals {
        self.quad.signals(tick)
    }

    pub fn light_in_flight(&self) -> usize {
        self.lighting.in_flight()
    }

    pub fn tick(&mut self, mut t: Tick) -> Result<Step, String> {
        let old = self.quad.signals(t.quad);
        let source = old.light.map(|r| lighting_consumer::Source {
            row: r.row,
            data: r.data,
            slot: r.descriptor.slot,
            mask: r.descriptor.mask,
            draw_bank: u8::from(r.descriptor.draw),
        });
        let lighting = self.lighting.tick(lighting_consumer::Tick {
            reset: t.quad.reset,
            ce: old.advance,
            source,
            context: t.context,
            output_ready: t.light_output_ready,
            retire_bank: t.retire_bank,
        })?;
        t.quad.light_ready = lighting.source_ready;
        let quad = self.quad.tick(t.quad)?;
        if quad.light_accepted != (old.advance && lighting.source_ready && old.light.is_some()) {
            return Err("Quad/Lighting source transfer disagreement".into());
        }
        Ok(Step { quad, lighting })
    }
}

pub fn wrapper_verilog() -> &'static str {
    include_str!("quad_lighting_ingress.v")
}

pub fn verilog(backend: quad_ingress::rtl::Backend) -> Result<String, String> {
    Ok(format!(
        "{}\n{}\n{}",
        wrapper_verilog(),
        quad_ingress::rtl::verilog(backend)?,
        lighting_consumer::verilog()?,
    ))
}
