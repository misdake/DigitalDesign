//! One actual Quad ingress feeding both selected Lighting and canonical UV rows.
//! Context8/Runtime owns UV reservations and release; the parent owns immutable
//! draw banks, branch-result storage, join and ROP. No payload shadow is added.
use super::{lighting_consumer, quad_ingress, quad_sampling_ingress};
use crate::lighting::ports::LightingContext;

#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub sampling: quad_sampling_ingress::Tick,
    pub lighting_context: LightingContext,
    pub lighting_output_ready: bool,
    pub retire_bank: Option<u8>,
}
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub sampling: quad_sampling_ingress::Step,
    pub lighting: lighting_consumer::Signals,
}
pub struct Ingress {
    sampling: quad_sampling_ingress::Ingress,
    lighting: lighting_consumer::Consumer,
}
impl Ingress {
    pub fn new(max_wall: u64) -> Result<Self, String> {
        Ok(Self {
            sampling: quad_sampling_ingress::Ingress::new(max_wall),
            lighting: lighting_consumer::Consumer::new(max_wall)?,
        })
    }
    pub fn signals(&self, tick: quad_sampling_ingress::Tick) -> quad_sampling_ingress::Signals {
        self.sampling.signals(tick)
    }
    pub fn light_in_flight(&self) -> usize {
        self.lighting.in_flight()
    }
    pub fn tick(&mut self, mut t: Tick) -> Result<Step, String> {
        let old = self.sampling.signals(t.sampling);
        let lighting = self.lighting.tick(lighting_consumer::Tick {
            reset: t.sampling.quad.reset,
            ce: old.quad.advance,
            source: old.quad.light.map(|r| lighting_consumer::Source {
                row: r.row,
                data: r.data,
                slot: r.descriptor.slot,
                mask: r.descriptor.mask,
                draw_bank: u8::from(r.descriptor.draw),
            }),
            context: t.lighting_context,
            output_ready: t.lighting_output_ready,
            retire_bank: t.retire_bank,
        })?;
        t.sampling.quad.light_ready = lighting.source_ready;
        let sampling = self.sampling.tick(t.sampling)?;
        Ok(Step { sampling, lighting })
    }
}
pub fn wrapper_verilog() -> &'static str {
    include_str!("pixel_ingress.v")
}
pub fn verilog(backend: quad_ingress::rtl::Backend) -> Result<String, String> {
    Ok(format!(
        "{}\n{}\n{}",
        wrapper_verilog(),
        quad_sampling_ingress::verilog(backend, false)?,
        lighting_consumer::verilog()?
    ))
}
