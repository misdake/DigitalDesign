//! Actual G3 T16, preparation, Raster and I wiring from pipeline_stream.v.
use super::{
    interpolator::{self, Stream, StreamInput, StreamOutput},
    prepare::{Params, PrepareInput},
    raster::{self, Raster},
    require,
    vertex::{self, Meshlet},
    ContractError,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub start: bool,
    pub slot: bool,
    pub mesh_color: u32,
    pub dma_write: bool,
    pub dma_address: u16,
    pub dma_data: u64,
    pub output_ready: bool,
    pub cp_read: bool,
    pub cp_address: u16,
    pub params: Params,
    pub pool_ready: bool,
    pub pool_quad_id: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub meshlet: vertex::Output,
    pub interpolation: StreamOutput,
    pub raster: raster::Output,
    pub quad_valid: bool,
    pub quad_x: u8,
    pub quad_y: u8,
    pub quad_rgb: bool,
    pub quad_color: u32,
    pub header_take: bool,
    pub word_ready: bool,
    pub idle: bool,
}
#[derive(Clone, Debug)]
pub struct State {
    pub packet_window: bool,
    pub live_bounds: [u8; 4],
}
impl Default for State {
    fn default() -> Self {
        Self {
            packet_window: false,
            live_bounds: [0, 24, 0, 14],
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Frontend {
    pub meshlet: Meshlet,
    pub interpolation: Stream,
    pub raster: Raster,
    pub state: State,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Vertex(vertex::Error),
    Contract(ContractError),
}
impl From<vertex::Error> for Error {
    fn from(e: vertex::Error) -> Self {
        Self::Vertex(e)
    }
}
impl From<ContractError> for Error {
    fn from(e: ContractError) -> Self {
        Self::Contract(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vertex(e) => e.fmt(f),
            Self::Contract(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Error {}
struct Eval {
    o: Output,
    mi: vertex::Input,
    si: StreamInput,
    ri: raster::Input,
    s_busy: bool,
}
impl Frontend {
    pub(crate) fn observe(&self, i: Input, trace: &mut super::trace::Trace) {
        let e = self.evaluate(i);
        let o = e.o;
        self.meshlet.observe(e.mi, trace);
        trace.load[4] = u64::from(o.raster.busy);
        trace.load[5] = u64::from(o.interpolation.compute.busy);
        trace.load[15] = u64::from(o.raster.quad_take);
        trace.load[16] = u64::from(i.ce && o.interpolation.compute.depth_valid);
        trace.load[25] = u64::from(e.s_busy);
        if i.reset {
            return;
        }
        let export = &self.meshlet.core.export;
        if i.ce && i.start && o.meshlet.ready {
            trace.event("draw", [u64::from(i.slot)]);
        }
        if i.ce && o.meshlet.done {
            trace.event("vt_end", []);
        }
        if o.meshlet.core.publish {
            trace.event("t_publish", [u64::from(export.produced & 15)]);
        }
        if o.header_take {
            trace.event("prepare", [u64::from(export.consumed & 15)]);
        }
        if o.meshlet.core.release_slot {
            trace.event("t_release", [u64::from(export.consumed & 15)]);
        }
        if i.ce && o.interpolation.context.raster_start {
            trace.event("r_start", []);
        }
        if i.ce && o.raster.done {
            trace.event("r_end", []);
        }
        if i.ce && o.interpolation.context.prepare.cfg_apply {
            trace.event("i_context", []);
        }
    }
    /// Keep the inherited V-only default constructor's explicit T boundary.
    pub fn with_triangles() -> Self {
        Self {
            meshlet: Meshlet::with_triangles(),
            ..Default::default()
        }
    }
    fn evaluate(&self, i: Input) -> Eval {
        let s_busy = self
            .interpolation
            .context
            .prepare
            .outputs(PrepareInput::default())
            .busy;
        let mut mi = vertex::Input {
            reset: i.reset,
            ce: i.ce,
            start: i.start,
            slot: i.slot,
            mesh_color: i.mesh_color,
            dma_write: i.dma_write,
            dma_address: i.dma_address,
            dma_data: i.dma_data,
            output_ready: i.output_ready,
            cp_read: i.cp_read,
            cp_address: i.cp_address,
            packet_ready: self.state.packet_window && !i.reset,
            ..Default::default()
        };
        // Export offers and their held data do not depend on the sink's ready.
        // Final CP readiness is evaluated again after the real sink is wired.
        let offers = self.meshlet.outputs(mi);
        let header_take = i.ce && offers.core.packet_valid && mi.packet_ready;
        let control = interpolator::Input {
            reset: i.reset,
            ce: i.ce,
            prepare_start: header_take,
            execution_idle: !self.raster.state.busy,
            descriptor: offers.core.descriptor.value as u32,
            raw_valid: offers.core.word_valid && s_busy && !i.reset,
            raw_index: offers.core.word_index,
            raw_data: offers.core.word_data,
            params: i.params,
            pool_ready: i.pool_ready,
            pool_quad_id: i.pool_quad_id,
            ..Default::default()
        };
        let mut si = StreamInput {
            control,
            ..Default::default()
        };
        let preliminary = self.interpolation.outputs(si);
        let context = preliminary.context;
        let ri = raster::Input {
            reset: i.reset,
            ce: i.ce,
            phase: self.interpolation.compute.state.phase,
            bbox_done: context.bound_valid && !context.bound_empty,
            bbox_empty: false,
            bbox_xlo: self.state.live_bounds[0],
            bbox_xhi: self.state.live_bounds[1],
            bbox_ylo: self.state.live_bounds[2],
            bbox_yhi: self.state.live_bounds[3],
            run_enable: context.run_enable,
            start: context.raster_start,
            geo_write: context.geo_write,
            geo_address: context.geo_address,
            geo_data: context.geo_data,
            quad_ready: context.quad_ready,
        };
        let ro = self.raster.outputs(ri);
        si.control.quad_valid = ro.quad_valid;
        si.control.sample_lane = ro.sample_lane;
        si.control.sample_valid = ro.sample_valid;
        si.sample_h = ro.sample_h.value as u32;
        si.sample_cover = ro.sample_cover;
        si.sample_anchor = ro.sample_anchor;
        si.quad_end_valid = ro.quad_end_valid;
        si.quad_mask = ro.quad_mask;
        let interpolation = self.interpolation.outputs(si);
        mi.word_ready = s_busy && interpolation.context.prepare.raw_ready && !i.reset;
        let meshlet = self.meshlet.outputs(mi);
        let local_drain = !ro.busy && interpolation.context.exec_idle;
        let o = Output {
            meshlet,
            interpolation,
            raster: ro,
            quad_valid: ro.quad_valid && context.run_enable,
            quad_x: ro.quad_x,
            quad_y: ro.quad_y,
            quad_rgb: context.pool_rgb,
            quad_color: context.pool_color,
            header_take,
            word_ready: mi.word_ready,
            idle: self.state.packet_window
                && local_drain
                && !s_busy
                && !meshlet.busy
                && meshlet.drained,
        };
        Eval {
            o,
            mi,
            si,
            ri,
            s_busy,
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.evaluate(i).o
    }
    pub fn clock(&mut self, i: Input) -> Result<(), Error> {
        let e = self.evaluate(i);
        let o = e.o;
        if i.reset {
            self.state = State::default();
        } else if i.ce {
            let check = |ok, rule| require(ok, "frontend", rule);
            check(
                !(o.header_take && e.s_busy),
                "second lookahead packet before prepare completion",
            )?;
            check(
                !(o.interpolation.context.raster_start && o.raster.busy),
                "geometry handoff before old R drain",
            )?;
            check(
                !o.interpolation.context.raster_start || e.s_busy,
                "execution handoff without pending prepare",
            )?;
            if o.header_take {
                check(
                    o.meshlet.core.descriptor.is_known(32),
                    "unknown T descriptor at prepare handoff",
                )?;
            }
            if o.raster.sample_valid
                || (o.interpolation.compute.phase < 3
                    && self.interpolation.compute.state.valid_pipe & 1 != 0)
            {
                check(
                    o.raster.sample_h.is_known(30),
                    "unknown R sample at I handoff",
                )?;
            }
            if o.interpolation.context.raster_start {
                let p = o.interpolation.context.prepare;
                self.state.live_bounds = [p.xlo, p.xhi, p.ylo, p.yhi];
            }
            self.state.packet_window = !o.header_take && !e.s_busy;
        }
        let meshlet = self.meshlet.clock(e.mi);
        let interpolation = self.interpolation.clock(e.si);
        let raster = self.raster.clock(e.ri);
        meshlet?;
        interpolation?;
        raster?;
        Ok(())
    }
}
