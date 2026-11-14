//! Actual four-phase R18 reciprocal, S17 attribute and analytic LOD datapath.
pub mod lod;
pub mod math;
use super::{prepare::Dsp, require, vertex::Bits, ContractError};
use math::{Multiply, Pair, Window, WindowInput};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub read_bank: bool,
    pub write_bank: bool,
    pub coefficient_bwe: bool,
    pub coefficient_cwe: bool,
    pub coefficient_bwa: u8,
    pub coefficient_cwa: u8,
    pub coefficient_bwd: Bits,
    pub coefficient_cwd: Bits,
    pub sample_valid: bool,
    pub sample_pid: u8,
    pub sample_cover: bool,
    pub sample_anchor: bool,
    pub sample_h: u32,
    pub cfg_valid: bool,
    pub cfg_z_bias: i8,
    pub cfg_far: u32,
    pub cfg_rgb: bool,
    pub cfg_shifts: [u8; 3],
    pub param_valid: bool,
    pub param_index: u8,
    pub param_bank: bool,
    pub param_data: Bits,
    pub cfg_tex_u: u8,
    pub cfg_tex_v: u8,
    pub cfg_max_mip: u8,
    pub cfg_bias: i8,
    pub quad_end_valid: bool,
    pub quad_end_qid: u8,
    pub quad_end_mask: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub phase: u8,
    pub busy: bool,
    pub result_we: bool,
    pub result_address: u8,
    pub result_data: Bits,
    pub depth_valid: bool,
    pub depth_pid: u8,
    pub depth_z16: u16,
    pub depth_pass: bool,
    pub lod_valid: bool,
    pub lod_id: u8,
    pub lod_value: u8,
    pub tap_valid: bool,
    pub tap_pid: u8,
    pub tap_index: u8,
    pub tap_a: Bits,
    pub tap_b: Bits,
    pub table_read: bool,
    pub table_address: u16,
    pub h_write: bool,
    pub h_address: u8,
    pub aligned_h: u32,
    pub mul_a: u32,
    pub mul_b: u32,
    pub corrected_r: u32,
    pub wide_a: Bits,
    pub lod: lod::Output,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub phase: u8,
    pub valid_pipe: u8,
    pub cover_pipe: u8,
    pub anchor_pipe: u8,
    pub pid: [u8; 4],
    pub z_bias: i8,
    pub rgb: bool,
    pub shifts: [u8; 3],
    pub d_acc: u32,
    pub d_zero: bool,
    pub negative_prefix: bool,
    pub fraction: u16,
    pub seed: u32,
    pub r: u32,
    pub weight0: u32,
    pub weight1: u32,
    pub clipped0: u32,
    pub hm0: u32,
    pub hm1: u32,
    pub hm_exponent: u8,
    pub hm_zero: bool,
    pub z_shift_early: i16,
    pub z_shift: i16,
    pub z_dzero_early: bool,
    pub z_dzero: bool,
    pub z_new: u16,
    pub z_early: u16,
    pub z_new_pass: bool,
    pub z_early_pass: bool,
    pub wide_valid_pipe: u8,
    pub wide_tag0: u8,
    pub wide_tag1: u8,
    pub log_return_valid: bool,
    pub log_return_tag: u8,
    pub param_row: u8,
}
#[derive(Clone, Debug)]
pub struct Compute {
    pub state: State,
    pub lod: lod::Lod,
    pub window: Window,
    pub multiply: Multiply,
    pub pair: [Pair; 2],
    pub wide: Dsp,
    pub numeric: [Bits; 260],
    pub table_q: Bits,
    pub params: [[Bits; 16]; 2],
    pub anchors: [Bits; 4],
    pub anchor_q: Bits,
}
impl Default for Compute {
    fn default() -> Self {
        let words = crate::frontend::interpolation_constants();
        let numeric = std::array::from_fn(|a| {
            if a < 256 {
                Bits::new(words[a], 36)
            } else {
                Bits::default()
            }
        });
        Self {
            state: State::default(),
            lod: lod::Lod::default(),
            window: Window::default(),
            multiply: Multiply::default(),
            pair: std::array::from_fn(|_| Pair::default()),
            wide: Dsp::default(),
            numeric,
            table_q: Bits::new(0, 36),
            params: [[Bits::default(); 16]; 2],
            anchors: [Bits::default(); 4],
            anchor_q: Bits::new(0, 18),
        }
    }
}
#[derive(Clone, Copy)]
struct Eval {
    o: Output,
    li: lod::Input,
    wi: WindowInput,
    z16: u16,
    numerical_pass: bool,
    launch: bool,
    param_read: bool,
}
impl Compute {
    fn evaluate(&self, i: Input) -> Eval {
        let s = &self.state;
        let phase = s.phase;
        let bit = |v: u8, n: u8| v & (1u8 << n) != 0u8;
        let launch = i.sample_valid && phase == 0 && !i.reset;
        let output_shift = ((s.z_shift as u16 & 63).wrapping_sub(15) & 63) as u8;
        let align = if phase == 3 {
            output_shift & 31
        } else {
            s.shifts[phase as usize]
        };
        let shift_input = if phase == 3 {
            (self.wide.result.value >> 23) & 0x1ffffff
        } else {
            u64::from(i.sample_h & 0x3fffffff)
        };
        let shifted = (shift_input >> align) as u32;
        let depth_word = if s.z_shift >= 48 || output_shift & 32 != 0 {
            0
        } else {
            shifted & 0x1ffffff
        };
        let z16 = depth_word as u16;
        let numerical_pass = bit(s.cover_pipe, 2)
            && !s.z_dzero
            && !(s.z_shift < 15 || (s.z_shift == 15 && self.wide.result.value & (1 << 47) != 0))
            && (s.z_shift < 47 || s.z_shift == 47 && self.wide.result.value & (1 << 47) != 0)
            && depth_word >> 16 == 0
            && z16 <= ((i.cfg_far >> 8) & 65535) as u16;
        let correction_inc = self.multiply.result & 256 != 0
            && (self.multiply.result & 255 != 0 || self.multiply.result & 512 != 0);
        let q = (self.multiply.result >> 9) & 0x3ffff;
        let corrected_r =
            ((u64::from(s.seed << 1).wrapping_sub((q << 1) | u64::from(correction_inc)) & 0x7ffff)
                >> 1) as u32;
        let (mul_a, mul_b) = match phase {
            0 => (corrected_r, s.hm0),
            1 => (s.r, s.hm1),
            2 => (u32::from(s.fraction), self.table_q.value as u32 & 511),
            _ => (0, 0),
        };
        let attr = [
            self.pair[0].result.slice(16, 18),
            self.pair[1].result.slice(16, 18),
        ];
        let result_index = match phase {
            2 => 0,
            3 => 1,
            _ => 2,
        };
        let result_we = bit(s.valid_pipe, 3) && matches!(phase, 0 | 2 | 3) && !i.reset;
        let result_data = match phase {
            2 => Bits::concat(
                Bits::new(u64::from(s.z_new_pass) << 11, 12),
                Bits::concat(attr[1].slice(0, 12), attr[0].slice(0, 12), 12),
                24,
            ),
            3 => Bits::concat(
                Bits::new(0, if s.rgb { 16 } else { 6 }),
                Bits::concat(
                    if s.rgb { attr[1].slice(0, 8) } else { attr[1] },
                    attr[0].slice(0, 12),
                    12,
                ),
                if s.rgb { 20 } else { 30 },
            ),
            0 => Bits::concat(
                Bits::new(u64::from(s.z_new), if s.rgb { 20 } else { 18 }),
                if s.rgb {
                    Bits::concat(attr[1].slice(0, 8), attr[0].slice(0, 8), 8)
                } else {
                    attr[0]
                },
                if s.rgb { 16 } else { 18 },
            ),
            _ => Bits::new(0, 36),
        };
        let reciprocal_valid = phase == 2 && bit(s.valid_pipe, 1) && !i.reset;
        let li = lod::Input {
            reset: i.reset,
            ce: i.ce,
            phase,
            read_bank: i.read_bank,
            write_bank: i.write_bank,
            coefficient_bwe: i.coefficient_bwe,
            coefficient_cwe: i.coefficient_cwe,
            coefficient_bwa: i.coefficient_bwa,
            coefficient_cwa: i.coefficient_cwa,
            coefficient_bwd: i.coefficient_bwd,
            coefficient_cwd: i.coefficient_cwd,
            cfg_valid: i.cfg_valid,
            cfg_tex_u: i.cfg_tex_u,
            cfg_tex_v: i.cfg_tex_v,
            cfg_max_mip: i.cfg_max_mip,
            cfg_bias: i.cfg_bias,
            cfg_rgb: i.cfg_rgb,
            quad_end_valid: i.quad_end_valid,
            quad_end_qid: i.quad_end_qid,
            quad_end_mask: i.quad_end_mask,
            reciprocal_valid,
            reciprocal_pid: s.pid[1],
            reciprocal_anchor: bit(s.anchor_pipe, 1),
            reciprocal_log_q8: ((u16::from(self.window.d_exponent) << 8)
                + ((self.table_q.value >> 27) & 255) as u16)
                & 8191,
            tap_valid: result_we,
            tap_pid: s.pid[3],
            tap_index: result_index,
            tap_anchor: bit(s.anchor_pipe, 3),
            tap_a: attr[0],
            tap_b: attr[1],
            wide_result_valid: bit(s.wide_valid_pipe, 1),
            wide_result_tag: s.wide_tag1,
            wide_result_data: self.wide.result,
            norm_valid: self.window.lod_valid,
            norm_zero: self.window.lod_zero,
            norm_exp: self.window.lod_exponent,
            norm_index: ((self.window.mant >> 11) & 63) as u8,
            norm_pending: self.window.pending(),
            log_result_valid: s.log_return_valid,
            log_result_tag: s.log_return_tag,
            log_result_data: ((self.table_q.value >> 27) & 255) as u8,
        };
        let lo = self.lod.outputs(li);
        let log_take = lo.log_valid && phase == 3 && !i.reset;
        let table_read = log_take
            || (phase == 0 && bit(s.valid_pipe, 0))
            || (matches!(phase, 1 | 2) && bit(s.valid_pipe, 1));
        let table_address = match phase {
            0 => 256 + u16::from(s.pid[0] & 1) * 2,
            1 => ((self.window.mant >> 9) & 255) as u16,
            2 => 257 + u16::from(s.pid[1] & 1) * 2,
            _ => u16::from(lo.log_index) * 4,
        };
        let normalized_h = if bit(s.cover_pipe, 1) {
            self.table_q.value as u32
        } else {
            0
        };
        let wi = WindowInput {
            reset: i.reset,
            ce: i.ce,
            phase,
            d_valid: bit(s.valid_pipe, 0),
            h_valid: bit(s.valid_pipe, 1),
            d: s.d_acc.max(1),
            h: normalized_h,
            lod_valid: lo.norm_input_valid,
            lod_value: lo.norm_input_value,
        };
        let o = Output {
            phase,
            busy: s.valid_pipe != 0 || lo.busy,
            result_we,
            result_address: s.pid[3] * 4 + result_index,
            result_data,
            depth_valid: phase == 1 && bit(s.valid_pipe, 3) && !i.reset,
            depth_pid: s.pid[3],
            depth_z16: s.z_early,
            depth_pass: s.z_early_pass,
            lod_valid: lo.lod_valid,
            lod_id: lo.lod_id,
            lod_value: lo.lod_value,
            tap_valid: result_we,
            tap_pid: s.pid[3],
            tap_index: result_index,
            tap_a: attr[0],
            tap_b: attr[1],
            table_read,
            table_address,
            h_write: launch || (phase == 1 && bit(s.valid_pipe, 0)),
            h_address: ((if phase == 0 { i.sample_pid } else { s.pid[0] }) & 1) * 2
                + u8::from(phase == 1),
            aligned_h: shifted,
            mul_a,
            mul_b,
            corrected_r,
            wide_a: if phase == 1 {
                Bits::new(u64::from(s.r), 18)
            } else {
                self.anchor_q
            },
            lod: lo,
        };
        Eval {
            o,
            li,
            wi,
            z16,
            numerical_pass,
            launch,
            param_read: (matches!(phase, 3 | 0) && bit(s.valid_pipe, 2))
                || (phase == 1 && bit(s.valid_pipe, 3)),
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.evaluate(i).o
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let e = self.evaluate(i);
        let o = e.o;
        if i.reset {
            self.state = State::default();
            self.table_q = Bits::new(0, 36);
            self.anchor_q = Bits::new(0, 18);
            self.lod.clock(e.li)?;
            self.window.clock(e.wi);
            self.multiply.clock(true, i.ce, 0, 0);
            for p in &mut self.pair {
                p.clock(true, i.ce, [0; 2], Bits::default());
            }
            self.wide.clock(
                true,
                i.ce,
                Bits::default(),
                false,
                Bits::default(),
                Bits::default(),
            );
            return Ok(());
        }
        if !i.ce {
            return Ok(());
        }
        let s = self.state.clone();
        let bit = |v: u8, n: u8| v & (1u8 << n) != 0u8;
        let check = |ok, rule| require(ok, "compute", rule);
        check(
            !i.sample_valid || s.phase == 0,
            "GPU contract: compute_error (sample phase)",
        )?;
        check(
            !i.quad_end_valid
                || (s.phase == 2
                    && bit(s.valid_pipe, 0)
                    && s.pid[0] & 3 == 3
                    && s.pid[0] >> 2 == i.quad_end_qid),
            "GPU contract: compute_error (quad end)",
        )?;
        check(
            !(s.phase == 2
                && bit(s.valid_pipe, 0)
                && i.sample_cover
                && (s.negative_prefix || i.sample_h & (1 << 29) != 0)),
            "GPU contract: compute_error (covered negative H)",
        )?;
        check(
            !(e.launch && (i.cfg_valid || (i.param_valid && i.write_bank == i.read_bank))),
            "GPU contract: compute_error (launch during configuration)",
        )?;
        check(
            !(i.param_valid && o.busy && i.write_bank == i.read_bank),
            "attribute write to live context",
        )?;
        check(
            !(i.cfg_valid && o.busy),
            "configuration commit before pixel drain",
        )?;
        check(
            !o.lod.operand_write
                || (o.lod.operand_data.is_known(18) && o.lod.operand_data.value & (1 << 17) == 0),
            "anchor does not fit unsigned17",
        )?;
        check(
            !(o.h_write && o.table_read && o.table_address == 256 + u16::from(o.h_address)),
            "numeric RAM cross-port address collision",
        )?;
        check(!e.wi.lod_valid || s.phase == 1, "LOD absolute phase")?;
        check(!self.window.abs_valid || s.phase == 2, "LOD window phase")?;
        if o.table_read {
            check(
                usize::from(o.table_address) < 260,
                "numeric RAM address beyond physical table",
            )?;
        }
        if e.wi.h_valid && s.cover_pipe & 2 != 0 {
            check(self.table_q.is_known(32), "unknown H at window")?;
        }
        if e.wi.lod_valid {
            check(e.wi.lod_value.is_known(36), "unknown derivative at window")?;
        }
        let pq = std::array::from_fn::<_, 2, _>(|n| {
            self.params[n][usize::from(u8::from(i.read_bank) * 4 + s.param_row)]
        });
        // All RAM reads and arithmetic sample old cells, even on a write edge.
        let table_next = if o.table_read {
            self.numeric[o.table_address as usize]
        } else {
            self.table_q
        };
        let anchor_next = if o.lod.operand_read {
            self.anchors[usize::from(o.lod.operand_address & 3)]
        } else {
            self.anchor_q
        };
        self.advance(i, e, s.clone());
        self.lod.clock(e.li)?;
        self.window.clock(e.wi);
        self.multiply.clock(false, true, o.mul_a, o.mul_b);
        for (n, p) in self.pair.iter_mut().enumerate() {
            p.clock(false, true, [s.weight0, s.weight1], pq[n]);
        }
        self.wide
            .clock(false, true, o.wide_a, false, o.lod.wide_b, o.lod.wide_c);
        self.table_q = table_next;
        self.anchor_q = anchor_next;
        if o.h_write {
            self.numeric[256 + o.h_address as usize] = Bits::new(u64::from(o.aligned_h), 36);
        }
        if o.lod.operand_write {
            self.anchors[usize::from(o.lod.operand_write_address & 3)] =
                o.lod.operand_data.slice(0, 18);
        }
        if i.param_valid {
            self.params[usize::from(i.param_bank)]
                [usize::from(u8::from(i.write_bank) * 4 + (i.param_index & 3))] =
                i.param_data.slice(0, 51);
        }
        Ok(())
    }
    fn advance(&mut self, i: Input, e: Eval, s: State) {
        let raw_weight = ((self.multiply.result >> 17) & 0x7ffff) as u32;
        let bit = |v: u8, n: u8| v & (1u8 << n) != 0u8;
        let n = &mut self.state;
        let phase = s.phase;
        n.phase = (phase + 1) & 3;
        n.wide_valid_pipe = ((s.wide_valid_pipe << 1) | u8::from(e.o.lod.wide_valid)) & 3;
        n.wide_tag1 = s.wide_tag0;
        if e.o.lod.wide_valid {
            n.wide_tag0 = e.o.lod.wide_tag;
        }
        n.log_return_valid = e.o.lod.log_valid && phase == 3;
        if n.log_return_valid {
            n.log_return_tag = e.o.lod.log_tag;
        }
        if i.cfg_valid {
            n.z_bias = i.cfg_z_bias;
            n.rgb = i.cfg_rgb;
            n.shifts = i.cfg_shifts.map(|v| v & 31);
        }
        if e.param_read {
            n.param_row = match phase {
                3 => 0,
                0 => 1,
                _ => 2,
            };
        }
        if phase == 0 {
            n.valid_pipe = ((s.valid_pipe << 1) | u8::from(e.launch)) & 15;
            n.cover_pipe = (s.cover_pipe << 1) & 15;
            n.anchor_pipe = (s.anchor_pipe << 1) & 15;
            n.pid[3] = s.pid[2];
            n.pid[2] = s.pid[1];
            n.pid[1] = s.pid[0];
            if e.launch {
                n.pid[0] = i.sample_pid & 31;
                n.d_acc = e.o.aligned_h;
                n.negative_prefix = i.sample_h & (1 << 29) != 0;
            }
            if bit(s.valid_pipe, 1) {
                n.hm1 = self.window.mant & 0x1ffff;
                n.r = e.o.corrected_r;
                n.z_shift_early = i16::from(s.z_bias) + i16::from(s.hm_exponent);
                n.z_dzero_early = s.hm_zero;
            }
        }
        if phase == 1 {
            if bit(s.valid_pipe, 0) {
                n.d_acc = s.d_acc.wrapping_add(e.o.aligned_h);
                n.negative_prefix = s.negative_prefix || i.sample_h & (1 << 29) != 0;
            }
            if bit(s.valid_pipe, 1) {
                n.fraction = (self.window.mant & 511) as u16;
            }
            if bit(s.valid_pipe, 3) {
                n.z_new = s.z_early;
                n.z_new_pass = s.z_early_pass;
            }
        }
        if phase == 2 {
            if bit(s.valid_pipe, 0) {
                n.d_acc = if i.sample_cover {
                    s.d_acc.wrapping_add(e.o.aligned_h)
                } else {
                    1
                };
                n.cover_pipe = (s.cover_pipe & !1) | u8::from(i.sample_cover);
                n.anchor_pipe = (s.anchor_pipe & !1) | u8::from(i.sample_anchor && i.sample_cover);
            }
            if bit(s.valid_pipe, 1) {
                n.seed = ((self.table_q.value >> 9) & 0x3ffff) as u32;
                n.hm0 = self.window.mant & 0x1ffff;
                n.hm_exponent = self.window.d_exponent;
                n.hm_zero = s.d_zero;
            }
            if bit(s.valid_pipe, 2) {
                n.clipped0 = raw_weight.min(65536);
            }
            n.z_shift = s.z_shift_early;
            n.z_dzero = s.z_dzero_early;
        }
        if phase == 3 {
            if bit(s.valid_pipe, 2) {
                n.z_early = e.z16;
                n.z_early_pass = e.numerical_pass;
            }
            if bit(s.valid_pipe, 0) {
                n.d_zero = s.d_acc == 0;
            }
            if bit(s.valid_pipe, 2) {
                n.weight0 = s.clipped0;
                n.weight1 = raw_weight.min(65536 - s.clipped0);
            }
        }
    }
}
