//! Demand-only texture-cache leaf: emulator behavior and independent golden.
//!
//! The expected texels are derived from the actual RAW565 byte image with a
//! test-local address/decoder that does not call any DUT bank or address helper.
//! A test-local memory adapter owns all request latency, acknowledgement mode
//! and backpressure. The golden never consults the sampler's `tap_map`,
//! `layer_offset`, bank array or address helpers.

#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;

use gpu_v2::memory::ports::{MemoryPort, Request, Response};
use gpu_v2::texture::emu::cache::{Access, CacheEmu, CacheTick, LineState, Output, REFILL_BEATS};
use gpu_v2::texture::ports::{Group4, Slot, TileKey};

/// Independent copy of the published mip-prefix table. The golden must not use
/// the sampler's own helper.
const LAYER_OFFSET: [usize; 11] = [0, 1, 2, 3, 4, 8, 24, 88, 344, 1368, 5464];

fn layer_offset_independent(n: usize) -> usize {
    LAYER_OFFSET[n]
}

/// Behavioral memory with an explicit acknowledgement policy.
///
/// `ack_delay` is the number of idle edges between the last numbered beat and
/// the terminal acknowledgement: `0` shares the last-beat edge (combined),
/// `>= 1` issues the terminal later with no beat (separate/delayed).
/// `same_edge_first` additionally returns beat 0 on the request-acceptance edge.
struct HostMemory {
    bytes: Vec<u8>,
    base: u64,
    ready_period: u64,
    fault_at: Option<u8>,
    ack_delay: u8,
    same_edge_first: bool,
    busy: bool,
    waiting: bool,
    addr: u64,
    count: u8,
    wait_edges: u8,
    edge: u64,
    max_edges: u64,
    pub requests: usize,
    pub beats: usize,
}

impl HostMemory {
    fn new(bytes: Vec<u8>, ready_period: u64, fault_at: Option<u8>) -> Self {
        Self::new_mode(bytes, ready_period, fault_at, 0, false)
    }

    fn new_mode(
        bytes: Vec<u8>,
        ready_period: u64,
        fault_at: Option<u8>,
        ack_delay: u8,
        same_edge_first: bool,
    ) -> Self {
        Self {
            bytes,
            base: u64::from(support::BASE),
            ready_period,
            fault_at,
            ack_delay,
            same_edge_first,
            busy: false,
            waiting: false,
            addr: 0,
            count: 0,
            wait_edges: 0,
            edge: 0,
            max_edges: 2_000_000,
            requests: 0,
            beats: 0,
        }
    }

    fn word(&self, index: usize) -> u64 {
        let offset = usize::try_from(self.addr - self.base).unwrap() + index * 8;
        u64::from_le_bytes(self.bytes[offset..offset + 8].try_into().unwrap())
    }
}

impl MemoryPort for HostMemory {
    fn cycle(&mut self, request: Option<Request>, _write: Option<u64>) -> Result<Response, String> {
        if self.edge >= self.max_edges {
            return Err("test memory watchdog".into());
        }
        let mut response = Response::default();
        let ready = self.ready_period == 0 || !self.edge.is_multiple_of(self.ready_period);
        if self.busy {
            if self.waiting {
                self.wait_edges += 1;
                if self.wait_edges >= self.ack_delay {
                    response.complete = Some(true);
                    self.busy = false;
                    self.waiting = false;
                }
            } else {
                let index = self.count;
                response.read = Some((index, self.word(usize::from(index))));
                self.beats += 1;
                if self.fault_at == Some(index) {
                    response.complete = Some(false);
                    self.busy = false;
                } else if index + 1 == REFILL_BEATS as u8 {
                    if self.ack_delay == 0 {
                        response.complete = Some(true);
                        self.busy = false;
                    } else {
                        self.waiting = true;
                        self.wait_edges = 0;
                    }
                } else {
                    self.count += 1;
                }
            }
        } else if let Some(request) = request {
            if ready && !request.write {
                self.busy = true;
                self.addr = request.address_bytes;
                self.count = 0;
                self.requests += 1;
                response.accepted = true;
                if self.same_edge_first {
                    response.read = Some((0, self.word(0)));
                    self.beats += 1;
                    self.count = 1;
                }
            }
        }
        self.edge += 1;
        Ok(response)
    }
}

/// Memory adapter that fails the very first call: a fatal adapter error with
/// unknown external ownership, not a protocol fault.
struct FatalMemory {
    calls: usize,
}

impl MemoryPort for FatalMemory {
    fn cycle(
        &mut self,
        _request: Option<Request>,
        _write: Option<u64>,
    ) -> Result<Response, String> {
        self.calls += 1;
        Err("test adapter fatal".into())
    }
}

/// Memory adapter that injects exactly one protocol violation: an unsolicited
/// response on the first edge, a bad/out-of-order beat, an early terminal or a
/// duplicate acceptance. Used only to check that the emulator faults and never
/// publishes a partial line.
struct ProtocolMemory {
    bytes: Vec<u8>,
    base: u64,
    busy: bool,
    addr: u64,
    count: u8,
    edge: u64,
    inject_first: Option<Response>,
    bad_beat: Option<u8>,
    malformed_beat: bool,
    early_success: bool,
    duplicate_accept: bool,
}

impl ProtocolMemory {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            base: u64::from(support::BASE),
            busy: false,
            addr: 0,
            count: 0,
            edge: 0,
            inject_first: None,
            bad_beat: None,
            malformed_beat: false,
            early_success: false,
            duplicate_accept: false,
        }
    }

    fn word(&self, index: usize) -> u64 {
        let offset = usize::try_from(self.addr - self.base).unwrap() + index * 8;
        u64::from_le_bytes(self.bytes[offset..offset + 8].try_into().unwrap())
    }
}

impl MemoryPort for ProtocolMemory {
    fn cycle(&mut self, request: Option<Request>, _write: Option<u64>) -> Result<Response, String> {
        if self.edge == 0 {
            if let Some(response) = self.inject_first {
                self.edge += 1;
                return Ok(response);
            }
        }
        let mut response = Response::default();
        if self.busy {
            let index = self.count;
            let reported = if self.malformed_beat && index == 0 {
                200
            } else if self.bad_beat == Some(index) {
                index.wrapping_add(1)
            } else {
                index
            };
            response.read = Some((reported, self.word(usize::from(index))));
            if self.early_success && index == 0 {
                response.complete = Some(true);
            }
            if self.duplicate_accept && index == 0 {
                response.accepted = true;
            }
            if index + 1 == REFILL_BEATS as u8 {
                response.complete = Some(true);
                self.busy = false;
            } else {
                self.count += 1;
            }
        } else if let Some(request) = request {
            if !request.write {
                self.busy = true;
                self.addr = request.address_bytes;
                self.count = 0;
                response.accepted = true;
            }
        }
        self.edge += 1;
        Ok(response)
    }
}

fn packet(
    key: TileKey,
    top_left_local: [u8; 2],
    coefficients: [u32; 4],
    first: bool,
    last: bool,
    quad_id: u8,
    lane: u8,
) -> i128 {
    Group4 {
        key,
        top_left_local,
        coefficients,
        first,
        last,
        quad_id,
        lane,
    }
    .pack72()
    .unwrap() as i128
}

/// Independent RAW565 word lookup from the byte image, row-major. It re-derives
/// the padded 8x8 tile address itself and never consults DUT banks or mapping.
fn golden_word(bytes: &[u8], slot: &Slot, key: TileKey, lx: usize, ly: usize) -> u16 {
    // `bytes` is the image relative to the slot base, exactly as the memory
    // adapter serves it, so only the in-slot offset is used here.
    let layer = if slot.has_full_mip {
        layer_offset_independent(usize::from(key.n))
    } else {
        0
    };
    let side = 1usize << key.n.saturating_sub(3);
    let tile = (layer + usize::from(key.y) * side + usize::from(key.x)) * 128;
    let offset = tile + (ly * 8 + lx) * 2;
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn golden_output(bytes: &[u8], slots: &[Slot], payload: i128) -> Output {
    let group = Group4::unpack72(payload).unwrap();
    let slot = slots[usize::from(group.key.slot)];
    let tx = usize::from(group.top_left_local[0]);
    let ty = usize::from(group.top_left_local[1]);
    let texels = std::array::from_fn(|t| {
        let lx = (tx + (t & 1)) & 7;
        let ly = (ty + (t >> 1)) & 7;
        golden_word(bytes, &slot, group.key, lx, ly)
    });
    Output { payload, texels }
}

fn expect_golden(bytes: &[u8], slots: &[Slot], packets: &[i128], got: &[Output]) {
    let want: Vec<_> = packets
        .iter()
        .map(|&payload| golden_output(bytes, slots, payload))
        .collect();
    assert_eq!(got, want);
}

/// Run with an explicit per-edge CE schedule. The schedule sees the current
/// edge index and the pre-edge diagnostic state.
fn drain_scheduled(
    emu: &mut CacheEmu<HostMemory>,
    packets: &[i128],
    max_edges: u64,
    mut schedule: impl FnMut(u64, &gpu_v2::texture::emu::cache::CacheState) -> bool,
) -> Vec<Output> {
    let mut ptr = 0;
    let mut outputs = Vec::new();
    for edge in 0..max_edges {
        let ce = schedule(edge, &emu.state());
        let input = packets.get(ptr).copied();
        let step = emu
            .tick(CacheTick {
                ce,
                input,
                output_ready: true,
            })
            .unwrap();
        if step.accepted {
            ptr += 1;
        }
        if step.transferred {
            outputs.push(step.output.expect("transferred output"));
        }
        if ptr == packets.len() && emu.idle() {
            return outputs;
        }
    }
    panic!("texture cache did not drain within the edge bound");
}

fn drain(emu: &mut CacheEmu<HostMemory>, packets: &[i128], max_edges: u64) -> Vec<Output> {
    drain_scheduled(emu, packets, max_edges, |_, _| true)
}

#[test]
fn first_access_misses_then_repeats_hit_and_match_independent_golden() {
    let slot = support::slot(10, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let packets = vec![
        packet(
            TileKey {
                slot: 0,
                n: 10,
                x: 3,
                y: 5,
            },
            [1, 2],
            [0, 0, 0, 511],
            true,
            true,
            0,
            0,
        ),
        packet(
            TileKey {
                slot: 0,
                n: 10,
                x: 3,
                y: 5,
            },
            [1, 2],
            [511, 0, 0, 0],
            true,
            true,
            1,
            0,
        ),
        packet(
            TileKey {
                slot: 0,
                n: 4,
                x: 1,
                y: 0,
            },
            [6, 6],
            [128, 128, 127, 128],
            true,
            true,
            2,
            3,
        ),
    ];
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        100_000,
    )
    .unwrap();
    let outputs = drain(&mut emu, &packets, 5_000);
    expect_golden(&bytes, &slots, &packets, &outputs);
    // One refill per distinct key; the repeated key is a hit.
    assert_eq!(emu.memory().requests, 2);
    assert!(emu.idle());
}

#[test]
fn every_local_seam_wraps_and_touches_all_four_banks() {
    let slot = support::slot(8, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let key = TileKey {
        slot: 0,
        n: 8,
        x: 2,
        y: 1,
    };
    let packets: Vec<i128> = (0..8_u8)
        .flat_map(|ly| {
            (0..8_u8)
                .map(move |lx| packet(key, [lx, ly], [0, 1, 2, 3], true, true, 0, (lx ^ ly) & 3))
        })
        .collect();
    // Only the first access misses; the remaining 63 are hits on the same tile.
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        100_000,
    )
    .unwrap();
    let outputs = drain(&mut emu, &packets, 20_000);
    expect_golden(&bytes, &slots, &packets, &outputs);
    assert_eq!(emu.memory().requests, 1);
    // The seam taps (7->0) must have produced real distinct words.
    let seam = golden_output(&bytes, &slots, packets[7 * 8 + 7]);
    assert_eq!(seam.texels[3], golden_word(&bytes, &slot, key, 0, 0));
}

#[test]
fn full_mip_levels_and_partial_chain_base_only() {
    let slot = support::slot(10, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    // One representative tile per level 0..=10.
    let packets: Vec<i128> = (0..=10_u8)
        .map(|n| {
            packet(
                TileKey {
                    slot: 0,
                    n,
                    x: 0,
                    y: 0,
                },
                [3, 5],
                [64, 64, 64, 319],
                true,
                true,
                n,
                0,
            )
        })
        .collect();
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        200_000,
    )
    .unwrap();
    let outputs = drain(&mut emu, &packets, 20_000);
    expect_golden(&bytes, &slots, &packets, &outputs);
    assert_eq!(emu.memory().requests, 11);

    // A partial chain only has its base level.
    let base_slot = support::slot(6, false);
    let base_slots = vec![base_slot];
    let base_bytes = support::asset(base_slot, support::pattern);
    let mut partial = CacheEmu::new(
        base_slots.clone(),
        HostMemory::new(base_bytes, 0, None),
        20_000,
    )
    .unwrap();
    let good = packet(
        TileKey {
            slot: 0,
            n: 6,
            x: 1,
            y: 1,
        },
        [0, 0],
        [511, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    assert_eq!(drain(&mut partial, &[good], 5_000).len(), 1);
    let missing = packet(
        TileKey {
            slot: 0,
            n: 5,
            x: 0,
            y: 0,
        },
        [0, 0],
        [511, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    let mut reject = CacheEmu::new(
        base_slots,
        HostMemory::new(support::asset(base_slot, support::pattern), 0, None),
        20_000,
    )
    .unwrap();
    assert!(reject
        .tick(CacheTick {
            ce: true,
            input: Some(missing),
            output_ready: true
        })
        .is_err());
    assert!(reject.faulted());
}

#[test]
fn five_keys_in_one_set_force_plru_eviction() {
    let slot = support::slot(7, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    // Same slot/level and same set (x,y low bits form the set index).
    let keys: Vec<TileKey> = [(0_u8, 0_u8), (4, 0), (8, 0), (12, 0), (0, 4)]
        .iter()
        .map(|&(x, y)| TileKey {
            slot: 0,
            n: 7,
            x,
            y,
        })
        .collect();
    assert!(keys.windows(2).all(|w| w[0].set() == w[1].set()));
    let packets: Vec<i128> = keys
        .iter()
        .enumerate()
        .map(|(i, &key)| packet(key, [0, 0], [511, 0, 0, 0], true, true, i as u8, 0))
        .collect();
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        200_000,
    )
    .unwrap();
    let outputs = drain(&mut emu, &packets, 30_000);
    expect_golden(&bytes, &slots, &packets, &outputs);
    assert_eq!(emu.memory().requests, 5, "fifth key must evict a way");
    assert!(emu.state().ready_lines <= 4);
}

#[test]
fn more_than_64_tiles_stay_correct_and_bounded() {
    let slot = support::slot(7, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let packets: Vec<i128> = (0..80_usize)
        .map(|i| {
            let key = TileKey {
                slot: 0,
                n: 7,
                x: (i % 16) as u8,
                y: (i / 16) as u8,
            };
            packet(
                key,
                [(i % 8) as u8, (i % 5) as u8],
                [0, 0, 255, 256],
                true,
                true,
                (i % 16) as u8,
                0,
            )
        })
        .collect();
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        2_000_000,
    )
    .unwrap();
    let outputs = drain(&mut emu, &packets, 500_000);
    expect_golden(&bytes, &slots, &packets, &outputs);
    assert!(emu.state().ready_lines <= 64);
}

#[test]
fn identical_repeated_pixels_are_hits() {
    let slot = support::slot(5, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let one = packet(
        TileKey {
            slot: 0,
            n: 5,
            x: 1,
            y: 1,
        },
        [2, 2],
        [127, 128, 128, 128],
        true,
        true,
        0,
        0,
    );
    let packets = vec![one; 12];
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        100_000,
    )
    .unwrap();
    let outputs = drain(&mut emu, &packets, 10_000);
    expect_golden(&bytes, &slots, &packets, &outputs);
    assert_eq!(emu.memory().requests, 1);
}

#[test]
fn ce_freezes_admission_and_holds_an_issued_read() {
    let slot = support::slot(5, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let one = packet(
        TileKey {
            slot: 0,
            n: 5,
            x: 2,
            y: 3,
        },
        [4, 1],
        [511, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        100_000,
    )
    .unwrap();
    // Run until the read is issued, then hold the enabled clock and confirm the
    // explicit read/result state is frozen but retained.
    let mut held_state = None;
    for _ in 0..40 {
        let step = emu
            .tick(CacheTick {
                ce: true,
                input: Some(one),
                output_ready: false,
            })
            .unwrap();
        if step.state.read_pending || step.state.result {
            held_state = Some(step.state.clone());
            break;
        }
    }
    let held = held_state.expect("a read must be issued");
    for _ in 0..5 {
        let step = emu
            .tick(CacheTick {
                ce: false,
                input: Some(one),
                output_ready: true,
            })
            .unwrap();
        assert!(!step.accepted && !step.transferred);
        assert_eq!(step.state.head, held.head);
        assert_eq!(step.state.result, held.result);
        assert_eq!(step.state.read_pending, held.read_pending);
        assert_eq!(step.state.pin, held.pin);
    }
    // Re-enable and drain to the independent golden.
    let mut outputs = Vec::new();
    for _ in 0..200 {
        let step = emu
            .tick(CacheTick {
                ce: true,
                input: None,
                output_ready: true,
            })
            .unwrap();
        if step.transferred {
            outputs.push(step.output.unwrap());
        }
        if emu.idle() {
            break;
        }
    }
    expect_golden(&bytes, &slots, &[one], &outputs);
}

#[test]
fn output_backpressure_holds_then_drains() {
    let slot = support::slot(6, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let packets: Vec<i128> = (0..8_u8)
        .map(|i| {
            packet(
                TileKey {
                    slot: 0,
                    n: 6,
                    x: i,
                    y: 0,
                },
                [i, 0],
                [256, 255, 0, 0],
                true,
                true,
                i,
                0,
            )
        })
        .collect();
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        100_000,
    )
    .unwrap();
    let mut outputs = Vec::new();
    let mut ptr = 0;
    // First hold the output ready low for a while.
    for _ in 0..30 {
        let input = packets.get(ptr).copied();
        let step = emu
            .tick(CacheTick {
                ce: true,
                input,
                output_ready: false,
            })
            .unwrap();
        if step.accepted {
            ptr += 1;
        }
        assert!(!step.transferred);
        if step.state.result {
            break;
        }
    }
    assert!(emu.state().result || emu.state().head);
    for _ in 0..2_000 {
        let input = packets.get(ptr).copied();
        let step = emu
            .tick(CacheTick {
                ce: true,
                input,
                output_ready: true,
            })
            .unwrap();
        if step.accepted {
            ptr += 1;
        }
        if step.transferred {
            outputs.push(step.output.unwrap());
        }
        if ptr == packets.len() && emu.idle() {
            break;
        }
    }
    expect_golden(&bytes, &slots, &packets, &outputs);
}

#[test]
fn request_backpressure_delays_acceptance_without_loss() {
    let slot = support::slot(5, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let one = packet(
        TileKey {
            slot: 0,
            n: 5,
            x: 2,
            y: 2,
        },
        [1, 6],
        [511, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 3, None),
        100_000,
    )
    .unwrap();
    let mut stalled = false;
    let mut outputs = Vec::new();
    let mut accepted = false;
    for _ in 0..2_000 {
        let input = (!accepted).then_some(one);
        let step = emu
            .tick(CacheTick {
                ce: true,
                input,
                output_ready: true,
            })
            .unwrap();
        if step.accepted {
            accepted = true;
        }
        if step.state.descriptor && !step.state.started {
            stalled = true;
        }
        if step.transferred {
            outputs.push(step.output.unwrap());
        }
        if emu.idle() {
            break;
        }
    }
    assert!(stalled, "the ready schedule must have delayed acceptance");
    expect_golden(&bytes, &slots, &[one], &outputs);
}

#[test]
fn memory_fault_drains_and_latches_terminal() {
    let slot = support::slot(5, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let one = packet(
        TileKey {
            slot: 0,
            n: 5,
            x: 2,
            y: 2,
        },
        [1, 6],
        [511, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    let mut emu = CacheEmu::new(slots, HostMemory::new(bytes, 0, Some(3)), 100_000).unwrap();
    let mut failure = None;
    for _ in 0..500 {
        if let Err(e) = emu.tick(CacheTick {
            ce: true,
            input: Some(one),
            output_ready: true,
        }) {
            failure = Some(e);
            break;
        }
    }
    assert_eq!(failure.as_deref(), Some("texture cache memory error"));
    assert!(emu.faulted());
    // The failed terminal ended the transport: the descriptor is released.
    assert!(emu.drained());
    assert_eq!(emu.state().filling_lines, 0);
    assert!(emu.output().is_none());
    assert_eq!(
        emu.tick(CacheTick::default()).unwrap_err(),
        "texture cache terminal fault; recreate before reuse"
    );
}

#[test]
fn invalidation_forces_a_new_refill_and_rebind_requires_drain() {
    let slot = support::slot(5, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let one = packet(
        TileKey {
            slot: 0,
            n: 5,
            x: 1,
            y: 1,
        },
        [0, 0],
        [511, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        100_000,
    )
    .unwrap();
    drain(&mut emu, &[one], 5_000);
    assert_eq!(emu.memory().requests, 1);
    let key = Group4::unpack72(one).unwrap().key;
    assert!(emu.invalidate(key));
    assert_eq!(emu.lookup(key), None);
    drain(&mut emu, &[one], 5_000);
    assert_eq!(emu.memory().requests, 2);
    assert!(emu.rebind(slots).is_ok());
}

#[test]
fn state_and_access_diagnostics_cover_miss_and_hit() {
    let slot = support::slot(5, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let one = packet(
        TileKey {
            slot: 0,
            n: 5,
            x: 2,
            y: 2,
        },
        [1, 6],
        [511, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    let mut emu = CacheEmu::new(slots, HostMemory::new(bytes, 0, None), 100_000).unwrap();
    let mut allocate = false;
    let mut ready = false;
    let mut hit = false;
    let mut accepted = false;
    for _ in 0..2_000 {
        let input = (!accepted).then_some(one);
        let step = emu
            .tick(CacheTick {
                ce: true,
                input,
                output_ready: true,
            })
            .unwrap();
        if step.accepted {
            accepted = true;
        }
        match step.access {
            Some(Access::Allocate { .. }) => allocate = true,
            Some(Access::Miss { .. }) => {}
            Some(Access::Filling { .. }) => {}
            Some(Access::Hit { .. }) => hit = true,
            None => {}
        }
        ready |= step.state.ready_lines == 1;
        if emu.idle() {
            break;
        }
    }
    assert!(allocate && hit && ready);
    assert_eq!(
        emu.line(emu.lookup(Group4::unpack72(one).unwrap().key).unwrap().0)
            .state,
        LineState::Ready
    );
}

#[test]
fn zero_coefficients_still_read_actual_taps() {
    let slot = support::slot(4, true);
    let slots = vec![slot];
    let bytes = support::asset(slot, support::pattern);
    let one = packet(
        TileKey {
            slot: 0,
            n: 4,
            x: 1,
            y: 1,
        },
        [3, 3],
        [0, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    let mut emu = CacheEmu::new(
        slots.clone(),
        HostMemory::new(bytes.clone(), 0, None),
        100_000,
    )
    .unwrap();
    let outputs = drain(&mut emu, &[one], 5_000);
    expect_golden(&bytes, &slots, &[one], &outputs);
    assert_ne!(outputs[0].texels, [0; 4]);
}

#[test]
fn watchdog_bounds_a_frozen_clock() {
    let slot = support::slot(5, true);
    let bytes = support::asset(slot, support::pattern);
    let mut emu = CacheEmu::new(vec![slot], HostMemory::new(bytes, 0, None), 3).unwrap();
    for _ in 0..3 {
        emu.tick(CacheTick {
            ce: false,
            input: None,
            output_ready: false,
        })
        .unwrap();
    }
    assert_eq!(
        emu.tick(CacheTick {
            ce: false,
            input: None,
            output_ready: false
        })
        .unwrap_err(),
        "texture cache wall watchdog"
    );
}

fn single_slot() -> (Slot, Vec<u8>, i128) {
    let slot = support::slot(5, true);
    let bytes = support::asset(slot, support::pattern);
    let one = packet(
        TileKey {
            slot: 0,
            n: 5,
            x: 2,
            y: 2,
        },
        [1, 6],
        [511, 0, 0, 0],
        true,
        true,
        0,
        0,
    );
    (slot, bytes, one)
}

#[test]
fn same_edge_accept_and_first_beat_matches_golden() {
    let (slot, bytes, one) = single_slot();
    let mut emu = CacheEmu::new(
        vec![slot],
        HostMemory::new_mode(bytes.clone(), 0, None, 0, true),
        100_000,
    )
    .unwrap();
    let outputs = drain(&mut emu, &[one], 5_000);
    expect_golden(&bytes, &[slot], &[one], &outputs);
    assert_eq!(emu.memory().requests, 1);
}

#[test]
fn separate_and_delayed_terminal_ack_match_golden() {
    for ack_delay in [1_u8, 2, 5] {
        let (slot, bytes, one) = single_slot();
        let mut emu = CacheEmu::new(
            vec![slot],
            HostMemory::new_mode(bytes.clone(), 0, None, ack_delay, false),
            100_000,
        )
        .unwrap();
        let outputs = drain(&mut emu, &[one], 10_000);
        expect_golden(&bytes, &[slot], &[one], &outputs);
        assert_eq!(emu.memory().requests, 1, "ack_delay={ack_delay}");
        assert_eq!(emu.memory().beats, REFILL_BEATS, "ack_delay={ack_delay}");
    }
}

#[test]
fn last_beat_is_not_an_acknowledgement() {
    let (slot, bytes, one) = single_slot();
    let mut emu = CacheEmu::new(
        vec![slot],
        HostMemory::new_mode(bytes.clone(), 0, None, 4, false),
        100_000,
    )
    .unwrap();
    // Run until all sixteen beats are delivered but the ACK has not arrived.
    let mut saw_filling_after_beats = false;
    let mut sent = false;
    for _ in 0..2_000 {
        let input = (!sent).then_some(one);
        let step = emu
            .tick(CacheTick {
                ce: true,
                input,
                output_ready: true,
            })
            .unwrap();
        if step.accepted {
            sent = true;
        }
        if emu.memory().beats >= REFILL_BEATS
            && step.state.filling_lines == 1
            && step.state.ready_lines == 0
        {
            saw_filling_after_beats = true;
        }
        if emu.idle() {
            break;
        }
    }
    assert!(
        saw_filling_after_beats,
        "the line must stay Filling until the delayed terminal ACK"
    );
    assert_eq!(
        emu.line(emu.lookup(Group4::unpack72(one).unwrap().key).unwrap().0)
            .state,
        LineState::Ready
    );
}

#[test]
fn ce_pause_every_other_edge_matches_golden() {
    let (slot, bytes, one) = single_slot();
    let mut emu =
        CacheEmu::new(vec![slot], HostMemory::new(bytes.clone(), 0, None), 100_000).unwrap();
    let outputs = drain_scheduled(&mut emu, &[one], 10_000, |edge, _| edge % 2 == 0);
    expect_golden(&bytes, &[slot], &[one], &outputs);
}

#[test]
fn ce_pauses_on_every_live_state_matches_golden() {
    let (slot, bytes, one) = single_slot();
    let mut emu =
        CacheEmu::new(vec![slot], HostMemory::new(bytes.clone(), 0, None), 100_000).unwrap();
    // Pause the enabled clock for two edges whenever a head, read, result or
    // descriptor is live, so the pause lands in every intermediate state.
    let mut paused = 0_u32;
    let outputs = drain_scheduled(&mut emu, &[one], 10_000, |_, state| {
        let live = state.head || state.read_pending || state.result || state.descriptor;
        if live && paused < 2 {
            paused += 1;
            false
        } else {
            paused = 0;
            true
        }
    });
    expect_golden(&bytes, &[slot], &[one], &outputs);
}

#[test]
fn abort_before_presentation_releases_and_never_publishes() {
    let (slot, bytes, one) = single_slot();
    let mut emu =
        CacheEmu::new(vec![slot], HostMemory::new(bytes.clone(), 0, None), 100_000).unwrap();
    // First edge admits the head; second edge allocates a not-yet-presented
    // descriptor. Abort now must release it because no request was offered.
    emu.tick(CacheTick {
        ce: true,
        input: Some(one),
        output_ready: true,
    })
    .unwrap();
    emu.tick(CacheTick {
        ce: true,
        input: None,
        output_ready: true,
    })
    .unwrap();
    assert!(emu.state().descriptor);
    assert!(!emu.state().presented);
    emu.abort();
    assert!(emu.drained());
    assert!(emu.output().is_none());
    assert_eq!(emu.state().ready_lines, 0);
    assert_eq!(emu.state().filling_lines, 0);
    let drain = emu.drain_tick(false).unwrap();
    assert!(drain.request.is_none());
    assert!(drain.drained);
}

#[test]
fn abort_while_held_request_drains_after_acceptance() {
    let (slot, bytes, one) = single_slot();
    // Ready low on the first few edges, so the presented request is held.
    let mut emu =
        CacheEmu::new(vec![slot], HostMemory::new(bytes.clone(), 2, None), 100_000).unwrap();
    // Admit and allocate, then present the request without acceptance.
    emu.tick(CacheTick {
        ce: true,
        input: Some(one),
        output_ready: true,
    })
    .unwrap();
    emu.tick(CacheTick {
        ce: true,
        input: None,
        output_ready: true,
    })
    .unwrap();
    // Offer once (not ready) so the descriptor is presented.
    let mut presented = false;
    for _ in 0..20 {
        let step = emu
            .tick(CacheTick {
                ce: true,
                input: None,
                output_ready: true,
            })
            .unwrap();
        if step.state.presented && !step.state.started {
            presented = true;
            break;
        }
    }
    assert!(presented, "the request must have been presented");
    emu.abort();
    assert!(!emu.drained(), "a presented request must be held");
    // Drain the accepted burst to its terminal event under a frozen CE.
    let mut terminal = false;
    for _ in 0..10_000 {
        let step = emu.drain_tick(false).unwrap();
        if step.response.complete.is_some() {
            terminal = true;
            break;
        }
    }
    assert!(terminal, "the accepted refill must terminate");
    assert!(emu.drained());
    assert!(emu.output().is_none());
    assert_eq!(emu.state().ready_lines, 0);
    assert_eq!(emu.state().filling_lines, 0);
}

#[test]
fn abort_mid_accepted_read_under_ce0_drains() {
    let (slot, bytes, one) = single_slot();
    let mut emu =
        CacheEmu::new(vec![slot], HostMemory::new(bytes.clone(), 0, None), 100_000).unwrap();
    // Run until the request is accepted and some beats have arrived.
    let mut accepted = false;
    let mut sent = false;
    for _ in 0..2_000 {
        let input = (!sent).then_some(one);
        let step = emu
            .tick(CacheTick {
                ce: true,
                input,
                output_ready: true,
            })
            .unwrap();
        if step.accepted {
            sent = true;
        }
        if step.state.started {
            accepted = true;
            break;
        }
    }
    assert!(accepted);
    emu.abort();
    assert!(!emu.drained());
    let mut terminal = false;
    for _ in 0..10_000 {
        let step = emu.drain_tick(false).unwrap();
        if step.response.complete.is_some() {
            terminal = true;
            break;
        }
    }
    assert!(terminal);
    assert!(emu.drained());
    assert!(emu.output().is_none());
    assert_eq!(emu.state().ready_lines, 0);
    assert_eq!(emu.state().filling_lines, 0);
}

#[test]
fn fatal_adapter_error_is_abandoned_not_retried() {
    let (slot, _bytes, one) = single_slot();
    let mut emu = CacheEmu::new(vec![slot], FatalMemory { calls: 0 }, 100_000).unwrap();
    let mut failure = None;
    for _ in 0..200 {
        if let Err(e) = emu.tick(CacheTick {
            ce: true,
            input: Some(one),
            output_ready: true,
        }) {
            failure = Some(e);
            break;
        }
    }
    assert_eq!(failure.as_deref(), Some("test adapter fatal"));
    assert!(emu.faulted());
    // Unknown external ownership: the ambiguous request is not retried.
    assert!(emu.drained());
    assert!(emu.output().is_none());
}

fn run_to_fault<M: MemoryPort>(emu: &mut CacheEmu<M>, one: i128, max_edges: u64) -> Option<String> {
    let mut sent = false;
    for _ in 0..max_edges {
        let input = (!sent).then_some(one);
        match emu.tick(CacheTick {
            ce: true,
            input,
            output_ready: true,
        }) {
            Ok(step) => {
                if step.accepted {
                    sent = true;
                }
                if emu.idle() {
                    return None;
                }
            }
            Err(error) => return Some(error),
        }
    }
    None
}

fn protocol_fault(inject: impl FnOnce(&mut ProtocolMemory), expected: &str) {
    let (slot, bytes, one) = single_slot();
    let mut memory = ProtocolMemory::new(bytes);
    inject(&mut memory);
    let mut emu = CacheEmu::new(vec![slot], memory, 100_000).unwrap();
    assert_eq!(
        run_to_fault(&mut emu, one, 2_000).as_deref(),
        Some(expected)
    );
    assert!(emu.faulted());
    assert!(emu.output().is_none());
    assert_eq!(emu.state().ready_lines, 0);
    assert_eq!(emu.state().filling_lines, 0);
}

#[test]
fn unsolicited_accept_faults() {
    protocol_fault(
        |m| {
            m.inject_first = Some(Response {
                accepted: true,
                ..Response::default()
            })
        },
        "texture cache unsolicited refill acceptance",
    );
}

#[test]
fn beat_without_descriptor_faults() {
    protocol_fault(
        |m| {
            m.inject_first = Some(Response {
                read: Some((0, 0)),
                ..Response::default()
            })
        },
        "texture cache beat without descriptor",
    );
}

#[test]
fn completion_without_descriptor_faults() {
    protocol_fault(
        |m| {
            m.inject_first = Some(Response {
                complete: Some(true),
                ..Response::default()
            })
        },
        "texture cache completion without descriptor",
    );
}

#[test]
fn out_of_order_beat_faults() {
    protocol_fault(
        |m| m.bad_beat = Some(2),
        "texture cache refill beat identity/order",
    );
}

#[test]
fn malformed_beat_index_faults() {
    protocol_fault(
        |m| m.malformed_beat = true,
        "texture cache refill beat identity/order",
    );
}

#[test]
fn last_beat_with_terminal_still_validates_index() {
    protocol_fault(
        |m| m.bad_beat = Some(15),
        "texture cache refill beat identity/order",
    );
}

#[test]
fn malformed_terminal_releases_transport_on_that_same_edge() {
    let (slot, bytes, one) = single_slot();
    let mut memory = ProtocolMemory::new(bytes);
    memory.bad_beat = Some(15);
    let mut emu = CacheEmu::new(vec![slot], memory, 2_000).unwrap();
    assert_eq!(
        run_to_fault(&mut emu, one, 2_000).as_deref(),
        Some("texture cache refill beat identity/order")
    );
    assert!(
        emu.drained(),
        "terminal must be consumed even with a malformed beat"
    );
    let state = emu.state();
    assert!(!state.head && !state.read_pending && !state.result);
    assert_eq!(state.ready_lines + state.filling_lines, 0);
    assert!(emu.drain_tick(false).unwrap().request.is_none());
}

#[test]
fn early_success_faults() {
    protocol_fault(
        |m| m.early_success = true,
        "texture cache early refill completion",
    );
}

#[test]
fn duplicate_accept_faults() {
    protocol_fault(
        |m| m.duplicate_accept = true,
        "texture cache duplicate refill acceptance",
    );
}

#[test]
fn output_and_input_ready_fanout_do_not_advance() {
    let (slot, bytes, _one) = single_slot();
    let emu = CacheEmu::new(vec![slot], HostMemory::new(bytes.clone(), 0, None), 100_000).unwrap();
    let wall = emu.state().wall;
    assert!(emu.input_ready(true));
    assert!(!emu.input_ready(false));
    assert!(emu.output().is_none());
    // Fanout reads must not touch the machine.
    assert_eq!(emu.state().wall, wall);
}
