//! Digital nine-bit-precode TMDS lanes; no serializer or clock ownership.
use super::{ColorPrecode, PixelPipeOutput, Precode};

/// None is an unknown control pin. Control's concrete Timing adapter supplies Some.
#[derive(Clone, Copy, Debug)]
pub struct TmdsInput {
    pub reset: Option<bool>,
    pub de: Option<bool>,
    pub hsync: Option<bool>,
    pub vsync: Option<bool>,
    pub precode: ColorPrecode,
}
impl TmdsInput {
    pub fn from_video(pixel_reset: bool, video: PixelPipeOutput) -> Self {
        Self {
            reset: Some(pixel_reset),
            de: Some(video.de),
            hsync: Some(video.hsync),
            vsync: Some(video.vsync),
            precode: video.precode,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TmdsLaneOutput {
    pub symbol: u16,
    /// A defined integer result, not a complete four-state RTL bit mask.
    pub symbol_defined: bool,
    pub disparity: i8,
    pub disparity_defined: bool,
    /// Passive collision origin, accumulated in this lane until known blank/reset.
    pub tainted: bool,
}
impl Default for TmdsLaneOutput {
    fn default() -> Self {
        Self {
            symbol: 0,
            symbol_defined: false,
            disparity: 0,
            disparity_defined: true,
            tainted: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TmdsOutput {
    /// Logical order R/G/B, matching ColorPrecode; physical pin order is separate.
    pub lanes: [TmdsLaneOutput; 3],
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Tmds {
    output: TmdsOutput,
}
fn signed6(value: i32) -> i8 {
    let bits = (value & 63) as i8;
    if bits & 32 != 0 {
        bits - 64
    } else {
        bits
    }
}
fn control_symbol(control: u8) -> u16 {
    match control & 3 {
        0 => 0x354,
        1 => 0x0ab,
        2 => 0x154,
        _ => 0x2ab,
    }
}
fn active(old: TmdsLaneOutput, code: Precode) -> TmdsLaneOutput {
    let tainted = old.tainted || code.collision;
    if !old.disparity_defined || !code.known {
        return TmdsLaneOutput {
            symbol_defined: false,
            disparity_defined: false,
            tainted,
            ..old
        };
    }
    let qm = code.value & 511;
    let ones = (qm & 255).count_ones() as i32;
    // Production: four-bit popcount; signed six-bit concat(0, ones, 0) - 6'sd8.
    let delta = i32::from(signed6(ones * 2 - 8));
    let d = i32::from(old.disparity);
    let qm8 = qm & 256 != 0;
    let invert = (!qm) & 255;
    let (symbol, next) = if d == 0 || ones == 4 {
        (
            (u16::from(!qm8) << 9) | (qm & 256) | if qm8 { qm & 255 } else { invert },
            if qm8 { d + delta } else { d - delta },
        )
    } else if (d > 0 && ones > 4) || (d < 0 && ones < 4) {
        (
            512 | (qm & 256) | invert,
            d - delta + if qm8 { 2 } else { 0 },
        )
    } else {
        (qm, d + delta - if qm8 { 0 } else { 2 })
    };
    // The zero branch assigns a six-bit add/sub. The other source expressions
    // widen to signed 32 bits through unsized zero, then truncate on assignment.
    // Wide signed arithmetic followed by this final six-bit wrap matches both.
    TmdsLaneOutput {
        symbol,
        symbol_defined: true,
        disparity: signed6(next),
        disparity_defined: true,
        tainted,
    }
}
impl Tmds {
    pub fn outputs(&self) -> TmdsOutput {
        self.output
    }
    /// Commit one pixel rise. Reset is synchronous, never an asynchronous setter.
    pub fn clock(&mut self, input: &TmdsInput) {
        if input.reset == Some(true) {
            self.output.lanes = [TmdsLaneOutput {
                symbol: 0x354,
                symbol_defined: true,
                disparity: 0,
                disparity_defined: true,
                tainted: false,
            }; 3];
        } else if input.reset.is_none() || input.de.is_none() {
            for (lane, code) in self.output.lanes.iter_mut().zip(input.precode.channels) {
                lane.symbol_defined = false;
                lane.disparity_defined = false;
                if input.de != Some(false) {
                    lane.tainted |= code.collision;
                }
            }
        } else if input.de == Some(false) {
            let blue_control = input
                .hsync
                .zip(input.vsync)
                .map(|(hs, vs)| u8::from(hs) | (u8::from(vs) << 1));
            for (index, lane) in self.output.lanes.iter_mut().enumerate() {
                let control = if index == 2 { blue_control } else { Some(0) };
                *lane = TmdsLaneOutput {
                    symbol: control.map_or(0, control_symbol),
                    symbol_defined: control.is_some(),
                    disparity: 0,
                    disparity_defined: true,
                    tainted: false,
                };
            }
        } else {
            self.output.lanes = std::array::from_fn(|index| {
                active(self.output.lanes[index], input.precode.channels[index])
            });
        }
    }
}
