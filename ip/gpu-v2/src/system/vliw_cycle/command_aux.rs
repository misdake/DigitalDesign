//! Production auxiliary CP registers and passive descriptor-Q monitor.
//! Bases/length stay in the held source Q; the instruction owns the cursor.
use super::super::{require, vertex::Bits, ContractError};
use super::{Phase, Processor, ProcessorInput, ProcessorOutput};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub clear_ready: bool,
    pub clear_complete: bool,
    pub clear_ok: bool,
    pub ui_done: bool,
    pub copy_ready: bool,
    pub copy_complete: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub clear_active: bool,
    pub clear_request: bool,
    pub clear_address: u32,
    pub clear_value: u16,
    pub ui_start: bool,
    pub ui_first: u16,
    pub ui_last: u16,
    pub copy_active: bool,
    pub copy_request: bool,
    pub copy_write: bool,
    pub copy_address: Bits,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Auxiliary {
    pub clear_flight: u8,
    pub clear_offer: bool,
    pub copy_address: Bits,
    /// Simulation-only loan checker, never an operand source.
    descriptor_guard: Option<u64>,
}
impl Auxiliary {
    pub fn outputs(&self, p: &Processor, i: ProcessorInput, o: ProcessorOutput) -> Output {
        let phase = p.phase;
        let command = p.command;
        let sector = (command >> 17) & 2047;
        let bases = p.draw.as_ref().unwrap().config;
        let base = if command & (1 << 16) != 0 {
            bases.depth_base
        } else {
            bases.color_base
        };
        let page = ((u32::from(base) >> 7) + (sector >> 9)) & 127;
        let copy_active = matches!(
            phase,
            Phase::CopyAddress | Phase::CopyRequest | Phase::CopyWait
        );
        Output {
            clear_active: matches!(phase, Phase::ClearRequest | Phase::ClearWait),
            clear_request: self.clear_offer,
            clear_address: (page << 16) | ((sector & 511) << 7),
            clear_value: command as u16,
            ui_start: i.ce && !o.stop && phase == Phase::ApplyWrite && command >> 28 == 11,
            ui_first: if command & (1 << 27) != 0 {
                (command >> 7 & 1023) as u16
            } else {
                0
            },
            ui_last: if command & (1 << 27) != 0 {
                (command >> 17 & 1023) as u16
            } else {
                749
            },
            copy_active,
            copy_request: !o.stop && phase == Phase::CopyRequest,
            copy_write: p.draw.as_ref().unwrap().object_index & 2 != 0,
            copy_address: self.copy_address,
        }
    }
    pub fn check(
        &self,
        p: &Processor,
        i: ProcessorInput,
        o: ProcessorOutput,
    ) -> Result<(), ContractError> {
        let a = o.auxiliary;
        let op = p.command >> 28;
        let check = |ok, rule| require(ok, "command_processor", rule);
        if i.ce && !o.stop && p.phase == Phase::Exec {
            let slot = p.command & 127;
            let object = p.command & 0x0fff_ff80 == 0 && (66..=79).contains(&slot);
            let d = p.draw.as_ref().unwrap();
            match op {
                8 => check(
                    p.command & 0x0ffe_0000 == 0
                        && d.target_valid
                        && p.all_fence
                        && i.render_idle
                        && !i.work_busy,
                    "CP plane CLEAR contract",
                )?,
                11 => check(
                    (66..=79).contains(&slot)
                        && a.ui_first <= a.ui_last
                        && a.ui_last < 750
                        && (p.command & (1 << 27) != 0 || p.command & 0x07ff_ff80 == 0)
                        && p.all_fence
                        && p.stored
                        && i.render_idle
                        && !i.work_busy
                        && d.target_valid,
                    "CP UI drain contract",
                )?,
                12 => check(
                    object && p.all_fence && p.stored && i.render_idle && !i.work_busy,
                    "CP MEMCPY drain contract",
                )?,
                _ => {}
            }
        }
        if i.ce && !o.stop && p.phase == Phase::ApplyWrite && op == 12 {
            let source = i.ram_q & 16383;
            let destination = i.ram_q >> 14 & 16383;
            let length = (i.ram_q >> 28 & 2047) + 1;
            check(
                i.ram_q >> 39 == 0
                    && source + length <= 16384
                    && destination + length <= 16384
                    && (source + length <= destination || destination + length <= source),
                "CP state data contract",
            )?;
        }
        if a.ui_start {
            check(i.ram_q >> 23 == 0, "CP UI descriptor")?;
        }
        if a.clear_active && i.auxiliary.clear_complete {
            check(i.auxiliary.clear_ok, "CP CLEAR memory response")?;
        }
        if a.copy_active {
            check(
                self.descriptor_guard == Some(i.ram_q),
                "CP MEMCPY descriptor Q changed",
            )?;
        }
        Ok(())
    }
    pub fn wall(&self, p: &Processor, n: &mut Processor, i: ProcessorInput, o: ProcessorOutput) {
        let a = o.auxiliary;
        let feedback = i.auxiliary;
        if i.ce && !o.stop && p.phase == Phase::ApplyWrite && p.command >> 28 == 12 {
            n.auxiliary.as_mut().unwrap().descriptor_guard = Some(i.ram_q);
        }
        if p.phase == Phase::ClearRequest && !self.clear_offer && self.clear_flight < 2 && !o.stop {
            n.auxiliary.as_mut().unwrap().clear_offer = true;
        }
        let take = self.clear_offer && feedback.clear_ready;
        let done = a.clear_active && feedback.clear_complete;
        if take {
            n.auxiliary.as_mut().unwrap().clear_offer = false;
            let sector = p.command >> 17 & 2047;
            if sector == 1599 || o.stop {
                n.phase = Phase::ClearWait;
            } else {
                n.command = (p.command & !(2047 << 17)) | (((sector + 1) & 2047) << 17);
            }
        }
        n.auxiliary.as_mut().unwrap().clear_flight = self
            .clear_flight
            .wrapping_add(u8::from(take))
            .wrapping_sub(u8::from(done))
            & 3;
        if a.clear_active && o.stop && !self.clear_offer {
            n.phase = Phase::ClearWait;
        }
        if p.phase == Phase::ClearWait
            && !self.clear_offer
            && (self.clear_flight == 0 || (self.clear_flight == 1 && feedback.clear_complete))
        {
            n.phase = if o.stop {
                Phase::Failed
            } else {
                Phase::ClearDone
            };
        }
        if p.phase == Phase::CopyAddress {
            if o.stop {
                n.phase = Phase::Failed;
            } else {
                let base = if a.copy_write {
                    i.ram_q >> 14 & 16383
                } else {
                    i.ram_q & 16383
                };
                n.auxiliary.as_mut().unwrap().copy_address =
                    Bits::new(base + u64::from(p.command >> 17 & 2047), 14);
                n.phase = Phase::CopyRequest;
            }
        }
        if p.phase == Phase::CopyRequest && feedback.copy_ready {
            n.phase = if o.stop {
                Phase::Failed
            } else {
                Phase::CopyWait
            };
        }
        if p.phase == Phase::CopyWait && feedback.copy_complete {
            let index = p.draw.as_ref().unwrap().object_index;
            if index == 0 {
                n.draw.as_mut().unwrap().object_index = 1;
            } else if o.stop {
                n.phase = Phase::Failed;
            } else if index & 2 == 0 {
                n.draw.as_mut().unwrap().object_index = 2;
                n.phase = Phase::CopyAddress;
            } else if u64::from(p.command >> 17 & 2047) == (i.ram_q >> 28 & 2047) {
                n.phase = Phase::CopyDone;
            } else {
                n.command = (p.command & !(2047 << 17)) | ((((p.command >> 17) + 1) & 2047) << 17);
                n.draw.as_mut().unwrap().object_index = 0;
                n.phase = Phase::CopyAddress;
            }
        }
    }
}
