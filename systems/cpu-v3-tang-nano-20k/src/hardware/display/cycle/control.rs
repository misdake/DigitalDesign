//! Display control cone: Fill + pixel timing + captured line writes.
//! Pixel color reads/conversion are not implemented by this control component.

use super::{
    Edges, Fill, FillInput, FillOutput, LineBuffer, LineBufferInput, RamWord, Timing, TimingInput,
    TimingOutput,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct ControlInput {
    pub reset: bool,
    pub video_locked: bool,
    pub boot_enable: bool,
    pub boot_failed: bool,
    pub memory_request_ready: bool,
    pub memory_data_valid: bool,
    pub memory_read_data: u64,
    pub memory_last: bool,
    pub memory_error: bool,
    pub device_index: u8,
    pub device_channel: u8,
    pub device_read_enable: bool,
    pub device_write_enable: bool,
    pub device_write_data: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlOutput {
    pub logic: FillOutput,
    pub pixel: TimingOutput,
}
#[derive(Clone, Debug, Default)]
pub struct DisplayControl {
    fill: Fill,
    timing: Timing,
    ram: LineBuffer,
}
impl DisplayControl {
    fn fill_input(&self, input: &ControlInput) -> FillInput {
        let pixel = self.timing.outputs();
        FillInput {
            reset: input.reset,
            boot_enable: input.boot_enable,
            boot_failed: input.boot_failed,
            memory_request_ready: input.memory_request_ready,
            memory_data_valid: input.memory_data_valid,
            memory_read_data: input.memory_read_data,
            memory_last: input.memory_last,
            memory_error: input.memory_error,
            device_index: input.device_index,
            device_channel: input.device_channel,
            device_read_enable: input.device_read_enable,
            device_write_enable: input.device_write_enable,
            device_write_data: input.device_write_data,
            released: pixel.released,
            frame_toggle: pixel.frame_toggle,
            pixel_underflow: pixel.underflow,
            pixel_started: pixel.started,
        }
    }
    pub fn outputs(&self, input: &ControlInput) -> ControlOutput {
        ControlOutput {
            logic: self.fill.outputs(&self.fill_input(input)),
            pixel: self.timing.outputs(),
        }
    }
    pub fn line_word(&self, bank: usize, address: u16) -> RamWord {
        self.ram.peek(bank, address)
    }
    /// A pure old-state view of the actual failure input to the pixel domain.
    pub fn memory_error_sticky(&self) -> bool {
        self.fill.memory_error_sticky()
    }
    /// No clock calendar, MC or backing is owned here. At coincident rises,
    /// pre-sample both domains before committing either state.
    pub fn clock(&mut self, edges: Edges, input: &ControlInput) {
        let fill_input = self.fill_input(input);
        let old = self.fill.outputs(&fill_input);
        let timing_input = TimingInput {
            reset: input.reset,
            video_locked: input.video_locked,
            boot_enable: input.boot_enable,
            boot_failed: input.boot_failed,
            memory_error: self.fill.memory_error_sticky(),
            published: old.published,
            group_valid: old.group_valid,
            group_linear: old.group_linear,
        };
        if edges.logic_rise {
            self.fill.clock(&fill_input);
        }
        if edges.pixel_rise {
            self.timing.clock(&timing_input);
        }
        // Capture the production fill writes. The independent dual-clock RAM
        // leaf retains its read API; scanout's color read calendar is absent here.
        self.ram.clock(
            Edges {
                logic_rise: edges.logic_rise,
                pixel_rise: false,
            },
            &LineBufferInput {
                write_enable: old.line_write,
                write_address: old.line_write_address,
                write_data: old.line_write_data,
                ..Default::default()
            },
        );
    }
}
