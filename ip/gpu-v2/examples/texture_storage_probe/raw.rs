//! Raw/wrapped/context transport study. The closed numerical ledger supplies
//! arithmetic goldens, not an independently emulated D/LOD arithmetic unit.
use super::*;
pub struct Calendar {
    pub fields: Vec<bound::FfBank>,
    pub span: u64,
    pub storage_plan: bound::StagePlan,
}
#[derive(Clone, Copy)]
struct Word {
    owner: usize,
    value: u64,
}
struct Context {
    owner: usize,
    raw_ready: bool,
    published: u64,
    read_start: Option<u64>,
    lod: Option<u64>,
    cohort: bool,
}
struct Cohort {
    owner: usize,
    slot: usize,
    lanes: Vec<usize>,
    next: usize,
    request: u64,
    params: u128,
}
#[derive(Default)]
pub(crate) struct Stats {
    pub enabled: u64,
    pub wall: u64,
    pub accepted: usize,
    pub captured: usize,
    pub peak_contexts: usize,
    pub peak_tickets: usize,
    pub first_last: Vec<(u64, u64)>,
    first_credit_block: Option<(u64, usize, usize, u64)>,
}
pub(crate) trait LaneSink {
    fn tick(&mut self, t: u64, ready: bool, out: &mut dyn Write) -> Result<(), String>;
    fn reserve(
        &mut self,
        t: u64,
        owner: usize,
        lane: usize,
        p: std::sync::Arc<bound::Program>,
        out: &mut dyn Write,
    ) -> Result<bool, String>;
    fn capture(
        &mut self,
        t: u64,
        owner: usize,
        lane: usize,
        uv: u64,
        params: u128,
        out: &mut dyn Write,
    ) -> Result<(), String>;
    fn live(&self) -> usize;
    fn idle(&self) -> bool;
}
fn expected_uv(p: &bound::Program, lane: usize, axis: usize) -> i128 {
    let f = &p.preparation().derivative.frame;
    f.events
        .iter()
        .find_map(|e| match e.operation {
            Operation::Read { memory, row }
                if f.memories[memory].name == "helper_uv" && row == lane * 2 + axis =>
            {
                Some(f.values[e.output.unwrap()].raw)
            }
            _ => None,
        })
        .unwrap()
}
fn lod_word(p: &bound::Program) -> u128 {
    let f = &p.preparation().lod.frame;
    let mut word = 0;
    let mut at = 0;
    for (name, width) in [
        ("nearest", 1),
        ("halve", 1),
        ("n0", 4),
        ("n1", 4),
        ("side0", 12),
        ("side1", 12),
        ("shift0", 18),
        ("parent0", 9),
        ("parent1", 9),
        ("last_fine", 1),
    ] {
        let raw = f.outputs.iter().find(|o| o.name == name).unwrap().raw as u128;
        word |= (raw & ((1 << width) - 1)) << at;
        at += width;
    }
    assert_eq!(at, 71);
    word
}
pub(crate) fn run(
    qs: &[QuadInput],
    d: &Calendar,
    stress: bool,
    inject_stale: bool,
    out: &mut impl Write,
) -> Result<Stats, String> {
    run_sink(qs, d, stress, inject_stale, None, out)
}
pub(crate) fn run_sink(
    qs: &[QuadInput],
    d: &Calendar,
    stress: bool,
    inject_stale: bool,
    mut sink: Option<&mut dyn LaneSink>,
    out: &mut impl Write,
) -> Result<Stats, String> {
    let b = bound::Binding::build()?;
    let programs = qs
        .iter()
        .map(|q| bound::Program::compile(q, &[support::slot(9, true)], b.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut raw = [[None::<Word>; 64]; 2];
    let mut wrapped = [None::<Word>; 32];
    let mut meta = [None::<Word>; 8];
    let mut lod_ram = [None::<(usize, u128)>; 8];
    let mut slots: Vec<Option<Context>> = (0..8).map(|_| None).collect();
    let mut ingress = None::<(usize, usize, usize)>;
    let mut high = [0_u64; 2];
    let mut pending_raw = None::<(usize, usize, [Word; 2])>;
    let mut pending_meta = None::<(usize, usize, u64, u128, bool)>;
    let mut pending_wrap = None::<(usize, usize, Word)>;
    let mut reader = None::<(usize, usize, usize)>;
    let mut window = [[None::<Word>; 2]; 4];
    let mut low_owner = [None; 4];
    let mut high_owner = [None; 4];
    let mut d_jobs = Vec::<(usize, u64)>::new();
    let mut d_cut = None::<(usize, i128)>; // One declared40-bit registered D handoff.
    let mut lod_cut = None::<(usize, u128)>; // One declared71-bit LOD W/bypass handoff.
    let mut cohort = None::<Cohort>;
    let mut tickets = Vec::<u64>::new();
    let mut next = 0;
    let mut stats = Stats::default();
    let mut begin = vec![0; qs.len()];
    let mut last = vec![0; qs.len()];
    let mut stale_injected = false;
    let mut local_ff = bound::storage::Replay::new(&d.storage_plan);
    while next < qs.len()
        || slots.iter().any(Option::is_some)
        || pending_wrap.is_some()
        || sink.as_ref().is_some_and(|s| !s.idle())
    {
        if stats.wall >= 200_000 {
            return Err("raw transport watchdog".into());
        }
        stats.wall += 1;
        let ce = !stress || stats.wall % 23 > 7;
        if !ce {
            continue;
        } // BP, address/return-valid, all work FF and pointers freeze together.
        let t = stats.enabled;
        stats.enabled += 1;
        tickets.retain(|&end| end > t);
        let input_valid = !stress || t % 19 > 4;
        let downstream = !stress || t % 140 > 35;
        if let Some(s) = sink.as_mut() {
            s.tick(t, downstream, out)?;
        }
        let mut raw_r = None;
        let mut raw_w = None;
        let mut wrap_r = None;
        let mut wrap_w = None;
        let mut meta_r = None;
        let mut meta_w = None;
        let mut lod_r = None;
        let mut lod_w = None;
        // Downstream capture observes the prior edge's BP before any same-edge write.
        if let Some((owner, row, data)) = pending_raw.take() {
            let helper = row / 2;
            for axis in 0..2 {
                if data[axis].owner != owner {
                    return Err("raw BP owner".into());
                }
                if row % 2 == 0 {
                    window[helper][axis] = Some(Word {
                        owner,
                        value: data[axis].value,
                    });
                    low_owner[helper] = Some(owner);
                } else {
                    if low_owner[helper] != Some(owner) {
                        return Err("raw high without matching low".into());
                    }
                    let w = window[helper][axis].as_mut().ok_or("missing low half")?;
                    w.value |= (data[axis].value & 15) << 36;
                    w.owner = owner;
                    let signed = if w.value >> 39 != 0 {
                        w.value as i128 - (1_i128 << 40)
                    } else {
                        w.value as i128
                    };
                    if signed != expected_uv(&programs[owner], helper, axis) {
                        return Err("raw signed40 mismatch".into());
                    }
                    high_owner[helper] = Some(owner);
                }
            }
            writeln!(out, "{stress},{t},raw capture,{owner},{row},-,-").unwrap();
        }
        for &(owner, start) in &d_jobs {
            let f = &programs[owner].preparation().derivative.frame;
            for field in &d.fields {
                let e = &f.events[f.values[field.value].producer];
                if let Operation::Read { memory, row } = e.operation {
                    if f.memories[memory].name == "helper_uv"
                        && field.read_times.contains(&(t - start))
                    {
                        let w = window[row / 2][row % 2].ok_or("D window absent")?;
                        if w.owner != owner
                            || high_owner[row / 2] != Some(owner)
                            || w.value != (f.values[field.value].raw as u64 & ((1_u64 << 40) - 1))
                        {
                            return Err("D read owner/value mismatch".into());
                        }
                        writeln!(out, "{stress},{t},D window read,{owner},{row},-,-").unwrap();
                    }
                }
            }
            if t == start + d.span {
                d_cut = Some((
                    owner,
                    f.outputs.iter().find(|o| o.name == "slope").unwrap().raw,
                ));
            }
        }
        d_jobs.retain(|&(_, start)| t < start + d.span);
        // LOD starts on phase6 from the actual D output, with a registered capture edge.
        if t % 8 == 6 {
            if let Some(slot) = slots
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.as_ref().is_some_and(|c| {
                        c.lod.is_none() && c.read_start.is_some_and(|start| t == start + d.span + 1)
                    })
                })
                .min_by_key(|(_, c)| c.as_ref().unwrap().owner)
                .map(|(s, _)| s)
            {
                let c = slots[slot].as_mut().unwrap();
                c.lod = Some(t);
                let source = &programs[c.owner].preparation().derivative.frame;
                let slope = source
                    .outputs
                    .iter()
                    .find(|o| o.name == "slope")
                    .unwrap()
                    .raw;
                if d_cut != Some((c.owner, slope)) {
                    return Err("D registered handoff owner/value".into());
                }
                let f = &programs[c.owner].preparation().lod.frame;
                let consumed = f
                    .events
                    .iter()
                    .find_map(|e| match e.operation {
                        Operation::Read { memory, .. } if f.memories[memory].name == "slope" => {
                            Some(f.values[e.output.unwrap()].raw)
                        }
                        _ => None,
                    })
                    .unwrap();
                if slope != consumed {
                    return Err("D to LOD value".into());
                }
                writeln!(
                    out,
                    "{stress},{t},LOD source capture,{},{slot},-,-",
                    c.owner
                )
                .unwrap();
            }
        }
        for (slot, c) in slots.iter().enumerate() {
            if let Some(c) = c {
                if c.lod.is_some_and(|start| t == start + 27) {
                    lod_cut = Some((c.owner, lod_word(&programs[c.owner])));
                }
                if c.lod.is_some_and(|start| t == start + 28) {
                    if lod_w.replace(slot).is_some() {
                        return Err("LOD W port conflict".into());
                    }
                    let cut = lod_cut.ok_or("missing LOD W handoff")?;
                    if cut.0 != c.owner {
                        return Err("LOD W handoff owner".into());
                    }
                    lod_ram[slot] = Some(cut);
                    writeln!(out, "{stress},{t},LOD write,{},{slot},-,-", c.owner).unwrap();
                }
            }
        }
        if let Some((owner, slot, header, mut params, direct)) = pending_meta.take() {
            if direct {
                let cut = lod_cut.ok_or("missing direct LOD handoff")?;
                if cut.0 != owner {
                    return Err("direct LOD handoff owner".into());
                }
                params = cut.1;
            }
            if slots[slot].as_ref().is_none_or(|c| c.owner != owner) {
                return Err("metadata return owner".into());
            }
            let p = &programs[owner];
            let expected = (p.input().quad_id as u64)
                | (p.input().mask as u64) << 4
                | (p.input().slot as u64) << 8;
            if header != expected {
                return Err("metadata value".into());
            }
            let lanes = (0..4)
                .filter(|lane| p.input().mask >> lane & 1 != 0)
                .collect::<Vec<_>>();
            if sink.is_none() && tickets.len() + lanes.len() > 16 {
                return Err("cohort tickets overflow".into());
            }
            // All lane credits are reserved before the first source read.
            if sink.is_none() {
                for i in 0..lanes.len() {
                    tickets.push(t + 1 + 2 * i as u64 + 26);
                }
                stats.peak_tickets = stats.peak_tickets.max(tickets.len());
            }
            if direct && slots[slot].as_ref().unwrap().lod.unwrap() + 27 != t {
                return Err("early LOD bypass".into());
            }
            if params != lod_word(p) {
                return Err("LOD view mismatch".into());
            }
            cohort = Some(Cohort {
                owner,
                slot,
                lanes,
                next: 0,
                request: t,
                params,
            });
        }
        if let Some((owner, lane, mut w)) = pending_wrap.take() {
            if inject_stale && !stale_injected {
                w.owner = owner + 1;
                stale_injected = true;
            }
            let c = cohort.as_ref().ok_or("missing cohort at capture")?;
            if w.owner != owner || c.owner != owner {
                return Err("wrapped BP owner".into());
            }
            let u = expected_uv(&programs[owner], lane, 0) as u64 & ((1 << 18) - 1);
            let v = expected_uv(&programs[owner], lane, 1) as u64 & ((1 << 18) - 1);
            if w.value != u | (v << 18) || c.params != lod_word(&programs[owner]) {
                return Err("coordinate source mismatch".into());
            }
            if let Some(s) = sink.as_mut() {
                s.capture(t, owner, lane, w.value, c.params, out)?;
            }
            last[owner] = t;
            stats.captured += 1;
            writeln!(
                out,
                "{stress},{t},coordinate source capture,{owner},{lane},-,-"
            )
            .unwrap();
            if c.next == c.lanes.len() {
                let slot = c.slot;
                slots[slot] = None;
                cohort = None;
                writeln!(
                    out,
                    "{stress},{t},source context release,{owner},{slot},-,-"
                )
                .unwrap();
            }
        }
        if let Some(c) = cohort.as_mut() {
            if (sink.is_none() && t == c.request)
                || (sink.is_some() && t >= c.request && t % 2 == 1)
            {
                let lane = c.lanes[c.next];
                let reserved = if let Some(s) = sink.as_mut() {
                    s.reserve(t, c.owner, lane, programs[c.owner].clone(), out)?
                } else {
                    true
                };
                if reserved {
                    let addr = c.slot * 4 + lane;
                    wrap_r = Some(addr);
                    let w = wrapped[addr].ok_or("unwritten wrapped row")?;
                    pending_wrap = Some((c.owner, lane, w));
                    c.next += 1;
                    c.request = t + 2;
                    writeln!(out, "{stress},{t},wrapped R,{},{addr},-,-", c.owner).unwrap();
                } else {
                    writeln!(out, "{stress},{t},lane credit stall,{},{lane},-,-", c.owner).unwrap();
                }
            }
        }
        // Header and saved LOD have distinct read/write owners. First-lane
        // bypass uses the next edge's LOD output; blocked cohorts read the bank.
        if t % 2 == 0
            && cohort.is_none()
            && pending_meta.is_none()
            && (downstream || sink.is_some())
        {
            if let Some(slot) = slots
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.as_ref().is_some_and(|c| {
                        !c.cohort && c.lod.is_some_and(|s| s + 27 == t + 1 || s + 28 < t)
                    })
                })
                .min_by_key(|(_, c)| c.as_ref().unwrap().owner)
                .map(|(s, _)| s)
            {
                let c = slots[slot].as_mut().unwrap();
                let mask = programs[c.owner].input().mask;
                let needed = mask.count_ones() as usize;
                // This bounded downstream fixture schedules fixed-latency
                // returns. A composed controller must use an actual pending
                // read-return credit, not predict an unaccepted consumer.
                let next_occupied = tickets.iter().filter(|&&end| end > t + 1).count();
                if needed == 0 {
                    // Mask is retained in the narrow admission/controller state
                    // (the old prep control is still fully billed). No wrapped
                    // consumer exists; D/LOD nevertheless ran on all helpers.
                    let owner = c.owner;
                    last[owner] = t;
                    slots[slot] = None;
                    writeln!(
                        out,
                        "{stress},{t},zero-mask source release,{owner},{slot},-,-"
                    )
                    .unwrap();
                } else if sink.is_some() || next_occupied + needed <= 16 {
                    meta_r = Some(slot);
                    let hdr = meta[slot].ok_or("metadata absent")?;
                    if hdr.owner != c.owner {
                        return Err("metadata read owner".into());
                    }
                    let direct = c.lod.unwrap() + 27 == t + 1;
                    let params = if direct {
                        0 // No future numerical output is captured on the R edge.
                    } else {
                        lod_r = Some(slot);
                        let (owner, word) = lod_ram[slot].ok_or("LOD absent")?;
                        if owner != c.owner {
                            return Err("LOD read owner".into());
                        }
                        word
                    };
                    pending_meta = Some((c.owner, slot, hdr.value, params, direct));
                    c.cohort = true;
                    writeln!(out, "{stress},{t},context R,{},{slot},{direct},-", c.owner).unwrap();
                } else {
                    let next_return = *tickets.iter().filter(|&&end| end > t + 1).min().unwrap();
                    stats.first_credit_block.get_or_insert((
                        t,
                        needed,
                        16 - next_occupied,
                        next_return,
                    ));
                    writeln!(
                        out,
                        "{stress},{t},cohort credit stall,{},{slot},{needed},{}",
                        c.owner,
                        16 - next_occupied
                    )
                    .unwrap();
                }
            }
        }
        if ingress.is_none() && next < programs.len() && input_valid {
            if let Some(slot) = slots.iter().position(Option::is_none) {
                let owner = next;
                next += 1;
                begin[owner] = t;
                slots[slot] = Some(Context {
                    owner,
                    raw_ready: false,
                    published: u64::MAX,
                    read_start: None,
                    lod: None,
                    cohort: false,
                });
                ingress = Some((owner, slot, 0));
                meta_w = Some(slot);
                let q = programs[owner].input();
                meta[slot] = Some(Word {
                    owner,
                    value: q.quad_id as u64 | (q.mask as u64) << 4 | (q.slot as u64) << 8,
                });
                stats.accepted += 1;
                writeln!(out, "{stress},{t},receiver reserve,{owner},{slot},-,-").unwrap();
            }
        }
        if let Some((owner, slot, row)) = ingress {
            if row % 2 == 1 || input_valid {
                let lane = row / 2;
                let addr = slot * 8 + row;
                raw_w = Some(addr);
                for axis in 0..2 {
                    let encoded =
                        expected_uv(&programs[owner], lane, axis) as u64 & ((1_u64 << 40) - 1);
                    let value = if row % 2 == 0 {
                        high[axis] = encoded >> 36;
                        encoded & ((1_u64 << 36) - 1)
                    } else {
                        high[axis]
                    };
                    raw[axis][addr] = Some(Word { owner, value });
                }
                if row % 2 == 0 {
                    let addr = slot * 4 + lane;
                    wrap_w = Some(addr);
                    let u = expected_uv(&programs[owner], lane, 0) as u64 & ((1 << 18) - 1);
                    let v = expected_uv(&programs[owner], lane, 1) as u64 & ((1 << 18) - 1);
                    wrapped[addr] = Some(Word {
                        owner,
                        value: u | (v << 18),
                    });
                }
                if row == 7 {
                    let c = slots[slot].as_mut().unwrap();
                    c.raw_ready = true;
                    c.published = t;
                    ingress = None;
                } else {
                    ingress = Some((owner, slot, row + 1));
                }
                writeln!(out, "{stress},{t},raw W,{owner},{addr},-,-").unwrap();
            }
        }
        if reader.is_none() && t % 8 == 0 {
            if let Some(slot) = slots
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.as_ref()
                        .is_some_and(|c| c.raw_ready && c.published < t && c.read_start.is_none())
                })
                .min_by_key(|(_, c)| c.as_ref().unwrap().owner)
                .map(|(s, _)| s)
            {
                let c = slots[slot].as_mut().unwrap();
                c.read_start = Some(t);
                reader = Some((c.owner, slot, 0));
                d_jobs.push((c.owner, t));
                local_ff.issue(t, &programs[c.owner].preparation().derivative.frame)?;
                if d_jobs.len() > 3 {
                    return Err("D active token bound".into());
                }
            }
        }
        if let Some((owner, slot, row)) = reader {
            let addr = slot * 8 + row;
            raw_r = Some(addr);
            let words = [
                raw[0][addr].ok_or("raw U absent")?,
                raw[1][addr].ok_or("raw V absent")?,
            ];
            if words.iter().any(|w| w.owner != owner) {
                return Err("raw request owner".into());
            }
            pending_raw = Some((owner, row, words));
            reader = if row == 7 {
                None
            } else {
                Some((owner, slot, row + 1))
            };
            writeln!(out, "{stress},{t},raw R,{owner},{addr},-,-").unwrap();
        }
        for (read, write, name) in [
            (raw_r, raw_w, "raw"),
            (wrap_r, wrap_w, "wrapped"),
            (meta_r, meta_w, "metadata"),
            (lod_r, lod_w, "LOD"),
        ] {
            if read.is_some() && read == write {
                return Err(format!("{name} same-row R/W dependency"));
            }
        }
        let live = slots.iter().filter(|s| s.is_some()).count();
        stats.peak_contexts = stats.peak_contexts.max(live);
        if let Some(s) = sink.as_ref() {
            stats.peak_tickets = stats.peak_tickets.max(s.live());
        }
        local_ff.tick(t)?;
    }
    if next != programs.len()
        || stats.captured != qs.iter().map(|q| q.mask.count_ones() as usize).sum()
    {
        return Err("raw study did not drain".into());
    }
    stats.first_last = begin.into_iter().zip(last).collect();
    assert!(local_ff.idle());
    Ok(stats)
}
pub fn probe(root: &Path, d: &Calendar) {
    let mut events = fs::File::create(root.join("raw_edges.csv")).unwrap();
    let mut summary = fs::File::create(root.join("raw_summary.csv")).unwrap();
    writeln!(
        events,
        "stress,enabled,event,owner,address_or_lane,direct_LOD,extra"
    )
    .unwrap();
    writeln!(summary,"case,stress,wall,enabled,accepted,covered_captures,peak_contexts,peak_lane_tickets,steady_quad_input_interval,maximum_context_age,first_credit_block").unwrap();
    for (case, stress) in [
        ("full", false),
        ("full", true),
        ("sparse", true),
        ("zero", true),
    ] {
        let mut qs = baseline::inputs("fractional", 15, 64);
        if case == "sparse" {
            for (i, q) in qs.iter_mut().enumerate() {
                q.mask = [1, 9, 15][i % 3];
            }
        }
        if case == "zero" {
            for (i, q) in qs.iter_mut().enumerate() {
                q.mask = if i % 7 == 0 { 0 } else { [1, 9, 15][i % 3] };
            }
        }
        // Exercise signed high halves and exact low18, with deliberately equal
        // sample values in distinct logical owners as well as changing values.
        for (i, q) in qs.iter_mut().enumerate() {
            if i % 5 == 0 {
                for uv in &mut q.uv {
                    uv[0] -= 1.0;
                    uv[1] += 2.0;
                }
            }
        }
        let s = run(&qs, d, stress, false, &mut events).unwrap();
        let age = s.first_last.iter().map(|(a, z)| z - a).max().unwrap();
        let interval = (s.first_last[48].0 - s.first_last[16].0) as f64 / 32.0;
        writeln!(
            summary,
            "{case},{stress},{},{},{},{},{},{},{interval:.6},{age},{}",
            s.wall,
            s.enabled,
            s.accepted,
            s.captured,
            s.peak_contexts,
            s.peak_tickets,
            csv(&format!("{:?}", s.first_credit_block))
        )
        .unwrap();
        println!("raw transport {case}/{stress}: enabled={} accepted={} peakContext={} peakTickets={} middle input II={interval} maxContextAge={age}",s.enabled,s.accepted,s.peak_contexts,s.peak_tickets);
        if case == "full" && !stress {
            assert_eq!(s.first_credit_block, Some((88, 4, 3, 90)));
        }
    }
    let qs = baseline::inputs("fractional", 15, 4);
    assert!(
        run(&qs, d, true, true, &mut std::io::sink()).is_err(),
        "stale owner injection must fail"
    );
}
