//! External stimulus and independent integer framebuffer golden, not GPU storage.
use gpu_v2::{
    framebuffer::ports as fb, lighting::ports::LightingOutput, memory::ports::MemoryPort,
    system::pixel::*,
};
use std::collections::VecDeque;

pub fn context() -> Context {
    Context {
        surface: fb::MaterializedSurface {
            color_base_bytes: 512,
            depth_base_bytes: 12288,
            width: 160,
            height: 32,
        },
        rop: fb::Context {
            depth: fb::DepthFunc::LessEqual,
            depth_write: true,
            blend: fb::Blend::SrcOver,
        },
        specular: [17, 93, 203],
        alpha: 97,
    }
}
pub fn image() -> Vec<u8> {
    (0..24576)
        .map(|i| ((i * 29 + 51) ^ (i >> 5)) as u8)
        .collect()
}
#[derive(Clone)]
pub struct Stimulus {
    pub quad: QuadInput,
    pub light: [LightingOutput; 4],
    pub sample: [[u8; 3]; 4],
}
pub fn synthetic(count: usize) -> Vec<Stimulus> {
    (0..count)
        .map(|i| {
            let tile = if i < 40 { i % 20 } else { 0 };
            Stimulus {
                quad: QuadInput {
                    header: fb::Header {
                        x: (tile % 10 * 16) as u16,
                        y: (tile / 10 * 16) as u8,
                        mask: [0, 1, 6, 15, 5][i % 5],
                    },
                    basic: std::array::from_fn(|lane| Basic {
                        tint: [
                            (i * 17 + lane * 7) as u8,
                            (i * 31 + lane * 19) as u8,
                            (i * 5 + lane * 47) as u8,
                        ],
                        depth: (40000 - i * 143 - lane * 37) as u16,
                    }),
                    default_light: i & 1 != 0,
                    default_sample: i & 2 != 0,
                },
                light: std::array::from_fn(|lane| LightingOutput {
                    g: ((i * 41 + lane * 97) % 512) as u16,
                    h: ((i * 23 + lane * 17) % 257) as u16,
                }),
                sample: std::array::from_fn(|lane| {
                    [
                        (i * 43 + lane * 37) as u8,
                        (i * 19 + lane * 61) as u8,
                        (i * 7 + lane * 53) as u8,
                    ]
                }),
            }
        })
        .collect()
}
pub fn nearest(n: u64, d: u64) -> u64 {
    let q = n / d;
    let r = n % d;
    q + u64::from(2 * r > d || 2 * r == d && q & 1 != 0)
}
pub fn rgb(tint: [u8; 3], texture: [u8; 3], light: LightingOutput, cs: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|i| {
        let base = nearest(u64::from(tint[i]) * u64::from(texture[i]), 255);
        nearest(
            base * u64::from(light.g) + u64::from(cs[i]) * u64::from(light.h),
            256,
        )
        .min(255) as u8
    })
}
/// All byte addresses, depth/blend and quantization are derived separately.
/// No DUT final fast division, bank mapping or framebuffer oracle is called.
pub fn golden(mut bytes: Vec<u8>, inputs: &[Stimulus], context: Context) -> Vec<u8> {
    for s in inputs {
        for lane in 0..4 {
            if s.quad.header.mask & (1 << lane) == 0 {
                continue;
            }
            let x = usize::from(s.quad.header.x) + lane % 2;
            let y = usize::from(s.quad.header.y) + lane / 2;
            let tile = (y / 16) * (usize::from(context.surface.width) / 16) + x / 16;
            let offset = tile * 512 + ((y % 16) * 16 + x % 16) * 2;
            let ca = context.surface.color_base_bytes as usize + offset;
            let da = context.surface.depth_base_bytes as usize + offset;
            let old_c = u16::from_le_bytes(bytes[ca..ca + 2].try_into().unwrap());
            let old_d = u16::from_le_bytes(bytes[da..da + 2].try_into().unwrap());
            let basic = s.quad.basic[lane];
            let pass = match context.rop.depth {
                fb::DepthFunc::Never => false,
                fb::DepthFunc::Less => basic.depth < old_d,
                fb::DepthFunc::Equal => basic.depth == old_d,
                fb::DepthFunc::LessEqual => basic.depth <= old_d,
                fb::DepthFunc::Greater => basic.depth > old_d,
                fb::DepthFunc::NotEqual => basic.depth != old_d,
                fb::DepthFunc::GreaterEqual => basic.depth >= old_d,
                fb::DepthFunc::Always => true,
            };
            if !pass {
                continue;
            }
            let light = if s.quad.default_light {
                LightingOutput { g: 256, h: 0 }
            } else {
                s.light[lane]
            };
            let texture = if s.quad.default_sample {
                [255; 3]
            } else {
                s.sample[lane]
            };
            let source = rgb(basic.tint, texture, light, context.specular);
            let codes = [old_c / 2048, old_c / 32 % 64, old_c % 32];
            let mut color = 0;
            for i in 0..3 {
                let bits = if i == 1 { 6 } else { 5 };
                let dest = u64::from(codes[i]) * (1 << (8 - bits))
                    + u64::from(codes[i]) / (1 << (2 * bits - 8));
                let value = if context.rop.blend == fb::Blend::Replace {
                    u64::from(source[i])
                } else {
                    nearest(
                        u64::from(source[i]) * u64::from(context.alpha)
                            + dest * (255 - u64::from(context.alpha)),
                        255,
                    )
                };
                color |= (nearest(value * ((1 << bits) - 1), 255) as u16) << [11, 5, 0][i];
            }
            bytes[ca..ca + 2].copy_from_slice(&color.to_le_bytes());
            if context.rop.depth_write {
                bytes[da..da + 2].copy_from_slice(&basic.depth.to_le_bytes());
            }
        }
    }
    bytes
}
#[derive(Default)]
pub struct Proof {
    pub retired: Vec<Ticket>,
    pub output_stall: bool,
    pub ce_memory_return: bool,
    pub ce_local_return: bool,
    pub out_of_order: bool,
    pub captured_before_commit: bool,
    pub read_edges: u64,
    pub mc_reads: u64,
    pub mc_writes: u64,
    pub mc_read_beats: u64,
    pub mc_write_beats: u64,
    pub mc_completions: u64,
}
pub fn replay<M: MemoryPort>(
    model: &mut Model,
    memory: &mut M,
    inputs: &[Stimulus],
    pauses: bool,
    max_steps: u64,
) -> Proof {
    let mut next = 0;
    let mut light = VecDeque::new();
    let mut sample = VecDeque::new();
    let mut proof = Proof::default();
    let mut previous = model.snapshot();
    let mut done = [(0_u8, 0_u8, 0_u8, 0_u8); 16];
    for wall in 0..max_steps {
        // Fill all global credits before releasing the oldest nondefault result.
        // Host queues are stimuli, not a queue hidden in the GPU model.
        let release = model.stats.admitted >= 16 || next == inputs.len();
        let tick = Tick {
            ce: !pauses || wall % 11 > 2,
            final_ready: !pauses || wall > 240 && wall % 37 > 10,
            quad: inputs.get(next).map(|s| s.quad),
            light: release.then(|| light.back().copied()).flatten(),
            sample: release.then(|| sample.front().copied()).flatten(),
            finish: next == inputs.len(),
        };
        let t = model.step(tick, memory).unwrap();
        assert!(!t.events.iter().any(|e| matches!(e, Event::Rejected(_))));
        // Consume precisely the offered item before enqueuing a new admission;
        // pushing to the LIFO light queue first could pop the wrong allocation.
        if t.light_accepted {
            let w = light.pop_back().unwrap();
            proof.out_of_order |= w.key.ticket.serial > proof.retired.len() as u64;
        }
        if t.sample_accepted {
            sample.pop_front();
        }
        if t.quad_accepted {
            let s = &inputs[next];
            if let Some(ticket) = t.ticket {
                for lane in (0..4).rev() {
                    if s.quad.header.mask >> lane & 1 == 0 {
                        continue;
                    }
                    let key = PixelKey { ticket, lane };
                    if !s.quad.default_light {
                        light.push_back(LightWrite {
                            key,
                            value: s.light[usize::from(lane)],
                        });
                    }
                    if !s.quad.default_sample {
                        sample.push_back(SampleWrite {
                            key,
                            rgb: s.sample[usize::from(lane)],
                        });
                    }
                }
            }
            next += 1;
        }
        for e in &t.events {
            match e {
                Event::Admitted(ticket) => {
                    let q = inputs[next - 1].quad;
                    done[usize::from(ticket.quad)] = (
                        q.header.mask,
                        0,
                        if q.default_light { q.header.mask } else { 0 },
                        if q.default_sample { q.header.mask } else { 0 },
                    );
                }
                Event::Joined(ticket) => {
                    let (mask, basic, light, sample) = done[usize::from(ticket.quad)];
                    assert_eq!(mask, basic & light & sample, "join before previous done");
                }
                Event::BasicDone(key) | Event::LightDone(key) | Event::SampleDone(key) => {
                    let (store, address) = match e {
                        Event::BasicDone(_) => {
                            (Store::Basic, key.ticket.quad * 8 + key.lane * 2 + 1)
                        }
                        Event::LightDone(_) => (Store::Light, key.ticket.quad * 4 + key.lane),
                        _ => (Store::Sample, key.ticket.quad * 4 + key.lane),
                    };
                    assert!(
                        t.accesses
                            .iter()
                            .any(|a| a.store == store && a.write && a.address == address),
                        "done without payload write"
                    );
                    let d = &mut done[usize::from(key.ticket.quad)];
                    match store {
                        Store::Basic => d.1 |= 1 << key.lane,
                        Store::Light => d.2 |= 1 << key.lane,
                        Store::Sample => d.3 |= 1 << key.lane,
                    }
                }
                Event::Retired(ticket) => {
                    assert_eq!(ticket.serial, proof.retired.len() as u64);
                    proof.retired.push(*ticket);
                }
                Event::ReadIssued { .. } => {
                    assert!(!previous.read_pending && !previous.return_valid);
                    // The preceding held output can leave on this edge. The
                    // read reserves a distinct return position for next edge,
                    // using an actual transfer, never a predicted ready credit.
                    assert!(!previous.output_valid || t.framebuffer.input_accepted);
                    proof.read_edges += 1;
                }
                Event::Captured { key, .. } => {
                    assert!(
                        !t.events
                            .iter()
                            .any(|e| matches!(e, Event::Consumed { key: k, .. } if k == key)),
                        "capture-to-consume bypass"
                    );
                    proof.ce_local_return |= !tick.ce;
                }
                _ => {}
            }
        }
        for (i, a) in t.accesses.iter().enumerate() {
            assert!(usize::from(a.address) < if a.store == Store::Basic { 128 } else { 64 });
            for b in &t.accesses[..i] {
                if a.store == b.store {
                    assert_ne!(a.write, b.write, "store port overbook");
                    assert_ne!(a.address, b.address, "same-row collision");
                }
            }
        }
        if t.framebuffer.response.accepted {
            if t.framebuffer.request.unwrap().write {
                proof.mc_writes += 1;
            } else {
                proof.mc_reads += 1;
            }
        }
        proof.mc_read_beats += u64::from(t.framebuffer.response.read.is_some());
        proof.mc_write_beats += u64::from(t.framebuffer.response.write_accepted);
        proof.mc_completions += u64::from(t.framebuffer.response.complete.is_some());
        proof.ce_memory_return |= !tick.ce && t.framebuffer.response.read.is_some();
        proof.captured_before_commit |=
            t.framebuffer.payload_captured.is_some() && t.framebuffer.committed.is_none();
        proof.output_stall |= model.stats.output_stalls > 0;
        previous = t.snapshot;
        if model.complete() {
            assert_eq!(next, inputs.len());
            assert!(light.is_empty() && sample.is_empty());
            assert_eq!(t.snapshot.live, 0);
            return proof;
        }
    }
    panic!("pixel replay did not finish within {max_steps} wall clocks");
}
