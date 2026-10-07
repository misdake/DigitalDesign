//! Pregenerated paced raster rows with independent controlled branch answers.
//! The DUT computes real registered Final; the sink accepts actual ROP rows.
use gpu_v2::{
    framebuffer::ports::{Blend, Context as RopContext, DepthFunc, Header},
    lighting::ports::*,
    system::pixel::{
        dispatch::{CommonContext, Input, SampleContext},
        foundation::{self, Pipeline, Store},
        Basic, LightWrite, PixelKey, SampleWrite,
    },
    texture::ports::Filter,
};
use std::collections::{BTreeMap, VecDeque};

fn context(unlit: bool, untextured: bool, epoch: u16) -> CommonContext {
    CommonContext {
        lighting: LightingContext {
            material: Material {
                unlit,
                specular_color: [31, 17, 93],
                shininess_code: 8,
            },
            light: Light {
                direction: [0, 0, 16384],
                ambient: 32,
                directional: 192,
            },
            projection: Projection::default(),
            epoch,
        },
        sample: (!untextured).then_some(SampleContext {
            slot: 0,
            size_log2: 6,
            filter: Filter::Nearest,
            bias_q8: 0,
        }),
        alpha: (epoch * 17) as u8,
        rop: RopContext {
            depth: DepthFunc::LessEqual,
            depth_write: true,
            blend: Blend::SrcOver,
        },
    }
}

fn input(id: foundation::Begin, index: usize) -> Input {
    Input {
        context: id.context,
        header: id.header,
        basic: std::array::from_fn(|lane| Basic {
            tint: [
                (index * 31 + lane * 9) as u8,
                (index * 47 + lane * 13) as u8,
                (index * 67 + lane * 17) as u8,
            ],
            depth: (60000 - index * 31 - lane * 17) as u16,
        }),
        light: std::array::from_fn(|lane| CompactPixelInput {
            normal: [0, (lane * 64) as i16, 1024],
            ndc: [(index * 73) as i32 - 8192, (lane * 123) as i32],
        }),
        uv_q16: std::array::from_fn(|lane| {
            [
                (index * 67 + lane * 811) as i64 - 16384,
                (index * 173 + lane * 397) as i64,
            ]
        }),
        force_coarsest: index.is_multiple_of(9),
    }
}

fn light(index: usize, lane: u8) -> LightingOutput {
    LightingOutput {
        g: ((index * 31 + usize::from(lane) * 47) % 512) as u16,
        h: ((index * 17 + usize::from(lane) * 29) % 257) as u16,
    }
}
fn texture(index: usize, lane: u8) -> [u8; 3] {
    [
        (index * 37 + usize::from(lane) * 61) as u8,
        (index * 23) as u8,
        (index * 71 + usize::from(lane) * 17) as u8,
    ]
}
fn nearest(n: u64, d: u64) -> u64 {
    let q = n / d;
    let r = n % d;
    q + u64::from(2 * r > d || (2 * r == d && q & 1 != 0))
}
fn golden(q: &Input, context: CommonContext, index: usize, lane: usize) -> [u32; 2] {
    if q.header.mask & (1 << lane) == 0 {
        return [0; 2];
    }
    let l = if context.lighting.material.unlit {
        LightingOutput { g: 256, h: 0 }
    } else {
        light(index, lane as u8)
    };
    let t = if context.sample.is_none() {
        [255; 3]
    } else {
        texture(index, lane as u8)
    };
    let rgb: [u8; 3] = std::array::from_fn(|c| {
        nearest(
            nearest(u64::from(q.basic[lane].tint[c]) * u64::from(t[c]), 255) * u64::from(l.g)
                + u64::from(context.lighting.material.specular_color[c]) * u64::from(l.h),
            256,
        )
        .min(255) as u8
    });
    [
        u32::from_le_bytes([rgb[0], rgb[1], rgb[2], context.alpha]),
        u32::from(q.basic[lane].depth),
    ]
}

struct ResultLog {
    publish: Vec<usize>,
    finals: Vec<usize>,
    rows: Vec<usize>,
    backpressure: usize,
}

fn controlled(stall: bool, sparse: bool, mixed: bool, period: usize) -> ResultLog {
    controlled_layout(stall, sparse, period, (mixed, mixed), 0)
}

fn controlled_layout(
    stall: bool,
    sparse: bool,
    period: usize,
    bypass: (bool, bool),
    layout: usize,
) -> ResultLog {
    let mut p = Pipeline::new(40_000).unwrap();
    let a = context(false, false, 11);
    let b = context(bypass.0, bypass.1, 23);
    let ia = p.open_draw(a).unwrap().unwrap();
    let ib = p.open_draw(b).unwrap().unwrap();
    assert!(
        p.open_draw(a).unwrap().is_none(),
        "third draw ignored live banks"
    );
    let count = if layout == 0 {
        96
    } else {
        6 * foundation::GLOBAL_SLOTS
    };
    let inputs: Vec<_> = (0..count)
        .map(|i| {
            let use_b = match layout {
                0 => i >= 48,
                1 => (i / foundation::GLOBAL_SLOTS) % 3 == 1,
                _ => (i / foundation::GLOBAL_SLOTS).is_multiple_of(3),
            };
            let id = if use_b { ib } else { ia };
            input(
                foundation::Begin {
                    context: id,
                    header: Header {
                        x: (2 * i) as u16,
                        y: 0,
                        mask: if sparse { [0, 1, 6, 15, 5][i % 5] } else { 15 },
                    },
                    force_coarsest: false,
                },
                i,
            )
        })
        .collect();
    let beats: Vec<_> = inputs
        .iter()
        .map(|&i| foundation::source_beats(i).unwrap())
        .collect();
    let mut source = 0;
    let mut row = 0;
    let mut owner = BTreeMap::new();
    let mut lights = VecDeque::new();
    let mut samples = VecDeque::new();
    let mut received = Vec::new();
    let mut closed = [false; 2];
    let mut releases = Vec::new();
    let mut log = ResultLog {
        publish: Vec::new(),
        finals: Vec::new(),
        rows: Vec::new(),
        backpressure: 0,
    };
    for wall in 0..40_000 {
        let ce = !stall || wall % 17 != 3 && wall % 17 != 4;
        let light_ready = !stall || wall % 13 != 6;
        let sample_ready = !stall || wall % 19 != 7;
        let rop_ready = !stall || wall % 11 < 7;
        let offer =
            (source < inputs.len() && wall >= 16 + source * period).then(|| beats[source][row]);
        let lw = lights
            .front()
            .copied()
            .filter(|&(due, _)| ce && due <= wall && (!stall || wall % 7 != 2))
            .map(|(_, w)| w);
        let sw = samples
            .front()
            .copied()
            .filter(|&(due, _)| ce && due <= wall && (!stall || wall % 5 != 1))
            .map(|(_, w)| w);
        let step = p
            .tick(foundation::Tick {
                ce,
                input: offer,
                lighting_ready: light_ready,
                sampling_ready: sample_ready,
                light: lw,
                sample: sw,
                final_issue_ready: !stall || wall % 23 != 10,
                final_result_ready: !stall || wall % 29 < 23,
                rop_ready,
            })
            .unwrap_or_else(|e| panic!("wall {wall} source {source} row {row}: {e}"));
        if lw.is_some() {
            lights.pop_front();
        }
        if sw.is_some() {
            samples.pop_front();
        }
        if offer.is_some() && !step.input_accepted && ce {
            log.backpressure += 1;
        }
        if let Some(ticket) = step.published {
            owner.insert(ticket.serial, source);
            log.publish.push(wall);
        }
        if step.input_accepted {
            row += 1;
            if row == 8 {
                row = 0;
                source += 1;
            }
        }
        if step.lighting_accepted {
            let job = step.signals.lighting.unwrap();
            let index = owner[&job.key.ticket.serial];
            assert_eq!(job.pixel, inputs[index].light[usize::from(job.key.lane)]);
            lights.push_back((
                wall + 12,
                LightWrite {
                    key: job.key,
                    value: light(index, job.key.lane),
                },
            ));
        }
        if step.sampling_accepted {
            let job = step.signals.sampling.unwrap();
            let index = owner[&job.ticket.serial];
            assert_eq!(job.uv_q16, inputs[index].uv_q16);
            assert_eq!(job.force_coarsest, inputs[index].force_coarsest);
            for lane in 0..4u8 {
                if job.mask & (1 << lane) != 0 {
                    samples.push_back((
                        wall + 14 + usize::from(lane) * 2,
                        SampleWrite {
                            key: PixelKey {
                                ticket: job.ticket,
                                lane,
                            },
                            rgb: texture(index, lane),
                        },
                    ));
                }
            }
        }
        if step.final_stage.accepted {
            log.finals.push(wall);
        }
        if ce && rop_ready {
            if let Some(r) = step.signals.rop {
                let index = owner[&r.ticket.serial];
                let q = &inputs[index];
                let context = if q.context == ia { a } else { b };
                assert_eq!(r.header, q.header);
                assert_eq!(r.context, context.rop);
                assert_eq!(
                    r.data,
                    golden(q, context, index, usize::from(r.row / 2))[usize::from(r.row & 1)],
                    "ROP input quad {index} row {}",
                    r.row
                );
                received.push((index, r.row));
                log.rows.push(wall);
            }
        }
        for store in [
            Store::Basic,
            Store::Light,
            Store::Sample,
            Store::Output,
            Store::LightDone,
            Store::SampleDone,
        ] {
            for write in [false, true] {
                assert!(
                    step.accesses
                        .iter()
                        .filter(|a| a.store == store && a.write == write)
                        .count()
                        <= 1,
                    "extra {store:?} port wall {wall}"
                );
            }
            for read in step
                .accesses
                .iter()
                .filter(|a| a.store == store && !a.write)
            {
                assert!(
                    !step
                        .accesses
                        .iter()
                        .any(|w| w.store == store && w.write && w.address == read.address),
                    "RAM R/W collision {store:?} at wall {wall}"
                );
            }
        }
        if source >= (if layout == 0 { 48 } else { count }) && !closed[0] {
            p.close_draw(ia).unwrap();
            closed[0] = true;
        }
        if source == count && !closed[1] {
            p.close_draw(ib).unwrap();
            closed[1] = true;
        }
        releases.extend(step.released_draws);
        if p.idle() && source == count {
            assert!(lights.is_empty() && samples.is_empty());
            let expected: Vec<_> = inputs
                .iter()
                .enumerate()
                .filter(|(_, q)| q.header.mask != 0)
                .flat_map(|(i, _)| (0..8u8).map(move |r| (i, r)))
                .collect();
            assert_eq!(received, expected);
            assert_eq!(releases, vec![ia, ib]);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/pixel-foundation-20261007");
            std::fs::create_dir_all(&dir).unwrap();
            let mut trace = String::from("event,wall\n");
            for (name, edges) in [
                ("published", &log.publish),
                ("final_accepted", &log.finals),
                ("rop_row", &log.rows),
            ] {
                for edge in edges {
                    trace.push_str(&format!("{name},{edge}\n"));
                }
            }
            std::fs::write(
                dir.join(format!(
                    "controlled-{stall}-{sparse}-{period}-{}-{}-{layout}.csv",
                    bypass.0, bypass.1
                )),
                trace,
            )
            .unwrap();
            assert!(
                p.open_draw(a).unwrap().is_some(),
                "retired bank not reusable"
            );
            return log;
        }
    }
    panic!("controlled pipeline watchdog");
}

#[test]
fn same_slot_active_bypass_active_and_initial_bypass_active() {
    for bypass in [(true, false), (false, true), (true, true)] {
        for layout in [1, 2] {
            controlled_layout(false, false, 8, bypass, layout);
        }
    }
}

#[test]
fn full_quads_publish_every_eight_edges_final_every_two_and_rop_rows_every_edge() {
    let log = controlled(false, false, false, 8);
    assert_eq!(log.publish.len(), 96);
    assert_eq!(log.finals.len(), 384);
    assert_eq!(log.rows.len(), 768);
    for pair in log.publish.windows(2) {
        assert_eq!(pair[1] - pair[0], 8, "writer restart gap");
    }
    for pair in log.finals.windows(2) {
        assert_eq!(pair[1] - pair[0], 2, "Final issue gap");
    }
    for pair in log.rows.windows(2) {
        assert_eq!(pair[1] - pair[0], 1, "ROP row gap");
    }
}

#[test]
fn faster_source_bursts_sparse_masks_bypass_and_independent_stalls() {
    controlled(false, false, false, 4);
    let log = controlled(true, true, true, 4);
    assert!(log.backpressure > 0);
}

#[test]
fn empty_draw_boundaries_and_third_draw_backpressure() {
    let mut p = Pipeline::new(128).unwrap();
    let a = context(true, true, 3);
    let ia = p.open_draw(a).unwrap().unwrap();
    let ib = p.open_draw(a).unwrap().unwrap();
    p.close_draw(ia).unwrap();
    p.close_draw(ib).unwrap();
    assert!(p.open_draw(a).unwrap().is_none());
    assert!(p
        .tick(foundation::Tick::default())
        .unwrap()
        .released_draws
        .is_empty());
    let step = p
        .tick(foundation::Tick {
            ce: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(step.released_draws, vec![ia, ib]);
    let next = p.open_draw(a).unwrap().unwrap();
    assert_ne!(next, ia);
    assert!(p.context(ia).is_err());
}

#[test]
fn partial_tail_counts_capacity_but_cannot_join_before_last_row() {
    let mut p = Pipeline::new(128).unwrap();
    let id = p.open_draw(context(true, true, 3)).unwrap().unwrap();
    for _ in 0..foundation::GLOBAL_SLOTS {
        p.tick(foundation::Tick {
            ce: true,
            ..Default::default()
        })
        .unwrap();
    }
    let q = input(
        foundation::Begin {
            context: id,
            header: Header {
                x: 0,
                y: 0,
                mask: 15,
            },
            force_coarsest: false,
        },
        1,
    );
    for (row, beat) in foundation::source_beats(q).unwrap().into_iter().enumerate() {
        let step = p
            .tick(foundation::Tick {
                ce: true,
                input: Some(beat),
                final_issue_ready: true,
                final_result_ready: true,
                rop_ready: true,
                ..Default::default()
            })
            .unwrap();
        assert!(step.input_accepted);
        assert_eq!(p.live_status(), 1, "private tail consumes one slot");
        assert_eq!(step.published.is_some(), row == 7);
        assert!(!step.final_stage.accepted);
        assert!(step.signals.rop.is_none());
        if row < 7 {
            assert!(p.close_draw(id).is_err());
        }
    }
    p.close_draw(id).unwrap();
}

#[test]
fn unsolicited_result_latches_fault_and_wall_watchdog_is_bounded() {
    let mut p = Pipeline::new(64).unwrap();
    let id = p.open_draw(context(false, false, 3)).unwrap().unwrap();
    for _ in 0..foundation::GLOBAL_SLOTS {
        p.tick(foundation::Tick {
            ce: true,
            ..Default::default()
        })
        .unwrap();
    }
    let q = input(
        foundation::Begin {
            context: id,
            header: Header {
                x: 0,
                y: 0,
                mask: 15,
            },
            force_coarsest: false,
        },
        1,
    );
    p.tick(foundation::Tick {
        ce: true,
        input: Some(foundation::source_beats(q).unwrap()[0]),
        ..Default::default()
    })
    .unwrap();
    let ticket = p.ticket(0).unwrap();
    assert!(p
        .tick(foundation::Tick {
            ce: true,
            light: Some(LightWrite {
                key: PixelKey { ticket, lane: 0 },
                value: LightingOutput { g: 256, h: 0 },
            }),
            ..Default::default()
        })
        .err()
        .unwrap()
        .contains("precedes actual issue"));
    assert!(p.faulted());
    assert!(p.tick(foundation::Tick::default()).is_err());
    let mut bounded = Pipeline::new(2).unwrap();
    for _ in 0..2 {
        bounded.tick(foundation::Tick::default()).unwrap();
    }
    assert!(bounded
        .tick(foundation::Tick::default())
        .err()
        .unwrap()
        .contains("watchdog"));
}
