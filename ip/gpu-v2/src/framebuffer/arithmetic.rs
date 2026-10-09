//! Independent exact ROP leaf arithmetic with a real registered-stage emulator.
//!
//! This module never calls `sim::oracle`, the bounded control model, or any
//! captured trace answer. Division by 255 is a bounded constant shift/add/
//! increment identity, not a general divider. `counted::model()` builds the leaf
//! datapath as a closed `audited::Model` numerical graph and the ledger is read
//! back from that report, so the recorded work follows the model rather than a
//! hand-written tally.
//!
//! Unlike a latency-only queue, [`Pipeline`] is a finite old-state register
//! emulator whose typed stages are exactly the eight arithmetic stages of
//! `rtl/rop_leaf.v`. Each [`LeafTick`] advances the stages on an enabled edge and
//! returns the pre-edge published result; there is no unbounded queue and no
//! evaluate-once-then-delay shortcut. The `CE` edge contract is shared by the
//! RTL and the emulator:
//!
//! * A lane is accepted on an enabled edge (`ce && in_valid`) and captured by
//!   stage 1 at the end of that edge. `in_ready` is `ce`; there is no output
//!   FIFO or backpressure, so the leaf accepts one lane per enabled edge (the
//!   cache reserves the four lane return destinations).
//! * `ce = 0` freezes all eight stages, the valid/key pipeline and the published
//!   result. `reset` clears every valid bit and the published result.
//! * With a lane accepted on enabled edge 1, stage 1 captures at the end of edge
//!   1, the multiply stage 3 captures at the end of edge 3, and the pack stage 8
//!   publishes `out_valid` at the end of edge 8. The result is observed pre-edge
//!   9, so the measured accept-to-return latency is [`LEAF_LATENCY`] = 8 enabled
//!   edges. Counting the acceptance edge itself, the first result is on enabled
//!   edge 9 and the last of `n` back-to-back lanes on enabled edge `8 + n`.
//!
//! Every multiplication below is a dedicated logical 8x8 product feeding one
//! output register (an II=1 logical-multiply baseline). No Gowin DSP macro
//! mapping, placement, fitted area or fmax is claimed; the register inventory and
//! the measured latency are the only hardware statements this module makes.

use super::ports::{Blend, Context, DepthFunc, Fragment};
use audited::{Fixed, FrameReport, Model};

/// Largest numerator produced by this leaf: `255*255 + 127`.
pub const DIV255_SAFE_MAX: u32 = 65_152;

/// Registered pipeline depth of `rtl/rop_leaf.v`, read from its stage registers.
///
/// Stage 1 input, 2 compare/expand, 3 multiply, 4 sum/round, 5 divide255,
/// 6 quantize-multiply, 7 quantize-divide, 8 pack. A lane accepted on enabled
/// edge `e` is observed on enabled edge `e + LEAF_LATENCY`.
pub const LEAF_STAGES: u8 = 8;
pub const LEAF_LATENCY: u8 = LEAF_STAGES;
pub const LEAF_INITIATION_INTERVAL: u8 = 1;

/// The registered leaf RTL, embedded so the certificate and the co-simulation
/// compile against the exact bytes that are simulated.
pub const RTL_SOURCE: &str = include_str!("rtl/rop_leaf.v");

/// Exact `floor(n / 255)` for `n <= DIV255_SAFE_MAX`.
///
/// The identity `(n + (n >> 8) + 1) >> 8` is exact on this closed range; the
/// excluded top of the 16-bit range (`65535`) is never produced by the leaf.
#[inline]
pub fn div255(n: u32) -> u32 {
    debug_assert!(n <= DIV255_SAFE_MAX, "div255 numerator out of range");
    (n + (n >> 8) + 1) >> 8
}

/// Bit-replication expand of a linear RGB565 code to UNORM8.
#[inline]
pub fn expand(code: u16) -> [u8; 3] {
    let r = ((code >> 11) & 31) as u8;
    let g = ((code >> 5) & 63) as u8;
    let b = (code & 31) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

#[inline]
fn quantize_channel(value: u8, steps: u32) -> u16 {
    div255(u32::from(value) * steps + 127) as u16
}

/// Round-to-nearest RGB565 quantization. The denominator is odd, so no tie occurs.
#[inline]
pub fn quantize(rgb: [u8; 3]) -> u16 {
    (quantize_channel(rgb[0], 31) << 11)
        | (quantize_channel(rgb[1], 63) << 5)
        | quantize_channel(rgb[2], 31)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Pixel {
    pub color: u16,
    pub depth: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultPixel {
    pub pixel: Pixel,
    pub color_written: bool,
    pub depth_written: bool,
}

/// Unsigned D16 comparison for one comparison code.
#[inline]
pub fn depth_pass(func: DepthFunc, source: u16, old: u16) -> bool {
    match func {
        DepthFunc::Never => false,
        DepthFunc::Less => source < old,
        DepthFunc::Equal => source == old,
        DepthFunc::LessEqual => source <= old,
        DepthFunc::Greater => source > old,
        DepthFunc::NotEqual => source != old,
        DepthFunc::GreaterEqual => source >= old,
        DepthFunc::Always => true,
    }
}

/// SRC_OVER for one linear UNORM8 channel: `round((A*S + (255-A)*D)/255)`.
#[inline]
pub fn src_over_channel(a: u8, s: u8, d: u8) -> u8 {
    let a = u32::from(a);
    let s = u32::from(s);
    let d = u32::from(d);
    div255(a * s + (255 - a) * d + 127) as u8
}

/// Exact destination result for one covered fragment, independent of the oracle.
///
/// `covered == false` never touches color or depth. `alpha == 0` under SRC_OVER
/// preserves the exact old RGB565 code while depth write stays independent.
pub fn pixel(old: Pixel, source: Fragment, covered: bool, context: Context) -> ResultPixel {
    let pass = covered && depth_pass(context.depth, source.depth, old.depth);
    if !pass {
        return ResultPixel {
            pixel: old,
            color_written: false,
            depth_written: false,
        };
    }
    let dest = expand(old.color);
    let rgb = std::array::from_fn(|i| match context.blend {
        Blend::Replace => source.rgba[i],
        Blend::SrcOver => src_over_channel(source.rgba[3], source.rgba[i], dest[i]),
    });
    ResultPixel {
        pixel: Pixel {
            color: quantize(rgb),
            depth: if context.depth_write {
                source.depth
            } else {
                old.depth
            },
        },
        color_written: true,
        depth_written: context.depth_write,
    }
}

/// One lane's inputs at the registered leaf boundary.
///
/// `key` is the two-bit lane/destination tag carried unchanged through the
/// pipeline; the cache reserves the four return destinations `0..=3`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafInput {
    pub key: u8,
    pub blend: Blend,
    pub depth: DepthFunc,
    pub depth_write: bool,
    pub covered: bool,
    pub old: Pixel,
    pub source: Fragment,
}

impl LeafInput {
    pub const KEY_MASK: u8 = 3;

    pub fn validate(&self) -> Result<(), String> {
        if self.key > Self::KEY_MASK {
            return Err("leaf key outside two bits".into());
        }
        Ok(())
    }

    pub fn context(&self) -> Context {
        Context {
            depth: self.depth,
            depth_write: self.depth_write,
            blend: self.blend,
        }
    }
}

/// A published leaf result carrying its lane key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafOutput {
    pub key: u8,
    pub result: ResultPixel,
}

/// One edge of the leaf clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeafTick {
    /// Clock enable. When false every register freezes.
    pub ce: bool,
    /// Lane offered on this edge; accepted iff `ce`.
    pub input: Option<LeafInput>,
}

/// Pre-edge observation and the edge outcome.
///
/// `output` is the published pack register *before* this edge (the value the
/// synchronous cache consumer observes). `returned` is the actual transfer on
/// this edge (`ce && output.is_some()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafStep {
    pub accepted: bool,
    pub output: Option<LeafOutput>,
    pub returned: bool,
}

// ---------------------------------------------------------------------------
// Typed finite old-state registers: exactly the eight RTL arithmetic stages.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct S1 {
    key: u8,
    blend: Blend,
    func: DepthFunc,
    depth_write: bool,
    covered: bool,
    old_color: u16,
    old_depth: u16,
    src_depth: u16,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct S2 {
    key: u8,
    dr: u8,
    dg: u8,
    db: u8,
    ia: u8,
    a: u8,
    sr: u8,
    sg: u8,
    sb: u8,
    gate: bool,
    dw: bool,
    blend: Blend,
    oc: u16,
    od: u16,
    sd: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct S3 {
    key: u8,
    psr: u16,
    pdr: u16,
    psg: u16,
    pdg: u16,
    psb: u16,
    pdb: u16,
    gate: bool,
    dw: bool,
    blend: Blend,
    sr: u8,
    sg: u8,
    sb: u8,
    oc: u16,
    od: u16,
    sd: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct S4 {
    key: u8,
    nr: u32,
    ng: u32,
    nb: u32,
    gate: bool,
    dw: bool,
    blend: Blend,
    sr: u8,
    sg: u8,
    sb: u8,
    oc: u16,
    od: u16,
    sd: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct S5 {
    key: u8,
    br: u8,
    bg: u8,
    bb: u8,
    gate: bool,
    dw: bool,
    oc: u16,
    od: u16,
    sd: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct S6 {
    key: u8,
    qmr: u32,
    qmg: u32,
    qmb: u32,
    gate: bool,
    dw: bool,
    oc: u16,
    od: u16,
    sd: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct S7 {
    key: u8,
    qr: u8,
    qg: u8,
    qb: u8,
    gate: bool,
    dw: bool,
    oc: u16,
    od: u16,
    sd: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct S8 {
    key: u8,
    color: u16,
    depth: u16,
    color_written: bool,
    depth_written: bool,
}

impl S1 {
    fn capture(input: &LeafInput) -> Self {
        Self {
            key: input.key & LeafInput::KEY_MASK,
            blend: input.blend,
            func: input.depth,
            depth_write: input.depth_write,
            covered: input.covered,
            old_color: input.old.color,
            old_depth: input.old.depth,
            src_depth: input.source.depth,
            sr: input.source.rgba[0],
            sg: input.source.rgba[1],
            sb: input.source.rgba[2],
            sa: input.source.rgba[3],
        }
    }

    fn convert(self) -> S2 {
        let [dr, dg, db] = expand(self.old_color);
        S2 {
            key: self.key,
            dr,
            dg,
            db,
            ia: 255 - self.sa,
            a: self.sa,
            sr: self.sr,
            sg: self.sg,
            sb: self.sb,
            gate: self.covered && depth_pass(self.func, self.src_depth, self.old_depth),
            dw: self.depth_write,
            blend: self.blend,
            oc: self.old_color,
            od: self.old_depth,
            sd: self.src_depth,
        }
    }
}

impl S2 {
    fn convert(self) -> S3 {
        S3 {
            key: self.key,
            psr: u16::from(self.a) * u16::from(self.sr),
            pdr: u16::from(self.ia) * u16::from(self.dr),
            psg: u16::from(self.a) * u16::from(self.sg),
            pdg: u16::from(self.ia) * u16::from(self.dg),
            psb: u16::from(self.a) * u16::from(self.sb),
            pdb: u16::from(self.ia) * u16::from(self.db),
            gate: self.gate,
            dw: self.dw,
            blend: self.blend,
            sr: self.sr,
            sg: self.sg,
            sb: self.sb,
            oc: self.oc,
            od: self.od,
            sd: self.sd,
        }
    }
}

impl S3 {
    fn convert(self) -> S4 {
        S4 {
            key: self.key,
            nr: u32::from(self.psr) + u32::from(self.pdr) + 127,
            ng: u32::from(self.psg) + u32::from(self.pdg) + 127,
            nb: u32::from(self.psb) + u32::from(self.pdb) + 127,
            gate: self.gate,
            dw: self.dw,
            blend: self.blend,
            sr: self.sr,
            sg: self.sg,
            sb: self.sb,
            oc: self.oc,
            od: self.od,
            sd: self.sd,
        }
    }
}

impl S4 {
    fn convert(self) -> S5 {
        let bypass = |blend: Blend, value: u32, src: u8| -> u8 {
            if blend == Blend::SrcOver {
                div255(value) as u8
            } else {
                src
            }
        };
        S5 {
            key: self.key,
            br: bypass(self.blend, self.nr, self.sr),
            bg: bypass(self.blend, self.ng, self.sg),
            bb: bypass(self.blend, self.nb, self.sb),
            gate: self.gate,
            dw: self.dw,
            oc: self.oc,
            od: self.od,
            sd: self.sd,
        }
    }
}

impl S5 {
    fn convert(self) -> S6 {
        S6 {
            key: self.key,
            qmr: (u32::from(self.br) << 5) - u32::from(self.br) + 127,
            qmg: (u32::from(self.bg) << 6) - u32::from(self.bg) + 127,
            qmb: (u32::from(self.bb) << 5) - u32::from(self.bb) + 127,
            gate: self.gate,
            dw: self.dw,
            oc: self.oc,
            od: self.od,
            sd: self.sd,
        }
    }
}

impl S6 {
    fn convert(self) -> S7 {
        S7 {
            key: self.key,
            qr: (div255(self.qmr) as u8) & 0x1f,
            qg: (div255(self.qmg) as u8) & 0x3f,
            qb: (div255(self.qmb) as u8) & 0x1f,
            gate: self.gate,
            dw: self.dw,
            oc: self.oc,
            od: self.od,
            sd: self.sd,
        }
    }
}

impl S7 {
    fn convert(self) -> S8 {
        S8 {
            key: self.key,
            color: if self.gate {
                (u16::from(self.qr) << 11) | (u16::from(self.qg) << 5) | u16::from(self.qb)
            } else {
                self.oc
            },
            depth: if self.gate && self.dw {
                self.sd
            } else {
                self.od
            },
            color_written: self.gate,
            depth_written: self.gate && self.dw,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeafStage {
    S1(S1),
    S2(S2),
    S3(S3),
    S4(S4),
    S5(S5),
    S6(S6),
    S7(S7),
    S8(S8),
}

impl LeafStage {
    fn key(self) -> u8 {
        match self {
            LeafStage::S1(s) => s.key,
            LeafStage::S2(s) => s.key,
            LeafStage::S3(s) => s.key,
            LeafStage::S4(s) => s.key,
            LeafStage::S5(s) => s.key,
            LeafStage::S6(s) => s.key,
            LeafStage::S7(s) => s.key,
            LeafStage::S8(s) => s.key,
        }
    }

    fn advance(self) -> Self {
        match self {
            LeafStage::S1(s) => LeafStage::S2(s.convert()),
            LeafStage::S2(s) => LeafStage::S3(s.convert()),
            LeafStage::S3(s) => LeafStage::S4(s.convert()),
            LeafStage::S4(s) => LeafStage::S5(s.convert()),
            LeafStage::S5(s) => LeafStage::S6(s.convert()),
            LeafStage::S6(s) => LeafStage::S7(s.convert()),
            LeafStage::S7(s) => LeafStage::S8(s.convert()),
            LeafStage::S8(s) => LeafStage::S8(s),
        }
    }
}

/// Real registered leaf emulator.
///
/// Fixed `[Option<LeafStage>; LEAF_STAGES]` state, no queue. `tick` computes the
/// next state from the old state only, so an acceptance on an edge can never be
/// funded by a result published on the same edge (there is no funding path at
/// all here). A wall bound makes every simulation finite.
#[derive(Clone, Debug)]
pub struct Pipeline {
    stages: [Option<LeafStage>; LEAF_STAGES as usize],
    wall: u64,
    enabled: u64,
    max_wall: u64,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new(4_000_000).expect("default rop leaf wall bound")
    }
}

impl Pipeline {
    pub fn new(max_wall: u64) -> Result<Self, String> {
        if max_wall == 0 || max_wall > 4_000_000 {
            return Err("rop leaf wall bound".into());
        }
        Ok(Self {
            stages: [None; LEAF_STAGES as usize],
            wall: 0,
            enabled: 0,
            max_wall,
        })
    }

    /// Clear every stage register and the published result.
    pub fn reset(&mut self) {
        self.stages = [None; LEAF_STAGES as usize];
        self.wall = 0;
        self.enabled = 0;
    }

    /// Advance one edge. `Step.output` is the pre-edge published result.
    pub fn tick(&mut self, tick: LeafTick) -> Result<LeafStep, String> {
        if self.wall >= self.max_wall {
            return Err("rop leaf wall watchdog".into());
        }
        self.wall += 1;
        let output = self.published();
        let accepted = tick.ce && tick.input.is_some();
        let input = tick.input.filter(|_| accepted);
        if let Some(input) = input {
            input.validate()?;
        }
        let mut next: [Option<LeafStage>; LEAF_STAGES as usize] = [None; LEAF_STAGES as usize];
        for (i, stage) in self
            .stages
            .iter()
            .take(LEAF_STAGES as usize - 1)
            .enumerate()
        {
            next[i + 1] = (*stage).map(LeafStage::advance);
        }
        if let Some(input) = input {
            next[0] = Some(LeafStage::S1(S1::capture(&input)));
        }
        if tick.ce {
            self.stages = next;
            self.enabled += 1;
        }
        Ok(LeafStep {
            accepted,
            output,
            returned: tick.ce && output.is_some(),
        })
    }

    fn published(&self) -> Option<LeafOutput> {
        match self.stages[LEAF_STAGES as usize - 1] {
            Some(LeafStage::S8(s)) => Some(LeafOutput {
                key: s.key,
                result: ResultPixel {
                    pixel: Pixel {
                        color: s.color,
                        depth: s.depth,
                    },
                    color_written: s.color_written,
                    depth_written: s.depth_written,
                },
            }),
            _ => None,
        }
    }

    pub fn in_flight(&self) -> usize {
        self.stages.iter().filter(|s| s.is_some()).count()
    }

    pub fn idle(&self) -> bool {
        self.in_flight() == 0
    }

    pub fn latency(&self) -> u8 {
        LEAF_LATENCY
    }

    pub fn initiation_interval(&self) -> u8 {
        LEAF_INITIATION_INTERVAL
    }

    pub fn enabled_edges(&self) -> u64 {
        self.enabled
    }

    pub fn wall(&self) -> u64 {
        self.wall
    }

    /// One lane key per stage, stage 1 first. Diagnostics only.
    pub fn stage_keys(&self) -> [Option<u8>; LEAF_STAGES as usize] {
        std::array::from_fn(|i| self.stages[i].map(LeafStage::key))
    }
}

// ---------------------------------------------------------------------------
// Counted closed audited model (graph provenance only).
// ---------------------------------------------------------------------------

fn product_count(report: &FrameReport) -> u64 {
    report.counts.logical_products.values().sum()
}

/// Counted work read back from the closed audited numerical Model.
///
/// These figures are *graph provenance*: they show which operations the closed
/// audited graph contains. They are not the physical leaf inventory (that is
/// `timed::Resources`) and the numerical answers are checked against the
/// independent `/255` golden in the tests, not against this model's value.
pub mod counted {
    use super::*;

    /// Operation counts for one covered SRC_OVER fragment, read from the audited
    /// report rather than written by hand. The physical leaf always computes the
    /// six blend products even under REPLACE, so REPLACE does not lower them.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Ledger {
        pub multiplies: u64,
        pub adds: u64,
        pub compares: u64,
        pub shifts: u64,
        pub selects: u64,
        pub slices: u64,
        pub resizes: u64,
        pub literals: u64,
        pub operations: u64,
    }

    /// Logical operation budget of the leaf graph. The multiplier count is the
    /// audited logical-product total (`A*S + (255-A)*D` per channel). It is a
    /// logical 8x8 multiply count, not a placed Gowin DSP macro tally.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct LogicalBudget {
        pub logical_multiplies: u8,
        pub logical_adds: u8,
        pub compares: u8,
        pub latency: u8,
        pub initiation_interval: u8,
    }

    /// The single closed numerical graph: one covered SrcOver fragment with a
    /// representative Less depth test, all zero-fraction UNORM/D16 data.
    pub fn model() -> FrameReport {
        build_model().expect("closed ROP numerical model")
    }

    pub fn ledger() -> Ledger {
        let report = model();
        let ops = &report.counts.operations;
        let get = |name: &str| ops.get(name).copied().unwrap_or(0);
        Ledger {
            multiplies: product_count(&report),
            adds: get("add"),
            compares: get("compare"),
            shifts: get("variable_shift"),
            selects: get("select"),
            slices: get("slice_wiring"),
            resizes: get("resize"),
            literals: get("literal"),
            operations: ops.values().sum(),
        }
    }

    pub fn budget() -> LogicalBudget {
        LogicalBudget {
            logical_multiplies: ledger().multiplies as u8,
            logical_adds: ledger().adds as u8,
            compares: ledger().compares as u8,
            latency: LEAF_LATENCY,
            initiation_interval: LEAF_INITIATION_INTERVAL,
        }
    }
}

/// Fixed cycle calendar and hardware certificate of the registered leaf.
///
/// The certificate binds every one of the eight stages to the typed emulator
/// registers that implement it and to exact anchors in `rtl/rop_leaf.v`. It also
/// carries a finite resource inventory and a *measured* accept-to-return latency
/// from the real [`Pipeline`]. It is deliberately not a generic ASAP/modulo
/// proof: the pattern is the implemented register calendar, and every anchor is
/// checked against the simulated RTL source.
pub mod timed {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Stage {
        Input,
        CompareExpand,
        Multiply,
        SumRound,
        Divide255,
        QuantizeMultiply,
        QuantizeDivide,
        Pack,
    }

    impl Stage {
        pub const ALL: [Stage; LEAF_STAGES as usize] = [
            Stage::Input,
            Stage::CompareExpand,
            Stage::Multiply,
            Stage::SumRound,
            Stage::Divide255,
            Stage::QuantizeMultiply,
            Stage::QuantizeDivide,
            Stage::Pack,
        ];

        /// 1-based stage number.
        pub const fn number(self) -> u8 {
            self as u8 + 1
        }

        pub const fn name(self) -> &'static str {
            match self {
                Stage::Input => "Input",
                Stage::CompareExpand => "CompareExpand",
                Stage::Multiply => "Multiply",
                Stage::SumRound => "SumRound",
                Stage::Divide255 => "Divide255",
                Stage::QuantizeMultiply => "QuantizeMultiply",
                Stage::QuantizeDivide => "QuantizeDivide",
                Stage::Pack => "Pack",
            }
        }

        pub const fn operation(self) -> &'static str {
            match self {
                Stage::Input => "input lane registers",
                Stage::CompareExpand => "D16 compare, coverage gate, RGB565 expand",
                Stage::Multiply => "six variable 8x8 products",
                Stage::SumRound => "per-channel sum plus 127",
                Stage::Divide255 => "bounded divide255 (REPLACE bypass)",
                Stage::QuantizeMultiply => "constant 31/63 scale plus 127",
                Stage::QuantizeDivide => "bounded divide255",
                Stage::Pack => "RGB565 pack and result gate",
            }
        }
    }

    /// Arithmetic class of a register stage's datapath.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub enum OpClass {
        Register,
        Compare,
        Expand,
        Multiply,
        Add,
        Divide255,
        Select,
        Pack,
    }

    /// A counted unit use inside one stage.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct LaneUse {
        pub class: OpClass,
        pub count: u32,
        pub width: u32,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct RegisterDecl {
        pub name: &'static str,
        pub width: u32,
    }

    const fn reg(name: &'static str, width: u32) -> RegisterDecl {
        RegisterDecl { name, width }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StageBinding {
        pub stage: Stage,
        pub emulator_type: &'static str,
        pub operation: &'static str,
        pub lanes: &'static [LaneUse],
        pub registers: &'static [RegisterDecl],
        /// Exact substring that must appear in `rtl/rop_leaf.v` for this stage.
        pub rtl_anchor: &'static str,
    }

    const S1_REGS: &[RegisterDecl] = &[
        reg("v1", 1),
        reg("k1", 2),
        reg("blend1", 1),
        reg("func1", 3),
        reg("dw1", 1),
        reg("covered1", 1),
        reg("oc1", 16),
        reg("od1", 16),
        reg("sd1", 16),
        reg("sr1", 8),
        reg("sg1", 8),
        reg("sb1", 8),
        reg("sa1", 8),
    ];
    const S2_REGS: &[RegisterDecl] = &[
        reg("v2", 1),
        reg("k2", 2),
        reg("dr2", 8),
        reg("dg2", 8),
        reg("db2", 8),
        reg("ia2", 8),
        reg("a2", 8),
        reg("sr2", 8),
        reg("sg2", 8),
        reg("sb2", 8),
        reg("gate2", 1),
        reg("dw2", 1),
        reg("blend2", 1),
        reg("oc2", 16),
        reg("od2", 16),
        reg("sd2", 16),
    ];
    const S3_REGS: &[RegisterDecl] = &[
        reg("v3", 1),
        reg("k3", 2),
        reg("psr3", 16),
        reg("pdr3", 16),
        reg("psg3", 16),
        reg("pdg3", 16),
        reg("psb3", 16),
        reg("pdb3", 16),
        reg("gate3", 1),
        reg("dw3", 1),
        reg("blend3", 1),
        reg("sr3", 8),
        reg("sg3", 8),
        reg("sb3", 8),
        reg("oc3", 16),
        reg("od3", 16),
        reg("sd3", 16),
    ];
    const S4_REGS: &[RegisterDecl] = &[
        reg("v4", 1),
        reg("k4", 2),
        reg("nr4", 17),
        reg("ng4", 17),
        reg("nb4", 17),
        reg("gate4", 1),
        reg("dw4", 1),
        reg("blend4", 1),
        reg("sr4", 8),
        reg("sg4", 8),
        reg("sb4", 8),
        reg("oc4", 16),
        reg("od4", 16),
        reg("sd4", 16),
    ];
    const S5_REGS: &[RegisterDecl] = &[
        reg("v5", 1),
        reg("k5", 2),
        reg("br5", 8),
        reg("bg5", 8),
        reg("bb5", 8),
        reg("gate5", 1),
        reg("dw5", 1),
        reg("oc5", 16),
        reg("od5", 16),
        reg("sd5", 16),
    ];
    const S6_REGS: &[RegisterDecl] = &[
        reg("v6", 1),
        reg("k6", 2),
        reg("qmr6", 17),
        reg("qmg6", 17),
        reg("qmb6", 17),
        reg("gate6", 1),
        reg("dw6", 1),
        reg("oc6", 16),
        reg("od6", 16),
        reg("sd6", 16),
    ];
    const S7_REGS: &[RegisterDecl] = &[
        reg("v7", 1),
        reg("k7", 2),
        reg("qr7", 5),
        reg("qg7", 6),
        reg("qb7", 5),
        reg("gate7", 1),
        reg("dw7", 1),
        reg("oc7", 16),
        reg("od7", 16),
        reg("sd7", 16),
    ];
    const S8_REGS: &[RegisterDecl] = &[
        reg("out_valid", 1),
        reg("out_key", 2),
        reg("new_color", 16),
        reg("new_depth", 16),
        reg("color_written", 1),
        reg("depth_written", 1),
    ];

    const MUL16: [LaneUse; 1] = [LaneUse {
        class: OpClass::Multiply,
        count: 6,
        width: 16,
    }];
    const ADD17: [LaneUse; 1] = [LaneUse {
        class: OpClass::Add,
        count: 3,
        width: 17,
    }];
    const DIV_SEL: [LaneUse; 2] = [
        LaneUse {
            class: OpClass::Divide255,
            count: 3,
            width: 17,
        },
        LaneUse {
            class: OpClass::Select,
            count: 3,
            width: 8,
        },
    ];
    const DIV255: [LaneUse; 1] = [LaneUse {
        class: OpClass::Divide255,
        count: 3,
        width: 17,
    }];
    const SELECT16: [LaneUse; 1] = [LaneUse {
        class: OpClass::Select,
        count: 2,
        width: 16,
    }];
    const COMPARE_EXPAND: [LaneUse; 2] = [
        LaneUse {
            class: OpClass::Compare,
            count: 1,
            width: 16,
        },
        LaneUse {
            class: OpClass::Expand,
            count: 3,
            width: 8,
        },
    ];

    /// The implemented eight-stage register calendar.
    pub const BINDINGS: [StageBinding; LEAF_STAGES as usize] = [
        StageBinding {
            stage: Stage::Input,
            emulator_type: "S1",
            operation: Stage::Input.operation(),
            lanes: &[],
            registers: S1_REGS,
            rtl_anchor: "v1 <= in_valid;",
        },
        StageBinding {
            stage: Stage::CompareExpand,
            emulator_type: "S2",
            operation: Stage::CompareExpand.operation(),
            lanes: &COMPARE_EXPAND,
            registers: S2_REGS,
            rtl_anchor: "gate2 <= covered1 && depth_pass(func1, sd1, od1);",
        },
        StageBinding {
            stage: Stage::Multiply,
            emulator_type: "S3",
            operation: Stage::Multiply.operation(),
            lanes: &MUL16,
            registers: S3_REGS,
            rtl_anchor: "psr3 <= {8'd0, a2} * {8'd0, sr2};",
        },
        StageBinding {
            stage: Stage::SumRound,
            emulator_type: "S4",
            operation: Stage::SumRound.operation(),
            lanes: &ADD17,
            registers: S4_REGS,
            rtl_anchor: "nr4 <= psr3 + pdr3 + 16'd127;",
        },
        StageBinding {
            stage: Stage::Divide255,
            emulator_type: "S5",
            operation: Stage::Divide255.operation(),
            lanes: &DIV_SEL,
            registers: S5_REGS,
            rtl_anchor: "br5 <= blend4 ? div_r4[7:0] : sr4;",
        },
        StageBinding {
            stage: Stage::QuantizeMultiply,
            emulator_type: "S6",
            operation: Stage::QuantizeMultiply.operation(),
            lanes: &ADD17,
            registers: S6_REGS,
            rtl_anchor: "qmr6 <= ({9'd0, br5} << 5) - {9'd0, br5} + 17'd127;",
        },
        StageBinding {
            stage: Stage::QuantizeDivide,
            emulator_type: "S7",
            operation: Stage::QuantizeDivide.operation(),
            lanes: &DIV255,
            registers: S7_REGS,
            rtl_anchor: "qr7 <= div_qr[4:0];",
        },
        StageBinding {
            stage: Stage::Pack,
            emulator_type: "S8",
            operation: Stage::Pack.operation(),
            lanes: &SELECT16,
            registers: S8_REGS,
            rtl_anchor: "new_color <= gate7 ? {qr7, qg7, qb7} : oc7;",
        },
    ];

    /// Finite resource inventory of the registered leaf.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Resources {
        pub logical_multiplies: u32,
        pub adders: u32,
        pub compares: u32,
        pub selects: u32,
        pub div255_units: u32,
        pub expand_units: u32,
        pub pipeline_registers: u32,
        pub pipeline_bits: u32,
    }

    impl Resources {
        fn from_bindings(bindings: &[StageBinding]) -> Self {
            let mut logical_multiplies = 0;
            let mut adders = 0;
            let mut compares = 0;
            let mut selects = 0;
            let mut div255_units = 0;
            let mut expand_units = 0;
            let mut pipeline_registers = 0;
            let mut pipeline_bits = 0;
            for binding in bindings {
                for lane in binding.lanes {
                    match lane.class {
                        OpClass::Multiply => logical_multiplies += lane.count,
                        OpClass::Add => adders += lane.count,
                        OpClass::Compare => compares += lane.count,
                        OpClass::Select => selects += lane.count,
                        OpClass::Divide255 => div255_units += lane.count,
                        OpClass::Expand => expand_units += lane.count,
                        OpClass::Register | OpClass::Pack => {}
                    }
                }
                for register in binding.registers {
                    pipeline_registers += 1;
                    pipeline_bits += register.width;
                }
            }
            Self {
                logical_multiplies,
                adders,
                compares,
                selects,
                div255_units,
                expand_units,
                pipeline_registers,
                pipeline_bits,
            }
        }
    }

    /// A bounded simulation of the real register emulator.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Measured {
        pub lanes: usize,
        pub first_accept_edge: u64,
        pub first_return_edge: u64,
        pub last_return_edge: u64,
        pub latency: u64,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Certificate {
        pub bindings: [StageBinding; LEAF_STAGES as usize],
        pub resources: Resources,
        pub latency: u8,
        pub initiation_interval: u8,
        pub measured: Measured,
    }

    pub(super) fn sample_input(key: u8, seed: u64) -> LeafInput {
        let mut x = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut next = || {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as u8
        };
        LeafInput {
            key: key & LeafInput::KEY_MASK,
            blend: if seed.is_multiple_of(2) {
                Blend::SrcOver
            } else {
                Blend::Replace
            },
            depth: match seed % 8 {
                0 => DepthFunc::Never,
                1 => DepthFunc::Less,
                2 => DepthFunc::Equal,
                3 => DepthFunc::LessEqual,
                4 => DepthFunc::Greater,
                5 => DepthFunc::NotEqual,
                6 => DepthFunc::GreaterEqual,
                _ => DepthFunc::Always,
            },
            depth_write: !seed.is_multiple_of(3),
            covered: !seed.is_multiple_of(5),
            old: Pixel {
                color: u16::from_le_bytes([next(), next()]),
                depth: u16::from_le_bytes([next(), next()]),
            },
            source: Fragment {
                rgba: [next(), next(), next(), next()],
                depth: u16::from_le_bytes([next(), next()]),
            },
        }
    }

    /// Bounded measurement of the real register pipeline for `n` back-to-back
    /// lanes accepted on consecutive enabled edges.
    pub fn measure(n: usize) -> Result<Measured, String> {
        if n == 0 || n > 4096 {
            return Err("rop leaf measured batch bounds".into());
        }
        let mut pipe = Pipeline::new(u64::from(LEAF_LATENCY) + n as u64 + 8)?;
        let mut offered = 0usize;
        let mut accepted = 0usize;
        let mut returned = 0usize;
        let mut measured = Measured {
            lanes: n,
            ..Measured::default()
        };
        let edges = u64::from(LEAF_LATENCY) + n as u64 + 4;
        for edge in 1..=edges {
            let input = if offered < n {
                let input = sample_input((offered % 4) as u8, offered as u64);
                offered += 1;
                Some(input)
            } else {
                None
            };
            let step = pipe.tick(LeafTick { ce: true, input })?;
            if step.accepted {
                accepted += 1;
                if measured.first_accept_edge == 0 {
                    measured.first_accept_edge = edge;
                }
            }
            if step.returned {
                returned += 1;
                if measured.first_return_edge == 0 {
                    measured.first_return_edge = edge;
                }
                measured.last_return_edge = edge;
            }
            if returned == n {
                break;
            }
        }
        if accepted != n || returned != n {
            return Err("rop leaf measured completion".into());
        }
        measured.latency = measured.first_return_edge - measured.first_accept_edge;
        if !pipe.idle() {
            return Err("rop leaf measured pipeline not drained".into());
        }
        Ok(measured)
    }

    fn binding_audit(binding: &StageBinding, rtl: &str) -> Result<(), String> {
        if !rtl.contains(binding.rtl_anchor) {
            return Err(format!(
                "stage {} anchor missing: {}",
                binding.stage.number(),
                binding.rtl_anchor
            ));
        }
        for register in binding.registers {
            if !rtl.contains(register.name) {
                return Err(format!(
                    "stage {} register {} missing from RTL",
                    binding.stage.number(),
                    register.name
                ));
            }
        }
        Ok(())
    }

    /// The fixed pattern resource census expected of the implemented calendar.
    pub fn fixed_resources() -> Resources {
        Resources::from_bindings(&BINDINGS)
    }

    /// Build and measure the certificate. The measurement uses the real
    /// [`Pipeline`]; no RTL answer is precomputed.
    pub fn certificate() -> Result<Certificate, String> {
        let bindings = BINDINGS;
        let resources = Resources::from_bindings(&bindings);
        if resources != fixed_resources() {
            return Err("rop leaf resource census drift".into());
        }
        let measured = measure(64)?;
        Ok(Certificate {
            bindings,
            resources,
            latency: LEAF_LATENCY,
            initiation_interval: LEAF_INITIATION_INTERVAL,
            measured,
        })
    }

    /// Audit the certificate against the actual RTL source and the measured
    /// pipeline. Returns the certificate on success.
    pub fn audit(rtl: &str) -> Result<Certificate, String> {
        let certificate = certificate()?;
        for binding in &certificate.bindings {
            binding_audit(binding, rtl)?;
        }
        for anchor in [
            "end else if (ce) begin",
            "out_valid <= 1'b0;",
            "assign in_ready = ce;",
            "out_key <= k7;",
        ] {
            if !rtl.contains(anchor) {
                return Err(format!("missing RTL CE/key anchor: {anchor}"));
            }
        }
        if certificate.latency != LEAF_LATENCY
            || certificate.initiation_interval != LEAF_INITIATION_INTERVAL
        {
            return Err("rop leaf latency/II certificate".into());
        }
        let measured = &certificate.measured;
        if measured.latency != u64::from(LEAF_LATENCY)
            || measured.first_return_edge != u64::from(LEAF_LATENCY) + measured.first_accept_edge
        {
            return Err(format!(
                "rop leaf measured edge convention (latency {}, first return {})",
                measured.latency, measured.first_return_edge
            ));
        }
        let expected = fixed_resources();
        if certificate.resources != expected {
            return Err("rop leaf resource inventory".into());
        }
        if certificate.resources.logical_multiplies != 6
            || certificate.resources.adders != 6
            || certificate.resources.compares != 1
            || certificate.resources.selects != 5
            || certificate.resources.div255_units != 6
            || certificate.resources.expand_units != 3
        {
            return Err("rop leaf fixed resource pattern".into());
        }
        Ok(certificate)
    }
}

/// Build the closed audited numerical graph for one covered SrcOver fragment.
fn build_model() -> Result<FrameReport, audited::Fault> {
    let mut model = Model::numerical();
    let old_color = model.input::<16, 0, false>("old_color", &[0x1234])?;
    let old_depth = model.input::<16, 0, false>("old_depth", &[4321])?;
    let rgba = model.input::<32, 0, false>("src_rgba", &[0x2850_3c01])?;
    let src_depth = model.input::<16, 0, false>("src_depth", &[1000])?;
    let frame = model.compute("rop_leaf", 4096)?;

    let oc = frame.read(old_color.at::<0>())?;
    let od = frame.read(old_depth.at::<0>())?;
    let rgba = frame.read(rgba.at::<0>())?;
    let sd = frame.read(src_depth.at::<0>())?;

    let a: Fixed<8, 0, false> = frame.slice::<8, 0, false, 24>(rgba)?;
    let sr: Fixed<8, 0, false> = frame.slice::<8, 0, false, 0>(rgba)?;
    let sg: Fixed<8, 0, false> = frame.slice::<8, 0, false, 8>(rgba)?;
    let sb: Fixed<8, 0, false> = frame.slice::<8, 0, false, 16>(rgba)?;

    // Expand RGB565 with replication (`c<<3 | c>>2`, `c<<2 | c>>4`).
    let r5: Fixed<5, 0, false> = frame.slice::<5, 0, false, 11>(oc)?;
    let g6: Fixed<6, 0, false> = frame.slice::<6, 0, false, 5>(oc)?;
    let b5: Fixed<5, 0, false> = frame.slice::<5, 0, false, 0>(oc)?;
    let r_hi = frame.shift_left_const::<3, 8, 0, false>(frame.resize_exact::<8, 0, false>(r5)?)?;
    let r_lo: Fixed<3, 0, false> = frame.slice::<3, 0, false, 2>(r5)?;
    let dr = frame.add::<8, 0, false>(r_hi, frame.resize_exact::<8, 0, false>(r_lo)?)?;
    let g_hi = frame.shift_left_const::<2, 8, 0, false>(frame.resize_exact::<8, 0, false>(g6)?)?;
    let g_lo: Fixed<2, 0, false> = frame.slice::<2, 0, false, 4>(g6)?;
    let dg = frame.add::<8, 0, false>(g_hi, frame.resize_exact::<8, 0, false>(g_lo)?)?;
    let b_hi = frame.shift_left_const::<3, 8, 0, false>(frame.resize_exact::<8, 0, false>(b5)?)?;
    let b_lo: Fixed<3, 0, false> = frame.slice::<3, 0, false, 2>(b5)?;
    let db = frame.add::<8, 0, false>(b_hi, frame.resize_exact::<8, 0, false>(b_lo)?)?;

    let ia: Fixed<8, 0, false> = frame.sub_same(Fixed::<8, 0, false>::constant::<255>(), a)?;

    // Six parallel 8x8 products: A*S and (255-A)*D per channel.
    let rows: [Fixed<8, 0, false>; 3] = [sr, sg, sb];
    let dests: [Fixed<8, 0, false>; 3] = [dr, dg, db];
    let mut blended = [Fixed::<8, 0, false>::constant::<0>(); 3];
    for i in 0..3 {
        let p_src: Fixed<16, 0, false> = frame.product(a, rows[i])?;
        let p_dst: Fixed<16, 0, false> = frame.product(ia, dests[i])?;
        let sum: Fixed<17, 0, false> = frame.add::<17, 0, false>(
            frame.resize_exact::<17, 0, false>(p_src)?,
            frame.resize_exact::<17, 0, false>(p_dst)?,
        )?;
        let sum = frame.add::<17, 0, false>(sum, Fixed::<17, 0, false>::constant::<127>())?;
        blended[i] = div255_model(&frame, sum)?;
    }

    // Quantize each blended channel: multiply by 31/63 with shifts/subtracts
    // (`v*31 = (v<<5)-v`, `v*63 = (v<<6)-v`), add 127, divide by 255.
    let shift_bits = [11u32, 5, 0];
    let mut color = Fixed::<16, 0, false>::constant::<0>();
    for i in 0..3 {
        let wide = frame.resize_exact::<14, 0, false>(blended[i])?;
        let doubled = frame.resize_exact::<14, 0, false>(blended[i])?;
        let scaled: Fixed<14, 0, false> = if i == 1 {
            frame.sub::<14, 0, false>(frame.shift_left_const::<6, 14, 0, false>(wide)?, doubled)?
        } else {
            frame.sub::<14, 0, false>(frame.shift_left_const::<5, 14, 0, false>(wide)?, doubled)?
        };
        let scaled = frame.add::<17, 0, false>(
            frame.resize_exact::<17, 0, false>(scaled)?,
            Fixed::<17, 0, false>::constant::<127>(),
        )?;
        let q: Fixed<8, 0, false> = div255_model(&frame, scaled)?;
        let q16 = frame.resize_exact::<16, 0, false>(q)?;
        let shifted = match shift_bits[i] {
            0 => q16,
            5 => frame.shift_left_const::<5, 16, 0, false>(q16)?,
            _ => frame.shift_left_const::<11, 16, 0, false>(q16)?,
        };
        color = frame.add::<16, 0, false>(color, shifted)?;
    }

    // Depth test and coverage gate (representative single 16-bit comparator).
    let pass = frame.less(sd, od)?;
    let new_color = frame.select(pass, color, oc)?;
    let new_depth = frame.select(pass, sd, od)?;
    frame.publish("new_color", new_color)?;
    frame.publish("new_depth", new_depth)?;
    frame.publish("color_written", pass)?;
    Ok(frame.finish())
}

/// Bounded exact `floor(n/255)` inside the audited graph.
fn div255_model(
    frame: &audited::Frame<'_>,
    n: Fixed<17, 0, false>,
) -> Result<Fixed<8, 0, false>, audited::Fault> {
    let n18 = frame.resize_exact::<18, 0, false>(n)?;
    let high = frame.shift(n, Fixed::<18, 0, true>::constant::<-8>())?;
    let high = frame.resize_exact::<18, 0, false>(high)?;
    let t = frame.add::<18, 0, false>(n18, high)?;
    let t = frame.add::<18, 0, false>(t, Fixed::<18, 0, false>::constant::<1>())?;
    let q = frame.shift(t, Fixed::<18, 0, true>::constant::<-8>())?;
    frame.resize_exact::<8, 0, false>(q)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(key: u8, seed: u64) -> LeafInput {
        timed::sample_input(key, seed)
    }

    /// Push one lane through the real register stages with `ce = 1` and return
    /// the published result on the edge it appears.
    fn run_single(pipe: &mut Pipeline, input: LeafInput) -> ResultPixel {
        let mut offered = Some(input);
        for _ in 0..=u64::from(LEAF_LATENCY) {
            let step = pipe
                .tick(LeafTick {
                    ce: true,
                    input: offered.take(),
                })
                .unwrap();
            if let Some(output) = step.output {
                return output.result;
            }
        }
        panic!("leaf did not publish a result");
    }

    #[test]
    fn div255_is_exact_over_the_produced_range() {
        for n in 0..=DIV255_SAFE_MAX {
            assert_eq!(div255(n), n / 255, "n={n}");
        }
    }

    #[test]
    fn independent_leaf_matches_every_rgb565_expand_and_quantize() {
        for code in 0..=u16::MAX {
            let e = expand(code);
            assert_eq!(quantize(e), code, "code={code}");
        }
    }

    #[test]
    fn audited_model_audits_and_counts_six_products() {
        let report = counted::model();
        report.audit().expect("audited report");
        let ledger = counted::ledger();
        assert_eq!(ledger.multiplies, 6, "A*S + (255-A)*D for three channels");
        assert_eq!(ledger.compares, 1);
        assert_eq!(counted::budget().latency, LEAF_LATENCY);
    }

    #[test]
    fn register_emulator_matches_reference_across_contexts() {
        let contexts = [
            Context {
                depth: DepthFunc::Less,
                depth_write: true,
                blend: Blend::Replace,
            },
            Context {
                depth: DepthFunc::Always,
                depth_write: true,
                blend: Blend::SrcOver,
            },
            Context {
                depth: DepthFunc::GreaterEqual,
                depth_write: false,
                blend: Blend::SrcOver,
            },
        ];
        for context in contexts {
            let old = Pixel {
                color: 0x1234,
                depth: 4321,
            };
            for alpha in [0u8, 1, 127, 254, 255] {
                for depth in [0u16, 4320, 4321, 4322, 65535] {
                    for covered in [false, true] {
                        let source = Fragment {
                            rgba: [200, 3, 77, alpha],
                            depth,
                        };
                        let input = LeafInput {
                            key: 2,
                            blend: context.blend,
                            depth: context.depth,
                            depth_write: context.depth_write,
                            covered,
                            old,
                            source,
                        };
                        let expected = pixel(old, source, covered, context);
                        let mut pipe = Pipeline::new(64).unwrap();
                        assert_eq!(run_single(&mut pipe, input), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn measured_latency_is_eight_enabled_edges() {
        let measured = timed::measure(1).unwrap();
        assert_eq!(measured.first_accept_edge, 1);
        assert_eq!(measured.first_return_edge, u64::from(LEAF_LATENCY) + 1);
        assert_eq!(measured.latency, u64::from(LEAF_LATENCY));
        let batch = timed::measure(12).unwrap();
        assert_eq!(batch.last_return_edge, batch.first_return_edge + 11);
        assert_eq!(batch.latency, u64::from(LEAF_LATENCY));
    }

    #[test]
    fn ce_zero_freezes_every_stage_and_reset_clears_valid() {
        let mut pipe = Pipeline::new(64).unwrap();
        let input = sample(3, 1);
        let first = pipe
            .tick(LeafTick {
                ce: true,
                input: Some(input),
            })
            .unwrap();
        assert!(first.accepted && first.output.is_none());
        let frozen = pipe.stage_keys();
        for _ in 0..3 {
            let step = pipe
                .tick(LeafTick {
                    ce: false,
                    input: None,
                })
                .unwrap();
            assert!(!step.accepted && step.output.is_none() && !step.returned);
            assert_eq!(pipe.stage_keys(), frozen, "ce=0 advanced a register");
        }
        let held = pipe
            .tick(LeafTick {
                ce: false,
                input: None,
            })
            .unwrap();
        assert!(held.output.is_none());
        pipe.reset();
        assert!(pipe.idle());
        assert!(pipe.stage_keys().iter().all(Option::is_none));
    }

    #[test]
    fn tick_accepts_one_lane_per_enabled_edge_without_a_queue() {
        let mut pipe = Pipeline::new(64).unwrap();
        let mut outputs = Vec::new();
        for edge in 0..(LEAF_LATENCY as usize + 4) {
            let input = (edge < 4).then(|| sample(edge as u8, edge as u64));
            let step = pipe.tick(LeafTick { ce: true, input }).unwrap();
            if let Some(output) = step.output {
                outputs.push(output);
            }
        }
        assert_eq!(outputs.len(), 4);
        assert!(pipe.idle());
        // The four lanes return in issue order with distinct keys.
        let keys: Vec<u8> = outputs.iter().map(|o| o.key).collect();
        assert_eq!(keys, vec![0, 1, 2, 3]);
    }

    #[test]
    fn timed_certificate_binds_rtl_and_typed_registers() {
        let certificate = timed::audit(RTL_SOURCE).expect("certificate audit");
        assert_eq!(certificate.bindings.len(), LEAF_STAGES as usize);
        assert_eq!(certificate.resources.logical_multiplies, 6);
        assert_eq!(certificate.resources.pipeline_registers, 96);
        assert_eq!(certificate.resources.pipeline_bits, 797);
        for (binding, stage) in certificate.bindings.iter().zip(timed::Stage::ALL) {
            assert_eq!(binding.stage, stage);
        }
        // The emulator type names are the actual private stage types.
        let types: Vec<&str> = certificate
            .bindings
            .iter()
            .map(|b| b.emulator_type)
            .collect();
        assert_eq!(types, vec!["S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8"]);
    }
}
