//! Independent data/port/order checks plus deterministic controller replay.
use super::*;
struct Replay<'a> {
    step: &'a Step,
    submission: usize,
}
impl RefillPort for Replay<'_> {
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        Ok(self.step.responses.clone())
    }
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        let requests: Vec<_> = self
            .step
            .events
            .iter()
            .filter_map(|e| {
                if let Event::Submitted { id, address, .. } = e {
                    Some((*id, *address))
                } else {
                    None
                }
            })
            .collect();
        let (id, expected) = requests
            .get(self.submission)
            .ok_or("unrecorded refill submission")?;
        if bytes != 128 || address != *expected {
            return Err("replayed memory request differs".into());
        }
        self.submission += 1;
        Ok(*id)
    }
}
pub fn audit(report: &Report) -> Result<(), Error> {
    report.hardware.validate()?;
    if report.steps.len() as u64 > report.hardware.max_cycles
        || report.programs.len() > report.hardware.max_quads
    {
        return Err("trace budget".into());
    }
    for p in &report.programs {
        p.audit(&report.slots, &report.hardware)?;
    }
    let mut replay = Machine::new(report.slots.clone(), report.hardware.clone())?;
    let mut next = 0;
    let mut output = Vec::new();
    let mut states = [State::Invalid; 64];
    let mut keys = [None; 64];
    let mut replacement = [[0_usize; 3]; 16];
    let mut tiles = [[0_u16; 64]; 64];
    let mut written_words = [0_u64; 64];
    let mut reservation = 0_u64;
    let mut consumed = VecDeque::new();
    let mut captures = VecDeque::new();
    let mut partials = VecDeque::new();
    let mut color_issues = Vec::new();
    let mut sample = None;
    let mut sum = [0_u64; 3];
    let mut expected_results = VecDeque::new();
    let mut color_clock = 0;
    let mut read_times = VecDeque::new();
    for step in &report.steps {
        if step.cycle != replay.stats.wall_cycles + 1 {
            return Err("trace cycle order".into());
        }
        let offered = if let Some(i) = step.offered {
            if i != next {
                return Err("offered quad order".into());
            }
            Some((
                i,
                report
                    .programs
                    .get(i)
                    .ok_or("offered program index")?
                    .clone(),
            ))
        } else {
            None
        };
        let mut memory = Replay {
            step,
            submission: 0,
        };
        let got = replay.step(&mut memory, offered, step.control)?;
        if &got != step {
            return Err(format!("controller replay differs at cycle {}", step.cycle).into());
        }
        if step.accepted {
            next += 1;
        }
        if step.control.ce {
            color_clock += 1;
        }
        let mut write_line = None;
        let mut read_count = 0;
        let mut allocations = 0;
        let mut produced_count = 0;
        for event in &step.events {
            if !step.control.ce
                && !matches!(
                    event,
                    Event::Beat { .. } | Event::Ready { .. } | Event::Submitted { .. }
                )
            {
                return Err("consumer state changed with CE=0".into());
            }
            match event {
                Event::Allocate {
                    key,
                    line,
                    address,
                    prefetch,
                } => {
                    allocations += 1;
                    if *line >= 64
                        || line / 4 != key.set()
                        || states[*line] == State::Filling
                        || reservation >> line & 1 != 0
                        || key.address(&report.slots)? != *address
                    {
                        return Err("allocation key/victim/reservation".into());
                    }
                    let protected = if *prefetch {
                        consumed.front().map(|(g, _): &(Group4, i128)| g.key)
                    } else {
                        None
                    };
                    let eligible = |way: usize| {
                        let i = key.set() * 4 + way;
                        states[i] != State::Filling
                            && reservation >> i & 1 == 0
                            && (protected.is_none() || keys[i] != protected)
                    };
                    let tree = replacement[key.set()];
                    let branch = tree[0];
                    let leaf = tree[branch + 1];
                    let other_leaf = tree[(branch ^ 1) + 1];
                    let order = [
                        branch * 2 + leaf,
                        branch * 2 + (leaf ^ 1),
                        (branch ^ 1) * 2 + other_leaf,
                        (branch ^ 1) * 2 + (other_leaf ^ 1),
                    ];
                    let victim = (0..4)
                        .find(|&w| states[key.set() * 4 + w] == State::Invalid && eligible(w))
                        .or_else(|| {
                            order
                                .into_iter()
                                .find(|&w| states[key.set() * 4 + w] == State::Ready && eligible(w))
                        });
                    if victim.map(|w| key.set() * 4 + w) != Some(*line) {
                        return Err("INVALID/PLRU order or protected-head eviction".into());
                    }
                    if keys
                        .iter()
                        .zip(states)
                        .enumerate()
                        .any(|(i, (k, s))| i != *line && s != State::Invalid && *k == Some(*key))
                    {
                        return Err("duplicate READY/FILLING key".into());
                    }
                    states[*line] = State::Filling;
                    keys[*line] = Some(*key);
                    written_words[*line] = 0;
                }
                Event::Beat { line, index, data } => {
                    if *line >= 64
                        || states[*line] != State::Filling
                        || reservation >> line & 1 != 0
                        || write_line.is_some()
                        || *index >= 16
                    {
                        return Err("refill port/state collision".into());
                    }
                    write_line = Some(*line);
                    for j in 0..4 {
                        let word = index * 4 + j;
                        let bit = 1_u64 << word;
                        if written_words[*line] & bit != 0 {
                            return Err("duplicate refill word".into());
                        }
                        written_words[*line] |= bit;
                        tiles[*line][word] = (data >> (j * 16)) as u16;
                    }
                }
                Event::Ready { key, line } => {
                    if states[*line] != State::Filling
                        || keys[*line] != Some(*key)
                        || written_words[*line] != u64::MAX
                    {
                        return Err("READY before all 64 words".into());
                    }
                    states[*line] = State::Ready;
                    touch(&mut replacement, *line);
                }
                Event::Produced { group, payload } => {
                    produced_count += 1;
                    if group.pack72()? != *payload as u128 {
                        return Err("packet decode differs from finished payload".into());
                    }
                    consumed.push_back((group.clone(), *payload));
                }
                Event::Read {
                    group,
                    payload,
                    line,
                    ..
                } => {
                    read_count += 1;
                    if states[*line] != State::Ready
                        || keys[*line] != Some(group.key)
                        || reservation >> line & 1 != 0
                        || write_line == Some(*line)
                        || consumed.pop_front() != Some((group.clone(), *payload))
                    {
                        return Err("demand order/read-during-write/reservation".into());
                    }
                    reservation |= 1 << line;
                    touch(&mut replacement, *line);
                    read_times.push_back((group.clone(), color_clock));
                    for instance in 4..16 {
                        let issue = color_clock + report.hardware.read_latency;
                        color_issues.push(audited::physical::DspIssue {
                            instance,
                            issue,
                            ready: issue + report.hardware.multiply_latency,
                            work: audited::physical::DspWork::Multiply {
                                a_bits: 9,
                                b_bits: 8,
                            },
                        });
                    }
                }
                Event::Captured { group, line, words } => {
                    if reservation >> line & 1 == 0
                        || states[*line] != State::Ready
                        || keys[*line] != Some(group.key)
                    {
                        return Err("lost read reservation".into());
                    }
                    let expected = std::array::from_fn(|t| {
                        let x = (usize::from(group.top_left_local[0]) + (t & 1)) % 8;
                        let y = (usize::from(group.top_left_local[1]) + (t >> 1)) % 8;
                        tiles[*line][y * 8 + x]
                    });
                    if *words != expected {
                        return Err("bank payload differs from row-major refill".into());
                    }
                    reservation &= !(1 << line);
                    captures.push_back((group.clone(), *words));
                }
                Event::Partial { group, value } => {
                    let (expected_group, words) =
                        captures.pop_front().ok_or("partial before capture")?;
                    if &expected_group != group {
                        return Err("partial order".into());
                    }
                    let mut partial = [0_u64; 3];
                    for (t, word) in words.into_iter().enumerate() {
                        let r = (word >> 11) as u64;
                        let g = ((word >> 5) & 63) as u64;
                        let b = (word & 31) as u64;
                        let rgb = [r * 8 + r / 4, g * 4 + g / 16, b * 8 + b / 4];
                        for c in 0..3 {
                            partial[c] += rgb[c] * u64::from(group.coefficients[t]);
                        }
                    }
                    if partial != value.map(u64::from) {
                        return Err("closed partial value differs".into());
                    }
                    partials.push_back((group.clone(), *value));
                }
                Event::Accumulate { group, value } => {
                    let identity = (group.quad_id, group.lane);
                    if group.first {
                        if sample.is_some() {
                            return Err("sample interrupted".into());
                        }
                        sample = Some(identity);
                        sum = [0; 3];
                    }
                    if sample != Some(identity) {
                        return Err("accumulator identity/order".into());
                    }
                    let (owner, partial) = partials.pop_front().ok_or("missing partial ledger")?;
                    if owner != *group {
                        return Err("partial feedback order".into());
                    }
                    for c in 0..3 {
                        sum[c] += u64::from(partial[c]);
                        if sum[c] > 130305 {
                            return Err("accumulator bound".into());
                        }
                    }
                    if sum != value.map(u64::from) {
                        return Err("feedback read old accumulator".into());
                    }
                    if group.last {
                        sample = None;
                        let rgb = sum.map(|n| ((n + 255) / 511) as u8);
                        expected_results.push_back(PixelResult {
                            quad_id: group.quad_id,
                            lane: group.lane,
                            rgb,
                        });
                    }
                }
                Event::Result { pixel } => {
                    if expected_results.pop_front() != Some(pixel.clone()) {
                        return Err("final normalization/order".into());
                    }
                    // All groups have the same fixed read/multiply/tree/feedback latency.
                    let mut start = None;
                    while let Some((group, cycle)) = read_times.pop_front() {
                        if group.last {
                            if (group.quad_id, group.lane) != (pixel.quad_id, pixel.lane) {
                                return Err("result pipeline order".into());
                            }
                            start = Some(cycle);
                            break;
                        }
                    }
                    if color_clock - start.ok_or("result without last issue")?
                        != report.hardware.color_latency()
                    {
                        return Err("static color latency".into());
                    }
                }
                Event::Commit { pixel } => {
                    if !step.control.result_ready {
                        return Err("commit under output backpressure".into());
                    }
                    output.push(pixel.clone());
                }
                Event::PrefetchHit { key, line } => {
                    if states[*line] != State::Ready || keys[*line] != Some(*key) {
                        return Err("prefetch READY lookup".into());
                    }
                    touch(&mut replacement, *line);
                }
                _ => {}
            }
        }
        if read_count > 1 || allocations > 1 || produced_count > 1 {
            return Err("per-edge issue capacity".into());
        }
        let s = &step.snapshot;
        if s.groups > report.hardware.group_capacity
            || s.hints > report.hardware.hint_capacity
            || s.descriptors > report.hardware.descriptor_capacity
            || s.quads > report.hardware.quad_capacity
            || s.results > report.hardware.result_capacity
            || s.result_credits > report.hardware.result_capacity
            || s.pipeline > report.hardware.color_latency() as usize
            || s.reservations != reservation
        {
            return Err("bounded storage/reservation accounting".into());
        }
    }
    if !replay.idle()
        || next != report.programs.len()
        || replay.stats != report.stats
        || output != report.pixels
        || !consumed.is_empty()
        || !captures.is_empty()
        || !partials.is_empty()
        || sample.is_some()
        || !expected_results.is_empty()
        || !read_times.is_empty()
    {
        return Err("final drain/report totals".into());
    }
    report
        .hardware
        .inventory()?
        .audit_issues(
            &color_issues,
            None,
            report.hardware.max_cycles + report.hardware.color_latency(),
        )
        .map_err(|e| Error(format!("color DSP packing: {e:?}")))?;
    Ok(())
}
fn touch(tree: &mut [[usize; 3]; 16], line: usize) {
    let branch = (line % 4) / 2;
    let leaf = line % 2;
    tree[line / 4][0] = branch ^ 1;
    tree[line / 4][branch + 1] = leaf ^ 1;
}
