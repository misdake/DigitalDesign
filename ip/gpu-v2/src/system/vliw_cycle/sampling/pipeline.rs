//! Closed selected Sampling owners; all ports are old-state wires, not peers.
use super::super::{postquad, require, vertex::Bits, ContractError};
use super::{cache, coefficient, color, direct, member, packet_backend, word::Word};
#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    pub base: u16,
    pub n: u8,
    pub mip: bool,
    pub filter: u8,
    pub framebuffer: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Memory {
    pub request_ready: bool,
    pub read: bool,
    pub index: u8,
    pub data: u64,
    pub complete: bool,
    pub ok: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub commit_allow: bool,
    pub abort: bool,
    pub source_valid: bool,
    pub source_descriptor: u32,
    pub source_row: u8,
    pub source_data: Bits,
    pub config: Config,
    pub invalidate: bool,
    pub packet_enable: bool,
    pub result_ready: bool,
    pub memory: Memory,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub reserved: u8,
    pub source_ready: bool,
    pub source_fire: bool,
    pub publication: Option<u8>,
    pub result: postquad::SampleResult,
    pub drained: bool,
    pub fault: bool,
    pub direct: direct::Output,
    pub coefficient: coefficient::Output,
    pub member: member::Output,
    pub packets: packet_backend::Output,
    pub cache: cache::Output,
    pub color: color::Output,
    pub selected_coarse: bool,
    pub selected_active: bool,
    pub member_issue: bool,
    pub finish: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Sampler {
    pub direct: direct::Direct,
    pub coefficient: coefficient::Coefficient,
    pub member: member::Member,
    pub packets: packet_backend::Backend,
    pub cache: cache::Cache,
    pub color: color::Color,
    pub reserved: u8,
    pub coarse_next: bool,
}
struct Wires {
    direct: direct::Input,
    coefficient: coefficient::Input,
    member: member::Input,
    packets: packet_backend::Input,
    cache: cache::Input,
    color: color::Input,
    out: Output,
}
impl Sampler {
    pub fn new(profile: postquad::Profile) -> Self {
        if profile == postquad::Profile::Native {
            Self {
                direct: direct::Direct::native(),
                coefficient: coefficient::Coefficient::native(),
                cache: cache::Cache::native(),
                color: color::Color::native(),
                ..Default::default()
            }
        } else {
            Self::default()
        }
    }
    fn connect(&self, i: Input) -> Wires {
        let advance = i.ce && !i.reset && i.commit_allow;
        let quad = i.source_descriptor as u8 & 15;
        let mask = (i.source_descriptor >> 5) as u8 & 15;
        let first = i.source_descriptor & (1 << 10) != 0;
        let credit = !i.reset && self.color.live & (1 << quad) == 0;
        let admit = !first || credit;
        let mut di = direct::Input {
            reset: i.reset,
            ce: i.ce,
            commit_allow: i.commit_allow,
            valid: i.source_valid && admit,
            uv: (i.source_data.slice(18, 16).value as u32) << 16
                | i.source_data.slice(0, 16).value as u32,
            quad,
            lane: i.source_row,
            mask,
            lod: (i.source_descriptor >> 11) as u8,
            max_n: i.config.n,
            mip: i.config.mip,
            filter: i.config.filter,
            framebuffer: i.config.framebuffer,
            out_ready: false,
        };
        let d = self.direct.outputs(di);
        let mut ci = coefficient::Input {
            reset: i.reset,
            proposal_ce: i.ce && !i.reset,
            commit_allow: i.commit_allow,
            valid: d.valid,
            owner: d.owner,
            result: d.result,
            available: 16u8.wrapping_sub(self.reserved),
            out_ready: false,
        };
        let co = self.coefficient.outputs(ci);
        // Conserving floors make corner0 nonzero for every nonzero parent.
        let fine = co.fine.slice(0, 9).value != 0;
        let coarse = co.coarse.slice(0, 9).value != 0;
        let selected = self.coarse_next || !fine;
        let active = if selected { coarse } else { fine };
        let finish = selected || !coarse;
        ci.out_ready = i.ce && !i.reset && co.valid && finish;
        let co = self.coefficient.outputs(ci);
        di.out_ready = i.ce && !i.reset && co.proposal_ready;
        let d = self.direct.outputs(di);
        let mi = member::Input {
            reset: i.reset,
            ce: advance,
            valid: advance && co.valid,
            flags: if selected { co.coarse } else { co.fine },
            meta: co.meta,
            coarse: selected,
            last_fine: !coarse,
        };
        let mo = self.member.outputs(mi);
        let mut pi = packet_backend::Input {
            reset: i.reset,
            proposal_ce: i.ce,
            commit_allow: i.commit_allow,
            abort: false,
            valid: mo.valid,
            member: mo.member,
            packet_enable: i.packet_enable,
            out_ready: false,
            texture_base: i.config.base,
            mip: i.config.mip,
            framebuffer: i.config.framebuffer,
        };
        let po = self.packets.outputs(pi);
        let color_ce = i.ce && !i.abort;
        let mut ki = cache::Input {
            reset: i.reset,
            proposal_ce: i.ce,
            commit_allow: i.commit_allow,
            abort: i.abort,
            valid: po.valid,
            payload: Word {
                value: po.payload.value.into(),
                known: po.payload.known.into(),
            },
            texture_invalidate: i.invalidate,
            weight_write: co.weight_write,
            weight_address: co.weight_address.value as u8,
            weight_value: co.weight_value,
            next_valid: po.next_valid,
            next_key: po.next_key.value as u16,
            out_ready: color_ce && !i.reset && self.color.busy,
            mem_req_ready: i.memory.request_ready,
            mem_resp_valid: i.memory.read,
            mem_resp_index: i.memory.index,
            mem_resp_data: i.memory.data,
            mem_resp_complete: i.memory.complete,
            mem_resp_ok: i.memory.ok,
        };
        let ko = self.cache.outputs(ki);
        pi.out_ready = ko.proposal_ready;
        let po = self.packets.outputs(pi);
        // Only ready-dependent packet strobes changed; its held head/Q did not.
        ki.next_valid = po.next_valid;
        ki.next_key = po.next_key.value as u16;
        let ko = self.cache.outputs(ki);
        let source_ready = d.ready && admit;
        let fault = i.abort || self.cache.fault;
        let li = color::Input {
            reset: i.reset,
            proposal_ce: color_ce,
            commit_allow: i.commit_allow,
            check_quad: quad,
            publish_valid: i.ce && !i.reset && !fault && i.source_valid && source_ready && first,
            publish_quad: quad,
            publish_mask: mask,
            valid: ko.valid,
            payload: ko.payload,
            texels: ko.texels,
            out_ready: i.result_ready,
        };
        let lo = self.color.outputs(li);
        Wires {
            direct: di,
            coefficient: ci,
            member: mi,
            packets: pi,
            cache: ki,
            color: li,
            out: Output {
                reserved: self.reserved,
                source_ready,
                source_fire: d.issue,
                publication: lo.publish_fire.then_some(i.source_descriptor as u8 & 31),
                result: postquad::SampleResult {
                    valid: !i.reset && !fault && lo.valid,
                    key: lo.key.value as u8,
                    last: lo.last,
                    rgb: lo.rgb,
                },
                drained: ko.drained,
                fault,
                direct: d,
                coefficient: co,
                member: mo,
                packets: po,
                cache: ko,
                color: lo,
                selected_coarse: selected,
                selected_active: active,
                member_issue: advance && co.valid,
                finish,
            },
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.connect(i).out
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let w = self.connect(i);
        let o = w.out;
        let bad = self.reserved > 16
            || o.packets.work.materialized > self.reserved
            || (o.packets.proposal_ack && self.reserved == 0)
            || (o.member.valid && !o.packets.proposal_ready)
            || (i.ce && !i.reset && o.coefficient.valid && !o.selected_active);
        let diagnostics = require(
            !(!i.reset && i.ce && bad),
            "command coefficient backend",
            "GPU contract: preflight_bad",
        );
        let direct = self.direct.clock(w.direct);
        let coefficient = self.coefficient.clock(w.coefficient);
        let member = self.member.clock(w.member);
        let packets = self.packets.clock(w.packets);
        let cache = self.cache.clock(w.cache);
        let color = self.color.clock(w.color);
        if i.reset {
            self.reserved = 0;
            self.coarse_next = false;
        } else if i.ce && i.commit_allow {
            self.reserved = self
                .reserved
                .wrapping_add(o.coefficient.reserved)
                .wrapping_sub(u8::from(o.packets.work.ack))
                & 31;
            if o.member_issue {
                self.coarse_next = !o.finish;
            }
        }
        diagnostics?;
        direct?;
        coefficient?;
        member?;
        packets?;
        cache?;
        color
    }
}
