//! Complete private preparation datapath; the runtime R/I context switch is external.
use super::super::{require, vertex::Bits, ContractError};
use super::{
    control,
    storage::{Dsp, Ports, Storage},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Params {
    /// Q16.16 in the selected depth units; native encoding maps physical far.
    pub far_z: u32,
    pub depth_shift: u8,
    pub tex_u: u8,
    pub tex_v: u8,
    pub max_mip: u8,
    pub bias: i8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub start: bool,
    pub execute_grant: bool,
    pub descriptor: u32,
    pub raw_valid: bool,
    pub raw_index: u8,
    pub raw_data: Bits,
    pub params: Params,
    pub replay_read: bool,
    pub replay_address: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub busy: bool,
    pub import_busy: bool,
    pub raw_ready: bool,
    pub done: bool,
    pub operation: u8,
    pub expected: u8,
    pub context_ready: bool,
    pub cfg_apply: bool,
    pub geo_write: bool,
    pub geo_address: u8,
    pub geo_data: Bits,
    pub z_bias: i8,
    pub shifts: [u8; 3],
    pub rgb: bool,
    pub far_z: u32,
    pub param_valid: bool,
    pub param_index: u8,
    pub param_bank: bool,
    pub param_data: Bits,
    pub bound_valid: bool,
    pub bound_empty: bool,
    pub xlo: u8,
    pub xhi: u8,
    pub ylo: u8,
    pub yhi: u8,
    pub ports: Ports,
    pub dsp_a: Bits,
    pub dsp_signed: bool,
    pub dsp_b: Bits,
    pub dsp_c: Bits,
}
#[derive(Clone, Debug)]
pub struct State {
    pub edge_no: u8,
    pub field: u8,
    pub edge_exp: u8,
    pub exp: [u8; 3],
    pub common_e: u8,
    pub raw_exponent: u8,
    pub depth_mantissa: Bits,
    pub delta_e: u8,
    pub sigma: bool,
    pub rgb: bool,
    pub seed_x: u8,
    pub seed_y: u8,
    pub read_valid: bool,
    pub read_exp: u8,
    pub quant_pipe: u8,
    pub quant_edge: u8,
    pub quant_field: u8,
    pub negative0: bool,
    pub negative1: bool,
    pub fraction: u8,
    pub quant_tail: bool,
    pub attribute_base: Bits,
    pub difference0: Bits,
}
impl Default for State {
    fn default() -> Self {
        Self {
            edge_no: 0,
            field: 0,
            edge_exp: 0,
            exp: [0; 3],
            common_e: 0,
            raw_exponent: 0,
            depth_mantissa: Bits::new(0, 31),
            delta_e: 0,
            sigma: false,
            rgb: false,
            seed_x: 0,
            seed_y: 0,
            read_valid: false,
            read_exp: 0,
            quant_pipe: 0,
            quant_edge: 0,
            quant_field: 0,
            negative0: false,
            negative1: false,
            fraction: 0,
            quant_tail: false,
            attribute_base: Bits::new(0, 16),
            difference0: Bits::new(0, 17),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Prepare {
    pub control: control::Control,
    pub state: State,
    pub storage: Storage,
    pub dsp: Dsp,
}
struct Eval {
    co: control::Output,
    ci: control::Input,
    input_phase: bool,
    collecting: bool,
    math: bool,
    take: bool,
    is_c: bool,
    next_exp: u8,
    edge_capture: bool,
    quant_read: bool,
    canonical: Bits,
    component: Bits,
    difference: Bits,
}
fn signed(value: u64, width: u32) -> i64 {
    ((value << (64 - width)) as i64) >> (64 - width)
}
fn zero_extend(value: Bits, from: u32, to: u32) -> Bits {
    Bits::concat(Bits::new(0, to - from), value.slice(0, from), from)
}
fn subtract(a: Bits, b: Bits, width: u32) -> Bits {
    if a.is_known(width) && b.is_known(width) {
        Bits::new(a.value.wrapping_sub(b.value), width)
    } else {
        Bits::default()
    }
}
impl Prepare {
    fn evaluate(&self, i: Input) -> (Output, Eval) {
        let s = &self.state;
        let active = self.control.regs.active;
        let op = self.control.outputs(control::Input::default()).operation;
        let input_phase = op >= 48;
        let expected = op & 15;
        let collecting = active && input_phase && expected < 9;
        let commit = active && op == 23;
        let finish = active && op == 24;
        let math = active && !input_phase && !commit && !finish;
        let ready = !i.reset && active && input_phase;
        let take = ready && i.raw_valid;
        let raw_kind = ((self.control.regs.u >> 6) & 7) as u8;
        let is_c = raw_kind & 1 != 0;
        let raw_value = if is_c {
            i.raw_data.slice(0, 38)
        } else {
            i.raw_data.slice(0, 27).extend(27, 38)
        };
        let magnitude = signed(raw_value.value, 38).unsigned_abs();
        let bitlen = 64 - magnitude.leading_zeros();
        let need = bitlen.saturating_sub(if is_c { 27 } else { 17 }) as u8;
        let next_exp = need.max(s.edge_exp);
        let edge_capture = collecting && take && i.raw_index == expected && is_c;
        let quant_read = edge_capture || s.field != 0;
        let shifts = s.exp.map(|e| {
            let delta = s.common_e.wrapping_sub(e) & 63;
            if delta & 32 != 0 {
                31
            } else {
                delta & 31
            }
        });
        let ci = control::Input {
            reset: i.reset,
            ce: i.ce,
            start: i.start && !active,
            input_take: take,
            normalize_wait: !i.execute_grant,
            rgb: s.rgb,
            seed_sign: (s.seed_y << 3) | s.seed_x,
            shifts,
            b: self.storage.bq(),
            result: self.dsp.result,
        };
        let co = self.control.outputs(ci);
        let c = self.storage.cq();
        let signed_attribute = co.attr_index == 0 || (co.attr_index == 1 && !co.attr_bank);
        let normal_slice = match (co.attr_index & 1, co.attr_bank) {
            (0, false) => 0,
            (0, true) => 12,
            _ => 24,
        };
        let rgb_slice = if co.attr_index & 2 == 0 {
            0
        } else if !co.attr_bank {
            8
        } else {
            16
        };
        let component = if signed_attribute {
            c.slice(normal_slice, 12).extend(12, 16)
        } else if s.rgb {
            zero_extend(c.slice(rgb_slice, 8), 8, 16)
        } else if co.attr_index & 2 != 0 && co.attr_bank {
            Bits::new(0, 16)
        } else if co.attr_index & 2 != 0 {
            c.slice(16, 16)
        } else {
            c.slice(0, 16)
        };
        let base = if signed_attribute {
            s.attribute_base.extend(16, 17)
        } else {
            zero_extend(s.attribute_base, 16, 17)
        };
        let current = if signed_attribute {
            component.extend(16, 17)
        } else {
            zero_extend(component, 16, 17)
        };
        let difference = subtract(current, base, 17);
        let param_data = Bits::concat(Bits::concat(base, difference, 17), s.difference0, 17);
        let canonical = self.dsp.result.slice(7, 28).extend(28, 36);
        let geo_write = !i.reset && s.quant_pipe & 2 != 0;
        let geo_address = (s.quant_field << 2) | s.quant_edge;
        let geo_data = if s.quant_field == 2 {
            canonical.slice(0, 30)
        } else {
            Bits::concat(canonical.slice(0, 29), Bits::new(0, 1), 1)
        };
        let magnitude = if canonical.is_known(28) {
            Bits::new(
                if canonical.value & (1 << 27) != 0 {
                    0u64.wrapping_sub(canonical.value) & 0x1ffff
                } else {
                    canonical.value & 0x1ffff
                },
                17,
            )
        } else {
            Bits::default()
        };
        let factor_index = (co.attr_index << 1) | u8::from(co.attr_bank);
        let uv_write = !s.rgb
            && matches!(factor_index, 3 | 4)
            && (co.attr0_take || co.attr1_take || co.attr_send);
        let vertex = if co.attr0_take {
            2
        } else if co.attr1_take {
            0
        } else {
            1
        };
        let operand_write = !i.reset
            && if math {
                uv_write
            } else {
                geo_write && s.quant_field < 2
            };
        let operand_address = if math {
            (if co.attr_index == 1 { 1 } else { 4 }) + vertex
        } else if s.quant_field == 1 {
            9 + s.quant_edge
        } else if s.quant_edge == 0 {
            0
        } else {
            6 + s.quant_edge
        };
        let cfg_apply = commit && !i.reset && i.execute_grant && !co.bound_empty;
        let mut ports = Ports {
            reset: i.reset,
            ce: i.ce,
            a_write: operand_write,
            a_write_address: operand_address,
            a_data: if math {
                zero_extend(component, 16, 18)
            } else {
                zero_extend(magnitude, 17, 18)
            },
            a_read: (math && co.a_read) || quant_read,
            a_address: if math {
                co.a_address
            } else {
                96 | (u8::from(s.sigma) << 3)
                    | if edge_capture {
                        next_exp & 7
                    } else {
                        s.read_exp & 7
                    }
            },
            b_write: geo_write,
            b_write_address: (if s.quant_field == 0 {
                6
            } else if s.quant_field == 1 {
                9
            } else {
                12
            }) + s.quant_edge,
            b_data: canonical,
            b_read: false,
            b_address: co.b_address,
            c_write: active && input_phase && take && expected != 9,
            c_write_address: co.import_row,
            c_data: Bits::concat(
                if collecting {
                    raw_value.extend(38, 48).slice(36, 12)
                } else {
                    Bits::new(0, 12)
                },
                i.raw_data.slice(0, 36),
                36,
            ),
            c_read: quant_read,
            c_address: 8
                | ((if s.field == 0 {
                    s.edge_no & 1
                } else {
                    u8::from(s.edge_no & 1 == 0)
                }) << 2)
                | s.field,
        };
        if math {
            ports.b_write = co.b_write;
            ports.b_write_address = co.b_write_address;
            ports.b_data = co.b_data;
            ports.c_write = co.c_write;
            ports.c_write_address = co.c_write_address;
            ports.c_data = co.c_data;
            ports.b_read = co.b_read;
            ports.c_read = co.c_read;
            ports.c_address = co.c_address;
        } else if commit {
            ports.b_write = cfg_apply;
            ports.b_write_address = 4;
            ports.b_data = zero_extend(s.depth_mantissa, 31, 36);
        }
        if i.replay_read {
            ports.b_read = true;
            ports.b_address = i.replay_address & 15;
        }
        if i.reset {
            ports.b_write = false;
            ports.c_write = false;
            ports.b_read = false;
            ports.c_read = false;
        }
        let window = if s.read_exp & 8 != 0 {
            c.slice(8, 30).extend(30, 36)
        } else {
            c.slice(0, 36)
        };
        let scale = 128i64 >> s.fraction;
        let bias = if s.quant_tail {
            if s.sigma {
                -scale
            } else {
                127 + scale
            }
        } else if s.negative0 ^ s.sigma {
            127
        } else {
            0
        };
        (
            Output {
                busy: active,
                import_busy: active && input_phase,
                raw_ready: ready,
                done: finish,
                operation: op,
                expected,
                context_ready: commit && !i.reset,
                cfg_apply,
                geo_write,
                geo_address,
                geo_data,
                z_bias: 38u8
                    .wrapping_sub(i.params.depth_shift & 3)
                    .wrapping_add(s.common_e)
                    .wrapping_sub(s.delta_e) as i8,
                shifts,
                rgb: s.rgb,
                far_z: i.params.far_z,
                param_valid: co.attr_send,
                param_index: co.attr_index,
                param_bank: co.attr_bank,
                param_data,
                bound_valid: co.bound_valid,
                bound_empty: co.bound_empty,
                xlo: co.xlo,
                xhi: co.xhi,
                ylo: co.ylo,
                yhi: co.yhi,
                ports,
                dsp_a: self.storage.aq,
                dsp_signed: !math || co.wide_signed,
                dsp_b: if math { self.storage.bq() } else { window },
                dsp_c: if math { c } else { Bits::new(bias as u64, 54) },
            },
            Eval {
                co,
                ci,
                input_phase,
                collecting,
                math,
                take,
                is_c,
                next_exp,
                edge_capture,
                quant_read,
                canonical,
                component,
                difference,
            },
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.evaluate(i).0
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let (o, e) = self.evaluate(i);
        if i.reset {
            self.control.clock(e.ci)?;
            self.storage.clock(o.ports);
            self.dsp
                .clock(true, i.ce, o.dsp_a, o.dsp_signed, o.dsp_b, o.dsp_c);
            self.state = State::default();
            return Ok(());
        }
        if !i.ce {
            return Ok(());
        }
        let check = |ok, rule| require(ok, "prepare", rule);
        check(i.params.depth_shift < 4, "depth shift encoding")?;
        if o.cfg_apply {
            check(
                i.params.far_z < 65535 * 256,
                "depth shift exceeds D16 far range",
            )?;
        }
        check(
            !(i.start && o.busy),
            "GPU contract: error (start while busy)",
        )?;
        check(
            !(e.take && i.raw_index != o.expected),
            "GPU contract: error (unexpected input index)",
        )?;
        if e.take {
            check(i.raw_data.is_known(54), "unknown T16 input")?;
        }
        if e.take && !i.start && e.collecting && e.is_c {
            check(e.next_exp <= 11, "GPU contract: error (geometry exponent)")?;
        }
        if e.take && !i.start && e.input_phase && o.expected == 9 {
            check(
                i.raw_data.value != 0 && i.raw_data.value & (1 << 53) == 0,
                "GPU contract: error (determinant)",
            )?;
        }
        if o.geo_write {
            check(e.canonical.is_known(36), "unknown geometry DSP return")?;
            let value = signed(e.canonical.value, 36);
            check(
                if self.state.quant_field < 2 {
                    (-131072..=131071).contains(&value)
                } else {
                    (-134217728..=134217727).contains(&value)
                },
                "geometry retained width",
            )?;
            check(
                !e.math && !(self.control.regs.active && o.operation == 23),
                "geometry pipeline overlaps parameter execution",
            )?;
        }
        check(
            !(i.replay_read && e.math && e.co.b_read),
            "prepare and geometry replay share PB read",
        )?;
        check(
            !(i.replay_read && o.ports.b_write && o.ports.b_address == o.ports.b_write_address),
            "PB replay overwritten before read",
        )?;
        check(
            !(o.ports.c_write
                && o.ports.c_read
                && o.ports.c_write_address == o.ports.c_address
                && !e.math),
            "streaming PC address collision",
        )?;
        check(
            !(o.ports.c_write && matches!(o.ports.c_write_address, 4 | 6 | 7)),
            "iz_params_async write to constant row",
        )?;
        check(
            !(e.math && e.co.c_write && e.co.c_write_address >= 8 && e.co.c_write_address != 11),
            "packed lifetime overwritten",
        )?;
        let s = &self.state;
        let mut n = s.clone();
        if e.co.attr0_take {
            n.attribute_base = e.component;
        }
        if e.co.attr1_take {
            n.difference0 = e.difference;
        }
        if i.start {
            n.edge_no = 0;
            n.field = 0;
            n.edge_exp = 0;
            n.exp = [0; 3];
            n.common_e = 0;
            n.sigma = i.descriptor & (1 << 18) != 0;
            n.rgb = i.descriptor & (1 << 19) != 0;
            n.read_valid = false;
            n.quant_pipe = 0;
            n.quant_edge = 0;
            n.quant_field = 0;
            n.fraction = 0;
            n.quant_tail = false;
        } else {
            if e.quant_read {
                n.field = if s.field == 2 { 0 } else { s.field + 1 };
            }
            if o.geo_write && s.quant_field < 2 {
                let sign = u8::from(s.negative1 ^ s.sigma);
                if s.quant_field == 0 {
                    n.seed_x = (s.seed_x & !(1 << s.quant_edge)) | (sign << s.quant_edge);
                } else {
                    n.seed_y = (s.seed_y & !(1 << s.quant_edge)) | (sign << s.quant_edge);
                }
            }
            if e.take {
                if e.collecting {
                    if matches!(o.expected, 0 | 3 | 6) {
                        n.raw_exponent = ((i.raw_data.value >> 48) & 63) as u8;
                    }
                    n.edge_exp = e.next_exp;
                    if e.is_c {
                        let exp = e.next_exp.wrapping_add(s.raw_exponent) & 63;
                        if s.edge_no < 2 {
                            n.exp[s.edge_no as usize] = exp;
                        } else {
                            n.exp[2] = exp;
                            n.common_e = s.exp[0].max(s.exp[1]).max(exp);
                        }
                        n.edge_exp = 0;
                        n.edge_no = (s.edge_no + 1) & 3;
                    }
                } else if o.expected == 9 {
                    n.delta_e = ((i.raw_data.value >> 31) & 127) as u8;
                    n.depth_mantissa = i.raw_data.slice(0, 31);
                }
            }
            n.read_valid = e.quant_read;
            n.quant_pipe = ((s.quant_pipe & 1) << 1) | u8::from(s.read_valid);
            if e.edge_capture {
                n.read_exp = e.next_exp;
            }
            if s.read_valid {
                let c = self.storage.cq();
                check(c.is_known(48), "unknown raw geometry PC read")?;
                n.negative0 = c.value & (1 << 47) != 0;
                n.fraction = s.read_exp & 7;
                n.quant_tail =
                    s.read_exp & 8 != 0 && c.value & (1 << 37) != 0 && c.value & 255 != 0;
            }
            n.negative1 = s.negative0;
            if s.quant_pipe & 2 != 0 {
                if s.quant_field == 2 {
                    n.quant_field = 0;
                    n.quant_edge = (s.quant_edge + 1) & 3;
                } else {
                    n.quant_field = s.quant_field + 1;
                }
            }
        }
        // Every component consumes the same pre-edge values before any bank changes.
        self.control.clock(e.ci)?;
        self.dsp
            .clock(false, true, o.dsp_a, o.dsp_signed, o.dsp_b, o.dsp_c);
        self.storage.clock(o.ports);
        self.state = n;
        Ok(())
    }
}
