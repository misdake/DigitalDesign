//! Explicit host elapsed/input events. No CPU instructions become clock cycles.
use crate::{input_device::ControllerSnapshot, system_sim::CpuV3SystemSim};
use std::time::{Duration, Instant};

mod xinput;
pub use xinput::{map_xinput, XInputError, XInputSample, XInputSource};

pub trait ControllerSource {
    fn poll(&mut self) -> Option<ControllerSnapshot>;
}

/// Input publication is shared; each CPU backend keeps its own time source.
pub trait InputSnapshotTarget {
    fn publish_snapshot(&mut self, snapshot: Option<ControllerSnapshot>);
}
impl InputSnapshotTarget for CpuV3SystemSim {
    fn publish_snapshot(&mut self, snapshot: Option<ControllerSnapshot>) {
        self.publish_input(snapshot);
    }
}
impl InputSnapshotTarget for crate::system_emu::CpuV3SystemEmu {
    fn publish_snapshot(&mut self, snapshot: Option<ControllerSnapshot>) {
        self.publish_input(snapshot);
    }
}

#[derive(Default)]
pub struct HostIo {
    source: Option<Box<dyn ControllerSource>>,
    poll_after: Duration,
    focused: bool,
}

impl HostIo {
    pub fn with_input(source: impl ControllerSource + 'static) -> Self {
        Self {
            source: Some(Box::new(source)),
            ..Self::default()
        }
    }

    pub fn with_xinput(slot: u32) -> Result<Self, XInputError> {
        Ok(Self::with_input(XInputSource::new(slot)?))
    }

    /// Time passes even with no CPU work, no input source or no Display wait.
    pub fn advance(&mut self, system: &mut CpuV3SystemSim, elapsed: Duration, focused: bool) {
        system.advance_host_time(elapsed);
        self.advance_input(system, elapsed, focused);
    }
    /// Poll input without injecting host elapsed time into clocked device timers.
    pub fn advance_input(
        &mut self,
        system: &mut impl InputSnapshotTarget,
        elapsed: Duration,
        focused: bool,
    ) {
        let Some(source) = self.source.as_mut() else {
            return;
        };
        if !focused {
            system.publish_snapshot(None);
            self.focused = false;
            self.poll_after = Duration::ZERO;
            return;
        }
        if !self.focused {
            self.poll_after = Duration::ZERO;
        }
        self.focused = true;
        if self.poll_after > elapsed {
            self.poll_after -= elapsed;
            return;
        }
        let snapshot = source.poll();
        self.poll_after = if snapshot.is_some() {
            Duration::from_millis(16)
        } else {
            Duration::from_secs(1)
        };
        system.publish_snapshot(snapshot);
    }
}

pub struct RealtimeHostIo {
    io: HostIo,
    last: Instant,
}

impl RealtimeHostIo {
    pub fn new(io: HostIo) -> Self {
        Self {
            io,
            last: Instant::now(),
        }
    }

    pub fn pump(&mut self, system: &mut CpuV3SystemSim, focused: bool) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last);
        self.last = now;
        self.io.advance(system, elapsed, focused);
    }

    pub fn pump_input(&mut self, system: &mut impl InputSnapshotTarget, focused: bool) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last);
        self.last = now;
        self.io.advance_input(system, elapsed, focused);
    }

    /// A machine reset establishes a new host-clock origin and input attempt.
    pub fn reset(&mut self) {
        self.last = Instant::now();
        self.io.poll_after = Duration::ZERO;
        self.io.focused = false;
    }
}
