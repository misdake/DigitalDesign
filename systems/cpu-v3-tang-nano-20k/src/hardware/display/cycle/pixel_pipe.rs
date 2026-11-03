//! Registered pixel/sync stage. TMDS encoders consume its old outputs separately.
use super::super::line_buffer::precode;
use super::{ColorPrecode, PixelReadInput, Precode, TimingOutput};

impl From<TimingOutput> for PixelReadInput {
    fn from(t: TimingOutput) -> Self {
        Self {
            pixel_reset: t.pixel_reset,
            h_count: t.h_count,
            v_count: t.v_count,
            scan_enabled: t.started && t.failure_sync & 2 == 0,
            scan_line_ready: t.scan_line_ready,
            scan_valid: t.scan_valid,
            scan_linear: t.scan_linear,
            slot: t.slot,
            second: t.second,
        }
    }
}
impl ColorPrecode {
    /// Pure observation decoder. Unknown channels retain numeric placeholders;
    /// consumers must inspect each channel's known/collision metadata.
    pub fn rgb888(self) -> [u8; 3] {
        self.channels.map(|code| {
            let q = code.value;
            let mut value = q & 1;
            for bit in 1..8 {
                value |= (((q >> bit) ^ (q >> (bit - 1)) ^ u16::from(q & 256 == 0)) & 1) << bit;
            }
            value as u8
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelPipeOutput {
    pub precode: ColorPrecode,
    pub rgb888: [u8; 3],
    pub de: bool,
    pub framebuffer: bool,
    pub hsync: bool,
    pub vsync: bool,
    /// Diagnostic only: the registered code came from a real ready source row.
    /// This differs from both active-video DE and the framebuffer region flag.
    pub effective_source: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct PixelPipe {
    precode: ColorPrecode,
    de: bool,
    framebuffer: bool,
    hsync: bool,
    vsync: bool,
    effective_source: bool,
}
impl Default for PixelPipe {
    fn default() -> Self {
        Self {
            precode: ColorPrecode {
                channels: [Precode {
                    value: precode::tmds_precode(0) as u16,
                    known: true,
                    collision: false,
                }; 3],
            },
            de: false,
            framebuffer: false,
            hsync: false,
            vsync: false,
            effective_source: false,
        }
    }
}
impl PixelPipe {
    pub fn outputs(&self) -> PixelPipeOutput {
        PixelPipeOutput {
            precode: self.precode,
            rgb888: self.precode.rgb888(),
            de: self.de,
            framebuffer: self.framebuffer,
            hsync: self.hsync,
            vsync: self.vsync,
            effective_source: self.effective_source,
        }
    }
    /// One pixel rise, from old Timing and the old read cone's selected pair.
    /// Calling outputs before this commit is the future encoder's input stage.
    pub fn clock(&mut self, timing: &TimingOutput, selected: ColorPrecode) {
        if timing.pixel_reset {
            *self = Self::default();
            return;
        }
        let enabled = timing.started && timing.failure_sync & 2 == 0;
        self.de = timing.active;
        self.framebuffer = enabled && timing.scan_valid && timing.active;
        self.hsync = timing.hsync;
        self.vsync = timing.vsync;
        self.effective_source =
            enabled && timing.active && timing.scan_valid && timing.scan_line_ready;
        self.precode = if self.effective_source {
            selected
        } else {
            Self::default().precode
        };
    }
}
