//! Live production frontend-to-pool wiring; pixel consumers remain external.
use super::{
    frontend::{self, Frontend},
    pool::{self, Pool},
    ContractError,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Backend {
    pub ce: bool,
    pub publish_ready: bool,
    pub descriptor_issue_slot: u8,
    pub basic_read: bool,
    pub basic_address: u8,
    pub depth_read: bool,
    pub depth_address: u8,
    pub light_ready: bool,
    pub sample_ready: bool,
    pub retire_valid: bool,
    pub retire_id: u8,
    pub empty_skip: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// Pool ready/ID in this nested value are replaced by the actual pool wires.
    pub frontend: frontend::Input,
    pub backend: Backend,
    pub draw_unlit: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub frontend: frontend::Output,
    pub pool: pool::Output,
    pub idle: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Producer {
    pub frontend: Frontend,
    pub pool: Pool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Frontend(frontend::Error),
    Pool(ContractError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frontend(e) => e.fmt(f),
            Self::Pool(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Error {}
impl Producer {
    pub(crate) fn observe(&self, i: Input, trace: &mut super::trace::Trace) {
        let (fi, pi, _) = self.connect(i);
        self.frontend.observe(fi, trace);
        self.pool.observe(pi, trace);
    }
    pub fn with_triangles() -> Self {
        Self {
            frontend: Frontend::with_triangles(),
            ..Default::default()
        }
    }
    fn connect(&self, i: Input) -> (frontend::Input, pool::Input, Output) {
        let b = i.backend;
        let mut pi = pool::Input {
            reset: i.frontend.reset,
            front_ce: i.frontend.ce,
            backend_ce: b.ce,
            draw_unlit: i.draw_unlit,
            publish_ready: b.publish_ready,
            descriptor_issue_slot: b.descriptor_issue_slot,
            basic_read: b.basic_read,
            basic_address: b.basic_address,
            depth_read: b.depth_read,
            depth_address: b.depth_address,
            light_ready: b.light_ready,
            sample_ready: b.sample_ready,
            retire_valid: b.retire_valid,
            retire_id: b.retire_id,
            empty_skip: b.empty_skip,
            ..Default::default()
        };
        let pool = self.pool.outputs(pi);
        let mut fi = i.frontend;
        fi.pool_ready = pool.quad_ready;
        fi.pool_quad_id = pool.quad_id;
        let front = self.frontend.outputs(fi);
        let co = front.interpolation.compute;
        pi.phase = co.phase;
        pi.quad_valid = front.quad_valid;
        pi.quad_x = front.quad_x;
        pi.quad_y = front.quad_y;
        pi.quad_rgb = front.quad_rgb;
        pi.quad_color = front.quad_color;
        pi.quad_end_valid = front.raster.quad_end_valid;
        pi.quad_end_id = front.interpolation.context.pool_end_id;
        pi.quad_end_mask = front.raster.quad_mask;
        pi.result_write = co.result_we;
        pi.result_address = co.result_address;
        pi.result_data = co.result_data;
        pi.depth_valid = co.depth_valid;
        pi.depth_pid = co.depth_pid;
        pi.depth_z16 = co.depth_z16;
        pi.depth_pass = co.depth_pass;
        pi.lod_valid = co.lod_valid;
        pi.lod_id = co.lod_id;
        pi.lod_value = co.lod_value;
        let pool = self.pool.outputs(pi);
        let idle = front.idle && !pool.busy;
        (
            fi,
            pi,
            Output {
                frontend: front,
                pool,
                idle,
            },
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.connect(i).2
    }
    pub fn clock(&mut self, i: Input) -> Result<(), Error> {
        let (fi, pi, _) = self.connect(i);
        let front = self.frontend.clock(fi);
        let pool = self.pool.clock(pi);
        front.map_err(Error::Frontend)?;
        pool.map_err(Error::Pool)
    }
}
