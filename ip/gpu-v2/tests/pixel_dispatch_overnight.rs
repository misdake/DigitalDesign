//! Controlled-result contract tests, not full numerical-engine integration.
use gpu_v2::framebuffer::ports::{Blend, Context as RopContext, DepthFunc, Header};
use gpu_v2::lighting::ports::{
    CompactPixelInput, Light, LightingContext, LightingOutput, Material, Projection,
};
use gpu_v2::system::pixel::{
    dispatch::*, final_rgb, Basic, LightWrite, PixelKey, SampleWrite, Ticket,
};
use gpu_v2::texture::ports::Filter;
use std::collections::{BTreeMap, VecDeque};

fn context(unlit: bool, untextured: bool) -> CommonContext {
    CommonContext {
        lighting: LightingContext {
            material: Material {
                unlit,
                specular_color: [5, 11, 23],
                shininess_code: 8,
            },
            light: Light {
                direction: [0, 0, 16384],
                ambient: 32,
                directional: 192,
            },
            projection: Projection {
                ray_scale: [-8192; 2],
                k: 8192,
            },
            epoch: 9,
        },
        sample: (!untextured).then_some(SampleContext {
            slot: 0,
            size_log2: 6,
            filter: Filter::Trilinear,
            bias_q8: 0,
        }),
        alpha: 187,
        rop: RopContext {
            depth: DepthFunc::LessEqual,
            depth_write: true,
            blend: Blend::SrcOver,
        },
    }
}

fn input(id: ContextId, index: usize, mask: u8) -> Input {
    Input {
        context: id,
        header: Header {
            x: ((index % 16) * 2) as u16,
            y: ((index / 16) * 2) as u8,
            mask,
        },
        basic: std::array::from_fn(|lane| Basic {
            tint: [(index * 17 + lane) as u8, (index + lane * 31) as u8, 197],
            depth: (index * 113 + lane) as u16,
        }),
        light: [CompactPixelInput {
            normal: [0, 0, 1024],
            ndc: [0; 2],
        }; 4],
        uv_q18: [[-17, 229]; 4],
    }
}

fn light_value(key: PixelKey) -> LightingOutput {
    LightingOutput {
        g: 160 + u16::from(key.lane),
        h: 16 + u16::from(key.lane),
    }
}
fn texture_value(key: PixelKey) -> [u8; 3] {
    [91 + key.lane, 181, 255]
}

#[test]
fn all_bypasses_order_ports_context_and_bounded_backpressure() {
    const LIMIT: u64 = 20_000;
    let mut d = Dispatcher::new(Config {
        ingress: 2,
        lighting: 1,
        sampling: 1,
        max_wall: LIMIT,
        ..Default::default()
    })
    .unwrap();
    let contexts = std::array::from_fn::<_, 4, _>(|i| {
        d.set_context(i as u8, context(i & 1 != 0, i & 2 != 0))
            .unwrap()
    });
    let inputs: Vec<_> = (0..72)
        .map(|i| input(contexts[i % 4], i, [15, 1, 6, 10, 0, 3][i % 6]))
        .collect();
    let mut offered = 0;
    let mut accepted = VecDeque::new();
    let mut owners = BTreeMap::new();
    let mut light = VecDeque::new();
    let mut sample = VecDeque::new();
    let mut final_results = VecDeque::new();
    let mut rows = Vec::new();
    let mut rop_order = Vec::new();
    let mut completed = false;
    for cycle in 0..LIMIT {
        let ce = cycle % 17 != 3 && cycle % 17 != 4;
        let s = d.signals();
        let light_write = light
            .front()
            .copied()
            .filter(|(due, _)| *due <= cycle && cycle % 7 != 0)
            .map(|(_, key)| LightWrite {
                key,
                value: light_value(key),
            });
        let sample_write = sample
            .front()
            .copied()
            .filter(|(due, _)| *due <= cycle && cycle % 11 != 0)
            .map(|(_, key)| SampleWrite {
                key,
                rgb: texture_value(key),
            });
        let final_result = final_results
            .front()
            .copied()
            .filter(|(due, _, _)| *due <= cycle && s.final_output_ready && cycle % 13 != 0)
            .map(|(_, key, rgb)| (key, rgb));
        let tick = Tick {
            ce,
            input: inputs.get(offered).copied(),
            lighting_ready: cycle % 19 != 0,
            sampling_ready: cycle % 23 != 0,
            light: light_write,
            sample: sample_write,
            final_ready: cycle % 7 != 2,
            final_result,
            rop_ready: cycle % 31 < 23,
            finish: offered == inputs.len(),
        };
        if ce {
            if tick.lighting_ready {
                if let Some(job) = s.lighting {
                    assert!(!d.context(job.context).unwrap().lighting.material.unlit);
                    for lane in 0..4 {
                        if job.mask & (1 << lane) != 0 {
                            light.push_back((
                                cycle + 7,
                                PixelKey {
                                    ticket: job.ticket,
                                    lane,
                                },
                            ));
                        }
                    }
                }
            }
            if tick.sampling_ready {
                if let Some(job) = s.sampling {
                    assert!(d.context(job.context).unwrap().sample.is_some());
                    assert_eq!(job.uv_q18, [[-17, 229]; 4]);
                    for lane in 0..4 {
                        if job.mask & (1 << lane) != 0 {
                            sample.push_back((
                                cycle + 29,
                                PixelKey {
                                    ticket: job.ticket,
                                    lane,
                                },
                            ));
                        }
                    }
                }
            }
            if let Some(job) = s.final_input {
                if tick.final_ready {
                    let i: usize = owners[&job.key.ticket.serial];
                    let q = inputs[i];
                    let ctx = context((i % 4) & 1 != 0, (i % 4) & 2 != 0);
                    let expected_light = if ctx.lighting.material.unlit {
                        LightingOutput { g: 256, h: 0 }
                    } else {
                        light_value(job.key)
                    };
                    let expected_texture = if ctx.sample.is_none() {
                        [255; 3]
                    } else {
                        texture_value(job.key)
                    };
                    assert_eq!(job.tint, q.basic[usize::from(job.key.lane)].tint);
                    assert_eq!(job.depth, q.basic[usize::from(job.key.lane)].depth);
                    assert_eq!(job.light, expected_light);
                    assert_eq!(job.texture, expected_texture);
                    let rgb = final_rgb(job.tint, job.texture, job.light, job.specular).unwrap();
                    final_results.push_back((cycle + 4, job.key, rgb));
                }
            }
            if tick.rop_ready {
                if let Some(row) = s.rop {
                    assert_eq!(row.row as usize, rows.len());
                    let i: usize = owners[&row.ticket.serial];
                    let q = inputs[i];
                    let lane = usize::from(row.row / 2);
                    let key = PixelKey {
                        ticket: row.ticket,
                        lane: lane as u8,
                    };
                    let ctx = context((i % 4) & 1 != 0, (i % 4) & 2 != 0);
                    let expected = if q.header.mask & (1 << lane) == 0 {
                        0
                    } else if row.row & 1 == 1 {
                        u32::from(q.basic[lane].depth)
                    } else {
                        let rgb = final_rgb(
                            q.basic[lane].tint,
                            if ctx.sample.is_none() {
                                [255; 3]
                            } else {
                                texture_value(key)
                            },
                            if ctx.lighting.material.unlit {
                                LightingOutput { g: 256, h: 0 }
                            } else {
                                light_value(key)
                            },
                            ctx.lighting.material.specular_color,
                        )
                        .unwrap();
                        u32::from_le_bytes([rgb[0], rgb[1], rgb[2], ctx.alpha])
                    };
                    assert_eq!(row.data, expected, "quad {i} row {}", row.row);
                    assert_eq!(row.header, q.header);
                    assert_eq!(row.context, ctx.rop);
                    rows.push(row.data);
                    if row.row == 7 {
                        rop_order.push(i);
                        rows.clear();
                    }
                }
            }
        }
        let step = d.tick(tick).unwrap();
        if ce && light_write.is_some() {
            light.pop_front();
        }
        if ce && sample_write.is_some() {
            sample.pop_front();
        }
        if ce && final_result.is_some() {
            final_results.pop_front();
        }
        if let Some(ticket) = step.dispatched {
            owners.insert(ticket.serial, accepted.pop_front().unwrap());
        }
        if step.input_accepted {
            if inputs[offered].header.mask != 0 {
                accepted.push_back(offered);
            }
            offered += 1;
        }
        for store in [Store::Basic, Store::Light, Store::Sample, Store::Output] {
            for write in [false, true] {
                assert!(
                    step.accesses
                        .iter()
                        .filter(|a| a.store == store && a.write == write)
                        .count()
                        <= 1
                );
            }
        }
        if step.complete {
            completed = true;
            break;
        }
    }
    assert!(completed, "bounded dispatch did not drain");
    assert_eq!(
        rop_order,
        (0..inputs.len())
            .filter(|&i| inputs[i].header.mask != 0)
            .collect::<Vec<_>>()
    );
    assert!(d.idle());
    assert!(d.stats.peak_status <= 16);
    assert_eq!(d.stats.input, inputs.len() as u64);
    let expected_light = inputs
        .iter()
        .filter(|q| q.context.slot & 1 == 0 && q.header.mask != 0)
        .count();
    let expected_sample = inputs
        .iter()
        .filter(|q| q.context.slot & 2 == 0 && q.header.mask != 0)
        .count();
    assert_eq!(d.stats.lighting_jobs, expected_light as u64);
    assert_eq!(d.stats.sampling_jobs, expected_sample as u64);
    for id in contexts {
        assert_eq!(d.context_references(id).unwrap(), 0);
    }
}

#[test]
fn context_pinned_in_ingress_and_unsolicited_returns_fault() {
    let mut d = Dispatcher::new(Config {
        max_wall: 100,
        ..Default::default()
    })
    .unwrap();
    let id = d.set_context(0, context(false, false)).unwrap();
    d.tick(Tick {
        ce: true,
        input: Some(input(id, 0, 1)),
        ..Default::default()
    })
    .unwrap();
    assert!(d.set_context(0, context(true, true)).is_err());
    let ticket = d
        .tick(Tick {
            ce: true,
            ..Default::default()
        })
        .unwrap()
        .allocated
        .unwrap();
    let error = d
        .tick(Tick {
            ce: true,
            light: Some(LightWrite {
                key: PixelKey { ticket, lane: 0 },
                value: LightingOutput { g: 256, h: 0 },
            }),
            ..Default::default()
        })
        .unwrap_err();
    assert!(error.contains("unsolicited"));
    assert!(d.faulted());
}

#[test]
fn zero_coverage_never_claims_context_or_branches_and_watchdog_is_bounded() {
    let mut d = Dispatcher::new(Config {
        max_wall: 3,
        ..Default::default()
    })
    .unwrap();
    assert!(d.idle());
    assert!(!d.complete());
    let id = d.set_context(0, context(false, false)).unwrap();
    let s = d
        .tick(Tick {
            ce: true,
            input: Some(input(id, 0, 0)),
            ..Default::default()
        })
        .unwrap();
    assert!(s.input_accepted);
    assert!(d.idle());
    assert_eq!(d.context_references(id).unwrap(), 0);
    assert!(d.signals().lighting.is_none());
    assert!(d.signals().sampling.is_none());
    let next = d.set_context(0, context(true, true)).unwrap();
    assert!(d.context(id).is_err());
    assert_ne!(id, next);
    d.tick(Tick::default()).unwrap();
    d.tick(Tick::default()).unwrap();
    assert!(d.tick(Tick::default()).unwrap_err().contains("watchdog"));
    let _host_witness = Ticket { quad: 0, serial: 0 };
}
