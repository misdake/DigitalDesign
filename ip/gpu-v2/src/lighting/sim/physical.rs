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
    /// Compare actual repeated value lifetimes before accepting ALAP phase
    /// compaction. A narrower local operator alone does not guarantee less
    /// retained storage. This excludes primitive-internal/control registers.
    pub fn compact_storage(self, plan: &Plan) -> Result<Self, String> {
        self.compact_storage_bounded(plan, 1)
    }

    /// At most 256 measured candidates: whole ALAP, then partial moves in reverse
    /// dependency order. Preserve lane/phase, input capture ages and commit;
    /// accept only independently checked, nonincreasing peak live-bit costs.
    pub fn compact_storage_bounded(mut self, plan: &Plan, attempts: usize) -> Result<Self, String> {
        if !(1..=256).contains(&attempts) {
            return Err("storage search budget must be 1..256".into());
        }
        let before = self.retained_values(plan)?;
        let order = dependency_order(plan)?;
        let mut candidate = self.clone().compact_lifetimes(plan)?;
        candidate.retime_wires(plan, &order, &self);
        let mut cost = before.peak_bits;
        if let Ok(after) = candidate.retained_values(plan) {
            if candidate.latency <= self.latency && after.peak_bits <= cost {
                cost = after.peak_bits;
                self = candidate.clone();
            }
        }
        let mut tried = 1;
        for &id in order.iter().rev() {
            if tried == attempts {
                break;
            }
            if self.slots[id].kind.is_none() || candidate.slots[id].issue <= self.slots[id].issue {
                continue;
            }
            tried += 1;
            let mut partial = self.clone();
            partial.slots[id] = candidate.slots[id].clone();
            // Absorbed members depend on their certified root. Recompute every
            // other wire from checked dependencies, while retaining capture ages
            // for precaptured pixel/context rows (no hidden input-buffer saving).
            partial.retime_wires(plan, &order, &self);
            // Some partial moves cross a still-early consumer; rejection is
            // expected. The validator checks all atomic cones and repeated lanes.
            if let Ok(live) = partial.retained_values(plan) {
                if live.peak_bits <= cost {
                    cost = live.peak_bits;
                    self = partial;
                }
            }
        }
        self.audit(plan)?;
        Ok(self)
    }

    fn retime_wires(&mut self, plan: &Plan, order: &[usize], captured: &Self) {
        for &wire in order {
            if self.slots[wire].kind.is_some() {
                continue;
            }
            let ready = if matches!(plan.template.events[wire].operation, Operation::Read { .. }) {
                captured.slots[wire].issue
            } else {
                plan.binding.dependencies[wire]
                    .iter()
                    .map(|&d| self.slots[d].ready)
                    .max()
                    .unwrap_or(0)
            };
            self.slots[wire].issue = ready;
            self.slots[wire].ready = ready;
        }
    }

    pub fn retained_values(&self, plan: &Plan) -> Result<LiveReport, String> {
        self.audit(plan)?;
        let times: Vec<_> = self
            .slots
            .iter()
            .map(|r| Timing {
                issue: r.issue,
                ready: r.ready,
            })
            .collect();
        retained_values(plan, &times, self.write_issue, self.initiation_interval)
    }

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
        let q13 = frame.memories.iter().any(|m| m.name == "RSQRT_Q13");
        let mut normalization_banks = Vec::new();
        if sq.is_some() || rsqrt.is_some() || q13 {
            for lane in 0..if q13 {
                h.normalize_reads.div_ceil(2)
            } else {
                h.normalize_reads
            } {
                normalization_banks.push(layout.banks.len());
                layout.banks.push(MemoryBank {
                    name: format!("normalize.{lane}"),
                    kind: RamKind::Bsram,
                    width: if q13 { 18 } else { 36 },
                    depth: if q13 { 1024 } else { 512 },
                    ports: vec![
                        MemoryPort {
                            read: true,
                            write: false,
                            read_latency: h.rom_latency,
                            write_latency: 1,
                            initiation_interval: 1,
                        };
                        if q13 { 2 } else { 1 }
                    ],
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
                "SQ" | "RSQRT" | "RSQRT_Q13" | "SQRT" => normalization_banks
                    .iter()
                    .map(|&bank| MemoryCopy {
                        slices: vec![MemorySlice {
                            bank,
                            base_row: if m.name == "SQRT" {
                                384
                            } else if m.name == "RSQRT" || m.name == "RSQRT_Q13" {
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
                "POWER" | "POWER_MIDPOINT_Q15" => {
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
                    || frame.memories[memory].name == "RSQRT_Q13"
                    || frame.memories[memory].name == "SQRT"
                {
                    let lane = r.lane.ok_or("ROM lane missing")?;
                    if q13 {
                        lane / 2
                    } else {
                        lane
                    }
                } else {
                    0
                };
                accesses.push(MemoryAccess {
                    event: r.event,
                    copy,
                    ports: vec![
                        if q13 && r.kind == Some(LaneKind::NormalizeRead) {
                            r.lane.unwrap() % 2
                        } else {
                            0
                        };
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
        let retained = retained_values(plan, &times, self.write_issue, self.initiation_interval)?;
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

fn dependency_order(plan: &Plan) -> Result<Vec<usize>, String> {
    let n = plan.template.events.len();
    let mut pending: Vec<_> = plan.binding.dependencies.iter().map(Vec::len).collect();
    let mut users = vec![Vec::new(); n];
    for (id, deps) in plan.binding.dependencies.iter().enumerate() {
        for &dep in deps {
            users[dep].push(id);
        }
    }
    let mut queue: std::collections::VecDeque<_> = (0..n).filter(|&id| pending[id] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(id) = queue.pop_front() {
        order.push(id);
        for &user in &users[id] {
            pending[user] -= 1;
            if pending[user] == 0 {
                queue.push_back(user);
            }
        }
    }
    if order.len() != n {
        return Err("storage search dependency cycle".into());
    }
    Ok(order)
}

fn retained_values(
    plan: &Plan,
    times: &[Timing],
    commit: u64,
    ii: u64,
) -> Result<LiveReport, String> {
    let frame = &plan.template;
    lifecycle::analyze_composed_policy(
        frame,
        times,
        &plan.binding.groups,
        &plan.binding.cones,
        &lifecycle::LifetimePolicy {
            commit_cycle: commit,
            period: Some(ii),
            max_cycle: plan.hardware.max_cycles,
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
    .map_err(|e| format!("lifetime: {e:?}"))
}
