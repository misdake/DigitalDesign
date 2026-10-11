//! Independent persistent state machine for the isolated command control kernel.
use super::control::*;

#[derive(Clone, Debug, Default)]
pub struct CommandEmu {
    pub state: State,
}
impl CommandEmu {
    fn invalid(&self, i: Inputs) -> bool {
        if i.external_fault
            || i.dma_done & !self.state.reserved & 15 != 0
            || i.draw_done && self.state.phase != Phase::DrawRun
        {
            return true;
        }
        if i.ce && self.state.phase == Phase::Idle {
            if let Some(r) = i.command {
                return !r.validated
                    || r.kinds.count_ones() != 1
                    || r.kinds & !31 != 0
                    || r.kinds & 3 != 0
                        && (r.token > 3
                            || r.kinds == 1 && self.state.reserved & (1 << r.token) != 0);
            }
        }
        false
    }
    pub fn outputs(&self, i: Inputs) -> Outputs {
        if i.reset {
            return Outputs::default();
        }
        let s = self.state;
        let bad = self.invalid(i);
        let fault = s.phase == Phase::Fault || bad;
        let mut set = i.event_set | (i.dma_done & s.reserved & 15);
        if s.phase == Phase::Idle && i.command.is_some() {
            set |= 16;
        }
        if fault {
            set |= 128;
        }
        let available = s.pending | set;
        let boundary = matches!(s.phase, Phase::Idle | Phase::Wait | Phase::Fault);
        let handler = if i.ce && boundary {
            (0..8)
                .map(|n| (s.cursor + n) % 8)
                .find(|id| *id != 5 && *id != 6 && available & (1 << id) != 0)
        } else {
            None
        };
        let enabled = i.ce && !fault;
        let capture =
            enabled && s.phase == Phase::Idle && handler == Some(4) && i.command.is_some();
        let dma_valid = enabled && s.phase == Phase::Dma;
        let draw_valid = enabled && s.phase == Phase::DrawRequest;
        let release_valid = enabled && s.phase == Phase::Release;
        let wait_ack =
            enabled && s.phase == Phase::Wait && (s.tokens | i.dma_done) & (1 << s.token) != 0;
        let dma_accept = dma_valid && i.dma_ready;
        let draw_accept = draw_valid && i.draw_ready;
        let release_accept = release_valid && i.release_ready;
        let fence_ack = enabled && s.phase == Phase::Fence && i.fence_ready;
        let command_done = dma_accept
            || wait_ack
            || release_accept
            || fence_ack
            || enabled && s.phase == Phase::DrawRun && s.draw_return;
        Outputs {
            command_capture: capture,
            command_done,
            handler,
            dma_valid,
            dma_accept,
            dma_token: s.token,
            wait_ack,
            draw_valid,
            draw_accept,
            release_valid,
            release_accept,
            fence_ack,
            fault,
        }
    }
    pub fn step(&mut self, i: Inputs) -> Outputs {
        let out = self.outputs(i);
        if i.reset {
            self.state = State::default();
            return out;
        }
        let old = self.state;
        let accepted_done = i.dma_done & old.reserved & 15;
        let mut set = i.event_set | accepted_done;
        if old.phase == Phase::Idle && i.command.is_some() {
            set |= 16;
        }
        if out.fault {
            set |= 128;
        }
        let mut ack = if i.ce { i.event_ack } else { 0 };
        if out.command_done {
            ack |= 16;
        }
        if out.wait_ack {
            ack |= 1 << old.token;
        }
        self.state.pending = (old.pending & !ack) | set;
        if let Some(id) = out.handler {
            self.state.cursor = (id + 1) % 8;
        }
        self.state.tokens = old.tokens | accepted_done;
        if old.phase == Phase::DrawRun && i.draw_done {
            self.state.draw_return = true;
        }
        if out.wait_ack {
            self.state.tokens &= !(1 << old.token);
            self.state.reserved &= !(1 << old.token);
        }
        if out.dma_accept {
            self.state.reserved |= 1 << old.token;
        }
        if out.fault {
            self.state.phase = Phase::Fault;
        } else if out.command_capture {
            let r = i.command.expect("capture has request");
            self.state.token = r.token & 3;
            self.state.phase = match r.kinds {
                1 => Phase::Dma,
                2 => Phase::Wait,
                4 => Phase::DrawRequest,
                8 => Phase::Release,
                16 => Phase::Fence,
                _ => unreachable!("guarded request"),
            };
        } else if out.command_done {
            self.state.phase = Phase::Idle;
            self.state.draw_return = false;
        } else if out.draw_accept {
            self.state.phase = Phase::DrawRun;
            self.state.draw_return = false;
        }
        out
    }
}
