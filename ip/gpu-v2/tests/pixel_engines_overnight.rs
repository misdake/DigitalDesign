//! Actual Lighting/Sampling/Final engines with a byte-backed MC fixture. ROP
//! consumes rows here; this is not full cache RTL or shared-MC performance proof.
use gpu_v2::framebuffer::ports::{Blend, Context as RopContext, DepthFunc, Header};
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, oracle},
    LightingProfile, LightingQuantization,
};
use gpu_v2::system::pixel::{
    composition::{self, FinalBranches},
    dispatch::*,
    final_rgb, Basic,
};
use gpu_v2::texture::{
    ports::{self as tex, RefillEvent, RefillPort},
    sim::oracle as tex_oracle,
};
use std::collections::{BTreeMap, VecDeque};
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod support;

struct Memory {
    bytes: Vec<u8>,
    queued: VecDeque<(u64, u64)>,
    active: Option<(u64, u64, usize)>,
    wall: u64,
    next: u64,
    beats: u64,
}
impl RefillPort for Memory {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        if bytes != 128 || address & 127 != 0 || self.queued.len() >= 4 {
            return Err("fixture burst/queue".into());
        }
        let id = self.next;
        self.next += 1;
        self.queued.push_back((id, address));
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        self.wall += 1;
        if let Some((id, address, beat)) = self.active {
            if beat == 16 {
                if self.wall.is_multiple_of(7) {
                    self.active = None;
                    return Ok(vec![RefillEvent::Complete { id }]);
                }
            } else if self.wall % 3 != 1 {
                let offset = (address - u64::from(support::BASE)) as usize + beat * 8;
                let raw = self.bytes.get(offset..offset + 8).ok_or("fixture bounds")?;
                let data = u64::from_le_bytes(raw.try_into().unwrap());
                self.active = Some((id, address, beat + 1));
                self.beats += 1;
                return Ok(vec![RefillEvent::Beat {
                    id,
                    index: beat,
                    data,
                    last: beat == 15,
                }]);
            }
        } else if let Some((id, address)) = self.queued.pop_front() {
            self.active = Some((id, address, 0));
            return Ok(vec![RefillEvent::Started { id }]);
        }
        Ok(vec![])
    }
}

fn context(mode: usize) -> CommonContext {
    CommonContext {
        lighting: LightingContext {
            material: Material {
                unlit: mode & 1 != 0,
                specular_color: [31, 17, 9],
                shininess_code: 8,
            },
            light: Light {
                direction: [0, 0, 16384],
                ambient: 32,
                directional: 192,
            },
            projection: Projection::default(),
            epoch: mode as u16 + 11,
        },
        sample: (mode & 2 == 0).then_some(SampleContext {
            slot: 0,
            size_log2: 6,
            filter: tex::Filter::Trilinear,
            bias_q8: 64,
        }),
        alpha: 193,
        rop: RopContext {
            depth: DepthFunc::Less,
            depth_write: true,
            blend: Blend::Replace,
        },
    }
}

#[test]
fn live_independent_queues_join_real_results_through_ce_and_wrap() {
    const LIMIT: u64 = 150_000;
    let slots = [support::slot(6, true)];
    let asset = support::asset(slots[0], support::pattern);
    let mut memory = Memory {
        bytes: asset.clone(),
        queued: VecDeque::new(),
        active: None,
        wall: 0,
        next: 0,
        beats: 0,
    };
    let mut golden_mem = support::Image {
        bytes: asset,
        requests: vec![],
    };
    let mut golden_cache = tex_oracle::Cache::new(slots.to_vec()).unwrap();
    let mut backend = FinalBranches::new(
        Config {
            max_wall: LIMIT,
            ..Default::default()
        },
        &slots,
    )
    .unwrap();
    let ids = std::array::from_fn::<_, 4, _>(|i| backend.set_context(i as u8, context(i)).unwrap());
    let inputs: Vec<_> = (0..36)
        .map(|i| Input {
            force_coarsest: false,
            context: ids[i % 4],
            header: Header {
                x: (i % 8 * 2) as u16,
                y: (i / 8 * 2) as u8,
                mask: [15, 5, 10, 3, 0][i % 5],
            },
            basic: std::array::from_fn(|lane| Basic {
                tint: [(i * 23 + lane) as u8, 181, (i * 7 + lane * 17) as u8],
                depth: (50000 - i * 13 - lane) as u16,
            }),
            light: std::array::from_fn(|lane| CompactPixelInput {
                normal: [[0, 0, 1024], [256, 384, 921], [0, 0, 0], [-1024, 0, 0]][lane],
                ndc: [(i as i32 * 1024 - 16384) / 4, (lane as i32 * 8192) / 4],
            }),
            uv_q16: std::array::from_fn(|lane| {
                [
                    (i as i64 * 4096 - 131072 + (lane as i64 & 1) * 8191) / 4,
                    (i as i64 * 2027 + (lane as i64 >> 1) * 4096) / 4,
                ]
            }),
        })
        .collect();
    let mut goldens = Vec::new();
    for (i, q) in inputs.iter().enumerate() {
        let c = context(i % 4);
        let light_cfg = oracle::Config::from_counted(counted::Config::lit_queue_resource_profile(
            LightingProfile::Fast,
            LightingQuantization::CompensatedFloor,
        ));
        let lighting = std::array::from_fn::<_, 4, _>(|lane| {
            if c.lighting.material.unlit {
                LightingOutput { g: 256, h: 0 }
            } else {
                oracle::evaluate_output(
                    q.light[lane].expanded().unwrap(),
                    c.lighting.material,
                    c.lighting.light,
                    c.lighting.projection,
                    light_cfg,
                )
                .unwrap()
            }
        });
        let mut colors = [[255; 3]; 4];
        if let Some(s) = c.sample.filter(|_| q.header.mask != 0) {
            let quad = tex::QuadInput {
                force_coarsest: false,
                quad_id: 0,
                mask: q.header.mask,
                uv: q.uv_q16.map(|v| v.map(|x| x as f64 / 65536.0)),
                slot: s.slot,
                material_size_log2: s.size_log2,
                filter: s.filter,
                lod_bias: f64::from(s.bias_q8) / 256.0,
            };
            for p in tex_oracle::sample(
                &quad,
                &mut golden_cache,
                &mut golden_mem,
                tex::Config::counted(),
            )
            .unwrap()
            .pixels
            {
                colors[usize::from(p.lane)] = p.rgb;
            }
        }
        goldens.push((lighting, colors));
    }
    let mut offered = 0;
    let mut accepted = VecDeque::new();
    let mut owners = BTreeMap::new();
    let mut retired_rows = 0;
    let mut output_quads = 0;
    let mut complete = false;
    for cycle in 0..LIMIT {
        let ce = cycle % 13 != 1 && cycle % 13 != 2;
        let signals = backend.branches().dispatch().signals();
        let final_ready = cycle % 7 != 0;
        let tick = composition::Tick {
            ce,
            input: inputs.get(offered).copied(),
            lighting_result_ready: cycle % 17 != 0,
            sampling_issue_ready: cycle % 19 != 0,
            sampling_result_ready: cycle % 23 != 0,
            final_issue_ready: final_ready,
            final_result_ready: cycle % 31 < 21,
            rop_ready: cycle % 29 < 19,
            finish: offered == inputs.len(),
        };
        if let Some(j) = signals.final_input {
            let i: usize = owners[&j.key.ticket.serial];
            let lane = usize::from(j.key.lane);
            assert_eq!(j.light, goldens[i].0[lane]);
            assert_eq!(j.texture, goldens[i].1[lane]);
            assert_eq!(j.tint, inputs[i].basic[lane].tint);
            assert_eq!(j.depth, inputs[i].basic[lane].depth);
        }
        if ce && tick.rop_ready {
            if let Some(row) = signals.rop {
                assert_eq!(row.row, retired_rows);
                let i: usize = owners[&row.ticket.serial];
                let lane = usize::from(row.row / 2);
                let expected = if inputs[i].header.mask & (1 << lane) == 0 {
                    0
                } else if row.row & 1 != 0 {
                    u32::from(inputs[i].basic[lane].depth)
                } else {
                    let rgb = final_rgb(
                        inputs[i].basic[lane].tint,
                        goldens[i].1[lane],
                        goldens[i].0[lane],
                        context(i % 4).lighting.material.specular_color,
                    )
                    .unwrap();
                    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 193])
                };
                assert_eq!(row.data, expected, "actual final row {i}/{lane}");
                retired_rows += 1;
                if retired_rows == 8 {
                    retired_rows = 0;
                    output_quads += 1;
                }
            }
        }
        let step = backend.step(&mut memory, tick).unwrap();
        if let Some(ticket) = step.branches.dispatch.dispatched {
            owners.insert(ticket.serial, accepted.pop_front().unwrap());
        }
        if step.branches.dispatch.input_accepted {
            if inputs[offered].header.mask != 0 {
                accepted.push_back(offered);
            }
            offered += 1;
        }
        if step.branches.dispatch.complete && backend.complete() {
            complete = true;
            break;
        }
    }
    assert!(complete, "bounded live engines failed to drain");
    assert_eq!(
        output_quads,
        inputs.iter().filter(|q| q.header.mask != 0).count()
    );
    assert!(memory.beats > 0);
    assert!(memory.next > 0);
    assert_eq!(memory.beats, memory.next * 16);
    assert!(memory.active.is_none() && memory.queued.is_empty());
    assert_eq!(
        backend.branches().dispatch().stats.lighting_jobs,
        inputs
            .iter()
            .filter(|q| q.header.mask != 0 && q.context.slot & 1 == 0)
            .count() as u64
    );
    assert_eq!(
        backend.branches().sampling().stats.compilations,
        inputs
            .iter()
            .filter(|q| q.header.mask != 0 && q.context.slot & 2 == 0)
            .count() as u64
    );
}
