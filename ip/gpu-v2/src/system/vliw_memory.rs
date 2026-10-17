//! Independent three-master GPU memory boundary. No compute enable or payload FIFO.
//! Addresses at the backend are bytes; the board arbiter consumes 22-bit halfwords.
//! Callers reserve complete read sinks and a continuous write source before admission.

use crate::frontend::vliw_dma::{self as dma, Loader};

pub const MODULE: &str = "gpu_vliw_memory";
pub const RTL: &str = include_str!("vliw_memory.v");
pub fn verilog() -> String {
    format!("{}\n{RTL}", dma::RTL)
}

pub const BACKEND_MODULE: &str = "gpu_v2_vliw_backend_three_port";
/// Replace the selected backend's complete texture/FB transport, rather than
/// adding another arbiter after it. Input is a generated, dependency-closed
/// backend source. Exact anchors fail closed when that source contract changes.
/// The stage2 source/factory is never edited by this function.
pub fn split_backend(source: &str) -> Result<String, String> {
    let normalized = source.replace("\r\n", "\n");
    let marker = "module gpu_v2_vliw_backend(";
    if normalized.matches(marker).count() != 1 {
        return Err("selected VLIW backend declaration changed".into());
    }
    let a = normalized.find(marker).unwrap();
    let b = a
        + normalized[a..]
            .find("endmodule")
            .ok_or("backend endmodule")?
        + 9;
    let mut body = normalized[a..b].to_owned();
    fn once(s: &mut String, old: &str, new: &str) -> Result<(), String> {
        if s.matches(old).count() != 1 {
            return Err(format!("memory split anchor changed: {old:?}"));
        }
        *s = s.replacen(old, new, 1);
        Ok(())
    }
    once(&mut body, marker, &format!("module {BACKEND_MODULE}("))?;
    once(&mut body," output wire mem_req_valid,mem_req_write,output wire[31:0]mem_req_addr,\n input wire mem_req_ready,input wire[63:0]mem_read_data,input wire mem_read_valid,\n output wire mem_write_valid,output wire[63:0]mem_write_data,\n input wire mem_write_ready,mem_complete_valid,mem_complete_ok",
        " output wire texture_request_valid,output wire[31:0]texture_request_address,\n input wire texture_request_ready,texture_read_valid,input wire[3:0]texture_read_index,\n input wire[63:0]texture_read_data,input wire texture_complete,texture_ok,\n output wire framebuffer_request_valid,framebuffer_request_write,output wire[31:0]framebuffer_request_address,\n input wire framebuffer_request_ready,framebuffer_read_valid,input wire[63:0]framebuffer_read_data,\n output wire framebuffer_write_valid,output wire[63:0]framebuffer_write_data,\n input wire framebuffer_write_ready,framebuffer_complete,framebuffer_ok")?;
    once(
        &mut body,
        " reg[1:0]arb_state;reg owner;reg[3:0]beat;\n",
        "",
    )?;
    once(
        &mut body,
        ".mem_resp_data(mem_read_data)",
        ".mem_resp_data(texture_read_data)",
    )?;
    once(
        &mut body,
        ".mem_resp_ok(mem_complete_ok)",
        ".mem_resp_ok(texture_ok)",
    )?;
    once(
        &mut body,
        ".mem_read_data(mem_read_data)",
        ".mem_read_data(framebuffer_read_data)",
    )?;
    once(
        &mut body,
        ".mem_complete_ok(mem_complete_ok)",
        ".mem_complete_ok(framebuffer_ok)",
    )?;
    // Both original clients retain their active request through terminal ACK.
    // Their drained/idle flags cover their memory lifetimes without an extra
    // transport owner. The enclosing GPU also includes the boundary's busy.
    once(
        &mut body,
        "assign idle=pipeline_idle&&sink_idle&&arb_state==0;",
        "assign idle=pipeline_idle&&sink_idle;",
    )?;
    let transport = body
        .find(" // Fixed-priority single-outstanding transport.")
        .ok_or("backend old transport marker")?;
    if body[transport..].matches("always @(posedge clk)").count() != 1 {
        return Err("backend old transport body changed".into());
    }
    body.truncate(transport);
    body.push_str(" // Independent client transactions; no texture/framebuffer arbitration here.\n assign texture_request_valid=txreq;assign texture_request_address=txaddr;\n assign txready=texture_request_ready;assign txbeat=texture_read_valid;\n assign txindex=texture_read_index;assign txcomplete=texture_complete;\n assign framebuffer_request_valid=fbreq;assign framebuffer_request_write=fbwrite;\n assign framebuffer_request_address=fbaddr;assign fbready=framebuffer_request_ready;\n assign fbbeat=framebuffer_read_valid;assign fbcomplete=framebuffer_complete;\n assign framebuffer_write_valid=fbwvalid;assign framebuffer_write_data=fbwdata;\n assign fbwready=framebuffer_write_ready;\nendmodule");
    let mut result = normalized;
    result.replace_range(a..b, &body);
    Ok(result)
}

/// A backend sector is exactly128 bytes and naturally aligned within8MiB.
pub const fn valid_sector(address_bytes: u32) -> bool {
    address_bytes < 8 * 1024 * 1024 && address_bytes & 127 == 0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackendRequest {
    pub address_bytes: u32,
    pub write: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicalRequest {
    pub address_halfword: u32,
    pub line_count_minus_1: u8,
}
impl PhysicalRequest {
    pub const fn beats(self) -> u8 {
        (self.line_count_minus_1 + 1) * 4
    }
    pub const fn valid(self) -> bool {
        matches!(self.line_count_minus_1, 0 | 1 | 3)
            && self.address_halfword < 1 << 22
            && self.address_halfword & ((self.line_count_minus_1 as u32 + 1) * 16 - 1) == 0
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicalFeedback {
    pub request_ready: bool,
    pub write_ready: bool,
    pub response: Option<dma::Response>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    /// Only descriptor/abort/page/complete fields are external. Transport fields
    /// are overwritten by this boundary and are never caller-selected.
    pub geometry: dma::Input,
    pub texture: Option<u32>,
    pub framebuffer: Option<BackendRequest>,
    pub framebuffer_write: Option<u64>,
    /// RO, FB read, FB write. Their progress is independent of compute CE.
    pub physical: [PhysicalFeedback; 3],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackendResponse {
    pub request_ready: bool,
    pub write_ready: bool,
    pub read: Option<(u8, u64)>,
    pub complete: Option<bool>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub geometry: dma::Output,
    pub texture: BackendResponse,
    pub framebuffer: BackendResponse,
    pub physical: [Option<PhysicalRequest>; 3],
    pub write_data: u64,
    pub busy: bool,
    pub fault: bool,
}
#[derive(Clone, Copy, Debug, Default)]
enum State {
    #[default]
    Idle,
    Request,
    Active,
    Error,
}
#[derive(Clone, Copy, Debug, Default)]
struct Transaction {
    state: State,
    request: PhysicalRequest,
    /// RO: texture rather than geometry. FB: writing rather than reading.
    mode: bool,
    legal: bool,
    beats: u8,
    bad: bool,
}
#[derive(Default)]
pub struct Boundary {
    loader: Loader,
    ro: Transaction,
    fb: Transaction,
    prefer_texture: bool,
    fault: bool,
}
impl Boundary {
    fn evaluate(&self, i: &Input) -> (Output, dma::Input) {
        let mut o = Output {
            write_data: i.framebuffer_write.unwrap_or(0),
            fault: self.fault,
            ..Output::default()
        };
        let mut di = i.geometry;
        di.reset = i.reset;
        di.request_ready = false;
        di.response = None;
        if !i.reset {
            let destination = &mut o.texture;
            match self.ro.state {
                State::Request => {
                    let ready = !self.ro.legal || i.physical[0].request_ready;
                    if self.ro.legal {
                        o.physical[0] = Some(self.ro.request);
                    }
                    if self.ro.mode {
                        destination.request_ready = ready;
                    } else {
                        di.request_ready = ready;
                    }
                }
                State::Error => {
                    if self.ro.mode {
                        destination.complete = Some(false);
                    } else {
                        di.response = Some(dma::Response {
                            error: true,
                            last: true,
                            ..dma::Response::default()
                        });
                    }
                }
                State::Active => {
                    if let Some(r) = i.physical[0].response {
                        if self.ro.mode {
                            if !r.error && self.ro.beats < 16 {
                                destination.read = Some((self.ro.beats, r.data));
                            }
                            if r.last || r.error {
                                destination.complete =
                                    Some(!r.error && !self.ro.bad && !malformed(self.ro, r));
                            }
                        } else {
                            di.response = Some(r);
                        }
                    }
                }
                State::Idle => {}
            }
            match self.fb.state {
                State::Request => {
                    let channel = if self.fb.mode { 2 } else { 1 };
                    let present = !self.fb.mode || i.framebuffer_write.is_some();
                    if self.fb.legal && present {
                        o.physical[channel] = Some(self.fb.request);
                    }
                    o.framebuffer.request_ready =
                        !self.fb.legal || (present && i.physical[channel].request_ready);
                }
                State::Error => o.framebuffer.complete = Some(false),
                State::Active => {
                    let channel = if self.fb.mode { 2 } else { 1 };
                    let f = i.physical[channel];
                    let wb = self.fb.mode && f.write_ready && self.fb.beats < 16;
                    let rb =
                        !self.fb.mode && f.response.is_some_and(|r| !r.error) && self.fb.beats < 16;
                    o.framebuffer.write_ready = wb;
                    if rb {
                        o.framebuffer.read = Some((self.fb.beats, f.response.unwrap().data));
                    }
                    if let Some(r) = f.response.filter(|r| r.last || r.error) {
                        o.framebuffer.complete = Some(
                            !r.error
                                && !self.fb.bad
                                && !fb_malformed(self.fb, i)
                                && self.fb.beats + u8::from(wb) + u8::from(rb) == 16,
                        );
                    }
                }
                State::Idle => {}
            }
        }
        o.geometry = self.loader.output(&di);
        o.busy = !matches!(self.ro.state, State::Idle)
            || !matches!(self.fb.state, State::Idle)
            || o.geometry.busy;
        (o, di)
    }
    pub fn output(&self, i: &Input) -> Output {
        self.evaluate(i).0
    }
    pub fn step(&mut self, i: Input) -> Output {
        let (o, di) = self.evaluate(&i);
        if i.reset {
            *self = Self::default();
            self.loader.step(di);
            return o;
        }
        self.loader.step(di);
        let ro_active = matches!(self.ro.state, State::Active);
        let fb_active = matches!(self.fb.state, State::Active);
        let fb_writing = self.fb.mode;
        match self.ro.state {
            State::Idle => {
                let geometry = o.geometry.request;
                if geometry.is_some() || i.texture.is_some() {
                    let texture =
                        i.texture.is_some() && (geometry.is_none() || self.prefer_texture);
                    let (request, legal) = if texture {
                        let a = i.texture.unwrap();
                        (
                            PhysicalRequest {
                                address_halfword: (a >> 1) & 0x3fffff,
                                line_count_minus_1: 3,
                            },
                            valid_sector(a),
                        )
                    } else {
                        let r = geometry.unwrap();
                        let p = PhysicalRequest {
                            address_halfword: r.address_halfword,
                            line_count_minus_1: r.line_count_minus_1,
                        };
                        (p, p.valid())
                    };
                    self.ro = Transaction {
                        state: State::Request,
                        request,
                        legal,
                        mode: texture,
                        ..Transaction::default()
                    };
                }
            }
            State::Request if di.request_ready || o.texture.request_ready => {
                self.ro.state = if self.ro.legal {
                    State::Active
                } else {
                    State::Error
                };
                self.prefer_texture = !self.ro.mode;
                self.fault |= !self.ro.legal;
            }
            State::Active => {
                if let Some(r) = i.physical[0].response {
                    self.ro.bad |= malformed(self.ro, r);
                    self.fault |= self.ro.bad || r.error;
                    self.ro.beats = (self.ro.beats + 1).min(31);
                    if r.last || r.error {
                        self.ro.state = State::Idle;
                    }
                }
            }
            State::Error => self.ro.state = State::Idle,
            _ => {}
        }
        let channel = if self.fb.mode { 2 } else { 1 };
        let f = i.physical[channel];
        match self.fb.state {
            State::Idle => {
                if let Some(r) = i.framebuffer {
                    self.fb = Transaction {
                        state: State::Request,
                        request: PhysicalRequest {
                            address_halfword: (r.address_bytes >> 1) & 0x3fffff,
                            line_count_minus_1: 3,
                        },
                        mode: r.write,
                        legal: valid_sector(r.address_bytes),
                        ..Transaction::default()
                    };
                }
            }
            State::Request if o.framebuffer.request_ready => {
                self.fb.state = if self.fb.legal {
                    State::Active
                } else {
                    State::Error
                };
                self.fault |= !self.fb.legal;
            }
            State::Active => {
                self.fb.bad |= fb_malformed(self.fb, &i);
                self.fault |= self.fb.bad || f.response.is_some_and(|r| r.error);
                self.fb.beats +=
                    u8::from(o.framebuffer.write_ready || o.framebuffer.read.is_some());
                if let Some(r) = f.response.filter(|r| r.last || r.error) {
                    self.fb.state = State::Idle;
                    self.fault |= !r.error && self.fb.beats != 16;
                }
            }
            State::Error => self.fb.state = State::Idle,
            _ => {}
        }
        if (i.physical[0].response.is_some() && !ro_active)
            || (i.physical[1].response.is_some() && (!fb_active || fb_writing))
            || (i.physical[2].response.is_some() && (!fb_active || !fb_writing))
        {
            self.fault = true;
        }
        o
    }
}
fn malformed(t: Transaction, r: dma::Response) -> bool {
    !r.error && (t.beats >= t.request.beats() || r.last != (t.beats + 1 == t.request.beats()))
}
fn fb_malformed(t: Transaction, i: &Input) -> bool {
    let f = i.physical[if t.mode { 2 } else { 1 }];
    if t.mode {
        (f.write_ready && t.beats < 16 && i.framebuffer_write.is_none())
            || f.response.is_some_and(|r| !r.error && !r.last)
    } else {
        f.response
            .is_some_and(|r| !r.error && (t.beats >= 16 || r.last != (t.beats == 15)))
    }
}
