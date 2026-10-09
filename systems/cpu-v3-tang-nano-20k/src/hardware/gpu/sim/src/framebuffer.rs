//! Clocked C16/Z16 framebuffer data array for the GPU v2 cmodel.
//!
//! This models the previous `FramebufferLaneArray` physical bank arrangement:
//! eight entries, four non-mirrored true-dual-port 1024x16 banks per plane,
//! and `BANK_ORDER=1`. The v2 tag/replacement policy is still under discussion;
//! tags, residency, dirty state, and SDRAM service belong to a separate cache
//! controller. No direct-mapped policy is implied by the entry number here.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FramebufferPlane {
    Color,
    Depth,
}

impl FramebufferPlane {
    const fn index(self) -> usize {
        match self {
            Self::Color => 0,
            Self::Depth => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferBeatAddress {
    pub entry: u8,
    pub y: u8,
    /// Horizontal group of four pixels, 0..3.
    pub group: u8,
}

impl FramebufferBeatAddress {
    fn check(self) {
        assert!(self.entry < 8 && self.y < 16 && self.group < 4);
    }

    fn word(self) -> usize {
        self.check();
        usize::from(self.entry) * 64 + usize::from(self.y) * 4 + usize::from(self.group)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferQuadAddress {
    pub entry: u8,
    pub x: u8,
    pub y: u8,
}

impl FramebufferQuadAddress {
    fn check(self) {
        assert!(self.entry < 8 && self.x < 16 && self.y < 16);
    }

    fn aligned(self) -> bool {
        self.x & 1 == 0 && self.y & 1 == 0
    }

    fn bank_word(self, bank: usize) -> usize {
        self.check();
        let row = (self.y & !1) | (((self.x >> 1) & 1) ^ u8::from(bank >= 2));
        usize::from(self.entry) * 64 + usize::from(row) * 4 + usize::from(self.x >> 2)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferMemoryWrite {
    pub plane: FramebufferPlane,
    pub address: FramebufferBeatAddress,
    pub data: u64,
    pub byte_mask: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferMemoryRead {
    pub plane: FramebufferPlane,
    pub address: FramebufferBeatAddress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferRenderWrite {
    pub address: FramebufferQuadAddress,
    /// Four 16-bit words in physical bank order.
    pub colors: u64,
    pub depths: u64,
    pub color_mask: u8,
    pub depth_mask: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferCycleInput {
    pub reset: bool,
    pub memory_write: Option<FramebufferMemoryWrite>,
    pub memory_read: Option<FramebufferMemoryRead>,
    pub memory_response_ready: bool,
    pub render_write: Option<FramebufferRenderWrite>,
    pub render_read: Option<FramebufferQuadAddress>,
    pub render_response_ready: bool,
}

impl Default for FramebufferCycleInput {
    fn default() -> Self {
        Self {
            reset: false,
            memory_write: None,
            memory_read: None,
            memory_response_ready: true,
            render_write: None,
            render_read: None,
            render_response_ready: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferCycleOutput {
    pub memory_write_accepted: bool,
    pub memory_read_accepted: bool,
    pub render_write_accepted: bool,
    pub render_read_accepted: bool,
    /// A previous response was accepted on this edge, before any new read result.
    pub memory_response_consumed: bool,
    pub render_response_consumed: bool,
    /// Registered outputs after this edge. They hold until accepted downstream.
    pub memory_response: Option<u64>,
    pub render_response: Option<(u64, u64)>,
}

#[derive(Clone, Debug)]
pub struct FramebufferBanks {
    words: [[Box<[u16; 1024]>; 4]; 2],
    /// Bit zero/one denote initialized low/high bytes. RTL RAM power-up is undefined.
    initialized: [[Box<[u8; 1024]>; 4]; 2],
    memory_output: [u64; 2],
    render_output: [u64; 2],
    memory_response: Option<(FramebufferPlane, bool)>,
    render_response: bool,
}

impl Default for FramebufferBanks {
    fn default() -> Self {
        Self {
            words: std::array::from_fn(|_| std::array::from_fn(|_| Box::new([0; 1024]))),
            initialized: std::array::from_fn(|_| std::array::from_fn(|_| Box::new([0; 1024]))),
            memory_output: [0; 2],
            render_output: [0; 2],
            memory_response: None,
            render_response: false,
        }
    }
}

fn swap_halves(value: u64) -> u64 {
    value.rotate_left(32)
}

fn beat_conflicts_quad(beat: FramebufferBeatAddress, quad: FramebufferQuadAddress) -> bool {
    beat.entry == quad.entry && beat.y >> 1 == quad.y >> 1 && beat.group == quad.x >> 2
}

impl FramebufferBanks {
    pub fn tick(&mut self, input: FramebufferCycleInput) -> FramebufferCycleOutput {
        if let Some(write) = input.memory_write {
            write.address.check();
        }
        if let Some(read) = input.memory_read {
            read.address.check();
        }
        if let Some(write) = input.render_write {
            write.address.check();
            assert!(write.color_mask < 16 && write.depth_mask < 16);
        }
        if let Some(read) = input.render_read {
            read.check();
        }

        let memory_response_consumed =
            !input.reset && self.memory_response.is_some() && input.memory_response_ready;
        let render_response_consumed =
            !input.reset && self.render_response && input.render_response_ready;
        let memory_write_accepted = !input.reset && input.memory_write.is_some();
        let active_memory_write = input
            .memory_write
            .filter(|write| memory_write_accepted && write.byte_mask != 0);
        let memory_read_accepted = !input.reset
            && self
                .memory_response
                .is_none_or(|_| input.memory_response_ready)
            && input.memory_read.is_some_and(|read| {
                active_memory_write.is_none_or(|write| write.plane != read.plane)
            });
        let active_memory_read = input.memory_read.filter(|_| memory_read_accepted);
        let render_write_accepted = !input.reset
            && input.render_write.is_some_and(|write| {
                write.address.aligned()
                    && active_memory_write.is_none_or(|memory| {
                        !beat_conflicts_quad(memory.address, write.address)
                            || match memory.plane {
                                FramebufferPlane::Color => write.color_mask == 0,
                                FramebufferPlane::Depth => write.depth_mask == 0,
                            }
                    })
                    && active_memory_read.is_none_or(|memory| {
                        !beat_conflicts_quad(memory.address, write.address)
                            || match memory.plane {
                                FramebufferPlane::Color => write.color_mask == 0,
                                FramebufferPlane::Depth => write.depth_mask == 0,
                            }
                    })
            });
        let active_render_write = input.render_write.filter(|_| render_write_accepted);
        let render_read_accepted = !input.reset
            && (!self.render_response || input.render_response_ready)
            && input.render_read.is_some_and(|read| {
                read.aligned()
                    && active_render_write
                        .is_none_or(|write| write.color_mask == 0 && write.depth_mask == 0)
                    && active_memory_write
                        .is_none_or(|write| !beat_conflicts_quad(write.address, read))
            });

        // Read old words before either physical port writes on this edge.
        if let Some(read) = active_memory_read {
            let plane = read.plane.index();
            let address = read.address.word();
            assert!(
                (0..4).all(|bank| self.initialized[plane][bank][address] == 3),
                "framebuffer memory read before LOAD/CLEAR initialization"
            );
            self.memory_output[plane] = (0..4).fold(0_u64, |data, bank| {
                data | (u64::from(self.words[plane][bank][address]) << (16 * bank))
            });
        }
        if render_read_accepted {
            let address = input.render_read.unwrap();
            for plane in 0..2 {
                assert!(
                    (0..4).all(|bank| self.initialized[plane][bank][address.bank_word(bank)] == 3),
                    "framebuffer render read before C/Z initialization"
                );
                self.render_output[plane] = (0..4).fold(0_u64, |data, bank| {
                    data | (u64::from(self.words[plane][bank][address.bank_word(bank)])
                        << (16 * bank))
                });
            }
        }

        if let Some(write) = active_memory_write {
            let plane = write.plane.index();
            let address = write.address.word();
            let data = if write.address.y & 1 != 0 {
                swap_halves(write.data)
            } else {
                write.data
            };
            let mask = if write.address.y & 1 != 0 {
                write.byte_mask.rotate_left(4)
            } else {
                write.byte_mask
            };
            for bank in 0..4 {
                let word = &mut self.words[plane][bank][address];
                for byte in 0..2 {
                    if mask & (1 << (2 * bank + byte)) != 0 {
                        let shift = 16 * bank + 8 * byte;
                        let byte_mask = 0xff_u16 << (8 * byte);
                        *word = (*word & !byte_mask)
                            | (((data >> shift) as u16) << (8 * byte) & byte_mask);
                        self.initialized[plane][bank][address] |= 1 << byte;
                    }
                }
            }
        }
        if let Some(write) = active_render_write {
            for plane in 0..2 {
                let (data, mask) = if plane == 0 {
                    (write.colors, write.color_mask)
                } else {
                    (write.depths, write.depth_mask)
                };
                for bank in 0..4 {
                    if mask & (1 << bank) != 0 {
                        let address = write.address.bank_word(bank);
                        self.words[plane][bank][address] = (data >> (16 * bank)) as u16;
                        self.initialized[plane][bank][address] = 3;
                    }
                }
            }
        }

        if input.reset {
            self.memory_response = None;
            self.render_response = false;
        } else {
            if input.memory_response_ready {
                self.memory_response = None;
            }
            if input.render_response_ready {
                self.render_response = false;
            }
            if let Some(read) = active_memory_read {
                self.memory_response = Some((read.plane, read.address.y & 1 != 0));
            }
            if render_read_accepted {
                self.render_response = true;
            }
        }
        FramebufferCycleOutput {
            memory_write_accepted,
            memory_read_accepted,
            render_write_accepted,
            render_read_accepted,
            memory_response_consumed,
            render_response_consumed,
            memory_response: self.memory_response.map(|(plane, odd_row)| {
                let data = self.memory_output[plane.index()];
                if odd_row {
                    swap_halves(data)
                } else {
                    data
                }
            }),
            render_response: self
                .render_response
                .then_some((self.render_output[0], self.render_output[1])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_color_and_depth_banks_read_back_with_odd_row_swizzle() {
        let mut banks = FramebufferBanks::default();
        for plane in [FramebufferPlane::Color, FramebufferPlane::Depth] {
            for y in 0..16 {
                for group in 0..4 {
                    let address = FramebufferBeatAddress { entry: 2, y, group };
                    let data = (0..4).fold(0_u64, |value, pixel| {
                        value
                            | (u64::from(u16::from(y) * 16 + u16::from(group) * 4 + pixel)
                                << (pixel * 16))
                    });
                    assert!(
                        banks
                            .tick(FramebufferCycleInput {
                                memory_write: Some(FramebufferMemoryWrite {
                                    plane,
                                    address,
                                    data,
                                    byte_mask: 0xff
                                }),
                                ..Default::default()
                            })
                            .memory_write_accepted
                    );
                }
            }
        }
        for y in (0..16).step_by(2) {
            for x in (0..16).step_by(2) {
                let result = banks.tick(FramebufferCycleInput {
                    render_read: Some(FramebufferQuadAddress { entry: 2, x, y }),
                    ..Default::default()
                });
                assert!(result.render_read_accepted);
                let (color, depth) = result.render_response.unwrap();
                assert_eq!(color, depth);
                let mut pixels = [0_u16; 4];
                for (bank, pixel) in pixels.iter_mut().enumerate() {
                    *pixel = (color >> (bank * 16)) as u16;
                }
                let even = [
                    u16::from(y) * 16 + u16::from(x),
                    u16::from(y) * 16 + u16::from(x) + 1,
                ];
                let odd = [
                    (u16::from(y) + 1) * 16 + u16::from(x),
                    (u16::from(y) + 1) * 16 + u16::from(x) + 1,
                ];
                let expected = if x & 2 == 0 {
                    [even[0], even[1], odd[0], odd[1]]
                } else {
                    [odd[0], odd[1], even[0], even[1]]
                };
                assert_eq!(pixels, expected, "quad ({x},{y})");
            }
        }
    }

    #[test]
    fn render_response_holds_under_backpressure() {
        let mut banks = FramebufferBanks::default();
        let address = FramebufferBeatAddress {
            entry: 0,
            y: 0,
            group: 0,
        };
        for plane in [FramebufferPlane::Color, FramebufferPlane::Depth] {
            for y in 0..2 {
                banks.tick(FramebufferCycleInput {
                    memory_write: Some(FramebufferMemoryWrite {
                        plane,
                        address: FramebufferBeatAddress { y, ..address },
                        data: 0x4444_3333_2222_1111,
                        byte_mask: 0xff,
                    }),
                    ..Default::default()
                });
            }
        }
        let quad = FramebufferQuadAddress {
            entry: 0,
            x: 0,
            y: 0,
        };
        let first = banks.tick(FramebufferCycleInput {
            render_read: Some(quad),
            render_response_ready: false,
            ..Default::default()
        });
        assert!(first.render_read_accepted);
        for _ in 0..3 {
            let stalled = banks.tick(FramebufferCycleInput {
                render_read: Some(quad),
                render_response_ready: false,
                ..Default::default()
            });
            assert!(!stalled.render_read_accepted);
            assert_eq!(stalled.render_response, first.render_response);
        }
    }

    #[test]
    fn plane_independence_and_same_bank_collision_are_explicit() {
        let mut banks = FramebufferBanks::default();
        let address = FramebufferBeatAddress {
            entry: 0,
            y: 0,
            group: 0,
        };
        let color = FramebufferMemoryWrite {
            plane: FramebufferPlane::Color,
            address,
            data: 0x4444_3333_2222_1111,
            byte_mask: 0xff,
        };
        let depth = FramebufferMemoryWrite {
            plane: FramebufferPlane::Depth,
            address,
            data: 0xdddd_cccc_bbbb_aaaa,
            byte_mask: 0xff,
        };
        banks.tick(FramebufferCycleInput {
            memory_write: Some(color),
            ..Default::default()
        });
        let independent = banks.tick(FramebufferCycleInput {
            memory_write: Some(depth),
            memory_read: Some(FramebufferMemoryRead {
                plane: FramebufferPlane::Color,
                address,
            }),
            ..Default::default()
        });
        assert!(independent.memory_write_accepted && independent.memory_read_accepted);
        assert_eq!(independent.memory_response, Some(color.data));
        let same_plane = banks.tick(FramebufferCycleInput {
            memory_write: Some(color),
            memory_read: Some(FramebufferMemoryRead {
                plane: FramebufferPlane::Color,
                address,
            }),
            ..Default::default()
        });
        assert!(same_plane.memory_write_accepted);
        assert!(!same_plane.memory_read_accepted);
        let no_op_write = banks.tick(FramebufferCycleInput {
            memory_write: Some(FramebufferMemoryWrite {
                byte_mask: 0,
                ..color
            }),
            memory_read: Some(FramebufferMemoryRead {
                plane: FramebufferPlane::Color,
                address,
            }),
            ..Default::default()
        });
        assert!(no_op_write.memory_read_accepted);
    }
}
