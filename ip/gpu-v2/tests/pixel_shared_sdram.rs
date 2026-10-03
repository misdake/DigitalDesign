//! Real single-MC composition; numerical preparation/final/ROP limits stay explicit.
use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
use gpu_v2::{
    framebuffer::ports as fb,
    lighting::{ports as light, sim::oracle as light_oracle},
    memory::ports::MemoryPort,
    system::pixel::*,
    texture::{ports as tex, sim::oracle as texture_oracle},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
#[allow(dead_code)]
#[path = "support/pixel.rs"]
mod pixel;
#[path = "support/sdram/shared_pixel.rs"]
mod shared;
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod texture;

const MAX: u64 = 40_000;
const TEXTURE_BASE: u32 = 0x10000;
const BACKGROUND_BASE: u64 = 0x20000;
struct Inputs {
    context: Context,
    light: light::LightingContext,
    slot: tex::Slot,
    asset: Vec<u8>,
    initial: Vec<u8>,
    quads: Vec<BranchQuad>,
    goldens: Vec<pixel::Stimulus>,
    expected: Vec<u8>,
}
struct GoldenTexture<'a>(&'a [u8]);
impl gpu_v2::frontend::ports::MemoryPort for GoldenTexture<'_> {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        if bytes != 128 || address < u64::from(TEXTURE_BASE) || !address.is_multiple_of(128) {
            return Err("independent relocated texture read".into());
        }
        let offset = (address - u64::from(TEXTURE_BASE)) as usize;
        let data = self
            .0
            .get(offset..offset + bytes)
            .ok_or("texture golden range")?;
        Ok(data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|w| u64::from_le_bytes(*w))
            .collect())
    }
}
fn inputs(count: usize, bypass: bool) -> Inputs {
    let light = light::LightingContext {
        material: light::Material {
            shininess_code: 8,
            specular_color: [17, 93, 203],
            unlit: false,
        },
        light: light::Light {
            direction: [0, 0, 16384],
            ambient: 43,
            directional: 217,
        },
        projection: light::Projection::default(),
        epoch: 19,
    };
    let mut context = pixel::context();
    context.rop.depth = fb::DepthFunc::Always;
    context.specular = light.material.specular_color;
    let mut slot = texture::slot(5, false);
    slot.base_address = TEXTURE_BASE;
    let asset = texture::asset(slot, texture::pattern);
    let mut initial: Vec<u8> = (0..0x21000)
        .map(|i| ((i * 13) ^ (i >> 6) ^ 0xa7) as u8)
        .collect();
    initial[..24576].copy_from_slice(&pixel::image());
    initial[TEXTURE_BASE as usize..TEXTURE_BASE as usize + asset.len()].copy_from_slice(&asset);
    assert!(u64::from(context.surface.depth_base_bytes) + 10240 < u64::from(TEXTURE_BASE));
    assert!(u64::from(TEXTURE_BASE) + (asset.len() as u64) < BACKGROUND_BASE);
    let quads: Vec<_> = (0..count)
        .map(|i| {
            // Four cold tiles, followed by distinct quads revisiting the same tiles.
            let uv = [[0.03, 0.04], [0.53, 0.04], [0.03, 0.54], [0.53, 0.54]][i % 4];
            let mut sample = texture::input(5, tex::Filter::Nearest, uv);
            sample.mask = if i == 6 {
                5
            } else if i == 11 {
                0
            } else {
                15
            };
            if i == 11 {
                sample.uv = [[f64::NAN; 2]; 4];
                sample.slot = 255;
            }
            BranchQuad {
                live: LiveQuad {
                    quad: QuadInput {
                        header: fb::Header {
                            x: (i % 10 * 16) as u16,
                            y: (i / 10 * 16) as u8,
                            mask: sample.mask,
                        },
                        basic: std::array::from_fn(|lane| Basic {
                            tint: [
                                (i * 19 + lane * 13 + 27) as u8,
                                (i * 31 + lane * 7 + 109) as u8,
                                (i * 5 + lane * 41 + 53) as u8,
                            ],
                            depth: (36000 - i * 311 - lane * 73) as u16,
                        }),
                        default_light: bypass || i == 10,
                        default_sample: bypass || i == 10,
                    },
                    light: std::array::from_fn(|lane| light::PixelInput {
                        normal: [
                            [0, 0, 16384],
                            [8192, 0, 16384],
                            [-8192, 4096, 16384],
                            [0, 0, 0],
                        ][lane],
                        ndc: [((i % 3) as i32 - 1) * 32768, (lane as i32 - 2) * 16384],
                    }),
                },
                sample: Some(sample),
            }
        })
        .collect();
    let mut cache = texture_oracle::Cache::new(vec![slot]).unwrap();
    let mut memory = GoldenTexture(&asset);
    let goldens: Vec<_> = quads
        .iter()
        .map(|q| {
            let mut sample = [[255; 3]; 4];
            if !q.live.quad.default_sample && q.live.quad.header.mask != 0 {
                for p in texture_oracle::sample(
                    q.sample.as_ref().unwrap(),
                    &mut cache,
                    &mut memory,
                    tex::Config::counted(),
                )
                .unwrap()
                .pixels
                {
                    sample[p.lane as usize] = p.rgb;
                }
            }
            pixel::Stimulus {
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
    let expected = pixel::golden(initial.clone(), &goldens, context);
    assert_ne!(expected, initial, "independent golden updates memory");
    Inputs {
        context,
        light,
        slot,
        asset,
        initial,
        quads,
        goldens,
        expected,
    }
}
fn fixture(i: &Inputs, loaded: bool) -> shared::Shared {
    // SAFETY: external immutable starting image, not an arithmetic intermediate.
    let image = unsafe {
        OracleImage::from_host(0, i.initial.clone(), "shared pixel initial fixture").unwrap()
    };
    shared::Shared::new(
        image,
        (
            u64::from(TEXTURE_BASE),
            u64::from(TEXTURE_BASE) + i.asset.len() as u64,
        ),
        BACKGROUND_BASE,
        loaded,
        MAX,
    )
    .unwrap()
}
fn artifacts() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-pixel-branches/shared-mc-integration")
}
fn assert_edge(s: &shared::Shared, before: (u64, u64)) {
    let now = s.clocks();
    assert_eq!(
        now,
        (before.0 + 1, before.1 + 2),
        "one physical tick per successful wrapper edge"
    );
    assert!(!s.tick_poisoned() && !s.physical_error());
    let stats = s.stats();
    assert_eq!(now.0, stats.init_edges + stats.frame_edges);
    assert!(stats.init_edges > 10_000 && stats.init_edges < 12_000);
}
fn background_drain(s: &shared::Shared, ro: &mut shared::RoView, fb: &mut shared::FbView) -> u64 {
    s.stop_background();
    assert!(s.gpu_idle());
    let start = s.stats().frame_edges;
    for _ in 0..2048 {
        if s.idle() {
            break;
        }
        ro.discard_edge().unwrap();
        let before = s.clocks();
        assert_eq!(fb.cycle(None, None).unwrap(), Default::default());
        assert_edge(s, before);
    }
    assert!(s.idle(), "finite separate background drain");
    s.stats().frame_edges - start
}
fn render(loaded: bool, pauses: bool, name: &str) {
    let i = inputs(12, false);
    let s = fixture(&i, loaded);
    let (mut ro, mut fb) = s.views();
    let mut dut = PixelBranches::new(i.context, i.light, vec![i.slot], MAX).unwrap();
    let mut next = 0;
    let mut owners = Vec::new();
    let mut samples = BTreeSet::new();
    let mut lights = BTreeSet::new();
    let mut refill_start = [0_u64; 12];
    let mut refill_delta = [None; 12];
    let mut returned_mask = [0_u8; 12];
    let mut pause_until = 0;
    let mut pause_triggered = false;
    let mut ce_beats = 0;
    let mut closed_beats = 0;
    let mut ce_fb_beats = 0;
    let mut flushes = 0;
    let mut final_write_ack = false;
    let mut refill_addresses = BTreeMap::new();
    for wall in 0..MAX {
        if pauses && s.ro_active() && !pause_triggered {
            pause_triggered = true;
            pause_until = wall + 45;
        }
        let ce = !pauses || (wall >= pause_until && wall % 17 > 3);
        let sample_ready = !pauses || wall >= 900;
        let before = s.clocks();
        let stats = s.stats();
        let offered = (dut.snapshot().live == 0)
            .then(|| i.quads.get(next).cloned())
            .flatten();
        let t = dut
            .step(
                BranchTick {
                    ce,
                    sample_ready,
                    quad: offered,
                    final_ready: !pauses || wall % 23 > 5,
                    light_ready: !pauses || wall % 19 > 4,
                    finish: next == i.quads.len(),
                    ..Default::default()
                },
                &mut ro,
                &mut fb,
            )
            .unwrap();
        assert_edge(&s, before);
        let after = s.stats();
        for event in &t.sampling.as_ref().unwrap().cache.events {
            if let gpu_v2::texture::sim::timed::Event::Submitted { id, address, .. } = event {
                assert!(refill_addresses.insert(*id, *address as usize).is_none());
            }
        }
        assert_eq!(after.frame_edges, t.live.model.wall);
        ce_beats += u64::from(!ce) * (after.ro_beats - stats.ro_beats);
        closed_beats += u64::from(!sample_ready) * (after.ro_beats - stats.ro_beats);
        ce_fb_beats += u64::from(!ce)
            * (after.fb_read_beats + after.fb_write_beats
                - stats.fb_read_beats
                - stats.fb_write_beats);
        assert!(!t
            .live
            .model
            .events
            .iter()
            .any(|e| matches!(e, Event::Rejected(_))));
        if t.live.model.quad_accepted {
            if let Some(ticket) = t.live.model.ticket {
                assert_eq!(ticket.serial as usize, owners.len());
                refill_start[next] = after.ro_submitted;
                owners.push(next);
            }
            next += 1;
        }
        if let Some((key, output, epoch)) = t.live.light_returned {
            let index = owners[key.ticket.serial as usize];
            assert_eq!(epoch, i.light.epoch);
            assert_eq!(output, i.goldens[index].light[key.lane as usize]);
            assert!(lights.insert((index, key.lane)));
        }
        if let Some(w) = t.sample_returned {
            let index = owners[w.key.ticket.serial as usize];
            assert_eq!(w.rgb, i.goldens[index].sample[w.key.lane as usize]);
            assert!(samples.insert((index, w.key.lane)));
            returned_mask[index] |= 1 << w.key.lane;
            if returned_mask[index] == i.quads[index].live.quad.header.mask {
                refill_delta[index] = Some(after.ro_submitted - refill_start[index]);
            }
        }
        flushes += t
            .live
            .model
            .events
            .iter()
            .filter(|e| matches!(e, Event::FlushRequested))
            .count();
        if t.live.model.framebuffer.response.complete == Some(true)
            && matches!(
                t.live.model.snapshot.phase,
                Phase::Flushing | Phase::Complete
            )
        {
            final_write_ack = true;
        }
        if dut.complete() {
            break;
        }
        assert!(wall + 1 < MAX, "render watchdog");
    }
    assert!(dut.complete() && s.gpu_idle() && dut.sampling_idle());
    assert_eq!(next, i.quads.len());
    assert_eq!(flushes, 1);
    assert!(final_write_ack);
    let expected_keys: BTreeSet<_> = i
        .quads
        .iter()
        .enumerate()
        .flat_map(|(index, q)| {
            (0..4)
                .filter(move |lane| {
                    !q.live.quad.default_sample && q.live.quad.header.mask & (1 << lane) != 0
                })
                .map(move |lane| (index, lane))
        })
        .collect();
    assert_eq!(samples, expected_keys);
    assert_eq!(lights, expected_keys);
    assert_eq!(
        &refill_delta[..4],
        &[Some(1); 4],
        "four distinct cold tiles"
    );
    assert_eq!(
        &refill_delta[4..10],
        &[Some(0); 6],
        "same persistent cache across later quads"
    );
    assert_eq!(s.stats().ro_submitted, 4);
    let render_edges = s.stats().frame_edges;
    let bg_edges = background_drain(&s, &mut ro, &mut fb);
    assert_eq!(
        s.image(),
        i.expected,
        "whole image including framebuffer, depth, guards, texture and background"
    );
    let stats = s.stats();
    assert_eq!(stats.ro_beats, 16 * stats.ro_submitted);
    assert_eq!(stats.ro_delivered_beats, stats.ro_beats);
    assert_eq!(stats.ro_delivered_complete, stats.ro_submitted);
    assert_eq!(stats.fb_read_beats, 16 * stats.fb_reads);
    assert_eq!(stats.fb_write_beats, 16 * stats.fb_writes);
    assert_eq!(stats.fb_terminals, stats.fb_reads + stats.fb_writes);
    assert_eq!(stats.bg_submitted, stats.bg_completed);
    assert_eq!(stats.max_ro_parents, 1);
    assert_eq!(stats.max_fb_outstanding, 1);
    assert_eq!(stats.max_ro_records, 1);
    for t in s.trace().iter().filter(|t| t.delivered.is_some()) {
        assert_eq!(t.client, shared::Client::Ro);
        assert_eq!(t.delivered.unwrap(), t.physical + 1);
        assert!(!t.discarded);
    }
    for t in s
        .trace()
        .iter()
        .filter(|t| t.client == shared::Client::Ro && t.event == "beat" && t.delivered.is_none())
    {
        let offset = refill_addresses[&t.id] + t.index as usize * 8;
        assert_eq!(
            t.data.unwrap(),
            u64::from_le_bytes(i.initial[offset..offset + 8].try_into().unwrap()),
            "every physical RO beat matches independent initial image"
        );
    }
    // Independent background payload checks, distinct from DUT bank/address helpers.
    for t in s.trace().iter().filter(|t| {
        t.event == "beat"
            && matches!(
                t.client,
                shared::Client::Display | shared::Client::Instruction | shared::Client::Data
            )
    }) {
        let client = match t.client {
            shared::Client::Display => 0,
            shared::Client::Instruction => 1,
            shared::Client::Data => 2,
            _ => unreachable!(),
        };
        let offset = BACKGROUND_BASE as usize + client * 32 + t.index as usize * 8;
        assert_eq!(
            t.data.unwrap(),
            u64::from_le_bytes(i.initial[offset..offset + 8].try_into().unwrap())
        );
    }
    if loaded {
        assert!(stats.bg_submitted.iter().all(|&n| n > 1));
    } else {
        assert_eq!(stats.bg_submitted, [0; 3]);
        assert_eq!(bg_edges, 0);
    }
    if pauses {
        assert!(ce_beats > 0 && closed_beats > 0 && ce_fb_beats > 0);
    }
    s.save(&artifacts(), name);
    std::fs::write(artifacts().join(format!("{name}-render.txt")), format!(
        "render_edges={render_edges}\nbackground_drain_edges={bg_edges}\nro_ce0_beats={ce_beats}\nro_closed_result_beats={closed_beats}\nfb_ce0_beats={ce_fb_beats}\nrefill_delta={refill_delta:?}\n" )).unwrap();
}
#[test]
fn shared_frame_cold_then_hot_without_background() {
    render(false, false, "solo");
}
#[test]
fn shared_frame_background_with_matching_solo_controls() {
    render(true, false, "loaded");
}
#[test]
fn shared_frame_background_ce_pause_and_closed_results() {
    render(true, true, "loaded-paused");
}
#[test]
fn stopped_background_drains_reserved_sinks_separately() {
    let i = inputs(1, true);
    let s = fixture(&i, true);
    let (mut ro, mut fb) = s.views();
    tex::RefillPort::step(&mut ro).unwrap();
    let before = s.clocks();
    assert_eq!(fb.cycle(None, None).unwrap(), Default::default());
    assert_edge(&s, before);
    assert!(s.gpu_idle() && !s.idle());
    assert_eq!(s.stats().bg_submitted, [1; 3]);
    let edges = background_drain(&s, &mut ro, &mut fb);
    assert!(edges > 0);
    assert_eq!(
        s.stats().bg_submitted,
        [1; 3],
        "no arrival after generator stop"
    );
    assert_eq!(s.stats().bg_completed, [1; 3]);
    assert_eq!(s.stats().max_bg_pending, [1; 3]);
    assert_eq!(s.image(), i.initial);
    s.save(&artifacts(), "background-only-drain");
}
fn fault_drain(
    dut: &mut PixelBranches,
    s: &shared::Shared,
    ro: &mut shared::RoView,
    fb: &mut shared::FbView,
) {
    s.abort_sampling();
    for wall in 0..MAX {
        ro.discard_edge().unwrap();
        let before = s.clocks();
        let t = dut
            .step(
                BranchTick {
                    ce: false,
                    ..Default::default()
                },
                ro,
                fb,
            )
            .unwrap();
        assert_edge(s, before);
        assert!(t.sampling.is_none() && t.sample_returned.is_none());
        assert!(!dut.complete());
        if dut.framebuffer_drained() && s.idle() {
            break;
        }
        assert!(wall + 1 < MAX, "combined fault drain watchdog");
    }
    assert!(dut.framebuffer_drained() && s.idle());
}
fn ro_abort(driver_failure: bool, name: &str) {
    let i = inputs(1, false);
    let s = fixture(&i, true);
    let (mut ro, mut fb) = s.views();
    let mut dut = PixelBranches::new(i.context, i.light, vec![i.slot], MAX).unwrap();
    let mut admitted = false;
    for wall in 0..MAX {
        let before = s.clocks();
        let t = dut
            .step(
                BranchTick {
                    quad: (!admitted).then(|| i.quads[0].clone()),
                    ..Default::default()
                },
                &mut ro,
                &mut fb,
            )
            .unwrap();
        assert_edge(&s, before);
        admitted |= t.live.model.quad_accepted;
        if s.ro_active() && s.stats().ro_beats >= 2 {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(s.ro_active() && !s.gpu_idle());
    if driver_failure {
        s.fail_next_ro_poll();
        let before = s.clocks();
        let error = dut
            .step(BranchTick::default(), &mut ro, &mut fb)
            .unwrap_err();
        assert!(error.contains("driver RO poll failure"), "{error}");
        assert_eq!(s.clocks(), before, "failure occurred before FB clock owner");
        assert!(dut.faulted() && !s.physical_error());
    } else {
        dut.abort();
    }
    fault_drain(&mut dut, &s, &mut ro, &mut fb);
    assert_eq!(
        s.image(),
        i.initial,
        "abort before framebuffer write changes no bytes"
    );
    let stats = s.stats();
    assert_eq!(
        stats.ro_delivered_complete, 0,
        "no successful refill publication after fault"
    );
    assert_eq!(stats.ro_terminals, 1);
    assert_eq!(stats.ro_discarded_parents, 1);
    assert!(stats.ro_discarded_beats > 0);
    assert_eq!(stats.driver_failures, u64::from(driver_failure));
    assert_eq!(stats.bg_submitted, stats.bg_completed);
    assert!(s
        .trace()
        .iter()
        .any(|t| t.discarded && t.event == "complete"));
    s.save(&artifacts(), name);
}
#[test]
fn abort_active_ro_uses_external_discard_and_combined_drain() {
    ro_abort(false, "abort-ro");
}
#[test]
fn driver_fault_before_tick_preserves_edge_and_drains_real_ro() {
    ro_abort(true, "driver-fault-ro");
}
#[test]
fn abort_active_fb_write_keeps_continuous_source_until_real_ack() {
    let i = inputs(1, true);
    let s = fixture(&i, true);
    let (mut ro, mut fb) = s.views();
    let mut dut = PixelBranches::new(i.context, i.light, vec![i.slot], MAX).unwrap();
    let mut admitted = false;
    let mut accepted_writes = Vec::new();
    for wall in 0..MAX {
        let before = s.clocks();
        let t = dut
            .step(
                BranchTick {
                    quad: (!admitted).then(|| i.quads[0].clone()),
                    finish: admitted,
                    ..Default::default()
                },
                &mut ro,
                &mut fb,
            )
            .unwrap();
        assert_edge(&s, before);
        admitted |= t.live.model.quad_accepted;
        if t.live.model.framebuffer.response.accepted {
            let r = t.live.model.framebuffer.request.unwrap();
            if r.write {
                accepted_writes.push(r.address_bytes as usize);
            }
        }
        if s.fb_write_active() && s.stats().fb_write_beats >= 4 && s.image() != i.initial {
            break;
        }
        assert!(wall + 1 < MAX);
    }
    assert!(s.fb_write_active());
    assert_eq!(accepted_writes.len(), 1);
    let before_abort = s.image();
    assert_ne!(
        before_abort, i.initial,
        "actual DQ writes happened before abort"
    );
    assert_ne!(before_abort, i.expected, "whole render is not complete");
    dut.abort();
    fault_drain(&mut dut, &s, &mut ro, &mut fb);
    let mut expected_partial = i.initial.clone();
    for a in accepted_writes {
        expected_partial[a..a + 128].copy_from_slice(&i.expected[a..a + 128]);
    }
    assert_eq!(
        s.image(),
        expected_partial,
        "only accepted physical burst completes; no rollback"
    );
    let stats = s.stats();
    assert_eq!(stats.fb_writes, 1);
    assert_eq!(stats.fb_write_beats, 16);
    assert_eq!(stats.fb_terminals, stats.fb_reads + stats.fb_writes);
    assert_eq!(stats.ro_submitted, 0);
    assert!(!dut.complete() && stats.bg_submitted == stats.bg_completed);
    s.save(&artifacts(), "abort-fb-write");
}
