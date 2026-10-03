//! Independent integer goldens and bounded per-edge register/owner certificates.
use gpu_v2::texture::emu::coefficient::*;
use std::collections::VecDeque;

fn input(index: usize, parent: u16, fractions: [[u8; 2]; 2]) -> Input {
    Input {
        parents: [parent, 511 - parent],
        fractions,
        nearest: false,
        metadata: Metadata {
            coordinates: std::array::from_fn(|p| {
                std::array::from_fn(|i| ((index * 37 + p * 173 + i * 59) & 1023) as u16)
            }),
            levels: [(index % 11) as u8, ((index + 7) % 11) as u8],
            slot: (index % 16) as u8,
            key: (index % 64) as u8,
            last_fine: parent == 511,
        },
    }
}
fn rows(v: Input, plane: usize) -> [u16; 2] {
    let fraction = if v.nearest {
        0
    } else {
        u64::from(v.fractions[plane][1])
    };
    let bottom = (u64::from(v.parents[plane]) * fraction / 256) as u16;
    [v.parents[plane] - bottom, bottom]
}
fn golden(v: Input) -> Output {
    Output {
        metadata: v.metadata,
        weights: std::array::from_fn(|p| {
            let [top, bottom] = rows(v, p);
            let fraction = if v.nearest {
                0
            } else {
                u64::from(v.fractions[p][0])
            };
            let tr = (u64::from(top) * fraction / 256) as u16;
            let br = (u64::from(bottom) * fraction / 256) as u16;
            [top - tr, tr, bottom - br, br]
        }),
    }
}
fn field_golden(v: Input, field: Field) -> u16 {
    use Field::*;
    let g = golden(v);
    let fraction = |p: usize, a: usize| {
        if v.nearest {
            0
        } else {
            u16::from(v.fractions[p][a])
        }
    };
    match field {
        Nearest => u16::from(v.nearest),
        FineParent => v.parents[0],
        CoarseParent => v.parents[1],
        FineURaw => u16::from(v.fractions[0][0]),
        FineVRaw => u16::from(v.fractions[0][1]),
        CoarseURaw => u16::from(v.fractions[1][0]),
        CoarseVRaw => u16::from(v.fractions[1][1]),
        FineU => fraction(0, 0),
        FineV => fraction(0, 1),
        CoarseU => fraction(1, 0),
        CoarseV => fraction(1, 1),
        FineTop => rows(v, 0)[0],
        FineBottom => rows(v, 0)[1],
        CoarseTop => rows(v, 1)[0],
        CoarseBottom => rows(v, 1)[1],
        FineTopLeft => g.weights[0][0],
        FineTopRight => g.weights[0][1],
        FineBottomLeft => g.weights[0][2],
        FineBottomRight => g.weights[0][3],
        CoarseTopLeft => g.weights[1][0],
        CoarseTopRight => g.weights[1][1],
        CoarseBottomLeft => g.weights[1][2],
        CoarseBottomRight => g.weights[1][3],
    }
}
/// Expected calendar is the explicit contract, not emulator-generated events.
fn operands(v: Input, site: Site, age: u8) -> [u16; 2] {
    use Field::*;
    let pair = match (site, age) {
        (Site::Multiply(0), 1) => (FineParent, FineV),
        (Site::Multiply(0), 2) => (CoarseParent, CoarseV),
        (Site::Multiply(1), 4) => (FineBottom, FineU),
        (Site::Multiply(1), 5) => (FineTop, FineU),
        (Site::Multiply(2), 5) => (CoarseBottom, CoarseU),
        (Site::Multiply(2), 6) => (CoarseTop, CoarseU),
        (Site::Subtract(0), 4) => (FineParent, FineBottom),
        (Site::Subtract(0), 5) => (CoarseParent, CoarseBottom),
        (Site::Subtract(1), 7) => (FineBottom, FineBottomRight),
        (Site::Subtract(1), 8) => (FineTop, FineTopRight),
        (Site::Subtract(2), 8) => (CoarseBottom, CoarseBottomRight),
        (Site::Subtract(2), 9) => (CoarseTop, CoarseTopRight),
        (Site::Select(0), 0) => (Nearest, FineURaw),
        (Site::Select(1), 0) => (Nearest, FineVRaw),
        (Site::Select(0), 1) => (Nearest, CoarseURaw),
        (Site::Select(1), 1) => (Nearest, CoarseVRaw),
        _ => panic!("unexpected issue {site:?}@{age}"),
    };
    [field_golden(v, pair.0), field_golden(v, pair.1)]
}
fn site_index(site: Site) -> usize {
    match site {
        Site::Multiply(i) => i as usize,
        Site::Subtract(i) => i as usize + 3,
        Site::Select(i) => i as usize + 6,
    }
}
#[derive(Clone, Copy)]
struct Flight {
    input: Input,
    accepted: u64,
    serial: usize,
}
#[derive(Clone, Copy, Debug)]
struct Owner {
    serial: usize,
    field: Field,
    source_bit: u8,
    birth: u64,
    expires: u64,
    value: bool,
}
#[derive(Clone, Copy)]
struct Pending {
    serial: usize,
    key: u8,
    ready: u64,
    value: u32,
}
struct Certificate {
    flights: VecDeque<Flight>,
    ready: VecDeque<Output>,
    operators: [VecDeque<Pending>; 8],
    bits: [Option<Owner>; NUMERIC_BITS],
    accepted: usize,
    consumed: usize,
    products: usize,
    returns: usize,
    writes: usize,
    peak_cohorts: u8,
    peak_ready: u8,
    frozen_consumes: usize,
}
impl Certificate {
    fn new() -> Self {
        Self {
            flights: VecDeque::new(),
            ready: VecDeque::new(),
            operators: std::array::from_fn(|_| VecDeque::new()),
            bits: [None; NUMERIC_BITS],
            accepted: 0,
            consumed: 0,
            products: 0,
            returns: 0,
            writes: 0,
            peak_cohorts: 0,
            peak_ready: 0,
            frozen_consumes: 0,
        }
    }
    fn flight(&self, now: u64, key: u8, age: u8) -> Flight {
        let f = *self
            .flights
            .iter()
            .find(|f| f.accepted + u64::from(age) == now)
            .expect("unowned operation");
        assert_eq!(f.input.metadata.key, key);
        f
    }
    fn check(&mut self, pre: Snapshot, tick: Tick, step: &Step) {
        let now = pre.enabled;
        assert_eq!(step.output, self.ready.front().copied(), "pre-edge output");
        assert_eq!(
            step.consumed,
            tick.ce && tick.output_ready && step.output.is_some()
        );
        let advances = tick.ce && pre.queued < 2;
        assert_eq!(step.snapshot.enabled, now + u64::from(advances));
        if step.consumed {
            assert_eq!(self.ready.pop_front(), step.output);
            self.consumed += 1;
            self.frozen_consumes += usize::from(!advances);
        }
        if step.accepted {
            assert!(step.input_ready && advances && now.is_multiple_of(2));
            let v = tick.input.unwrap();
            let demand = v.parents.into_iter().filter(|p| *p != 0).count() as u8;
            assert_eq!(step.work_reserved, demand);
            assert!(tick.work_available >= demand);
            self.flights.push_back(Flight {
                input: v,
                accepted: now,
                serial: self.accepted,
            });
            self.accepted += 1;
        } else {
            assert_eq!(step.work_reserved, 0);
        }
        if !advances {
            assert_eq!(step.snapshot.numeric_words, pre.numeric_words);
            assert_eq!(step.snapshot.product_registers, pre.product_registers);
            assert_eq!(step.snapshot.valid, pre.valid);
            assert_eq!(step.snapshot.phase, pre.phase);
            assert_eq!(step.snapshot.cohort_pointers, pre.cohort_pointers);
            assert_eq!(step.snapshot.cohorts, pre.cohorts);
        }
        let mut issued = [false; 8];
        let mut returned = [false; 8];
        let mut touched = [false; NUMERIC_BITS];
        let mut captures = Vec::new();
        let mut saw_write = false;
        for event in &step.events {
            match *event {
                Event::Issued {
                    site,
                    key,
                    age,
                    operands: actual,
                } => {
                    assert!(advances);
                    let f = self.flight(now, key, age);
                    let index = site_index(site);
                    assert!(!issued[index], "site collision");
                    issued[index] = true;
                    let expected = operands(f.input, site, age);
                    assert_eq!(actual, expected, "runtime operands {site:?}@{age}");
                    let (latency, value) = match site {
                        Site::Multiply(_) => {
                            self.products += 1;
                            (3, u32::from(actual[0]) * u32::from(actual[1]))
                        }
                        Site::Subtract(_) => (1, u32::from(actual[0]) - u32::from(actual[1])),
                        Site::Select(_) => (
                            1,
                            if actual[0] != 0 {
                                0
                            } else {
                                u32::from(actual[1])
                            },
                        ),
                    };
                    self.operators[index].push_back(Pending {
                        serial: f.serial,
                        key,
                        ready: now + latency,
                        value,
                    });
                }
                Event::Returned {
                    site,
                    key,
                    age,
                    value,
                } => {
                    assert!(advances);
                    let f = self.flight(now, key, age);
                    let index = site_index(site);
                    assert!(!returned[index]);
                    returned[index] = true;
                    let expected = self.operators[index].pop_front().expect("unissued return");
                    assert_eq!(
                        (f.serial, key, now, value),
                        (
                            expected.serial,
                            expected.key,
                            expected.ready,
                            expected.value
                        )
                    );
                    self.returns += 1;
                }
                Event::Access(a) => {
                    assert!(advances);
                    let f = self.flight(now, a.key, a.age);
                    let layout = LAYOUT[a.field as usize];
                    assert_eq!(a.width, layout.width);
                    assert_eq!(a.low, layout.lows[(f.accepted % 8 / 2) as usize]);
                    assert_eq!(
                        a.value,
                        field_golden(f.input, a.field),
                        "field {:?} serial {} age {}",
                        a.field,
                        f.serial,
                        a.age
                    );
                    match a.kind {
                        AccessKind::Read => {
                            assert!(!saw_write, "read after reuse write");
                            for bit in 0..a.width {
                                let old = self.bits[a.low + usize::from(bit)]
                                    .expect("uncaptured FF read");
                                assert_eq!(
                                    (old.serial, old.field, old.source_bit),
                                    (f.serial, a.field, bit),
                                    "wrong bit owner"
                                );
                                assert!(
                                    old.birth <= now && old.expires >= now,
                                    "expired/early FF read"
                                );
                                assert_eq!(old.value, a.value >> bit & 1 != 0);
                            }
                        }
                        AccessKind::Write => {
                            saw_write = true;
                            let birth = f.accepted + u64::from(layout.birth);
                            assert!(birth == now || birth == now + 1, "late/early write");
                            for bit in 0..a.width {
                                let address = a.low + usize::from(bit);
                                assert!(!touched[address], "two physical FF writes/edge");
                                touched[address] = true;
                                if let Some(old) = self.bits[address] {
                                    assert!(
                                        old.expires <= now,
                                        "overwrite live owner {old:?} at {now}, new {:?}",
                                        a.field
                                    );
                                }
                                self.bits[address] = Some(Owner {
                                    serial: f.serial,
                                    field: a.field,
                                    source_bit: bit,
                                    birth,
                                    expires: f.accepted + u64::from(layout.last_read),
                                    value: a.value >> bit & 1 != 0,
                                });
                            }
                            self.writes += 1;
                        }
                    }
                }
                Event::OutputRegister { key } => {
                    self.flight(now, key, 10);
                }
                Event::Captured(out) => {
                    let f = self.flight(now, out.metadata.key, 11);
                    assert_eq!(out, golden(f.input));
                    assert_eq!(out.weights[0].iter().sum::<u16>(), f.input.parents[0]);
                    assert_eq!(out.weights[1].iter().sum::<u16>(), f.input.parents[1]);
                    self.ready.push_back(out);
                    captures.push(f.serial);
                }
                Event::Accepted { key, work_reserved } => {
                    assert!(step.accepted);
                    assert_eq!(key, tick.input.unwrap().metadata.key);
                    assert_eq!(work_reserved, step.work_reserved);
                }
                Event::Consumed(out) => assert_eq!(Some(out), step.output),
            }
        }
        if advances {
            for pipe in &self.operators {
                assert!(
                    pipe.front().is_none_or(|p| p.ready > now),
                    "missing registered return"
                );
                assert!(pipe.len() <= 3);
            }
        }
        for serial in captures {
            assert_eq!(self.flights.pop_front().unwrap().serial, serial);
        }
        for (address, owner) in self.bits.iter().enumerate() {
            let actual = step.snapshot.numeric_words[address / 64] >> (address % 64) & 1 != 0;
            assert_eq!(
                actual,
                owner.is_some_and(|o| o.value),
                "physical bit differs {address}"
            );
        }
        assert_eq!(self.flights.len(), usize::from(step.snapshot.cohorts));
        assert_eq!(self.ready.len(), usize::from(step.snapshot.queued));
        assert!(self.flights.len() <= 6 && self.ready.len() <= 2);
        assert_eq!(step.snapshot.numeric_words[4] >> 59, 0);
        self.peak_cohorts = self.peak_cohorts.max(step.snapshot.cohorts);
        self.peak_ready = self.peak_ready.max(step.snapshot.queued);
    }
    fn drained(&self) {
        assert!(self.flights.is_empty() && self.ready.is_empty());
        assert!(self.operators.iter().all(VecDeque::is_empty));
        assert_eq!(self.accepted, self.consumed);
        assert_eq!(self.products, 6 * self.accepted);
        assert_eq!(self.returns, 16 * self.accepted);
    }
}
fn checked(r: &mut CoefficientEmu, c: &mut Certificate, tick: Tick) -> Step {
    let pre = r.snapshot();
    let step = r.tick(tick).unwrap();
    c.check(pre, tick, &step);
    step
}

#[test]
fn exhaustive_splits_execute_real_pipeline_and_certify_every_register_owner() {
    let mut r = CoefficientEmu::new(400_000).unwrap();
    let mut c = Certificate::new();
    let mut sent = 0;
    for _ in 0..400_000 {
        let offer = (sent < 512 * 256).then(|| {
            let p = (sent / 256) as u16;
            let f = (sent % 256) as u8;
            input(
                sent,
                p,
                [
                    [(sent.wrapping_mul(37) & 255) as u8, f],
                    [f.wrapping_mul(19), 255 - f],
                ],
            )
        });
        let s = checked(
            &mut r,
            &mut c,
            Tick {
                input: offer,
                ..Tick::default()
            },
        );
        sent += usize::from(s.accepted);
        if sent == 512 * 256 && r.idle() {
            break;
        }
    }
    assert!(r.idle());
    assert_eq!(sent, 512 * 256);
    assert_eq!(c.peak_cohorts, 6);
    c.drained();
    println!(
        "exhaustive vectors={sent} products={} returns={} physical field writes={} enabled={}",
        c.products,
        c.returns,
        c.writes,
        r.snapshot().enabled
    );
}

#[test]
fn joint_edge_grid_varies_all_four_fractions_independently() {
    let edges = [0, 1, 127, 128, 129, 254, 255];
    let parents = [0, 1, 2, 127, 255, 256, 383, 510, 511];
    let vectors = parents.len() * edges.len().pow(4);
    let mut r = CoefficientEmu::new(100_000).unwrap();
    let mut c = Certificate::new();
    let mut sent = 0;
    for _ in 0..100_000 {
        let offer = (sent < vectors).then(|| {
            let mut digits = sent / parents.len();
            let fractions = std::array::from_fn(|_| {
                std::array::from_fn(|_| {
                    let f = edges[digits % edges.len()];
                    digits /= edges.len();
                    f
                })
            });
            input(sent, parents[sent % parents.len()], fractions)
        });
        let s = checked(
            &mut r,
            &mut c,
            Tick {
                input: offer,
                ..Tick::default()
            },
        );
        sent += usize::from(s.accepted);
        if sent == vectors && r.idle() {
            break;
        }
    }
    assert!(r.idle());
    assert_eq!(sent, 21_609);
    c.drained();
    println!("independent four-fraction edge-grid vectors={sent}");
}

#[test]
fn hand_vectors_and_nearest_ignore_fraction_values_on_fixed_six_product_calendar() {
    let mut cases = vec![
        input(0, 255, [[128; 2]; 2]),
        input(1, 511, [[128; 2]; 2]),
        input(2, 0, [[255, 1], [1, 255]]),
    ];
    assert_eq!(golden(cases[0]).weights, [[64, 64, 64, 63], [64; 4]]);
    assert_eq!(golden(cases[1]).weights, [[128, 128, 128, 127], [0; 4]]);
    for f in 0..=255 {
        let mut v = input(
            usize::from(f) + 3,
            511,
            [[f, 255 - f], [f.wrapping_mul(17), f.wrapping_mul(73)]],
        );
        v.nearest = true;
        cases.push(v);
    }
    let mut r = CoefficientEmu::new(20_000).unwrap();
    let mut c = Certificate::new();
    let mut sent = 0;
    let mut first_capture = None;
    let mut first_consume = None;
    for _ in 0..20_000 {
        let before = r.snapshot().enabled;
        let s = checked(
            &mut r,
            &mut c,
            Tick {
                input: cases.get(sent).copied(),
                ..Tick::default()
            },
        );
        sent += usize::from(s.accepted);
        if s.events.iter().any(|e| matches!(e, Event::Captured(_))) && first_capture.is_none() {
            first_capture = Some(before);
        }
        if s.consumed && first_consume.is_none() {
            first_consume = Some(before);
        }
        if sent == cases.len() && r.idle() {
            break;
        }
    }
    assert_eq!(first_capture, Some(11));
    assert_eq!(first_consume, Some(12));
    assert!(r.idle());
    assert_eq!(sent, cases.len());
    c.drained();
}

#[test]
fn work16_ready_full_ce_and_reused_metadata_drain_with_actual_acknowledgements() {
    let mut r = CoefficientEmu::new(20_000).unwrap();
    let mut c = Certificate::new();
    let mut sent = 0usize;
    let mut work_used = 0u8;
    // Fixture-owned downstream leases: release only after consumption and an
    // explicit later last-use acknowledgement. At most16 work entries exist.
    let mut waiting: VecDeque<(Metadata, u8)> = VecDeque::new();
    let mut acknowledgements: VecDeque<(usize, u8)> = VecDeque::new();
    let mut pause_ages = [false; 12];
    // Age0 is an external offer before its accepted edge, unlike later live
    // pipeline ages; force its first three attempted captures to CE=0.
    pause_ages[0] = true;
    let mut pause_remaining = 3;
    let mut saw_no_work = false;
    let mut saw_one_not_two = false;
    let mut saw_full_no_future_credit = false;
    let mut saw_work_unchanged_by_consume = false;
    let mut offer_changes = 0;
    let mut rng = 0x8319_2b47u32;
    for wall in 0..20_000 {
        while acknowledgements.front().is_some_and(|a| a.0 == wall) {
            work_used -= acknowledgements.pop_front().unwrap().1;
        }
        // Pause at every numerical pipeline age, including capture. Clock age
        // is fixture state, never a scheduling/operand source for the emulator.
        let pre = r.snapshot();
        if pause_remaining == 0 {
            if let Some(age) = (0..12).find(|&a| {
                !pause_ages[a]
                    && c.flights
                        .iter()
                        .any(|f| f.accepted + a as u64 == pre.enabled)
            }) {
                pause_ages[age] = true;
                pause_remaining = 3;
            }
        }
        let ce = if pause_remaining != 0 {
            pause_remaining -= 1;
            false
        } else {
            wall % 23 > 2
        };
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let offer = (sent < 193).then(|| {
            // Early demands need two credits. Later mix single/coarse-only,
            // nearest and independently varied fractions and repeating keys.
            let parent = if sent < 16 {
                // First fill all16 with two-plane owners, then accept one
                // single-plane owner after an ACK and hold the next two-plane
                // offer with exactly one free credit.
                if sent == 8 {
                    511
                } else {
                    255
                }
            } else {
                [0, 1, 2, 127, 255, 256, 383, 510, 511][sent % 9]
            };
            let mut v = input(
                sent * 64 + wall % 64,
                parent,
                [
                    [rng as u8, (rng >> 8) as u8],
                    [(rng >> 16) as u8, (rng >> 24) as u8],
                ],
            );
            v.metadata.key = (sent % 3) as u8;
            v.nearest = sent >= 16 && parent == 511 && sent.is_multiple_of(2);
            v
        });
        let available = 16 - work_used;
        let used_before = work_used;
        let tick = Tick {
            ce,
            input: offer,
            output_ready: wall > 100 && wall % 19 > 3,
            work_available: available,
        };
        let s = checked(&mut r, &mut c, tick);
        if pre.queued == 2 && s.consumed {
            assert!(!s.accepted && !s.input_ready);
            assert_eq!(s.snapshot.enabled, pre.enabled);
            saw_full_no_future_credit = true;
        }
        if let Some(v) = offer {
            let demand = v.parents.into_iter().filter(|p| *p != 0).count() as u8;
            let eligible = ce && pre.queued < 2 && pre.cohorts < 6 && pre.phase & 0x55 != 0;
            if eligible && available < demand {
                assert!(!s.input_ready && !s.accepted);
                saw_no_work |= available == 0;
                saw_one_not_two |= available == 1 && demand == 2;
            }
            if !s.accepted {
                offer_changes += 1;
            } else {
                waiting.push_back((v.metadata, demand));
                work_used += s.work_reserved;
                sent += 1;
            }
        }
        if s.consumed {
            let out = s.output.unwrap();
            let (owner, cost) = waiting.pop_front().unwrap();
            assert_eq!(out.metadata, owner);
            let due = acknowledgements
                .back()
                .map_or(wall + 151, |a| (wall + 151).max(a.0 + 11));
            acknowledgements.push_back((due, cost));
            // Output consumption reports no negative reservation/release pulse.
            assert_eq!(work_used, used_before + s.work_reserved);
            saw_work_unchanged_by_consume = true;
        }
        assert!(work_used <= 16);
        assert!(waiting.len() + acknowledgements.len() <= 16);
        assert_eq!(
            work_used,
            waiting
                .iter()
                .map(|v| v.1)
                .chain(acknowledgements.iter().map(|v| v.1))
                .sum::<u8>()
        );
        if sent == 193 && r.idle() && work_used == 0 {
            break;
        }
    }
    assert!(r.idle() && work_used == 0 && waiting.is_empty() && acknowledgements.is_empty());
    assert_eq!(sent, 193);
    assert!(pause_ages.into_iter().all(|p| p));
    assert!(saw_no_work && saw_full_no_future_credit && saw_work_unchanged_by_consume);
    // Mixtures must actually reach the one-free/two-required boundary.
    assert!(saw_one_not_two);
    assert!(offer_changes > 100 && c.frozen_consumes > 0);
    assert_eq!((c.peak_cohorts, c.peak_ready), (6, 2));
    c.drained();
    println!("pressure vectors={sent}, wall={}, enabled={}, blocked offer changes={offer_changes}, frozen consumes={}", r.snapshot().wall, r.snapshot().enabled, c.frozen_consumes);
}

#[test]
fn invalid_offers_are_checked_only_on_eligible_capture_and_faults_are_terminal() {
    let good = input(0, 255, [[128; 2]; 2]);
    let mut invalid = Vec::new();
    for parents in [[512, 0], [u16::MAX, 511], [0, 0], [255, 255]] {
        let mut v = good;
        v.parents = parents;
        invalid.push(v);
    }
    let mut v = good;
    v.nearest = true;
    invalid.push(v);
    let mut v = good;
    v.metadata.last_fine = true;
    invalid.push(v);
    let mut v = good;
    v.metadata.coordinates[1][3] = 1024;
    invalid.push(v);
    let mut v = good;
    v.metadata.levels[0] = 11;
    invalid.push(v);
    let mut v = good;
    v.metadata.slot = 16;
    invalid.push(v);
    let mut v = good;
    v.metadata.key = 64;
    invalid.push(v);
    for v in invalid {
        let mut r = CoefficientEmu::new(20_000).unwrap();
        assert!(
            !r.tick(Tick {
                ce: false,
                input: Some(v),
                ..Tick::default()
            })
            .unwrap()
            .accepted
        );
        assert!(
            !r.tick(Tick {
                input: Some(good),
                work_available: 1,
                ..Tick::default()
            })
            .unwrap()
            .accepted
        );
        // Work shortage rejected the input but advanced the empty calendar to
        // odd phase; this invalid payload remains external on that phase.
        assert!(
            !r.tick(Tick {
                input: Some(v),
                ..Tick::default()
            })
            .unwrap()
            .accepted
        );
        assert!(!r.faulted());
        assert_eq!(
            r.tick(Tick {
                input: Some(v),
                ..Tick::default()
            })
            .unwrap_err(),
            Fault::Input
        );
        assert!(r.faulted());
        assert_eq!(r.tick(Tick::default()).unwrap_err(), Fault::Terminal);
    }
    let mut r = CoefficientEmu::new(20_000).unwrap();
    assert_eq!(
        r.tick(Tick {
            input: Some(good),
            work_available: 17,
            ..Tick::default()
        })
        .unwrap_err(),
        Fault::WorkCredit
    );
    let mut r = CoefficientEmu::new(2).unwrap();
    for _ in 0..2 {
        assert!(
            !r.tick(Tick {
                ce: false,
                ..Tick::default()
            })
            .unwrap()
            .accepted
        );
    }
    assert_eq!(r.tick(Tick::default()).unwrap_err(), Fault::Watchdog);
    assert_eq!(r.tick(Tick::default()).unwrap_err(), Fault::Terminal);
    assert_eq!(CoefficientEmu::new(0).err(), Some(Fault::WatchdogBound));
    assert_eq!(
        CoefficientEmu::new(2_000_001).err(),
        Some(Fault::WatchdogBound)
    );
    let mut r = CoefficientEmu::new(100).unwrap();
    for _ in 0..40 {
        r.tick(Tick {
            input: Some(good),
            output_ready: false,
            ..Tick::default()
        })
        .unwrap();
        if r.snapshot().queued == 2 {
            break;
        }
    }
    assert_eq!(r.snapshot().queued, 2);
    let mut bad = good;
    bad.parents = [0, 0];
    let s = r
        .tick(Tick {
            input: Some(bad),
            ..Tick::default()
        })
        .unwrap();
    assert!(!s.accepted && !s.input_ready && s.consumed);
    assert!(!r.faulted());
    assert_eq!(
        r.tick(Tick {
            input: Some(bad),
            ..Tick::default()
        })
        .unwrap_err(),
        Fault::Input
    );
}
