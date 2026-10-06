//! Conservative, independent one-quad preparation controller. Every arithmetic
//! leaf advances actual registers. No Program, sampled frame, delayed answer,
//! or cache timing template is retained. The packet head is one paid 72-bit FF.
//! This baseline deliberately serializes lanes, planes and emitted groups.
use super::{runtime_membership as member, runtime_packet as packet, transport::Member, Binding};
use crate::texture::emu::{
    coefficient::{self, CoefficientEmu},
    coordinate::{self, CoordinateEmu},
    derivative::{self, DerivativeEmu},
    lod::{self, LodEmu},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Derivative,
    LodIssue,
    Lod,
    CoordinateIssue,
    Coordinate,
    CoefficientIssue,
    Coefficient,
    MemberIssue,
    Member,
    PacketIssue,
    Packet,
    Head,
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
    /// Pre-edge head; visible while paused/full, transferred only on CE+ready.
    pub output: Option<i128>,
    pub transferred: bool,
    pub phase: Phase,
}
/// Additional state beyond the six arithmetic leaves. The declarations count
/// conservative separate banks, including fields inactive in some phases;
/// Rust enum/Option layout and host watchdog counters are not a hardware bill.
pub const CONTROLLER_DATA_BITS: usize = 144 + 76 + 83 + 171 + 92 + 72;
pub const CONTROLLER_CONTROL_BITS: usize = 4 + 2 + 1 + 2 + 1 + 1;

#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    /// Nearest's exact coefficient row is [511,0,0,0], with no coarse plane.
    /// Reuse the existing result bank; no extra payload buffer is allocated.
    pub nearest_bypass: bool,
    /// Remove only transport alignment registers: Membership7->5, Packet9->3.
    /// All arithmetic edges stay registered; the old overlapped Runtime retains
    /// its default alignment and inventory. The serial controller needs none.
    pub short_alignment: bool,
}

/// Constructor-only source for RTL lowering. Only immutable operation/format/
/// placement/literal-ROM descriptions survive; no input-dependent answer does.
pub fn structural_calendars() -> Result<
    (
        derivative::Calendar,
        derivative::Calendar,
        derivative::Calendar,
    ),
    String,
> {
    let binding = Binding::build()?;
    super::runtime_preparation::calendars(&binding)
}

pub struct PreparationEmu {
    config: Config,
    d: DerivativeEmu,
    l: LodEmu,
    c: CoordinateEmu,
    k: CoefficientEmu,
    m: member::Pipeline,
    p: packet::Pipeline,
    phase: Phase,
    uv: [u32; 8],
    pending_lod: Option<lod::Input>,
    lod: Option<lod::Output>,
    coefficients: Option<coefficient::Output>,
    member: Option<Member>,
    head: Option<i128>,
    lane: u8,
    plane: usize,
    tap: u8,
    wall: u64,
    max_wall: u64,
    fault: bool,
}
impl PreparationEmu {
    pub fn new(max_wall: u64) -> Result<Self, String> {
        Self::with_config(max_wall, Config::default())
    }
    pub fn with_config(max_wall: u64, config: Config) -> Result<Self, String> {
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err("serial preparation watchdog".into());
        }
        let (d, l, c) = structural_calendars()?;
        Ok(Self {
            config,
            d: DerivativeEmu::new(d)?,
            l: LodEmu::new(l)?,
            c: CoordinateEmu::new(c)?,
            k: CoefficientEmu::new(max_wall).map_err(|e| format!("coefficient {e:?}"))?,
            m: member::Pipeline::with_short_alignment(config.short_alignment),
            p: packet::Pipeline::with_short_alignment(config.short_alignment),
            phase: Phase::Idle,
            uv: [0; 8],
            pending_lod: None,
            lod: None,
            coefficients: None,
            member: None,
            head: None,
            lane: 0,
            plane: 0,
            tap: 0,
            wall: 0,
            max_wall,
            fault: false,
        })
    }
    pub fn idle(&self) -> bool {
        self.phase == Phase::Idle
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn faulted(&self) -> bool {
        self.fault
    }
    pub fn output(&self) -> Option<i128> {
        self.head
    }
    pub fn input_ready(&self, ce: bool) -> bool {
        ce && !self.fault && self.idle() && self.d.phase().is_multiple_of(8)
    }
    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        if self.fault || self.wall >= self.max_wall {
            self.fault = true;
            return Err("serial preparation terminal/watchdog".into());
        }
        self.wall += 1;
        let r = self.advance(tick);
        if r.is_err() {
            self.fault = true;
        }
        r
    }
    fn advance(&mut self, tick: Tick) -> Result<Step, String> {
        let ce = tick.ce;
        let old = self.phase;
        let output = self.head;
        let transferred = ce && tick.output_ready && output.is_some();
        let input_ready = ce && old == Phase::Idle && self.d.phase().is_multiple_of(8);
        let incoming = tick.input.filter(|_| input_ready);
        if let Some(i) = incoming {
            if i.header.quad >= 16
                || i.header.mask >= 16
                || i.header.slot >= 16
                || i.uv
                    .iter()
                    .any(|v| !(-(1_i64 << 39)..(1_i64 << 39)).contains(v))
            {
                return Err("serial preparation input width".into());
            }
        }
        let d_out = self.d.output()?;
        let l_out = self.l.output()?;
        let c_out = self.c.output()?;
        let l_in = (ce && old == Phase::LodIssue && self.l.phase().is_multiple_of(8))
            .then_some(self.pending_lod)
            .flatten();
        let c_in = if ce && old == Phase::CoordinateIssue && self.c.phase().is_multiple_of(2) {
            let v = self.lod.ok_or("serial coordinate context")?.context;
            Some(coordinate::Input {
                uv: [
                    self.uv[self.lane as usize * 2],
                    self.uv[self.lane as usize * 2 + 1],
                ],
                shift: v.shift,
                nearest: v.nearest,
                halve: v.halve,
                side: v.side,
            })
        } else {
            None
        };
        let k_in = if ce && old == Phase::CoefficientIssue {
            self.coefficients.map(|v| coefficient::Input {
                parents: self.lod.unwrap().context.parents,
                fractions: v.weights.map(|w| [w[0] as u8, w[1] as u8]),
                nearest: self.lod.unwrap().context.nearest,
                metadata: v.metadata,
            })
        } else {
            None
        };
        let m_in = if ce && old == Phase::MemberIssue {
            let v = self.coefficients.ok_or("serial coefficient row")?;
            Some(member::Input {
                weights: v.weights[self.plane],
                coordinates: v.metadata.coordinates[self.plane],
                slot: v.metadata.slot,
                level: v.metadata.levels[self.plane],
                key: v.metadata.key,
                fine: self.plane == 0,
                last_fine: v.metadata.last_fine,
            })
        } else {
            None
        };
        let p_in = (ce && old == Phase::PacketIssue).then(|| packet::Input {
            member: self.member.unwrap(),
            tap: self.tap,
        });
        let accepted = incoming.is_some();
        let d_edge = self.d.tick(ce, incoming.filter(|i| i.header.mask != 0))?;
        let l_edge = self.l.tick(ce, l_in)?;
        let c_edge = self.c.tick(ce, c_in)?;
        let k_edge = self
            .k
            .tick(coefficient::Tick {
                ce,
                input: k_in,
                output_ready: ce && old == Phase::Coefficient,
                work_available: 16,
            })
            .map_err(|e| format!("serial coefficient {e:?}"))?;
        let m_out = self.m.tick(ce, m_in)?;
        let p_out = self.p.tick(ce, p_in)?;
        if ce {
            match old {
                Phase::Idle if d_edge.accepted => {
                    self.phase = if incoming.unwrap().header.mask == 0 {
                        Phase::Idle
                    } else {
                        Phase::Derivative
                    };
                }
                Phase::Derivative if d_out.is_some() => {
                    let v = d_out.unwrap();
                    self.uv = v.uv;
                    self.pending_lod = Some(v.into());
                    self.phase = Phase::LodIssue;
                }
                Phase::LodIssue if l_edge.accepted => {
                    self.pending_lod = None;
                    self.phase = Phase::Lod;
                }
                Phase::Lod if l_out.is_some() => {
                    self.lod = l_out;
                    self.lane = l_out.unwrap().mask.trailing_zeros() as u8;
                    self.phase = Phase::CoordinateIssue;
                }
                Phase::CoordinateIssue if c_edge.accepted => self.phase = Phase::Coordinate,
                Phase::Coordinate if c_out.is_some() => {
                    let o = c_out.unwrap();
                    let l = self.lod.unwrap();
                    // A 171-bit phase-overlay operand row: the first four weight
                    // slices temporarily hold Q8 coordinate fractions.
                    self.coefficients = Some(coefficient::Output {
                        weights: o.fractions.map(|f| [f[0] as u16, f[1] as u16, 0, 0]),
                        metadata: coefficient::Metadata {
                            coordinates: o.coordinates,
                            levels: l.context.levels,
                            slot: l.slot,
                            key: l.quad * 4 + self.lane,
                            last_fine: l.context.last_fine,
                        },
                    });
                    if self.config.nearest_bypass && l.context.nearest {
                        let row = self.coefficients.as_mut().unwrap();
                        row.weights = [[511, 0, 0, 0], [0; 4]];
                        self.plane = 0;
                        self.phase = Phase::MemberIssue;
                    } else {
                        self.phase = Phase::CoefficientIssue;
                    }
                }
                Phase::CoefficientIssue if k_edge.accepted => {
                    self.coefficients = None;
                    self.phase = Phase::Coefficient;
                }
                Phase::Coefficient if k_edge.output.is_some() => {
                    let v = k_edge.output.unwrap();
                    self.plane = usize::from(v.weights[0] == [0; 4]);
                    self.coefficients = Some(v);
                    self.phase = Phase::MemberIssue;
                }
                Phase::MemberIssue => self.phase = Phase::Member,
                Phase::Member if m_out.is_some() => {
                    let v = m_out.unwrap();
                    self.tap = v.emit().trailing_zeros() as u8;
                    self.member = Some(v);
                    self.phase = Phase::PacketIssue;
                }
                Phase::PacketIssue => self.phase = Phase::Packet,
                Phase::Packet if p_out.is_some() => {
                    self.head = p_out;
                    self.phase = Phase::Head;
                }
                Phase::Head if transferred => {
                    self.head = None;
                    let v = self.member.unwrap();
                    let higher = v.emit() & (15_u8 << (self.tap + 1));
                    if higher != 0 {
                        self.tap = higher.trailing_zeros() as u8;
                        self.phase = Phase::PacketIssue;
                    } else if self.plane == 0 && self.coefficients.unwrap().weights[1] != [0; 4] {
                        self.plane = 1;
                        self.member = None;
                        self.phase = Phase::MemberIssue;
                    } else {
                        self.member = None;
                        self.coefficients = None;
                        let higher = self.lod.unwrap().mask & (15_u8 << (self.lane + 1));
                        if higher != 0 {
                            self.lane = higher.trailing_zeros() as u8;
                            self.phase = Phase::CoordinateIssue;
                        } else {
                            self.lod = None;
                            self.phase = Phase::Idle;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(Step {
            input_ready,
            accepted,
            output,
            transferred,
            phase: self.phase,
        })
    }
}
