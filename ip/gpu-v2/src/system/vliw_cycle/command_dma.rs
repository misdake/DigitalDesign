//! Two production logical jobs, held offer and two ordered return descriptors.
use super::{require, ContractError, Request, Response};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cache {
    pub valid: bool,
    /// Sector-aligned halfword source; bit0 selects the table's RAM rows480..495.
    pub source_halfword: u32,
    pub bank: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub valid: bool,
    pub mesh: bool,
    pub source_halfword: u32,
    pub destination: u8,
    pub size: u8,
    pub blocks: u8,
    pub slot: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub stop: bool,
    pub cache: Cache,
    pub work: Work,
    pub request_ready: bool,
    pub response: Response,
    pub response_bad: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub cache_ready: bool,
    pub cache_done: bool,
    pub cache_done_bank: bool,
    pub work_ready: bool,
    pub work_busy: bool,
    pub work_done: bool,
    pub mesh_begin: bool,
    pub mesh_done: bool,
    pub mesh_slot: bool,
    pub mesh_begin_slot: bool,
    pub mesh_done_slot: bool,
    pub uniform_done: bool,
    pub uniform_bank: u8,
    pub ram_write: bool,
    pub ram_address: u16,
    pub ram_data: u64,
    pub request: Request,
    pub idle: bool,
    pub mesh_loading: bool,
    pub fault: bool,
}
#[derive(Clone, Copy, Debug, Default)]
struct Job {
    source: u32,
    destination: u8,
    size: u8,
    left: u8,
    mesh: bool,
    page: bool,
    header: bool,
}
#[derive(Clone, Copy, Debug, Default)]
struct Offer {
    valid: bool,
    cache: bool,
    last: bool,
    source: u32,
    destination: u16,
    count: u8,
}
#[derive(Clone, Debug, Default)]
pub struct Dma {
    jobs: [Job; 2],
    put: u8,
    issued: u8,
    retired: u8,
    offer: Offer,
    cache_return: [bool; 2],
    last_return: [bool; 2],
    rp: u8,
    rh: u8,
    cursor: u16,
    next_destination: u8,
    pulses: Output,
}
impl Dma {
    pub(crate) fn observe(&self, i: Input, trace: &mut super::trace::Trace) {
        if i.reset {
            return;
        }
        let o = self.outputs(i);
        let returns = self.occupancy().1;
        if i.response.valid && returns != 0 && i.response.last {
            trace.event("dma_return", [u64::from(self.rh & 1)]);
            if !i.stop
                && !self.cache_return[usize::from(self.rh & 1)]
                && self.last_return[usize::from(self.rh & 1)]
            {
                trace.event("dma_done", [u64::from(self.retired & 1)]);
            }
        }
        if i.ce && i.work.valid && o.work_ready {
            let w = i.work;
            trace.event(
                "dma_job",
                [
                    u64::from(self.put & 1),
                    u64::from(w.mesh),
                    u64::from(w.slot),
                    u64::from(w.source_halfword),
                    u64::from(w.size),
                    u64::from(w.blocks),
                ],
            );
        }
        if self.offer.valid && i.request_ready {
            trace.event(
                "dma_request",
                [
                    u64::from(self.rp & 1),
                    if self.offer.cache {
                        2
                    } else {
                        u64::from(self.issued & 1)
                    },
                    u64::from(o.request.address_halfword),
                    u64::from(o.request.count),
                ],
            );
        }
    }
    pub fn occupancy(&self) -> (u8, u8) {
        (
            self.put.wrapping_sub(self.retired) & 3,
            self.rp.wrapping_sub(self.rh) & 3,
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        let (jobs, returns) = self.occupancy();
        let head = self.jobs[(self.retired & 1) as usize];
        let other = self.jobs[((self.retired & 1) ^ 1) as usize];
        Output {
            cache_ready: !self.offer.valid && returns < 2 && !i.stop && i.cache.valid,
            work_ready: jobs < 2 && !i.stop,
            work_busy: jobs != 0,
            mesh_slot: head.page,
            mesh_loading: jobs != 0
                && head.mesh
                && (jobs == 1 || other.mesh && other.page == head.page),
            ram_write: returns != 0 && i.response.valid && !i.stop,
            ram_address: self.cursor,
            ram_data: i.response.data,
            request: Request {
                valid: self.offer.valid && !i.reset,
                address_halfword: self.offer.source << 4,
                count: self.offer.count,
            },
            idle: !self.offer.valid && returns == 0 && jobs == 0,
            ..self.pulses
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let o = self.outputs(i);
        let (jobs, returns) = self.occupancy();
        let take = self.offer.valid && i.request_ready;
        let accept = i.ce && i.work.valid && o.work_ready;
        if !i.reset {
            require(jobs <= 2 && returns <= 2, "command_dma", "queue capacity")?;
            require(
                !take || self.offer.destination & 3 == 0,
                "command_dma",
                "destination alignment",
            )?;
            if accept {
                let w = i.work;
                let size = w.size & 3;
                require(
                    w.source_halfword & 15 == 0
                        && (w.mesh
                            || size != 3
                                && (size != 1 || w.source_halfword & 16 == 0)
                                && (size != 2 || w.source_halfword & 48 == 0)),
                    "command_dma",
                    "work source alignment",
                )?;
            }
            require(
                !o.cache_ready || i.cache.source_halfword & 62 == 0,
                "command_dma",
                "cache source alignment",
            )?;
            require(
                returns == 0 || !i.response.valid || !i.response.error && !i.response_bad,
                "command_dma",
                "response_error or response_bad",
            )?;
        }
        let mut n = self.clone();
        if i.reset {
            n.put = 0;
            n.issued = 0;
            n.retired = 0;
            n.rp = 0;
            n.rh = 0;
            n.offer.valid = false;
            n.cursor = 0;
            n.pulses = Output::default();
            for j in &mut n.jobs {
                j.source = 0;
                j.destination = 0;
                j.mesh = false;
                j.page = false;
            }
        } else {
            n.pulses.cache_done = false;
            n.pulses.work_done = false;
            n.pulses.mesh_begin = false;
            n.pulses.mesh_done = false;
            n.pulses.uniform_done = false;
            if accept {
                let w = i.work;
                n.jobs[(self.put & 1) as usize] = Job {
                    source: (w.source_halfword >> 4) & 0x3ffff,
                    destination: if w.mesh {
                        64 | u8::from(w.slot)
                    } else {
                        w.destination & 127
                    },
                    size: w.size & 3,
                    left: (w.blocks & 7) + 1,
                    mesh: w.mesh,
                    page: w.slot,
                    header: w.mesh,
                };
                n.put = (self.put + 1) & 3;
                if w.mesh {
                    n.pulses.mesh_begin = true;
                    n.pulses.mesh_begin_slot = w.slot;
                }
            }
            if !self.offer.valid && returns < 2 && !i.stop {
                if i.cache.valid {
                    n.offer = Offer {
                        valid: true,
                        cache: true,
                        last: false,
                        source: (i.cache.source_halfword >> 4) & 0x3ffff,
                        destination: if i.cache.source_halfword & 1 != 0 {
                            480
                        } else {
                            448 | (u16::from(i.cache.bank) << 4)
                        },
                        count: 3,
                    };
                } else if self.issued != self.put {
                    let j = self.jobs[(self.issued & 1) as usize];
                    n.offer = Offer {
                        valid: true,
                        cache: false,
                        last: !j.mesh || !j.header && j.left == 1,
                        source: j.source,
                        destination: u16::from(j.destination) << 2,
                        count: if j.mesh {
                            if j.header {
                                0
                            } else {
                                3
                            }
                        } else {
                            (j.size & 2) | u8::from(j.size != 0)
                        },
                    };
                }
            }
            if take {
                n.offer.valid = false;
                if returns != 0 {
                    n.next_destination = (self.offer.destination >> 2) as u8;
                }
                n.cache_return[(self.rp & 1) as usize] = self.offer.cache;
                n.last_return[(self.rp & 1) as usize] = self.offer.last;
                n.rp = (self.rp + 1) & 3;
                if !self.offer.cache {
                    let at = (self.issued & 1) as usize;
                    let j = self.jobs[at];
                    if j.mesh {
                        n.jobs[at].source = (j.source + 4) & 0x3ffff;
                        if j.header {
                            n.jobs[at].header = false;
                            n.jobs[at].destination = u8::from(j.page) << 5;
                        } else {
                            n.jobs[at].destination = (j.destination + 4) & 127;
                            n.jobs[at].left = j.left.wrapping_sub(1) & 15;
                        }
                    }
                    if self.offer.last {
                        n.issued = (self.issued + 1) & 3;
                    }
                }
            }
            if o.ram_write {
                n.cursor = (self.cursor + 1) & 511;
            }
            if take && returns == 0 {
                n.cursor = self.offer.destination;
            }
            if i.response.valid && returns != 0 && i.response.last {
                n.rh = (self.rh + 1) & 3;
                if returns > 1 {
                    n.cursor = u16::from(self.next_destination) << 2;
                } else if take {
                    n.cursor = self.offer.destination;
                }
                if !i.stop {
                    if self.cache_return[(self.rh & 1) as usize] {
                        n.pulses.cache_done = true;
                        n.pulses.cache_done_bank = self.cursor & 16 != 0;
                    } else if self.last_return[(self.rh & 1) as usize] {
                        n.pulses.work_done = true;
                        n.retired = (self.retired + 1) & 3;
                        let j = self.jobs[(self.retired & 1) as usize];
                        if j.mesh {
                            n.pulses.mesh_done = true;
                            n.pulses.mesh_done_slot = j.page;
                        } else if j.destination >> 4 == 5 && j.destination & 3 == 0 {
                            n.pulses.uniform_done = true;
                            n.pulses.uniform_bank = (j.destination >> 2) & 3;
                        }
                    }
                }
            }
            if i.stop && !self.offer.valid && returns == 0 {
                n.retired = self.put;
                n.issued = self.put;
            }
        }
        *self = n;
        Ok(())
    }
}
