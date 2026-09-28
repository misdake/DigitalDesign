//! Functional meshlet-to-colored-quad reference for the small geometry core.
//!
//! This model has no clock, DSP ownership, or ready/valid timing. It defines
//! source order, errors, clipping, fan order, coverage, colors, and the raw
//! slot ABI. The cycle emulator and RTL consume the same typed meshlets.

use super::{clip_color_triangle, color_at, ColorVertex};
use crate::hardware::gpu::rastersim::{raster, setup};

pub const SLOT_WORDS: usize = 512;
pub const MAX_VERTICES: usize = 64;
pub const MAX_TRIANGLES: usize = 128;
pub const VERTEX_BASE: usize = 8;
pub const TRIANGLE_BASE: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawVertex {
    pub position: [i32; 4],
    pub rgb565: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Meshlet {
    /// Row-major Q16.16 matrix, with no implicit W component.
    pub matrix: [[i32; 4]; 4],
    pub vertices: Vec<RawVertex>,
    pub triangles: Vec<[u8; 3]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFault {
    Index { corner: u8, index: u8 },
    MvpOverflow { corner: u8, row: u8 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimError {
    Capacity,
    RecordLimit { max_records: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimRecord {
    /// Every clipped fan triangle is visible here, even if coverage rejects it.
    Fan {
        slot: u8,
        source: u8,
        primitive: u8,
        clip: [ColorVertex; 3],
    },
    Quad {
        slot: u8,
        source: u8,
        primitive: u8,
        tile: u16,
        x: u16,
        y: u16,
        mask: u8,
        color: [u16; 4],
    },
    TileEnd {
        slot: u8,
        source: u8,
        primitive: u8,
        tile: u16,
    },
    PrimitiveEnd {
        slot: u8,
        source: u8,
        primitive: u8,
    },
    SourceEnd {
        slot: u8,
        source: u8,
        last: bool,
        fault: Option<SourceFault>,
    },
}

impl Meshlet {
    fn check_capacity(&self) -> Result<(), SimError> {
        if self.vertices.len() > MAX_VERTICES || self.triangles.len() > MAX_TRIANGLES {
            Err(SimError::Capacity)
        } else {
            Ok(())
        }
    }

    /// Encodes exactly the 512x64 word layout used by `GpuMeshletMicrocore`.
    pub fn words(&self) -> Result<[u64; SLOT_WORDS], SimError> {
        self.check_capacity()?;
        let mut words = [0; SLOT_WORDS];
        for row in 0..4 {
            for pair in 0..2 {
                let low = self.matrix[row][pair * 2] as u32;
                let high = self.matrix[row][pair * 2 + 1] as u32;
                words[row * 2 + pair] = u64::from(low) | (u64::from(high) << 32);
            }
        }
        for (i, vertex) in self.vertices.iter().enumerate() {
            let at = VERTEX_BASE + i * 3;
            words[at] =
                u64::from(vertex.position[0] as u32) | (u64::from(vertex.position[1] as u32) << 32);
            words[at + 1] =
                u64::from(vertex.position[2] as u32) | (u64::from(vertex.position[3] as u32) << 32);
            words[at + 2] = u64::from(vertex.rgb565);
        }
        for (i, triangle) in self.triangles.iter().enumerate() {
            words[TRIANGLE_BASE + i] = u64::from(triangle[0])
                | (u64::from(triangle[1]) << 8)
                | (u64::from(triangle[2]) << 16);
        }
        Ok(words)
    }

    /// Decodes a slot image without checking triangle indices. Bad indices
    /// become per-source error records when the meshlet runs.
    pub fn from_words(
        words: &[u64; SLOT_WORDS],
        vertex_count: usize,
        triangle_count: usize,
    ) -> Result<Self, SimError> {
        if vertex_count > MAX_VERTICES || triangle_count > MAX_TRIANGLES {
            return Err(SimError::Capacity);
        }
        let matrix = std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                let pair = words[row * 2 + column / 2];
                (pair >> (column % 2 * 32)) as u32 as i32
            })
        });
        let vertices = (0..vertex_count)
            .map(|i| {
                let at = VERTEX_BASE + i * 3;
                RawVertex {
                    position: [
                        words[at] as u32 as i32,
                        (words[at] >> 32) as u32 as i32,
                        words[at + 1] as u32 as i32,
                        (words[at + 1] >> 32) as u32 as i32,
                    ],
                    rgb565: words[at + 2] as u16,
                }
            })
            .collect();
        let triangles = (0..triangle_count)
            .map(|i| {
                let word = words[TRIANGLE_BASE + i];
                [word as u8, (word >> 8) as u8, (word >> 16) as u8]
            })
            .collect();
        Ok(Self {
            matrix,
            vertices,
            triangles,
        })
    }
}

/// Signed full-width dot product with round-to-nearest, ties-to-even Q16.16.
fn transform(position: [i32; 4], matrix: &[[i32; 4]; 4]) -> Result<[i32; 4], u8> {
    let mut clip = [0; 4];
    for row in 0..4 {
        let sum: i128 = (0..4)
            .map(|column| i128::from(matrix[row][column]) * i128::from(position[column]))
            .sum();
        let quotient = sum.div_euclid(1 << 16);
        let remainder = sum.rem_euclid(1 << 16);
        let increment = remainder > (1 << 15) || (remainder == (1 << 15) && quotient & 1 != 0);
        clip[row] = i32::try_from(quotient + i128::from(increment)).map_err(|_| row as u8)?;
    }
    Ok(clip)
}

fn source_vertices(meshlet: &Meshlet, indices: [u8; 3]) -> Result<[ColorVertex; 3], SourceFault> {
    let mut vertices = [ColorVertex::from_clip_rgb565([0; 4], 0); 3];
    for (corner, index) in indices.into_iter().enumerate() {
        let Some(raw) = meshlet.vertices.get(usize::from(index)) else {
            return Err(SourceFault::Index {
                corner: corner as u8,
                index,
            });
        };
        let clip =
            transform(raw.position, &meshlet.matrix).map_err(|row| SourceFault::MvpOverflow {
                corner: corner as u8,
                row,
            })?;
        vertices[corner] = ColorVertex::from_clip_rgb565(clip, raw.rgb565);
    }
    Ok(vertices)
}

fn push(
    records: &mut Vec<SimRecord>,
    record: SimRecord,
    max_records: usize,
) -> Result<(), SimError> {
    if records.len() >= max_records {
        return Err(SimError::RecordLimit { max_records });
    }
    records.push(record);
    Ok(())
}

/// Functional reference. `max_records` bounds every test and caller run.
pub fn run_meshlet(
    slot: u8,
    meshlet: &Meshlet,
    max_records: usize,
) -> Result<Vec<SimRecord>, SimError> {
    meshlet.check_capacity()?;
    let mut records = Vec::new();
    let mut unit = setup::SetupUnit::default();
    for (source, indices) in meshlet.triangles.iter().copied().enumerate() {
        let fault = match source_vertices(meshlet, indices) {
            Ok(vertices) => {
                for (primitive, triangle) in clip_color_triangle(vertices).into_iter().enumerate() {
                    push(
                        &mut records,
                        SimRecord::Fan {
                            slot,
                            source: source as u8,
                            primitive: primitive as u8,
                            clip: triangle,
                        },
                        max_records,
                    )?;
                    if let Some(geometry) = unit.setup_triangle(&triangle.map(|v| v.clip)) {
                        let produced = std::cell::RefCell::new(Vec::new());
                        raster::traverse(
                            &[geometry],
                            |_, _| {},
                            |_, tile, quad| {
                                if quad.mask != 0 {
                                    let color = std::array::from_fn(|lane| {
                                        if quad.mask & (1 << lane) != 0 {
                                            color_at(
                                                &triangle,
                                                &geometry,
                                                (quad.x + lane as i32 % 2) as u16,
                                                (quad.y + lane as i32 / 2) as u16,
                                            )
                                        } else {
                                            0
                                        }
                                    });
                                    produced.borrow_mut().push(SimRecord::Quad {
                                        slot,
                                        source: source as u8,
                                        primitive: primitive as u8,
                                        tile: tile.index,
                                        x: quad.x as u16,
                                        y: quad.y as u16,
                                        mask: quad.mask,
                                        color,
                                    });
                                }
                            },
                            |_, tile| {
                                produced.borrow_mut().push(SimRecord::TileEnd {
                                    slot,
                                    source: source as u8,
                                    primitive: primitive as u8,
                                    tile: tile.index,
                                });
                            },
                        );
                        for record in produced.into_inner() {
                            push(&mut records, record, max_records)?;
                        }
                    }
                    push(
                        &mut records,
                        SimRecord::PrimitiveEnd {
                            slot,
                            source: source as u8,
                            primitive: primitive as u8,
                        },
                        max_records,
                    )?;
                }
                None
            }
            Err(fault) => Some(fault),
        };
        push(
            &mut records,
            SimRecord::SourceEnd {
                slot,
                source: source as u8,
                last: source + 1 == meshlet.triangles.len(),
                fault,
            },
            max_records,
        )?;
    }
    Ok(records)
}

#[cfg(test)]
#[path = "microcore_sim_tests.rs"]
mod tests;
