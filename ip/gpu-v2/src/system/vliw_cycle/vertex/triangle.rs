//! The selected bounded-F16 T control cone. RF/MAC/storage remain in Core.
use super::{mask, signed, Bits};
#[derive(Clone, Debug, Default)]
pub struct Triangle {
    pub any_front: bool,
    pub sticky: bool,
    pub align_negative: bool,
    pub negative: bool,
    pub cross_groups: u8,
    pub qmax: u8,
    pub det_exp: u8,
}
#[derive(Clone, Copy)]
pub(super) struct TInput {
    pub reset: bool,
    pub active: bool,
    pub pc: u16,
    pub ins: u128,
    pub cull: bool,
    pub refs: u32,
    pub work: Bits,
    pub unpacked: Bits,
    pub acc: Bits,
    pub y: Bits,
}
#[derive(Clone, Copy)]
pub(super) struct TComb {
    pub next_pc: u16,
    pub pos_read: bool,
    pub pos_address: u16,
    pub work_read: bool,
    pub work_write: bool,
    pub work_ra: u16,
    pub work_wa: u16,
    pub work_data: Bits,
    pub mac_enable: bool,
    pub mac_constant: bool,
    pub mac_a: Bits,
    pub acc_write: bool,
    pub acc_data: Bits,
    pub coefficient_write: bool,
    pub coefficient_row: u8,
    pub coefficient_data: Bits,
    pub publish: bool,
    pub done: bool,
    norm: u8,
    norm_more: bool,
    coarse: bool,
    cross_final_q: u8,
    exp_next: u8,
}
impl Triangle {
    pub(super) fn comb(&self, i: TInput) -> TComb {
        let field = |lo: u32, n: u32| ((i.ins >> lo) & ((1u128 << n) - 1)) as u8;
        let bit = |n: u32| i.ins & (1u128 << n) != 0;
        let active = i.active && !i.reset;
        let norm = field(6, 3);
        let flow = field(40, 3);
        let a = i.acc.value & mask(54);
        let prefix = |n: u32| {
            let p = a >> n;
            p != 0 && p != mask(54 - n)
        };
        let cross_more = prefix(26);
        let det_more = prefix(31) || (a as u32 == 0x80000000);
        let norm_more = match norm {
            1 => prefix(29),
            3 => det_more,
            4 => a & (1 << 30) == 0,
            _ => false,
        };
        let coarse = match norm {
            1 => true,
            3 => prefix(34),
            _ => a & (15 << 27) == 0,
        };
        let tail = if prefix(28) {
            3
        } else if prefix(27) {
            2
        } else if cross_more {
            1
        } else {
            0
        };
        let magnitude = norm == 3 && !det_more;
        let corrected = (a as u32
            ^ if magnitude && a & (1 << 53) != 0 {
                u32::MAX
            } else {
                0
            })
        .wrapping_add(u32::from(
            a & (1 << 53) != 0 && if magnitude { !self.sticky } else { self.sticky },
        ));
        let rf_we = active && field(29, 2) != 0;
        let rf_wa = field(17, 4);
        let front = i.unpacked.value & (1 << 31) == 0 && i.unpacked.value & mask(31) != 0;
        let reject = if flow == 5 {
            a == 0 || a & (1 << 53) != 0 && i.cull
        } else {
            !(self.any_front || rf_we && rf_wa == 9 && front)
        };
        let which = match field(9, 2) {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        let vertex = (i.refs >> (which * 6)) & 63;
        let norm_known = i.acc.is_known(54);
        let acc_data = if magnitude {
            if norm_known {
                Bits::new(u64::from(corrected), 54)
            } else {
                Bits::default()
            }
        } else if norm == 4 {
            if norm_known {
                Bits::new(
                    if coarse {
                        (a & mask(27)) << 4
                    } else {
                        (a & mask(30)) << 1
                    },
                    54,
                )
            } else {
                Bits::default()
            }
        } else if norm_known {
            Bits::new((signed(a, 54) >> if coarse { 4 } else { 1 }) as u64, 54)
        } else {
            Bits::default()
        };
        let work_data = if bit(70) {
            Bits::concat(Bits::new(0, 6), i.y.slice(26, 27).extend(27, 30), 30)
        } else {
            Bits::concat(
                Bits::new(
                    if norm == 1 {
                        u64::from(self.cross_groups) << 3
                    } else {
                        0
                    },
                    6,
                ),
                if norm_known {
                    Bits::new(u64::from(corrected), 30)
                } else {
                    Bits::default()
                },
                30,
            )
        };
        let align =
            ((self.qmax >> 2).wrapping_sub((i.work.value >> 33) as u8) & 7) * 4 + (self.qmax & 3);
        let mac_a = if i.work.slice(33, 3).is_known(3) {
            Bits::new(u64::from(0x04000000u32 >> align), 32)
        } else {
            Bits::default()
        };
        let coefficient = match field(35, 3) {
            1 => Bits::concat(
                Bits::new(u64::from(self.qmax) << 12, 18),
                i.work.slice(0, 27).extend(27, 36),
                36,
            ),
            2 => i.work.slice(0, 27).extend(27, 54),
            3 => Bits::concat(
                Bits::new(u64::from(self.det_exp), 23),
                i.acc.slice(0, 31),
                31,
            ),
            _ => i.y.slice(0, 38).extend(38, 54),
        };
        let exp_base = if bit(34) { self.qmax } else { self.det_exp };
        let exp_step = if bit(34) {
            30
        } else if norm == 4 {
            if coarse {
                124
            } else {
                127
            }
        } else if coarse {
            4
        } else {
            1
        };
        TComb {
            next_pc: if norm != 0 && norm_more {
                i.pc
            } else {
                (i.pc + 1) & 511
            },
            pos_read: active && bit(59),
            pos_address: (u16::from(field(4, 2)) << 6) | vertex as u16,
            work_read: active && bit(53),
            work_write: active && bit(54) && (!bit(68) || !norm_more),
            work_ra: u16::from(field(55, 4)),
            work_wa: u16::from(field(60, 3)),
            work_data,
            mac_enable: active && bit(31) && (!bit(68) || !norm_more),
            mac_constant: active && bit(69),
            mac_a,
            acc_write: active && (norm != 0 && norm_more || magnitude),
            acc_data,
            coefficient_write: active && bit(67),
            coefficient_row: field(63, 4),
            coefficient_data: coefficient,
            publish: active && flow == 6,
            done: active && (flow == 6 || (flow == 2 || flow == 5) && reject),
            norm,
            norm_more,
            coarse,
            cross_final_q: (self.cross_groups << 2) | tail,
            exp_next: exp_base.wrapping_add(exp_step) & 127,
        }
    }
    pub(super) fn clock(&mut self, i: TInput, x: TComb, start: bool, enable: bool) {
        if i.reset {
            *self = Self::default();
            return;
        }
        if !enable {
            return;
        }
        if start {
            self.any_front = false;
            self.negative = false;
            self.cross_groups = 0;
            self.qmax = 0;
            return;
        }
        if !i.active {
            return;
        }
        let bit = |n: u32| i.ins & (1u128 << n) != 0;
        let field = |lo: u32, n: u32| ((i.ins >> lo) & ((1u128 << n) - 1)) as u8;
        if field(29, 2) != 0 && [3, 6, 9].contains(&field(17, 4)) {
            self.any_front |= i.unpacked.value & (1 << 31) == 0 && i.unpacked.value & mask(31) != 0;
        }
        if bit(9) && x.norm == 0 {
            self.sticky = false;
        }
        if bit(11) {
            self.cross_groups = 0;
        }
        if bit(14) {
            self.qmax = 0;
        }
        if bit(69) {
            self.align_negative = i.work.value & (1 << 29) != 0;
        }
        if field(40, 3) == 5 {
            self.negative = i.acc.value & (1 << 53) != 0;
        }
        if bit(34) || x.norm_more && (x.norm == 3 || x.norm == 4) {
            self.det_exp = x.exp_next;
        }
        if x.norm != 0 {
            if x.norm_more && x.norm != 4 {
                self.sticky |= i.acc.value & if x.coarse { 15 } else { 1 } != 0;
                if x.norm == 1 {
                    self.cross_groups = self.cross_groups.wrapping_add(1) & 7;
                }
            } else if !x.norm_more && x.norm == 1 && x.cross_final_q > self.qmax {
                self.qmax = x.cross_final_q;
            }
        }
    }
}
