//! Bounded storage study over the reproducible native trace. No controller,
//! numerical contract, RTL or fitted resource claim is changed by this probe.
#[path = "texture_bound_probe.rs"]
#[allow(dead_code)]
pub(crate) mod baseline;
use audited::{physical::Timing, MemoryKind, Operation};
pub(crate) use baseline::{physical, support};
use gpu_v2::texture::{
    ports::*,
    sim::{
        staged::{binding, bound},
        timed,
    },
};
use resource_scheduler::{ModuloGraph, SearchConfig};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    path::Path,
};
#[path = "texture_storage_probe/color.rs"]
mod color;
#[path = "texture_storage_probe/layout.rs"]
mod layout;
#[path = "texture_storage_probe/packet.rs"]
pub(crate) mod packet;
#[path = "texture_storage_probe/raw.rs"]
pub(crate) mod raw;

struct Interval {
    name: &'static str,
    owner: usize,
    bits: u64,
    birth: u64,
    last: u64,
}
fn add(xs: &mut Vec<Interval>, name: &'static str, owner: usize, bits: u64, birth: u64, last: u64) {
    assert!(last >= birth, "{name}: reversed lifetime");
    xs.push(Interval {
        name,
        owner,
        bits,
        birth,
        last,
    });
}
pub(crate) fn stream_d(root: &Path, b: &bound::Binding) -> raw::Calendar {
    let q = support::input(9, Filter::Trilinear, [0.003; 2]);
    let p = bound::prepare(&q, &[support::slot(9, true)]).unwrap();
    let f = &p.derivative.frame;
    let evidence = binding::Evidence::build(f).unwrap();
    let mut graph = b.derivative.graph.clone();
    for e in &f.events {
        if let Operation::Read { memory, row } = e.operation {
            if f.memories[memory].kind == MemoryKind::Input
                && f.memories[memory].name == "helper_uv"
            {
                graph.nodes[e.id].earliest = 2 + 2 * (row / 2) as u64;
            }
        }
    }
    let mg = ModuloGraph::from_graph(&graph).unwrap();
    let schedule = resource_scheduler::modulo_schedule(&mg, 8, &SearchConfig::default()).unwrap();
    let times: Vec<_> = schedule
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| Timing {
            issue: n.issue,
            ready: n.issue
                + graph.nodes[i]
                    .resource
                    .map_or(0, |r| graph.resources[r].latency),
        })
        .collect();
    evidence
        .lowering
        .audit_timing(f, &times, 1, 100000)
        .unwrap();
    let cones = evidence.lowering.logic_cones(f, 1).unwrap();
    let fields = bound::storage::fields(
        f,
        &graph,
        &times,
        &cones,
        &evidence.lowering,
        &["slope".into()],
        schedule.span,
    )
    .unwrap();
    let mut out = fs::File::create(root.join("stream_d.csv")).unwrap();
    writeln!(out, "event,operation,issue,ready,site,lane").unwrap();
    for (e, t) in f.events.iter().zip(&times) {
        writeln!(
            out,
            "{},{},{},{},{},{:?}",
            e.id,
            csv(&format!("{:?}", e.operation)),
            t.issue,
            t.ready,
            csv(&format!(
                "{:?}",
                graph.nodes[e.id].resource.map(|r| &graph.resources[r].name)
            )),
            schedule.nodes[e.id].lane
        )
        .unwrap();
    }
    let mut windows = fs::File::create(root.join("stream_d_window.csv")).unwrap();
    writeln!(windows,"value,helper,source_low,width,first_low_return,full_return,last_consumer,next_low_return,one_window_safe").unwrap();
    let mut safe = true;
    for field in &fields {
        let e = &f.events[f.values[field.value].producer];
        if let Operation::Read { memory, row } = e.operation {
            if f.memories[memory].name == "helper_uv" {
                let birth_low = 1 + 2 * (row / 2) as u64;
                let next = if field.source_low >= 36 {
                    birth_low + 9
                } else {
                    birth_low + 8
                };
                let good = field.last_read < next;
                safe &= good;
                writeln!(
                    windows,
                    "{},{},{},{},{},{},{},{},{good}",
                    field.value,
                    row / 2,
                    field.source_low,
                    field.width,
                    birth_low,
                    birth_low + 1,
                    field.last_read,
                    next
                )
                .unwrap();
            }
        }
    }
    let local: Vec<_> = fields.iter().filter(|v| {
        !matches!(f.events[f.values[v.value].producer].operation, Operation::Read { memory, .. } if f.memories[memory].kind == MemoryKind::Input)
    }).cloned().collect();
    let packed = bound::storage::Layout::build(&local, 8).unwrap();
    println!("stream D: span={} oneRawWindowSafe={safe} localFF={} controlFF={} readMux={} writeMux={} enableGates={} rawWindowFF=320",schedule.span,
        packed.ff_bits,packed.control_ff_bits,packed.read_selector_tree_bits,packed.write_selector_tree_bits,packed.control_boolean_gates);
    assert!(safe, "single window lifetime conflict");
    // Replay consumes only II/fields/layout. This holder is never submitted as
    // an arithmetic StagePlan certificate; graph dependencies were audited above.
    let mut storage_plan = std::sync::Arc::try_unwrap(bound::Binding::build().unwrap())
        .unwrap_or_else(|_| panic!("fresh binding unexpectedly shared"))
        .derivative;
    storage_plan.packed_fields = local;
    storage_plan.packed = packed;
    raw::Calendar {
        fields,
        span: schedule.span,
        storage_plan,
    }
}
pub(crate) fn csv(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn liveness(r: &bound::system::Report) -> Vec<Interval> {
    use bound::control::Event as P;
    let mut xs = vec![];
    let mut accepts = BTreeMap::new();
    let mut d = BTreeMap::new();
    let mut lod = BTreeMap::new();
    let mut coord = BTreeMap::new();
    let mut coef = BTreeMap::new();
    let mut members = BTreeMap::new();
    let mut packet = VecDeque::new();
    let mut last_coord = BTreeMap::new();
    let mut t = 0;
    for s in &r.preparation {
        if !s.ce {
            assert!(s.events.is_empty());
            continue;
        }
        for e in &s.events {
            match *e {
                P::Accept { program, .. } => {
                    accepts.insert(program, t);
                }
                P::Issue {
                    stage,
                    program,
                    lane,
                    plane,
                    packet: index,
                } => match stage {
                    "derivative" => {
                        d.insert(program, t);
                    }
                    "lod" => {
                        lod.insert(program, t);
                    }
                    "coordinate" => {
                        coord.insert((program, lane), t);
                        last_coord.insert(program, t);
                    }
                    "coefficient" => {
                        coef.insert((program, lane), t);
                    }
                    "membership" => {
                        members.insert((program, lane, plane), t);
                    }
                    "packet" => {
                        packet.push_back((program, lane, plane, index, t));
                    }
                    _ => unreachable!(),
                },
                P::SharedRelease { program, .. } => {
                    add(
                        &mut xs,
                        "context occupied capacity",
                        program,
                        387,
                        accepts[&program],
                        t,
                    );
                }
                P::Packet { program, payload } => {
                    let (pi, li, wi, gi, issue) = packet.pop_front().unwrap();
                    assert_eq!(pi, program);
                    let frame = &r.programs[pi].preparation().lanes[li].packets[wi][gi].frame;
                    assert_eq!(
                        frame
                            .outputs
                            .iter()
                            .find(|o| o.name == "packet")
                            .unwrap()
                            .raw,
                        payload
                    );
                    add(&mut xs, "packet credit (reserved)", pi, 72, issue, t);
                    add(
                        &mut xs,
                        "packet stable holding",
                        pi,
                        72,
                        issue + r.binding.packet.span() + 1,
                        t,
                    );
                }
                _ => {}
            }
        }
        t += 1;
    }
    assert!(packet.is_empty());
    for (&pi, &a) in &accepts {
        let p = r.programs[pi].preparation();
        let dt = d[&pi];
        let lt = lod[&pi];
        let end = *last_coord
            .get(&pi)
            .unwrap_or(&(lt + r.binding.lod.span() + 2));
        add(&mut xs, "context raw high176", pi, 176, a, dt);
        add(&mut xs, "context identity12", pi, 12, a, end);
        add(&mut xs, "context LOD parameters23", pi, 23, a, lt);
        add(
            &mut xs,
            "context slope40",
            pi,
            40,
            dt + r.binding.derivative.span() + 1,
            lt,
        );
        if !p.lanes.is_empty() {
            add(
                &mut xs,
                "context LOD consumer fields71",
                pi,
                71,
                lt + r.binding.lod.span() + 1,
                end,
            );
        }
        for lane in 0..4 {
            let read = p
                .lanes
                .iter()
                .position(|l| l.lane as usize == lane)
                .map_or(dt, |li| coord[&(pi, li)]);
            add(&mut xs, "context wrapped low36 alias", pi, 36, a, read);
        }
        for (li, l) in p.lanes.iter().enumerate() {
            let ct = coord[&(pi, li)];
            let ft = coef[&(pi, li)];
            let first = members[&(pi, li, 0)];
            let last = members[&(pi, li, l.memberships.len() - 1)];
            add(&mut xs, "coordinate credit (reserved)", pi, 150, ct, ft);
            add(
                &mut xs,
                "coordinate pass37",
                pi,
                37,
                ct,
                ct + r.binding.coordinate.span() + 1,
            );
            add(
                &mut xs,
                "coordinate ready150",
                pi,
                150,
                ct + r.binding.coordinate.span() + 1,
                ft,
            );
            add(
                &mut xs,
                "coefficient pass99",
                pi,
                99,
                ft,
                ft + r.binding.coefficient.span() + 1,
            );
            add(
                &mut xs,
                "coefficient ready171",
                pi,
                171,
                ft + r.binding.coefficient.span() + 1,
                last,
            );
            add(
                &mut xs,
                "merged lane row171 (study)",
                pi,
                171,
                ct,
                first + 1,
            );
        }
    }
    // Observe actual color consumers. Identity is the sequential read token,
    // never just the reusable quad/lane ID or sampled color.
    struct C {
        owner: usize,
        birth: u64,
        partial: Option<u64>,
        acc: Option<u64>,
        last: bool,
    }
    let mut color = VecDeque::<C>::new();
    let mut result = VecDeque::new();
    let mut feedback = None;
    let mut clock = 0;
    let mut serial = 0;
    for step in &r.cache.steps {
        if !step.control.ce {
            continue;
        }
        for e in &step.events {
            match e {
                timed::Event::Read { group, .. } => {
                    color.push_back(C {
                        owner: serial,
                        birth: clock,
                        partial: None,
                        acc: None,
                        last: group.last,
                    });
                    serial += 1;
                }
                timed::Event::Captured { .. } => {}
                timed::Event::Partial { .. } => {
                    color
                        .iter_mut()
                        .find(|c| c.partial.is_none())
                        .unwrap()
                        .partial = Some(clock);
                }
                timed::Event::Accumulate { .. } => {
                    if let Some((owner, birth)) = feedback.take() {
                        add(&mut xs, "color feedback58", owner, 58, birth, clock);
                    }
                    let pos = color.iter().position(|c| c.acc.is_none()).unwrap();
                    color[pos].acc = Some(clock);
                    if !color[pos].last {
                        let c = color.remove(pos).unwrap();
                        color_fields(&mut xs, &c, clock);
                        feedback = Some((c.owner, clock));
                    }
                }
                timed::Event::Result { .. } => {
                    let pos = color
                        .iter()
                        .position(|c| c.last && c.acc.is_some())
                        .unwrap();
                    let c = color.remove(pos).unwrap();
                    color_fields(&mut xs, &c, clock);
                    result.push_back((c.owner, clock));
                }
                timed::Event::Commit { .. } => {
                    let (owner, birth) = result.pop_front().unwrap();
                    add(&mut xs, "native result FIFO30", owner, 30, birth, clock);
                }
                _ => {}
            }
        }
        clock += 1;
    }
    assert!(color.is_empty() && result.is_empty() && feedback.is_none());
    fn color_fields(xs: &mut Vec<Interval>, c: &C, end: u64) {
        let partial = c.partial.unwrap();
        let acc = c.acc.unwrap();
        add(xs, "color whole token252", c.owner, 252, c.birth, end);
        add(xs, "color weights36", c.owner, 36, c.birth, partial);
        add(xs, "color address12", c.owner, 12, c.birth, c.birth + 1);
        add(xs, "color marker8", c.owner, 8, c.birth, end);
        add(xs, "color words64", c.owner, 64, c.birth + 1, partial);
        add(xs, "color partial51", c.owner, 51, partial, acc);
        if c.last {
            add(xs, "color final sum51", c.owner, 51, acc, end);
        }
    }
    xs
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map(String::as_str)
            .unwrap_or("target/gpu-v2-texture-storage-study"),
    );
    fs::create_dir_all(root).unwrap();
    let b = bound::Binding::build().unwrap();
    let d = stream_d(root, &b);
    raw::probe(root, &d);
    packet::probe(root);
    layout::probe(root, &b, &d);
    if args.get(1).is_some_and(|s| s == "--calendar-only") {
        return;
    }
    let mut life = fs::File::create(root.join("field_lifetimes.csv")).unwrap();
    let mut peaks = fs::File::create(root.join("occupancy.csv")).unwrap();
    writeln!(
        life,
        "profile,mask,quads,loaded,stress,field,owner,bits,birth_enabled,last_consumer_enabled"
    )
    .unwrap();
    writeln!(
        peaks,
        "profile,mask,quads,loaded,stress,field,post_edge_peak_bits,post_edge_peak_records,inclusive_edge_peak_bits,maximum_age"
    )
    .unwrap();
    let slot = support::slot(9, true);
    let bytes = support::asset(slot, support::pattern);
    for (profile, mask, n, loaded, cold, stress) in [
        ("bilinear", 15, 128, false, false, false),
        ("fractional", 15, 128, false, false, false),
        ("seams", 15, 128, false, false, false),
        ("fractional", 1, 128, false, false, false),
        ("fractional", 9, 128, false, false, false),
        ("cold-scan", 15, 64, false, false, false),
        ("bilinear", 15, 128, true, false, false),
        ("bilinear", 15, 1, false, false, false),
        ("fractional", 15, 4, false, false, false),
        ("bilinear", 15, 1, false, true, false),
        ("fractional", 15, 64, true, false, true),
    ] {
        let qs = baseline::inputs(profile, mask, n);
        let mut memory =
            physical::Physical::new(u64::from(support::BASE), bytes.clone(), !cold, loaded);
        let r = bound::system::run(
            &qs,
            &[slot],
            &mut memory,
            bound::control::Hardware {
                storage: bound::control::Storage::Packed,
                ..Default::default()
            },
            timed::Hardware {
                prefetch: false,
                ..Default::default()
            },
            |clock| timed::Control {
                ce: !stress || clock % 23 > 7,
                result_ready: !stress || (clock > 1200 && clock % 19 > 3),
            },
        )
        .unwrap();
        if (profile == "bilinear" && n == 128 && !loaded) || (profile == "fractional" && stress) {
            color::probe(root, profile, &r, stress);
        }
        let xs = liveness(&r);
        let mut groups = BTreeMap::<&str, Vec<&Interval>>::new();
        for i in &xs {
            writeln!(
                life,
                "{profile},{mask},{n},{loaded},{stress},{},{},{},{},{}",
                i.name, i.owner, i.bits, i.birth, i.last
            )
            .unwrap();
            groups.entry(i.name).or_default().push(i);
        }
        for (name, ivs) in groups {
            let mut edges = BTreeMap::<u64, (i64, i64)>::new();
            let mut inclusive = BTreeMap::<u64, i64>::new();
            let mut age = 0;
            for i in ivs {
                edges.entry(i.birth).or_default().0 += i.bits as i64;
                edges.entry(i.birth).or_default().1 += 1;
                edges.entry(i.last).or_default().0 -= i.bits as i64;
                edges.entry(i.last).or_default().1 -= 1;
                *inclusive.entry(i.birth).or_default() += i.bits as i64;
                *inclusive.entry(i.last + 1).or_default() -= i.bits as i64;
                age = age.max(i.last - i.birth);
            }
            let (mut bits, mut count, mut pb, mut pc) = (0, 0, 0, 0);
            for (db, dc) in edges.into_values() {
                bits += db;
                count += dc;
                pb = pb.max(bits);
                pc = pc.max(count);
            }
            assert_eq!((bits, count), (0, 0));
            let (mut live, mut ip) = (0, 0);
            for n in inclusive.into_values() {
                live += n;
                ip = ip.max(live);
            }
            assert_eq!(live, 0);
            if name == "color whole token252" {
                assert_eq!(pc as usize, r.cache.stats.peak_pipeline);
            }
            if name == "native result FIFO30" {
                assert_eq!(pc as usize, r.cache.stats.peak_results);
            }
            if name == "packet credit (reserved)" {
                assert_eq!(pc as usize, r.stats.peak_packet);
            }
            if name == "coordinate credit (reserved)" {
                assert_eq!(pc as usize, r.stats.peak_coordinates);
            }
            writeln!(
                peaks,
                "{profile},{mask},{n},{loaded},{stress},{name},{pb},{pc},{ip},{age}"
            )
            .unwrap();
        }
        println!("field trace {profile}/{mask}/{n} loaded={loaded} stress={stress}: {} intervals, {} wall cycles, init={} color peak={} result peak={}",xs.len(),r.cache.stats.wall_cycles,memory.init_cycles,r.cache.stats.peak_pipeline,r.cache.stats.peak_results);
    }
}
