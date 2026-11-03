//! The production board descriptor boundary; payload/response stay with the GPU.
use gpu_v2::system::vliw_cycle::Request;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    pub request: Request,
    pub writing: bool,
}
impl Slot {
    pub fn ready(&self, reset: bool) -> bool {
        !reset && !self.request.valid
    }
    pub fn clock(&mut self, reset: bool, input: Request, writing: bool, downstream_ready: bool) {
        if reset {
            self.request.valid = false;
        } else if input.valid && self.ready(reset) {
            self.request = input;
            self.request.address_halfword &= 0x3f_ffff;
            self.request.count &= 3;
            self.writing = writing;
        } else if self.request.valid && downstream_ready {
            self.request.valid = false;
        }
    }
}
