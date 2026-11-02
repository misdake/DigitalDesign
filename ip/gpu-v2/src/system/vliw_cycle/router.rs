//! Production CONTROL=0 command router: two RO owners and two FB descriptors.
use super::{require, ContractError, Physical, Request, Response};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Texture {
    pub valid: bool,
    pub address_bytes: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Framebuffer {
    pub valid: bool,
    pub writing: bool,
    pub address_bytes: u32,
    pub write_valid: bool,
    pub write_data: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub reset: bool,
    pub dma: Request,
    pub texture: Texture,
    pub framebuffer: Framebuffer,
    pub physical: [Physical; 3],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub dma_ready: bool,
    pub dma_response: Response,
    pub dma_bad: bool,
    pub texture_ready: bool,
    pub texture_read: bool,
    pub texture_index: u8,
    pub texture_data: u64,
    pub texture_complete: bool,
    pub texture_ok: bool,
    pub framebuffer_ready: bool,
    pub framebuffer_write_ready: bool,
    pub framebuffer_read: bool,
    pub framebuffer_data: u64,
    pub framebuffer_complete: bool,
    pub framebuffer_ok: bool,
    pub physical: [Request; 3],
    pub write_data: u64,
    pub busy: bool,
    pub fault: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Router {
    present: bool,
    offer_texture: bool,
    prefer_texture: bool,
    address: u32,
    count: u8,
    rp: u8,
    rh: u8,
    owners: [bool; 2],
    counts: [u8; 2],
    ro_beat: u8,
    fb_used: u8,
    fb_writing: bool,
    fb_beat: u8,
    fb_data_done: u8,
}
fn sector(a: u32) -> bool {
    a < 0x800000 && a & 127 == 0
}
fn burst(a: u32, c: u8) -> bool {
    match c & 3 {
        0 => a & 15 == 0,
        1 => a & 31 == 0,
        3 => a & 63 == 0,
        _ => false,
    }
}
impl Router {
    pub fn occupancy(&self) -> (u8, u8, u8) {
        (
            self.rp.wrapping_sub(self.rh) & 3,
            self.fb_used,
            self.fb_data_done,
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        let used = self.occupancy().0;
        let owner_texture = self.owners[(self.rh & 1) as usize];
        let ro = i.physical[0];
        let accept = self.present && used < 2 && ro.request_ready && !i.reset;
        let fb = i.framebuffer;
        let capacity = self.fb_used < 2 && (self.fb_used == 0 || fb.writing == self.fb_writing);
        let fr = i.physical[1];
        let fw = i.physical[2];
        let response = if self.fb_writing {
            fw.response
        } else {
            fr.response
        };
        let active = self.fb_used != 0;
        Output {
            dma_ready: accept && !self.offer_texture,
            dma_response: Response {
                valid: !i.reset && !owner_texture && used != 0 && ro.response.valid,
                last: ro.response.last,
                error: false,
                data: ro.response.data,
            },
            texture_ready: accept && self.offer_texture,
            texture_read: used != 0 && owner_texture && ro.response.valid && !i.reset,
            texture_index: self.ro_beat & 15,
            texture_data: ro.response.data,
            texture_complete: !i.reset
                && owner_texture
                && used != 0
                && ro.response.valid
                && ro.response.last,
            texture_ok: true,
            framebuffer_ready: capacity
                && !i.reset
                && if fb.writing {
                    fw.request_ready && fb.write_valid
                } else {
                    fr.request_ready
                },
            framebuffer_write_ready: active && self.fb_writing && fw.write_ready && !i.reset,
            framebuffer_read: active && !self.fb_writing && fr.response.valid && !i.reset,
            framebuffer_data: fr.response.data,
            framebuffer_complete: !i.reset && active && response.valid && response.last,
            framebuffer_ok: true,
            physical: [
                Request {
                    valid: self.present && used < 2 && !i.reset,
                    address_halfword: self.address,
                    count: self.count,
                },
                Request {
                    valid: fb.valid && capacity && !i.reset && !fb.writing,
                    address_halfword: (fb.address_bytes >> 1) & 0x3fffff,
                    count: 3,
                },
                Request {
                    valid: fb.valid && capacity && !i.reset && fb.writing && fb.write_valid,
                    address_halfword: (fb.address_bytes >> 1) & 0x3fffff,
                    count: 3,
                },
            ],
            write_data: fb.write_data,
            busy: self.present || used != 0 || active,
            ..Output::default()
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        if i.reset {
            *self = Self::default();
            return Ok(());
        }
        let o = self.outputs(i);
        let used = self.occupancy().0;
        let ro = i.physical[0].response;
        let fb = i.framebuffer;
        let fr = i.physical[1].response;
        let fw = i.physical[2].response;
        let fb_response = if self.fb_writing { fw } else { fr };
        require(!ro.valid || !ro.error, "router", "ro_response_error")?;
        require(!fr.valid || !fr.error, "router", "fb_r_response_error")?;
        require(!fw.valid || !fw.error, "router", "fb_w_response_error")?;
        require(!ro.valid || used != 0, "router", "unowned RO response")?;
        require(
            !fr.valid || self.fb_used != 0 && !self.fb_writing,
            "router",
            "unowned FB read",
        )?;
        require(
            !fw.valid || self.fb_used != 0 && self.fb_writing,
            "router",
            "unowned FB write",
        )?;
        if ro.valid {
            let beats = match self.counts[(self.rh & 1) as usize] {
                3 => 16,
                1 => 8,
                _ => 4,
            };
            require(
                self.ro_beat < beats && ro.last == (self.ro_beat + 1 == beats),
                "router",
                "ro_malformed",
            )?;
        }
        if self.fb_writing {
            require(
                !o.framebuffer_write_ready || fb.write_valid,
                "router",
                "write source hole",
            )?;
            require(
                !fb_response.valid
                    || fb_response.last
                        && (self.fb_data_done != 0
                            || o.framebuffer_write_ready && self.fb_beat == 15),
                "router",
                "write terminal before payload",
            )?;
        } else {
            require(
                !fb_response.valid || fb_response.last == (self.fb_beat == 15),
                "router",
                "read LAST framing",
            )?;
        }
        require(!fb.valid || sector(fb.address_bytes), "router", "fb_legal")?;
        require(
            self.fb_used <= 2 && used <= 2,
            "router",
            "descriptor capacity",
        )?;
        if self.present {
            require(
                if self.offer_texture {
                    i.texture.valid
                        && (i.texture.address_bytes >> 1) == self.address
                        && sector(i.texture.address_bytes)
                } else {
                    i.dma.valid
                        && (i.dma.address_halfword & 0x3fffff) == self.address
                        && (i.dma.count & 3) == self.count
                },
                "router",
                "presented request changed",
            )?;
        }
        let choose_texture = if self.prefer_texture {
            i.texture.valid
        } else {
            !i.dma.valid && i.texture.valid
        };
        if !self.present && used < 2 && (i.dma.valid || i.texture.valid) {
            require(
                if choose_texture {
                    sector(i.texture.address_bytes)
                } else {
                    burst(i.dma.address_halfword, i.dma.count)
                },
                "router",
                "RO request",
            )?;
        }
        let mut n = self.clone();
        if !self.present && used < 2 && (i.dma.valid || i.texture.valid) {
            n.present = true;
            n.offer_texture = choose_texture;
            n.address = if choose_texture {
                i.texture.address_bytes >> 1
            } else {
                i.dma.address_halfword & 0x3fffff
            };
            n.count = if choose_texture { 3 } else { i.dma.count & 3 };
        }
        if self.present && used < 2 && i.physical[0].request_ready {
            n.present = false;
            n.owners[(self.rp & 1) as usize] = self.offer_texture;
            n.counts[(self.rp & 1) as usize] = self.count;
            n.rp = (self.rp + 1) & 3;
            n.prefer_texture = !self.offer_texture;
        }
        if used != 0 && ro.valid {
            if ro.last {
                n.rh = (self.rh + 1) & 3;
                n.ro_beat = 0;
            } else {
                n.ro_beat = (self.ro_beat + 1).min(31);
            }
        }
        let accept = fb.valid && o.framebuffer_ready;
        let terminal = self.fb_used != 0 && fb_response.valid && fb_response.last;
        if accept && self.fb_used == 0 {
            n.fb_writing = fb.writing;
        }
        match (accept, terminal) {
            (true, false) => n.fb_used = (self.fb_used + 1) & 3,
            (false, true) => n.fb_used = self.fb_used.wrapping_sub(1) & 3,
            _ => {}
        }
        if o.framebuffer_write_ready || o.framebuffer_read {
            n.fb_beat = (self.fb_beat + 1) & 15;
        }
        match (
            o.framebuffer_write_ready && self.fb_beat == 15,
            terminal && self.fb_writing,
        ) {
            (true, false) => n.fb_data_done = (self.fb_data_done + 1) & 3,
            (false, true) => n.fb_data_done = self.fb_data_done.wrapping_sub(1) & 3,
            _ => {}
        }
        *self = n;
        Ok(())
    }
}
