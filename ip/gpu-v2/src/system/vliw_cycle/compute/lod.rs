//! Fixed four-gradient calendar and independently owned coefficient/offset banks.
use super::{
    super::{require, vertex::Bits, ContractError},
    math::signed,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub phase: u8,
    pub read_bank: bool,
    pub write_bank: bool,
    pub coefficient_bwe: bool,
    pub coefficient_cwe: bool,
    pub coefficient_bwa: u8,
    pub coefficient_cwa: u8,
    pub coefficient_bwd: Bits,
    pub coefficient_cwd: Bits,
    pub cfg_valid: bool,
    pub cfg_tex_u: u8,
    pub cfg_tex_v: u8,
    pub cfg_max_mip: u8,
    pub cfg_bias: i8,
    pub cfg_rgb: bool,
    pub quad_end_valid: bool,
    pub quad_end_qid: u8,
    pub quad_end_mask: u8,
    pub reciprocal_valid: bool,
    pub reciprocal_pid: u8,
    pub reciprocal_anchor: bool,
    pub reciprocal_log_q8: u16,
    pub tap_valid: bool,
    pub tap_pid: u8,
    pub tap_index: u8,
    pub tap_anchor: bool,
    pub tap_a: Bits,
    pub tap_b: Bits,
    pub wide_result_valid: bool,
    pub wide_result_tag: u8,
    pub wide_result_data: Bits,
    pub norm_valid: bool,
    pub norm_zero: bool,
    pub norm_exp: u8,
    pub norm_index: u8,
    pub norm_pending: bool,
    pub log_result_valid: bool,
    pub log_result_tag: u8,
    pub log_result_data: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub operand_write: bool,
    pub operand_write_address: u8,
    pub operand_data: Bits,
    pub operand_read: bool,
    pub operand_address: u8,
    pub wide_valid: bool,
    pub wide_b: Bits,
    pub wide_c: Bits,
    pub wide_tag: u8,
    pub norm_input_valid: bool,
    pub norm_input_value: Bits,
    pub log_valid: bool,
    pub log_index: u8,
    pub log_tag: u8,
    pub lod_valid: bool,
    pub lod_id: u8,
    pub lod_value: u8,
    pub busy: bool,
}
#[derive(Clone, Debug)]
pub struct Lod {
    pub arrival: bool,
    pub arrival_run: bool,
    pub calendar_valid: u8,
    pub calendar_run: u8,
    pub kernel_valid: bool,
    pub kernel_run: bool,
    pub kernel_id: u8,
    pub axis: u8,
    pub issue_live: bool,
    pub issue_tag: u8,
    pub finish_valid: u8,
    pub finish_run: u8,
    pub result_id: u8,
    pub finish_pending: bool,
    pub pending_run: bool,
    pub finish_offset: Bits,
    pub bias_base: i16,
    pub rgb: bool,
    pub log_context: u8,
    pub sampled_zero: bool,
    pub sampled_exp: u8,
    pub maximum: u16,
    pub have_gradient: bool,
    pub lod_valid: bool,
    pub lod_id: u8,
    pub lod_value: u8,
    pub b: [Bits; 16],
    pub c: [Bits; 16],
    pub offsets: [Bits; 16],
    pub b_address: u8,
    pub c_address: u8,
    pub offset_address: u8,
    pub end_id: u8,
    pub anchor_known: u8,
    pub anchor_stamp: [u8; 4],
    pub dlog_known: u8,
}
impl Default for Lod {
    fn default() -> Self {
        let mut c = [Bits::default(); 16];
        for a in [4, 6, 12] {
            c[a] = Bits::new(0, 36);
        }
        c[7] = Bits::new(131071, 36);
        Self {
            arrival: false,
            arrival_run: false,
            calendar_valid: 0,
            calendar_run: 0,
            kernel_valid: false,
            kernel_run: false,
            kernel_id: 0,
            axis: 0,
            issue_live: false,
            issue_tag: 0,
            finish_valid: 0,
            finish_run: 0,
            result_id: 0,
            finish_pending: false,
            pending_run: false,
            finish_offset: Bits::new(0, 16),
            bias_base: -3712,
            rgb: false,
            log_context: 0,
            sampled_zero: true,
            sampled_exp: 0,
            maximum: 0,
            have_gradient: false,
            lod_valid: false,
            lod_id: 0,
            lod_value: 0,
            b: [Bits::default(); 16],
            c,
            offsets: [Bits::default(); 16],
            b_address: 0,
            c_address: 0,
            offset_address: 0,
            end_id: 0,
            anchor_known: 0,
            anchor_stamp: [0; 4],
            dlog_known: 0,
        }
    }
}
impl Lod {
    pub fn outputs(&self, i: Input) -> Output {
        Output {
            operand_write: i.tap_valid
                && i.tap_anchor
                && !self.rgb
                && matches!(i.tap_index, 1 | 2)
                && !i.reset,
            operand_write_address: 4 | ((i.tap_pid >> 2 & 1) << 1) | u8::from(i.tap_index == 2),
            operand_data: if i.tap_index == 1 { i.tap_b } else { i.tap_a },
            operand_read: i.phase == 2 && self.kernel_valid && self.kernel_run && !i.reset,
            operand_address: 4 | ((self.kernel_id & 1) << 1) | (self.axis >> 1),
            wide_valid: self.issue_live && i.phase == 3 && !i.reset,
            wide_b: self.b[self.b_address as usize],
            wide_c: self.c[self.c_address as usize].extend(36, 54),
            wide_tag: self.issue_tag,
            norm_input_valid: i.wide_result_valid,
            norm_input_value: i.wide_result_data.slice(0, 36),
            log_valid: i.norm_valid && i.phase == 3 && !i.reset,
            log_index: i.norm_index,
            log_tag: self.log_context,
            lod_valid: self.lod_valid,
            lod_id: self.lod_id,
            lod_value: self.lod_value,
            busy: self.arrival
                || self.calendar_valid != 0
                || self.kernel_valid
                || self.issue_live
                || self.finish_valid != 0
                || i.norm_pending
                || i.wide_result_valid
                || i.log_result_valid
                || self.finish_pending,
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        if i.reset {
            let old = self.clone();
            *self = Self::default();
            self.b = old.b;
            self.c = old.c;
            self.offsets = old.offsets;
            return Ok(());
        }
        if !i.ce {
            return Ok(());
        }
        let o = self.outputs(i);
        let old = self.clone();
        let check = |ok, rule| require(ok, "lod", rule);
        check(
            !(i.coefficient_cwe && matches!(i.coefficient_cwa & 15, 4 | 6 | 7 | 12)),
            "iz_params_async write to constant row",
        )?;
        let meta_write = i.reciprocal_valid && i.reciprocal_anchor && !old.rgb;
        let meta_read =
            i.phase == 3 && old.finish_valid & 2 != 0 && old.finish_run & 2 != 0 && !i.reset;
        check(
            !(i.cfg_valid && o.busy),
            "fixed configuration commit before LOD drain",
        )?;
        check(
            !i.quad_end_valid || i.phase == 2,
            "GPU contract: error (quad end phase)",
        )?;
        check(
            !i.quad_end_valid || i.quad_end_qid == old.end_id,
            "fixed LOD address order",
        )?;
        let anchor_row = usize::from(o.operand_address & 3);
        check(
            !o.operand_read
                || (old.anchor_known & (1 << anchor_row) != 0
                    && old.anchor_stamp[anchor_row] == old.kernel_id),
            "anchor missing or overwritten",
        )?;
        check(
            !(o.operand_read && o.operand_write && o.operand_address == o.operand_write_address),
            "anchor read/write address collision",
        )?;
        check(
            !meta_read || old.dlog_known & (1 << old.result_id) != 0,
            "Dlog missing",
        )?;
        check(
            !i.wide_result_valid
                || (i.wide_result_data.is_known(54)
                    && signed(i.wide_result_data.value, 54)
                        == signed(i.wide_result_data.value, 36)),
            "LOD derivative overflow signed36",
        )?;
        check(
            !(i.phase == 3 && old.calendar_valid & 4 != 0 && old.kernel_valid),
            "fixed LOD kernel overlap",
        )?;
        check(!i.norm_valid || i.phase == 3, "LOD normalization phase")?;
        check(!i.log_result_valid || i.phase == 0, "LOD lookup phase")?;
        check(
            !(i.phase == 0 && old.finish_valid & 2 != 0 && old.finish_run & 2 != 0)
                || (i.log_result_valid && i.log_result_tag == (old.result_id << 2 | 3)),
            "LOD completion calendar differs from arithmetic",
        )?;
        self.lod_valid = false;
        self.finish_pending = false;
        if old.finish_pending {
            self.lod_valid = true;
            self.lod_id = old.result_id;
            self.result_id = (old.result_id + 1) & 7;
            self.lod_value = if !old.pending_run || !old.have_gradient {
                0
            } else {
                check(
                    old.finish_offset.is_known(16),
                    "unknown LOD offset at completion",
                )?;
                let value = (old.maximum as i16).wrapping_add(old.finish_offset.value as i16);
                if value < 0 {
                    0
                } else if value > ((i.cfg_max_mip & 31) as i16) << 8 {
                    (i.cfg_max_mip & 31) << 3
                } else {
                    ((value as u16 >> 5) & 255) as u8
                }
            };
        }
        if i.wide_result_valid {
            self.log_context = i.wide_result_tag & 31;
        }
        if i.cfg_valid {
            self.bias_base = (-3712i16).wrapping_add(i16::from(i.cfg_bias) << 4);
            self.rgb = i.cfg_rgb;
        }
        if i.phase == 2 {
            self.arrival = i.quad_end_valid && !old.rgb;
            self.arrival_run = i.quad_end_mask != 0;
            if i.quad_end_valid && old.rgb {
                self.lod_valid = true;
                self.lod_id = old.result_id;
                self.lod_value = 0;
                self.kernel_id = (old.kernel_id + 1) & 7;
                self.result_id = (old.result_id + 1) & 7;
            }
        }
        if i.phase == 3 {
            self.calendar_valid = ((old.calendar_valid << 1) | u8::from(old.arrival)) & 7;
            self.calendar_run = ((old.calendar_run << 1) | u8::from(old.arrival_run)) & 7;
            self.arrival = false;
            if old.calendar_valid & 4 != 0 {
                self.kernel_valid = true;
                self.kernel_run = old.calendar_run & 4 != 0;
                self.axis = 0;
            }
        }
        if i.phase == 0 {
            self.finish_valid =
                ((old.finish_valid << 1) | u8::from(old.kernel_valid && old.axis == 3)) & 3;
            self.finish_run = ((old.finish_run << 1) | u8::from(old.kernel_run)) & 3;
            if old.finish_valid & 2 != 0 {
                self.finish_pending = true;
                self.pending_run = old.finish_run & 2 != 0;
                self.finish_offset = old.offsets[old.offset_address as usize];
            }
        }
        if i.phase == 2 {
            self.issue_live = o.operand_read;
            self.issue_tag = old.kernel_id << 2 | old.axis;
            if old.kernel_valid {
                if old.axis == 3 {
                    self.kernel_valid = false;
                    self.kernel_id = (old.kernel_id + 1) & 7;
                } else {
                    self.axis = old.axis + 1;
                }
            }
        }
        if o.wide_valid {
            self.issue_live = false;
        }
        if o.log_valid {
            self.sampled_zero = i.norm_zero;
            self.sampled_exp = i.norm_exp & 63;
        }
        if i.log_result_valid {
            let first = i.log_result_tag & 3 == 0;
            let texture = if i.log_result_tag & 2 != 0 {
                i.cfg_tex_v
            } else {
                i.cfg_tex_u
            };
            let score = (((u16::from(old.sampled_exp) + u16::from(texture & 31)) << 8)
                | u16::from(i.log_result_data))
                & 0x7fff;
            let take = !old.sampled_zero && (first || !old.have_gradient || score > old.maximum);
            self.have_gradient = !old.sampled_zero || (!first && old.have_gradient);
            self.maximum = if take {
                score
            } else if first {
                0
            } else {
                old.maximum
            };
        }
        if i.phase == 0 || o.operand_read {
            self.b_address =
                (u8::from(i.read_bank) << 3) | if i.phase == 0 { 4 } else { old.axis & 1 };
            self.c_address = (u8::from(i.read_bank) << 3) | if i.phase == 0 { 4 } else { old.axis };
        }
        if meta_read {
            self.offset_address = old.result_id;
        }
        if meta_write {
            let address = i.reciprocal_pid >> 2 & 7;
            self.offsets[address as usize] = Bits::new(
                old.bias_base.wrapping_sub(i.reciprocal_log_q8 as i16) as u64,
                16,
            );
            self.dlog_known |= 1 << address;
        }
        if i.coefficient_bwe {
            self.b[usize::from(i.coefficient_bwa & 15)] = i.coefficient_bwd.slice(0, 36);
        }
        if i.coefficient_cwe {
            self.c[usize::from(i.coefficient_cwa & 15)] = i.coefficient_cwd.slice(0, 36);
        }
        if i.quad_end_valid {
            self.end_id = (old.end_id + 1) & 7;
        }
        if o.operand_write {
            let a = usize::from(o.operand_write_address & 3);
            self.anchor_known |= 1 << a;
            self.anchor_stamp[a] = i.tap_pid >> 2 & 7;
        }
        Ok(())
    }
}
