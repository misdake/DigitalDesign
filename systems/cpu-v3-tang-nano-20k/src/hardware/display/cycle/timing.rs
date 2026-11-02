//! Pixel-domain timing/ownership cone. Color lookup and TMDS are separate datapaths.

#[derive(Clone, Copy, Debug, Default)]
pub struct TimingInput {
    pub reset: bool,
    pub video_locked: bool,
    pub boot_enable: bool,
    pub boot_failed: bool,
    pub memory_error: bool,
    pub published: u8,
    pub group_valid: u8,
    pub group_linear: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingOutput {
    pub h_count: u16,
    pub v_count: u16,
    pub hsync: bool,
    pub vsync: bool,
    pub active: bool,
    pub released: u8,
    pub frame_toggle: bool,
    pub started: bool,
    pub underflow: bool,
    pub slot: bool,
    pub second: bool,
    pub vertical_repeat: bool,
    pub scan_valid: bool,
    pub scan_line_ready: bool,
    pub scan_linear: bool,
    pub pixel_reset: bool,
    /// Diagnostic crossing registers, sampled before this instant's commit.
    pub publish_sync: u8,
    pub enable_sync: u8,
    pub failure_sync: u8,
    pub reset_sync: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    publish_meta: u8,
    publish_sync: u8,
    enable_sync: u8,
    failure_sync: u8,
    reset_sync: u8,
    h: u16,
    v: u16,
    released: u8,
    frame_toggle: bool,
    started: bool,
    underflow: bool,
    slot: bool,
    second: bool,
    repeat: bool,
    valid: bool,
    ready: bool,
    linear: bool,
}
impl Timing {
    pub fn outputs(&self) -> TimingOutput {
        let c = crate::display::ACTIVE_DISPLAY_CONFIG;
        TimingOutput {
            h_count: self.h,
            v_count: self.v,
            hsync: u32::from(self.h) < c.h_sync_end,
            vsync: u32::from(self.v) < c.v_sync_end,
            active: u32::from(self.h) >= c.h_active_start
                && u32::from(self.h) < c.h_active_end
                && u32::from(self.v) >= c.v_active_start
                && u32::from(self.v) < c.v_active_end,
            released: self.released,
            frame_toggle: self.frame_toggle,
            started: self.started,
            underflow: self.underflow,
            slot: self.slot,
            second: self.second,
            vertical_repeat: self.repeat,
            scan_valid: self.valid,
            scan_line_ready: self.ready,
            scan_linear: self.linear,
            pixel_reset: self.reset_sync & 4 == 0,
            publish_sync: self.publish_sync,
            enable_sync: self.enable_sync,
            failure_sync: self.failure_sync,
            reset_sync: self.reset_sync,
        }
    }
    /// Commit one pixel rise. Every decision uses old registers, including the
    /// old reset synchronizer; the independent crossing chains always advance.
    pub fn clock(&mut self, input: &TimingInput) {
        let old = *self;
        let c = crate::display::ACTIVE_DISPLAY_CONFIG;
        self.publish_meta = input.published & 3;
        self.publish_sync = old.publish_meta;
        self.enable_sync = ((old.enable_sync << 1) | u8::from(input.boot_enable)) & 3;
        self.failure_sync =
            ((old.failure_sync << 1) | u8::from(input.boot_failed || input.memory_error)) & 3;
        self.reset_sync =
            ((old.reset_sync << 1) | u8::from(!input.reset && input.video_locked)) & 7;
        if old.reset_sync & 4 == 0 {
            self.h = 0;
            self.v = 0;
            self.released = 0;
            self.slot = false;
            self.second = false;
            self.frame_toggle = false;
            self.repeat = false;
            self.started = false;
            self.underflow = false;
            self.ready = false;
            self.linear = false;
            self.valid = false;
            return;
        }
        let enabled = old.started && old.failure_sync & 2 == 0;
        let bit = 1 << u8::from(old.slot);
        let available = (old.publish_sync ^ old.released) & 3;
        if u32::from(old.h) == c.h_total - 1 {
            self.h = 0;
            if enabled && u32::from(old.v) == c.v_active_end - 1 {
                self.frame_toggle = !old.frame_toggle;
            }
            if u32::from(old.v) == c.v_total - 1 {
                self.v = 0;
                if !old.started
                    && old.enable_sync & 2 != 0
                    && old.failure_sync & 2 == 0
                    && available == 3
                {
                    self.started = true;
                }
                if !old.valid
                    && old.enable_sync & 2 != 0
                    && old.failure_sync & 2 == 0
                    && input.group_valid & 3 == 3
                    && available == 3
                {
                    self.valid = true;
                }
            } else {
                self.v = old.v + 1;
            }
        } else {
            self.h = old.h + 1;
        }
        if old.h == 0 {
            self.ready = enabled && available & bit != 0;
            self.linear = input.group_linear & bit != 0;
        }
        if enabled
            && (old.valid || input.group_valid & bit == 0)
            && u32::from(old.h) == c.h_active_end - 1
            && u32::from(old.v) >= c.v_active_start
            && u32::from(old.v) < c.v_active_end
        {
            if old.repeat {
                self.repeat = false;
                if old.ready {
                    self.second = !old.second;
                    if old.second {
                        self.released = old.released ^ bit;
                        self.slot = !old.slot;
                    }
                } else {
                    self.underflow = true;
                }
            } else {
                self.repeat = true;
            }
        }
    }
}
