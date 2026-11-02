//! Logic-domain fill, publication, CDC return and Device3 mirror.
//! Pixel feedback is sampled input; this component does not advance a consumer or MC.

#[derive(Clone, Copy, Debug, Default)]
pub struct FillInput {
    pub reset: bool,
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
    pub released: u8,
    pub frame_toggle: bool,
    pub pixel_underflow: bool,
    pub pixel_started: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillOutput {
    pub memory_request_valid: bool,
    pub memory_urgent: bool,
    pub memory_address: u32,
    pub line_write: bool,
    pub line_write_address: u16,
    pub line_write_data: u64,
    pub published: u8,
    pub group_valid: u8,
    pub group_linear: u8,
    pub device_read_data: u16,
    pub underflow: bool,
    pub started_ack: bool,
    pub startup_error: bool,
    pub frame_event: bool,
}

const UNIT_MASK: u32 = (1 << 18) - 1;
const BASE: u32 = crate::FRAMEBUFFER_A_BASE_WORD >> 4;
const LAST_BASE: u32 = ((1 << 22) - (crate::GPU_FRAMEBUFFER_PHYSICAL_SECTORS as u32 * 64)) >> 4;
const LAST_BURST: u8 = (crate::FRAMEBUFFER_WIDTH / 16 - 1) as u8;
const LAST_GROUP: u8 = (crate::FRAMEBUFFER_HEIGHT / 2 - 1) as u8;

#[derive(Clone, Copy, Debug)]
pub struct Fill {
    published: u8,
    release_meta: u8,
    release_sync: u8,
    slot: bool,
    group: u8,
    active_base: u32,
    tile_base: u32,
    shadow_base: u32,
    shadow_high_bad: bool,
    shadow_low_bad: bool,
    pending_base: u32,
    shadow_linear: bool,
    pending_linear: bool,
    active_linear: bool,
    group_linear: u8,
    front_valid: bool,
    group_valid: u8,
    low_written: bool,
    high_written: bool,
    pending: bool,
    invalid: bool,
    frame_complete: bool,
    frame_index: u16,
    frame_meta: bool,
    frame_sync: bool,
    frame_seen: bool,
    underflow_meta: bool,
    underflow_sync: bool,
    started_meta: bool,
    started_sync: bool,
    burst: u8,
    second: bool,
    beat: u8,
    burst_active: bool,
    memory_error: bool,
}

impl Default for Fill {
    fn default() -> Self {
        Self {
            published: 0,
            release_meta: 0,
            release_sync: 0,
            slot: false,
            group: 0,
            active_base: BASE,
            tile_base: BASE,
            shadow_base: BASE,
            shadow_high_bad: false,
            shadow_low_bad: false,
            pending_base: BASE,
            shadow_linear: false,
            pending_linear: false,
            active_linear: false,
            group_linear: 0,
            front_valid: false,
            group_valid: 0,
            low_written: false,
            high_written: false,
            pending: false,
            invalid: false,
            frame_complete: false,
            frame_index: 0,
            frame_meta: false,
            frame_sync: false,
            frame_seen: false,
            underflow_meta: false,
            underflow_sync: false,
            started_meta: false,
            started_sync: false,
            burst: 0,
            second: false,
            beat: 0,
            burst_active: false,
            memory_error: false,
        }
    }
}

impl Fill {
    fn allowed(&self, input: &FillInput) -> bool {
        input.boot_enable
            && !input.boot_failed
            && ((self.published ^ self.release_sync) & (1 << u8::from(self.slot)) == 0)
            && !self.burst_active
            && !self.frame_complete
            && !self.memory_error
    }

    /// Pure old-state outputs. Invalid payload is immaterial when line_write is false.
    pub fn outputs(&self, input: &FillInput) -> FillOutput {
        let partial = self.low_written ^ self.high_written;
        let status = u16::from(self.pending)
            | (u16::from(partial) << 1)
            | (u16::from(self.invalid) << 2)
            | (u16::from(self.underflow_sync) << 3)
            | (u16::from(self.memory_error) << 4);
        let active = self.active_base << 4;
        let device_read_data = if input.device_read_enable && input.device_index & 7 == 3 {
            match input.device_channel & 15 {
                0 => self.frame_index,
                1 => active as u16,
                2 => (active >> 16) as u16,
                3 => status,
                4 => u16::from(self.active_linear),
                _ => 0,
            }
        } else {
            0
        };
        FillOutput {
            memory_request_valid: self.front_valid && self.allowed(input),
            memory_urgent: true,
            memory_address: ((self.tile_base
                + (u32::from(self.burst) << 8)
                + (u32::from(self.group & 7) << 1)
                + u32::from(self.second))
                & UNIT_MASK)
                << 4,
            line_write: self.burst_active
                && input.memory_data_valid
                && !input.memory_error
                && !self.memory_error
                && !input.boot_failed
                && !input.reset,
            line_write_address: u16::from(self.slot) * 200
                + u16::from(self.second) * 100
                + u16::from(self.burst) * 4
                + u16::from(self.beat),
            line_write_data: input.memory_read_data,
            published: self.published,
            group_valid: self.group_valid,
            group_linear: self.group_linear,
            device_read_data,
            underflow: input.pixel_underflow || self.memory_error,
            started_ack: self.started_sync,
            startup_error: self.memory_error || self.underflow_sync,
            frame_event: self.frame_sync != self.frame_seen,
        }
    }

    /// Commit exactly one logic rise from a pre-sampled pixel/MC/device snapshot.
    pub fn clock(&mut self, input: &FillInput) {
        let old = *self;
        let output = old.outputs(input);
        self.release_meta = input.released & 3;
        self.release_sync = old.release_meta;
        self.frame_meta = input.frame_toggle;
        self.frame_sync = old.frame_meta;
        self.underflow_meta = input.pixel_underflow;
        self.underflow_sync = old.underflow_meta;
        self.started_meta = input.pixel_started;
        self.started_sync = old.started_meta;
        if input.reset {
            // These two synchronizers have no RTL reset assignments.
            let release = (self.release_meta, self.release_sync);
            let started = (self.started_meta, self.started_sync);
            *self = Self::default();
            (self.release_meta, self.release_sync) = release;
            (self.started_meta, self.started_sync) = started;
            return;
        }
        if input.memory_error {
            self.memory_error = true;
            self.burst_active = false;
        }
        if output.frame_event {
            self.frame_seen = old.frame_sync;
            self.frame_index = old.frame_index.wrapping_add(1);
            if old.frame_complete {
                self.group = 0;
                self.frame_complete = false;
                if old.pending {
                    self.active_base = old.pending_base;
                    self.active_linear = old.pending_linear;
                    self.tile_base = old.pending_base;
                    self.front_valid = true;
                    self.pending = false;
                } else {
                    self.tile_base = old.active_base;
                }
            }
        }
        // Device writes follow frame handling. A same-edge new NEXT_SWAP wins
        // pending assignment, while this frame still applies the old descriptor.
        if input.device_write_enable && input.device_index & 7 == 3 {
            let data = input.device_write_data;
            match input.device_channel & 15 {
                1 => {
                    self.shadow_base = (old.shadow_base & !0xfff) | u32::from(data >> 4);
                    self.shadow_low_bad = data & 15 != 0;
                    self.low_written = true;
                    self.invalid = false;
                }
                2 => {
                    self.shadow_base = (old.shadow_base & 0xfff) | (u32::from(data & 63) << 12);
                    self.shadow_high_bad = data & !63 != 0;
                    self.high_written = true;
                    self.invalid = false;
                }
                4 => self.shadow_linear = data & 1 != 0,
                3 if data == 1 && old.low_written && old.high_written => {
                    self.low_written = false;
                    self.high_written = false;
                    if !old.shadow_high_bad && !old.shadow_low_bad && old.shadow_base <= LAST_BASE {
                        self.pending_base = old.shadow_base;
                        self.pending_linear = old.shadow_linear;
                        self.pending = true;
                        self.invalid = false;
                    } else {
                        self.invalid = true;
                    }
                }
                _ => {}
            }
        }
        if output.memory_request_valid && input.memory_request_ready {
            self.burst_active = true;
            self.beat = 0;
        }
        if old.burst_active && input.memory_data_valid && !old.memory_error {
            if input.memory_error || input.memory_last != (old.beat == 3) {
                self.memory_error = true;
                self.burst_active = false;
            } else if old.beat == 3 {
                self.burst_active = false;
                self.beat = 0;
                if !old.second {
                    self.second = true;
                } else {
                    self.second = false;
                    self.burst = if old.burst == LAST_BURST {
                        0
                    } else {
                        old.burst + 1
                    };
                }
            } else {
                self.beat = old.beat + 1;
            }
        }
        let black = !old.front_valid && old.allowed(input);
        let real = old.burst_active
            && input.memory_data_valid
            && !old.memory_error
            && !input.memory_error
            && input.memory_last
            && old.beat == 3
            && old.second
            && old.burst == LAST_BURST;
        if black || real {
            let bit = 1 << u8::from(old.slot);
            self.published = old.published ^ bit;
            self.group_linear = (old.group_linear & !bit) | if old.active_linear { bit } else { 0 };
            self.group_valid = (old.group_valid & !bit) | if old.front_valid { bit } else { 0 };
            self.slot = !old.slot;
            if old.group == LAST_GROUP {
                self.frame_complete = true;
            } else {
                self.group = old.group + 1;
                if old.group & 7 == 7 {
                    self.tile_base = (old.tile_base + 16) & UNIT_MASK;
                }
            }
        }
    }
}
