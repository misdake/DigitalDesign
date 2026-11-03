//! Production prepare/replay/context owner from `interpolator_stream.v`.
//!
//! The compute engine supplies its actual busy signal and consumes `prepare`
//! and `write_bank`. This owner does not synthesize samples or compute results.
use super::{
    prepare::{Params, Prepare, PrepareInput, PrepareOutput},
    require,
    vertex::Bits,
    ContractError,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub prepare_start: bool,
    pub execution_idle: bool,
    pub descriptor: u32,
    pub raw_valid: bool,
    pub raw_index: u8,
    pub raw_data: Bits,
    pub params: Params,
    /// Standalone Context peer. Stream always derives this from its Compute.
    pub compute_busy: bool,
    pub pool_ready: bool,
    pub pool_quad_id: u8,
    pub quad_valid: bool,
    pub sample_lane: u8,
    pub sample_valid: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub prepare: PrepareOutput,
    pub prepare_input: PrepareInput,
    pub raster_start: bool,
    pub execute_grant: bool,
    pub run_enable: bool,
    pub busy: bool,
    pub exec_idle: bool,
    pub quad_ready: bool,
    pub quad_take: bool,
    pub sample_pid: u8,
    pub pool_rgb: bool,
    pub pool_color: u32,
    pub pool_end_id: u8,
    pub read_bank: bool,
    pub write_bank: bool,
    pub replay_read: bool,
    pub replay_address: u8,
    pub geo_write: bool,
    pub geo_address: u8,
    pub geo_data: Bits,
    pub bound_valid: bool,
    pub bound_empty: bool,
    pub coefficient_bwe: bool,
    pub coefficient_cwe: bool,
    pub coefficient_bwa: u8,
    pub coefficient_cwa: u8,
}
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub rgb: bool,
    pub active_bank: bool,
    pub raster_pending: bool,
    pub current_id: u8,
    pub replay: bool,
    pub replay_slot: u8,
    pub prepared_color: u32,
    pub pool_color: u32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            rgb: false,
            active_bank: false,
            raster_pending: false,
            current_id: 0,
            replay: false,
            replay_slot: 0,
            prepared_color: u32::MAX,
            pool_color: u32::MAX,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub prepare: Prepare,
    pub state: State,
}
impl Context {
    pub fn outputs(&self, i: Input) -> Output {
        let s = &self.state;
        let mut pi = PrepareInput {
            reset: i.reset,
            ce: i.ce,
            start: i.prepare_start,
            descriptor: i.descriptor,
            raw_valid: i.raw_valid,
            raw_index: i.raw_index,
            raw_data: i.raw_data,
            params: i.params,
            ..Default::default()
        };
        // context_ready and empty do not depend on grant. The second evaluation
        // only connects combinational ports; no child state is advanced here.
        let first = self.prepare.outputs(pi);
        let discard = first.context_ready && first.bound_empty;
        let raster_start =
            first.context_ready && !first.bound_empty && i.execution_idle && !s.raster_pending;
        let grant = discard || ((i.execution_idle || s.raster_pending) && !i.compute_busy);
        let replay_read = (s.replay && s.replay_slot < 9) || raster_start;
        let replay_index = if raster_start { 0 } else { s.replay_slot };
        let replay_address = match replay_index {
            0 => 6,
            1 => 9,
            2 => 12,
            3 => 7,
            4 => 10,
            5 => 13,
            6 => 8,
            7 => 11,
            _ => 14,
        };
        pi.execute_grant = grant;
        pi.replay_read = replay_read;
        pi.replay_address = replay_address;
        let p = self.prepare.outputs(pi);
        let return_index = s.replay_slot.wrapping_sub(1) & 15;
        let geo_address = match return_index {
            0 => 0,
            1 => 4,
            2 => 8,
            3 => 1,
            4 => 5,
            5 => 9,
            6 => 2,
            7 => 6,
            _ => 10,
        };
        let b = self.prepare.storage.bq();
        let geo_data = if geo_address & 8 != 0 {
            b.slice(0, 30)
        } else {
            Bits::concat(b.slice(0, 29), Bits::new(0, 1), 1)
        };
        let run_enable = !s.raster_pending;
        let quad_ready = i.pool_ready && run_enable;
        let quad_take = i.quad_valid && quad_ready;
        let write_bank = !s.active_bank;
        Output {
            prepare: p,
            prepare_input: pi,
            raster_start,
            execute_grant: grant,
            run_enable,
            busy: p.busy || i.compute_busy || s.replay,
            exec_idle: !i.compute_busy,
            quad_ready,
            quad_take,
            sample_pid: ((if quad_take {
                i.pool_quad_id
            } else {
                s.current_id
            }) & 7)
                * 4
                + (i.sample_lane & 3),
            pool_rgb: s.rgb,
            pool_color: s.pool_color,
            pool_end_id: s.current_id,
            read_bank: s.active_bank,
            write_bank,
            replay_read,
            replay_address,
            geo_write: s.replay && s.replay_slot <= 9 && !i.reset,
            geo_address,
            geo_data,
            bound_valid: ((s.replay && s.replay_slot == 10) || discard) && !i.reset,
            bound_empty: p.bound_empty,
            coefficient_bwe: p.ports.b_write && matches!(p.ports.b_write_address, 0 | 1 | 4),
            coefficient_cwe: p.ports.c_write && p.ports.c_write_address < 4,
            coefficient_bwa: (u8::from(write_bank) << 3) | (p.ports.b_write_address & 7),
            coefficient_cwa: (u8::from(write_bank) << 3) | (p.ports.c_write_address & 7),
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let o = self.outputs(i);
        if i.reset {
            self.state = State::default();
            return self.prepare.clock(o.prepare_input);
        }
        if !i.ce {
            return Ok(());
        }
        let check = |ok, rule| require(ok, "interpolator", rule);
        // Preserve the production ownership monitors rather than adding a
        // recovery path. Replay only borrows the private PB read port.
        // The override hides the core read strobe, so inspect its scheduler.
        let core = self.prepare.outputs(PrepareInput {
            replay_read: false,
            ..o.prepare_input
        });
        check(
            !(o.replay_read && core.ports.b_read),
            "prepare and geometry replay share PB read",
        )?;
        check(
            !(o.replay_read
                && o.prepare.ports.b_write
                && o.replay_address == o.prepare.ports.b_write_address),
            "PB replay overwritten before read",
        )?;
        check(
            !o.prepare.cfg_apply || o.execute_grant,
            "configuration published before old execution drain",
        )?;
        check(
            !o.raster_start || i.execution_idle,
            "geometry handoff before old raster drain",
        )?;
        check(
            !(i.sample_valid && self.state.raster_pending),
            "new raster sample before I context switch",
        )?;
        check(
            !(o.prepare.context_ready
                && o.bound_empty
                && (o.prepare.cfg_apply || o.raster_start || self.state.replay)),
            "empty context acquired runtime ownership",
        )?;
        let s = &mut self.state;
        if o.raster_start {
            s.replay = true;
            s.replay_slot = 1;
            s.raster_pending = true;
        } else if s.replay {
            if s.replay_slot == 10 {
                s.replay = false;
                s.replay_slot = 0;
            } else {
                s.replay_slot = (s.replay_slot + 1) & 15;
            }
        }
        if o.prepare.cfg_apply {
            s.raster_pending = false;
            s.active_bank = !s.active_bank;
            s.rgb = o.prepare.rgb;
            s.pool_color = s.prepared_color;
        }
        if i.raw_valid && o.prepare.raw_ready {
            if i.raw_index == 10 {
                s.prepared_color =
                    (s.prepared_color & 0xfffc0000) | ((i.raw_data.value >> 36) as u32 & 0x3ffff);
            }
            if i.raw_index == 11 {
                s.prepared_color = (s.prepared_color & 0x3ffff)
                    | (((i.raw_data.value >> 36) as u32 & 0x3fff) << 18);
            }
        }
        if o.quad_take {
            s.current_id = i.pool_quad_id & 7;
        }
        self.prepare.clock(o.prepare_input)
    }
}

/// Actual prepare-to-I composition. Raster and the producer pool connect here;
/// there is no external peer that can manufacture the I busy or result events.
#[derive(Clone, Copy, Debug, Default)]
pub struct StreamInput {
    pub control: Input,
    pub sample_h: u32,
    pub sample_cover: bool,
    pub sample_anchor: bool,
    pub quad_end_valid: bool,
    pub quad_mask: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct StreamOutput {
    pub context: Output,
    pub compute: super::compute::Output,
    pub compute_input: super::compute::Input,
}
#[derive(Clone, Debug, Default)]
pub struct Stream {
    pub context: Context,
    pub compute: super::compute::Compute,
}
impl Stream {
    fn connect(&self, i: StreamInput) -> (Input, super::compute::Input, Output) {
        let mut ci = i.control;
        ci.compute_busy = self.compute.outputs(super::compute::Input::default()).busy;
        let o = self.context.outputs(ci);
        let p = o.prepare;
        let pi = super::compute::Input {
            reset: ci.reset,
            ce: ci.ce,
            read_bank: o.read_bank,
            write_bank: o.write_bank,
            coefficient_bwe: o.coefficient_bwe,
            coefficient_cwe: o.coefficient_cwe,
            coefficient_bwa: o.coefficient_bwa,
            coefficient_cwa: o.coefficient_cwa,
            coefficient_bwd: p.ports.b_data,
            coefficient_cwd: p.ports.c_data.slice(0, 36),
            sample_valid: ci.sample_valid,
            sample_pid: o.sample_pid,
            sample_cover: i.sample_cover,
            sample_anchor: i.sample_anchor,
            sample_h: i.sample_h,
            cfg_valid: p.cfg_apply,
            cfg_z_bias: p.z_bias,
            cfg_far: p.far_z,
            cfg_rgb: p.rgb,
            cfg_shifts: p.shifts,
            param_valid: p.param_valid,
            param_index: p.param_index,
            param_bank: p.param_bank,
            param_data: p.param_data,
            cfg_tex_u: ci.params.tex_u,
            cfg_tex_v: ci.params.tex_v,
            cfg_max_mip: ci.params.max_mip,
            cfg_bias: ci.params.bias,
            quad_end_valid: i.quad_end_valid,
            quad_end_qid: o.pool_end_id,
            quad_end_mask: i.quad_mask,
        };
        (ci, pi, o)
    }
    pub fn outputs(&self, i: StreamInput) -> StreamOutput {
        let (_, pi, context) = self.connect(i);
        StreamOutput {
            context,
            compute: self.compute.outputs(pi),
            compute_input: pi,
        }
    }
    pub fn clock(&mut self, i: StreamInput) -> Result<(), ContractError> {
        let (ci, pi, _) = self.connect(i);
        let preparation = self.context.clock(ci);
        let computation = self.compute.clock(pi);
        preparation?;
        computation
    }
}
