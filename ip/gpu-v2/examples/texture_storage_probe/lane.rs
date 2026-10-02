//! One16-row lane ring. A row is reserved before each wrapped R, not per quad.
//! Geometry W is even; operand R is odd; weights W is odd; member R is even.
//! Arithmetic values are goldens. The finite storage and capture calendar is real.
use super::*;
#[derive(Clone, Copy)]
struct Word {
    serial: usize,
    data: u128,
}
struct Ref {
    serial: usize,
    row: usize,
    owner: usize,
    lane: usize,
    source: Arc<bound::Program>,
    edge: u64,
}
impl Clone for Ref {
    fn clone(&self) -> Self {
        Self {
            serial: self.serial,
            row: self.row,
            owner: self.owner,
            lane: self.lane,
            source: self.source.clone(),
            edge: self.edge,
        }
    }
}
struct Head {
    record: Ref,
    geometry: u128,
    weights: u128,
    next: usize,
}
fn value(f: &audited::FrameReport, name: &str) -> u128 {
    f.outputs.iter().find(|o| o.name == name).unwrap().raw as u128
}
fn input_value(f: &audited::FrameReport, name: &str, row: usize, width: u32) -> u128 {
    f.events
        .iter()
        .find_map(|e| match e.operation {
            audited::Operation::Read { memory, row: r }
                if f.memories[memory].name == name && r == row =>
            {
                Some(f.values[e.output.unwrap()].raw as u128 & ((1_u128 << width) - 1))
            }
            _ => None,
        })
        .unwrap()
}
fn lane_data(p: &bound::Program, lane: usize) -> &bound::Lane {
    p.preparation()
        .lanes
        .iter()
        .find(|l| usize::from(l.lane) == lane)
        .unwrap()
}
fn geometry(p: &bound::Program, lane: usize) -> u128 {
    let l = lane_data(p, lane);
    let lod = &p.preparation().lod.frame;
    let mut word = 0;
    let mut bit = 0;
    for w in 0..2 {
        for a in 0..2 {
            for tap in 0..2 {
                let v = value(&l.coordinate.frame, &format!("t{w}.{a}.{tap}"));
                assert!(v < 1024);
                word |= v << bit;
                bit += 10;
            }
        }
    }
    for (v, width) in [
        (value(lod, "n0"), 4),
        (value(lod, "n1"), 4),
        (value(lod, "slot"), 4),
        (value(lod, "quad"), 4),
        (lane as u128, 2),
        (value(lod, "last_fine"), 1),
    ] {
        word |= v << bit;
        bit += width;
    }
    assert_eq!(bit, 99);
    word
}
fn operands(p: &bound::Program, lane: usize) -> u128 {
    let l = lane_data(p, lane);
    let lod = &p.preparation().lod.frame;
    let mut word = value(lod, "parent0") | (value(lod, "parent1") << 9);
    let mut bit = 18;
    for w in 0..2 {
        for a in 0..2 {
            word |= value(&l.coordinate.frame, &format!("f{w}.{a}")) << bit;
            bit += 8;
        }
    }
    word |= value(lod, "nearest") << bit;
    bit += 1;
    assert_eq!(bit, 51);
    word
}
fn weights(p: &bound::Program, lane: usize) -> u128 {
    let l = lane_data(p, lane);
    let mut word = 0;
    for w in 0..2 {
        for tap in 0..4 {
            word |= value(&l.coefficient.frame, &format!("w{w}.{tap}")) << (9 * (w * 4 + tap));
        }
    }
    word
}
#[derive(Default)]
pub struct Stats {
    pub reserved: usize,
    pub captured: usize,
    pub returned: usize,
    pub peak_rows: usize,
    pub peak_coord: usize,
    pub peak_coeff: usize,
    pub peak_ready: usize,
    pub peak_work: usize,
    pub stalls: usize,
    pub pause_with_older_progress: usize,
    pub first_work_block: Option<(u64, usize, usize)>,
    pub first_source_block: Option<(u64, usize, usize, usize, usize)>,
}
pub struct Controller {
    geometry: [Option<Word>; 16],
    operand: [Option<Word>; 16],
    // Before the operand R, work admission needs the actual captured LOD bit.
    // Geometry has only the member reader; these paid flags avoid a hidden R.
    last_fine: [bool; 16],
    live: [Option<Ref>; 16],
    allocate: usize,
    reclaim: usize,
    coord_count: usize,
    coordinate: VecDeque<Ref>,
    coord_ready: VecDeque<Ref>,
    pending_coeff: Option<(Ref, Word)>,
    coefficient: VecDeque<Ref>,
    written: VecDeque<Ref>,
    ready: VecDeque<Ref>,
    pending_head: Option<(Ref, Word, Word)>,
    head: Option<Head>,
    // Bounded external member endpoint. Its completed token handshake is the
    // work-credit acknowledgement; no plane/packet RAM adapter is introduced.
    members: VecDeque<Ref>,
    work_count: usize,
    next_tick: u64,
    last_progress: bool,
    pub stats: Stats,
    inject_stale: bool,
    injected: bool,
}
impl Controller {
    fn new(inject_stale: bool) -> Self {
        Self {
            geometry: [None; 16],
            operand: [None; 16],
            last_fine: [false; 16],
            live: std::array::from_fn(|_| None),
            allocate: 0,
            reclaim: 0,
            coord_count: 0,
            coordinate: VecDeque::new(),
            coord_ready: VecDeque::new(),
            pending_coeff: None,
            coefficient: VecDeque::new(),
            written: VecDeque::new(),
            ready: VecDeque::new(),
            pending_head: None,
            head: None,
            members: VecDeque::new(),
            work_count: 0,
            next_tick: 0,
            last_progress: false,
            stats: Stats::default(),
            inject_stale,
            injected: false,
        }
    }
    fn coeff_count(&self) -> usize {
        self.coefficient.len() + self.written.len() + usize::from(self.pending_coeff.is_some())
    }
    fn ready_count(&self) -> usize {
        self.ready.len()
            + usize::from(self.pending_head.is_some())
            + usize::from(self.head.is_some())
    }
    fn audit(&mut self) -> Result<(), String> {
        let rows = self.allocate - self.reclaim;
        if rows > 16
            || self.coord_count > 6
            || self.coeff_count() > 6
            || self.ready_count() > 2
            || self.work_count > 16
            || rows
                != self.coord_count + self.coeff_count() + self.ready_count()
                    - usize::from(self.head.is_some())
        {
            return Err("lane row/coordinate/coefficient/ready/work credit ownership".into());
        }
        self.stats.peak_rows = self.stats.peak_rows.max(rows);
        self.stats.peak_coord = self.stats.peak_coord.max(self.coord_count);
        self.stats.peak_coeff = self.stats.peak_coeff.max(self.coeff_count());
        self.stats.peak_ready = self.stats.peak_ready.max(self.ready_count());
        self.stats.peak_work = self.stats.peak_work.max(self.work_count);
        Ok(())
    }
}
impl raw::LaneSink for Controller {
    fn live(&self) -> usize {
        self.allocate - self.reclaim
    }
    fn idle(&self) -> bool {
        self.live() == 0 && self.head.is_none() && self.members.is_empty() && self.work_count == 0
    }
    fn tick(&mut self, t: u64, ready: bool, out: &mut dyn Write) -> Result<(), String> {
        if t != self.next_tick {
            return Err("lane enabled clock ordering".into());
        }
        self.next_tick += 1;
        self.last_progress = false;
        let (mut geometry_r, mut geometry_w, mut operand_r, mut operand_w) =
            (None, None, None, None);
        // Existing finite member destination: accepted work before coefficient
        // issue protects all fixed member operations. Consumer stalls stop new
        // issues, never old arithmetic. Credit returns only on an actual ACK.
        if ready && self.members.front().is_some_and(|r| r.edge < t) {
            let r = self.members.pop_front().unwrap();
            self.work_count -= 1;
            self.last_progress = true;
            writeln!(
                out,
                "lane,{t},external member ACK,{},{},{},{}",
                r.owner, r.lane, r.row, r.serial
            )
            .unwrap();
        }
        // Only the previous edge's owned head may feed the member input.
        if let Some(h) = self.head.as_mut() {
            let l = lane_data(&h.record.source, h.record.lane);
            let planes = if h.geometry & (1 << 98) != 0 { 1 } else { 2 };
            if planes != l.memberships.len() {
                return Err("member plane count from captured head flag".into());
            }
            if h.geometry != geometry(&h.record.source, h.record.lane)
                || h.weights != weights(&h.record.source, h.record.lane)
            {
                return Err("member head owner/value".into());
            }
            let frame = &l.memberships[h.next].frame;
            for tap in 0..4 {
                if input_value(frame, "weights", tap, 9)
                    != ((h.weights >> (36 * h.next + 9 * tap)) & 511)
                    || input_value(frame, "coords", tap, 10)
                        != ((h.geometry >> (40 * h.next + 10 * tap)) & 1023)
                {
                    return Err("member actual head fields differ from closed inputs".into());
                }
            }
            for (row, v) in [
                (0, (h.geometry >> 88) & 15),
                (1, (h.geometry >> (80 + 4 * h.next)) & 15),
                (2, (h.geometry >> 92) & 15),
            ] {
                if input_value(frame, "identity", row, 4) != v {
                    return Err("member head identity fields".into());
                }
            }
            let mut r = h.record.clone();
            r.edge = t + 7;
            h.next += 1;
            self.members.push_back(r.clone());
            writeln!(
                out,
                "lane,{t},member capture,{},{},{},{}",
                r.owner, r.lane, r.row, r.serial
            )
            .unwrap();
            if h.next == planes {
                self.head = None;
            }
            self.last_progress = true;
        }
        if let Some((mut r, g, w)) = self.pending_head.take() {
            if r.edge + 1 != t
                || self.head.is_some()
                || r.serial != self.reclaim
                || g.serial != r.serial
                || w.serial != r.serial
            {
                return Err("lane head return reservation/owner/order".into());
            }
            if self.geometry[r.row].is_none_or(|old| old.serial != r.serial || old.data != g.data)
                || self.operand[r.row]
                    .is_none_or(|old| old.serial != r.serial || old.data != w.data)
            {
                return Err("lane RAM replaced before capture".into());
            }
            self.geometry[r.row] = None;
            self.operand[r.row] = None;
            self.live[r.row] = None;
            self.reclaim += 1;
            self.stats.returned += 1;
            r.edge = t;
            writeln!(
                out,
                "lane,{t},head capture/free row,{},{},{},{}",
                r.owner, r.lane, r.row, r.serial
            )
            .unwrap();
            self.head = Some(Head {
                record: r,
                geometry: g.data,
                weights: w.data,
                next: 0,
            });
            self.last_progress = true;
        }
        if let Some((mut r, w)) = self.pending_coeff.take() {
            if r.edge + 1 != t
                || !t.is_multiple_of(2)
                || w.serial != r.serial
                || w.data != operands(&r.source, r.lane)
            {
                return Err("coefficient return/phase/value".into());
            }
            r.edge = t;
            self.coefficient.push_back(r.clone());
            writeln!(
                out,
                "lane,{t},coefficient input capture,{},{},{},{}",
                r.owner, r.lane, r.row, r.serial
            )
            .unwrap();
        }
        if let Some(mut r) = self.coordinate.pop_front_if(|r| r.edge + 10 == t) {
            geometry_w = Some(r.row);
            operand_w = Some(r.row);
            if self.geometry[r.row].is_some() || self.operand[r.row].is_some() {
                return Err("coordinate overwrite live lane row".into());
            }
            let g = geometry(&r.source, r.lane);
            if (g & (1 << 98) != 0) != self.last_fine[r.row] {
                return Err("geometry last-plane bit differs from actual captured LOD".into());
            }
            self.geometry[r.row] = Some(Word {
                serial: r.serial,
                data: g,
            });
            self.operand[r.row] = Some(Word {
                serial: r.serial,
                data: operands(&r.source, r.lane),
            });
            r.edge = t;
            self.coord_ready.push_back(r.clone());
            writeln!(
                out,
                "lane,{t},geometry/operand W,{},{},{},{}",
                r.owner, r.lane, r.row, r.serial
            )
            .unwrap();
        }
        if let Some(mut r) = self.coefficient.pop_front_if(|r| r.edge + 11 == t) {
            if operand_w.replace(r.row).is_some() {
                return Err("lane operand W port collision".into());
            }
            if self.operand[r.row].is_none_or(|old| old.serial != r.serial) {
                return Err("weights W wrong row owner".into());
            }
            self.operand[r.row] = Some(Word {
                serial: r.serial,
                data: weights(&r.source, r.lane),
            });
            r.edge = t;
            self.written.push_back(r.clone());
            writeln!(
                out,
                "lane,{t},weights W,{},{},{},{}",
                r.owner, r.lane, r.row, r.serial
            )
            .unwrap();
        }
        // Publish only a completed stored row, and keep all head/pending
        // locations inside the original two coefficient-ready credits.
        if self.ready_count() < 2 {
            if let Some(r) = self.written.pop_front() {
                self.ready.push_back(r);
            }
        }
        if t.is_multiple_of(2)
            && self.pending_head.is_none()
            && self.ready.front().is_some_and(|r| r.edge < t)
        {
            let mut r = self.ready.pop_front().unwrap();
            geometry_r = Some(r.row);
            operand_r = Some(r.row);
            let g = self.geometry[r.row].ok_or("member geometry R absent")?;
            let mut w = self.operand[r.row].ok_or("member weights R absent")?;
            if self.inject_stale && !self.injected {
                w.serial += 1;
                self.injected = true;
            }
            r.edge = t;
            self.pending_head = Some((r.clone(), g, w));
            writeln!(
                out,
                "lane,{t},member geometry/weights R,{},{},{},{}",
                r.owner, r.lane, r.row, r.serial
            )
            .unwrap();
        }
        if t % 2 == 1 && self.pending_coeff.is_none() && self.coeff_count() < 6 {
            if let Some(r) = self.coord_ready.front() {
                let cost = if self.last_fine[r.row] { 1 } else { 2 };
                if cost != lane_data(&r.source, r.lane).memberships.len() {
                    return Err(
                        "coefficient work reservation from actual captured plane flag".into(),
                    );
                }
                if r.edge < t && self.work_count + cost <= 16 {
                    let mut r = self.coord_ready.pop_front().unwrap();
                    operand_r = Some(r.row);
                    let w = self.operand[r.row].ok_or("coefficient operand R absent")?;
                    if w.serial != r.serial || w.data != operands(&r.source, r.lane) {
                        return Err("coefficient operand R owner/value".into());
                    }
                    // Actual accepted R reserves coefficient return/phase/work.
                    // The coordinate descriptor is consumed here, while its
                    // physical geometry row stays owned until the later head.
                    self.coord_count -= 1;
                    self.work_count += cost;
                    r.edge = t;
                    self.pending_coeff = Some((r.clone(), w));
                    writeln!(
                        out,
                        "lane,{t},coefficient operand R/free coordinate credit,{},{},{},{}",
                        r.owner, r.lane, r.row, r.serial
                    )
                    .unwrap();
                } else if r.edge < t && self.work_count + cost > 16 {
                    self.stats
                        .first_work_block
                        .get_or_insert((t, self.work_count, cost));
                    writeln!(
                        out,
                        "lane,{t},coefficient work credit stall,{},{},{},{}",
                        r.owner, r.lane, r.row, r.serial
                    )
                    .unwrap();
                }
            }
        }
        for (read, write, name) in [
            (geometry_r, geometry_w, "geometry"),
            (operand_r, operand_w, "operand/weights"),
        ] {
            if read.is_some() && read == write {
                return Err(format!("lane {name} same-row R/W"));
            }
        }
        self.audit()
    }
    fn reserve(
        &mut self,
        t: u64,
        owner: usize,
        lane: usize,
        p: Arc<bound::Program>,
        out: &mut dyn Write,
    ) -> Result<bool, String> {
        if t % 2 != 1 {
            return Err("wrapped R must protect next even arithmetic phase".into());
        }
        if self.coord_count == 6 || self.live() == 16 {
            self.stats.stalls += 1;
            if self.last_progress {
                self.stats.pause_with_older_progress += 1;
            }
            let block = (
                t,
                self.live(),
                self.coord_count,
                self.coeff_count(),
                self.work_count,
            );
            self.stats.first_source_block.get_or_insert(block);
            return Ok(false);
        }
        let serial = self.allocate;
        let row = serial & 15;
        if self.live[row].is_some() {
            return Err("lane allocation before head capture".into());
        }
        self.live[row] = Some(Ref {
            serial,
            row,
            owner,
            lane,
            source: p,
            edge: t,
        });
        self.allocate += 1;
        self.coord_count += 1;
        self.stats.reserved += 1;
        writeln!(
            out,
            "lane,{t},reserve before wrapped R,{owner},{lane},{row},{serial}"
        )
        .unwrap();
        self.audit()?;
        Ok(true)
    }
    fn capture(
        &mut self,
        t: u64,
        owner: usize,
        lane: usize,
        uv: u64,
        params: u128,
        out: &mut dyn Write,
    ) -> Result<(), String> {
        let row = self
            .live
            .iter()
            .position(|r| {
                r.as_ref()
                    .is_some_and(|r| r.owner == owner && r.lane == lane)
            })
            .ok_or("wrapped return without lane reservation")?;
        let mut r = self.live[row].as_ref().unwrap().clone();
        if !t.is_multiple_of(2) || r.edge + 1 != t {
            return Err("coordinate capture/phase without protected reservation".into());
        }
        let f = &lane_data(&r.source, lane).coordinate.frame;
        for (index, low) in [(0, 0), (1, 18)] {
            if input_value(f, "wrapped_uv", index, 18) != ((uv as u128 >> low) & ((1 << 18) - 1)) {
                return Err("coordinate actual wrapped inputs".into());
            }
        }
        for (name, index, width, low) in [
            ("flags", 0, 1, 0),
            ("flags", 1, 1, 1),
            ("side", 0, 12, 10),
            ("side", 1, 12, 22),
            ("coordinate_shift", 0, 18, 34),
        ] {
            if input_value(f, name, index, width) != ((params >> low) & ((1 << width) - 1)) {
                return Err("coordinate actual LOD inputs".into());
            }
        }
        let last_fine = params & (1 << 70) != 0;
        if last_fine != (value(&r.source.preparation().lod.frame, "last_fine") != 0) {
            return Err("coordinate actual plane flag".into());
        }
        self.last_fine[row] = last_fine;
        r.edge = t;
        self.coordinate.push_back(r.clone());
        self.stats.captured += 1;
        writeln!(
            out,
            "lane,{t},coordinate input capture,{owner},{lane},{row},{}",
            r.serial
        )
        .unwrap();
        self.audit()
    }
}
pub fn probe(root: &Path, d: &raw::Calendar) {
    let mut summary = fs::File::create(root.join("lane_summary.csv")).unwrap();
    writeln!(summary,"case,stress,wall,enabled,quads,covered,reserved,captured,returned,peak_context,peak_rows,peak_coord,peak_coeff,peak_ready,peak_work,stalls,older_progress_during_pause,quad_input_interval,max_context_age,first_work_block,first_source_block").unwrap();
    for (case, stress) in [
        ("bilinear", false),
        ("full", false),
        ("full", true),
        ("sparse", true),
        ("zero", true),
    ] {
        let mut qs = baseline::inputs(
            if case == "bilinear" {
                "bilinear"
            } else {
                "fractional"
            },
            15,
            64,
        );
        for (i, q) in qs.iter_mut().enumerate() {
            if case == "sparse" {
                q.mask = [1, 9, 15][i % 3];
            }
            if case == "zero" {
                q.mask = if i % 7 == 0 { 0 } else { [1, 9, 15][i % 3] };
            }
            if i % 5 == 0 {
                for uv in &mut q.uv {
                    uv[0] -= 1.0;
                    uv[1] += 2.0;
                }
            }
        }
        let mut edges = fs::File::create(root.join(format!("lane_{case}_{stress}.csv"))).unwrap();
        writeln!(
            edges,
            "source,enabled,event,owner,lane_or_address,row_or_direct,serial_or_extra"
        )
        .unwrap();
        let mut controller = Controller::new(false);
        let s = raw::run_sink(&qs, d, stress, false, Some(&mut controller), &mut edges).unwrap();
        let covered = qs
            .iter()
            .map(|q| q.mask.count_ones() as usize)
            .sum::<usize>();
        let c = &controller.stats;
        assert_eq!(
            (c.reserved, c.captured, c.returned),
            (covered, covered, covered)
        );
        let interval = (s.first_last[48].0 - s.first_last[16].0) as f64 / 32.0;
        let age = s.first_last.iter().map(|(a, z)| z - a).max().unwrap();
        writeln!(
            summary,
            "{case},{stress},{},{},64,{covered},{},{},{},{},{},{},{},{},{},{},{},{interval},{age},{},{}",
            s.wall,
            s.enabled,
            c.reserved,
            c.captured,
            c.returned,
            s.peak_contexts,
            c.peak_rows,
            c.peak_coord,
            c.peak_coeff,
            c.peak_ready,
            c.peak_work,
            c.stalls,
            c.pause_with_older_progress,
            previous::csv(&format!("{:?}",c.first_work_block)),previous::csv(&format!("{:?}",c.first_source_block))
        )
        .unwrap();
        println!("per-lane {case}/{stress}: II={interval} rows{} C{} F{} ready{} work{} stalls{} olderProgress{}",c.peak_rows,c.peak_coord,c.peak_coeff,c.peak_ready,c.peak_work,c.stalls,c.pause_with_older_progress);
        if case == "full" && stress {
            assert!(c.stalls > 0 && c.pause_with_older_progress > 0);
        }
        if case == "bilinear" {
            assert_eq!(interval, 8.0);
            assert_eq!(c.stalls, 0);
        }
        if case == "full" && !stress {
            assert_eq!(c.first_work_block, Some((85, 16, 2)));
            assert_eq!(c.first_source_block, Some((85, 12, 6, 5, 16)));
        }
    }
    let qs = baseline::inputs("fractional", 15, 4);
    let mut c = Controller::new(true);
    let error = raw::run_sink(&qs, d, true, false, Some(&mut c), &mut std::io::sink())
        .err()
        .expect("equal-value stale lane reply must fail");
    assert!(c.injected, "stale-owner injection must actually execute");
    assert_eq!(error, "lane head return reservation/owner/order");
    let mut cost = fs::File::create(root.join("lane_cost.csv")).unwrap();
    writeln!(cost, "field,extra_FF,RAM16SDP4_delta,scope").unwrap();
    let rows = [
        ("coordinate row tags5x4", 20),
        ("coefficient row tags6x4", 24),
        ("completed coefficient flags6", 6),
        ("coefficient pending return ref", 5),
        ("member pending return ref", 5),
        ("head valid/plane cursor", 2),
        ("two index-front refs", 10),
        ("geometry and operand R addresses", 10),
        ("wrapped pending row index", 4),
        ("captured LOD last-fine flag per row", 16),
    ];
    for (name, bits) in rows {
        writeln!(
            cost,
            "{name},{bits},0,conservative extra while retaining old480 prep control"
        )
        .unwrap();
    }
    assert_eq!(rows.iter().map(|(_, n)| n).sum::<u64>(), 102);
    writeln!(
        cost,
        "TOTAL,102,0,6695 provisional base becomes6797; no fitted Logic"
    )
    .unwrap();
    let mut selectors = fs::File::create(root.join("lane_selectors.csv")).unwrap();
    writeln!(selectors, "path,bits,source_count,bit_mux_nodes,scope").unwrap();
    for (name, bits) in [
        ("operand R address: coefficient/member", 4),
        ("operand W address: coordinate/weights", 4),
        ("operand W data: operands/weights", 72),
    ] {
        writeln!(
            selectors,
            "{name},{bits},2,{bits},explicit external selection; not fitted LUTs"
        )
        .unwrap();
    }
    writeln!(selectors,"PORT_SUBTOTAL,80,2,80,geometry has one R and one W source; bank internal mux belongs to RAM16 cells").unwrap();
    writeln!(selectors,"captured plane flag read,1,16,15,one flag selected by accepted coordinate descriptor before work admission").unwrap();
    writeln!(
        selectors,
        "TOTAL,95,mixed,95,bit mux nodes; not fitted Logic"
    )
    .unwrap();
    let mut controls = fs::File::create(root.join("lane_controls.csv")).unwrap();
    writeln!(controls, "state,width,limit,predicate_or_transition,scope").unwrap();
    for (name, bits, limit, predicate) in [
        (
            "row count",
            5,
            16,
            "reserve increments; full head capture decrements",
        ),
        (
            "coordinate credit",
            3,
            6,
            "wrapped R increments; accepted coefficient R decrements",
        ),
        (
            "coefficient processing credit",
            3,
            6,
            "coefficient R increments; completed descriptor publication decrements",
        ),
        (
            "coefficient ready credit",
            2,
            2,
            "completed descriptor publication increments; last plane issue decrements",
        ),
        (
            "work credit",
            5,
            16,
            "reserve one or two at coefficient R; external ACK decrements",
        ),
        (
            "ring allocate/reclaim pointer",
            4,
            16,
            "increment only on respective ownership transfer",
        ),
        ("plane flag capture decoder",4,16,"coordinate capture decodes row4 to sixteen one-bit write enables; four inverted address bits and sixteen four-input comparisons"),
    ] {
        writeln!(controls,"{name},{bits},{limit},{predicate},paid original control retained; finite counter/compare logic still requires lowering").unwrap();
    }
}
