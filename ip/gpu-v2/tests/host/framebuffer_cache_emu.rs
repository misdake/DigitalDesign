//! Actual registered ROP/cache: independent byte golden and strict transport.
use gpu_v2::framebuffer::{
    emu::{FramebufferEmu, OutputRow},
    ports::*,
    sim::fixture::Fixture,
};

#[test]
fn owned_inventory_includes_actual_leaf_and_full_rop_phase() {
    use gpu_v2::framebuffer::{arithmetic, emu};
    let leaf = arithmetic::timed::audit(arithmetic::RTL_SOURCE).unwrap();
    assert_eq!(
        emu::LEAF_REGISTER_BITS,
        leaf.resources.pipeline_bits as usize
    );
    assert_eq!(emu::ROP_STATE_BITS, 525);
    assert_eq!(emu::TOTAL_LOGICAL_BITS, 67_767);
}

struct StrictMemory {
    inner: Fixture,
    held: Option<Request>,
    active: Option<(Request, u8)>,
    held_word: Option<u64>,
    simultaneous_ack: bool,
    last_write: u64,
    writes: u64,
}
impl StrictMemory {
    fn new(bytes: Vec<u8>, simultaneous_ack: bool) -> Self {
        Self {
            inner: Fixture::new(bytes),
            held: None,
            active: None,
            held_word: None,
            simultaneous_ack,
            last_write: 0,
            writes: 0,
        }
    }
}
impl MemoryPort for StrictMemory {
    fn cycle(&mut self, request: Option<Request>, write: Option<u64>) -> Result<Response, String> {
        if let Some(held) = self.held {
            assert_eq!(request, Some(held));
        }
        if let Some((q, n)) = self.active {
            assert!(request.is_none(), "request repeated after acceptance");
            if q.write && n < 16 {
                assert!(write.is_some(), "producer inserted write bubble");
            }
        }
        if let Some(word) = self.held_word {
            assert_eq!(write, Some(word), "stalled skid changed");
        }
        let mut response = self.inner.cycle(request, write)?;
        if response.accepted {
            let q = request.unwrap();
            assert!(self.active.is_none());
            if q.write {
                assert!(
                    write.is_some(),
                    "write admitted without prefetched beat zero"
                );
            }
            self.active = Some((q, 0));
            self.held = None;
        } else if request.is_some() {
            self.held = request;
        }
        self.held_word = None;
        if let Some((q, n)) = self.active.as_mut() {
            if response.write_accepted || response.read.is_some() {
                *n += 1;
                if q.write {
                    self.last_write = self.inner.cycle;
                    self.writes += 1;
                }
            }
            if q.write && *n < 16 && !response.write_accepted {
                self.held_word = write;
            }
            if self.simultaneous_ack && *n == 16 && response.complete.is_none() {
                // Fixture owns a delayed ACK; consume it privately, without a
                // second DUT clock. This adapter presents terminal ACK with beat16.
                loop {
                    let terminal = self.inner.cycle(None, None)?;
                    if terminal.complete.is_some() {
                        response.complete = terminal.complete;
                        break;
                    }
                    assert!(self.inner.cycle < 2_000_000);
                }
            }
        }
        if response.complete.is_some() {
            self.active = None;
            self.held_word = None;
        }
        Ok(response)
    }
}

fn run(
    quads: &[Quad],
    ctx: Context,
    mut mem: StrictMemory,
    ce_pause: bool,
) -> (FramebufferEmu, StrictMemory) {
    let mut model = FramebufferEmu::new(surface(), ctx, 100_000).unwrap();
    let mut cursor = 0;
    let mut flush = false;
    let mut issues = 0;
    let mut returns = 0;
    for wall in 0..100_000 {
        let row = quads.get(cursor / 8).map(|q| OutputRow {
            header: q.header,
            row: (cursor % 8) as u8,
            data: q.rows()[cursor % 8],
        });
        let t = model
            .step(!ce_pause || wall % 11 < 8, row, &mut mem)
            .unwrap();
        if t.input_accepted {
            cursor += 1;
        }
        issues += usize::from(t.arithmetic_issue.is_some());
        returns += usize::from(t.arithmetic_return.is_some());
        for a in &t.accesses {
            assert_eq!(
                t.accesses
                    .iter()
                    .filter(|b| a.bank == b.bank && a.port == b.port)
                    .count(),
                1
            );
        }
        if cursor == quads.len() * 8 && !flush {
            model.request_flush().unwrap();
            flush = true;
        }
        if model.flush_complete {
            assert!(model.idle() && mem.inner.idle());
            assert_eq!(issues, quads.len() * 4);
            assert_eq!(returns, issues);
            return (model, mem);
        }
    }
    panic!("actual cache wall watchdog");
}

#[test]
fn real_leaf_full_image_eviction_stalls_and_terminal_ack() {
    let quads: Vec<_> = (0..60)
        .map(|i| {
            quad(
                (i % 10) * 16,
                ((i / 10 % 2) * 16) as u8,
                [1, 5, 15][(i / 20) as usize],
                i as u8,
            )
        })
        .collect();
    for simultaneous in [false, true] {
        for blend in [Blend::Replace, Blend::SrcOver] {
            let c = Context { blend, ..context() };
            let expected = golden(image(), &quads, surface(), c);
            let mut mem = StrictMemory::new(image(), simultaneous);
            mem.inner.request_period = 5;
            mem.inner.beat_period = 3;
            mem.inner.ack_delay = 11;
            let (m, mem) = run(&quads, c, mem, true);
            assert_eq!(mem.inner.bytes, expected);
            assert_eq!(m.stats.quads, 60);
            assert!(m.stats.misses > 8 && mem.writes > 0);
            assert!(m.dirty().iter().all(|x| *x == [false; 2]));
        }
    }
}

#[test]
fn every_depth_mode_zero_alpha_and_context_ownership() {
    for depth in [
        DepthFunc::Never,
        DepthFunc::Less,
        DepthFunc::Equal,
        DepthFunc::LessEqual,
        DepthFunc::Greater,
        DepthFunc::NotEqual,
        DepthFunc::GreaterEqual,
        DepthFunc::Always,
    ] {
        for write in [false, true] {
            let mut quads = vec![quad(2, 2, 5, 57), quad(2, 2, 15, 99)];
            quads[1].pixels.iter_mut().for_each(|p| p.rgba[3] = 0);
            let c = Context {
                depth,
                depth_write: write,
                ..context()
            };
            let expected = golden(image(), &quads, surface(), c);
            let (mut m, mem) = run(&quads, c, StrictMemory::new(image(), false), false);
            assert_eq!(mem.inner.bytes, expected);
            m.set_context(context()).unwrap();
            let row = OutputRow {
                header: quads[0].header,
                row: 0,
                data: quads[0].rows()[0],
            };
            let mut mem = mem;
            assert!(m.step(true, Some(row), &mut mem).unwrap().input_accepted);
            assert!(m.set_context(c).is_err());
        }
    }
}

#[test]
fn abort_and_failed_ack_drain_with_compute_frozen() {
    for write in [false, true] {
        for fail in [false, true] {
            let mut m = FramebufferEmu::new(surface(), context(), 10_000).unwrap();
            let mut mem = StrictMemory::new(image(), false);
            mem.inner.ack_delay = 19;
            if fail {
                mem.inner.fail_request = Some(if write { 9 } else { 1 });
            }
            let q = quad(0, 0, 15, 17);
            let mut cursor = 0;
            let mut flush = false;
            let mut aborted = false;
            for _ in 0..10_000 {
                let row = (cursor < 8).then(|| OutputRow {
                    header: q.header,
                    row: cursor as u8,
                    data: q.rows()[cursor],
                });
                let t = m.step(!aborted, row, &mut mem).unwrap();
                if t.input_accepted {
                    cursor += 1;
                }
                if cursor == 8 && !flush {
                    m.request_flush().unwrap();
                    flush = true;
                }
                if !fail && !aborted && t.response.accepted && t.request.unwrap().write == write {
                    m.abort();
                    aborted = true;
                }
                if m.drained() {
                    break;
                }
            }
            assert!(m.drained() && mem.inner.idle());
            assert!(!m.flush_complete);
            assert!(m.request_flush().is_err());
            if !write {
                assert!(m.tags().iter().all(Option::is_none));
            } else {
                assert!(m.dirty()[0][0]);
            }
        }
    }
}
fn surface() -> MaterializedSurface {
    MaterializedSurface {
        color_base_bytes: 0,
        depth_base_bytes: 16384,
        width: 160,
        height: 32,
    }
}
fn context() -> Context {
    Context {
        depth: DepthFunc::Always,
        depth_write: true,
        blend: Blend::SrcOver,
    }
}
fn quad(x: u16, y: u8, mask: u8, n: u8) -> Quad {
    Quad {
        header: Header { x, y, mask },
        pixels: std::array::from_fn(|i| Fragment {
            rgba: [n, 80 + i as u8 * 31, 255 - n, 97],
            depth: 1000 + u16::from(n) + i as u16,
        }),
    }
}
fn image() -> Vec<u8> {
    (0..32768)
        .map(|i| ((i * 71 + 19) ^ (i >> 3)) as u8)
        .collect()
}
// Independent real-number reference, separate address derivation and quantizer.
fn golden(mut bytes: Vec<u8>, qs: &[Quad], s: MaterializedSurface, c: Context) -> Vec<u8> {
    for q in qs {
        for lane in 0..4 {
            if q.header.mask & (1 << lane) == 0 {
                continue;
            }
            let x = q.header.x as usize + lane % 2;
            let y = q.header.y as usize + lane / 2;
            let offset =
                (((y / 16) * (s.width as usize / 16) + x / 16) * 256 + (y % 16) * 16 + x % 16) * 2;
            let ca = s.color_base_bytes as usize + offset;
            let da = s.depth_base_bytes as usize + offset;
            let old = u16::from_le_bytes([bytes[ca], bytes[ca + 1]]);
            let depth = u16::from_le_bytes([bytes[da], bytes[da + 1]]);
            let f = q.pixels[lane];
            let comparisons = [
                false,
                f.depth < depth,
                f.depth == depth,
                f.depth <= depth,
                f.depth > depth,
                f.depth != depth,
                f.depth >= depth,
                true,
            ];
            if !comparisons[c.depth as usize] {
                continue;
            }
            let codes = [
                (old / 2048) as u32,
                ((old / 32) % 64) as u32,
                (old % 32) as u32,
            ];
            let mut out = 0u16;
            for i in 0..3 {
                let bits = if i == 1 { 6 } else { 5 };
                let m = (1u32 << bits) - 1;
                let d = ((codes[i] << (8 - bits)) + (codes[i] >> (2 * bits - 8))) as f64;
                let v = if c.blend == Blend::Replace {
                    f.rgba[i] as f64
                } else {
                    (f.rgba[3] as f64 / 255.0 * f.rgba[i] as f64
                        + (1.0 - f.rgba[3] as f64 / 255.0) * d)
                        .round()
                };
                let quant = (v * m as f64 / 255.0).round() as u16;
                out |= quant << [11, 5, 0][i];
            }
            bytes[ca..ca + 2].copy_from_slice(&out.to_le_bytes());
            if c.depth_write {
                bytes[da..da + 2].copy_from_slice(&f.depth.to_le_bytes());
            }
        }
    }
    bytes
}
