//! A conservative concrete bank/replica and DSP placement for static lighting.
use super::*;
use audited::{
    lifecycle::{self, LiveReport, RegisterBudget},
    physical::*,
};

pub struct PhysicalReport {
    pub dsp: DspUsage,
    pub memory: MemoryUsage,
    pub memory_cells: GowinMemoryUsage,
    pub retained: LiveReport,
    pub layout: MemoryLayout,
    pub accesses: Vec<MemoryAccess>,
}
impl PeriodicSchedule {
    /// Check declared target placement, ports, and retained values for one body.
    /// Uniform inputs are already latched; this does not execute arithmetic/RTL.
    pub fn audit_physical(
        &self,
        plan: &Plan,
        memory_budget: GowinMemoryBudget,
        registers: &RegisterBudget,
    ) -> Result<PhysicalReport, String> {
        self.audit(plan)?;
        let frame = &plan.template;
        let h = plan.hardware;
        let times: Vec<_> = self
            .slots
            .iter()
            .map(|r| Timing {
                issue: r.issue,
                ready: r.ready,
            })
            .collect();
        let inventory = h.dsp_inventory()?;
        let mut issues = Vec::new();
        for r in &self.slots {
            let mode = match r.kind {
                Some(LaneKind::SmallMultiply) => DspMode::Multiply9,
                Some(LaneKind::LargeMultiply) => DspMode::Multiply18,
                Some(LaneKind::PairMultiplyAdd) => DspMode::PairMultiplyAdd,
                _ => continue,
            };
            let instance = inventory
                .instances
                .iter()
                .enumerate()
                .filter(|(_, d)| d.mode == mode)
                .nth(r.lane.ok_or("DSP lane missing")?)
                .map(|(id, _)| id)
                .ok_or("DSP lane not placed")?;
            let work = if mode == DspMode::PairMultiplyAdd {
                let g = plan
                    .binding
                    .groups
                    .iter()
                    .find(|g| g.result_event == r.event)
                    .ok_or("pair certificate missing")?;
                DspWork::PairMultiplyAdd {
                    a_bits: [
                        frame.values[g.operands[0]].format.bits,
                        frame.values[g.operands[2]].format.bits,
                    ],
                    b_bits: [
                        frame.values[g.operands[1]].format.bits,
                        frame.values[g.operands[3]].format.bits,
                    ],
                    accumulator_bits: frame.values[g.operands[4]].format.bits,
                }
            } else {
                let e = &frame.events[r.event];
                DspWork::Multiply {
                    a_bits: frame.values[e.inputs[0]].format.bits,
                    b_bits: frame.values[e.inputs[1]].format.bits,
                }
            };
            issues.push(DspIssue {
                instance,
                issue: r.issue,
                ready: r.ready,
                work,
            });
        }
        inventory
            .audit_issues(&issues, Some(self.initiation_interval), h.max_cycles)
            .map_err(|e| format!("DSP calendar: {e:?}"))?;
        let mut layout = MemoryLayout::default();
        let sq = frame.memories.iter().position(|m| m.name == "SQ");
        let rsqrt = frame.memories.iter().position(|m| m.name == "RSQRT");
        let mut normalization_banks = Vec::new();
        if sq.is_some() || rsqrt.is_some() {
            for lane in 0..h.normalize_reads {
                normalization_banks.push(layout.banks.len());
                layout.banks.push(MemoryBank {
                    name: format!("normalize.{lane}"),
                    kind: RamKind::Bsram,
                    width: 36,
                    depth: 512,
                    ports: vec![MemoryPort {
                        read: true,
                        write: false,
                        read_latency: h.rom_latency,
                        write_latency: 1,
                        initiation_interval: 1,
                    }],
                    collision: ReadDuringWrite::Forbidden,
                });
            }
        }
        for (memory, m) in frame
            .memories
            .iter()
            .enumerate()
            .filter(|(_, m)| m.kind != MemoryKind::Input)
        {
            let copies = match m.name.as_str() {
                "SQ" | "RSQRT" => normalization_banks
                    .iter()
                    .map(|&bank| MemoryCopy {
                        slices: vec![MemorySlice {
                            bank,
                            base_row: if m.name == "RSQRT" {
                                sq.map_or(0, |id| frame.memories[id].rows)
                            } else {
                                0
                            },
                            bit_offset: 0,
                            source_low: 0,
                            width: m.format.bits,
                        }],
                    })
                    .collect(),
                "POWER" => {
                    let mut slices = Vec::new();
                    for (part, (low, width)) in [(0, 16), (16, 12)].into_iter().enumerate() {
                        let bank = layout.banks.len();
                        layout.banks.push(MemoryBank {
                            name: format!("power.{part}"),
                            kind: RamKind::Bsram,
                            width: 18,
                            depth: 1024,
                            ports: vec![MemoryPort {
                                read: true,
                                write: false,
                                read_latency: h.rom_latency,
                                write_latency: 1,
                                initiation_interval: 1,
                            }],
                            collision: ReadDuringWrite::Forbidden,
                        });
                        slices.push(MemorySlice {
                            bank,
                            base_row: 0,
                            bit_offset: 0,
                            source_low: low,
                            width,
                        });
                    }
                    vec![MemoryCopy { slices }]
                }
                "POWER_CONTEXT" => {
                    let bank = layout.banks.len();
                    layout.banks.push(MemoryBank {
                        name: "power-context".into(),
                        kind: RamKind::Ssram,
                        width: 43,
                        depth: 32,
                        ports: vec![MemoryPort {
                            read: true,
                            write: false,
                            read_latency: h.rom_latency,
                            write_latency: 1,
                            initiation_interval: 1,
                        }],
                        collision: ReadDuringWrite::Forbidden,
                    });
                    vec![MemoryCopy {
                        slices: vec![MemorySlice {
                            bank,
                            base_row: 0,
                            bit_offset: 0,
                            source_low: 0,
                            width: 43,
                        }],
                    }]
                }
                _ => return Err("unknown lighting ROM placement".into()),
            };
            layout.placements.push(MemoryPlacement { memory, copies });
        }
        let mut accesses = Vec::new();
        for r in &self.slots {
            if let Operation::Read { memory, .. } = frame.events[r.event].operation {
                if frame.memories[memory].kind == MemoryKind::Input {
                    continue;
                }
                let placement = layout
                    .placements
                    .iter()
                    .find(|p| p.memory == memory)
                    .ok_or("store not placed")?;
                let copy = if frame.memories[memory].name == "SQ"
                    || frame.memories[memory].name == "RSQRT"
                {
                    r.lane.ok_or("ROM lane missing")?
                } else {
                    0
                };
                accesses.push(MemoryAccess {
                    event: r.event,
                    copy,
                    ports: vec![
                        0;
                        placement
                            .copies
                            .get(copy)
                            .ok_or("ROM lane lacks replica")?
                            .slices
                            .len()
                    ],
                });
            }
        }
        layout
            .audit_periodic_composed_accesses(
                frame,
                &times,
                &plan.binding.groups,
                &plan.binding.cones,
                &accesses,
                self.initiation_interval,
                h.max_cycles,
            )
            .map_err(|e| format!("memory calendar: {e:?}"))?;
        let memory = layout
            .audit(frame)
            .map_err(|e| format!("memory layout: {e:?}"))?;
        let memory_cells = layout
            .audit_gowin_budget(frame, memory_budget)
            .map_err(|e| format!("memory budget: {e:?}"))?;
        let retained = lifecycle::analyze_composed_policy(
            frame,
            &times,
            &plan.binding.groups,
            &plan.binding.cones,
            &lifecycle::LifetimePolicy {
                commit_cycle: self.write_issue,
                period: Some(self.initiation_interval),
                max_cycle: h.max_cycles,
                invariant_inputs: frame
                    .memories
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.kind == MemoryKind::Input && m.name.starts_with("context."))
                    .map(|(id, _)| id)
                    .collect(),
                retained_outputs: Some(vec!["g".into(), "h".into()]),
            },
        )
        .map_err(|e| format!("lifetime: {e:?}"))?;
        retained
            .audit_budget(registers)
            .map_err(|e| format!("register budget: {e:?}"))?;
        Ok(PhysicalReport {
            dsp: inventory.audit().map_err(|e| format!("DSP: {e:?}"))?,
            memory,
            memory_cells,
            retained,
            layout,
            accesses,
        })
    }
}
