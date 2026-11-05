use super::*;
#[derive(Clone, Copy, Debug, Default)]
pub struct SinkInput {
    /// Row valid/index/data are replaced by the actual paired C/D source.
    pub cache: Input,
    pub lane_valid: bool,
    pub rgba: Bits,
    pub depth: Bits,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct SinkOutput {
    pub cache: Output,
    pub lane: u8,
    pub depth_phase: bool,
    pub lane_ready: bool,
    pub lane_accept: bool,
    pub color_ready: bool,
    pub color_accept: bool,
    pub quad_accepted: bool,
    pub raw_head_retire: bool,
    pub idle: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Sink {
    pub cache: Cache,
    pub phase: bool,
    pub lane: u8,
}
impl Sink {
    pub(crate) fn observe(
        &self,
        i: SinkInput,
        trace: &mut super::super::trace::Trace,
        quad_slot: u8,
    ) {
        let (ci, _) = self.connect(i);
        self.cache.observe(ci, trace, quad_slot);
    }
    fn connect(&self, i: SinkInput) -> (Input, SinkOutput) {
        let mut ci = i.cache;
        ci.row_valid = i.lane_valid && !ci.reset && self.phase == ci.shade_phase;
        ci.row_index = (self.lane << 1) | u8::from(self.phase);
        ci.data = if self.phase {
            Bits::concat(Bits::new(0, 16), i.depth.slice(0, 16), 16)
        } else {
            i.rgba
        };
        ci.flush_request &= !i.lane_valid && !self.phase && self.lane == 0;
        let c = self.cache.outputs(ci);
        let lane_accept = c.row_accept && self.phase;
        let o = SinkOutput {
            cache: c,
            lane: self.lane,
            depth_phase: self.phase,
            lane_ready: !ci.reset && self.phase,
            lane_accept,
            color_ready: !ci.shade_phase && !ci.reset && !self.phase && c.row_ready,
            color_accept: c.row_accept && !self.phase,
            quad_accepted: lane_accept && self.lane == 3,
            raw_head_retire: ci.ce
                && !ci.reset
                && i.lane_valid
                && self.phase
                && self.phase == ci.shade_phase
                && c.row_ready
                && self.lane == 3,
            idle: c.idle && !self.phase && self.lane == 0,
        };
        (ci, o)
    }
    pub fn outputs(&self, i: SinkInput) -> SinkOutput {
        self.connect(i).1
    }
    pub fn clock(&mut self, i: SinkInput) -> Result<(), ContractError> {
        let (ci, o) = self.connect(i);
        let result = self.cache.clock(ci);
        if ci.reset {
            self.phase = false;
            self.lane = 0;
        } else if o.cache.row_accept {
            self.phase = !self.phase;
            if o.depth_phase {
                self.lane = (self.lane + 1) & 3;
            }
        }
        result
    }
}
