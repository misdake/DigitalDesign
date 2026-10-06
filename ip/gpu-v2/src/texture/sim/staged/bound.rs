//! Input-independent preparation calendars and conservative storage bindings.
//! Primitive registered logic is a declared target, not a fitted fmax claim.
use super::*;
use audited::{
    lifecycle::{self, LifetimePolicy, LiveReport},
    physical::{self, LogicCone, Timing},
    MemoryKind, Operation, Resource,
};
use resource_scheduler::{
    Graph, ModuloGraph, ModuloSchedule, Node, Resource as Site, SearchConfig,
};
use std::{collections::BTreeMap, sync::Arc};
#[expect(
    dead_code,
    reason = "Legacy private ingress/live ports are retained; Runtime uses its live executor"
)]
pub mod control;
pub mod inventory;
pub mod runtime;
mod runtime_inventory;
mod runtime_membership;
#[cfg(test)]
mod runtime_numerical_tests;
mod runtime_packet;
mod runtime_preparation;
pub mod runtime_rtl;
pub mod serial;
pub mod serial_rtl;
pub mod session;
pub mod storage;
pub mod system;
mod transport;

// Test-only exclusion of fresh counted evaluation on the live numerical path.
// Legacy Program construction occurs outside this scope and remains supported.
#[cfg(test)]
pub(super) mod counted_call_guard {
    use std::cell::Cell;
    thread_local! {
        static LIVE: Cell<bool> = const { Cell::new(false) };
        static CALLS: Cell<[u64; 5]> = const { Cell::new([0; 5]) };
    }
    pub(super) struct Scope;
    impl Scope {
        pub(super) fn enter() -> Self {
            LIVE.with(|v| assert!(!v.replace(true), "nested live numerical scope"));
            Self
        }
    }
    impl Drop for Scope {
        fn drop(&mut self) {
            LIVE.with(|v| v.set(false));
        }
    }
    pub(in crate::texture::sim::staged) fn call(which: usize) {
        CALLS.with(|v| {
            let mut calls = v.get();
            calls[which] += 1;
            v.set(calls);
        });
        LIVE.with(|v| assert!(!v.get(), "counted arithmetic on live Runtime path"));
    }
    pub(super) fn calls() -> [u64; 5] {
        CALLS.with(Cell::get)
    }
}

pub struct Lane {
    pub lane: u8,
    pub coordinate: Stage,
    pub coefficient: Stage,
    pub planes: Vec<Plane>,
    pub memberships: Vec<Stage>,
    pub packets: Vec<Vec<Stage>>,
}
pub struct Preparation {
    pub derivative: Stage,
    pub lod: Stage,
    pub lanes: Vec<Lane>,
    pub payloads: Vec<i128>,
}
fn coefficient(c: &Stage, q: &Stage) -> Result<Stage, Fault> {
    let mut m = Model::numerical();
    let parents: CoefficientStore = m.input("parents", &[c.raw("parent0"), c.raw("parent1")])?;
    let fractions: FractionStore = m.input(
        "fractions",
        &(0..2)
            .flat_map(|w| (0..2).map(move |a| q.raw(&format!("f{w}.{a}"))))
            .collect::<Vec<_>>(),
    )?;
    let nearest = m.input::<1, 0, false>("nearest", &[c.raw("nearest")])?;
    let f = m.compute("texture_fixed_coefficients", 256)?;
    let nearest = f.read(nearest.at::<0>())?;
    for w in 0..2 {
        let parent = f.read(pair_address(parents, w))?;
        let uv: [Fraction; 2] = [
            f.select(
                nearest,
                Fraction::constant::<0>(),
                f.read(pair_address(fractions, w * 2))?,
            )?,
            f.select(
                nearest,
                Fraction::constant::<0>(),
                f.read(pair_address(fractions, w * 2 + 1))?,
            )?,
        ];
        let row_product: WeightProduct = f.product(parent, uv[1])?;
        let high: Coefficient = f.slice::<9, 0, false, 8>(row_product)?;
        let rows = [f.sub_same(parent, high)?, high];
        for (r, p) in rows.into_iter().enumerate() {
            let product: WeightProduct = f.product(p, uv[0])?;
            let high: Coefficient = f.slice::<9, 0, false, 8>(product)?;
            f.publish(&format!("w{w}.{}", r * 2), f.sub_same(p, high)?)?;
            f.publish(&format!("w{w}.{}", r * 2 + 1), high)?;
        }
    }
    Stage::finish(f)
}
pub fn prepare(q: &QuadInput, slots: &[Slot]) -> Result<Preparation, counted::Error> {
    let slot = oracle::check_input(q, slots)?;
    let derivative = super::derivatives(q, slot)?;
    let lod = super::lod(&derivative)?;
    let mut lanes = vec![];
    let mut payloads = vec![];
    for lane in 0..4 {
        if q.mask >> lane & 1 == 0 {
            continue;
        }
        let coordinate = super::coordinates(&derivative, &lod, lane)?;
        let coefficient = coefficient(&lod, &coordinate)?;
        let mut planes = vec![];
        let mut memberships = vec![];
        let mut packets = vec![];
        for which in 0..2 {
            if lod.raw(&format!("parent{which}")) == 0 {
                continue;
            }
            let p = super::plane(&lod, &coordinate, &coefficient, lane, which)?;
            let member = membership(&lod, &coordinate, &coefficient, lane, which)?;
            let mut words = vec![];
            for t in 0..4 {
                if member.raw(&format!("emit{t}")) == 0 {
                    continue;
                }
                let word = packet(&member, t)?;
                payloads.push(word.raw("packet"));
                words.push(word);
            }
            memberships.push(member);
            packets.push(words);
            planes.push(p);
        }
        lanes.push(Lane {
            lane: lane as u8,
            coordinate,
            coefficient,
            planes,
            memberships,
            packets,
        });
    }
    Ok(Preparation {
        derivative,
        lod,
        lanes,
        payloads,
    })
}
fn membership(c: &Stage, q: &Stage, w: &Stage, lane: usize, which: usize) -> Result<Stage, Fault> {
    membership_values(
        std::array::from_fn(|t| w.raw(&format!("w{which}.{t}"))),
        std::array::from_fn(|i| q.raw(&format!("t{which}.{}.{}", i / 2, i % 2))),
        [c.raw("slot"), c.raw(&format!("n{which}")), c.raw("quad")],
        lane as i128,
        [i128::from(which == 0), c.raw("last_fine")],
    )
}
// One counted body for legacy prepare and captured actual coefficient operands.
fn membership_values(
    weights: [i128; 4],
    coords: [i128; 4],
    identity: [i128; 3],
    lane: i128,
    flags: [i128; 2],
) -> Result<Stage, Fault> {
    #[cfg(test)]
    counted_call_guard::call(0);
    let mut m = Model::numerical();
    let weights: CoefficientStore = m.input("weights", &weights)?;
    let coords: TexelCoordinateStore = m.input("coords", &coords)?;
    let identity = m.input::<4, 0, false>("identity", &identity)?;
    let lane_id = m.input::<2, 0, false>("lane", &[lane])?;
    let flags = m.input::<1, 0, false>("flags", &flags)?;
    let f = m.compute("texture_membership", 256)?;
    let a = f.read(coords.at::<0>())?;
    let b = f.read(coords.at::<1>())?;
    let c0 = f.read(coords.at::<2>())?;
    let d = f.read(coords.at::<3>())?;
    let tx = [f.slice::<7, 0, false, 3>(a)?, f.slice::<7, 0, false, 3>(b)?];
    let ty = [
        f.slice::<7, 0, false, 3>(c0)?,
        f.slice::<7, 0, false, 3>(d)?,
    ];
    let same_x = counted::eq(&f, tx[0], tx[1])?;
    let same_y = counted::eq(&f, ty[0], ty[1])?;
    let same_xy = f.select(same_x, same_y, Bit::constant::<0>())?;
    let same = [Bit::constant::<1>(), same_x, same_y, same_xy];
    let mut nonzero = [Bit::constant::<0>(); 4];
    for (t, v) in nonzero.iter_mut().enumerate() {
        let w = f.read(pair_address(weights, t))?;
        f.publish(&format!("w{t}"), w)?;
        *v = counted::not(&f, counted::eq(&f, w, Coefficient::constant::<0>())?)?;
    }
    for t in 0..4 {
        let mut emit = nonzero[t];
        for j in 0..t {
            emit = f.select(
                f.select(same[t ^ j], nonzero[j], Bit::constant::<0>())?,
                Bit::constant::<0>(),
                emit,
            )?;
        }
        f.publish(&format!("emit{t}"), emit)?;
    }
    for i in 0..2 {
        f.publish(&format!("tx{i}"), tx[i])?;
        f.publish(&format!("ty{i}"), ty[i])?;
    }
    f.publish("lx", f.slice::<3, 0, false, 0>(a)?)?;
    f.publish("ly", f.slice::<3, 0, false, 0>(c0)?)?;
    f.publish("same_x", same_x)?;
    f.publish("same_y", same_y)?;
    f.publish("slot", f.read(identity.at::<0>())?)?;
    f.publish("n", f.read(identity.at::<1>())?)?;
    f.publish("quad", f.read(identity.at::<2>())?)?;
    f.publish("lane", f.read(lane_id.at::<0>())?)?;
    let fine = f.read(flags.at::<0>())?;
    f.publish("fine", fine)?;
    f.publish(
        "final",
        f.select(fine, f.read(flags.at::<1>())?, Bit::constant::<1>())?,
    )?;
    Stage::finish(f)
}
fn packet(p: &Stage, tap: usize) -> Result<Stage, Fault> {
    packet_values(|name| p.raw(name), tap)
}
// Complete92-bit membership plus2-bit tap input capture, same counted body.
fn packet_values(p: impl Fn(&str) -> i128, tap: usize) -> Result<Stage, Fault> {
    #[cfg(test)]
    counted_call_guard::call(1);
    let mut m = Model::numerical();
    let weights: CoefficientStore = m.input(
        "weights",
        &(0..4).map(|i| p(&format!("w{i}"))).collect::<Vec<_>>(),
    )?;
    let tiles = m.input::<7, 0, false>("tiles", &[p("tx0"), p("tx1"), p("ty0"), p("ty1")])?;
    let local = m.input::<3, 0, false>("local", &[p("lx"), p("ly")])?;
    let meta = m.input::<4, 0, false>("meta", &[p("slot"), p("n"), p("quad")])?;
    let lane = m.input::<2, 0, false>("lane", &[p("lane"), tap as i128])?;
    let flags =
        m.input::<1, 0, false>("flags", &[p("same_x"), p("same_y"), p("fine"), p("final")])?;
    let emit = m.input::<1, 0, false>(
        "emit",
        &(0..4).map(|i| p(&format!("emit{i}"))).collect::<Vec<_>>(),
    )?;
    let f = m.compute("texture_packet", 384)?;
    let tap = f.read(lane.at::<1>())?;
    let x = f.slice::<1, 0, false, 0>(tap)?;
    let y = f.slice::<1, 0, false, 1>(tap)?;
    let e: [Bit; 4] = [
        f.read(emit.at::<0>())?,
        f.read(emit.at::<1>())?,
        f.read(emit.at::<2>())?,
        f.read(emit.at::<3>())?,
    ];
    let valid = f.select(y, f.select(x, e[3], e[2])?, f.select(x, e[1], e[0])?)?;
    f.require::<true>(valid)?;
    let sx = f.read(flags.at::<0>())?;
    let sy = f.read(flags.at::<1>())?;
    let fine = f.read(flags.at::<2>())?;
    let mut last = f.read(flags.at::<3>())?;
    for (i, active) in e.iter().enumerate().skip(1) {
        let greater = match i {
            1 => f.less(tap, Fixed::<2, 0, false>::constant::<1>())?,
            2 => f.less(tap, Fixed::<2, 0, false>::constant::<2>())?,
            _ => f.less(tap, Fixed::<2, 0, false>::constant::<3>())?,
        };
        last = f.select(
            f.select(greater, *active, Bit::constant::<0>())?,
            Bit::constant::<0>(),
            last,
        )?;
    }
    let first = f.select(
        x,
        Bit::constant::<0>(),
        f.select(y, Bit::constant::<0>(), fine)?,
    )?;
    let mut word = GroupWord::constant::<0>();
    word = counted::pack_field::<0>(&f, word, f.read(meta.at::<0>())?)?;
    word = counted::pack_field::<4>(&f, word, f.read(meta.at::<1>())?)?;
    word = counted::pack_field::<8>(
        &f,
        word,
        f.select(x, f.read(tiles.at::<1>())?, f.read(tiles.at::<0>())?)?,
    )?;
    word = counted::pack_field::<15>(
        &f,
        word,
        f.select(y, f.read(tiles.at::<3>())?, f.read(tiles.at::<2>())?)?,
    )?;
    word = counted::pack_field::<22>(&f, word, f.read(local.at::<0>())?)?;
    word = counted::pack_field::<25>(&f, word, f.read(local.at::<1>())?)?;
    for i in 0..4 {
        let mx = if i & 1 == 0 {
            f.select(x, sx, Bit::constant::<1>())?
        } else {
            f.select(x, Bit::constant::<1>(), sx)?
        };
        let my = if i & 2 == 0 {
            f.select(y, sy, Bit::constant::<1>())?
        } else {
            f.select(y, Bit::constant::<1>(), sy)?
        };
        let w = f.select(
            f.select(mx, my, Bit::constant::<0>())?,
            f.read(pair_address(weights, i))?,
            Coefficient::constant::<0>(),
        )?;
        word = match i {
            0 => counted::pack_field::<28>(&f, word, w)?,
            1 => counted::pack_field::<37>(&f, word, w)?,
            2 => counted::pack_field::<46>(&f, word, w)?,
            _ => counted::pack_field::<55>(&f, word, w)?,
        };
    }
    word = counted::pack_field::<64>(&f, word, first)?;
    word = counted::pack_field::<65>(&f, word, last)?;
    word = counted::pack_field::<66>(&f, word, f.read(meta.at::<2>())?)?;
    word = counted::pack_field::<70>(&f, word, f.read(lane.at::<0>())?)?;
    f.publish("packet", word)?;
    Stage::finish(f)
}
fn shape(f: &FrameReport) -> Vec<String> {
    let mut result: Vec<_> = f
        .memories
        .iter()
        .map(|m| {
            format!(
                "{}:{:?}:{}:{:?}:{:?}:{}",
                m.name, m.format, m.rows, m.kind, m.ports, m.read_only
            )
        })
        .collect();
    result.extend(f.events.iter().map(|e| {
        let op = match e.operation {
            Operation::Read { memory, row: _ }
                if f.memories[memory].kind == MemoryKind::Rom && !e.inputs.is_empty() =>
            {
                format!("indexed_read:{memory}")
            }
            _ => format!("{:?}", e.operation),
        };
        format!(
            "{op}:{:?}:{:?}:{:?}:{:?}",
            e.inputs,
            e.control,
            e.output.map(|v| f.values[v].format),
            if e.operation == Operation::Literal {
                e.output.map(|v| f.values[v].raw)
            } else {
                None
            }
        )
    }));
    result
}
pub struct StagePlan {
    pub graph: Graph,
    pub calendar: ModuloSchedule,
    pub times: Vec<Timing>,
    pub life: LiveReport,
    pub fixed_ff_bits: u64,
    pub dsp_pipeline_bits: u64,
    pub rom_ram16_cells: usize,
    /// Declared per-site operand selection demand; not a fitted LUT count.
    pub operand_mux_tree_bits: u64,
    pub sites: Vec<(Resource, usize)>,
    signature: Vec<String>,
    lowering: physical::lowering::Plan,
    cones: Vec<LogicCone>,
    certified_graph: Graph,
    retained: Vec<String>,
    certified_sites: Vec<(Resource, usize)>,
    pub ff_banks: Vec<FfBank>,
    pub packed: storage::Layout,
    pub packed_fields: Vec<FfBank>,
}
fn storage_ports(f: &FrameReport, ports: &[String]) -> Vec<String> {
    ports
        .iter()
        .filter(|name| {
            !(matches!(
                f.name.as_str(),
                "texture_derivatives" | "texture_lod_context"
            ) && *name == "base")
                && !(f.name == "texture_lod_context" && *name == "shift1")
        })
        .cloned()
        .collect()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FfBank {
    pub value: usize,
    pub source_low: u32,
    pub width: u32,
    pub slots: u64,
    pub birth: u64,
    pub last_read: u64,
    pub read_times: Vec<u64>,
}
fn operand_mux_bits(
    f: &FrameReport,
    graph: &Graph,
    calendar: &ModuloSchedule,
    cones: &[LogicCone],
) -> u64 {
    use std::collections::BTreeSet;
    let mut inputs = BTreeMap::<(usize, usize, usize), BTreeSet<usize>>::new();
    for e in &f.events {
        let Some(r) = graph.nodes[e.id].resource else {
            continue;
        };
        let values = cones
            .iter()
            .find(|p| p.result_event == e.id)
            .map_or(e.inputs.as_slice(), |p| p.operands.as_slice());
        for (operand, &value) in values.iter().enumerate() {
            inputs
                .entry((r, calendar.nodes[e.id].lane.unwrap(), operand))
                .or_default()
                .insert(value);
        }
    }
    inputs
        .values()
        .map(|vs| {
            let width = vs
                .iter()
                .map(|&v| f.values[v].format.bits)
                .max()
                .unwrap_or(0);
            u64::from(width) * vs.len().saturating_sub(1) as u64
        })
        .sum()
}
fn storage_banks(
    f: &FrameReport,
    life: &LiveReport,
    lowering: &physical::lowering::Plan,
    ii: u64,
) -> Result<Vec<FfBank>, String> {
    use std::collections::{BTreeMap, BTreeSet};
    let wires: BTreeSet<_> = lowering
        .wiring_adds
        .iter()
        .map(|p| p.result_event)
        .collect();
    let eq: BTreeMap<_, _> = lowering
        .equalities
        .iter()
        .flat_map(|p| {
            p.absorbed_events
                .iter()
                .map(move |&e| (e, f.events[p.result_event].output.unwrap()))
        })
        .collect();
    let mut sources = vec![BTreeSet::<usize>::new(); f.values.len()];
    let mut inputs = BTreeMap::new();
    for e in &f.events {
        let Some(v) = e.output else { continue };
        if e.operation == Operation::Literal {
            continue;
        }
        if let Some(&root) = eq.get(&e.id) {
            sources[v].insert(root);
            continue;
        }
        if wires.contains(&e.id) {
            for &input in &e.inputs {
                let s = sources[input].clone();
                sources[v].extend(s);
            }
        } else if e.inputs.len() == 1
            && matches!(
                e.operation,
                Operation::Resize
                    | Operation::BinaryScale
                    | Operation::Slice(_)
                    | Operation::ShiftLeft(_)
                    | Operation::RescaleFloor(_)
            )
        {
            sources[v] = sources[e.inputs[0]].clone();
        } else if let Operation::Read { memory, row } = e.operation {
            if f.memories[memory].kind == MemoryKind::Input {
                sources[v].insert(*inputs.entry((memory, row)).or_insert(v));
            } else {
                sources[v].insert(v);
            }
        } else {
            sources[v].insert(v);
        }
    }
    let mut banks: BTreeMap<_, _> = life
        .intervals
        .iter()
        .filter(|i| !wires.contains(&f.values[i.value].producer))
        .map(|i| {
            (
                i.value,
                FfBank {
                    value: i.value,
                    source_low: 0,
                    width: i.bits,
                    slots: 0,
                    birth: i.start,
                    last_read: i.end - 1,
                    read_times: vec![],
                },
            )
        })
        .collect();
    // A disjoint aggregate is a view of fields, not a registered 72-bit adder
    // result. Carry every field to the aggregate's last use; no lifetime drops.
    for i in &life.intervals {
        if wires.contains(&f.values[i.value].producer) {
            for &source in &sources[i.value] {
                let bank = banks
                    .get_mut(&source)
                    .ok_or("wiring field origin missing")?;
                bank.last_read = bank.last_read.max(i.end - 1);
            }
        }
    }
    for b in banks.values_mut() {
        b.slots = (b.last_read + 1 - b.birth).div_ceil(ii);
    }
    Ok(banks.into_values().collect())
}
impl StagePlan {
    pub fn span(&self) -> u64 {
        self.calendar.span
    }
    pub fn ii(&self) -> u64 {
        self.calendar.initiation_interval
    }
    pub fn build(f: &FrameReport, ii: u64, retained: Vec<String>) -> Result<Self, String> {
        let evidence = super::binding::Evidence::build(f).map_err(|e| format!("binding: {e:?}"))?;
        let lowering = evidence.lowering;
        let mut cones = lowering
            .logic_cones(f, 1)
            .map_err(|e| format!("cones: {e:?}"))?;
        // Explicit charged singleton splits keep shared h/shift legal and billed.
        if f.name == "texture_lod_context" {
            let shared =
                super::binding::lod_shared_h_cone(f).map_err(|e| format!("shared h: {e:?}"))?;
            for id in [shared.absorbed_events[0], shared.result_event] {
                cones.push(LogicCone::singleton(f, id, 1).map_err(|e| format!("split: {e:?}"))?);
            }
        }
        let mut rs = lowering
            .resources(f)
            .map_err(|e| format!("resources: {e:?}"))?;
        for (e, r) in f.events.iter().zip(&mut rs) {
            if let Operation::Read { memory, .. } = e.operation {
                if f.memories[memory].kind == MemoryKind::Input {
                    if !e.inputs.is_empty() {
                        return Err("bound FF inputs require static fields".into());
                    }
                    // Captured FF fields are continuously driven, with fanout;
                    // they are not multiple RAM read transactions.
                    *r = None;
                }
            }
        }
        let mut counts = BTreeMap::<Resource, usize>::new();
        for r in rs.iter().flatten() {
            *counts.entry(*r).or_default() += 1;
        }
        let mut graph = Graph {
            nodes: vec![],
            resources: vec![],
        };
        let mut map = BTreeMap::new();
        let mut sites = vec![];
        let mut rom_ram16_cells = 0;
        for (&r, &count) in &counts {
            let lanes = count.div_ceil(ii as usize);
            if r == Resource::Dsp18 && lanes > 3 {
                return Err("coefficient DSP lanes exceed three".into());
            }
            if matches!(r, Resource::Read(_)) && lanes > 1 {
                return Err("ROM requires extra read replicas".into());
            }
            let latency = if matches!(r, Resource::Dsp18) { 3 } else { 1 };
            if let Resource::Read(m) = r {
                let mem = &f.memories[m];
                if mem.kind != MemoryKind::Rom {
                    return Err("unsupported bound memory".into());
                }
                rom_ram16_cells += mem.rows.div_ceil(16) * mem.format.bits as usize;
            }
            map.insert(r, graph.resources.len());
            graph.resources.push(Site {
                name: format!("{r:?}"),
                lanes,
                latency,
                initiation_interval: 1,
            });
            sites.push((r, lanes));
        }
        let deps =
            physical::logic_dependencies(f, &cones).map_err(|e| format!("dependencies: {e:?}"))?;
        graph.nodes = f
            .events
            .iter()
            .zip(rs)
            .zip(deps)
            .map(|((e, r), predecessors)| Node {
                name: format!("{:?}", e.operation),
                predecessors,
                earliest: 0,
                resource: r.map(|r| map[&r]),
            })
            .collect();
        let modulo = ModuloGraph::from_graph(&graph).map_err(|e| format!("modulo graph: {e:?}"))?;
        let calendar = resource_scheduler::modulo_schedule(&modulo, ii, &SearchConfig::default())
            .map_err(|e| format!("modulo: {e:?}"))?;
        let times: Vec<_> = calendar
            .nodes
            .iter()
            .enumerate()
            .map(|(id, n)| Timing {
                issue: n.issue,
                ready: n.issue
                    + graph.nodes[id]
                        .resource
                        .map_or(0, |r| graph.resources[r].latency),
            })
            .collect();
        physical::audit_logic_dependencies(f, &times, &cones, 100000)
            .map_err(|e| format!("gates: {e:?}"))?;
        lowering
            .audit_timing(f, &times, 1, 100000)
            .map_err(|e| format!("wiring gates: {e:?}"))?;
        let policy = LifetimePolicy {
            commit_cycle: calendar.span,
            period: Some(ii),
            max_cycle: 100000,
            invariant_inputs: vec![],
            retained_outputs: Some(retained.clone()),
        };
        let life = lifecycle::analyze_composed_policy(f, &times, &[], &cones, &policy)
            .map_err(|e| format!("lifecycle: {e:?}"))?;
        // Dedicated rotating FF slots per retained origin: no optimistic global
        // peak reuse. Ports use the fixed body offset/iteration, never raw values.
        let ff_banks = storage_banks(f, &life, &lowering, ii)?;
        let fixed_ff_bits = ff_banks.iter().map(|b| u64::from(b.width) * b.slots).sum();
        let dsp_pipeline_bits = sites
            .iter()
            .filter(|(r, _)| *r == Resource::Dsp18)
            .map(|(_, n)| *n as u64 * 17 * 3)
            .sum();
        let packed_retained = storage_ports(f, &retained);
        let packed_fields = storage::fields(
            f,
            &graph,
            &times,
            &cones,
            &lowering,
            &packed_retained,
            calendar.span,
        )?;
        let result = Self {
            packed: storage::Layout::build(&packed_fields, ii)?,
            packed_fields,
            operand_mux_tree_bits: operand_mux_bits(f, &graph, &calendar, &cones),
            certified_sites: sites.clone(),
            ff_banks,
            certified_graph: graph.clone(),
            retained,
            graph,
            calendar,
            times,
            life,
            fixed_ff_bits,
            dsp_pipeline_bits,
            rom_ram16_cells,
            sites,
            signature: shape(f),
            lowering,
            cones,
        };
        result.audit(f)?;
        Ok(result)
    }
    pub fn audit(&self, f: &FrameReport) -> Result<(), String> {
        f.audit().map_err(|e| format!("bound numerical: {e:?}"))?;
        if shape(f) != self.signature {
            return Err("universal stage shape mismatch".into());
        }
        if self.graph != self.certified_graph {
            return Err("bound resource graph mutation".into());
        }
        self.lowering
            .resources(f)
            .map_err(|e| format!("lowering reuse: {e:?}"))?;
        physical::audit_logic_dependencies(f, &self.times, &self.cones, 100000)
            .map_err(|e| format!("stage timing: {e:?}"))?;
        let graph = ModuloGraph::from_graph(&self.graph).map_err(|e| format!("graph: {e:?}"))?;
        if !resource_scheduler::check_modulo(&graph, &self.calendar).is_ok() {
            return Err("periodic site conflict".into());
        }
        if self.times.iter().enumerate().any(|(id, t)| {
            *t != (Timing {
                issue: self.calendar.nodes[id].issue,
                ready: self.calendar.nodes[id].issue
                    + self.graph.nodes[id]
                        .resource
                        .map_or(0, |r| self.graph.resources[r].latency),
            })
        }) {
            return Err("bound times mismatch".into());
        }
        let policy = LifetimePolicy {
            commit_cycle: self.span(),
            period: Some(self.ii()),
            max_cycle: 100000,
            invariant_inputs: vec![],
            retained_outputs: Some(self.retained.clone()),
        };
        let life = lifecycle::analyze_composed_policy(f, &self.times, &[], &self.cones, &policy)
            .map_err(|e| format!("lifecycle reuse: {e:?}"))?;
        let banks = storage_banks(f, &life, &self.lowering, self.ii())?;
        let packed_retained = storage_ports(f, &self.retained);
        let packed_fields = storage::fields(
            f,
            &self.graph,
            &self.times,
            &self.cones,
            &self.lowering,
            &packed_retained,
            self.span(),
        )?;
        self.packed.audit(&packed_fields, self.ii())?;
        if self.packed_fields != packed_fields {
            return Err("packed field liveness mutation".into());
        }
        let ff: u64 = banks.iter().map(|b| u64::from(b.width) * b.slots).sum();
        if life != self.life || ff != self.fixed_ff_bits {
            return Err("bound retained storage mutation".into());
        }
        if self.sites != self.certified_sites || self.ff_banks != banks {
            return Err("physical bank/site mutation".into());
        }
        let ram: usize = self
            .sites
            .iter()
            .filter_map(|(r, _)| match r {
                Resource::Read(m) => {
                    Some(f.memories[*m].rows.div_ceil(16) * f.memories[*m].format.bits as usize)
                }
                _ => None,
            })
            .sum();
        let dsp: u64 = self
            .sites
            .iter()
            .filter(|(r, _)| *r == Resource::Dsp18)
            .map(|(_, n)| *n as u64 * 17 * 3)
            .sum();
        if ram != self.rom_ram16_cells || dsp != self.dsp_pipeline_bits {
            return Err("bound RAM/DSP storage mutation".into());
        }
        if self.operand_mux_tree_bits
            != operand_mux_bits(f, &self.graph, &self.calendar, &self.cones)
        {
            return Err("bound operand selection mutation".into());
        }
        for bank in &self.ff_banks {
            if bank.slots * self.ii() <= bank.last_read - bank.birth {
                return Err("rotating FF overwrite".into());
            }
        }
        Ok(())
    }
}
pub struct Binding {
    pub derivative: StagePlan,
    pub lod: StagePlan,
    pub coordinate: StagePlan,
    pub coefficient: StagePlan,
    pub plane: StagePlan,
    pub packet: StagePlan,
}
pub struct Program {
    pub(super) input: QuadInput,
    pub(super) preparation: Preparation,
    pub(super) binding: Arc<Binding>,
}
impl Program {
    pub fn compile(
        q: &QuadInput,
        slots: &[Slot],
        binding: Arc<Binding>,
    ) -> Result<Arc<Self>, String> {
        let preparation = prepare(q, slots).map_err(|e| format!("bound preparation: {e:?}"))?;
        binding.audit(&preparation)?;
        Ok(Arc::new(Self {
            input: q.clone(),
            preparation,
            binding,
        }))
    }
    pub fn input(&self) -> &QuadInput {
        &self.input
    }
    pub fn preparation(&self) -> &Preparation {
        &self.preparation
    }
}
impl Binding {
    pub fn build() -> Result<Arc<Self>, String> {
        let mut q = QuadInput {
            force_coarsest: false,
            quad_id: 0,
            mask: 1,
            uv: [[0.003, 0.003]; 4],
            slot: 0,
            material_size_log2: 9,
            filter: Filter::Trilinear,
            lod_bias: 0.5,
        };
        q.uv[1][0] += 1.0 / 512.0;
        let p = prepare(
            &q,
            &[Slot {
                base_address: 4096,
                max_size_log2: 9,
                has_full_mip: true,
                valid: true,
            }],
        )
        .map_err(|e| format!("canonical: {e:?}"))?;
        let outputs = |f: &FrameReport| f.outputs.iter().map(|o| o.name.clone()).collect();
        let d = &p.derivative.frame;
        let c = &p.lod.frame;
        let l = &p.lanes[0];
        let keep_d = d
            .outputs
            .iter()
            .filter(|o| !o.name.starts_with('d'))
            .map(|o| o.name.clone())
            .collect();
        let keep_c = c
            .outputs
            .iter()
            .filter(|o| o.name != "lod" && o.name != "lambda" && !o.name.starts_with("prefix"))
            .map(|o| o.name.clone())
            .collect();
        let keep_coord = l
            .coordinate
            .frame
            .outputs
            .iter()
            .filter(|o| !o.name.starts_with('q'))
            .map(|o| o.name.clone())
            .collect();
        Ok(Arc::new(Self {
            derivative: StagePlan::build(d, 8, keep_d)?,
            lod: StagePlan::build(c, 8, keep_c)?,
            coordinate: StagePlan::build(&l.coordinate.frame, 2, keep_coord)?,
            coefficient: StagePlan::build(&l.coefficient.frame, 2, outputs(&l.coefficient.frame))?,
            plane: StagePlan::build(&l.memberships[0].frame, 1, outputs(&l.memberships[0].frame))?,
            packet: StagePlan::build(&l.packets[0][0].frame, 1, outputs(&l.packets[0][0].frame))?,
        }))
    }
    pub fn audit(&self, p: &Preparation) -> Result<(), String> {
        self.derivative.audit(&p.derivative.frame)?;
        self.lod.audit(&p.lod.frame)?;
        for l in &p.lanes {
            self.coordinate.audit(&l.coordinate.frame)?;
            self.coefficient.audit(&l.coefficient.frame)?;
            for p in &l.memberships {
                self.plane.audit(&p.frame)?;
            }
            for words in &l.packets {
                for word in words {
                    self.packet.audit(&word.frame)?;
                }
            }
        }
        Ok(())
    }
}
