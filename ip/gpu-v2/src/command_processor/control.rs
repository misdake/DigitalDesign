//! Stage-one semantic control ports. Not an encoded command ABI.
use super::{ports::Command, sim::oracle};

/// Driver-validated typed command, retained by the external payload owner until
/// command_done. Context validation is outside the synthesized control kernel.
#[derive(Clone, Debug)]
pub struct PreparedCommand {
    command: Command,
    control: Request,
}
impl PreparedCommand {
    pub fn payload(&self) -> &Command {
        &self.command
    }
    pub fn request(&self) -> Request {
        self.control
    }
    pub fn new(command: Command) -> Result<Self, String> {
        oracle::validate(std::slice::from_ref(&command), 1)?;
        let (kind, token) = match &command {
            Command::Dma(d) => (1, d.completion_token),
            Command::Wait(t) => (2, *t),
            Command::Draw { .. } => (4, 0),
            Command::Release { .. } => (8, 0),
            Command::Fence => (16, 0),
            Command::Unsupported(_) => unreachable!("oracle rejected unsupported"),
        };
        Ok(Self {
            command,
            control: Request {
                kinds: kind,
                token,
                validated: true,
            },
        })
    }
}
/// Owns the full checked typed payload outside the synthesized control probe.
/// A control capture never releases it. The matching action acceptance moves
/// it to the consumer; DRAW remains busy until a separate command_done.
#[derive(Debug, Default)]
pub struct CommandPort {
    held: Option<PreparedCommand>,
    captured: bool,
    draw_started: bool,
    faulted: bool,
}
impl CommandPort {
    pub fn present(&mut self, command: Command) -> Result<(), String> {
        if self.held.is_some() || self.captured || self.faulted {
            return Err("command port occupied or faulted".into());
        }
        let prepared = PreparedCommand::new(command)?;
        self.held = Some(prepared);
        Ok(())
    }
    pub fn request(&self) -> Option<Request> {
        if self.captured || self.faulted {
            None
        } else {
            self.held.as_ref().map(PreparedCommand::request)
        }
    }
    pub fn payload(&self) -> Option<&Command> {
        self.held.as_ref().map(PreparedCommand::payload)
    }
    /// Invoke once with the current real kernel edge. Returns an owned original
    /// typed command only when its corresponding downstream action accepts.
    pub fn edge(&mut self, o: Outputs) -> Result<Option<Command>, String> {
        if o.fault {
            self.faulted = true;
            return Err("kernel fault; retain external ownership until reset/drain".into());
        }
        if o.command_capture && (self.captured || self.held.is_none()) {
            return Err("foreign or duplicate command capture".into());
        }
        let accepted =
            o.dma_accept || o.wait_ack || o.draw_accept || o.release_accept || o.fence_ack;
        if accepted {
            if !self.captured {
                return Err("action before old command capture".into());
            }
            let Some(p) = &self.held else {
                return Err("action has no retained payload".into());
            };
            let matches = match &p.command {
                Command::Dma(d) => o.dma_accept && o.dma_token == d.completion_token,
                Command::Wait(_) => o.wait_ack,
                Command::Draw { .. } => o.draw_accept,
                Command::Release { .. } => o.release_accept,
                Command::Fence => o.fence_ack,
                Command::Unsupported(_) => false,
            };
            if !matches {
                return Err("action/payload identity mismatch".into());
            }
        }
        if o.command_done && (!self.captured || !accepted && !self.draw_started) {
            return Err("completion before old owner/action".into());
        }
        if o.command_capture {
            self.captured = true;
        }
        let transferred = if accepted {
            self.held.take().map(|p| p.command)
        } else {
            None
        };
        if o.draw_accept {
            self.draw_started = true;
        }
        if o.command_done {
            self.captured = false;
            self.draw_started = false;
        }
        Ok(transferred)
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Request {
    /// Semantic booleans in DMA/WAIT/DRAW/RELEASE/FENCE/unsupported order.
    pub kinds: u8,
    pub token: u8,
    pub validated: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub reset: bool,
    pub ce: bool,
    pub command: Option<Request>,
    pub event_set: u8,
    /// Explicit handler acknowledgment; ignored while core CE is zero.
    pub event_ack: u8,
    pub dma_done: u8,
    pub dma_ready: bool,
    pub draw_ready: bool,
    pub draw_done: bool,
    pub release_ready: bool,
    pub fence_ready: bool,
    /// Downstream rejection/fault, including stale lease or descriptor ownership.
    pub external_fault: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Dma,
    Wait,
    DrawRequest,
    DrawRun,
    Release,
    Fence,
    Fault,
}
impl Phase {
    pub fn code(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Dma => 1,
            Self::Wait => 2,
            Self::DrawRequest => 3,
            Self::DrawRun => 4,
            Self::Release => 5,
            Self::Fence => 6,
            Self::Fault => 7,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub phase: Phase,
    pub token: u8,
    pub tokens: u8,
    pub reserved: u8,
    pub pending: u8,
    pub cursor: u8,
    pub draw_return: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outputs {
    pub command_capture: bool,
    pub command_done: bool,
    pub handler: Option<u8>,
    pub dma_valid: bool,
    pub dma_accept: bool,
    pub dma_token: u8,
    pub wait_ack: bool,
    pub draw_valid: bool,
    pub draw_accept: bool,
    pub release_valid: bool,
    pub release_accept: bool,
    pub fence_ack: bool,
    pub fault: bool,
}
