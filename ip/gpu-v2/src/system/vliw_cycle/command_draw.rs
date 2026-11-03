//! Installed command fields and the production CP's DRAW-side registers.
//! This is control state only; it never renders or clocks a memory service.
use super::{Error, Phase, Processor, ProcessorInput, ProcessorOutput};
use crate::system::vliw_cycle::require;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bindings {
    pub far_z: u32,
    pub depth_shift: u8,
    pub texture_base: u16,
    pub texture_n: u8,
    pub texture_mip: bool,
    pub texture_filter: u8,
    pub texture_framebuffer: bool,
    pub lod_bias: i8,
    pub unlit: bool,
    /// 512-byte units, with seven low zero bits (64-KiB alignment).
    pub color_base: u16,
    pub depth_base: u16,
    pub mesh_color: u32,
    pub specular: u32,
    pub depth_func: u8,
    pub depth_write: bool,
    pub blend: bool,
    pub epoch: u16,
    pub mode: u8,
    pub code: u8,
    pub light: [i16; 3],
    pub ray: [i16; 3],
    pub ambient: u16,
    pub directional: u16,
}
impl Default for Bindings {
    fn default() -> Self {
        Self {
            far_z: 0,
            depth_shift: 0,
            texture_base: 0,
            texture_n: 0,
            texture_mip: false,
            texture_filter: 0,
            texture_framebuffer: false,
            lod_bias: 0,
            unlit: false,
            color_base: 0,
            depth_base: 0,
            mesh_color: u32::MAX,
            specular: 0,
            depth_func: 0,
            depth_write: false,
            blend: false,
            epoch: 0,
            mode: 0,
            code: 0,
            light: [0; 3],
            ray: [0; 3],
            ambient: 0,
            directional: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub work_ready: bool,
    pub meshlet_ready: bool,
    pub meshlet_done: bool,
    pub mesh_loading: bool,
    pub mesh_slot: bool,
    pub mesh_begin: bool,
    pub mesh_begin_slot: bool,
    pub mesh_done: bool,
    pub mesh_done_slot: bool,
    pub uniform_done: bool,
    pub uniform_bank: u8,
    pub header_write: bool,
    pub header_slot: bool,
    pub header_data: u64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Draw {
    pub config: Bindings,
    pub global_valid: bool,
    pub render_valid: bool,
    pub target_valid: bool,
    pub pages: u8,
    pub uniforms: u8,
    pub page_uniform: u8,
    pub object_index: u8,
}
impl Draw {
    pub fn new_submission(&mut self) {
        self.global_valid = false;
        self.render_valid = false;
        self.target_valid = false;
        self.pages = 0;
        self.uniforms = 0;
    }
    pub fn wall(&mut self, i: Input) {
        if i.mesh_begin {
            self.pages &= !(1 << u8::from(i.mesh_begin_slot));
        }
        if i.mesh_done {
            self.pages |= 1 << u8::from(i.mesh_done_slot);
        }
        if i.uniform_done {
            self.uniforms |= 1 << (i.uniform_bank & 3);
        }
        if i.header_write {
            let shift = 2 * u8::from(i.header_slot);
            self.page_uniform =
                (self.page_uniform & !(3 << shift)) | (((i.header_data >> 16) as u8 & 3) << shift);
        }
    }
    pub fn inputs(&self, p: &Processor, i: ProcessorInput) -> bool {
        let slot = p.command & 1 != 0;
        self.pages & (1 << u8::from(slot)) != 0
            && (!i.work_busy || i.draw.mesh_loading && i.draw.mesh_slot != slot)
    }
    fn uniform(&self, command: u32) -> u8 {
        (self.page_uniform >> (2 * (command & 1))) & 3
    }
    pub fn outputs(&self, p: &Processor, i: ProcessorInput, o: &mut ProcessorOutput) {
        let op = p.command >> 28;
        o.bindings = self.config;
        o.draw_slot = p.command & 1 != 0;
        o.start = p.phase == Phase::DrawStart && self.inputs(p, i) && !o.stop;
        o.texture_invalidate = i.ce && !o.stop && p.phase == Phase::Exec && op == 9;
        o.work = super::super::command_dma::Work {
            valid: p.phase == Phase::Exec && !o.stop && matches!(op, 1 | 2),
            mesh: op == 2,
            source_halfword: if op == 2 {
                ((p.command >> 12) & 0xffff) << 6
            } else {
                ((p.command >> 10) & 0x3ffff) << 4
            },
            destination: ((p.command >> 3) & 127) as u8,
            size: ((p.command >> 1) & 3) as u8,
            blocks: ((p.command >> 1) & 7) as u8,
            slot: o.draw_slot,
        };
        if matches!(p.phase, Phase::ApplyRead | Phase::ColorRead) {
            o.ram_read = !o.stop;
            o.ram_address = if p.phase == Phase::ColorRead {
                320 + 16 * u16::from(self.uniform(p.command)) + 11
            } else {
                ((p.command & 127) << 2) as u16 | u16::from(self.object_index)
            };
        }
    }
    pub fn check(&self, p: &Processor, i: ProcessorInput) -> Result<(), Error> {
        let check = |ok, rule| require(ok, "command_processor", rule).map_err(Error::from);
        if !i.ce || i.abort || p.failed {
            return Ok(());
        }
        let op = p.command >> 28;
        if p.phase == Phase::Exec {
            let payload = p.command & 0x0fff_ffff;
            let d = (p.command >> 3) & 127;
            let size = (p.command >> 1) & 3;
            let state_d = p.command & 127;
            let object_legal =
                payload >> 7 == 0 && (66..=79).contains(&state_d) && !(op == 5 && state_d == 79);
            match op {
                1 => {
                    let source = ((p.command >> 10) & 0x3ffff) << 5;
                    let bytes = 32u32 << size;
                    let staging = (66..80).contains(&d) && d * 32 + bytes <= 80 * 32;
                    let uniform = (80..=92).contains(&d) && d.is_multiple_of(4) && size == 2;
                    check(
                        payload & 1 == 0
                            && size != 3
                            && source & (bytes - 1) == 0
                            && source + bytes <= 0x800000
                            && (staging || uniform),
                        "CP local DMA contract",
                    )?;
                }
                2 => check(
                    payload & 0xff0 == 0
                        && (((p.command >> 12) & 0xffff) + ((p.command >> 1) & 7) + 2 <= 65536),
                    "CP mesh DMA contract",
                )?,
                3 => check(
                    payload >> 1 == 0
                        && self.global_valid
                        && self.render_valid
                        && self.uniforms & (1 << self.uniform(p.command)) != 0
                        && self.target_valid,
                    "CP DRAW contract",
                )?,
                5 | 6 => check(
                    object_legal && p.all_fence && i.render_idle && !i.work_busy,
                    "CP state command contract",
                )?,
                10 => check(
                    object_legal && p.all_fence && p.stored && i.render_idle && !i.work_busy,
                    "CP target drain contract",
                )?,
                9 => check(
                    payload == 0 && p.all_fence && i.render_idle && !i.work_busy,
                    "CP drain contract",
                )?,
                _ => {}
            }
        }
        if p.phase == Phase::ApplyWrite && !matches!(op, 11 | 12) {
            check(!self.object_bad(op, i.ram_q), "CP state data contract")?;
        }
        if p.phase == Phase::ColorWrite {
            check(i.ram_q >> 32 == 0, "CP mesh color encoding")?;
        }
        if p.phase == Phase::DrawStart && self.inputs(p, i) && i.draw.meshlet_ready && i.work_busy {
            check(
                i.draw.mesh_loading && i.draw.mesh_slot != (p.command & 1 != 0),
                "DRAW overlaps DMA writer",
            )?;
        }
        Ok(())
    }
    fn object_bad(&self, op: u32, q: u64) -> bool {
        if op == 10 {
            let c = q & 0x3fff;
            let d = (q >> 14) & 0x3fff;
            q >> 28 != 0
                || c & 127 != 0
                || d & 127 != 0
                || c > 15984
                || d > 15984
                || !(c + 400 <= d || d + 400 <= c)
        } else if op == 6 {
            match self.object_index {
                0 => {
                    q >> 56 != 0
                        || (q & (1 << 23) != 0
                            && ((q >> 16) & 15 != 9
                                || q & (1 << 20) != 0
                                || (q >> 21) & 3 > 1
                                || (q >> 24) & 255 != 0
                                || q & 511 != 0
                                || q & 65535 > 63488))
                }
                1 => q & 0xffff_ffc0 != 0 || q >> 40 != 0,
                _ => q != 0,
            }
        } else {
            match self.object_index {
                0 => {
                    let mapped = q as u32 & 0x00ff_ffff;
                    let shift = (q >> 24) as u8 & 3;
                    q as u32 >> 26 != 0
                        || mapped == 0
                        || mapped >= 65535 * 256
                        || mapped >> shift >= 200 * 65536
                        || mapped & ((1 << shift) - 1) != 0
                }
                1 => false,
                2 => {
                    q & 0xfe00_fe00 != 0
                        || q & 511 > 256
                        || (q >> 16) & 511 > 256
                        || (q >> 39) & 511 != 0
                        || (q >> 34) & 31 > 16
                }
                _ => q != 0,
            }
        }
    }
    pub fn apply(&mut self, op: u32, index: u8, q: u64) -> bool {
        let c = &mut self.config;
        match (op, index) {
            (10, _) => {
                c.color_base = q as u16 & 0x3f80;
                c.depth_base = (q >> 14) as u16 & 0x3f80;
                self.target_valid = true;
                return true;
            }
            (6, 0) => {
                c.texture_base = q as u16;
                c.texture_n = (q >> 16) as u8 & 15;
                c.texture_mip = q & (1 << 20) != 0;
                c.texture_filter = (q >> 21) as u8 & 3;
                c.texture_framebuffer = q & (1 << 23) != 0;
                c.specular = (q >> 32) as u32 & 0xff_ffff;
            }
            (6, 1) => {
                c.depth_func = q as u8 & 7;
                c.depth_write = q & 8 != 0;
                c.blend = q & 16 != 0;
                c.unlit = q & 32 != 0;
                c.lod_bias = (q >> 32) as i8;
                self.render_valid = true;
                return true;
            }
            (5, 0) => {
                c.far_z = q as u32 & 0x00ff_ffff;
                c.depth_shift = (q >> 24) as u8 & 3;
                c.light[0] = (q >> 32) as i16;
                c.light[1] = (q >> 48) as i16;
            }
            (5, 1) => {
                c.light[2] = q as i16;
                c.ray = [(q >> 16) as i16, (q >> 32) as i16, (q >> 48) as i16];
            }
            (5, 2) => {
                c.ambient = q as u16 & 511;
                c.directional = (q >> 16) as u16 & 511;
                c.mode = (q >> 32) as u8 & 3;
                c.code = (q >> 34) as u8 & 31;
                c.epoch = (q >> 48) as u16;
                self.global_valid = true;
                return true;
            }
            _ => unreachable!(),
        }
        self.object_index = (index + 1) & 3;
        false
    }
}
