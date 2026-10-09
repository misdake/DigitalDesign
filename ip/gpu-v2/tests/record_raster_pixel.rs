//! Test-only semantic record connection. Setup/recovery remain atomic host oracle.
use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
use gpu_v2::{
    framebuffer::ports as fb,
    geometry::record_transport as rec,
    lighting::{ports as light, sim::oracle as light_oracle},
    memory::ports::MemoryPort,
    system::pixel::*,
    texture::{ports as tex, sim::oracle as texture_oracle},
    triangle::{ports as tri, sim::oracle},
    vertex::ports::Transformed,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
#[allow(dead_code)]
#[path = "support/pixel.rs"]
mod pixel;
#[path = "support/record_raster.rs"]
mod rr;
#[allow(dead_code)]
#[path = "support/sdram/shared_pixel.rs"]
mod shared;
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod texture;

const CONTEXT: u64 = 93;
const TEX_BASE: u32 = 0x10000;
const BG_BASE: u64 = 0x20000;
fn artifacts() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-pixel-branches/record-raster-integration")
}
fn input(id: u32, points: [[f64; 2]; 3], color: [u16; 3], normal: [i16; 3]) -> tri::Input {
    tri::Input {
        id,
        vertices: std::array::from_fn(|i| Transformed {
            clip: [
                ((points[i][0] / 16.0 - 1.0) * 65536.0).round_ties_even() as i32,
                ((1.0 - points[i][1] / 16.0) * 65536.0).round_ties_even() as i32,
                0,
                65536,
            ],
            normal,
            uv: [[64, 96], [4095, 96], [64, 4095]][i],
            rgb565: color[i],
        }),
    }
}
fn corpus() -> Vec<tri::Input> {
    vec![
        input(
            1,
            [[4., 4.], [20., 4.], [4., 20.]],
            [0xf800; 3],
            [0, 0, 16384],
        ),
        input(
            2,
            [[4.1, 4.1], [5.7, 4.1], [4.1, 5.7]],
            [0x07e0; 3],
            [-8192, 4096, 12288],
        ),
        input(
            3,
            [[-130., 6.], [5., 6.], [5., 25.]],
            [0x1234, 0xabcd, 0xffff],
            [8192, -4096, 16384],
        ),
        input(
            4,
            [[-50., -50.], [-40., -50.], [-50., -40.]],
            [0x001f; 3],
            [0, 0, 0],
        ),
    ]
}
fn reports(inputs: &[tri::Input]) -> Vec<oracle::Report> {
    assert!(inputs.len() <= rr::MAX_SOURCES as usize);
    inputs
        .iter()
        .map(|i| oracle::run(i, rr::profile()).unwrap())
        .collect()
}
// Bounded external oracle record producer. Consumer Reader never sees Reports.
struct Producer {
    batches: Vec<Vec<rr::Encoded>>,
    source: usize,
    fan: usize,
    row: usize,
    owner_accepted: bool,
    end_sent: bool,
    key: Option<rec::Key>,
    stopped: bool,
    blocked: u64,
}
impl Producer {
    fn new(reports: &[oracle::Report]) -> Self {
        let batches: Vec<_> = reports
            .iter()
            .map(|r| {
                assert!(r.triangles.len() <= 6);
                (0..r.triangles.len())
                    .map(|fan| rr::Encoded::from_report(r, fan).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(batches.iter().map(Vec::len).sum::<usize>() <= rr::MAX_RECORDS as usize);
        Self {
            batches,
            source: 0,
            fan: 0,
            row: 0,
            owner_accepted: false,
            end_sent: false,
            key: None,
            stopped: false,
            blocked: 0,
        }
    }
    fn owner(&self) -> rec::SourceOwner {
        rec::SourceOwner {
            ticket: self.source as u64 + 1,
            context: CONTEXT,
        }
    }
    fn done(&self) -> bool {
        self.source == self.batches.len()
    }
    fn action(&self) -> rec::Input {
        let mut a = rec::Input::default();
        if self.stopped || self.done() {
            return a;
        }
        if !self.owner_accepted {
            a.source_captured = Some(self.owner());
            return a;
        }
        if !self.end_sent {
            a.source_end = Some(rec::SourceEnd {
                source: self.owner(),
                fans: self.batches[self.source].len() as u8,
            });
        }
        if self.fan < self.batches[self.source].len() {
            if let Some(key) = self.key {
                if self.row < rr::WORDS {
                    a.write = Some(rec::Write {
                        key,
                        row: self.row,
                        word: self.batches[self.source][self.fan].0[self.row],
                    });
                }
            } else {
                a.reserve = Some(rec::Reserve {
                    source: self.owner(),
                    fan: self.fan as u8,
                    rows: rr::WORDS,
                });
            }
        }
        a
    }
    fn observe(
        &mut self,
        ce: bool,
        action: rec::Input,
        events: &[rec::Event],
        words: &mut BTreeMap<u64, [u64; rr::WORDS]>,
    ) {
        if ce && action.source_end.is_some() {
            self.end_sent = true;
        }
        if ce
            && action.reserve.is_some()
            && !events.iter().any(|e| matches!(e, rec::Event::Reserved(_)))
        {
            self.blocked += 1;
        }
        for e in events {
            match e {
                rec::Event::SnapshotAccepted(o) => {
                    assert_eq!(*o, self.owner());
                    self.owner_accepted = true;
                }
                rec::Event::Reserved(k) => {
                    assert_eq!(k.source, self.owner());
                    assert_eq!(k.fan, self.fan as u8);
                    assert!(words
                        .insert(k.generation, self.batches[self.source][self.fan].0)
                        .is_none());
                    self.key = Some(*k);
                }
                rec::Event::RowWritten { key, row } => {
                    assert_eq!(Some(*key), self.key);
                    assert_eq!(*row, self.row);
                    self.row += 1;
                }
                rec::Event::Published(k) => {
                    assert_eq!(Some(*k), self.key);
                    assert_eq!(self.row, rr::WORDS);
                    self.fan += 1;
                    self.row = 0;
                    self.key = None;
                }
                rec::Event::SnapshotLastUseAck(o) => {
                    assert_eq!(*o, self.owner());
                    assert_eq!(self.fan, self.batches[self.source].len());
                    self.source += 1;
                    self.fan = 0;
                    self.owner_accepted = false;
                    self.end_sent = false;
                }
                _ => {}
            }
        }
    }
}
fn attributes() -> rr::Attributes {
    let c = rr::profile();
    rr::Attributes {
        near: c.near_raw,
        far: c.far_raw,
        slot: 0,
        size: 5,
        filter: tex::Filter::Trilinear,
        default_light: false,
        default_sample: false,
    }
}
fn lighting() -> light::LightingContext {
    light::LightingContext {
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
    }
}
fn context() -> Context {
    let mut c = pixel::context();
    c.surface.width = 32;
    c.rop.depth = fb::DepthFunc::Always;
    c
}
fn slot() -> tex::Slot {
    let mut s = texture::slot(5, true);
    s.base_address = TEX_BASE;
    s
}
struct Fixture {
    initial: Vec<u8>,
    asset: Vec<u8>,
    slot: tex::Slot,
}
impl Fixture {
    fn new() -> Self {
        let slot = slot();
        let asset = texture::asset(slot, texture::pattern);
        let mut initial: Vec<_> = (0..0x21000)
            .map(|i| ((i * 13) ^ (i >> 6) ^ 0xa7) as u8)
            .collect();
        initial[..24576].copy_from_slice(&pixel::image());
        initial[TEX_BASE as usize..TEX_BASE as usize + asset.len()].copy_from_slice(&asset);
        assert!(asset.len().is_multiple_of(128));
        Self {
            initial,
            asset,
            slot,
        }
    }
    fn shared(&self) -> shared::Shared {
        // SAFETY: independent external initial memory stimulus, not computed results.
        let image = unsafe {
            OracleImage::from_host(0, self.initial.clone(), "record raster external image").unwrap()
        };
        shared::Shared::new(
            image,
            (
                u64::from(TEX_BASE),
                u64::from(TEX_BASE) + self.asset.len() as u64,
            ),
            BG_BASE,
            true,
            rr::MAX_WALL,
        )
        .unwrap()
    }
}
struct GoldenMemory<'a>(&'a [u8]);
impl gpu_v2::frontend::ports::MemoryPort for GoldenMemory<'_> {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        if bytes != 128 || !address.is_multiple_of(128) {
            return Err("golden sector".into());
        }
        let offset = address
            .checked_sub(u64::from(TEX_BASE))
            .ok_or("golden texture address")? as usize;
        let data = self
            .0
            .get(offset..offset + bytes)
            .ok_or("golden asset bounds")?;
        Ok(data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect())
    }
}
// Independent original-vertex orientation; no decoded edge recurrence or contains().
fn covered(v: [[i64; 2]; 3], x: u16, y: u16) -> bool {
    let s = [i128::from(x) * 16 + 8, i128::from(y) * 16 + 8];
    (0..3).all(|i| {
        let p = v[i];
        let q = v[(i + 1) % 3];
        let cross = i128::from(q[0] - p[0]) * (s[1] - i128::from(p[1]))
            - i128::from(q[1] - p[1]) * (s[0] - i128::from(p[0]));
        cross > 0 || cross == 0 && (q[1] < p[1] || q[1] == p[1] && q[0] > p[0])
    })
}
fn reference_quad(r: &oracle::Report, fan: usize, x: u16, y: u16) -> Option<BranchQuad> {
    let mut mask = 0;
    for lane in 0..4 {
        if covered(
            r.triangles[fan].vertices,
            x + (lane % 2) as u16,
            y + (lane / 2) as u16,
        ) {
            mask |= 1 << lane;
        }
    }
    if mask == 0 {
        return None;
    }
    let samples: Vec<_> = (0..4)
        .map(|lane| {
            r.reference([
                f64::from(x) + 0.5 + (lane % 2) as f64,
                f64::from(y) + 0.5 + (lane / 2) as f64,
            ])
            .unwrap()
        })
        .collect();
    let mut q = BranchQuad {
        live: LiveQuad {
            quad: QuadInput {
                header: fb::Header {
                    x,
                    y: y as u8,
                    mask,
                },
                basic: [Basic::default(); 4],
                default_light: false,
                default_sample: false,
            },
            light: [light::PixelInput {
                normal: [0; 3],
                ndc: [0; 2],
            }; 4],
        },
        sample: Some(tex::QuadInput {
            quad_id: 0,
            mask,
            uv: [[0.; 2]; 4],
            slot: 0,
            material_size_log2: 5,
            filter: tex::Filter::Trilinear,
            lod_bias: 0.,
        }),
    };
    for (lane, s) in samples.iter().enumerate() {
        q.sample.as_mut().unwrap().uv[lane] = s.quantized.uv.map(|v| v as f64 / 262144.);
        if mask >> lane & 1 != 0 {
            q.live.quad.basic[lane] = Basic {
                tint: s
                    .rgb
                    .map(|v| (v.clamp(0., 1.) * 255.).round_ties_even() as u8),
                depth: s.quantized.depth,
            };
            let px = i32::from(x) + (lane % 2) as i32;
            let py = i32::from(y) + (lane / 2) as i32;
            q.live.light[lane] = light::PixelInput {
                normal: s.quantized.normal.map(|v| i16::try_from(v).unwrap()),
                ndc: [px * 4096 + 2048 - 65536, 65536 - py * 4096 - 2048],
            };
        }
    }
    Some(q)
}
// Checker-only vectors. They never drive the consumer or PixelBranches offers.
type GoldenFrame = (Vec<(u64, BranchQuad)>, Vec<pixel::Stimulus>, Vec<u8>, bool);
fn golden(reports: &[oracle::Report], f: &Fixture) -> GoldenFrame {
    let mut quads = Vec::new();
    let mut stimuli = Vec::new();
    let mut generation = 0;
    let mut cache = texture_oracle::Cache::new(vec![f.slot]).unwrap();
    let mut memory = GoldenMemory(&f.asset);
    let light = lighting();
    let mut helper_effect = false;
    for r in reports {
        for fan in 0..r.triangles.len() {
            generation += 1;
            for ty in 0..2 {
                for tx in 0..2 {
                    for y in (ty * 16..ty * 16 + 16).step_by(2) {
                        for x in (tx * 16..tx * 16 + 16).step_by(2) {
                            let Some(q) = reference_quad(r, fan, x, y) else {
                                continue;
                            };
                            let sampled = texture_oracle::sample(
                                q.sample.as_ref().unwrap(),
                                &mut cache,
                                &mut memory,
                                tex::Config::counted(),
                            )
                            .unwrap();
                            let mut colors = [[255; 3]; 4];
                            for p in &sampled.pixels {
                                colors[p.lane as usize] = p.rgb;
                            }
                            if q.live.quad.header.mask != 15 {
                                let mut bad = q.sample.clone().unwrap();
                                let first = (0..4).find(|&i| bad.mask >> i & 1 != 0).unwrap();
                                for i in 0..4 {
                                    if bad.mask >> i & 1 == 0 {
                                        bad.uv[i] = bad.uv[first];
                                    }
                                }
                                let mut bad_cache =
                                    texture_oracle::Cache::new(vec![f.slot]).unwrap();
                                let changed = texture_oracle::sample(
                                    &bad,
                                    &mut bad_cache,
                                    &mut memory,
                                    tex::Config::counted(),
                                )
                                .unwrap();
                                helper_effect |= sampled
                                    .pixels
                                    .iter()
                                    .zip(changed.pixels)
                                    .any(|(a, b)| a.rgb != b.rgb);
                            }
                            let lights = std::array::from_fn(|lane| {
                                if q.live.quad.header.mask >> lane & 1 == 0 {
                                    return light::LightingOutput { g: 0, h: 0 };
                                }
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
                            });
                            stimuli.push(pixel::Stimulus {
                                quad: q.live.quad,
                                light: lights,
                                sample: colors,
                            });
                            quads.push((generation, q));
                        }
                    }
                }
            }
        }
    }
    let expected = pixel::golden(f.initial.clone(), &stimuli, context());
    (quads, stimuli, expected, helper_effect)
}
#[test]
fn codec_identity_reference_and_rejections() {
    let r = reports(&corpus());
    let mut count = 0;
    for report in &r {
        for fan in 0..report.triangles.len() {
            let encoded = rr::Encoded::from_report(report, fan).unwrap();
            let d = rr::Decoded::from_words(&encoded.0).unwrap();
            for (a, b) in d.coefficients.iter().flatten().zip(
                report
                    .source
                    .as_ref()
                    .unwrap()
                    .cache
                    .coefficients
                    .iter()
                    .flatten(),
            ) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
            for y in 0..32 {
                for x in 0..32 {
                    if covered(report.triangles[fan].vertices, x, y) {
                        let actual = d.evaluate(f64::from(x) + 0.5, f64::from(y) + 0.5).unwrap();
                        let expected = report
                            .evaluate([f64::from(x) + 0.5, f64::from(y) + 0.5])
                            .unwrap();
                        assert_eq!(
                            actual.uv().unwrap(),
                            expected.quantized.uv.map(|v| v as f64 / 262144.)
                        );
                        assert_eq!(
                            actual.normal().unwrap(),
                            expected.quantized.normal.map(|v| i16::try_from(v).unwrap())
                        );
                        assert_eq!(
                            actual
                                .depth(report.config.near_raw, report.config.far_raw)
                                .unwrap(),
                            expected.quantized.depth
                        );
                        let reference = report
                            .reference([f64::from(x) + 0.5, f64::from(y) + 0.5])
                            .unwrap();
                        for i in 0..2 {
                            assert!(
                                (expected.quantized.uv[i] - reference.quantized.uv[i]).abs() <= 1
                            );
                        }
                        count += 1;
                    }
                }
            }
            for (row, bit) in [
                (0, 34),
                (2, 12),
                (3, 35),
                (4, 30),
                (41, 20),
                (44, 16),
                (47, 16),
                (50, 16),
                (11, 36),
            ] {
                let mut bad = encoded.0;
                bad[row] |= 1 << bit;
                assert!(rr::Decoded::from_words(&bad).is_err(), "row {row}");
            }
            let mut invalid = rr::Decoded::from_words(&encoded.0).unwrap();
            invalid.determinant = -invalid.determinant;
            assert!(invalid.evaluate(8.5, 8.5).unwrap_err().contains("helper W"));
        }
    }
    assert!(count > 100);
    let too_large = rr::Values {
        a: [0., 0., 0., 0., 0., 3., 0., 0.],
        w: 1.,
    };
    assert!(too_large.normal().unwrap_err().contains("narrowing"));
    let uv = rr::Values {
        a: [2_000_000., 0., 0., 0., 0., 0., 0., 0.],
        w: 1.,
    };
    assert!(uv.uv().is_err());
    let bad = oracle::run(
        &corpus()[0],
        tri::Config {
            width: 32,
            height: 32,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(rr::Encoded::from_report(&bad, 0).is_err());
    std::fs::create_dir_all(artifacts()).unwrap();
    std::fs::write(
        artifacts().join("codec.txt"),
        format!(
            "covered comparisons={count}\ncoefficient bit identity and canonical rejection PASS\n"
        ),
    )
    .unwrap();
}
fn compare_offer(actual: &BranchQuad, expected: &BranchQuad) {
    assert_eq!(
        actual.live.quad, expected.live.quad,
        "independent Basic/header final-code boundary"
    );
    for lane in 0..4 {
        assert_eq!(
            actual.live.light[lane].normal,
            expected.live.light[lane].normal
        );
        assert_eq!(actual.live.light[lane].ndc, expected.live.light[lane].ndc);
    }
    assert_eq!(
        actual.sample.as_ref().unwrap().uv,
        expected.sample.as_ref().unwrap().uv,
        "independent helper Q18 codes"
    );
}
#[test]
fn real_record_shared_mc_full_image_and_holds() {
    run_frame(false, "frame");
}
#[test]
fn real_record_shared_mc_ce_finish_last_hold() {
    run_frame(true, "frame-paused");
}

#[test]
fn reversed_shared_edge_cursor_has_no_duplicate_or_missing_pixels() {
    let inputs = [
        input(
            1,
            [[8., 8.], [16., 8.], [8., 16.]],
            [0xffff; 3],
            [0, 0, 16384],
        ),
        input(
            2,
            [[16., 8.], [16., 16.], [8., 16.]],
            [0xffff; 3],
            [0, 0, 16384],
        ),
    ];
    let reports = reports(&inputs);
    let mut producer = Producer::new(&reports);
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let mut words = BTreeMap::new();
    let mut pixels = BTreeSet::new();
    let mut quads = 0;
    for wall in 0..4096 {
        let ce = wall % 9 != 0;
        let offer = reader.offer().cloned();
        let accepted = ce && offer.is_some();
        if accepted {
            // This focused cursor test has an explicit always-ready captured
            // test sink; it does not claim a PixelBranches/MC transfer.
            let q = offer.unwrap();
            let generation = if reader.stats.released == 0 { 1 } else { 2 };
            let expected = reference_quad(
                &reports[generation - 1],
                0,
                q.live.quad.header.x,
                u16::from(q.live.quad.header.y),
            )
            .unwrap();
            compare_offer(&q, &expected);
            for lane in 0..4 {
                if q.live.quad.header.mask >> lane & 1 != 0 {
                    assert!(
                        pixels.insert((
                            q.live.quad.header.x + (lane % 2) as u16,
                            u16::from(q.live.quad.header.y) + (lane / 2) as u16
                        )),
                        "shared diagonal shaded twice"
                    );
                }
            }
            quads += 1;
        }
        let a = producer.action();
        let e = reader.step(ce, a, wall % 11 != 0, accepted).unwrap();
        producer.observe(ce, a, &e, &mut words);
        for event in e {
            if let rec::Event::ConsumerCaptured(r) = event {
                assert_eq!(r.word, words[&r.key.generation][r.row]);
            }
        }
        if producer.done() && reader.drained() {
            break;
        }
        assert!(wall + 1 < 4096);
    }
    let expected: BTreeSet<_> = (8..16).flat_map(|x| (8..16).map(move |y| (x, y))).collect();
    assert_eq!(pixels, expected);
    assert_eq!(pixels.len(), 64);
    assert!(quads > 0 && reader.stats.released == 2);
    reader.save(&artifacts(), "shared-edge");
}

#[test]
fn actual_reused_codec_record_rejects_stale_generation_and_owner() {
    let reports = reports(&corpus()[..2]);
    let encoded: Vec<_> = reports
        .iter()
        .map(|r| rr::Encoded::from_report(r, 0).unwrap())
        .collect();
    assert_ne!(
        encoded[0].0[42], encoded[1].0[42],
        "dirty old attribute word differs"
    );
    let mut transport = rec::Controller::new(
        CONTEXT,
        rec::Limits {
            wall_edges: 512,
            sources: 2,
            records: 2,
        },
    )
    .unwrap();
    let mut old: Option<rec::Key> = None;
    for (i, words) in encoded.iter().enumerate() {
        let owner = rec::SourceOwner {
            ticket: i as u64 + 1,
            context: CONTEXT,
        };
        assert!(transport
            .step(rec::Input {
                ce: true,
                source_captured: Some(owner),
                ..Default::default()
            })
            .unwrap()
            .contains(&rec::Event::SnapshotAccepted(owner)));
        let e = transport
            .step(rec::Input {
                ce: true,
                reserve: Some(rec::Reserve {
                    source: owner,
                    fan: 0,
                    rows: rr::WORDS,
                }),
                source_end: Some(rec::SourceEnd {
                    source: owner,
                    fans: 1,
                }),
                ..Default::default()
            })
            .unwrap();
        let key = e
            .iter()
            .find_map(|e| {
                if let rec::Event::Reserved(k) = e {
                    Some(*k)
                } else {
                    None
                }
            })
            .unwrap();
        for row in 0..rr::WORDS {
            assert!(transport
                .step(rec::Input {
                    ce: true,
                    write: Some(rec::Write {
                        key,
                        row,
                        word: words.0[row]
                    }),
                    ..Default::default()
                })
                .unwrap()
                .contains(&rec::Event::RowWritten { key, row }));
        }
        assert!(transport
            .step(rec::Input {
                ce: true,
                ..Default::default()
            })
            .unwrap()
            .contains(&rec::Event::Published(key)));
        if let Some(stale) = old {
            assert_eq!(key.slot, stale.slot);
            assert_ne!(key.generation, stale.generation);
            let read = |key| rec::Input {
                ce: true,
                read: Some(rec::Read {
                    key,
                    row: 42,
                    consumer: rec::Consumer::Attribute,
                    last_attribute_capture: false,
                }),
                ..Default::default()
            };
            assert_eq!(transport.step(read(stale)).unwrap_err(), rec::Fault::Owner);
            let mut wrong = key;
            wrong.source.ticket = 1;
            assert_eq!(transport.step(read(wrong)).unwrap_err(), rec::Fault::Owner);
            wrong = key;
            wrong.source.context += 1;
            assert_eq!(
                transport.step(read(wrong)).unwrap_err(),
                rec::Fault::Context
            );
        }
        // Scalar codec-word consumer owns no quad/coverage references. Its only
        // read is really captured before the test's final acknowledgment.
        transport
            .step(rec::Input {
                ce: true,
                read: Some(rec::Read {
                    key,
                    row: 42,
                    consumer: rec::Consumer::Attribute,
                    last_attribute_capture: true,
                }),
                ..Default::default()
            })
            .unwrap();
        transport
            .step(rec::Input {
                ce: true,
                ..Default::default()
            })
            .unwrap();
        let e = transport
            .step(rec::Input {
                ce: true,
                return_ready: true,
                ..Default::default()
            })
            .unwrap();
        assert!(e.iter().any(
            |e| matches!(e,rec::Event::ConsumerCaptured(r) if r.key==key &&r.word==words.0[42])
        ));
        assert!(transport
            .step(rec::Input {
                ce: true,
                last_quad_ack: Some(key),
                ..Default::default()
            })
            .unwrap()
            .contains(&rec::Event::RecordReleased(key)));
        old = Some(key);
    }
    assert!(transport.drained());
}

fn drain_fault(
    reader: &mut rr::Reader,
    dut: &mut PixelBranches,
    s: &shared::Shared,
    ro: &mut shared::RoView,
    fb: &mut shared::FbView,
) {
    reader.cancel();
    dut.abort();
    s.abort_sampling();
    for wall in 0..4096 {
        ro.discard_edge().unwrap();
        let clock = s.clocks();
        let t = dut.step(BranchTick::default(), ro, fb).unwrap();
        assert_eq!(s.clocks(), (clock.0 + 1, clock.1 + 2));
        assert!(t.sampling.is_none() && t.sample_returned.is_none() && !dut.complete());
        reader
            .step(true, rec::Input::default(), true, false)
            .unwrap();
        if reader.drained() && dut.framebuffer_drained() && s.idle() {
            return;
        }
        assert!(
            wall + 1 < 4096,
            "finite independent record/FB/RO fault drain"
        );
    }
    panic!("unreachable drain bound");
}
#[test]
fn abort_pending_record_read_discards_instead_of_success() {
    abort_case("pending", "abort-pending");
}
#[test]
fn abort_held_quad_cancels_record_references() {
    abort_case("hold", "abort-held");
}
#[test]
fn abort_real_fb_write_preserves_continuous_source_and_partial_image() {
    abort_case("write", "abort-write");
}
fn abort_case(target: &str, name: &str) {
    let reports = reports(&corpus());
    let f = Fixture::new();
    let (expected_quads, _, expected, _) = golden(&reports, &f);
    let mut producer = Producer::new(&reports);
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let s = f.shared();
    let (mut ro, mut fb) = s.views();
    let mut dut = PixelBranches::new(context(), lighting(), vec![f.slot], rr::MAX_WALL).unwrap();
    let mut words = BTreeMap::new();
    let mut accepted = 0;
    let mut triggered = false;
    let mut accepted_writes = Vec::new();
    for wall in 0..rr::MAX_WALL {
        let v = reader.view();
        let stop = match target {
            "pending" => reader.stats.released > 0 && v.pending,
            "hold" => accepted > 0 && v.phase == "hold",
            "write" => {
                s.fb_write_active() && s.stats().fb_write_beats > 0 && s.image() != f.initial
            }
            _ => unreachable!(),
        };
        if stop {
            triggered = true;
            break;
        }
        let offer = reader.offer().cloned();
        let finish = producer.done() && reader.drained() && offer.is_none();
        let clock = s.clocks();
        let t = dut
            .step(
                BranchTick {
                    quad: offer.clone(),
                    finish,
                    ..Default::default()
                },
                &mut ro,
                &mut fb,
            )
            .unwrap();
        assert_eq!(s.clocks(), (clock.0 + 1, clock.1 + 2));
        if t.live.model.framebuffer.response.accepted {
            let request = t.live.model.framebuffer.request.unwrap();
            if request.write {
                accepted_writes.push(request.address_bytes as usize);
            }
        }
        if t.live.model.quad_accepted {
            compare_offer(offer.as_ref().unwrap(), &expected_quads[accepted].1);
            accepted += 1;
        }
        let action = producer.action();
        let events = reader
            .step(true, action, true, t.live.model.quad_accepted)
            .unwrap();
        producer.observe(true, action, &events, &mut words);
        assert!(wall + 1 < rr::MAX_WALL, "abort trigger watchdog");
    }
    assert!(triggered && accepted > 0);
    let cancelled_owners: BTreeSet<_> = reader
        .trace
        .iter()
        .filter_map(|(_, e)| {
            if let rec::Event::Published(k) = e {
                Some(k.generation)
            } else {
                None
            }
        })
        .filter(|g| {
            !reader
                .trace
                .iter()
                .any(|(_, e)| matches!(e,rec::Event::RecordReleased(k) if k.generation==*g))
        })
        .collect();
    let trace_before = reader.trace.len();
    producer.stopped = true;
    drain_fault(&mut reader, &mut dut, &s, &mut ro, &mut fb);
    assert!(
        reader.drained()
            && dut.framebuffer_drained()
            && s.idle()
            && dut.faulted()
            && !dut.complete()
    );
    for (_, event) in &reader.trace[trace_before..] {
        assert!(
            !matches!(event,rec::Event::RecordReleased(k) if cancelled_owners.contains(&k.generation))
        );
        assert!(!matches!(event, rec::Event::SnapshotLastUseAck(_)));
    }
    if target != "write" {
        assert!(!cancelled_owners.is_empty() && reader.stats.aborted > 0);
        assert_eq!(s.stats().fb_writes, 0);
        assert_eq!(
            s.image(),
            f.initial,
            "dirty cache discard is not external rollback"
        );
    } else {
        let mut partial = f.initial.clone();
        // Rows0..3 are unchanged; changed DQ bytes first occur in rows4..7.
        assert_eq!(accepted_writes, [512, 640]);
        for address in accepted_writes {
            partial[address..address + 128].copy_from_slice(&expected[address..address + 128]);
        }
        assert_eq!(
            s.image(),
            partial,
            "independent accepted-write footprint, no rollback"
        );
        assert_eq!(s.stats().fb_write_beats, 32);
        assert_eq!(
            s.stats().fb_terminals,
            s.stats().fb_reads + s.stats().fb_writes
        );
    }
    reader.save(&artifacts(), name);
    s.save(&artifacts(), name);
    std::fs::write(artifacts().join(format!("{name}-summary.txt")),format!("accepted={accepted}\ncancelled_owners={cancelled_owners:?}\nrecord/FB/RO/background drained; no render success\n")).unwrap();
}

#[test]
fn poisoned_mc_does_not_become_global_drain_after_record_cancel() {
    use gpu_v2::texture::ports::RefillPort;
    let f = Fixture::new();
    let s = f.shared();
    s.stop_background();
    let (mut ro, mut fb) = s.views();
    let reports = reports(&[corpus().remove(0)]);
    let mut producer = Producer::new(&reports);
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let mut words = BTreeMap::new();
    ro.step().unwrap();
    assert!(
        fb.cycle(
            Some(gpu_v2::memory::ports::Request {
                address_bytes: 512,
                write: true
            }),
            Some(0x9876)
        )
        .unwrap()
        .accepted
    );
    let a = producer.action();
    let e = reader.step(true, a, true, false).unwrap();
    producer.observe(true, a, &e, &mut words);
    ro.step().unwrap();
    ro.submit_read(u64::from(TEX_BASE), 128).unwrap();
    assert!(fb.cycle(None, Some(0x9876)).unwrap().write_accepted);
    let a = producer.action();
    let e = reader.step(true, a, true, false).unwrap();
    producer.observe(true, a, &e, &mut words);
    ro.step().unwrap();
    let clock = s.clocks();
    let error = fb.cycle(None, None).unwrap_err();
    assert!(error.contains("source underrun"));
    assert!(s.tick_poisoned() && !s.physical_error());
    assert_eq!(s.clocks(), clock);
    reader.cancel();
    for _ in 0..8 {
        reader
            .step(true, rec::Input::default(), true, false)
            .unwrap();
        if reader.drained() {
            break;
        }
    }
    assert!(reader.drained() && !s.gpu_idle() && !s.idle());
    assert!(fb
        .cycle(None, Some(0x4321))
        .unwrap_err()
        .contains("no retry"));
    assert_eq!(s.clocks(), clock);
    reader.save(&artifacts(), "poison");
    s.save(&artifacts(), "poison");
}

fn run_frame(pauses: bool, name: &str) {
    let reports = reports(&corpus());
    let f = Fixture::new();
    let (expected_quads, goldens, expected, helper_effect) = golden(&reports, &f);
    assert!(
        helper_effect,
        "uncovered helpers materially change covered texture RGB"
    );
    let mut producer = Producer::new(&reports);
    let mut reader = rr::Reader::new(CONTEXT, attributes()).unwrap();
    let s = f.shared();
    let (mut ro, mut fb) = s.views();
    let mut dut = PixelBranches::new(context(), lighting(), vec![f.slot], rr::MAX_WALL).unwrap();
    let mut words = BTreeMap::new();
    let mut accepted = 0;
    let mut results = 0;
    let mut lights = 0;
    let mut owners = BTreeMap::new();
    let mut masks = Vec::new();
    let mut pause_hits = BTreeSet::new();
    let mut pause_left = 0;
    let mut finish_intent = false;
    let mut finish_sent = false;
    let mut release_before_complete = false;
    let mut hold_previous: Option<BranchQuad> = None;
    let mut seen_keys = Vec::new();
    for wall in 0..rr::MAX_WALL {
        let v = reader.view();
        assert!(v.published <= 2 && v.free <= 2);
        if pauses && pause_left == 0 {
            let hit = if v.pending && !pause_hits.contains("pending") {
                Some("pending")
            } else if v.captured && !pause_hits.contains("captured") {
                Some("captured")
            } else if v.helper_lane == Some(2) && !pause_hits.contains("helper") {
                Some("helper")
            } else if v.phase == "confirm" && v.captured && !pause_hits.contains("confirm") {
                Some("confirm")
            } else {
                None
            };
            if let Some(hit) = hit {
                if pause_hits.insert(hit) {
                    pause_left = 5;
                }
            }
        }
        let offer = reader.offer().cloned();
        let last = offer.is_some() && accepted + 1 == expected_quads.len();
        if pauses && last && pause_hits.insert("last-hold") {
            pause_left = 9;
            finish_intent = true;
        }
        let ce = pause_left == 0;
        if pause_left > 0 {
            pause_left -= 1;
        }
        if let Some(old) = &hold_previous {
            compare_offer(offer.as_ref().expect("held offer disappeared"), old);
        }
        let finish = ce && producer.done() && reader.drained() && offer.is_none();
        finish_sent |= finish;
        let clock = s.clocks();
        let t = dut
            .step(
                BranchTick {
                    ce,
                    quad: offer.clone(),
                    finish,
                    ..Default::default()
                },
                &mut ro,
                &mut fb,
            )
            .unwrap();
        assert_eq!(s.clocks(), (clock.0 + 1, clock.1 + 2));
        if t.live.model.quad_accepted {
            compare_offer(offer.as_ref().unwrap(), &expected_quads[accepted].1);
            masks.push(offer.as_ref().unwrap().live.quad.header.mask);
            let ticket = t.live.model.ticket.unwrap();
            assert!(owners.insert(ticket.serial, accepted).is_none());
            accepted += 1;
        }
        hold_previous = offer.filter(|_| !t.live.model.quad_accepted);
        if let Some(w) = t.sample_returned {
            let index = owners[&w.key.ticket.serial];
            assert_eq!(w.rgb, goldens[index].sample[w.key.lane as usize]);
            results += 1;
        }
        if let Some((key, value, _)) = t.live.light_returned {
            let index = owners[&key.ticket.serial];
            assert_eq!(value, goldens[index].light[key.lane as usize]);
            lights += 1;
        }
        let action = producer.action();
        // Keep the captured first return blocked while the producer fills two
        // actual Published slots; this forces a real third-record reserve stall.
        let ready = wall >= 250;
        let events = reader
            .step(ce, action, ready, t.live.model.quad_accepted)
            .unwrap();
        producer.observe(ce, action, &events, &mut words);
        for e in &events {
            match e {
                rec::Event::Reserved(k) => seen_keys.push(*k),
                rec::Event::ConsumerCaptured(r) => assert_eq!(
                    r.word, words[&r.key.generation][r.row],
                    "actual returned word/owner"
                ),
                rec::Event::RecordReleased(_) => {
                    release_before_complete |= !dut.complete();
                }
                _ => {}
            }
        }
        if dut.complete() && reader.drained() {
            break;
        }
        assert!(wall + 1 < rr::MAX_WALL, "finite frame watchdog");
    }
    assert!(finish_sent && dut.complete() && reader.drained() && s.gpu_idle());
    assert_eq!(accepted, expected_quads.len());
    assert_eq!(reader.stats.helpers, 4 * accepted as u64);
    let covered: usize = masks.iter().map(|m| m.count_ones() as usize).sum();
    assert_eq!(results, covered);
    assert_eq!(lights, covered);
    assert_eq!(reader.stats.covered_captures, covered as u64);
    assert_eq!(reader.stats.confirmations, reader.stats.released);
    assert_eq!(reader.stats.released, words.len() as u64);
    assert!(producer.blocked > 0 && reader.stats.peak_published == 2);
    assert!(seen_keys.iter().enumerate().any(|(i, a)| seen_keys[..i]
        .iter()
        .any(|b| b.slot == a.slot && b.generation != a.generation)));
    assert!(release_before_complete && masks.iter().any(|&m| m != 15));
    if pauses {
        assert!(finish_intent && pause_hits.len() == 5);
    }
    s.stop_background();
    for _ in 0..4096 {
        if s.idle() {
            break;
        }
        ro.discard_edge().unwrap();
        fb.cycle(None, None).unwrap();
    }
    assert!(s.idle());
    assert_eq!(
        s.image(),
        expected,
        "independent complete color/depth/texture/guard image"
    );
    reader.save(&artifacts(), name);
    s.save(&artifacts(), name);
    std::fs::write(artifacts().join(format!("{name}-summary.txt")),format!("quads={accepted}\ncovered={covered}\nblocked_reserve={}\npause_hits={pause_hits:?}\nhelper_effect={helper_effect}\n",producer.blocked)).unwrap();
}
