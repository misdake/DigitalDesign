//! Actual Work16 -> two-edge Packet -> native Group35/H15 composition.
use super::{
    super::{require, vertex::Bits, ContractError},
    group, packet,
    word::Word,
    work,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub proposal_ce: bool,
    pub commit_allow: bool,
    pub abort: bool,
    pub valid: bool,
    pub member: Word,
    pub packet_enable: bool,
    pub out_ready: bool,
    pub texture_base: u16,
    pub mip: bool,
    pub framebuffer: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub ready: bool,
    pub proposal_ready: bool,
    pub proposal_ack: bool,
    pub advance: bool,
    pub fault: bool,
    pub valid: bool,
    pub payload: Bits,
    pub next_valid: bool,
    pub next_key: Bits,
    pub work: work::Output,
    pub group: group::Output,
    pub packet: packet::Output,
    pub issue: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Backend {
    pub work: work::Work,
    pub packet: packet::Packet,
    pub group: group::Group,
    pub stopped: bool,
}
impl Backend {
    fn connect(&self, i: Input) -> (work::Input, packet::Input, group::Input, Output) {
        let fault = self.stopped || i.abort;
        let attempted = i.proposal_ce && !i.reset && !fault;
        let advance = attempted && i.commit_allow;
        let initial = group::Input {
            reset: i.reset,
            ce: advance,
            ..Default::default()
        };
        let go = self.group.outputs(initial);
        let wi = work::Input {
            reset: i.reset,
            ce: advance,
            valid: i.valid,
            member: i.member,
            out_ready: go.producer_ready && i.packet_enable,
        };
        let wo = self.work.outputs(wi);
        let issue = wo.valid && go.producer_ready && i.packet_enable;
        let pi = packet::Input {
            reset: i.reset,
            ce: advance,
            valid: issue,
            member: wo.member,
            tap: wo.tap,
            texture_base: i.texture_base,
            mip: i.mip,
            framebuffer: i.framebuffer,
        };
        let po = self.packet.outputs(pi);
        let gi = group::Input {
            reserve: issue,
            write: po.valid,
            payload: po.packet,
            consume: go.valid && i.out_ready,
            ..initial
        };
        let go = self.group.outputs(gi);
        let last = (wo.member.slice(36, 4).value >> ((wo.tap.value & 3) + 1)) == 0;
        (
            wi,
            pi,
            gi,
            Output {
                ready: advance && wo.ready,
                proposal_ready: !i.reset && !fault && wo.ready,
                proposal_ack: attempted && issue && last,
                advance,
                fault,
                valid: !i.reset && !fault && go.valid,
                payload: go.payload,
                next_valid: go.next_valid,
                next_key: go.next_key,
                work: wo,
                group: go,
                packet: po,
                issue: advance && issue,
            },
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.connect(i).3
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let (wi, pi, gi, o) = self.connect(i);
        let bad = i.valid && o.work.ready && i.member.slice(36, 4).value == 0
            || pi.valid && o.work.member.slice(36, 4).value & (1 << o.work.tap.value) == 0
            || o.packet.valid && o.group.producer_count == 0;
        let preflight = require(
            !(!i.reset && !i.abort && i.proposal_ce && !o.fault && bad),
            "command packet backend",
            "GPU contract: preflight_bad",
        );
        let work = self.work.clock(wi);
        let packet = self.packet.clock(pi);
        let group = self.group.clock(gi);
        if i.reset {
            self.stopped = false;
        } else if i.abort {
            self.stopped = true;
        }
        preflight?;
        work?;
        packet?;
        group
    }
}
