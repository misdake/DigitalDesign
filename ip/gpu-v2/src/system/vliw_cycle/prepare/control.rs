//! The actual 512x36 preparation ROM and its old-edge control registers.
use super::super::{require, vertex::Bits, ContractError};
use std::sync::OnceLock;

fn rom() -> &'static [u64; 512] {
    static WORDS: OnceLock<[u64; 512]> = OnceLock::new();
    WORDS.get_or_init(|| {
        let mut words = [0; 512];
        let mut seen = [false; 512];
        for line in include_str!("../../../frontend/vliw/prepare_program.v").lines() {
            if let Some(line) = line.strip_prefix("mem[") {
                let (index, value) = line.split_once("]=36'h").unwrap();
                let index: usize = index.parse().unwrap();
                assert!(!seen[index]);
                seen[index] = true;
                words[index] = u64::from_str_radix(value.trim_end_matches(';'), 16).unwrap();
            }
        }
        assert!(seen.into_iter().all(|x| x));
        words
    })
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub start: bool,
    pub input_take: bool,
    pub normalize_wait: bool,
    pub rgb: bool,
    pub seed_sign: u8,
    pub shifts: [u8; 3],
    pub b: Bits,
    pub result: Bits,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub operation: u8,
    pub rom_read: bool,
    pub rom_address: u16,
    pub import_row: u8,
    pub import_kind: u8,
    pub a_read: bool,
    pub a_address: u8,
    pub b_read: bool,
    pub b_address: u8,
    pub b_write: bool,
    pub b_write_address: u8,
    pub b_data: Bits,
    pub c_read: bool,
    pub c_address: u8,
    pub c_write: bool,
    pub c_write_address: u8,
    pub c_data: Bits,
    pub wide_valid: bool,
    pub wide_signed: bool,
    pub active: bool,
    pub done: bool,
    pub bound_valid: bool,
    pub bound_empty: bool,
    pub xlo: u8,
    pub xhi: u8,
    pub ylo: u8,
    pub yhi: u8,
    pub attr_send: bool,
    pub attr_index: u8,
    pub attr_bank: bool,
    pub attr0_take: bool,
    pub attr1_take: bool,
}
#[derive(Clone, Debug)]
pub struct Registers {
    pub u: u64,
    pub issued_op: u8,
    pub active: bool,
    pub done: bool,
    pub axis: bool,
    pub index: u8,
    pub d_kind: u8,
    pub aux_kind: u8,
    pub sign0: bool,
    pub nonzero0: bool,
    pub empty: bool,
    pub eligible: u8,
    pub search_main: bool,
    pub dual_search: bool,
    pub aux_pending: bool,
    pub search_bit: u8,
    pub prefix: u8,
    pub aux_prefix: u8,
    pub xlo: u8,
    pub xhi: u8,
    pub ylo: u8,
    pub yhi: u8,
}
impl Default for Registers {
    fn default() -> Self {
        Self {
            u: 0,
            issued_op: 0,
            active: false,
            done: false,
            axis: false,
            index: 0,
            d_kind: 0,
            aux_kind: 0,
            sign0: false,
            nonzero0: false,
            empty: false,
            eligible: 0,
            search_main: false,
            dual_search: false,
            aux_pending: false,
            search_bit: 0,
            prefix: 0,
            aux_prefix: 0,
            xlo: 0,
            xhi: 24,
            ylo: 0,
            yhi: 14,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Control {
    pub regs: Registers,
}
struct Eval {
    op: u8,
    pair_finish: bool,
    pair_eligible: bool,
    endpoint_search: bool,
    aux_search: bool,
    dual_math: bool,
    search_start: bool,
    search_aux: bool,
    initial_bit: u8,
    candidate_accept: bool,
    trial: u8,
    final_prefix: u8,
    aux_final: u8,
    lower: u8,
    aux_lower: u8,
    aux_empty: bool,
    next_index: u8,
    next_axis: bool,
}
fn kind(value: u64) -> u8 {
    if value == 0 {
        0
    } else if value & (1 << 53) != 0 {
        2
    } else {
        1
    }
}
fn high_bit(value: u8) -> u8 {
    if value == 0 {
        0
    } else {
        (7 - value.leading_zeros()) as u8
    }
}
impl Control {
    pub fn rom_word(address: u16) -> u64 {
        rom()[usize::from(address & 511)]
    }
    fn evaluate(&self, i: Input) -> (Output, Eval) {
        let r = &self.regs;
        let u = r.u;
        let op = if u & (1 << 35) != 0 { r.issued_op } else { 0 };
        let next_index = if r.index == 2 { 0 } else { (r.index + 1) & 3 };
        let next_axis = if r.index == 2 { !r.axis } else { r.axis };
        let dual_math = r.eligible == 3;
        let result_negative = i.result.value & (1 << 53) != 0;
        let pair_eligible = !((!r.nonzero0 && i.b.value == 0)
            || (r.nonzero0 && i.b.value != 0 && r.sign0 == (i.b.value & (1 << 35) != 0)));
        let endpoint_fail = (r.d_kind == 0 || r.d_kind == 2) && result_negative;
        let endpoint_search =
            (r.d_kind == 1 && result_negative) || (r.d_kind == 2 && !result_negative);
        let aux_fail = (r.aux_kind == 0 || r.aux_kind == 2) && result_negative;
        let aux_search =
            (r.aux_kind == 1 && result_negative) || (r.aux_kind == 2 && !result_negative);
        let candidate_accept = if r.d_kind == 2 {
            !result_negative
        } else {
            result_negative
        };
        let aux_accept = if r.aux_kind == 2 {
            !result_negative
        } else {
            result_negative
        };
        let trial = (r.prefix | (1u8 << r.search_bit)) & 31;
        let final_prefix = if candidate_accept { trial } else { r.prefix };
        let aux_trial = (r.aux_prefix | (1u8 << r.search_bit)) & 15;
        let aux_final = if aux_accept { aux_trial } else { r.aux_prefix };
        let lower = final_prefix + 1;
        let main_empty = if r.d_kind == 1 {
            lower > if r.axis { r.yhi } else { r.xhi }
        } else {
            final_prefix < if r.axis { r.ylo } else { r.xlo }
        };
        let aux_lower = aux_final + 1;
        let aux_empty = if r.aux_kind == 1 {
            aux_lower > r.yhi
        } else {
            aux_final < r.ylo
        };
        let pair_finish = (op == 3 && r.axis && r.eligible == 0)
            || (op == 16 && !dual_math && !endpoint_fail && !endpoint_search)
            || (op == 17 && !aux_fail && !r.search_main && !aux_search)
            || (op == 20 && r.search_bit == 0 && !r.empty && !main_empty);
        let mut next_pc = (u & 63) as u8;
        if op >= 48 && !i.input_take {
            next_pc = op;
        }
        match op {
            23 => next_pc = if i.normalize_wait { 23 } else { 24 },
            3 => {
                if !r.axis {
                    next_pc = 0;
                } else if r.eligible == 0 {
                    next_pc = 21;
                } else if !dual_math {
                    next_pc = 41;
                }
            }
            16 => {
                if endpoint_fail {
                    next_pc = 22;
                } else if !dual_math {
                    next_pc = if endpoint_search { 18 } else { 21 };
                }
            }
            17 => {
                next_pc = if aux_fail {
                    22
                } else if r.search_main || aux_search {
                    18
                } else {
                    21
                }
            }
            20 => {
                next_pc = if r.search_bit != 0 {
                    18
                } else if r.empty || main_empty {
                    22
                } else {
                    21
                }
            }
            21 if i.rgb => next_pc = 40,
            39 if r.axis && r.index == 2 => next_pc = 40,
            _ => {}
        }
        if pair_finish {
            next_pc = if r.index == 2 { 21 } else { 0 };
        }
        let mut fetch_axis = r.axis;
        let mut fetch_index = r.index;
        match op {
            3 => fetch_axis = if !r.axis { true } else { r.eligible & 1 == 0 },
            17 => {
                if !r.search_main {
                    fetch_axis = !r.axis;
                }
            }
            31 => {
                fetch_axis = false;
                fetch_index = 0;
            }
            39 => {
                fetch_axis = next_axis;
                fetch_index = next_index;
            }
            _ => {}
        }
        if pair_finish {
            fetch_axis = false;
            fetch_index = (r.index + 1) & 3;
        }
        if next_pc == 3 {
            fetch_axis = r.eligible & 1 != 0;
        }
        let address = if i.start {
            48
        } else {
            (u16::from(fetch_index) << 7) | (u16::from(fetch_axis) << 6) | u16::from(next_pc)
        };
        let search_start = (op == 16 && !dual_math && endpoint_search)
            || (op == 17 && (r.search_main || aux_search));
        let search_axis = if op == 17 && !r.search_main {
            !r.axis
        } else {
            r.axis
        };
        let main_search_bit = high_bit(if search_axis { r.yhi } else { r.xhi });
        let paired_search_bit = high_bit(r.yhi) + 1;
        let initial_bit =
            if op == 17 && r.search_main && aux_search && paired_search_bit > main_search_bit {
                paired_search_bit
            } else {
                main_search_bit
            };
        let a_code = ((u >> 6) & 31) as u8;
        let c_code = ((u >> 15) & 15) as u8;
        let mut ar = a_code;
        let mut br = ((u >> 11) & 15) as u8;
        let mut cr = c_code;
        if (16..=18).contains(&a_code) {
            ar = 32 | (i.shifts[(a_code - 16) as usize] & 31);
        }
        match a_code {
            19 => ar = 64 | initial_bit,
            20 => {
                ar = 64
                    | (u8::from(r.aux_pending && !aux_accept) << 3)
                    | (r.search_bit.wrapping_sub(1) & 7)
            }
            21 => ar = 64 | (u8::from(!candidate_accept) << 3) | (r.search_bit.wrapping_sub(1) & 7),
            22 => ar = if r.d_kind == 1 { 80 } else { 50 },
            23 => ar = if r.aux_kind == 1 { 80 } else { 50 },
            24 => ar = 80,
            25 => ar = 81,
            _ => {}
        }
        if op == 17 && !r.search_main {
            br = u8::from(!r.axis);
        }
        let aux_issue = u & (1 << 34) != 0;
        let c_axis = r.axis ^ aux_issue;
        if c_code == 5 {
            cr = 6 | ((i.seed_sign >> (3 * u8::from(c_axis) + r.index)) & 1);
        }
        if c_code == 15 {
            cr = 6 | ((i.seed_sign >> (3 * u8::from(next_axis) + next_index)) & 1);
        }
        let search_aux = r.dual_search && r.search_bit != 0;
        let read_gate = match op {
            3 => r.axis && r.eligible != 0,
            16 | 17 => search_start,
            18 => search_aux,
            20 => r.search_bit != 0,
            _ => !matches!(op, 5 | 6 | 10 | 11 | 14) || dual_math,
        };
        let ports = ((u >> 19) & 15) as u8;
        let short = u & (1 << 27) != 0;
        let bdata = if short {
            i.result.slice(17, 18).extend(18, 36)
        } else {
            i.result.slice(0, 36)
        };
        (
            Output {
                operation: op,
                rom_read: (i.start || r.active) && !i.reset,
                rom_address: address,
                import_row: ((u >> 15) & 15) as u8,
                import_kind: ((u >> 6) & 7) as u8,
                a_read: r.active && ports & 1 != 0 && read_gate && !i.reset,
                a_address: ar,
                b_read: r.active && ports & 2 != 0 && read_gate && !i.reset,
                b_address: br,
                b_write: r.active && u & (1 << 25) != 0 && (op != 9 || dual_math) && !i.reset,
                b_write_address: if short {
                    15
                } else {
                    u8::from(u & (1 << 26) != 0)
                },
                b_data: bdata,
                c_read: r.active
                    && ports & 4 != 0
                    && (!aux_issue || if op == 19 { search_aux } else { dual_math })
                    && !i.reset,
                c_address: cr,
                c_write: r.active
                    && u & (1 << 28) != 0
                    && (u & (1 << 33) == 0 || if op == 18 { r.aux_pending } else { dual_math })
                    && !i.reset,
                c_write_address: ((u >> 29) & 15) as u8,
                c_data: i.result,
                wide_valid: r.active
                    && ports & 8 != 0
                    && (!aux_issue || if op == 19 { search_aux } else { dual_math })
                    && !(op == 39 && r.axis && r.index == 2)
                    && !i.reset,
                wide_signed: u & (1 << 23) != 0,
                active: r.active,
                done: r.done,
                bound_valid: r.active && matches!(op, 21 | 22) && !i.reset,
                bound_empty: r.empty || op == 22,
                xlo: r.xlo,
                xhi: r.xhi,
                ylo: r.ylo,
                yhi: r.yhi,
                attr_send: r.active && u & (1 << 24) != 0 && !i.reset,
                attr_index: r.index,
                attr_bank: r.axis,
                attr0_take: r.active && op == 1 && !i.reset,
                attr1_take: r.active && op == 2 && !i.reset,
            },
            Eval {
                op,
                pair_finish,
                pair_eligible,
                endpoint_search,
                aux_search,
                dual_math,
                search_start,
                search_aux,
                initial_bit,
                candidate_accept,
                trial,
                final_prefix,
                aux_final,
                lower,
                aux_lower,
                aux_empty,
                next_index,
                next_axis,
            },
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.evaluate(i).0
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        if i.reset {
            self.regs = Registers::default();
            return Ok(());
        }
        if !i.ce {
            return Ok(());
        }
        let (o, e) = self.evaluate(i);
        let r = &self.regs;
        if r.active {
            if matches!(e.op, 1 | 2) {
                require(
                    i.b.is_known(36),
                    "prepare",
                    "unknown PB at attribute/bounds consumer",
                )?;
            }
            if matches!(e.op, 8 | 45 | 37 | 38 | 39) || (e.op == 9 && e.dual_math) {
                require(i.result.is_known(54), "prepare", "unknown DSP result")?;
                let top = i.result.value >> 35;
                require(
                    top == if i.result.value & (1 << 35) != 0 {
                        (1 << 19) - 1
                    } else {
                        0
                    },
                    "prepare",
                    if matches!(e.op, 37..=39) {
                        "G/S overflow"
                    } else {
                        "bbox D overflow"
                    },
                )?;
            }
            if matches!(e.op, 16 | 17 | 20) {
                require(i.result.is_known(54), "prepare", "unknown bounds predicate")?;
            }
            require(
                !(e.op == 18 && r.aux_pending && !r.dual_search),
                "prepare",
                "unexpected auxiliary retire",
            )?;
        }
        let mut n = r.clone();
        if o.rom_read {
            n.u = Self::rom_word(o.rom_address);
            n.issued_op = (o.rom_address & 63) as u8;
        }
        n.done = false;
        if i.start {
            n.active = true;
            n.axis = false;
            n.index = 0;
            n.empty = false;
            n.eligible = 0;
            n.aux_pending = false;
            n.xlo = 0;
            n.xhi = 24;
            n.ylo = 0;
            n.yhi = 14;
        } else if r.active {
            match e.op {
                1 => {
                    n.sign0 = i.b.value & (1 << 35) != 0;
                    n.nonzero0 = i.b.value != 0;
                }
                2 => {
                    n.eligible = (r.eligible & !(1 << u8::from(r.axis)))
                        | (u8::from(e.pair_eligible) << u8::from(r.axis))
                }
                3 => n.axis = if !r.axis { true } else { r.eligible & 1 == 0 },
                8 | 45 => n.d_kind = kind(i.result.value),
                9 => {
                    if e.dual_math {
                        n.aux_kind = kind(i.result.value);
                    }
                }
                16 => n.search_main = e.endpoint_search,
                17 => {
                    n.dual_search = r.search_main && e.aux_search;
                    if !r.search_main {
                        n.axis = !r.axis;
                        n.d_kind = r.aux_kind;
                    }
                }
                18 => {
                    if r.aux_pending {
                        n.aux_prefix = e.aux_final;
                        if r.search_bit == 0 {
                            if r.aux_kind == 1 {
                                if e.aux_lower > r.ylo {
                                    n.ylo = e.aux_lower & 15;
                                }
                            } else if e.aux_final < r.yhi {
                                n.yhi = e.aux_final;
                            }
                            if e.aux_empty {
                                n.empty = true;
                            }
                        }
                    }
                }
                20 => {
                    if e.candidate_accept {
                        n.prefix = e.trial;
                    }
                    n.aux_pending = e.search_aux;
                    if r.search_bit != 0 {
                        n.search_bit = r.search_bit - 1;
                    } else if r.d_kind == 1 {
                        if r.axis {
                            if e.lower > r.ylo {
                                n.ylo = e.lower & 15;
                            }
                        } else if e.lower > r.xlo {
                            n.xlo = e.lower & 31;
                        }
                    } else if r.axis {
                        if e.final_prefix < r.yhi {
                            n.yhi = e.final_prefix & 15;
                        }
                    } else if e.final_prefix < r.xhi {
                        n.xhi = e.final_prefix;
                    }
                }
                22 => n.empty = true,
                31 => {
                    n.axis = false;
                    n.index = 0;
                }
                39 => {
                    n.index = e.next_index;
                    n.axis = e.next_axis;
                }
                24 => {
                    n.active = false;
                    n.done = true;
                }
                _ => {}
            }
            if e.search_start {
                n.search_bit = e.initial_bit;
                n.prefix = 0;
                n.aux_prefix = 0;
                n.aux_pending = false;
                if e.op == 16 {
                    n.dual_search = false;
                }
            }
            if e.pair_finish {
                n.axis = false;
                n.index = (r.index + 1) & 3;
                n.eligible = 0;
                n.aux_pending = false;
            }
        }
        self.regs = n;
        Ok(())
    }
}
