use super::{check, Bits, Core, CoreInput, CoreOutput, Error};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    #[default]
    Idle = 0,
    Meta0 = 1,
    Meta0Q = 2,
    Meta1 = 3,
    Meta1Q = 4,
    Meta3 = 5,
    Meta3Q = 6,
    Check = 7,
    VStart = 9,
    VWait = 10,
    Index = 11,
    IndexQ = 12,
    TStart = 13,
    TWait = 14,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub start: bool,
    pub slot: bool,
    pub mesh_color: u32,
    pub dma_write: bool,
    pub dma_address: u16,
    pub dma_data: u64,
    pub output_ready: bool,
    pub packet_ready: bool,
    pub word_ready: bool,
    pub cp_read: bool,
    pub cp_address: u16,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub ready: bool,
    pub busy: bool,
    pub done: bool,
    pub drained: bool,
    pub dma_ready: bool,
    pub cp_ready: bool,
    pub cp_q: Bits,
    pub core: CoreOutput,
}
#[derive(Clone, Debug, Default)]
pub struct Meshlet {
    pub core: Core,
    pub phase: Phase,
    pub active_slot: bool,
    pub vertices: u8,
    pub triangles: u8,
    pub group_position: u8,
    pub index_group: u64,
    pub uniform_base: u8,
    pub index_cursor: u8,
    pub attribute_mode: bool,
    pub normal_mode: bool,
    pub cull_negative: bool,
    pub sequence: u8,
    pub generation: u8,
    ready: bool,
    done: bool,
    /// Actual parent binding registers, used by its passive ownership monitors.
    pub input_active: bool,
    pub monitored_page: bool,
}
impl Meshlet {
    pub fn with_triangles() -> Self {
        Self {
            core: Core::with_triangles(),
            ..Self::default()
        }
    }
    fn connect(&self, i: Input) -> (CoreInput, bool) {
        let cp = self.phase == Phase::Idle && !i.start;
        let index_read = self.phase == Phase::Index && self.index_cursor & 128 == 0;
        let source_read =
            matches!(self.phase, Phase::Meta0 | Phase::Meta1 | Phase::Meta3) || index_read;
        let word = match self.phase {
            Phase::Meta0 => 0,
            Phase::Meta1 => 1,
            _ => 3,
        };
        let inactive_payload =
            i.dma_address & 768 == 0 && (i.dma_address & 128 != 0) != self.active_slot;
        let inactive_metadata =
            (i.dma_address & 1023) >> 3 == 32 && (i.dma_address & 4 != 0) != self.active_slot;
        let cp_region = (i.dma_address & 511) >> 6 == 7
            || (i.dma_address & 511) >> 6 == 4 && (i.dma_address >> 3) & 7 != 0;
        let safe = self.phase == Phase::Idle || inactive_payload || inactive_metadata || cp_region;
        (
            CoreInput {
                reset: i.reset,
                ce: i.ce,
                start_v: self.phase == Phase::VStart,
                start_t: self.phase == Phase::TStart,
                triangle_descriptor: u32::from(self.generation) << 28
                    | u32::from(self.sequence) << 21
                    | u32::from(self.normal_mode) << 20
                    | u32::from(self.attribute_mode) << 19
                    | (self.index_group as u32 & 0x3ffff),
                cull_negative: self.cull_negative,
                packet_ready: i.packet_ready,
                word_ready: i.word_ready,
                meshlet_reset: self.phase == Phase::Check,
                vertex_count: self.vertices,
                uniform_base: self.uniform_base,
                config_base: 129 + u16::from(self.active_slot) * 2,
                vertex_base: u16::from(self.active_slot) * 128,
                mesh_color: i.mesh_color,
                dma_write: i.dma_write && safe,
                dma_address: i.dma_address,
                dma_data: i.dma_data,
                output_ready: i.output_ready,
                source_read: source_read || cp && i.cp_read,
                source_address: if cp {
                    i.cp_address & 511
                } else if index_read {
                    u16::from(self.active_slot) * 128 + u16::from(self.index_cursor & 127)
                } else {
                    256 + u16::from(self.active_slot) * 4 + word
                },
            },
            safe,
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        let (c, safe) = self.connect(i);
        let o = self.core.outputs(c);
        Output {
            ready: self.ready,
            busy: self.phase != Phase::Idle,
            done: self.done,
            drained: o.drained,
            dma_ready: o.dma_ready && safe,
            cp_ready: self.phase == Phase::Idle && !i.start && o.source_ready,
            cp_q: o.source_data,
            core: o,
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), Error> {
        let (c, _) = self.connect(i);
        let o = self.outputs(i);
        if !i.reset {
            check(
                !i.dma_write || o.dma_ready,
                "DMA sink lost independent write port",
            )?;
            check(
                !(i.cp_read && self.input_active),
                "CP read while V owns source Q",
            )?;
            check(
                !(self.input_active
                    && i.dma_write
                    && i.dma_address < 256
                    && (i.dma_address & 128 != 0) == self.monitored_page),
                "DMA overwrote active input page",
            )?;
        }
        let mut next = (
            self.phase,
            self.active_slot,
            self.vertices,
            self.triangles,
            self.uniform_base,
            self.index_cursor,
            self.attribute_mode,
            self.normal_mode,
            self.cull_negative,
            self.sequence,
            self.generation,
            self.ready,
            self.done,
            self.group_position,
            self.index_group,
        );
        if i.reset {
            next = (
                Phase::Idle,
                false,
                0,
                0,
                0,
                0,
                false,
                false,
                false,
                0,
                0,
                false,
                false,
                0,
                0,
            );
        } else if i.ce {
            next.12 = false;
            if self.phase == Phase::Idle && o.core.meshlet_ready {
                next.11 = true;
            }
            match self.phase {
                Phase::Idle if i.start && self.ready => {
                    next.11 = false;
                    next.1 = i.slot;
                    next.0 = Phase::Meta0;
                }
                Phase::Meta0 if o.core.source_ready => next.0 = Phase::Meta0Q,
                Phase::Meta0Q => {
                    let v = o.core.source_data.get(64, "meshlet header0")?;
                    let triangles = ((v >> 8) & 127) as u8;
                    if triangles != 0 && !self.core.supports_triangles() {
                        return Err(Error::UnsupportedTriangles {
                            count: triangles,
                            slot: self.active_slot,
                        });
                    }
                    next.2 = (v & 127) as u8;
                    next.3 = triangles;
                    next.4 = ((v >> 16) & 63) as u8;
                    next.5 = ((v >> 24) & 127) as u8;
                    next.8 = v & (1 << 31) != 0;
                    next.0 = Phase::Meta1;
                }
                Phase::Meta1 if o.core.source_ready => next.0 = Phase::Meta1Q,
                Phase::Meta1Q => {
                    let v = o.core.source_data.get(64, "meshlet header1")?;
                    next.6 = v & 16 != 0;
                    next.9 = ((v >> 5) & 127) as u8;
                    next.10 = ((v >> 12) & 15) as u8;
                    next.0 = Phase::Meta3;
                }
                Phase::Meta3 if o.core.source_ready => next.0 = Phase::Meta3Q,
                Phase::Meta3Q => {
                    next.7 = o
                        .core
                        .source_data
                        .slice(40, 1)
                        .get(1, "meshlet normal mode")?
                        != 0;
                    next.0 = Phase::Check;
                }
                Phase::Check => {
                    check(
                        self.vertices <= 64
                            && self.triangles <= 96
                            && self.uniform_base >> 2 == 5
                            && (self.triangles == 0
                                || self.vertices != 0
                                    && u16::from(self.index_cursor)
                                        >= (u16::from(self.vertices) * 3).div_ceil(2)),
                        "invalid meshlet header",
                    )?;
                    next.0 = Phase::VStart;
                }
                Phase::VStart => next.0 = Phase::VWait,
                Phase::VWait if o.core.done_v => {
                    if self.triangles == 0 || o.core.published_vertices == 0 {
                        next.12 = true;
                        next.0 = Phase::Idle;
                    } else {
                        next.0 = Phase::Index;
                    }
                }
                Phase::Index => {
                    check(self.index_cursor & 128 == 0, "index cursor")?;
                    if o.core.source_ready {
                        next.5 = self.index_cursor.wrapping_add(1);
                        next.0 = Phase::IndexQ;
                    }
                }
                Phase::IndexQ => {
                    next.14 = o.core.source_data.get(54, "index group")?;
                    next.13 = 0;
                    next.0 = Phase::TStart;
                }
                Phase::TStart if o.core.triangle_ready => next.0 = Phase::TWait,
                Phase::TWait if o.core.done_t => {
                    if self.triangles == 1 {
                        next.3 = 0;
                        next.12 = true;
                        next.0 = Phase::Idle;
                    } else {
                        next.3 = self.triangles - 1;
                        next.9 = self.sequence.wrapping_add(1) & 127;
                        next.14 = self.index_group >> 18;
                        if self.group_position == 2 {
                            next.0 = Phase::Index;
                        } else {
                            next.13 = self.group_position + 1;
                            next.0 = Phase::TStart;
                        }
                    }
                }
                _ => {}
            }
        }
        self.core.clock(c)?;
        if i.reset {
            self.input_active = false;
            self.monitored_page = false;
        } else {
            if self.done {
                self.input_active = false;
            }
            if i.ce && i.start && self.ready {
                self.input_active = true;
                self.monitored_page = i.slot;
            }
        }
        (
            self.phase,
            self.active_slot,
            self.vertices,
            self.triangles,
            self.uniform_base,
            self.index_cursor,
            self.attribute_mode,
            self.normal_mode,
            self.cull_negative,
            self.sequence,
            self.generation,
            self.ready,
            self.done,
            self.group_position,
            self.index_group,
        ) = next;
        Ok(())
    }
}
