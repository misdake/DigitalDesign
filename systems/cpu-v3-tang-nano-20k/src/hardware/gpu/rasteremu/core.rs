//! Cycle model of the rasterizer pipeline, bit-exact against `rastersim`.
//!
//! `RasterCore` follows the `GpuCore` shape: `combine` computes the device
//! arbiter grants from the current state, `advance` applies one clock edge.
//! Every value computation calls the same leaf functions as the functional
//! sim (`rastersim::clip`/`setup`/`raster`), so the two agree bit for bit;
//! the emu adds what the sim does not model: per-stage FSM timing, FIFO
//! backpressure, and shared-device arbitration.
//!
//! Device sharing (constants, tuned later): one `RcpUnit` and one
//! `Multiplier36x18` shared by clip and setup with fixed priority
//! (clip before setup), two `Multiplier18x18` instances shared by setup and
//! the tile corner test (same priority order). Adders are unshared (LUTs are
//! cheap). Sharing affects only timing: device semantics do not depend on
//! the instance count.

use std::collections::VecDeque;

use crate::hardware::gpu::rastersim::clip::{self, ClipVertex};
use crate::hardware::gpu::rastersim::devices::{
    Adder40, Multiplier18x18, RcpOutput, RcpUnit, Saturator,
};
use crate::hardware::gpu::rastersim::fixed::{ClipDist, E40, Q16, S12_4, S13_4, S2_29, U0_18};
use crate::hardware::gpu::rastersim::raster::{self, TileVisit};
use crate::hardware::gpu::rastersim::setup::{
    self, area2, depth_rcp, edge_coefficients, edge_eval, fill_rule, ndc_rcp, quad_aabb,
    TriangleSetup,
};
use crate::{FRAMEBUFFER_TILE, FRAMEBUFFER_TILE_COLUMNS, FRAMEBUFFER_TILE_ROWS};

/// Shared device instance counts.
pub const RCP_COUNT: usize = 1;
pub const M36_COUNT: usize = 1;
pub const M36X36_COUNT: usize = 1;
pub const M18_COUNT: usize = 2;

/// FIFO depths. Storage geometry is not modeled; depth only sets the
/// backpressure timing.
pub const CLIP_OUT_DEPTH: usize = 4;
pub const TRI_FIFO_DEPTH: usize = 4;
pub const TILE_FIFO_DEPTH: usize = 8;
pub const QUAD_FIFO_DEPTH: usize = 24;
/// Prefetch outstanding limit K (per the Phase-1 design).
pub const K_PREFETCH: u32 = 2;

/// One quad FIFO entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuadItem {
    /// A quad with at least one covered pixel (zero masks are not enqueued).
    Quad {
        tri: u32,
        tile: u16,
        x: i32,
        y: i32,
        mask: u8,
    },
    /// All quads of the tile are enqueued before this marker.
    TileEnd { tile: u16 },
}

/// A tile job handed from tile raster to quad raster.
#[derive(Clone, Copy, Debug)]
struct TileJob {
    setup: TriangleSetup,
    visit: TileVisit,
}

// ---------------------------------------------------------------------------
// Clip stage: serial Sutherland-Hodgman, one edge (or one sub-step of an
// intersection) per cycle, fixed plane order.

#[derive(Default)]
struct ClipStage {
    phase: ClipPhase,
    tri: [ClipVertex; 3],
    poly: Vec<ClipVertex>,
    next: Vec<ClipVertex>,
    plane_pos: usize,
}

#[derive(Default)]
enum ClipPhase {
    #[default]
    Idle,
    /// One cycle: outcode classify.
    Classify,
    /// Walking the polygon edges of one plane; `index` is the current vertex
    /// and `prev`/`d_prev` the previous vertex and its distance.
    Plane {
        index: usize,
        prev: ClipVertex,
        d_prev: ClipDist,
    },
    /// Intersection sub-FSM; the plane walk resumes afterwards.
    Intersect(IsectState),
    /// Fan triangulation: emit one triangle per cycle.
    Fan { index: usize },
}

#[derive(Clone, Copy)]
struct IsectState {
    resume_index: usize,
    prev: ClipVertex,
    d_prev: ClipDist,
    step: IsectStep,
    num: ClipDist,
    den: ClipDist,
    /// Divider state: remaining remainder and the quotient being built.
    remainder: i64,
    quotient: u32,
    iter: u8,
    out_v: ClipVertex,
    in_v: ClipVertex,
    result: ClipVertex,
    lerp_i: u8,
    push_cur: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum IsectStep {
    /// 32 iterative shift-subtract cycles (no shared device; Adder41 path).
    Quotient,
    /// Four lerp components, one per cycle (shared 36x36 multiplier).
    Lerp,
}

// ---------------------------------------------------------------------------
// Setup stage: per-vertex rcp/divide/viewport/snap/depth, then area, edge
// coefficients, AABB. One device operation per cycle.

#[derive(Default)]
struct SetupStage {
    phase: SetupPhase,
    tri: [ClipVertex; 3],
    x: [S12_4; 3],
    y: [S12_4; 3],
    depth: [U0_18; 3],
    rcp: Option<RcpOutput>,
    ndc_x: S2_29,
    ndc_y: S2_29,
    prod_x: Q16,
    prod_y: Q16,
    area2: E40,
    cx: [S13_4; 3],
    cy: [S13_4; 3],
    top_left: [bool; 3],
    aabb: Option<[i32; 4]>,
}

#[derive(Clone, Copy, Default)]
enum SetupPhase {
    #[default]
    Idle,
    /// Legal-projection validation (`w >= 1/8`, `0 <= z <= w`); one cycle.
    Validate,
    Rcp {
        i: usize,
    },
    NdcX {
        i: usize,
    },
    NdcY {
        i: usize,
    },
    Viewport {
        i: usize,
    },
    Snap {
        i: usize,
    },
    Depth {
        i: usize,
    },
    Area,
    Coeff {
        i: usize,
    },
    Aabb,
    Emit,
}

// ---------------------------------------------------------------------------
// Tile stage: row-major walk of the triangle's tile AABB with the worst
// corner test; one edge evaluation per cycle (two 18x18 multipliers).

#[derive(Default)]
struct TileStage {
    setup: Option<TriangleSetup>,
    tx0: i32,
    tx1: i32,
    ty0: i32,
    ty1: i32,
    tx: i32,
    ty: i32,
    /// Corner-test progress for the current tile: next edge to evaluate.
    corner_edge: u8,
}

// ---------------------------------------------------------------------------
// Quad stage: per tile, incremental edge functions on the even-aligned quad
// grid; one quad per cycle.

struct QuadStage {
    job: Option<TileJob>,
    qx: i32,
    qy: i32,
    /// Edge values at the current quad origin (incremental).
    e_base: [E40; 3],
    /// Edge values at the current row's first quad origin.
    e_row: [E40; 3],
    /// Initialization progress: edges evaluated so far for a new tile.
    init_edges: u8,
}

impl Default for QuadStage {
    fn default() -> Self {
        Self {
            job: None,
            qx: 0,
            qy: 0,
            e_base: [E40::zero(); 3],
            e_row: [E40::zero(); 3],
            init_edges: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Device arbitration.

/// Stages that can request shared devices, in fixed priority order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ReqStage {
    Clip = 0,
    Setup = 1,
    Tile = 2,
}

/// Per-cycle device requests, computed in `combine`.
#[derive(Default)]
struct DeviceReq {
    rcp: Option<ReqStage>,
    m36: Option<ReqStage>,
    m36x36: Option<ReqStage>,
    /// Number of 18x18 multiplier instances requested per stage.
    m18: [u8; 3],
}

/// Grants decided by the arbiter for this cycle.
#[derive(Default, Clone, Copy)]
pub struct Grants {
    rcp: Option<ReqStage>,
    m36: Option<ReqStage>,
    m36x36: Option<ReqStage>,
    /// 18x18 instances granted per stage.
    m18: [u8; 3],
}

/// A trace event emitted by the core (rendered by the harness).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RasterEvent {
    Scene(String),
    /// Triangle FIFO enqueue (full setup record).
    Tri(TriangleSetup),
    /// Tile accepted into the tile FIFO with a prefetch acquire.
    Prefetch(u16),
    /// Quad FIFO enqueue of an entry (QUAD or tile-end).
    Quad(QuadItem),
    /// Scene completion: number of emitted triangles.
    DoneDraw(u32),
}

impl RasterEvent {
    /// The event body without the `RAST <seq>` prefix (compare format).
    pub fn body(&self) -> String {
        match self {
            Self::Scene(name) => format!("SCENE {name}"),
            Self::Tri(setup) => setup.fifo_line(),
            Self::Prefetch(tile) => format!("PREFETCH {tile}"),
            Self::Quad(QuadItem::Quad {
                tri, x, y, mask, ..
            }) => {
                format!("QUAD {tri} {x} {y} {mask:x}")
            }
            Self::Quad(QuadItem::TileEnd { tile }) => format!("TILE_END {tile}"),
            Self::DoneDraw(triangles) => format!("DONE draw {triangles}"),
        }
    }
}

/// The rasterizer cycle model.
#[derive(Default)]
pub struct RasterCore {
    clip: ClipStage,
    setup: SetupStage,
    tile: TileStage,
    quad: QuadStage,
    clip_out: VecDeque<[ClipVertex; 3]>,
    tri_fifo: VecDeque<TriangleSetup>,
    tile_fifo: VecDeque<TileJob>,
    quad_fifo: VecDeque<QuadItem>,
    /// Rolling triangle id, assigned on emit exactly like `SetupUnit`.
    next_tri_id: u32,
    /// Prefetch acquires not yet retired by a tile-end.
    prefetch_outstanding: u32,
    /// Merger pin: the tile whose quads are currently draining.
    pinned: Option<u16>,
    /// Triangles whose tile walk completed (for the DONE event).
    walked_triangles: u32,
}

impl RasterCore {
    pub fn tri_fifo_len(&self) -> usize {
        self.tri_fifo.len()
    }

    pub fn quad_fifo_len(&self) -> usize {
        self.quad_fifo.len()
    }

    /// Triangles retired so far (for the DONE event payload).
    pub fn walked_triangles(&self) -> u32 {
        self.walked_triangles
    }

    /// True when every stage and FIFO is idle/empty (input consumed).
    pub fn idle(&self) -> bool {
        matches!(self.clip.phase, ClipPhase::Idle)
            && matches!(self.setup.phase, SetupPhase::Idle)
            && self.tile.setup.is_none()
            && self.quad.job.is_none()
            && self.clip_out.is_empty()
            && self.tri_fifo.is_empty()
            && self.tile_fifo.is_empty()
            && self.quad_fifo.is_empty()
    }

    // -----------------------------------------------------------------
    // combine: device requests and arbitration.

    fn device_requests(&self) -> DeviceReq {
        let mut req = DeviceReq::default();
        // Clip: only the intersection lerp needs a shared device (the wide
        // 36x36 multiplier); the iterative divider is an Adder41 path.
        if let ClipPhase::Intersect(state) = &self.clip.phase {
            if state.step == IsectStep::Lerp {
                req.m36x36 = Some(ReqStage::Clip);
            }
        }
        match self.setup.phase {
            SetupPhase::Rcp { .. } => req.rcp = Some(ReqStage::Setup),
            SetupPhase::NdcX { .. } | SetupPhase::NdcY { .. } | SetupPhase::Depth { .. } => {
                req.m36 = Some(ReqStage::Setup);
            }
            SetupPhase::Viewport { .. } | SetupPhase::Area => req.m18[ReqStage::Setup as usize] = 2,
            _ => {}
        }
        if self.tile.setup.is_some() && self.tile.corner_edge < 3 {
            req.m18[ReqStage::Tile as usize] = 2;
        }
        req
    }

    /// Fixed-priority arbitration: clip before setup before tile. An M18
    /// request is for both instances at once (viewport x/y in parallel,
    /// corner-test edges one per cycle); it gets both or waits.
    pub fn combine(&self) -> Grants {
        let req = self.device_requests();
        let mut grants = Grants::default();
        for stage in [ReqStage::Clip, ReqStage::Setup, ReqStage::Tile] {
            if grants.rcp.is_none() && req.rcp == Some(stage) && RCP_COUNT > 0 {
                grants.rcp = Some(stage);
            }
            if grants.m36.is_none() && req.m36 == Some(stage) && M36_COUNT > 0 {
                grants.m36 = Some(stage);
            }
            if grants.m36x36.is_none() && req.m36x36 == Some(stage) && M36X36_COUNT > 0 {
                grants.m36x36 = Some(stage);
            }
        }
        let mut m18_left = M18_COUNT as u8;
        for stage in [ReqStage::Clip, ReqStage::Setup, ReqStage::Tile] {
            let want = req.m18[stage as usize];
            if want > 0 && want <= m18_left {
                grants.m18[stage as usize] = want;
                m18_left -= want;
            }
        }
        grants
    }

    // -----------------------------------------------------------------
    // advance: one clock edge. `events` collects trace events; `sink_ready`
    // is the quad FIFO downstream readiness for this cycle.

    pub fn advance(
        &mut self,
        grants: Grants,
        input: &mut VecDeque<[ClipVertex; 3]>,
        sink_ready: bool,
        events: &mut Vec<RasterEvent>,
    ) {
        self.clip_advance(grants, input);
        self.setup_advance(grants, events);
        self.tile_advance(grants, events);
        self.quad_advance(events);
        self.sink_advance(sink_ready);
    }

    fn clip_advance(&mut self, grants: Grants, input: &mut VecDeque<[ClipVertex; 3]>) {
        match &mut self.clip.phase {
            ClipPhase::Idle => {
                if let Some(tri) = input.pop_front() {
                    self.clip.tri = tri;
                    self.clip.phase = ClipPhase::Classify;
                }
            }
            ClipPhase::Classify => match clip::classify(&self.clip.tri) {
                clip::Classify::Accept => {
                    if self.clip_out.len() < CLIP_OUT_DEPTH {
                        self.clip_out.push_back(self.clip.tri);
                        self.clip.phase = ClipPhase::Idle;
                    }
                }
                clip::Classify::Reject => self.clip.phase = ClipPhase::Idle,
                clip::Classify::Clip => {
                    self.clip.poly = self.clip.tri.to_vec();
                    self.clip.plane_pos = 0;
                    self.clip.next.clear();
                    let prev = *self.clip.poly.last().unwrap();
                    self.clip.phase = ClipPhase::Plane {
                        index: 0,
                        prev,
                        d_prev: clip::distances(&prev)[clip::CLIP_PLANES[0]],
                    };
                }
            },
            ClipPhase::Plane {
                index,
                prev,
                d_prev,
            } => {
                let (index, prev, d_prev) = (*index, *prev, *d_prev);
                let plane = clip::CLIP_PLANES[self.clip.plane_pos];
                let cur = self.clip.poly[index];
                let d_cur = clip::distances(&cur)[plane];
                let prev_in = !d_prev.is_negative();
                let cur_in = !d_cur.is_negative();
                match (prev_in, cur_in) {
                    (true, true) => {
                        self.clip.next.push(cur);
                        self.clip_step_plane(index, cur, d_cur);
                    }
                    (false, false) => self.clip_step_plane(index, cur, d_cur),
                    _ => {
                        // Crossing edge; the intersection is always computed
                        // from the outside endpoint (AB == BA).
                        let (out_v, d_out, in_v, d_in) = if prev_in {
                            (cur, d_cur, prev, d_prev)
                        } else {
                            (prev, d_prev, cur, d_cur)
                        };
                        match clip::intersect_prepare(d_out, d_in) {
                            None => {
                                // Endpoint exactly on the plane: the inside
                                // vertex is the intersection (pushed by the
                                // same rule as the reference).
                                self.clip.next.push(in_v);
                                if cur_in {
                                    self.clip.next.push(cur);
                                }
                                self.clip_step_plane(index, cur, d_cur);
                            }
                            Some((num, den)) => {
                                self.clip.phase = ClipPhase::Intersect(IsectState {
                                    resume_index: index,
                                    prev: cur,
                                    d_prev: d_cur,
                                    step: IsectStep::Quotient,
                                    num,
                                    den,
                                    remainder: num.raw(),
                                    quotient: 0,
                                    iter: 0,
                                    out_v,
                                    in_v,
                                    result: ClipVertex::default(),
                                    lerp_i: 0,
                                    push_cur: cur_in,
                                });
                            }
                        }
                    }
                }
            }
            ClipPhase::Intersect(state) => {
                let mut st = *state;
                match st.step {
                    IsectStep::Quotient => {
                        // The shift-subtract divider runs on its own Adder41
                        // path (no shared multiplier grant), one bit/cycle.
                        let den_raw = st.den.raw();
                        clip::quotient_step(&mut st.remainder, &mut st.quotient, den_raw);
                        st.iter += 1;
                        if st.iter == 32 {
                            st.step = IsectStep::Lerp;
                        }
                    }
                    IsectStep::Lerp => {
                        if grants.m36x36 != Some(ReqStage::Clip) {
                            return;
                        }
                        let t = st.quotient;
                        let value = match st.lerp_i {
                            0 => clip::lerp_q16(st.out_v.x, st.in_v.x, t),
                            1 => clip::lerp_q16(st.out_v.y, st.in_v.y, t),
                            2 => clip::lerp_q16(st.out_v.z, st.in_v.z, t),
                            _ => clip::lerp_q16(st.out_v.w, st.in_v.w, t),
                        };
                        match st.lerp_i {
                            0 => st.result.x = value,
                            1 => st.result.y = value,
                            2 => st.result.z = value,
                            _ => st.result.w = value,
                        }
                        st.lerp_i += 1;
                        if st.lerp_i < 4 {
                            self.clip.phase = ClipPhase::Intersect(st);
                            return;
                        }
                        // Done: push the intersection, then resume the walk.
                        self.clip.next.push(st.result);
                        if st.push_cur {
                            self.clip.next.push(st.prev);
                        }
                        self.clip.phase = ClipPhase::Plane {
                            index: st.resume_index,
                            prev: st.prev,
                            d_prev: st.d_prev,
                        };
                        self.clip_step_plane(st.resume_index, st.prev, st.d_prev);
                    }
                }
                if matches!(self.clip.phase, ClipPhase::Intersect(_)) {
                    self.clip.phase = ClipPhase::Intersect(st);
                }
            }
            ClipPhase::Fan { index } => {
                let i = *index;
                if self.clip.poly.len() < 3 || i + 2 >= self.clip.poly.len() {
                    self.clip.phase = ClipPhase::Idle;
                    return;
                }
                if self.clip_out.len() < CLIP_OUT_DEPTH {
                    let p = &self.clip.poly;
                    self.clip_out.push_back([p[0], p[i + 1], p[i + 2]]);
                    self.clip.phase = ClipPhase::Fan { index: i + 1 };
                }
            }
        }
    }

    /// Advances the plane walk past vertex `index`; at the end of the
    /// polygon, moves to the next clip plane or to fan triangulation.
    fn clip_step_plane(&mut self, index: usize, cur: ClipVertex, d_cur: ClipDist) {
        let next_index = index + 1;
        if next_index < self.clip.poly.len() {
            self.clip.phase = ClipPhase::Plane {
                index: next_index,
                prev: cur,
                d_prev: d_cur,
            };
            return;
        }
        // Plane done: the built polygon becomes the input of the next plane.
        std::mem::swap(&mut self.clip.poly, &mut self.clip.next);
        self.clip.next.clear();
        self.clip.plane_pos += 1;
        if self.clip.poly.len() < 3 || self.clip.plane_pos >= clip::CLIP_PLANES.len() {
            self.clip.phase = if self.clip.poly.len() < 3 {
                ClipPhase::Idle
            } else {
                ClipPhase::Fan { index: 0 }
            };
            return;
        }
        let plane = clip::CLIP_PLANES[self.clip.plane_pos];
        let prev = *self.clip.poly.last().unwrap();
        self.clip.phase = ClipPhase::Plane {
            index: 0,
            prev,
            d_prev: clip::distances(&prev)[plane],
        };
    }

    fn setup_advance(&mut self, grants: Grants, events: &mut Vec<RasterEvent>) {
        match self.setup.phase {
            SetupPhase::Idle => {
                if let Some(tri) = self.clip_out.pop_front() {
                    self.setup.tri = tri;
                    self.setup.phase = SetupPhase::Validate;
                }
            }
            SetupPhase::Validate => {
                // Legal-projection validation: reject (never clamp)
                // triangles whose clipped vertices violate `w >= 1/8` or
                // `0 <= z <= w`, exactly like the reference setup stage.
                // The rejected triangle is dropped without an event.
                if !self.setup.tri.iter().any(|v| !setup::clip_vertex_valid(v)) {
                    self.setup.phase = SetupPhase::Rcp { i: 0 };
                } else {
                    self.setup.phase = SetupPhase::Idle;
                }
            }
            SetupPhase::Rcp { i } => {
                if grants.rcp != Some(ReqStage::Setup) {
                    return;
                }
                self.setup.rcp = Some(RcpUnit::rcp_q16(self.setup.tri[i].w));
                self.setup.phase = SetupPhase::NdcX { i };
            }
            SetupPhase::NdcX { i } => {
                if grants.m36 != Some(ReqStage::Setup) {
                    return;
                }
                self.setup.ndc_x = ndc_rcp(self.setup.tri[i].x, self.setup.rcp.unwrap());
                self.setup.phase = SetupPhase::NdcY { i };
            }
            SetupPhase::NdcY { i } => {
                if grants.m36 != Some(ReqStage::Setup) {
                    return;
                }
                self.setup.ndc_y = ndc_rcp(self.setup.tri[i].y, self.setup.rcp.unwrap());
                self.setup.phase = SetupPhase::Viewport { i };
            }
            SetupPhase::Viewport { i } => {
                if grants.m18[ReqStage::Setup as usize] < 2 {
                    return;
                }
                // hi carries 15 fractional bits; *half-width and rescaled to
                // Q16.16 is a << 1. Both multipliers work in parallel.
                let hi_x = self.setup.ndc_x.high17();
                let hi_y = self.setup.ndc_y.high17();
                self.setup.prod_x = Q16::from_product(
                    Multiplier18x18::mul("viewport.x", hi_x, setup::HALF_WIDTH) << 1,
                );
                self.setup.prod_y = Q16::from_product(
                    Multiplier18x18::mul("viewport.y", hi_y, setup::HALF_HEIGHT) << 1,
                );
                self.setup.phase = SetupPhase::Snap { i };
            }
            SetupPhase::Snap { i } => {
                let x_q16 = Q16::from_product(Adder40::add_fx(
                    "viewport.offset",
                    self.setup.prod_x,
                    setup::VIEWPORT_CENTER_X,
                ));
                let y_q16 = Q16::from_product(Adder40::sub_fx(
                    "viewport.offset",
                    setup::VIEWPORT_CENTER_Y,
                    self.setup.prod_y,
                ));
                self.setup.x[i] = Saturator::snap_s12_4(x_q16);
                self.setup.y[i] = Saturator::snap_s12_4(y_q16);
                self.setup.phase = SetupPhase::Depth { i };
            }
            SetupPhase::Depth { i } => {
                if grants.m36 != Some(ReqStage::Setup) {
                    return;
                }
                self.setup.depth[i] = depth_rcp(self.setup.tri[i].z, self.setup.rcp.unwrap());
                if i + 1 < 3 {
                    self.setup.phase = SetupPhase::Rcp { i: i + 1 };
                } else {
                    self.setup.phase = SetupPhase::Area;
                }
            }
            SetupPhase::Area => {
                if grants.m18[ReqStage::Setup as usize] < 2 {
                    return;
                }
                self.setup.area2 = area2(&self.setup.x, &self.setup.y);
                if self.setup.area2.is_negative() || self.setup.area2 == E40::zero() {
                    // Backface or degenerate: nothing is emitted.
                    self.setup.phase = SetupPhase::Idle;
                    return;
                }
                self.setup.phase = SetupPhase::Coeff { i: 0 };
            }
            SetupPhase::Coeff { i } => {
                // Combinational per edge; the three-edge record is assembled
                // on the first cycle and kept stable.
                if i == 0 {
                    let (cx, cy, top_left) = edge_coefficients(&self.setup.x, &self.setup.y);
                    self.setup.cx = cx;
                    self.setup.cy = cy;
                    self.setup.top_left = top_left;
                }
                self.setup.phase = if i + 1 < 3 {
                    SetupPhase::Coeff { i: i + 1 }
                } else {
                    SetupPhase::Aabb
                };
            }
            SetupPhase::Aabb => {
                self.setup.aabb = quad_aabb(&self.setup.x, &self.setup.y);
                self.setup.phase = SetupPhase::Emit;
            }
            SetupPhase::Emit => {
                if self.tri_fifo.len() >= TRI_FIFO_DEPTH {
                    return;
                }
                if let Some(quad_aabb) = self.setup.aabb {
                    let id = self.next_tri_id;
                    self.next_tri_id = self.next_tri_id.wrapping_add(1);
                    let record = TriangleSetup {
                        id,
                        x: self.setup.x,
                        y: self.setup.y,
                        depth: self.setup.depth,
                        cx: self.setup.cx,
                        cy: self.setup.cy,
                        top_left: self.setup.top_left,
                        area2: self.setup.area2,
                        quad_aabb,
                    };
                    events.push(RasterEvent::Tri(record));
                    self.tri_fifo.push_back(record);
                }
                self.setup.phase = SetupPhase::Idle;
            }
        }
    }

    fn tile_advance(&mut self, grants: Grants, events: &mut Vec<RasterEvent>) {
        if self.tile.setup.is_none() {
            let Some(record) = self.tri_fifo.pop_front() else {
                return;
            };
            let [ax0, ay0, ax1, ay1] = record.quad_aabb;
            let tile = FRAMEBUFFER_TILE as i32;
            self.tile.tx0 = (ax0 / tile).clamp(0, FRAMEBUFFER_TILE_COLUMNS as i32 - 1);
            self.tile.tx1 = (ax1 / tile).clamp(0, FRAMEBUFFER_TILE_COLUMNS as i32 - 1);
            self.tile.ty0 = (ay0 / tile).clamp(0, FRAMEBUFFER_TILE_ROWS as i32 - 1);
            self.tile.ty1 = (ay1 / tile).clamp(0, FRAMEBUFFER_TILE_ROWS as i32 - 1);
            self.tile.tx = self.tile.tx0;
            self.tile.ty = self.tile.ty0;
            self.tile.corner_edge = 0;
            self.tile.setup = Some(record);
            return;
        }
        let setup = self.tile.setup.unwrap();
        // Corner test: one edge per cycle (two 18x18 multipliers).
        if self.tile.corner_edge < 3 {
            if grants.m18[ReqStage::Tile as usize] < 2 {
                return;
            }
            let edge = self.tile.corner_edge as usize;
            if !raster::tile_corner_covered(&setup, edge, self.tile.tx, self.tile.ty) {
                // Rejected tile: skip it (no FIFO entry, no prefetch).
                self.tile_next();
            } else {
                self.tile.corner_edge += 1;
            }
            return;
        }
        // Accepted tile: backpressure from the tile FIFO and prefetch limit.
        if self.tile_fifo.len() >= TILE_FIFO_DEPTH || self.prefetch_outstanding >= K_PREFETCH {
            return;
        }
        let tile = FRAMEBUFFER_TILE as i32;
        let rect = [
            setup.quad_aabb[0].max(self.tile.tx * tile),
            setup.quad_aabb[1].max(self.tile.ty * tile),
            setup.quad_aabb[2].min(self.tile.tx * tile + tile - 1),
            setup.quad_aabb[3].min(self.tile.ty * tile + tile - 1),
        ];
        let index = (self.tile.ty * FRAMEBUFFER_TILE_COLUMNS as i32 + self.tile.tx) as u16;
        let visit = TileVisit { index, rect };
        events.push(RasterEvent::Prefetch(index));
        self.prefetch_outstanding += 1;
        self.tile_fifo.push_back(TileJob { setup, visit });
        self.tile_next();
    }

    fn tile_next(&mut self) {
        self.tile.corner_edge = 0;
        self.tile.tx += 1;
        if self.tile.tx > self.tile.tx1 {
            self.tile.tx = self.tile.tx0;
            self.tile.ty += 1;
            if self.tile.ty > self.tile.ty1 {
                self.tile.setup = None;
                self.walked_triangles += 1;
            }
        }
    }

    fn quad_advance(&mut self, events: &mut Vec<RasterEvent>) {
        if self.quad.job.is_none() {
            let Some(job) = self.tile_fifo.pop_front() else {
                return;
            };
            self.quad.job = Some(job);
            self.quad.qx = job.visit.rect[0];
            self.quad.qy = job.visit.rect[1];
            self.quad.init_edges = 0;
            return;
        }
        let job = self.quad.job.unwrap();
        let qx1 = job.visit.rect[2];
        let qy1 = job.visit.rect[3];
        // Per-tile initialization: evaluate the three edges at the first quad
        // origin, one edge per cycle.
        if self.quad.init_edges < 3 {
            let i = self.quad.init_edges as usize;
            let e = edge_eval(
                &job.setup,
                i,
                S12_4::pixel_center(self.quad.qx),
                S12_4::pixel_center(self.quad.qy),
            );
            self.quad.e_base[i] = e;
            self.quad.e_row[i] = e;
            self.quad.init_edges += 1;
            return;
        }
        if self.quad_fifo.len() >= QUAD_FIFO_DEPTH {
            return;
        }
        // One quad per cycle: derive the four pixel edge values from the
        // quad origin by 40-bit adds (one pixel step = 16 raw units).
        let mut mask = 0u8;
        for dy in 0..2 {
            for dx in 0..2 {
                let x = self.quad.qx + dx;
                let y = self.quad.qy + dy;
                if x > qx1 || y > qy1 {
                    continue;
                }
                let covered = (0..3).all(|i| {
                    // One pixel step = 16 raw units; the delta is an
                    // edge-function (E40) scale value, not an S13_4.
                    let step_x = if dx == 1 {
                        job.setup.cx[i].raw() << 4
                    } else {
                        0
                    };
                    let step_y = if dy == 1 {
                        job.setup.cy[i].raw() << 4
                    } else {
                        0
                    };
                    let e = E40::from_raw(Adder40::add(
                        "edge.accum",
                        Adder40::add("edge.accum", self.quad.e_base[i].raw(), step_x),
                        step_y,
                    ));
                    fill_rule(job.setup.top_left[i], e)
                });
                if covered {
                    mask |= 1 << (dy * 2 + dx);
                }
            }
        }
        if mask != 0 {
            let item = QuadItem::Quad {
                tri: job.setup.id,
                tile: job.visit.index,
                x: self.quad.qx,
                y: self.quad.qy,
                mask,
            };
            events.push(RasterEvent::Quad(item));
            self.quad_fifo.push_back(item);
        }
        // Advance the quad cursor, updating the incremental edge values.
        let next_qx = self.quad.qx + 2;
        if next_qx <= qx1 {
            for i in 0..3 {
                // One quad step = 2 pixels = 32 raw units (E40 scale).
                let step = job.setup.cx[i].raw() << 5;
                self.quad.e_base[i] =
                    E40::from_raw(Adder40::add("edge.accum", self.quad.e_base[i].raw(), step));
            }
            self.quad.qx = next_qx;
            return;
        }
        let next_qy = self.quad.qy + 2;
        if next_qy <= qy1 {
            for i in 0..3 {
                let step = job.setup.cy[i].raw() << 5;
                self.quad.e_row[i] =
                    E40::from_raw(Adder40::add("edge.accum", self.quad.e_row[i].raw(), step));
                self.quad.e_base[i] = self.quad.e_row[i];
            }
            self.quad.qx = job.visit.rect[0];
            self.quad.qy = next_qy;
            return;
        }
        // Tile done: enqueue the tile-end marker.
        let item = QuadItem::TileEnd {
            tile: job.visit.index,
        };
        events.push(RasterEvent::Quad(item));
        self.quad_fifo.push_back(item);
        self.quad.job = None;
    }

    /// Merger/sink: pops one quad FIFO entry when the sink is ready. The pin
    /// is acquired on the first quad of a tile and released by its tile-end;
    /// the tile-end also retires one outstanding prefetch.
    fn sink_advance(&mut self, sink_ready: bool) {
        if !sink_ready {
            return;
        }
        let Some(item) = self.quad_fifo.pop_front() else {
            return;
        };
        match item {
            QuadItem::Quad { tile, .. } => match self.pinned {
                None => self.pinned = Some(tile),
                Some(current) => assert_eq!(current, tile, "pin conflict across tiles"),
            },
            QuadItem::TileEnd { tile } => {
                // A tile with zero covered pixels acquires no pin; its
                // tile-end only retires the prefetch.
                if self.pinned == Some(tile) {
                    self.pinned = None;
                }
                self.prefetch_outstanding -= 1;
            }
        }
    }
}
