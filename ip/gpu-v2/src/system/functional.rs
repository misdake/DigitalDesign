//! Bounded execution of the current native command ABI, without RTL timing.
//!
//! Uses physical halfwords directly; successful completion means every write is
//! done. Geometry and shading follow the selected native integer contracts.
use super::vliw_command::{self as abi, Command, DeviceSubmission};
use std::fmt;
#[path = "functional/geometry.rs"]
mod geometry;
#[path = "functional/lighting.rs"]
mod lighting;
#[path = "functional/shading.rs"]
mod shading;

#[cfg(feature = "functional-batch")]
pub use geometry::batch::{BatchStats, RebinMode};

pub const PLANE_WORDS: usize = 400 * 256;
const MEMORY_WORDS: usize = 0x400000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionError {
    pub byte_pc: u32,
    pub word: u32,
    pub reason: String,
    pub effects: Effects,
}
impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GPU command at {:#x} ({:#010x}): {}",
            self.byte_pc, self.word, self.reason
        )
    }
}
impl std::error::Error for ExecutionError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    /// Conservative physical byte envelope [first, end) of completed writes.
    /// Gaps inside this envelope need not have changed; memory is authoritative.
    pub written_bytes: Option<(u32, u32)>,
    /// Last successfully bound color/depth byte addresses, not a new target ID.
    pub target: Option<[u32; 2]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    work: usize,
    pub effects: Effects,
}
impl Report {
    fn charge(&mut self, work: usize, maximum: usize) -> Result<(), String> {
        self.work = self.work.checked_add(work).ok_or("work overflow")?;
        if self.work > maximum {
            return Err("functional GPU work limit".into());
        }
        Ok(())
    }
}
fn column_offset(x: usize, y: usize) -> usize {
    ((x >> 4) * 16 + (y >> 4)) * 256 + (y & 15) * 16 + (x & 15)
}

/// Work is counted in commands and halfwords touched, never GPU cycles.
pub struct Executor {
    maximum_work: usize,
    #[cfg(feature = "functional-batch")]
    batch_stats: BatchStats,
    #[cfg(feature = "functional-batch")]
    rebin_mode: RebinMode,
}
impl Default for Executor {
    fn default() -> Self {
        Self::new(64_000_000)
    }
}
impl Executor {
    pub fn new(maximum_work: usize) -> Self {
        Self {
            maximum_work,
            #[cfg(feature = "functional-batch")]
            batch_stats: BatchStats::default(),
            #[cfg(feature = "functional-batch")]
            rebin_mode: RebinMode::default(),
        }
    }

    #[cfg(feature = "functional-batch")]
    pub fn batch_stats(&self) -> &BatchStats {
        &self.batch_stats
    }
    #[cfg(feature = "functional-batch")]
    pub fn with_rebin_mode(mut self, mode: RebinMode) -> Self {
        self.rebin_mode = mode;
        self
    }

    pub fn execute(
        &mut self,
        memory: &mut [u16],
        submission: DeviceSubmission,
    ) -> Result<Report, ExecutionError> {
        self.execute_memory(
            &mut Memory {
                words: memory,
                access: None,
                effects: Effects::default(),
            },
            submission,
        )
    }

    /// Capture flags are per physical byte: bit0 known, bit1 writable.
    /// Successful GPU writes also make their bytes known for later commands.
    pub fn execute_captured(
        &mut self,
        memory: &mut [u16],
        access: &mut [u8],
        submission: DeviceSubmission,
    ) -> Result<Report, ExecutionError> {
        #[cfg(feature = "functional-batch")]
        {
            self.batch_stats = BatchStats::default();
        }
        if access.len() != memory.len() * 2 || access.iter().any(|&flags| flags > 3) {
            return Err(ExecutionError {
                byte_pc: 0,
                word: 0,
                reason: "capture byte access mask".into(),
                effects: Effects::default(),
            });
        }
        self.execute_memory(
            &mut Memory {
                words: memory,
                access: Some(access),
                effects: Effects::default(),
            },
            submission,
        )
    }

    fn execute_memory(
        &mut self,
        memory: &mut Memory<'_>,
        submission: DeviceSubmission,
    ) -> Result<Report, ExecutionError> {
        #[cfg(feature = "functional-batch")]
        {
            self.batch_stats = BatchStats::default();
        }
        let outcome = self.execute_commands(memory, submission);
        match outcome {
            Ok(mut report) => {
                report.effects = memory.effects;
                Ok(report)
            }
            Err(mut error) => {
                error.effects = memory.effects;
                Err(error)
            }
        }
    }

    fn execute_commands(
        &mut self,
        memory: &mut Memory<'_>,
        submission: DeviceSubmission,
    ) -> Result<Report, ExecutionError> {
        let base = usize::from(submission.base_sector) * 64;
        let count = usize::from(submission.count_minus_one) + 1;
        let error = |byte_pc, word, reason: String| ExecutionError {
            byte_pc,
            word,
            reason,
            effects: Effects::default(),
        };
        abi::DeviceSubmission::new((base * 2) as u32, (count * 4) as u32)
            .and_then(|_| bounds(memory, base, count.div_ceil(32) * 64).map(|_| ()))
            .map_err(|reason| error(0, 0, reason.into()))?;
        let mut state = State::default();
        let mut pc = 0;
        #[cfg(feature = "functional-batch")]
        let mut batch_allowed = true;
        while pc < count {
            let at = base + pc * 2;
            let instruction = memory
                .read(at..at + 2)
                .map_err(|reason| error((pc * 4) as u32, 0, reason))?;
            let word = u32::from(instruction[0]) | (u32::from(instruction[1]) << 16);
            #[cfg(feature = "functional-batch")]
            if batch_allowed && word >> 28 == 3 {
                match geometry::batch::try_execute(
                    memory,
                    &state,
                    base,
                    pc..count,
                    self.maximum_work,
                    self.rebin_mode,
                    &mut self.batch_stats,
                ) {
                    geometry::batch::Attempt::Applied(next, next_pc) => {
                        state = *next;
                        pc = next_pc;
                        continue;
                    }
                    geometry::batch::Attempt::Fallback => batch_allowed = false,
                    geometry::batch::Attempt::Skip => {}
                }
            }
            let result = state.command(memory, word, self.maximum_work);
            let end = result.map_err(|reason| error((pc * 4) as u32, word, reason))?;
            if end {
                if pc + 1 != count {
                    return Err(error(
                        (pc * 4) as u32,
                        word,
                        "END is not the last instruction".into(),
                    ));
                }
                return Ok(state.report);
            }
            pc += 1;
        }
        Err(error((count * 4) as u32, 0, "missing final END".into()))
    }
}

struct Memory<'a> {
    words: &'a mut [u16],
    access: Option<&'a mut [u8]>,
    effects: Effects,
}

impl Memory<'_> {
    fn read(&self, range: std::ops::Range<usize>) -> Result<&[u16], String> {
        bounds(self, range.start, range.len())?;
        if let Some(access) = &self.access {
            if let Some(offset) = access[range.start * 2..range.end * 2]
                .iter()
                .position(|flags| flags & 1 == 0)
            {
                return Err(format!(
                    "read of uncaptured byte {:#x}",
                    range.start * 2 + offset
                ));
            }
        }
        Ok(&self.words[range])
    }

    fn write(&mut self, range: std::ops::Range<usize>) -> Result<&mut [u16], String> {
        bounds(self, range.start, range.len())?;
        if let Some(access) = &mut self.access {
            let bytes = &mut access[range.start * 2..range.end * 2];
            if let Some(offset) = bytes.iter().position(|flags| flags & 2 == 0) {
                return Err(format!(
                    "write outside captured writable bytes at {:#x}",
                    range.start * 2 + offset
                ));
            }
            for flags in bytes {
                *flags |= 1;
            }
        }
        let first = (range.start * 2) as u32;
        let end = (range.end * 2) as u32;
        self.effects.written_bytes = Some(match self.effects.written_bytes {
            Some((old_first, old_end)) => (old_first.min(first), old_end.max(end)),
            None => (first, end),
        });
        Ok(&mut self.words[range])
    }

    fn load(&self, address: usize) -> Result<u16, String> {
        let range = bounds(self, address, 1)?;
        Ok(self.read(range)?[0])
    }

    fn byte(&self, address: usize) -> Result<u8, String> {
        bounds(self, address / 2, 1)?;
        if self
            .access
            .as_ref()
            .is_some_and(|access| access[address] & 1 == 0)
        {
            return Err(format!("read of uncaptured byte {address:#x}"));
        }
        Ok((self.words[address / 2] >> ((address & 1) * 8)) as u8)
    }

    fn store(&mut self, address: usize, value: u16) -> Result<(), String> {
        let range = bounds(self, address, 1)?;
        self.write(range)?[0] = value;
        Ok(())
    }

    fn copy(
        &mut self,
        from: std::ops::Range<usize>,
        to: std::ops::Range<usize>,
    ) -> Result<(), String> {
        self.read(from.clone())?;
        self.write(to.clone())?;
        self.words.copy_within(from, to.start);
        Ok(())
    }
}

fn bounds(
    memory: &Memory<'_>,
    start: usize,
    count: usize,
) -> Result<std::ops::Range<usize>, &'static str> {
    let end = start
        .checked_add(count)
        .ok_or("physical address overflow")?;
    if end > memory.words.len().min(MEMORY_WORDS) {
        return Err("physical memory extent");
    }
    Ok(start..end)
}

#[derive(Clone)]
struct State {
    local: [u16; 2048],
    loaded: [bool; 128],
    target: Option<[usize; 2]>,
    global: Option<shading::Global>,
    render: Option<shading::RenderState>,
    pages: [Option<geometry::Meshlet>; 2],
    uniforms: [bool; 4],
    all_fence: bool,
    stored: bool,
    report: Report,
    #[cfg(feature = "functional-batch")]
    pending: Option<Vec<geometry::batch::DrawInput>>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            local: [0; 2048],
            loaded: [false; 128],
            target: None,
            global: None,
            render: None,
            pages: [None, None],
            uniforms: [false; 4],
            all_fence: false,
            stored: false,
            report: Report::default(),
            #[cfg(feature = "functional-batch")]
            pending: None,
        }
    }
}
impl State {
    fn charge(&mut self, work: usize, maximum: usize) -> Result<(), String> {
        self.report.charge(work, maximum)
    }

    fn fenced(&self, stored: bool) -> Result<(), String> {
        if !self.all_fence || (stored && !self.stored) {
            return Err("explicit WAIT(all)/FLUSH boundary required".into());
        }
        Ok(())
    }

    fn object<const N: usize>(&self, word: u32, ranged_ui: bool) -> Result<[u8; N], String> {
        let slot = (word & 127) as usize;
        if !(66..80).contains(&slot)
            || slot + N / 32 > 80
            || (!ranged_ui && word & 0x0fff_ff80 != 0)
        {
            return Err("object instruction encoding/range".into());
        }
        if !self.loaded[slot..slot + N / 32]
            .iter()
            .all(|&loaded| loaded)
        {
            return Err("object has not been DMA loaded in this submission".into());
        }
        let mut bytes = [0; N];
        for (out, value) in bytes
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&self.local[slot * 16..slot * 16 + N / 2])
        {
            out.copy_from_slice(&value.to_le_bytes());
        }
        Ok(bytes)
    }

    fn command(
        &mut self,
        memory: &mut Memory<'_>,
        word: u32,
        maximum: usize,
    ) -> Result<bool, String> {
        self.charge(1, maximum)?;
        match word >> 28 {
            0 if word == Command::NOP.0 => {}
            1 => {
                let source = ((word >> 10) & 0x3ffff) * 32;
                let slot = ((word >> 3) & 127) as usize;
                let size = match (word >> 1) & 3 {
                    0 => 32,
                    1 => 64,
                    2 => 128,
                    _ => return Err("DMA_LOCAL size".into()),
                };
                if Command::dma_local(source, (slot * 4) as u16, size)?.0 != word {
                    return Err("DMA_LOCAL reserved bits".into());
                }
                let count = size as usize / 2;
                let range = bounds(memory, source as usize / 2, count)?;
                self.charge(count, maximum)?;
                self.local[slot * 16..slot * 16 + count].copy_from_slice(memory.read(range)?);
                self.loaded[slot..slot + size as usize / 32].fill(true);
                if slot >= 80 {
                    self.uniforms[(slot - 80) / 4] = true;
                }
            }
            2 => {
                let source = (word >> 12) & 65535;
                let slot = (word & 1) as usize;
                let bytes = (((word >> 1) & 7) + 1) * 128;
                if Command::dma_meshlet(source * 128, slot as u8, bytes)?.0 != word {
                    return Err("DMA_MESHLET reserved bits".into());
                }
                let header = bounds(memory, source as usize * 64, 16)?;
                let payload = bounds(memory, source as usize * 64 + 64, bytes as usize / 2)?;
                self.charge(16 + bytes as usize / 2, maximum)?;
                self.pages[slot] = Some(geometry::Meshlet::decode(
                    memory.read(header)?,
                    memory.read(payload)?,
                )?);
            }
            3 => {
                let slot = (word & 1) as usize;
                if Command::draw_meshlet(slot as u8)?.0 != word {
                    return Err("DRAW_MESHLET reserved bits".into());
                }
                let mesh = self.pages[slot]
                    .as_ref()
                    .ok_or("DRAW_MESHLET needs a loaded input page")?
                    .clone();
                if !self.uniforms[mesh.uniform] {
                    return Err("DRAW_MESHLET needs a loaded Instance128".into());
                }
                let start = (80 + mesh.uniform * 4) * 16;
                let uniform = std::array::from_fn(|i| {
                    u32::from(self.local[start + 2 * i])
                        | (u32::from(self.local[start + 2 * i + 1]) << 16)
                });
                let global = self.global.ok_or("DRAW_MESHLET needs SET_GLOBAL")?;
                let render = self.render.ok_or("DRAW_MESHLET needs SET_RENDER")?;
                let target = self.target.ok_or("DRAW_MESHLET needs SET_TARGET")?;
                render.texture.validate_extent(memory)?;
                self.all_fence = false;
                self.stored = false;
                let state = shading::DrawState {
                    global,
                    render,
                    target,
                    color: uniform[22],
                    rgb: mesh.rgb,
                };
                #[cfg(feature = "functional-batch")]
                if let Some(pending) = &mut self.pending {
                    pending.push(geometry::batch::DrawInput {
                        mesh,
                        uniform,
                        state,
                        pc: 0, // Collector installs the actual command PC.
                    });
                    return Ok(false);
                }
                geometry::draw(memory, &mesh, &uniform, state, &mut self.report, maximum)?;
            }
            4 if word & 0x0fff_fffc == 0 => {
                if word & 2 != 0 {
                    self.all_fence = true;
                }
            }
            5 => {
                self.fenced(false)?;
                let object = self.object::<64>(word, false)?;
                self.global = Some(shading::Global::decode(object)?);
            }
            6 => {
                self.fenced(false)?;
                let object = self.object::<32>(word, false)?;
                self.render = Some(shading::RenderState::decode(object)?);
            }
            7 if word == Command::FLUSH.0 => {
                self.fenced(false)?;
                // All supported writes have already reached physical memory.
                self.stored = true;
            }
            8 if word & 0x0ffe_0000 == 0 => {
                self.fenced(false)?;
                let target = self.target.ok_or("CLEAR requires SET_TARGET")?;
                let plane = usize::from(word & (1 << 16) != 0);
                let range = bounds(memory, target[plane], PLANE_WORDS)?;
                self.charge(PLANE_WORDS, maximum)?;
                memory.write(range)?.fill(word as u16);
            }
            9 if word == Command::TEX_INVALIDATE.0 => {
                self.fenced(false)?;
                // This direct-memory backend retains no Sampling cache lines.
            }
            10 => {
                self.fenced(true)?;
                let object = self.object::<32>(word, false)?;
                let descriptor = u64::from_le_bytes(object[..8].try_into().unwrap());
                let color = ((descriptor & 0x3fff) * 512) as u32;
                let depth = (((descriptor >> 14) & 0x3fff) * 512) as u32;
                let surface = crate::framebuffer::ports::MaterializedSurface {
                    width: 400,
                    height: 240,
                    color_base_bytes: color,
                    depth_base_bytes: depth,
                };
                if abi::target_params(surface)? != object {
                    return Err("SET_TARGET reserved data".into());
                }
                bounds(memory, color as usize / 2, PLANE_WORDS)?;
                bounds(memory, depth as usize / 2, PLANE_WORDS)?;
                self.target = Some([color as usize / 2, depth as usize / 2]);
                memory.effects.target = Some([color, depth]);
                self.stored = false;
            }
            11 => {
                self.fenced(true)?;
                let target = self.target.ok_or("UI_GRID requires SET_TARGET")?;
                let ranged = word & (1 << 27) != 0;
                let object = self.object::<32>(word, ranged)?;
                let slot = (word & 127) as u16 * 4;
                let first = if ranged {
                    ((word >> 7) & 1023) as u16
                } else {
                    0
                };
                let end = if ranged {
                    (((word >> 17) & 1023) + 1) as u16
                } else {
                    750
                };
                let canonical = if ranged {
                    Command::ui_grid_range(slot, first, end)?
                } else {
                    Command::ui_grid(slot)?
                };
                if canonical.0 != word {
                    return Err("UI_GRID instruction encoding".into());
                }
                let descriptor = u64::from_le_bytes(object[..8].try_into().unwrap());
                let grid = (descriptor & 65535) as u32 * 128;
                let atlas = ((descriptor >> 16) & 127) as u32 * 65536;
                if abi::ui_params(grid, atlas)? != object {
                    return Err("UI_GRID reserved data".into());
                }
                let grid_range = bounds(memory, grid as usize / 2, 384)?;
                let atlas_range = bounds(memory, atlas as usize / 2, 32768)?;
                let color = target[0]..target[0] + PLANE_WORDS;
                for asset in [grid_range, atlas_range] {
                    if asset.start < color.end && color.start < asset.end {
                        return Err("UI_GRID asset overlaps the color target".into());
                    }
                }
                for cell in usize::from(first)..usize::from(end) {
                    self.charge(1, maximum)?;
                    let address = grid as usize + cell;
                    let char_id = usize::from(memory.byte(address)?);
                    if char_id == 255 {
                        continue;
                    }
                    for y in 0..16 {
                        for x in 0..8 {
                            self.charge(1, maximum)?;
                            // Odd atlas rows store each group as 2,3,0,1.
                            let stored_x = x ^ if y & 1 != 0 { 2 } else { 0 };
                            let source = atlas as usize / 2 + char_id * 128 + y * 8 + stored_x;
                            let rgba = memory.load(source)?;
                            if rgba & 1 != 0 {
                                self.charge(1, maximum)?;
                                let pixel_x = cell % 50 * 8 + x;
                                let pixel_y = cell / 50 * 16 + y;
                                let offset = column_offset(pixel_x, pixel_y);
                                memory.store(
                                    target[0] + offset,
                                    (rgba & 0xffc0) | ((rgba >> 5) & 32) | ((rgba >> 1) & 31),
                                )?;
                            }
                        }
                    }
                }
                self.pages = [None, None];
            }
            12 => {
                self.fenced(true)?;
                let object = self.object::<32>(word, false)?;
                let descriptor = u64::from_le_bytes(object[..8].try_into().unwrap());
                let source = ((descriptor & 0x3fff) * 512) as u32;
                let destination = (((descriptor >> 14) & 0x3fff) * 512) as u32;
                let bytes = (((descriptor >> 28) & 0x7ff) + 1) as u32 * 512;
                if abi::memcpy_params(source, destination, bytes)? != object {
                    return Err("MEMCPY reserved data".into());
                }
                let from = bounds(memory, source as usize / 2, bytes as usize / 2)?;
                let to = bounds(memory, destination as usize / 2, bytes as usize / 2)?;
                self.charge(bytes as usize, maximum)?;
                memory.copy(from, to)?;
            }
            15 if word == Command::END.0 => {
                self.fenced(true)?;
                return Ok(true);
            }
            _ => return Err("unknown opcode or reserved instruction bits".into()),
        }
        Ok(false)
    }
}
