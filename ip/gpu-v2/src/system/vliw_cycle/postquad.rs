//! Selected three-port backend join, Final, paired source and physical TDP sink.
//! Lighting and Sampling return/credit ports are explicit until their owners are
//! connected. The producer pool stays the single authoritative payload store.
use super::{
    pixel_math::{Final, FinalInput, FinalOutput, SharedBank, SharedInput},
    pool, producer, require, tdp,
    vertex::Bits,
    ContractError,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Memory {
    pub request_ready: bool,
    pub read_data: u64,
    pub read_valid: bool,
    pub write_ready: bool,
    pub complete: bool,
    pub ok: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct LightResult {
    pub valid: bool,
    pub key: u8,
    pub last: bool,
    pub g: Bits,
    pub h: Bits,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct SampleResult {
    pub valid: bool,
    pub key: u8,
    pub last: bool,
    pub rgb: Bits,
    pub alpha: Bits,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub serial: tdp::Serial,
    pub reset: bool,
    pub ce: bool,
    pub abort: bool,
    pub pool: pool::Output,
    pub light: LightResult,
    pub sample: SampleResult,
    pub light_source_ready: bool,
    pub sample_source_ready: bool,
    pub sample_publication: Option<u8>,
    pub sampling_drained: bool,
    pub specular: u32,
    pub depth_func: u8,
    pub depth_write: bool,
    pub blend: bool,
    pub color_base: u16,
    pub depth_base: u16,
    pub flush_request: bool,
    pub memory: Memory,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub feedback: producer::Backend,
    pub fault: bool,
    pub advance: bool,
    pub pipeline_idle: bool,
    pub idle: bool,
    pub light_result: bool,
    pub sample_result: bool,
    pub final_issue: bool,
    pub final_result: bool,
    pub final_stage: FinalOutput,
    pub sink: tdp::SinkOutput,
}
#[derive(Clone, Debug)]
pub struct LoanBank {
    pub cells: [Bits; 128],
    pub q: Bits,
}
struct LoanPorts {
    reset: bool,
    reset_q: bool,
    width: u32,
    read: bool,
    read_address: u8,
    write: bool,
    write_address: Bits,
    data: Bits,
}
impl Default for LoanBank {
    fn default() -> Self {
        Self {
            cells: [Bits::default(); 128],
            q: Bits::default(),
        }
    }
}
impl LoanBank {
    fn clock(&mut self, ports: LoanPorts) {
        let LoanPorts {
            reset,
            reset_q,
            width,
            read,
            read_address,
            write,
            write_address,
            data,
        } = ports;
        if reset {
            if reset_q {
                self.q = Bits::new(0, width);
            }
            return;
        }
        if Self::collision(read, read_address, write, write_address) {
            return;
        }
        if read {
            self.q = self.cells[usize::from(read_address & 127)];
        }
        if write && write_address.is_known(7) {
            self.cells[(write_address.value & 127) as usize] = data.slice(0, width);
        }
    }
    fn collision(read: bool, read_address: u8, write: bool, write_address: Bits) -> bool {
        read && write
            && write_address.is_known(7)
            && u64::from(read_address & 127) == write_address.value & 127
    }
}
/// The portable owned-Q template retains Q on reset; the native SDP36 resets
/// its hard output. Both retain payload cells and require a paid source read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    #[default]
    Portable,
    Native,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub stopped: bool,
    pub shade_phase: bool,
    pub light_complete: bool,
    pub sample_complete: bool,
    pub light_last: u8,
    pub sample_last: u8,
    pub active: bool,
    pub header: Bits,
    pub wait: bool,
    pub issue_slot: u8,
    pub issue_lane: u8,
    pub bypass: u8,
    pub public_upper: [Bits; 16],
}
#[derive(Clone, Debug, Default)]
pub struct Join {
    pub profile: Profile,
    pub state: State,
    pub light_bank: LoanBank,
    pub sample_bank: LoanBank,
    pub final_stage: Final,
    pub shared: SharedBank,
    pub sink: tdp::Sink,
}
struct Wiring {
    o: Output,
    fi: FinalInput,
    si: tdp::SinkInput,
    read: bool,
    read_address: u8,
    source_advance: bool,
    sample_address: Bits,
    light_collision: bool,
    sample_collision: bool,
}
impl Join {
    pub(crate) fn observe(&self, i: Input, trace: &mut super::trace::Trace) {
        let e = self.connect(i);
        trace.load[13] = u64::from(e.o.final_stage.credits);
        trace.load[19] = u64::from(e.o.final_issue);
        trace.load[20] = u64::from(e.o.sink.lane_accept);
        self.sink.observe(e.si, trace, i.pool.retire_slot);
        if e.o.final_issue {
            trace.event(
                "final",
                [
                    u64::from(self.state.issue_slot & 31),
                    u64::from(self.state.issue_lane),
                ],
            );
        }
    }
    pub fn native() -> Self {
        Self {
            profile: Profile::Native,
            ..Default::default()
        }
    }
    fn connect(&self, i: Input) -> Wiring {
        let s = &self.state;
        let p = i.pool;
        let fault = s.stopped || i.abort;
        let compute_ce = i.ce && !fault;
        let advance = compute_ce && !i.reset;
        let issue_header = p.descriptor_issue.value;
        let issue_ahead = s.issue_slot != p.retire_slot;
        let exists = s.issue_slot != p.published_slot;
        let light_ready = issue_header & 32 != 0
            || s.light_complete && (!issue_ahead || s.light_last != p.retire_slot);
        let sample_ready = issue_header & 64 != 0
            || s.sample_complete && (!issue_ahead || s.sample_last != p.retire_slot);
        let covered = issue_header & (1 << s.issue_lane) != 0;
        let out_covered = s.header.value & (1 << self.sink.lane) != 0;
        let source_exists = s.active && issue_header & 15 != 0 && exists;
        let room = !issue_ahead || s.issue_lane != 3;
        let read = source_exists && !s.wait && covered && light_ready && sample_ready;
        let read_address = (s.issue_slot << 2) | s.issue_lane;
        let skip_issue = source_exists && !s.wait && !covered && room;
        let light_result = advance && i.light.valid;
        let sample_result = advance && i.sample.valid;
        let upper = s.public_upper[usize::from(i.sample.key >> 2 & 15)];
        let sample_address = Bits::concat(upper, Bits::new(i.sample.key.into(), 6), 6);
        let g = if s.bypass & 1 != 0 {
            Bits::new(256, 9)
        } else {
            self.light_bank.q.slice(0, 9)
        };
        let h = if s.bypass & 1 != 0 {
            Bits::new(0, 9)
        } else {
            self.light_bank.q.slice(9, 9)
        };
        let texture = if s.bypass & 2 != 0 {
            p.basic_data
        } else {
            self.sample_bank.q
        };
        let fi = FinalInput {
            reset: i.reset,
            ce: advance,
            proposal_ce: i.ce,
            commit_allow: compute_ce,
            abort: i.abort,
            shade_phase: s.shade_phase,
            in_valid: s.active && s.wait && room,
            tint: p.issue_color,
            texture: texture.slice(0, 24),
            texture_alpha: if s.bypass & 2 != 0 {
                Bits::new(255, 8)
            } else {
                self.sample_bank.q.slice(24, 8)
            },
            specular: Bits::new(i.specular.into(), 24),
            g,
            h,
            shared_spec_product: self.shared.spec_product(),
            out_ready: false,
        };
        let fo = self.final_stage.outputs(fi);
        let output_issued = issue_ahead || s.issue_lane > self.sink.lane;
        let lane_valid =
            s.active && output_issued && (!out_covered || fo.out_valid || self.sink.phase);
        let pipeline_idle = !s.active && p.global_used == 0 && i.sampling_drained && !fo.out_valid;
        let ci = tdp::Input {
            serial: i.serial,
            reset: i.reset,
            ce: i.ce,
            commit_allow: compute_ce,
            shade_phase: s.shade_phase,
            shared_rop_product: self.shared.rop_product(),
            prefetch_abort: fault,
            prefetch_valid: p.prefetch_valid,
            prefetch_tile: p.prefetch_tile,
            tile: (((s.header.value >> 19) & 15) << 5 | ((s.header.value >> 11) & 31)) as u16,
            x: (((s.header.value >> 8) & 255) << 1) as u16,
            y: (((s.header.value >> 16) & 127) << 1) as u8,
            mask: (s.header.value & 15) as u8,
            alpha: p.depth_data.slice(16, 8),
            depth_func: i.depth_func,
            depth_write: i.depth_write,
            blend: i.blend,
            color_base: i.color_base,
            depth_base: i.depth_base,
            flush_request: i.flush_request && pipeline_idle,
            mem_req_ready: i.memory.request_ready,
            mem_read_data: i.memory.read_data,
            mem_read_valid: i.memory.read_valid,
            mem_write_ready: i.memory.write_ready,
            mem_complete_valid: i.memory.complete,
            mem_complete_ok: i.memory.ok,
            ..Default::default()
        };
        let si = tdp::SinkInput {
            cache: ci,
            lane_valid,
            rgba: Bits::concat(
                fo.texture_alpha,
                if out_covered {
                    fo.rgb
                } else {
                    Bits::new(0, 24)
                },
                24,
            ),
            depth: p.depth_data.slice(0, 16),
        };
        let sink = self.sink.outputs(si);
        let fi = FinalInput {
            out_ready: lane_valid && sink.color_ready && out_covered,
            ..fi
        };
        let fo = self.final_stage.outputs(fi);
        let empty_skip = advance
            && !s.active
            && p.empty_head
            && p.empty_skip_ready
            && s.issue_slot == p.retire_slot
            && !s.wait
            && s.issue_lane == 0
            && !self.sink.phase
            && !fo.out_valid;
        let join_issue = s.active && s.wait && fo.proposal_ready && room;
        let raw_retire = i.ce && lane_valid && sink.lane_ready && self.sink.lane == 3;
        let feedback = producer::Backend {
            ce: advance,
            publish_ready: true,
            descriptor_issue_slot: s.issue_slot,
            basic_read: read,
            basic_address: read_address,
            depth_read: sink.color_accept,
            depth_address: (p.retire_slot << 2) | self.sink.lane,
            light_ready: i.light_source_ready,
            sample_ready: i.sample_source_ready,
            retire_valid: raw_retire,
            retire_id: p.retire_slot,
            empty_skip,
        };
        let light_collision = LoanBank::collision(
            advance && read,
            read_address,
            light_result,
            Bits::new(i.light.key.into(), 7),
        );
        let sample_collision =
            LoanBank::collision(advance && read, read_address, sample_result, sample_address);
        Wiring {
            o: Output {
                feedback,
                fault,
                advance,
                pipeline_idle,
                idle: pipeline_idle && sink.idle,
                light_result,
                sample_result,
                final_issue: fo.in_accept,
                final_result: sink.color_accept && out_covered,
                final_stage: fo,
                sink,
            },
            fi,
            si,
            read,
            read_address,
            source_advance: join_issue || skip_issue,
            sample_address,
            light_collision,
            sample_collision,
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.connect(i).o
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let w = self.connect(i);
        let s = &self.state;
        let o = w.o;
        let p = i.pool;
        let mut n = s.clone();
        let monitor = require(
            i.reset || i.abort || !(w.light_collision || w.sample_collision || p.basic_collision),
            "postquad",
            "GPU contract: stopped",
        );
        if i.reset {
            n.stopped = false;
            n.shade_phase = false;
            n.active = false;
            n.wait = false;
            n.issue_slot = 0;
            n.bypass = 0;
            n.issue_lane = 0;
            n.light_complete = false;
            n.sample_complete = false;
            n.light_last = 0;
            n.sample_last = 0;
        } else {
            n.stopped |= i.abort;
            if let Some(slot) = i.sample_publication {
                n.public_upper[usize::from(slot & 15)] = Bits::new(u64::from(slot >> 4 & 1), 1);
            }
            if o.advance {
                n.shade_phase = !s.shade_phase;
                if o.sink.quad_accepted && p.retire_slot == s.light_last {
                    n.light_complete = false;
                }
                if o.light_result && i.light.last {
                    n.light_complete = true;
                    n.light_last = i.light.key >> 2 & 31;
                }
                if o.sink.quad_accepted && p.retire_slot == s.sample_last {
                    n.sample_complete = false;
                }
                if o.sample_result && i.sample.last {
                    n.sample_complete = true;
                    n.sample_last = (w.sample_address.value >> 2 & 31) as u8;
                }
                if !s.active && p.retire_slot != p.published_slot && !p.empty_head {
                    n.active = true;
                    n.header = p.descriptor_head;
                }
                if w.read {
                    n.wait = true;
                    n.bypass = (p.descriptor_issue.value >> 5 & 3) as u8;
                }
                if w.source_advance {
                    n.wait = false;
                }
                if w.source_advance || o.feedback.empty_skip {
                    if o.feedback.empty_skip || s.issue_lane == 3 {
                        n.issue_lane = 0;
                        n.issue_slot = (s.issue_slot + 1) & 31;
                    } else {
                        n.issue_lane = s.issue_lane + 1;
                    }
                }
                if o.sink.quad_accepted {
                    if (p.retire_slot + 1) & 31 != p.published_slot {
                        n.active = p.descriptor_next.value & 15 != 0;
                        n.header = p.descriptor_next;
                    } else {
                        n.active = false;
                    }
                }
            }
        }
        self.light_bank.clock(LoanPorts {
            reset: i.reset,
            reset_q: self.profile == Profile::Native,
            width: 18,
            read: o.advance && w.read,
            read_address: w.read_address,
            write: o.light_result,
            write_address: Bits::new(i.light.key.into(), 7),
            data: Bits::concat(i.light.h, i.light.g, 9),
        });
        self.sample_bank.clock(LoanPorts {
            reset: i.reset,
            reset_q: self.profile == Profile::Native,
            width: 32,
            read: o.advance && w.read,
            read_address: w.read_address,
            write: o.sample_result,
            write_address: w.sample_address,
            data: Bits::concat(i.sample.alpha, i.sample.rgb, 24),
        });
        let final_result = self.final_stage.clock(w.fi);
        let sink_result = self.sink.clock(w.si);
        self.shared.clock(SharedInput {
            rop_ce: o.sink.cache.shared_rop_ce,
            rop_a: o.sink.cache.shared_rop_a,
            rop_b: o.sink.cache.shared_rop_b,
            spec_ce: o.final_stage.shared_spec_ce,
            spec_a: o.final_stage.shared_spec_a,
            spec_b: o.final_stage.shared_spec_b,
        });
        self.state = n;
        monitor?;
        final_result?;
        sink_result
    }
}
