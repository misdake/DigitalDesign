//! Sequential semantic reference: DMA fully completes before its data is consumed.
use super::super::ports::*;
use crate::{
    command_processor::{ports::*, sim::oracle as commands},
    scratchpad::{ports::*, sim::oracle::Scratchpad},
    vertex::{ports::*, sim::oracle as transform},
};
pub struct Report {
    pub outputs: Vec<DrawOutput>,
    pub slots: [Slot; 2],
    pub scratchpad: Scratchpad,
    pub fence: bool,
}
pub fn run(input: &Input) -> Result<Report, String> {
    let mut memory = input;
    run_with_memory(input, &mut memory)
}
pub fn run_with_memory(input: &Input, memory: &mut impl MemoryPort) -> Result<Report, String> {
    commands::validate(&input.commands, 64)?;
    if input.memory.len() > 1_048_576 {
        return Err("frontend memory bound".into());
    }
    let mut sp = Scratchpad::default();
    let mut leases = [None; 2];
    let mut tokens = [false; 4];
    let mut slots: [Slot; 2] = std::array::from_fn(|_| Slot::default());
    let mut outputs = Vec::new();
    let mut fence = false;
    for command in &input.commands {
        match command {
            Command::Dma(d) => {
                if tokens[usize::from(d.completion_token)] {
                    return Err("DMA token unacknowledged".into());
                }
                let lease = sp.reserve(*d)?;
                leases[lease.region] = Some(lease);
                let data = memory.read_dma(d.physical_addr, d.byte_count)?;
                if data.len() != d.byte_count / 8 {
                    return Err("DMA source returned the wrong number of beats".into());
                }
                for word in data {
                    sp.dma_beat(lease, word)?;
                }
                sp.complete(lease)?;
                tokens[usize::from(d.completion_token)] = true;
            }
            Command::Wait(token) => {
                if !tokens[usize::from(*token)] {
                    return Err("WAIT has no producer".into());
                }
                tokens[usize::from(*token)] = false;
            }
            Command::Draw {
                region,
                byte_offset,
                vertices,
                context,
            } => {
                let lease = leases[*region].ok_or("DRAW region has no DMA lease")?;
                sp.acquire(lease)?;
                let slot = slots
                    .iter()
                    .position(|s| !s.held)
                    .ok_or("no transformed output credit")?;
                let epoch = slots[slot].allocate()?;
                let mut result = Vec::new();
                for vertex in 0..*vertices {
                    let start = region * REGION_BYTES + byte_offset + vertex * 12;
                    let mut cells = [0; 3];
                    for (k, cell) in cells.iter_mut().enumerate() {
                        let addr = start + k * 4;
                        *cell = (sp.read64(lease, addr & !7)? >> ((addr & 7) * 8)) as u32;
                    }
                    let transformed = transform::run(
                        context,
                        PackedVertex(cells),
                        &transform::Config::default(),
                    )?
                    .output;
                    slots[slot].rows[vertex * 7..vertex * 7 + 7]
                        .copy_from_slice(&transformed.rows());
                    slots[slot].ready[vertex] = true;
                    result.push(transformed);
                }
                sp.release(lease)?;
                slots[slot].finish_production(epoch)?;
                outputs.push(DrawOutput {
                    slot,
                    epoch,
                    vertices: result,
                });
            }
            Command::Release { slot, epoch } => slots[*slot].release(*epoch)?,
            Command::Fence => fence = true,
            Command::Unsupported(_) => return Err("unsupported command".into()),
        }
    }
    Ok(Report {
        outputs,
        slots,
        scratchpad: sp,
        fence,
    })
}
