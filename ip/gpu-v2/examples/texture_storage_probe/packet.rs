//! Single-owner synchronous packet pool. Cache readiness is explicit stimulus;
//! this transport probe does not replace the native cache/MC composition.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    epoch: u32,
    serial: usize,
    bits: u64,
}
#[derive(Clone, Copy)]
struct Ref {
    serial: usize,
    row: usize,
    edge: u64,
}
struct Pool<const HEADS: usize> {
    banks: [[Option<Row>; 64]; 2],
    producer: VecDeque<Ref>,
    groups: VecDeque<Ref>,
    pending: Option<(Ref, [Row; 2], usize)>,
    heads: [Option<(Ref, i128)>; HEADS],
    pop_head: usize,
    read_head: usize,
    allocate: usize,
    reclaim: usize,
    epoch: u32,
}
impl<const HEADS: usize> Default for Pool<HEADS> {
    fn default() -> Self {
        Self {
            banks: [[None; 64]; 2],
            producer: VecDeque::new(),
            groups: VecDeque::new(),
            pending: None,
            heads: [None; HEADS],
            pop_head: 0,
            read_head: 0,
            allocate: 0,
            reclaim: 0,
            epoch: 0,
        }
    }
}
#[derive(Default)]
struct Stats {
    wall: u64,
    enabled: u64,
    allocations: usize,
    captures: usize,
    consumed: usize,
    peak_p: usize,
    peak_g: usize,
    peak_rows: usize,
    wraps: usize,
    reads: usize,
    writes: usize,
    consume_edges: Vec<u64>,
    fault_edge: Option<u64>,
    reset_edge: Option<u64>,
    accepted_beats: usize,
    peak_heads: usize,
    peak_head_reservations: usize,
    triple_overlap: usize,
    captures_while_stopped: usize,
    ce_edges: usize,
    drain_beats_under_ce: usize,
    ce_with_pending: usize,
    sudden_return_capture: usize,
    fault_with_two_heads: bool,
    post_fault_beats: usize,
    post_fault_beats_under_ce: usize,
    full_g_consume_without_transfer: usize,
}
impl<const HEADS: usize> Pool<HEADS> {
    fn valid_heads(&self) -> usize {
        self.heads.iter().filter(|h| h.is_some()).count()
    }
    fn group_count(&self) -> usize {
        self.groups.len() + usize::from(self.pending.is_some()) + self.valid_heads()
    }
    fn reset(&mut self) -> Result<(), String> {
        if self.pending.is_some() || self.producer.iter().any(|r| r.edge == u64::MAX) {
            return Err("pool reset with outstanding local owner".into());
        }
        self.banks = [[None; 64]; 2];
        self.producer.clear();
        self.groups.clear();
        self.heads = [None; HEADS];
        self.pop_head = 0;
        self.read_head = 0;
        self.allocate = 0;
        self.reclaim = 0;
        self.epoch += 1;
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bad {
    None,
    EarlyTransfer,
    TransferReleasesRow,
    StaleReturn,
    ResetPending,
    HeadOverwrite,
    FutureConsume,
}
#[derive(Clone, Copy)]
struct Config {
    stress: bool,
    partial_fault: bool,
    fault_at: Option<u64>,
    bad: Bad,
    sudden: bool,
}
fn run(
    words: &[i128],
    stress: bool,
    partial_fault: bool,
    fault_at: Option<u64>,
    bad: Bad,
    out: &mut impl Write,
) -> Result<Stats, String> {
    run_pool::<1>(
        words,
        Config {
            stress,
            partial_fault,
            fault_at,
            bad,
            sudden: false,
        },
        out,
    )
}
fn run_pool<const HEADS: usize>(
    words: &[i128],
    cfg: Config,
    out: &mut impl Write,
) -> Result<Stats, String> {
    assert!((1..=2).contains(&HEADS));
    let Config {
        stress,
        partial_fault,
        fault_at,
        bad,
        sudden,
    } = cfg;
    let mut pool = Pool::<HEADS>::default();
    let mut s = Stats::default();
    let mut next = 0;
    let mut writes = VecDeque::<Ref>::new();
    let mut terminal = false;
    let mut injected = false;
    // A real accepted serial MC burst continues under CE=0 and terminal fault.
    // It is a fault-drain fixture, not a second synthetic refill latency model.
    let mut mc = physical::Physical::new(u64::from(support::BASE), vec![0x6d; 128], true, false);
    let mut id = if partial_fault && HEADS == 1 {
        Some(mc.submit_read(u64::from(support::BASE), 128)?)
    } else {
        None
    };
    let mut mc_live = id.is_some();
    while next < words.len() || !pool.producer.is_empty() || pool.group_count() != 0 {
        if s.wall > 200_000 {
            return Err("packet transport watchdog".into());
        }
        s.wall += 1;
        if partial_fault && HEADS == 2 && s.wall == 64 {
            id = Some(mc.submit_read(u64::from(support::BASE), 128)?);
            mc_live = true;
        }
        let ce = stress && s.wall % 23 <= 7;
        for response in mc.step()? {
            match response {
                RefillEvent::Beat {
                    id: rid,
                    index,
                    data,
                    last,
                } if Some(rid) == id => {
                    if index != s.accepted_beats
                        || data != 0x6d6d_6d6d_6d6d_6d6d
                        || last != (index == 15)
                    {
                        return Err("accepted MC drain beat order/data/last".into());
                    }
                    if terminal {
                        s.post_fault_beats += 1;
                        s.post_fault_beats_under_ce += usize::from(ce);
                    }
                    s.accepted_beats += 1;
                    if ce {
                        s.drain_beats_under_ce += 1;
                    }
                    if index == 3 {
                        terminal = true;
                        s.fault_edge = Some(s.enabled);
                        s.fault_with_two_heads = pool.valid_heads() == 2;
                    }
                    if HEADS == 2 {
                        writeln!(
                            out,
                            "{stress},{},MC beat{index} CE{} terminal{terminal},{},{index},0,{}",
                            s.enabled,
                            u8::from(ce),
                            pool.epoch,
                            pool.group_count()
                        )
                        .unwrap();
                    }
                }
                RefillEvent::Complete { id: rid } if Some(rid) == id => mc_live = false,
                _ => {}
            }
        }
        if ce {
            s.ce_edges += 1;
            s.ce_with_pending += usize::from(pool.pending.is_some());
            continue; // SDP CE/OCE and its reserved reply freeze together.
        }
        let t = s.enabled;
        s.enabled += 1;
        let previous_groups = pool.group_count();
        if fault_at == Some(t) {
            terminal = true;
            s.fault_edge = Some(t);
        }
        let ready = if sudden {
            !(100..340).contains(&t) && t % 29 > 2
        } else {
            !stress || (t > 240 && t % 19 > 4 && t % 97 > 13)
        };
        let (mut consumed_here, mut captured_here, mut read_here) = (false, false, false);
        // Only the previous edge's valid skid is visible to the consumer.
        if !terminal && ready {
            let target = pool.pop_head;
            if let Some((r, value)) = pool.heads[target].take() {
                if value != words[r.serial] || r.serial != s.consumed || r.edge + 1 >= t {
                    return Err("packet cache consumer owner/value/order".into());
                }
                pool.pop_head = (pool.pop_head + 1) % HEADS;
                consumed_here = true;
                s.consumed += 1;
                s.consume_edges.push(t);
                writeln!(
                    out,
                    "{stress},{t},{},{},{},{},{}",
                    if HEADS == 1 {
                        "consume".into()
                    } else {
                        format!("consume head{target}")
                    },
                    pool.epoch,
                    r.serial,
                    r.row,
                    pool.group_count()
                )
                .unwrap();
            }
        }
        let mut read_row = None;
        let mut write_row = None;
        if let Some((r, mut data, target)) = pool.pending.take() {
            if bad == Bad::ResetPending && !injected {
                pool.pending = Some((r, data, target));
                return pool.reset().map(|()| s);
            }
            if bad == Bad::StaleReturn && !injected {
                data[0].epoch = pool.epoch + 1; // Equal numerical payload, wrong owner.
                injected = true;
            }
            if r.edge + 1 != t || pool.heads[target].is_some() || r.serial != pool.reclaim {
                return Err("packet return reservation/order".into());
            }
            for (bank, value) in data.iter().enumerate() {
                if value.epoch != pool.epoch
                    || value.serial != r.serial
                    || pool.banks[bank][r.row] != Some(*value)
                {
                    return Err("packet synchronous reply owner/value/epoch".into());
                }
            }
            let word = data[0].bits as i128 | (data[1].bits as i128) << 36;
            if word != words[r.serial] {
                return Err("packet 72bit capture".into());
            }
            if bad == Bad::FutureConsume && !injected {
                // Deliberately offer the newly captured value on this edge.
                // The consumer contract permits only capture on an older edge.
                if r.edge + 1 >= t {
                    return Err("packet consumer attempted return-edge bypass".into());
                }
            }
            pool.heads[target] = Some((r, word));
            captured_here = true;
            if !ready {
                s.captures_while_stopped += 1;
                if sudden && (100..340).contains(&t) {
                    s.sudden_return_capture += 1;
                }
            }
            for bank in &mut pool.banks {
                bank[r.row] = None;
            }
            pool.reclaim += 1;
            s.captures += 1;
            writeln!(
                out,
                "{stress},{t},{},{},{},{},{}",
                if HEADS == 1 {
                    "capture/free-row".into()
                } else {
                    format!("capture/free-row head{target}")
                },
                pool.epoch,
                r.serial,
                r.row,
                pool.group_count()
            )
            .unwrap();
        }
        if let Some(r) = writes.pop_front_if(|r| r.edge == t) {
            write_row = Some(r.row);
            let p = pool
                .producer
                .iter_mut()
                .find(|p| p.serial == r.serial)
                .ok_or("missing packet W owner")?;
            if p.edge != u64::MAX {
                return Err("packet W repeated".into());
            }
            p.edge = t;
            for (bank, shift) in [0, 36].into_iter().enumerate() {
                if pool.banks[bank][r.row].is_some() {
                    return Err("packet row overwrite".into());
                }
                pool.banks[bank][r.row] = Some(Row {
                    epoch: pool.epoch,
                    serial: r.serial,
                    bits: ((words[r.serial] >> shift) & ((1_i128 << 36) - 1)) as u64,
                });
            }
            s.writes += 1;
            writeln!(
                out,
                "{stress},{t},W,{},{},{},{}",
                pool.epoch,
                r.serial,
                r.row,
                pool.group_count()
            )
            .unwrap();
        }
        if !terminal {
            // Readiness uses the pre-transfer Group occupancy; no full32 bypass.
            if previous_groups == 32
                && consumed_here
                && pool.producer.front().is_some_and(|r| r.edge < t)
            {
                s.full_g_consume_without_transfer += 1;
            }
            if previous_groups < 32
                && pool
                    .producer
                    .front()
                    .is_some_and(|r| r.edge < t || bad == Bad::EarlyTransfer)
            {
                let mut r = pool.producer.pop_front().unwrap();
                if r.edge >= t {
                    return Err("packet transfer before payload W publication".into());
                }
                if bad == Bad::TransferReleasesRow && !injected {
                    for bank in &mut pool.banks {
                        bank[r.row] = None;
                    }
                    injected = true;
                }
                r.edge = t;
                pool.groups.push_back(r);
                writeln!(
                    out,
                    "{stress},{t},index transfer,{},{},{},{}",
                    pool.epoch,
                    r.serial,
                    r.row,
                    pool.group_count()
                )
                .unwrap();
            }
            // Consume, synchronous capture and R reservation have different
            // owned locations. The next target is never an occupied location.
            let target = if bad == Bad::HeadOverwrite {
                pool.pop_head
            } else {
                pool.read_head
            };
            if (pool.heads[target].is_none() || bad == Bad::HeadOverwrite)
                && pool.pending.is_none()
                && pool.valid_heads() < HEADS
                && pool.groups.front().is_some_and(|r| r.edge < t)
            {
                if pool.heads[target].is_some() {
                    return Err("packet R destination already owned".into());
                }
                let mut r = pool.groups.pop_front().unwrap();
                read_row = Some(r.row);
                let data = [
                    pool.banks[0][r.row].ok_or("packet R absent low")?,
                    pool.banks[1][r.row].ok_or("packet R absent high")?,
                ];
                if data
                    .iter()
                    .any(|w| w.epoch != pool.epoch || w.serial != r.serial)
                {
                    return Err("packet read owner".into());
                }
                r.edge = t;
                pool.pending = Some((r, data, target));
                pool.read_head = (pool.read_head + 1) % HEADS;
                read_here = true;
                s.reads += 1;
                writeln!(
                    out,
                    "{stress},{t},{},{},{},{},{}",
                    if HEADS == 1 {
                        "R reserve".into()
                    } else {
                        format!("R reserve head{target}")
                    },
                    pool.epoch,
                    r.serial,
                    r.row,
                    pool.group_count()
                )
                .unwrap();
            }
            if next < words.len() && pool.producer.len() < 16 {
                let row = pool.allocate & 63;
                if pool.banks.iter().any(|b| b[row].is_some()) || pool.allocate - pool.reclaim >= 64
                {
                    return Err("packet ring allocation before reclaim".into());
                }
                let r = Ref {
                    serial: next,
                    row,
                    edge: u64::MAX,
                };
                pool.producer.push_back(r);
                writes.push_back(Ref { edge: t + 8, ..r });
                next += 1;
                pool.allocate += 1;
                s.allocations += 1;
                s.wraps = pool.allocate / 64;
            }
        }
        if read_row.is_some() && read_row == write_row {
            return Err("packet same-row R/W".into());
        }
        let (p, g, rows) = (
            pool.producer.len(),
            pool.group_count(),
            pool.allocate - pool.reclaim,
        );
        let head_reservations = pool.valid_heads() + usize::from(pool.pending.is_some());
        if p > 16
            || g > 32
            || rows > 48
            || rows != p + g - pool.valid_heads()
            || head_reservations > HEADS
        {
            return Err("packet physical rows/two credit domains".into());
        }
        s.peak_p = s.peak_p.max(p);
        s.peak_g = s.peak_g.max(g);
        s.peak_rows = s.peak_rows.max(rows);
        s.peak_heads = s.peak_heads.max(pool.valid_heads());
        s.peak_head_reservations = s.peak_head_reservations.max(head_reservations);
        s.triple_overlap += usize::from(consumed_here && captured_here && read_here);
        if terminal && writes.is_empty() && pool.pending.is_none() && !mc_live {
            pool.reset()?;
            s.reset_edge = Some(t);
            // Recreation starts only after all old local returns/writes and the
            // accepted MC burst have drained. A new epoch checks old data cannot
            // survive even when row0 and the numerical word are reused.
            let r = Row {
                epoch: pool.epoch,
                serial: 0,
                bits: 7,
            };
            pool.banks[0][0] = Some(r);
            pool.banks[1][0] = Some(Row { bits: 0, ..r });
            assert!(pool.banks[0][0].is_some_and(|w| w.epoch == pool.epoch));
            pool.banks = [[None; 64]; 2];
            return Ok(s);
        }
    }
    if s.captures != words.len()
        || s.reads != words.len()
        || s.writes != words.len()
        || s.consumed != words.len()
        || pool.allocate != pool.reclaim
    {
        return Err("packet transport incomplete drain".into());
    }
    Ok(s)
}
// The original driver reproduces the single-head baseline; the increment driver
// calls this entry point instead.
#[allow(dead_code)]
pub fn probe_increment(root: &Path) {
    let qs = baseline::inputs("seams", 15, 32);
    let words: Vec<_> = qs
        .iter()
        .flat_map(|q| {
            bound::prepare(q, &[support::slot(9, true)])
                .unwrap()
                .payloads
        })
        .collect();
    let mut summary = fs::File::create(root.join("packet_two_head_summary.csv")).unwrap();
    writeln!(summary,"case,wall,enabled,allocated,captured,consumed,peak_P,peak_G,peak_rows,wraps,peak_heads,peak_head_reservations,sampled_packet_interval,triple_overlap,capture_while_stopped,sudden_return_capture,CE_edges,CE_with_pending,MC_beats,MC_beats_under_CE,fault_edge,reset_edge,fault_with_two_heads").unwrap();
    let mut audit = fs::File::create(root.join("packet_two_head_audit.csv")).unwrap();
    writeln!(audit,"case,sampled_interval_begin_index,sampled_interval_end_index,full_G_consume_without_transfer,post_fault_beats,post_fault_beats_under_CE").unwrap();
    for (case, stress, partial_fault, fault_at, sudden) in [
        ("hot", false, false, None, false),
        ("CE-long-miss", true, false, None, false),
        ("CE-sudden-stop", true, false, None, true),
        ("fault-at-return", false, false, Some(12), false),
        ("partial-MC-fault", true, true, None, false),
    ] {
        let mut edges = fs::File::create(root.join(format!("packet_two_head_{case}.csv"))).unwrap();
        writeln!(edges, "stress,enabled,event,epoch,serial,row,group_credits").unwrap();
        let s = run_pool::<2>(
            &words,
            Config {
                stress,
                partial_fault,
                fault_at,
                bad: Bad::None,
                sudden,
            },
            &mut edges,
        )
        .unwrap();
        let interval = if s.consume_edges.len() > 96 {
            (s.consume_edges[96] - s.consume_edges[32]) as f64 / 64.0
        } else {
            0.0
        };
        writeln!(
            summary,
            "{case},{},{},{},{},{},{},{},{},{},{},{},{interval},{},{},{},{},{},{},{},{:?},{:?},{}",
            s.wall,
            s.enabled,
            s.allocations,
            s.captures,
            s.consumed,
            s.peak_p,
            s.peak_g,
            s.peak_rows,
            s.wraps,
            s.peak_heads,
            s.peak_head_reservations,
            s.triple_overlap,
            s.captures_while_stopped,
            s.sudden_return_capture,
            s.ce_edges,
            s.ce_with_pending,
            s.accepted_beats,
            s.drain_beats_under_ce,
            s.fault_edge,
            s.reset_edge,
            s.fault_with_two_heads
        )
        .unwrap();
        writeln!(
            audit,
            "{case},32,96,{},{},{}",
            s.full_g_consume_without_transfer, s.post_fault_beats, s.post_fault_beats_under_ce
        )
        .unwrap();
        if case == "hot" {
            assert_eq!(interval, 1.0);
            assert_eq!(&s.consume_edges[..3], &[12, 13, 14]);
            assert!(s.triple_overlap >= words.len() - 2);
        }
        if !partial_fault && fault_at.is_none() {
            assert_eq!(
                (s.allocations, s.captures, s.consumed),
                (words.len(), words.len(), words.len())
            );
            assert_eq!(s.wraps, words.len() / 64);
        }
        if stress && !partial_fault {
            assert_eq!(
                (s.peak_p, s.peak_g, s.peak_heads, s.peak_head_reservations),
                (16, 32, 2, 2)
            );
            assert!(s.ce_edges > 0 && s.ce_with_pending > 0 && s.captures_while_stopped > 0);
            assert!(s.full_g_consume_without_transfer > 0);
        }
        if sudden {
            assert!(s.sudden_return_capture > 0);
        }
        if fault_at.is_some() {
            assert_eq!(s.consumed, 0); // Edge12 would consume head0; fault suppresses it.
            assert_eq!(s.captures, 2); // Reserved head1 return still captured on edge12.
        }
        if partial_fault {
            assert_eq!(s.accepted_beats, 16);
            assert!(s.fault_with_two_heads && s.drain_beats_under_ce > 0);
            assert_eq!(s.post_fault_beats, 12);
            assert!(s.post_fault_beats_under_ce > 0);
        }
        if partial_fault || fault_at.is_some() {
            assert!(s.reset_edge.unwrap() > s.fault_edge.unwrap());
        }
        println!("packet two-head {case}: {} enabled P{} G{} rows{} heads{} wraps{} II={interval} overlaps{}",
            s.enabled,s.peak_p,s.peak_g,s.peak_rows,s.peak_head_reservations,s.wraps,s.triple_overlap);
    }
    let mut negative = fs::File::create(root.join("packet_two_head_negatives.csv")).unwrap();
    writeln!(negative, "injection,detected_error").unwrap();
    for (bad, name, expected) in [
        (
            Bad::EarlyTransfer,
            "early index publication",
            "packet transfer before payload W publication",
        ),
        (
            Bad::TransferReleasesRow,
            "index transfer frees row",
            "packet R absent low",
        ),
        (
            Bad::StaleReturn,
            "equal-value stale epoch",
            "packet synchronous reply owner/value/epoch",
        ),
        (
            Bad::ResetPending,
            "reset before reserved return",
            "pool reset with outstanding local owner",
        ),
        (
            Bad::HeadOverwrite,
            "read reserves occupied head",
            "packet R destination already owned",
        ),
        (
            Bad::FutureConsume,
            "consume on return edge",
            "packet consumer attempted return-edge bypass",
        ),
    ] {
        let err = run_pool::<2>(
            &words[..32],
            Config {
                stress: false,
                partial_fault: false,
                fault_at: None,
                bad,
                sudden: false,
            },
            &mut std::io::sink(),
        )
        .err()
        .expect("packet negative must execute and fail");
        assert_eq!(err, expected);
        writeln!(negative, "{name},{err}").unwrap();
    }
    let mut cost = fs::File::create(root.join("packet_two_head_cost.csv")).unwrap();
    writeln!(cost, "field,extra_FF,bit_mux_nodes,scope").unwrap();
    for (name, bits) in [
        ("second owned packet payload", 72),
        ("second head valid", 1),
        ("consumer head pointer", 1),
        ("read reservation head pointer", 1),
        ("pending return target", 1),
    ] {
        writeln!(
            cost,
            "{name},{bits},0,one old head retained; pending data stays in paid BSRAM output"
        )
        .unwrap();
    }
    writeln!(
        cost,
        "consumer payload selection,0,72,two 72bit owned heads; one consumer"
    )
    .unwrap();
    writeln!(
        cost,
        "consumer/read target valid selection,0,2,two independent one-bit selectors"
    )
    .unwrap();
    writeln!(
        cost,
        "TOTAL,76,74,no additional RAM16/BSRAM/DSP; not fitted Logic"
    )
    .unwrap();
    let mut controls = fs::File::create(root.join("packet_two_head_controls.csv")).unwrap();
    writeln!(controls, "state,width,transition,scope").unwrap();
    for (name, width, transition) in [
        (
            "head valid",
            2,
            "clear selected old head on consume; set reserved target on full capture",
        ),
        ("pending valid", 1, "clear on capture; set on R reservation"),
        (
            "read/pop pointer",
            1,
            "toggle only on respective accepted R/consume",
        ),
        (
            "pending target",
            1,
            "capture read pointer on R; gate one destination's 72 data enables",
        ),
        ("return target decode",1,"one inverted target plus two return-valid AND target enables; each drives 72 data bits"),
        (
            "producer count",
            5,
            "allocate increments; published index transfer decrements; limit16",
        ),
        (
            "Group count",
            6,
            "transfer increments only if pre-edge count below32; consume decrements",
        ),
        (
            "row count",
            6,
            "allocate increments; full capture decrements; upperbound48",
        ),
    ] {
        writeln!(controls,"{name},{width},{transition},counter compare/valid decode/enable logic requires lowering; no LUT-free claim").unwrap();
    }
}
pub fn probe(root: &Path) {
    let qs = baseline::inputs("seams", 15, 32);
    let mut words = vec![];
    for q in &qs {
        words.extend(
            bound::prepare(q, &[support::slot(9, true)])
                .unwrap()
                .payloads,
        );
    }
    let mut edges = fs::File::create(root.join("packet_edges.csv")).unwrap();
    writeln!(edges, "stress,enabled,event,epoch,serial,row,group_credits").unwrap();
    let mut summary = fs::File::create(root.join("packet_summary.csv")).unwrap();
    writeln!(summary,"case,wall,enabled,allocated,captured,consumed,peak_producer,peak_group,peak_rows,wraps,steady_packet_interval,fault_edge,reset_edge,MC_beats").unwrap();
    for (case, stress, fault, edge) in [
        ("hot", false, false, None),
        ("CE-long-miss", true, false, None),
        ("fault-at-return", false, false, Some(11)),
        ("partial-MC-fault", true, true, None),
    ] {
        let s = run(&words, stress, fault, edge, Bad::None, &mut edges).unwrap();
        let interval = if s.consume_edges.len() > 96 {
            (s.consume_edges[96] - s.consume_edges[32]) as f64 / 64.0
        } else {
            0.0
        };
        writeln!(
            summary,
            "{case},{},{},{},{},{},{},{},{},{},{interval},{:?},{:?},{}",
            s.wall,
            s.enabled,
            s.allocations,
            s.captures,
            s.consumed,
            s.peak_p,
            s.peak_g,
            s.peak_rows,
            s.wraps,
            s.fault_edge,
            s.reset_edge,
            s.accepted_beats
        )
        .unwrap();
        if !stress && !fault && edge.is_none() {
            assert_eq!(interval, 2.0);
            // Minimal obstruction: R10, capture11, consume12; R12,
            // capture13, consume14. No return->consumer bypass is implied.
            assert_eq!(&s.consume_edges[..3], &[12, 14, 16]);
        }
        if fault {
            assert_eq!(s.accepted_beats, 16);
            assert!(s.reset_edge.unwrap() >= s.fault_edge.unwrap());
        }
        if stress && !fault {
            assert_eq!((s.peak_p, s.peak_g, s.peak_rows), (16, 32, 47));
        }
        println!(
            "packet {case}: {} enabled, P{} G{} rows{} wraps{} interval={interval}",
            s.enabled, s.peak_p, s.peak_g, s.peak_rows, s.wraps
        );
    }
    for bad in [
        Bad::EarlyTransfer,
        Bad::TransferReleasesRow,
        Bad::StaleReturn,
        Bad::ResetPending,
    ] {
        assert!(
            run(&words[..32], false, false, None, bad, &mut std::io::sink()).is_err(),
            "packet injected negative case must fail"
        );
    }
}
