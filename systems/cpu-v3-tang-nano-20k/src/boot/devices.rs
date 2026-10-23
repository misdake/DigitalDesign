//! Functional model of the retained system-control device.
use super::device_abi::{
    D_INVALIDATE_ALL, ICACHE_INVALIDATE_ALL_DELAYED, SYSCTL_LED, SYSCTL_MILLISECONDS, SYSCTL_UART,
    SYSCTL_WATCH_EXPECTED, SYSCTL_WATCH_TARGET,
};
use crate::{Device, Word};
use std::cell::Cell;
/// Device-0 model of the system-control block. The UART transmitter is never
/// busy; writes are captured for test assertions.
#[derive(Default)]
pub struct SystemControlDevice {
    pub led: Option<Word>,
    pub uart: Vec<u8>,
    pub icache_invalidations: u32,
    pub dcache_invalidations: u32,
    watch_device: u8,
    watch_channel: u8,
    watch_expected: u16,
    watch_active: bool,
    milliseconds: Cell<u16>,
}

impl SystemControlDevice {
    /// Host-scheduled functional time, independent of instructions and reads.
    /// This model has no cycle timing; the caller supplies elapsed wall time.
    pub fn advance_milliseconds(&self, elapsed: u16) {
        self.milliseconds
            .set(self.milliseconds.get().wrapping_add(elapsed));
    }

    pub fn active_watch(&self) -> Option<(u8, u8, u16)> {
        self.watch_active
            .then_some((self.watch_device, self.watch_channel, self.watch_expected))
    }

    pub fn observe_watch(&mut self, value: u16) {
        if self.watch_active && value != self.watch_expected {
            self.watch_active = false;
        }
    }
}

impl Device for SystemControlDevice {
    fn read(&mut self, _memory: &mut [Word], channel: u8) -> Word {
        if channel == SYSCTL_MILLISECONDS {
            self.milliseconds.get()
        } else {
            0
        }
    }

    fn write(&mut self, _memory: &mut [Word], channel: u8, value: Word) {
        match channel {
            ICACHE_INVALIDATE_ALL_DELAYED => self.icache_invalidations += 1,
            D_INVALIDATE_ALL => self.dcache_invalidations += 1,
            SYSCTL_LED => self.led = Some(value),
            SYSCTL_UART => self.uart.push(value as u8),
            SYSCTL_WATCH_TARGET => {
                self.watch_device = (value & 7) as u8;
                self.watch_channel = ((value >> 3) & 15) as u8;
            }
            SYSCTL_WATCH_EXPECTED if !self.watch_active => {
                self.watch_expected = value;
                self.watch_active = true;
            }
            _ => {}
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn cpu_hold_watch(&self) -> Option<(u8, u8, Word)> {
        self.active_watch()
    }

    fn observe_cpu_hold(&mut self, value: Word) {
        self.observe_watch(value);
    }
}
