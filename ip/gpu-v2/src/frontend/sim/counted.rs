//! Component-owned numerical frames are consumed only at their typed boundaries.
use super::{super::ports::*, oracle};
use crate::{
    command_processor::sim::counted as command_counted,
    scratchpad::sim::counted::{self as sp, Transaction, VertexRead},
    vertex::sim::counted as vertex,
};
pub struct Report {
    pub output: oracle::Report,
    pub commands: audited::FrameReport,
    pub scratchpad: sp::Report,
    pub vertices: Vec<vertex::BatchReport>,
}
pub fn run(input: &Input) -> Result<Report, String> {
    let commands = command_counted::run(&input.commands, 64)?;
    let output = oracle::run(input)?;
    let mut transactions = Vec::new();
    let mut vertices = Vec::new();
    let mut packets = Vec::new();
    let address = |name: &str| {
        commands
            .outputs
            .iter()
            .find(|o| o.name == name)
            .map(|o| o.raw)
            .ok_or_else(|| format!("missing command address {name}"))
    };
    for (command_index, command) in input.commands.iter().enumerate() {
        match command {
            crate::command_processor::ports::Command::Dma(d) => {
                for beat in 0..d.byte_count / 8 {
                    transactions.push(Transaction::DmaWrite {
                        address: address(&format!("command.{command_index}.dma.{beat}.scratchpad"))?
                            as usize,
                        data: input.read_beat(address(&format!(
                            "command.{command_index}.dma.{beat}.physical"
                        ))? as u64)?,
                    });
                }
            }
            crate::command_processor::ports::Command::Draw {
                region: _,
                byte_offset: _,
                vertices: count,
                context,
            } => {
                let mut addresses = Vec::new();
                let prior_reads = transactions
                    .iter()
                    .filter(|t| matches!(t, Transaction::CoreRead { .. }))
                    .count();
                let packet_addresses = (0..*count)
                    .map(|v| {
                        Ok((
                            address(&format!("command.{command_index}.vertex.{v}.first"))? as usize,
                            address(&format!("command.{command_index}.vertex.{v}.second"))?
                                as usize,
                            address(&format!("command.{command_index}.vertex.{v}.high_half"))? != 0,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                for &(first, second, _) in &packet_addresses {
                    for addr in [first, second] {
                        if !addresses.contains(&addr) {
                            addresses.push(addr);
                            transactions.push(Transaction::CoreRead { address: addr });
                        }
                    }
                }
                for (first, second, high_half) in packet_addresses {
                    packets.push(VertexRead {
                        first: prior_reads
                            + addresses
                                .iter()
                                .position(|&a| a == first)
                                .ok_or("first packet read")?,
                        second: prior_reads
                            + addresses
                                .iter()
                                .position(|&a| a == second)
                                .ok_or("second packet read")?,
                        high_half,
                    });
                }
                // This consumed scratchpad frame is the only payload bridge.
                let scratch = sp::run_with_vertices(&transactions, &packets)?;
                let packed = scratch.vertices[scratch.vertices.len() - count..]
                    .iter()
                    .map(|&bits| {
                        crate::vertex::ports::PackedVertex([
                            bits as u32,
                            (bits >> 32) as u32,
                            (bits >> 64) as u32,
                        ])
                    })
                    .collect::<Vec<_>>();
                vertices.push(vertex::run_batch(context, &packed)?);
            }
            _ => {}
        }
    }
    let scratchpad = sp::run_with_vertices(&transactions, &packets)?;
    if vertices.len() != output.outputs.len()
        || vertices
            .iter()
            .zip(&output.outputs)
            .any(|(v, o)| v.outputs != o.vertices)
    {
        return Err("frontend stage mismatch".into());
    }
    Ok(Report {
        output,
        commands,
        scratchpad,
        vertices,
    })
}
