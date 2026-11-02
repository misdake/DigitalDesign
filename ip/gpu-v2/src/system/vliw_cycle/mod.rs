//! Cycle models of the production command-profile transport, not a full GPU.
//!
//! `outputs` observes old state. `clock` commits one wall edge, independently
//! of compute CE. The caller owns all RAM, clocks and physical memory traffic.
//! Invalid peers return a contract violation at the monitored edge; they do
//! not acquire the checked standalone profile's recovery/rollback semantics.

pub mod command;
pub mod command_dma;
pub mod router;

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractError {
    pub module: &'static str,
    pub rule: &'static str,
}
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.module, self.rule)
    }
}
impl std::error::Error for ContractError {}

fn require(ok: bool, module: &'static str, rule: &'static str) -> Result<(), ContractError> {
    if ok {
        Ok(())
    } else {
        Err(ContractError { module, rule })
    }
}

/// Physical pins: response payload/LAST remain explicit even on invalid cycles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Response {
    pub valid: bool,
    pub last: bool,
    pub error: bool,
    pub data: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub valid: bool,
    pub address_halfword: u32,
    pub count: u8,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Physical {
    pub request_ready: bool,
    pub write_ready: bool,
    pub response: Response,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub stop: bool,
    pub cache: command_dma::Cache,
    pub work: command_dma::Work,
    pub texture: router::Texture,
    pub framebuffer: router::Framebuffer,
    /// RO, framebuffer read, framebuffer write; advanced by one external owner.
    pub physical: [Physical; 3],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub dma: command_dma::Output,
    pub router: router::Output,
}
#[derive(Clone, Debug, Default)]
pub struct Transport {
    pub dma: command_dma::Dma,
    pub router: router::Router,
}
impl Transport {
    fn connect(&self, i: Input) -> (command_dma::Input, router::Input) {
        let mut di = command_dma::Input {
            reset: i.reset,
            ce: i.ce,
            stop: i.stop,
            cache: i.cache,
            work: i.work,
            ..Default::default()
        };
        let ri = router::Input {
            reset: i.reset,
            dma: self.dma.outputs(di).request,
            texture: i.texture,
            framebuffer: i.framebuffer,
            physical: i.physical,
        };
        let ro = self.router.outputs(ri);
        di.request_ready = ro.dma_ready;
        di.response = ro.dma_response;
        di.response_bad = ro.dma_bad;
        (di, ri)
    }
    pub fn outputs(&self, i: Input) -> Output {
        let (di, ri) = self.connect(i);
        Output {
            dma: self.dma.outputs(di),
            router: self.router.outputs(ri),
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let (di, ri) = self.connect(i);
        let mut next = self.clone();
        next.dma.clock(di)?;
        next.router.clock(ri)?;
        *self = next;
        Ok(())
    }
}
