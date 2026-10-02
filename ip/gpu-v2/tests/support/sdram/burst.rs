//! Exact low-level combination projection, with no host Service queue/latency shim.
use digital_design_hardware_gowin::sdram_memory_controller::{
    emu::{idle_inputs, Combination},
    ports::OracleImage,
};
use gpu_v2::memory::ports::{MemoryPort, Request, Response, BURST_BEATS, BURST_BYTES};

struct Active {
    request: Request,
    writes: usize,
    reads: usize,
}

pub struct Adapter {
    pub combination: Combination,
    active: Option<Active>,
    held_request: Option<Request>,
    held_write: Option<u64>,
    max_cycles: u64,
}
impl Adapter {
    pub fn new(image: OracleImage, max_cycles: u64) -> Result<Self, String> {
        if max_cycles == 0 {
            return Err("GPU burst adapter cycle bound".into());
        }
        Ok(Self {
            combination: Combination::new(image, 32)?,
            active: None,
            held_request: None,
            held_write: None,
            max_cycles,
        })
    }
    pub fn idle(&self) -> bool {
        self.active.is_none() && self.held_request.is_none()
    }
}
impl MemoryPort for Adapter {
    fn cycle(&mut self, request: Option<Request>, write: Option<u64>) -> Result<Response, String> {
        if self.combination.cycle >= self.max_cycles {
            return Err("GPU burst adapter watchdog".into());
        }
        if self.held_request.is_some() && self.held_request != request {
            return Err("blocked GPU request changed".into());
        }
        if self.held_write.is_some() && self.held_write != write {
            return Err("blocked GPU write beat changed".into());
        }
        if self.active.is_some() && request.is_some() {
            return Err("GPU port has an outstanding transaction".into());
        }
        if let Some(r) = request {
            r.validate()?;
            if !self
                .combination
                .bridge
                .pins
                .contains(r.address_bytes, BURST_BYTES)
            {
                return Err("GPU burst outside SDRAM image".into());
            }
        }
        let mut input = idle_inputs();
        let writing = self
            .active
            .as_ref()
            .map(|a| a.request.write)
            .or(request.map(|r| r.write))
            .unwrap_or(false);
        // First beat is required before request admission; descriptor ready is
        // not exposed until its continuous write source has been reserved.
        if let Some(r) = request.filter(|r| !r.write || write.is_some()) {
            if r.write {
                input.gpu_fb_w_request_valid = true;
                input.gpu_fb_w_write = true;
                input.gpu_fb_w_address = r.address_bytes / 2;
                input.gpu_fb_w_line_count_minus_1 = 3;
            } else {
                input.gpu_fb_r_request_valid = true;
                input.gpu_fb_r_address = r.address_bytes / 2;
                input.gpu_fb_r_line_count_minus_1 = 3;
            }
        }
        input.gpu_fb_w_write_data = write.unwrap_or(0);
        let o = self.combination.output(&input);
        let accepted = if writing {
            input.gpu_fb_w_request_valid && o.gpu_fb_w_request_ready
        } else {
            input.gpu_fb_r_request_valid && o.gpu_fb_r_request_ready
        };
        let mut response = Response {
            accepted,
            ..Response::default()
        };
        if accepted {
            self.active = Some(Active {
                request: request.unwrap(),
                writes: 0,
                reads: 0,
            });
        }
        self.held_request = request.filter(|_| !accepted);
        if let Some(active) = &mut self.active {
            let terminal = if active.request.write {
                if o.gpu_fb_w_write_data_ready {
                    if write.is_none() || active.writes >= BURST_BEATS {
                        return Err("accepted GPU write source underrun/overrun".into());
                    }
                    response.write_accepted = true;
                    active.writes += 1;
                }
                o.gpu_fb_w_response_valid
            } else {
                if o.gpu_fb_r_response_valid && !o.gpu_fb_r_error {
                    if active.reads >= BURST_BEATS {
                        return Err("GPU burst read overrun".into());
                    }
                    response.read = Some((active.reads as u8, o.gpu_fb_r_read_data));
                    active.reads += 1;
                }
                o.gpu_fb_r_response_valid && (o.gpu_fb_r_response_last || o.gpu_fb_r_error)
            };
            if terminal {
                let error = if active.request.write {
                    o.gpu_fb_w_error
                } else {
                    o.gpu_fb_r_error
                };
                if !error
                    && (if active.request.write {
                        active.writes
                    } else {
                        active.reads
                    }) != BURST_BEATS
                {
                    return Err("GPU burst completion before all data".into());
                }
                response.complete = Some(!error);
            }
        }
        self.held_write = write.filter(|_| {
            writing
                && !response.write_accepted
                && response.complete.is_none()
                && self.active.as_ref().is_none_or(|a| a.writes < BURST_BEATS)
        });
        if response.complete.is_some() {
            self.active = None;
            self.held_write = None;
        }
        self.combination.tick(&input)?;
        Ok(response)
    }
}
