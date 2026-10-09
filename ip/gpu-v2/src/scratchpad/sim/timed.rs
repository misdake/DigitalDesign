//! Independent checks for scheduled true-dual-port DMA and core bank accesses.
use super::counted::{Report, Transaction};
use audited::{physical::*, Operation};
fn wiring_certificate(frame: &audited::FrameReport) -> Result<(), String> {
    let mut constant = vec![false; frame.values.len()];
    let mut masks = vec![0_u128; frame.values.len()];
    for e in &frame.events {
        let Some(value) = e.output else { continue };
        let format = frame.values[value].format;
        if format.signed || format.fraction != 0 {
            return Err("scratchpad interface format".into());
        }
        let limit = (1_u128 << format.bits) - 1;
        let is_constant = matches!(e.operation, Operation::Literal)
            || !e.inputs.is_empty()
                && e.inputs.iter().all(|&v| constant[v])
                && !matches!(e.operation, Operation::Read { .. });
        let mask = if is_constant {
            frame.values[value].raw as u128
        } else {
            match e.operation {
                Operation::Read { .. } => limit,
                Operation::Resize | Operation::BinaryScale => masks[e.inputs[0]] & limit,
                Operation::Slice(offset) => (masks[e.inputs[0]] >> offset) & limit,
                Operation::ShiftLeft(amount) => (masks[e.inputs[0]] << amount) & limit,
                Operation::Add
                    if e.inputs.len() == 2 && masks[e.inputs[0]] & masks[e.inputs[1]] == 0 =>
                {
                    masks[e.inputs[0]] | masks[e.inputs[1]]
                }
                _ => {
                    return Err(
                        "scratchpad arithmetic is not certified constant/concatenation wiring"
                            .into(),
                    )
                }
            }
        };
        constant[value] = is_constant;
        masks[value] = mask;
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub transaction: usize,
    pub issue: u64,
    pub ready: u64,
}
pub fn audit(report: &Report, transfers: &[Transfer], max_cycles: u64) -> Result<(), String> {
    report.frame.audit().map_err(|e| format!("{e:?}"))?;
    wiring_certificate(&report.frame)?;
    let expected = super::counted::run_with_vertices(&report.transactions, &report.vertex_reads)?;
    if expected.frame.counts != report.frame.counts
        || expected.reads != report.reads
        || expected.vertices != report.vertices
        || expected
            .frame
            .values
            .iter()
            .map(|v| (v.format, v.raw))
            .collect::<Vec<_>>()
            != report
                .frame
                .values
                .iter()
                .map(|v| (v.format, v.raw))
                .collect::<Vec<_>>()
    {
        return Err("scratchpad numerical payload/transaction certificate".into());
    }
    if transfers.len() != report.transactions.len() || max_cycles == 0 {
        return Err("scratchpad timing shape/bound".into());
    }
    let mut times = vec![Timing { issue: 0, ready: 0 }; report.frame.events.len()];
    let mut accesses = Vec::new();
    let mut bank_events = vec![false; report.frame.events.len()];
    let mut operation = 0;
    // All four bank events of one transaction share one issue edge.
    for e in &report.frame.events {
        if let Operation::Read { memory, .. } | Operation::Write { memory, .. } = e.operation {
            if report.frame.memories[memory].name.starts_with("SP") {
                let t = transfers
                    .get(operation / 4)
                    .ok_or("extra scratchpad bank operation")?;
                if t.transaction != operation / 4 || t.ready != t.issue + 1 || t.ready > max_cycles
                {
                    return Err("scratchpad registered bank timing".into());
                }
                times[e.id] = Timing {
                    issue: t.issue,
                    ready: t.ready,
                };
                bank_events[e.id] = true;
                let port = if matches!(
                    report.transactions[t.transaction],
                    Transaction::DmaWrite { .. }
                ) {
                    0
                } else {
                    1
                };
                accesses.push(MemoryAccess {
                    event: e.id,
                    copy: 0,
                    ports: vec![port],
                });
                operation += 1;
            }
        }
    }
    if operation != transfers.len() * 4 {
        return Err("scratchpad bank event shape".into());
    }
    let deps = bound_dependencies(&report.frame, &[]).map_err(|e| format!("{e:?}"))?;
    for event in 0..report.frame.events.len() {
        if !bank_events[event] {
            let ready = deps[event]
                .iter()
                .map(|&p| times[p].ready)
                .max()
                .unwrap_or(0);
            times[event] = Timing {
                issue: ready,
                ready,
            };
        }
    }
    let mut layout = MemoryLayout::default();
    for (memory, m) in report
        .frame
        .memories
        .iter()
        .enumerate()
        .filter(|(_, m)| m.name.starts_with("SP"))
    {
        let bank = layout.banks.len();
        let port = MemoryPort {
            read: true,
            write: true,
            read_latency: 1,
            write_latency: 1,
            initiation_interval: 1,
        };
        layout.banks.push(MemoryBank {
            name: m.name.clone(),
            kind: RamKind::Bsram,
            width: 18,
            depth: 1024,
            ports: vec![port, port],
            collision: ReadDuringWrite::Forbidden,
        });
        layout.placements.push(MemoryPlacement {
            memory,
            copies: vec![MemoryCopy {
                slices: vec![MemorySlice {
                    bank,
                    base_row: 0,
                    bit_offset: 0,
                    source_low: 0,
                    width: 16,
                }],
            }],
        });
    }
    layout
        .audit_accesses(&report.frame, &times, &accesses, max_cycles)
        .map_err(|e| format!("{e:?}"))?;
    layout
        .audit_gowin_budget(
            &report.frame,
            GowinMemoryBudget {
                bsram_blocks: 4,
                ssram_cells: 0,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}
