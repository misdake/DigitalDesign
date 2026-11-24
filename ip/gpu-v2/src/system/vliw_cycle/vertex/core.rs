use super::export::{ExportComb, ExportWrite};
use super::triangle::{TComb, TInput};
use super::{check, mask, signed, Bits, Error, InputRam, RamPorts};
use super::{Export, Triangle};
use std::{collections::BTreeMap, sync::OnceLock};

fn rom() -> &'static [u128; 512] {
    static ROM: OnceLock<[u128; 512]> = OnceLock::new();
    ROM.get_or_init(|| {
        let source = include_str!("../../../frontend/vliw/vertex/control_rom.v");
        let mut words = [0u128; 512];
        let mut seen = [false; 512];
        let constant = source
            .lines()
            .find(|s| s.contains("COMPACT_USED_BITS=72'h"))
            .unwrap()
            .split("72'h")
            .nth(1)
            .unwrap()
            .trim_end_matches(';');
        let compact_mask = u128::from_str_radix(constant, 16).unwrap();
        for line in source.lines().filter(|s| s.starts_with("mem[")) {
            let (index, expression) = line.strip_prefix("mem[").unwrap().split_once("]=").unwrap();
            let index: usize = index.parse().unwrap();
            assert!(!seen[index]);
            seen[index] = true;
            let expression = expression.strip_suffix(';').unwrap();
            let expression = if expression.starts_with("SP_COMPACT ? ") {
                expression
                    .strip_prefix("SP_COMPACT ? ")
                    .unwrap()
                    .split(" : ")
                    .next()
                    .unwrap()
            } else {
                expression
            };
            words[index] = u128::from_str_radix(expression.strip_prefix("72'h").unwrap(), 16)
                .unwrap()
                & compact_mask;
        }
        assert!(seen.iter().all(|v| *v));
        words
    })
}

fn fields() -> &'static BTreeMap<&'static str, (u32, u32)> {
    static FIELDS: OnceLock<BTreeMap<&'static str, (u32, u32)>> = OnceLock::new();
    FIELDS.get_or_init(|| {
        let mut out = BTreeMap::new();
        for line in include_str!("../../../frontend/vliw/vertex/control_fields.vh").lines() {
            if let Some((left, right)) = line.split_once(" = control[") {
                let name = left
                    .split_whitespace()
                    .last()
                    .unwrap()
                    .strip_suffix("_raw")
                    .unwrap();
                let (hi, lo) = right.trim_end_matches("];").split_once(':').unwrap();
                let lo: u32 = lo.parse().unwrap();
                let hi: u32 = hi.parse().unwrap();
                out.insert(name, (lo, hi - lo + 1));
            }
        }
        assert_eq!(out.len(), 24);
        out
    })
}
#[derive(Clone, Copy)]
struct Ins(u128);
impl Ins {
    fn field(self, key: &str) -> u8 {
        let &(lo, n) = fields().get(key).unwrap();
        ((self.0 >> lo) & ((1 << n) - 1)) as u8
    }
    fn bit(self, n: u32) -> bool {
        self.0 & (1u128 << n) != 0
    }
    fn target(self) -> u16 {
        u16::from(self.field("target")) | (u16::from(self.field("target_hi")) << 7)
    }
    fn push(self) -> u8 {
        self.field("up_push") | (self.field("up_hi") << 2)
    }
}
fn add(a: Bits, b: Bits, width: u32) -> Bits {
    if a.is_known(width) && b.is_known(width) {
        Bits::new(a.value.wrapping_add(b.value), width)
    } else {
        Bits::default()
    }
}
fn product(a: Bits, b: Bits, high: bool) -> Bits {
    if a.is_known(32) && b.is_known(36) {
        let av = if high {
            i128::from(signed(a.value, 32) >> 18)
        } else {
            i128::from(a.value & mask(18))
        };
        Bits::new((av * i128::from(signed(b.value, 36))) as u64, 54)
    } else {
        Bits::default()
    }
}
fn mac(pl: Bits, ph: Bits, c: Bits) -> Bits {
    let lo = add(pl, c, 54);
    let hi = add(ph, lo.slice(18, 36).extend(36, 54), 54);
    Bits::concat(hi.slice(0, 36), lo.slice(0, 18), 18)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CoreInput {
    pub reset: bool,
    pub ce: bool,
    pub start_v: bool,
    pub vertex_billboard: bool,
    pub world_transform: bool,
    pub billboard_vertical: bool,
    pub start_t: bool,
    pub triangle_descriptor: u32,
    pub cull_negative: bool,
    pub packet_ready: bool,
    pub word_ready: bool,
    pub meshlet_reset: bool,
    pub vertex_count: u8,
    pub uniform_base: u8,
    pub config_base: u16,
    pub vertex_base: u16,
    pub mesh_color: u32,
    pub dma_write: bool,
    pub dma_address: u16,
    pub dma_data: u64,
    pub output_ready: bool,
    pub source_read: bool,
    pub source_address: u16,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct CoreOutput {
    pub prepared_light: Option<(bool, u16)>,
    pub busy: bool,
    pub done_v: bool,
    pub done_t: bool,
    pub triangle_ready: bool,
    pub packet_valid: bool,
    pub descriptor: Bits,
    pub word_valid: bool,
    pub word_index: u8,
    pub word_data: Bits,
    pub coefficient_write: bool,
    pub coefficient_address: u16,
    pub coefficient_data: Bits,
    pub publish: bool,
    pub packet_take: bool,
    pub word_take: bool,
    pub release_slot: bool,
    pub export_read: bool,
    pub export_address: u16,
    pub position_read: bool,
    pub position_read_address: u16,
    pub meshlet_ready: bool,
    pub drained: bool,
    pub dma_ready: bool,
    pub source_ready: bool,
    pub source_data: Bits,
    pub published_vertices: u8,
    pub emit_valid: bool,
    pub emit_address: u16,
    pub emit_data: Bits,
    pub position_write: bool,
    pub position_address: u16,
    pub attribute_write: bool,
    pub attribute_address: u16,
}
#[derive(Clone, Debug)]
pub struct Registers {
    pub busy: bool,
    pub done_v: bool,
    pub done_t: bool,
    pub triangle_mode: bool,
    pub refs: u32,
    pub cull: bool,
    pub building_descriptor: u32,
    pub bounds_safe: bool,
    pub published_vertices: u8,
    pub pc: u16,
    pub remaining: u8,
    pub uniform_base: u8,
    pub config_base: u16,
    pub vertex_pointer: u16,
    pub emit_pointer: u16,
    pub vertex_id: u8,
    pub field: u8,
    pub epoch: bool,
    pub control: u128,
    pub origin: [Bits; 3],
    pub quant_shift: u8,
    pub normal18: bool,
    pub unpacked: Bits,
    pub unpack_kind: u8,
    pub phase: u8,
    pub acc: Bits,
    pub pl: Bits,
    pub ph: Bits,
    pub packed: Bits,
}
impl Default for Registers {
    fn default() -> Self {
        Self {
            busy: false,
            done_v: false,
            done_t: false,
            triangle_mode: false,
            refs: 0,
            cull: false,
            building_descriptor: 0,
            bounds_safe: false,
            published_vertices: 0,
            pc: 0,
            remaining: 0,
            uniform_base: 0,
            config_base: 0,
            vertex_pointer: 0,
            emit_pointer: 0,
            vertex_id: 0,
            field: 0,
            epoch: false,
            control: 0,
            origin: [Bits::new(0, 32); 3],
            quant_shift: 0,
            normal18: false,
            unpacked: Bits::new(0, 32),
            unpack_kind: 0,
            phase: 0,
            acc: Bits::new(0, 54),
            pl: Bits::new(0, 54),
            ph: Bits::new(0, 54),
            packed: Bits::new(0, 54),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Core {
    pub regs: Registers,
    pub input: InputRam,
    pub triangle: Triangle,
    pub export: Export,
    pub position_q: Bits,
    triangles_enabled: bool,
    rf: [Bits; 16],
    positions: [Bits; 512],
    attributes: [Bits; 512],
    colors: [Bits; 2],
}
impl Default for Core {
    fn default() -> Self {
        Self {
            regs: Registers::default(),
            input: InputRam::default(),
            triangle: Triangle::default(),
            export: Export::default(),
            position_q: Bits::default(),
            triangles_enabled: false,
            rf: [Bits::new(0, 32); 16],
            positions: [Bits::default(); 512],
            attributes: [Bits::default(); 512],
            colors: [Bits::default(); 2],
        }
    }
}
struct Comb {
    o: CoreOutput,
    ports: RamPorts,
    step: bool,
    launch: bool,
    launch_t: bool,
    t: TComb,
    tin: TInput,
    export: ExportComb,
    next_pc: u16,
    left: Bits,
    pack_value: Bits,
    rf_write: Bits,
    mac_a: Bits,
    mac_b: Bits,
    mac_y: Bits,
    normal12: Bits,
}
impl Core {
    pub(crate) fn observe(&self, i: CoreInput, trace: &mut super::super::trace::Trace) -> bool {
        let x = self.comb(i);
        if x.step {
            let c = self.regs.control;
            trace.event(
                "vt_step",
                [
                    u64::from(self.regs.pc),
                    u64::from(self.regs.triangle_mode),
                    c as u32 as u64,
                    (c >> 32) as u32 as u64,
                    (c >> 64) as u64,
                ],
            );
            let ins = Ins(c);
            if ins.field("rf_wsrc") != 0 {
                let bits = x.rf_write;
                trace.event(
                    "vt_rf_write",
                    [
                        u64::from(ins.field("rf_dst")),
                        (bits.value & bits.known) & 0xffffffff,
                        bits.known & 0xffffffff,
                    ],
                );
            }
        }
        if x.o.word_take {
            let bits = x.o.word_data;
            trace.event(
                "t_word",
                [
                    u64::from(self.export.consumed & 15),
                    u64::from(x.o.word_index),
                    (bits.value & bits.known) as u32 as u64,
                    (bits.value & bits.known) >> 32,
                    bits.known as u32 as u64,
                    bits.known >> 32,
                ],
            );
        }
        x.step
    }
    pub fn with_triangles() -> Self {
        Self {
            triangles_enabled: true,
            ..Self::default()
        }
    }
    pub fn supports_triangles(&self) -> bool {
        self.triangles_enabled
    }
    pub fn rom_word(address: u16) -> u128 {
        rom()[(address & 511) as usize]
    }
    pub fn rf_word(&self, address: u8) -> Bits {
        self.rf[(address & 15) as usize]
    }
    pub fn position(&self, address: u16) -> Bits {
        self.positions[(address & 511) as usize]
    }
    pub fn attribute(&self, address: u16) -> Bits {
        self.attributes[(address & 511) as usize]
    }
    pub fn epoch_color(&self, epoch: bool) -> Bits {
        self.colors[usize::from(epoch)]
    }
    pub fn mac_y(&self) -> Bits {
        let c = Ins(self.regs.control);
        mac(
            self.regs.pl,
            self.regs.ph,
            if self.regs.triangle_mode && c.bit(70) {
                Bits::new(
                    if self.triangle.align_negative {
                        mask(26)
                    } else {
                        0
                    },
                    54,
                )
            } else if c.field("acc_op") == 1 {
                Bits::new(
                    if !self.regs.triangle_mode && self.regs.pc & 192 == 192 && c.bit(70) {
                        1 << 23
                    } else {
                        0
                    },
                    54,
                )
            } else {
                self.regs.acc
            },
        )
    }
    fn comb(&self, i: CoreInput) -> Comb {
        let r = &self.regs;
        let c = Ins(r.control);
        let l = c.field("l_src");
        let rr = c.field("r_src");
        let emit = c.field("emit");
        let vertex_emit = !r.triangle_mode && emit != 0 && !(r.field == 3 && emit == 1);
        let matrix_active = !r.triangle_mode && r.pc & 192 == 192;
        let matrix_store = matrix_active && c.bit(62);
        let step = i.ce
            && r.busy
            && !i.reset
            && (!vertex_emit || i.output_ready)
            && !(matrix_store && i.dma_write);
        let source_ready =
            !r.busy && !i.reset && !i.start_v && !i.start_t && i.source_address & 512 == 0;
        let source_take = i.ce && i.source_read && source_ready;
        let work = c.field("work_en") != 0 || c.field("work_we") != 0;
        let address = |src| match src {
            1 => {
                (u16::from(if c.bit(61) { 23 } else { r.uniform_base }) << 4)
                    | u16::from(c.field("u_off") >> 1)
            }
            3 => (r.config_base << 1) | u16::from(c.field("cfg_off") >> 1),
            _ => r.vertex_pointer >> 1,
        };
        let la = address(l) & 1023;
        let ra = if work {
            384 | u16::from(c.field("work_addr"))
        } else {
            address(rr) & 1023
        };
        let left = if c.field("swap") != 0 {
            self.input.right_q()
        } else {
            self.input.left_q()
        };
        let right = if c.field("swap") != 0 {
            self.input.left_q()
        } else {
            self.input.right_q()
        };
        let a = self.rf[c.field("rf_mac") as usize];
        let pack = self.rf[c.field("rf_pack") as usize];
        let component = r.unpacked.slice(0, 10);
        let position = if component.is_known(10) && r.origin[(r.phase.min(2)) as usize].is_known(32)
        {
            Bits::new(
                r.origin[(r.phase.min(2)) as usize]
                    .value
                    .wrapping_add(component.value << r.quant_shift),
                32,
            )
        } else {
            Bits::default()
        };
        let normal = Bits::concat(component.extend(10, 25), Bits::new(0, 7), 7);
        let unpacked = match r.unpack_kind {
            1 => position,
            2 => normal,
            5 => {
                if r.unpacked.is_known(32) {
                    Bits::new(0u32.wrapping_sub(r.unpacked.value as u32) as u64, 32)
                } else {
                    Bits::default()
                }
            }
            _ => r.unpacked,
        };
        let y = self.mac_y();
        let tin = TInput {
            reset: i.reset,
            active: r.triangle_mode,
            pc: r.pc,
            ins: r.control,
            cull: r.cull,
            refs: r.refs,
            work: self.input.right_q(),
            unpacked: r.unpacked,
            acc: r.acc,
            y,
        };
        let t = self.triangle.comb(tin);
        let export = self
            .export
            .comb(i.reset, i.ce, i.packet_ready, i.word_ready);
        let rf_write = match c.field("rf_wsrc") {
            1 => unpacked,
            3 => Bits::new(
                match c.field("cfg_idx") {
                    0 => (-399i32) as u32,
                    1 => (-239i32) as u32,
                    _ => 512,
                } as u64,
                32,
            ),
            _ => r.acc.slice(16, 32),
        };
        let packed_normal = if pack.is_known(32) {
            let q = if r.normal18 {
                signed(pack.value, 18)
            } else {
                4 * signed(pack.value >> 2, 16)
            };
            let floor = q >> 6;
            let rem = q & 63;
            let rounded = floor + i64::from(rem > 32 || rem == 32 && floor & 1 != 0);
            Bits::new(rounded.clamp(-2048, 2047) as u64, 12)
        } else {
            Bits::default()
        };
        let flow = c.field("flow");
        let mut next_pc = (r.pc + 1) & 511;
        match flow {
            1 | 3 | 7 => next_pc = c.target(),
            2 if r.remaining == 1 => next_pc = c.target(),
            4 => next_pc = r.pc,
            _ => {}
        }
        if r.triangle_mode {
            next_pc = t.next_pc;
        } else if matrix_active && c.bit(71) {
            next_pc = if i.vertex_billboard { 320 } else { 256 };
        }
        if matrix_active && c.bit(68) && i.vertex_billboard {
            next_pc = c.target() | (u16::from(i.billboard_vertical) << 4);
        }
        let write = step && vertex_emit;
        Comb {
            o: CoreOutput {
                prepared_light: (step
                    && matrix_store
                    && !i.vertex_billboard
                    && ((r.control >> 66) & 3) == 3)
                    .then_some((c.bit(63), r.acc.slice(24, 9).value as u16)),
                busy: r.busy,
                done_v: r.done_v,
                done_t: r.done_t,
                triangle_ready: !r.busy && !i.reset && !self.export.full(),
                packet_valid: export.packet_valid,
                descriptor: self.export.head().slice(0, 32),
                word_valid: self.export.valid && !i.reset,
                word_index: self.export.index,
                word_data: self.export.data,
                coefficient_write: step && t.coefficient_write,
                coefficient_address: u16::from(t.coefficient_row) * 16
                    + u16::from(self.export.produced & 15),
                coefficient_data: t.coefficient_data,
                publish: step && t.publish,
                packet_take: export.packet_take,
                word_take: export.word_take,
                release_slot: export.release,
                export_read: export.read,
                export_address: export.address,
                position_read: step && t.pos_read,
                position_read_address: t.pos_address,
                meshlet_ready: !r.busy
                    && !i.reset
                    && (self.export.empty() || ((self.export.head().value >> 32) != 0) == r.epoch),
                drained: !r.busy
                    && !i.reset
                    && self.export.empty()
                    && self.export.import_state == 0,
                dma_ready: !i.reset && i.dma_address & 512 == 0,
                source_ready,
                source_data: self.input.source_q(),
                published_vertices: r.published_vertices,
                emit_valid: r.busy && vertex_emit && !i.reset,
                emit_address: r.emit_pointer,
                emit_data: r.packed.slice(0, 36),
                position_write: write && r.field < 3,
                position_address: (u16::from(r.field & 3) << 6) | u16::from(r.vertex_id),
                attribute_write: write && r.field >= 3,
                attribute_address: if r.triangle_mode {
                    u16::from(t.coefficient_row) * 16 + u16::from(self.export.produced & 15)
                } else {
                    256 | (u16::from(r.field >> 2) << 7)
                        | (u16::from(r.epoch) << 6)
                        | u16::from(r.vertex_id)
                },
            },
            ports: RamPorts {
                reset: i.reset,
                dma_write: i.dma_write && !i.reset && i.dma_address & 512 == 0,
                dma_address: i.dma_address,
                dma_data: i.dma_data,
                matrix_write: step && matrix_store,
                matrix_index: ((r.control >> 63) & 31) as u8,
                matrix_data: r.acc.slice(16, 32),
                left_read: source_take || step && !r.triangle_mode && l != 0,
                right_read: source_take
                    || step
                        && if r.triangle_mode {
                            t.work_read
                        } else {
                            rr != 0 || c.field("work_en") != 0
                        },
                left_address: if source_take { i.source_address } else { la },
                right_address: if source_take {
                    i.source_address
                } else if r.triangle_mode {
                    384 | t.work_ra
                } else {
                    ra
                },
                work_write: step
                    && if r.triangle_mode {
                        t.work_write
                    } else {
                        c.field("work_we") != 0
                    },
                work_address: if r.triangle_mode { 384 | t.work_wa } else { ra },
                work_data: if r.triangle_mode {
                    t.work_data
                } else {
                    r.packed.slice(0, 36)
                },
            },
            step,
            launch: i.ce && i.start_v && !r.busy && !i.reset,
            launch_t: self.triangles_enabled
                && i.ce
                && i.start_t
                && !r.busy
                && !i.start_v
                && !self.export.full()
                && !i.reset,
            t,
            tin,
            export,
            next_pc,
            left,
            pack_value: pack,
            rf_write,
            mac_a: if t.mac_constant { t.mac_a } else { a },
            mac_b: if r.triangle_mode {
                if c.bit(3) {
                    pack.slice(0, 27).extend(27, 36)
                } else {
                    self.input.right_q().slice(0, 30).extend(30, 36)
                }
            } else {
                right
            },
            mac_y: y,
            normal12: packed_normal,
        }
    }
    pub fn outputs(&self, i: CoreInput) -> CoreOutput {
        self.comb(i).o
    }
    pub fn clock(&mut self, i: CoreInput) -> Result<(), Error> {
        let x = self.comb(i);
        let r = &self.regs;
        let c = Ins(r.control);
        if i.reset {
            self.input.clock(x.ports)?;
            self.regs = Registers::default();
            self.triangle = Triangle::default();
            self.position_q = Bits::new(0, 36);
            self.export.clock(
                true,
                i.ce,
                x.export,
                ExportWrite {
                    publish: false,
                    descriptor: Bits::default(),
                    coefficient: false,
                    address: 0,
                    data: Bits::default(),
                },
                &self.attributes,
                &self.colors,
            )?;
            return Ok(());
        }
        if !self.triangles_enabled && i.ce && i.start_t && !r.busy && !i.start_v {
            return Err(Error::UnsupportedStartT);
        }
        if x.launch {
            check(
                i.uniform_base & 32 == 0
                    && i.config_base & 256 == 0
                    && i.vertex_base & 512 == 0
                    && u32::from(i.vertex_base & 1023) * 2 + u32::from(i.vertex_count & 127) * 3
                        <= 1024,
                "compact input contract",
            )?;
            check(
                u16::from(i.vertex_count & 127) + u16::from(r.published_vertices) <= 64,
                "Vertex capacity",
            )?;
        }
        if x.launch_t {
            check(
                (0..3).all(|n| {
                    ((i.triangle_descriptor >> (n * 6)) & 63) < u32::from(r.published_vertices)
                }),
                "refs_valid",
            )?;
        }
        if i.ce && i.meshlet_reset {
            check(
                x.o.meshlet_ready,
                "attribute epoch reused before final indexed export",
            )?;
        }
        if x.step && r.triangle_mode && c.field("rf_wsrc") == 1 {
            let u = r.unpacked.get(32, "T position")?;
            check(
                (-(1 << 25)..(1 << 25)).contains(&signed(u, 32)),
                "triangle position exceeds bounded F16 range",
            )?;
        }
        if x.step && r.triangle_mode && ((r.control >> 6) & 7) != 0 {
            r.acc.get(54, "T normalization")?;
        }
        if x.o.coefficient_write {
            x.o.coefficient_data.get(54, "T coefficient")?;
        }
        if x.o.attribute_write && !self.export.empty() {
            check(
                ((self.export.head().value >> 32) != 0) != r.epoch,
                "V writes an attribute epoch still owned by queued T",
            )?;
        }
        check(
            !(x.o.position_read
                && x.o.position_write
                && x.o.position_read_address == x.o.position_address),
            "position cross-port collision",
        )?;
        check(
            !(x.export.read && x.o.attribute_write && x.export.address == x.o.attribute_address),
            "attribute cross-port collision",
        )?;
        if x.o.position_write || x.o.attribute_write {
            x.o.emit_data.get(36, "valid Vertex result")?;
        }
        let vertex_start = if i.world_transform {
            192
        } else if i.vertex_billboard {
            320
        } else {
            256
        };
        let mut n = r.clone();
        if x.launch || x.launch_t || x.step {
            n.control = Self::rom_word(if x.launch {
                vertex_start
            } else if x.launch_t {
                128
            } else {
                x.next_pc
            });
        }
        if x.step
            && if r.triangle_mode {
                x.t.mac_enable
            } else {
                c.field("mul_en") != 0
            }
        {
            n.pl = product(x.mac_a, x.mac_b, false);
            n.ph = product(x.mac_a, x.mac_b, true);
        }
        if i.ce {
            n.done_v = false;
            n.done_t = false;
            if x.step {
                if r.triangle_mode && x.t.acc_write {
                    n.acc = x.t.acc_data;
                } else if c.field("acc_op") != 0 {
                    n.acc = x.mac_y;
                }
            }
            if i.meshlet_reset {
                n.published_vertices = 0;
            }
            if x.launch {
                n.triangle_mode = false;
                n.busy = i.vertex_count & 127 != 0;
                n.done_v = i.vertex_count & 127 == 0;
                n.pc = vertex_start;
                n.bounds_safe = !i.vertex_billboard;
                n.remaining = i.vertex_count & 127;
                n.uniform_base = i.uniform_base & 63;
                n.config_base = i.config_base & 511;
                n.vertex_pointer = (i.vertex_base & 1023) << 1;
                n.emit_pointer = 0;
                n.vertex_id = r.published_vertices & 63;
                n.field = 0;
            } else if x.launch_t {
                n.triangle_mode = true;
                n.busy = true;
                n.pc = 128;
                n.refs = i.triangle_descriptor & 0x3ffff;
                n.cull = i.cull_negative;
                n.building_descriptor = i.triangle_descriptor;
            } else if x.step && r.triangle_mode {
                n.pc = x.next_pc;
                if c.push() == 4 || c.push() == 5 {
                    n.unpacked = self.position_q.slice(0, 32);
                    n.unpack_kind = c.push();
                }
                if x.t.done {
                    n.busy = false;
                    n.done_t = true;
                }
            } else if x.step {
                n.pc = x.next_pc;
                if r.pc & 192 == 192 && c.bit(71) {
                    n.uniform_base = 22;
                }
                if r.pc & 384 == 256 {
                    // Once disabled (in particular for billboards), this AND
                    // cannot observe an otherwise unused bounds operand.
                    if c.bit(51) && r.bounds_safe {
                        let value = r.unpacked.get(32, "bounds eligibility operand")?;
                        let over = |k: u32| {
                            if value & (1 << 31) != 0 {
                                value >> (17 + k) != mask(32 - (17 + k))
                            } else {
                                value >> (17 + k) != 0
                            }
                        };
                        n.bounds_safe = r.bounds_safe
                            && r.quant_shift <= 14
                            && !(c.bit(43) && over(2)
                                || c.bit(44) && over(7)
                                || c.bit(45) && over(12));
                    }
                    let reject = if c.field("flow") == 7 && r.bounds_safe && c.field("cfg_idx") < 3
                    {
                        let acc = r.acc.get(54, "bounds predicate")?;
                        match c.field("cfg_idx") {
                            0 => !(acc & (1 << 47) == 0 && (acc & mask(47)) >> 16 != 0),
                            1 => acc & (1 << 53) != 0 && ((acc >> 32) & mask(21)) != mask(21),
                            _ => {
                                acc & (1 << 53) == 0
                                    && (acc >> 33 != 0
                                        || acc & (1 << 32) != 0 && acc & mask(32) != 0)
                            }
                        }
                    } else {
                        false
                    };
                    if c.field("flow") == 7 && reject {
                        n.busy = false;
                        n.done_v = true;
                    }
                    if c.field("flow") == 7 && c.bit(70) && !reject {
                        n.epoch = !r.epoch;
                    }
                }
                if c.field("flow") == 3 {
                    n.remaining = r.remaining.wrapping_sub(1) & 127;
                }
                if c.field("flow") == 4 {
                    n.busy = false;
                    n.done_v = true;
                }
                if c.field("l_src") == 2 || c.field("r_src") == 2 {
                    n.vertex_pointer = (r.vertex_pointer + 1) & 2047;
                }
                if x.o.position_write || x.o.attribute_write {
                    n.emit_pointer = (r.emit_pointer + 1) & 1023;
                    if r.field == 4 {
                        n.field = 0;
                        n.vertex_id = (r.vertex_id + 1) & 63;
                        n.published_vertices = (r.published_vertices + 1) & 127;
                    } else {
                        n.field = r.field + 1;
                    }
                }
                if c.field("rf_wsrc") == 1 {
                    n.phase = if r.phase == 2 || r.unpack_kind >= 3 {
                        0
                    } else {
                        (r.phase + 1) & 3
                    };
                    if r.unpack_kind < 3 {
                        n.unpacked = Bits::concat(Bits::new(0, 10), r.unpacked.slice(10, 22), 22);
                    }
                }
                if c.push() != 0 {
                    n.unpacked = if c.push() == 4 {
                        Bits::new(0, 32)
                    } else {
                        x.left.slice(0, 32)
                    };
                    if c.bit(71) {
                        let bit = x.left.slice(31, 1);
                        n.unpacked.set(
                            0,
                            10,
                            if bit.is_known(1) {
                                Bits::new(
                                    if (bit.value != 0) == c.bit(70) {
                                        1023
                                    } else {
                                        0
                                    },
                                    10,
                                )
                            } else {
                                Bits::default()
                            },
                        );
                    }
                    n.unpack_kind = c.push();
                    n.phase = c.field("cfg_idx");
                }
                if c.field("cfg_we") != 0 {
                    match c.field("cfg_idx") {
                        0..=2 => n.origin[c.field("cfg_idx") as usize] = x.left.slice(0, 32),
                        _ => {
                            n.quant_shift = x.left.slice(0, 5).get(5, "grid shift")? as u8;
                            n.normal18 = x.left.slice(8, 1).get(1, "normal mode")? != 0;
                        }
                    }
                }
                match c.field("pack_op") {
                    1 => n.packed = Bits::concat(Bits::new(0, 22), x.pack_value, 32),
                    2 => n.packed = Bits::concat(Bits::new(0, 42), x.normal12, 12),
                    3 => n.packed.set(12, 12, x.normal12),
                    4 => n.packed.set(24, 12, x.normal12),
                    6 => n.packed = x.pack_value.extend(32, 54),
                    _ => {}
                }
            }
        }
        let descriptor = Bits::concat(
            Bits::new(u64::from(r.epoch), 1),
            Bits::new(
                u64::from(
                    (r.building_descriptor & !(1 << 18))
                        | (u32::from(self.triangle.negative) << 18),
                ),
                32,
            ),
            32,
        );
        self.export.clock(
            false,
            i.ce,
            x.export,
            ExportWrite {
                publish: x.o.publish,
                descriptor,
                coefficient: x.o.coefficient_write,
                address: x.o.coefficient_address,
                data: x.o.coefficient_data,
            },
            &self.attributes,
            &self.colors,
        )?;
        if x.o.position_read {
            check(
                x.o.position_read_address >> 6 < 3,
                "T position read outside planes",
            )?;
            self.position_q = self.positions[x.o.position_read_address as usize];
        }
        self.triangle
            .clock(x.tin, x.t, x.launch_t, i.ce && (x.step || x.launch_t));
        self.input.clock(x.ports)?;
        if x.step && c.field("rf_wsrc") != 0 {
            self.rf[c.field("rf_dst") as usize] = x.rf_write.slice(0, 32);
        }
        if x.o.position_write {
            self.positions[x.o.position_address as usize] = x.o.emit_data;
        }
        if x.o.attribute_write {
            self.attributes[x.o.attribute_address as usize] = x.o.emit_data;
        }
        if x.o.coefficient_write {
            self.attributes[x.o.coefficient_address as usize] = x.o.coefficient_data.slice(0, 36);
        }
        if i.ce && i.meshlet_reset {
            self.colors[usize::from(!r.epoch)] = Bits::new(u64::from(i.mesh_color), 32);
        }
        self.regs = n;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn billboard_zero_normal_needs_no_uniform_tail() {
        let mut core = Core::default();
        core.regs.busy = true;
        core.regs.pc = 327;
        core.regs.control = Core::rom_word(327);
        core.regs.uniform_base = 22;
        core.regs.bounds_safe = false;
        core.rf[12] = Bits::default();
        // The derived World/View block has no initialized word30.
        assert_eq!(core.input.peek(22 * 16 + 15)[0].known, 0);
        for _ in 0..3 {
            core.clock(CoreInput {
                ce: true,
                ..Default::default()
            })
            .unwrap();
        }
        assert_eq!(core.regs.pc, 384);
        assert_eq!(core.rf_word(12), Bits::new(0, 32));
    }

    #[test]
    fn disabled_bounds_do_not_read_a_dead_eligibility_operand() {
        let mut core = Core::default();
        core.regs.busy = true;
        // Last Billboard bootstrap word: bit51 belongs to its branch target,
        // while bounds were disabled when this meshlet launched.
        core.regs.pc = 329;
        core.regs.control = 1 << 51;
        core.regs.bounds_safe = false;
        core.regs.unpacked = Bits::default();
        core.clock(CoreInput {
            ce: true,
            ..Default::default()
        })
        .unwrap();
        assert!(!core.regs.bounds_safe);
    }
}
