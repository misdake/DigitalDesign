//! Two separate controlled memory fixtures: not shared-MC integration.
use gpu_v2::{
    framebuffer::{ports as fb, sim::fixture::Fixture},
    lighting::{ports as light, sim::oracle as light_oracle},
    system::pixel::*,
    texture::{ports as tex, sim::oracle as texture_oracle},
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[allow(dead_code)]
#[path = "support/pixel.rs"]
mod support;
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod texture_support;

const MAX: u64 = 400_000;
struct TextureFixture {
    bytes: Vec<u8>,
    queue: VecDeque<(u64, u64)>,
    active: Option<(u64, u64, u8, u64)>,
    wall: u64,
    next: u64,
    requests: u64,
    beats: u64,
    completions: u64,
    fail: bool,
}
impl TextureFixture {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            queue: VecDeque::new(),
            active: None,
            wall: 0,
            next: 0,
            requests: 0,
            beats: 0,
            completions: 0,
            fail: false,
        }
    }
    fn idle(&self) -> bool {
        self.queue.is_empty() && self.active.is_none()
    }
}
impl tex::RefillPort for TextureFixture {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        if self.fail {
            return Err("controlled texture submit failure".into());
        }
        if bytes != 128
            || !address.is_multiple_of(128)
            || address < u64::from(texture_support::BASE)
            || address - u64::from(texture_support::BASE) + 128 > self.bytes.len() as u64
        {
            return Err("texture fixture address".into());
        }
        assert!(self.queue.len() < 4);
        let id = self.next;
        self.next += 1;
        self.requests += 1;
        self.queue.push_back((id, address));
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<tex::RefillEvent>, String> {
        if self.fail {
            return Err("controlled texture transport failure".into());
        }
        self.wall += 1;
        if let Some((id, address, index, due)) = self.active.as_mut() {
            if *index < 16 && self.wall.is_multiple_of(3) {
                let offset = (*address - u64::from(texture_support::BASE)) as usize
                    + usize::from(*index) * 8;
                let event = tex::RefillEvent::Beat {
                    id: *id,
                    index: usize::from(*index),
                    data: u64::from_le_bytes(self.bytes[offset..offset + 8].try_into().unwrap()),
                    last: *index == 15,
                };
                *index += 1;
                self.beats += 1;
                if *index == 16 {
                    *due = self.wall + 11;
                }
                return Ok(vec![event]);
            }
            if *index == 16 && self.wall >= *due {
                let id = *id;
                self.active = None;
                self.completions += 1;
                return Ok(vec![tex::RefillEvent::Complete { id }]);
            }
        } else if let Some((id, address)) = self.queue.pop_front() {
            self.active = Some((id, address, 0, 0));
            return Ok(vec![tex::RefillEvent::Started { id }]);
        }
        Ok(vec![])
    }
}
fn lighting(shininess: u8) -> light::LightingContext {
    light::LightingContext {
        material: light::Material {
            unlit: false,
            specular_color: if shininess == 16 {
                [73, 149, 231]
            } else {
                [17, 93, 203]
            },
            shininess_code: shininess,
        },
        light: light::Light {
            direction: if shininess == 16 {
                [9830, 0, 13107]
            } else {
                [0, 0, 16384]
            },
            ambient: if shininess == 16 { 16 } else { 43 },
            directional: if shininess == 16 { 240 } else { 217 },
        },
        projection: light::Projection::default(),
        epoch: 19,
    }
}
fn quads(count: usize) -> Vec<BranchQuad> {
    (0..count)
        .map(|i| {
            let mut q = texture_support::input(
                5,
                [
                    tex::Filter::Nearest,
                    tex::Filter::Bilinear,
                    tex::Filter::Trilinear,
                ][i % 3],
                [-0.13 + (i % 9) as f64 / 11.0, 0.07 + (i % 5) as f64 / 9.0],
            );
            let mask = if i % 13 == 12 {
                0
            } else if i % 5 == 4 {
                5
            } else {
                15
            };
            q.mask = mask;
            q.uv[1][0] += 1.0 / 32.0;
            q.uv[2][1] += 1.0 / 32.0;
            q.uv[3][0] += 1.0 / 32.0;
            q.uv[3][1] += 1.0 / 32.0;
            q.lod_bias = if i % 3 == 2 { 0.5 } else { 0.0 };
            BranchQuad {
                live: LiveQuad {
                    quad: QuadInput {
                        header: fb::Header {
                            x: ((i % 8) * 16) as u16,
                            y: ((i / 8 % 2) * 16) as u8,
                            mask,
                        },
                        basic: std::array::from_fn(|lane| Basic {
                            tint: [
                                (17 + i * 31 + lane * 19) as u8,
                                (99 + i * 7 + lane * 43) as u8,
                                (193 + i * 13 + lane * 11) as u8,
                            ],
                            depth: (45000 - i * 311 - lane * 71) as u16,
                        }),
                        default_light: (i % 4) & 1 != 0,
                        default_sample: (i % 4) & 2 != 0,
                    },
                    light: std::array::from_fn(|lane| light::PixelInput {
                        normal: [
                            [0, 0, 0],
                            [0, 0, 16384],
                            [-730, 2119, -7133],
                            [32767, -32768, 16384],
                        ][(i + lane) % 4],
                        ndc: [
                            (((i % 3) as i32 - 1) * 32768) / 4,
                            ((lane as i32 - 2) * 16384) / 4,
                        ],
                    }),
                },
                sample: Some(q),
            }
        })
        .collect()
}
fn reference_inputs(
    qs: &[BranchQuad],
    light: light::LightingContext,
    slot: tex::Slot,
    asset: &[u8],
) -> Vec<support::Stimulus> {
    let mut cache = texture_oracle::Cache::new(vec![slot]).unwrap();
    let mut image = texture_support::Image {
        bytes: asset.to_vec(),
        requests: vec![],
    };
    let inputs: Vec<_> = qs
        .iter()
        .map(|q| {
            let mut sample = [[255; 3]; 4];
            if !q.live.quad.default_sample && q.live.quad.header.mask != 0 {
                for p in texture_oracle::sample(
                    q.sample.as_ref().unwrap(),
                    &mut cache,
                    &mut image,
                    tex::Config::counted(),
                )
                .unwrap()
                .pixels
                {
                    sample[usize::from(p.lane)] = p.rgb;
                }
            }
            support::Stimulus {
                quad: q.live.quad,
                sample,
                light: std::array::from_fn(|lane| {
                    let o = light_oracle::evaluate(
                        q.live.light[lane],
                        light.material,
                        light.light,
                        light.projection,
                        light_oracle::Config {
                            rounding: light_oracle::RoundingPolicy {
                                power: light_oracle::Rounding::Floor,
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    light::LightingOutput {
                        g: o.g as u16,
                        h: o.h as u16,
                    }
                }),
            }
        })
        .collect();
    inputs
}
fn golden(
    qs: &[BranchQuad],
    ctx: Context,
    light: light::LightingContext,
    slot: tex::Slot,
    asset: &[u8],
) -> Vec<u8> {
    support::golden(
        support::image(),
        &reference_inputs(qs, light, slot, asset),
        ctx,
    )
}

#[test]
fn both_real_branches_full_frame_guard_wrap_defaults_and_stalls() {
    for shininess in [0, 16] {
        let qs = quads(40);
        let mut ctx = support::context();
        let light = lighting(shininess);
        ctx.specular = light.material.specular_color;
        let slot = texture_support::slot(5, true);
        let asset = texture_support::asset(slot, texture_support::pattern);
        let goldens = reference_inputs(&qs, light, slot, &asset);
        let expected = support::golden(support::image(), &goldens, ctx);
        assert_ne!(
            expected,
            support::image(),
            "golden must actually update memory"
        );
        let mut texture = TextureFixture::new(asset);
        let original_texture = texture.bytes.clone();
        let mut framebuffer = Fixture::new(support::image());
        framebuffer.request_period = 5;
        framebuffer.beat_period = 3;
        framebuffer.ack_delay = 17;
        let mut dut = PixelBranches::new(ctx, light, vec![slot], MAX).unwrap();
        let mut next = 0;
        let mut owners = BTreeMap::new();
        let mut allocations = [None; 16];
        let mut sample_accepted = BTreeSet::new();
        let mut sample_done = BTreeSet::new();
        let mut light_issued = BTreeSet::new();
        let mut light_done = BTreeSet::new();
        let mut ready = BTreeMap::<u64, (u8, u8, u8)>::new();
        let mut reused = 0;
        let mut reads = 0;
        let mut captures = 0;
        let mut consumes = 0;
        let mut retired = 0;
        let mut output_stall = false;
        let mut ce_returns = false;
        let mut held_result = false;
        let mut admission_wait = false;
        let mut samples = BTreeSet::new();
        let mut lightvalues = BTreeSet::new();
        let mut write_committed = false;
        let mut fb_write = false;
        let mut trace = String::from("wall,events\n");
        let mut fb_acks = 0;
        let mut final_ack = false;
        let mut issued_reads = BTreeMap::new();
        let mut captured_reads = BTreeMap::new();
        let mut consumed_reads = BTreeSet::new();
        let mut output_rows = BTreeMap::<u64, u8>::new();
        let mut held_color = None;
        let mut texture_captures = 0;
        let mut color_groups = 0;
        let mut ce_edges = 0;
        let mut sampler_edges = 0;
        let mut sampler_enabled = 0;
        let mut first_admit = None;
        let mut first_sample = None;
        let mut peak_sample_ids = 0;
        for wall in 0..MAX {
            let t = dut
                .step(
                    BranchTick {
                        ce: wall % 17 > 3,
                        final_ready: wall > 100 && wall % 43 > 16,
                        light_ready: wall % 23 > 7,
                        sample_issue_ready: wall % 37 > 12,
                        sample_ready: wall % 29 > 10,
                        quad: qs.get(next).cloned(),
                        finish: next == qs.len(),
                    },
                    &mut texture,
                    &mut framebuffer,
                )
                .unwrap();
            let c = &t.live.model;
            ce_edges += u64::from(c.ce);
            assert!(!c.events.iter().any(|e| matches!(e, Event::Rejected(_))));
            if c.quad_accepted {
                if let Some(ticket) = c.ticket {
                    assert_eq!(ticket.serial, owners.len() as u64);
                    assert_eq!(ticket.quad, u8::try_from(ticket.serial % 16).unwrap());
                    if allocations[usize::from(ticket.quad)].is_some() {
                        reused += 1;
                    }
                    allocations[usize::from(ticket.quad)] = Some(ticket.serial);
                    owners.insert(ticket.serial, next);
                    let q = &qs[next].live.quad;
                    ready.insert(
                        ticket.serial,
                        (
                            0,
                            if q.default_light { q.header.mask } else { 0 },
                            if q.default_sample { q.header.mask } else { 0 },
                        ),
                    );
                }
                next += 1;
            }
            if let Some(ticket) = t.sample_admitted {
                first_admit.get_or_insert(c.wall);
                assert!(owners.contains_key(&ticket.serial));
                assert!(sample_accepted.insert(ticket.serial));
            }
            if let Some((key, _)) = t.live.light_issued {
                assert!(light_issued.insert((key.ticket.serial, key.lane)));
            }
            if let Some((key, v, _)) = t.live.light_returned {
                assert_eq!(
                    v,
                    goldens[owners[&key.ticket.serial]].light[usize::from(key.lane)]
                );
                assert!(light_issued.contains(&(key.ticket.serial, key.lane)));
                lightvalues.insert((v.g, v.h));
            }
            if let Some(w) = t.sample_returned {
                first_sample.get_or_insert(c.wall);
                assert_eq!(
                    w.rgb,
                    goldens[owners[&w.key.ticket.serial]].sample[usize::from(w.key.lane)]
                );
                assert!(sample_accepted.contains(&w.key.ticket.serial));
                assert!(sample_done.insert((w.key.ticket.serial, w.key.lane)));
                samples.insert(w.rgb);
                assert!(c.sample_accepted);
                assert!(c.events.contains(&Event::SampleDone(w.key)));
                assert!(c.accesses.iter().any(|a| a.store == Store::Sample
                    && a.write
                    && a.address == w.key.ticket.quad * 4 + w.key.lane));
            }
            if c.framebuffer.response.accepted {
                fb_write = c.framebuffer.request.unwrap().write;
            }
            if c.framebuffer.response.complete == Some(true) && fb_write {
                write_committed = true;
                fb_acks += 1;
                final_ack |=
                    c.snapshot.phase == Phase::Flushing || c.snapshot.phase == Phase::Complete;
            }
            if !c.events.is_empty()
                || t.sample_admitted.is_some()
                || t.live.light_issued.is_some()
                || c.framebuffer.response.complete.is_some()
            {
                use std::fmt::Write;
                writeln!(trace,"{},\"sample_admit={:?}; light_issue={:?}; sample_return={:?}; light_return={:?}; fb={:?}; events={:?}\"",c.wall,t.sample_admitted,t.live.light_issued,t.sample_returned,t.live.light_returned,c.framebuffer.response,c.events).unwrap();
            }
            for e in &c.events {
                match e {
                    Event::BasicDone(k) => {
                        ready.get_mut(&k.ticket.serial).unwrap().0 |= 1 << k.lane
                    }
                    Event::LightDone(k) => {
                        assert!(light_done.insert((k.ticket.serial, k.lane)));
                        ready.get_mut(&k.ticket.serial).unwrap().1 |= 1 << k.lane;
                    }
                    Event::SampleDone(k) => {
                        ready.get_mut(&k.ticket.serial).unwrap().2 |= 1 << k.lane
                    }
                    Event::Joined(ticket) => {
                        let m = qs[owners[&ticket.serial]].live.quad.header.mask;
                        assert_eq!(ready[&ticket.serial], (m, m, m));
                    }
                    Event::ReadIssued { key, depth } => {
                        reads += 1;
                        assert!(issued_reads
                            .insert((key.ticket.serial, key.lane, *depth), c.wall)
                            .is_none());
                        let q = &qs[owners[&key.ticket.serial]].live.quad;
                        if !depth {
                            for store in [Store::Light, Store::Sample] {
                                let want = if store == Store::Light {
                                    !q.default_light
                                } else {
                                    !q.default_sample
                                };
                                assert_eq!(
                                    c.accesses.iter().any(|a| a.store == store && !a.write),
                                    want
                                );
                            }
                        }
                    }
                    Event::Captured { key, depth } => {
                        captures += 1;
                        let id = (key.ticket.serial, key.lane, *depth);
                        assert!(c.wall > issued_reads[&id]);
                        assert!(captured_reads.insert(id, c.wall).is_none());
                        ce_returns |= !c.ce;
                    }
                    Event::Consumed { key, depth } => {
                        consumes += 1;
                        let id = (key.ticket.serial, key.lane, *depth);
                        assert!(c.wall > captured_reads[&id]);
                        assert!(consumed_reads.insert(id));
                    }
                    Event::OutputAccepted { ticket, row } => {
                        let count = output_rows.entry(ticket.serial).or_default();
                        assert_eq!(*row, *count);
                        *count += 1;
                        let mask = qs[owners[&ticket.serial]].live.quad.header.mask;
                        if mask >> (row / 2) & 1 != 0 {
                            assert!(consumed_reads.contains(&(
                                ticket.serial,
                                row / 2,
                                row % 2 == 1
                            )));
                        }
                    }
                    Event::Retired(ticket) => {
                        assert_eq!(output_rows[&ticket.serial], 8);
                        assert_eq!(ticket.serial, retired);
                        retired += 1;
                    }
                    Event::Complete => assert!(write_committed && final_ack && framebuffer.idle()),
                    _ => {}
                }
            }
            if let Some(s) = &t.sampling {
                peak_sample_ids = peak_sample_ids
                    .max(s.snapshot.result_lanes.iter().filter(|m| **m != 0).count());
                sampler_edges += 1;
                sampler_enabled += u64::from(s.effective_ce);
                if let Some(previous) = held_color {
                    assert_eq!(s.color.output, Some(previous));
                }
                held_color = s
                    .color
                    .output
                    .filter(|_| !s.control.ce || !s.control.result_ready);
                texture_captures += s
                    .cache
                    .events
                    .iter()
                    .filter(|e| matches!(e, gpu_v2::texture::sim::timed::Event::Captured { .. }))
                    .count();
                color_groups += usize::from(s.color.accepted);
                admission_wait |= s.offered.is_some() && !s.accepted;
                held_result |=
                    s.color.output.is_some() && (!s.control.ce || !s.control.result_ready);
            }
            output_stall |= c.snapshot.output_valid && !c.framebuffer.input_accepted;
            if dut.complete() {
                break;
            }
            assert!(wall + 1 < MAX, "bounded composition failed to complete");
        }
        assert!(dut.complete());
        assert!(texture.idle() && framebuffer.idle());
        assert_eq!(next, qs.len());
        assert_eq!(
            framebuffer.bytes, expected,
            "whole framebuffer + depth + guards"
        );
        assert_eq!(texture.bytes, original_texture);
        assert_eq!(reads, captures);
        assert_eq!(captures, consumes);
        assert!(reads > 0 && reused >= 16 && retired > 32);
        assert!(texture.requests > 0);
        assert!(peak_sample_ids > 1);
        assert!(texture_captures >= sample_done.len() && color_groups == texture_captures);
        assert_eq!(texture.beats, texture.requests * 16);
        assert_eq!(texture.completions, texture.requests);
        assert!(samples.len() > 12 && lightvalues.len() > 2);
        assert!(held_result && output_stall && admission_wait && ce_returns);
        let wanted_sample: usize = qs
            .iter()
            .filter(|q| !q.live.quad.default_sample)
            .map(|q| q.live.quad.header.mask.count_ones() as usize)
            .sum();
        let wanted_light: usize = qs
            .iter()
            .filter(|q| !q.live.quad.default_light)
            .map(|q| q.live.quad.header.mask.count_ones() as usize)
            .sum();
        assert_eq!(sample_done.len(), wanted_sample);
        assert_eq!(light_done.len(), wanted_light);
        assert_eq!(light_issued.len(), wanted_light);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/gpu-pixel-branches/persistent/traces");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("branches-shininess-{shininess}.csv")),
            trace,
        )
        .unwrap();
        println!("MATCHED_PROOF shininess={shininess} walls={} ce_edges={ce_edges} sampler_edges={sampler_edges} sampler_enabled={sampler_enabled} sample_quads={} sample_pixels={} first_admit={} first_sample={} complete={} refills={} beats={}",framebuffer.cycle,sample_accepted.len(),sample_done.len(),first_admit.unwrap(),first_sample.unwrap(),framebuffer.cycle,texture.requests,texture.beats);
        println!("BRANCH_PROOF shininess={shininess} walls={} quads={retired} reused={reused} light={} sample={} reads={reads} captures={captures} consumes={consumes} texture_requests={} texture_beats={} fb_write_acks={fb_acks} distinct_sample={} distinct_light={} final_ack={final_ack}", framebuffer.cycle,light_done.len(),sample_done.len(),texture.requests,texture.beats,samples.len(),lightvalues.len());
    }
}

#[test]
fn finish_keeps_allocated_unaccepted_sampling_offer_and_writes_before_done() {
    let q = quads(1).remove(0);
    let ctx = support::context();
    let light = lighting(7);
    let slot = texture_support::slot(5, true);
    let asset = texture_support::asset(slot, texture_support::pattern);
    let expected = golden(std::slice::from_ref(&q), ctx, light, slot, &asset);
    let mut texture = TextureFixture::new(asset);
    let mut framebuffer = Fixture::new(support::image());
    let mut dut = PixelBranches::new(ctx, light, vec![slot], MAX).unwrap();
    let mut allocated = false;
    let mut admitted = false;
    let mut done = 0;
    for wall in 0..MAX {
        let t = dut
            .step(
                BranchTick {
                    quad: (!allocated).then(|| q.clone()),
                    finish: allocated,
                    sample_issue_ready: wall > 100,
                    sample_ready: wall > 700,
                    ce: wall % 13 > 2,
                    ..Default::default()
                },
                &mut texture,
                &mut framebuffer,
            )
            .unwrap();
        allocated |= t.live.model.ticket.is_some();
        if allocated && wall <= 100 {
            assert!(!admitted && !dut.complete());
            assert_eq!(texture.requests, 0);
        }
        if t.sample_admitted.is_some() {
            admitted = true;
            assert!(wall > 100);
        }
        if t.sample_returned.is_some() {
            assert!(admitted && wall > 700);
            done += 1;
        }
        if dut.complete() {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(dut.complete() && admitted);
    assert_eq!(done, 4);
    assert_eq!(framebuffer.bytes, expected);
    assert!(texture.idle() && framebuffer.idle());
}

#[test]
fn texture_fault_is_terminal_and_external_transport_drains_separately() {
    let q = quads(1).remove(0);
    let slot = texture_support::slot(5, true);
    let mut texture = TextureFixture::new(texture_support::asset(slot, texture_support::pattern));
    let mut framebuffer = Fixture::new(support::image());
    let mut dut = PixelBranches::new(support::context(), lighting(3), vec![slot], MAX).unwrap();
    let t = dut
        .step(
            BranchTick {
                quad: Some(q),
                ..Default::default()
            },
            &mut texture,
            &mut framebuffer,
        )
        .unwrap();
    assert!(t.live.model.ticket.is_some());
    let mut failed = false;
    for wall in 0..MAX {
        texture.fail = texture.requests > 0;
        let t = dut.step(BranchTick::default(), &mut texture, &mut framebuffer);
        if t.is_err() {
            failed = true;
            break;
        }

        assert!(wall + 1 < MAX);
    }
    assert!(failed && dut.faulted() && !dut.complete());
    texture.fail = false;
    // A distinct caller owns texture drain: never tick a faulted Session or
    // advance the same controller twice through independent adapters.
    for wall in 0..MAX {
        tex::RefillPort::step(&mut texture).unwrap();
        dut.step(
            BranchTick {
                ce: false,
                ..Default::default()
            },
            &mut texture,
            &mut framebuffer,
        )
        .unwrap();
        if texture.idle() && dut.framebuffer_drained() {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(texture.idle() && dut.framebuffer_drained());
    assert!(!dut.complete());
}

#[test]
fn default_and_zero_coverage_do_not_compile_invalid_unused_branches() {
    let mut qs = quads(2);
    qs[0].live.quad.default_light = true;
    qs[0].live.quad.default_sample = true;
    qs[1].live.quad.header.mask = 0;
    qs[1].sample.as_mut().unwrap().mask = 0;
    for q in &mut qs {
        q.sample.as_mut().unwrap().slot = 255;
        q.sample.as_mut().unwrap().uv = [[f64::NAN; 2]; 4];
        q.live.light[0].ndc = [i32::MAX; 2];
    }
    let ctx = support::context();
    let inputs: Vec<_> = qs
        .iter()
        .map(|q| support::Stimulus {
            quad: q.live.quad,
            light: [light::LightingOutput { g: 256, h: 0 }; 4],
            sample: [[255; 3]; 4],
        })
        .collect();
    let expected = support::golden(support::image(), &inputs, ctx);
    let mut texture = TextureFixture::new(vec![]);
    let mut framebuffer = Fixture::new(support::image());
    let mut dut =
        PixelBranches::new(ctx, lighting(0), vec![texture_support::slot(5, true)], MAX).unwrap();
    let mut next = 0;
    for wall in 0..MAX {
        let t = dut
            .step(
                BranchTick {
                    quad: qs.get(next).cloned(),
                    finish: next == qs.len(),
                    ..Default::default()
                },
                &mut texture,
                &mut framebuffer,
            )
            .unwrap();
        if t.live.model.quad_accepted {
            next += 1;
        }
        assert!(
            t.sampling
                .as_ref()
                .is_some_and(|s| s.offered.is_none() && !s.accepted && s.cache.events.is_empty())
                && t.sample_returned.is_none()
                && t.live.light_issued.is_none()
        );
        assert!(t
            .live
            .model
            .accesses
            .iter()
            .all(|a| a.store == Store::Basic));
        if dut.complete() {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(dut.complete());
    assert_eq!(texture.requests, 0);
    assert_eq!(framebuffer.bytes, expected);
}

#[test]
fn framebuffer_failure_after_real_branch_results_never_completes_successfully() {
    let q = quads(1).remove(0);
    let slot = texture_support::slot(5, true);
    let mut texture = TextureFixture::new(texture_support::asset(slot, texture_support::pattern));
    let initial = support::image();
    let mut framebuffer = Fixture::new(initial.clone());
    framebuffer.fail_request = Some(1);
    let mut dut = PixelBranches::new(support::context(), lighting(8), vec![slot], MAX).unwrap();
    let mut allocated = false;
    let mut sample = 0;
    let mut light = 0;
    let mut fault = false;
    for wall in 0..MAX {
        if dut.faulted() {
            tex::RefillPort::step(&mut texture).unwrap();
        }
        match dut.step(
            BranchTick {
                quad: (!allocated).then(|| q.clone()),
                finish: allocated,
                ..Default::default()
            },
            &mut texture,
            &mut framebuffer,
        ) {
            Ok(t) => {
                allocated |= t.live.model.ticket.is_some();
                sample += u32::from(t.sample_returned.is_some());
                light += u32::from(t.live.light_returned.is_some());
                assert!(!t.live.model.events.contains(&Event::Complete));
            }
            Err(_) => assert!(dut.faulted()),
        }
        fault |= dut.faulted();
        if fault && dut.framebuffer_drained() && texture.idle() {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(fault && dut.framebuffer_drained() && !dut.complete());
    assert_eq!((sample, light), (4, 4));
    assert!(framebuffer.idle() && texture.idle());
    assert_eq!(framebuffer.bytes, initial);
}

#[test]
fn persistent_cache_survives_idle_gaps_and_global_wrap_with_different_rgb() {
    assert_eq!(SAMPLING_OFFER_BITS, 180);
    let mut qs = quads(20);
    for (i, q) in qs.iter_mut().enumerate() {
        q.live.quad.header.mask = if i % 3 == 2 { 5 } else { 15 };
        q.live.quad.default_light = false;
        q.live.quad.default_sample = false;
        let sample = q.sample.as_mut().unwrap();
        sample.mask = q.live.quad.header.mask;
        sample.filter = tex::Filter::Nearest;
        sample.lod_bias = 0.0;
        sample.uv = [[(i % 5 + 1) as f64 / 32.0, ((i * 3) % 5 + 1) as f64 / 32.0]; 4];
    }
    let ctx = support::context();
    let light = lighting(8);
    let slot = texture_support::slot(5, true);
    let asset = texture_support::asset(slot, texture_support::pattern);
    let goldens = reference_inputs(&qs, light, slot, &asset);
    let expected = support::golden(support::image(), &goldens, ctx);
    assert_ne!(goldens[0].sample[0], goldens[16].sample[0]);
    let mut texture = TextureFixture::new(asset);
    let mut framebuffer = Fixture::new(support::image());
    let mut dut = PixelBranches::new(ctx, light, vec![slot], MAX).unwrap();
    let mut next = 0;
    let mut waiting = false;
    let mut gap = 0;
    let mut gap_edges = 0;
    let mut requests_after_first = None;
    let mut got = BTreeSet::new();
    let mut wrapped = false;
    let mut admitted_wall = 0;
    let mut first = 0;
    let mut latencies = vec![];
    let mut completed_windows = 0;
    for wall in 0..MAX {
        let in_gap = gap > 0;
        let request_before = texture.requests;
        let t = dut
            .step(
                BranchTick {
                    quad: (!waiting && !in_gap)
                        .then(|| qs.get(next).cloned())
                        .flatten(),
                    finish: next == qs.len() && !waiting && !in_gap,
                    ..Default::default()
                },
                &mut texture,
                &mut framebuffer,
            )
            .unwrap();
        if t.live.model.quad_accepted {
            let ticket = t.live.model.ticket.unwrap();
            assert_eq!(ticket.serial, next as u64);
            wrapped |= ticket.serial >= 16 && ticket.quad == 0;
            next += 1;
            waiting = true;
        }
        if let Some(ticket) = t.sample_admitted {
            admitted_wall = t.live.wall;
            assert_eq!(ticket.serial, (next - 1) as u64);
        }
        if let Some(w) = t.sample_returned {
            let index = w.key.ticket.serial as usize;
            assert!(got.insert((index, w.key.lane)));
            assert_eq!(w.rgb, goldens[index].sample[usize::from(w.key.lane)]);
            if first == 0 {
                first = t.live.wall;
            }
            if t.sampling.as_ref().unwrap().snapshot.result_lanes[usize::from(w.key.ticket.quad)]
                == 0
            {
                latencies.push(t.live.wall - admitted_wall);
            }
        }
        if waiting && dut.sampling_idle() && dut.snapshot().live == 0 {
            completed_windows += 1;
            waiting = false;
            gap = 37;
            let first_requests = *requests_after_first.get_or_insert(texture.requests);
            assert_eq!(
                texture.requests, first_requests,
                "warm/gap/wrap must not refill again"
            );
        } else if in_gap {
            gap -= 1;
            gap_edges += 1;
            assert!(dut.sampling_idle());
            assert_eq!(texture.requests, request_before);
        }
        assert_eq!(
            texture.wall, framebuffer.cycle,
            "distinct ports each step once per wall"
        );
        if dut.complete() {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(dut.complete() && wrapped && gap_edges >= 20 * 37);
    assert_eq!(completed_windows, 20);
    assert_eq!(texture.requests, 1);
    assert_eq!(latencies.len(), 20);
    assert!(latencies[1..].iter().all(|n| *n < latencies[0]));
    assert_eq!(
        got.len(),
        qs.iter()
            .map(|q| q.live.quad.header.mask.count_ones() as usize)
            .sum()
    );
    assert_eq!(framebuffer.bytes, expected);
    assert_eq!(dut.sampling_stats().admissions, 20);
    assert_eq!(dut.sampling_stats().compilations, 0);
    println!("WARM_PROOF windows={completed_windows} gap_edges={gap_edges} requests={} first={first} cold_interval={} warm_intervals={:?} final={}",texture.requests,latencies[0],&latencies[1..],framebuffer.cycle);
}

#[test]
fn persistent_real_capacity_stall_retains_allocated_offer_and_recovers() {
    let mut qs = quads(24);
    for (i, q) in qs.iter_mut().enumerate() {
        q.live.quad.header.mask = 15;
        q.live.quad.default_light = false;
        q.live.quad.default_sample = false;
        let sample = q.sample.as_mut().unwrap();
        sample.mask = 15;
        sample.filter = tex::Filter::Bilinear;
        sample.lod_bias = 0.0;
        sample.uv = [[(i % 4) as f64 / 4.0, (i / 4 % 4) as f64 / 4.0]; 4];
    }
    let ctx = support::context();
    let light = lighting(8);
    let slot = texture_support::slot(5, true);
    let asset = texture_support::asset(slot, texture_support::pattern);
    let goldens = reference_inputs(&qs, light, slot, &asset);
    let expected = support::golden(support::image(), &goldens, ctx);
    let mut texture = TextureFixture::new(asset);
    let mut framebuffer = Fixture::new(support::image());
    let mut dut = PixelBranches::new(ctx, light, vec![slot], MAX).unwrap();
    let mut next = 0;
    let mut public_ids = 0;
    let mut global = 0;
    let mut credits = 0;
    let mut groups = 0;
    let mut producer = 0;
    let mut hold = None;
    let mut stable = 0;
    let mut paused_beats = 0;
    let mut closed_beats = 0;
    let mut held_offer = 0;
    let mut accepted = BTreeSet::new();
    let mut returned = BTreeSet::new();
    for wall in 0..MAX {
        let t = dut
            .step(
                BranchTick {
                    quad: qs.get(next).cloned(),
                    finish: next == qs.len(),
                    ce: wall % 23 > 7,
                    sample_ready: wall >= 7000,
                    ..Default::default()
                },
                &mut texture,
                &mut framebuffer,
            )
            .unwrap();
        if t.live.model.quad_accepted {
            next += 1;
        }
        if let Some(ticket) = t.sample_admitted {
            assert!(accepted.insert(ticket.serial));
        }
        let s = t.sampling.as_ref().unwrap();
        if let Some(old) = hold {
            assert_eq!(s.color.output, Some(old));
            stable += 1;
        }
        hold = s
            .color
            .output
            .filter(|_| !s.control.ce || !s.control.result_ready);
        public_ids = public_ids.max(s.snapshot.result_lanes.iter().filter(|m| **m != 0).count());
        global = global.max(t.live.model.snapshot.live);
        credits = credits.max(s.snapshot.color.result_credits);
        let pool = s.snapshot.cache.packet_pool.as_ref().unwrap();
        groups = groups.max(pool.groups);
        producer = producer.max(pool.producer);
        let beats = s
            .cache
            .events
            .iter()
            .filter(|e| matches!(e, gpu_v2::texture::sim::timed::Event::Beat { .. }))
            .count();
        paused_beats += usize::from(!s.control.ce) * beats;
        closed_beats += usize::from(!s.control.result_ready) * beats;
        held_offer += usize::from(s.offered.is_some() && !s.accepted && s.control.ce);
        if let Some(w) = t.sample_returned {
            assert!(wall >= 7000 && accepted.contains(&w.key.ticket.serial));
            assert!(returned.insert((w.key.ticket.serial, w.key.lane)));
            assert_eq!(
                w.rgb,
                goldens[w.key.ticket.serial as usize].sample[usize::from(w.key.lane)]
            );
        }
        if wall < 7000 {
            assert!(!dut.complete());
            assert!(t.sample_returned.is_none());
        }
        if dut.complete() {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(dut.complete());
    assert_eq!(framebuffer.bytes, expected);
    assert_eq!(returned.len(), 24 * 4);
    assert_eq!(credits, 16);
    assert_eq!(global, 16);
    assert!(public_ids > 1 && producer <= 16 && groups <= 32);
    assert!(stable > 0 && held_offer > 0 && paused_beats > 0 && closed_beats > 0);
    assert!(dut.sampling_stats().link.color_gated_edges > 0);
    assert_eq!(dut.sampling_stats().admissions, 24);
    assert_eq!(dut.sampling_stats().compilations, 0);
    println!("CAPACITY_PROOF ids={public_ids} global={global} credits={credits} producer={producer} groups={groups} stable={stable} held_offer={held_offer} paused_beats={paused_beats} closed_beats={closed_beats}");
}

#[test]
fn captured_offer_rne_ties_and_bias_clamp_preserve_reference_rgb() {
    let mut qs = quads(12);
    for (i, q) in qs.iter_mut().enumerate() {
        q.live.quad.default_sample = false;
        let sample = q.sample.as_mut().unwrap();
        sample.filter = tex::Filter::Nearest;
        sample.lod_bias = [0.0, 1.5 / 256.0, -1.5 / 256.0, 33.0, -33.0, 2.5 / 256.0][i % 6];
        let base = 32768.0 + (i % 2) as f64;
        sample.uv = std::array::from_fn(|lane| {
            [
                (base + 0.5 + lane as f64 / 2.0) / 65536.0,
                (16384.5 - lane as f64 / 2.0) / 65536.0,
            ]
        });
    }
    let slot = texture_support::slot(5, true);
    let light = lighting(8);
    let ctx = support::context();
    let asset = texture_support::asset(slot, texture_support::pattern);
    let gs = reference_inputs(&qs, light, slot, &asset);
    let expected = support::golden(support::image(), &gs, ctx);
    let mut texture = TextureFixture::new(asset);
    let mut framebuffer = Fixture::new(support::image());
    let mut dut = PixelBranches::new(ctx, light, vec![slot], MAX).unwrap();
    let mut next = 0;
    let mut owners = vec![];
    let mut got = 0;
    for wall in 0..MAX {
        let t = dut
            .step(
                BranchTick {
                    quad: qs.get(next).cloned(),
                    finish: next == qs.len(),
                    sample_issue_ready: wall % 41 > 17,
                    ce: wall % 13 > 2,
                    ..Default::default()
                },
                &mut texture,
                &mut framebuffer,
            )
            .unwrap();
        if t.live.model.quad_accepted {
            if t.live.model.ticket.is_some() {
                owners.push(next);
            }
            next += 1;
        }
        if let Some(w) = t.sample_returned {
            got += 1;
            assert_eq!(
                w.rgb,
                gs[owners[w.key.ticket.serial as usize]].sample[usize::from(w.key.lane)]
            );
        }
        if dut.complete() {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(dut.complete() && got > 0);
    assert_eq!(framebuffer.bytes, expected);
}

#[test]
fn allocated_offer_survives_sender_change_and_cannot_fund_same_edge_allocation() {
    let mut qs = quads(3);
    for q in &mut qs {
        q.live.quad.default_sample = false;
    }
    let ctx = support::context();
    let light = lighting(8);
    let slot = texture_support::slot(5, true);
    let asset = texture_support::asset(slot, texture_support::pattern);
    let gs = reference_inputs(&qs[..2], light, slot, &asset);
    let expected = support::golden(support::image(), &gs, ctx);
    let mut texture = TextureFixture::new(asset);
    let mut framebuffer = Fixture::new(support::image());
    let mut dut = PixelBranches::new(ctx, light, vec![slot], MAX).unwrap();
    let mut allocated = 0;
    let mut admitted = BTreeSet::new();
    let mut results = BTreeSet::new();
    for wall in 0..MAX {
        let t = dut
            .step(
                BranchTick {
                    quad: Some(qs[allocated.min(2)].clone()),
                    sample_issue_ready: wall == 20 || wall >= 700,
                    finish: allocated >= 2,
                    ..Default::default()
                },
                &mut texture,
                &mut framebuffer,
            )
            .unwrap();
        if wall < 20 && allocated == 1 {
            assert!(!t.live.model.quad_accepted);
        }
        if wall == 20 {
            assert_eq!(t.sample_admitted.unwrap().serial, 0);
            assert!(
                !t.live.model.quad_accepted,
                "accepted offer credit is not pre-edge credit"
            );
        }
        if wall == 21 {
            assert_eq!(t.live.model.ticket.unwrap().serial, 1);
            assert!(
                t.sample_admitted.is_none(),
                "new allocation cannot issue sampling on its own edge"
            );
        }
        if t.live.model.quad_accepted {
            allocated += 1;
            assert!(allocated <= 2);
        }
        if let Some(ticket) = t.sample_admitted {
            assert!(admitted.insert(ticket.serial));
        }
        if let Some(w) = t.sample_returned {
            assert!(admitted.contains(&w.key.ticket.serial));
            assert!(results.insert((w.key.ticket.serial, w.key.lane)));
            assert_eq!(
                w.rgb,
                gs[w.key.ticket.serial as usize].sample[usize::from(w.key.lane)]
            );
        }
        if wall == 699 {
            assert_eq!(dut.sampling_stats().admissions, 1);
            assert_eq!(dut.sampling_stats().compilations, 0);
            assert_eq!(
                results.len(),
                4,
                "held new offer must not freeze accepted older work"
            );
            assert!(!dut.complete());
        }
        if dut.complete() {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(dut.complete());
    assert_eq!(allocated, 2);
    assert_eq!(admitted.len(), 2);
    assert_eq!(results.len(), 8);
    assert_eq!(framebuffer.bytes, expected);
}
