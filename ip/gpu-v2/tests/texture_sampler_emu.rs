//! Whole registered sampler; closed numerical golden is constructed only in tests.
use gpu_v2::{
    framebuffer::sim::fixture::Fixture,
    memory::ports::{MemoryPort, Request, Response},
    texture::{
        emu::{
            derivative,
            sampler::{SamplerEmu, Tick},
        },
        ports::*,
        sim::oracle,
    },
};
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod support;

struct ReadMemory {
    inner: Fixture,
    held: Option<Request>,
    requests: usize,
}
impl MemoryPort for ReadMemory {
    fn cycle(&mut self, q: Option<Request>, w: Option<u64>) -> Result<Response, String> {
        assert!(w.is_none());
        if let Some(old) = self.held {
            assert_eq!(q, Some(old));
        }
        if let Some(q) = q {
            assert!(!q.write);
        }
        let r = self.inner.cycle(q, w)?;
        self.requests += usize::from(r.accepted);
        self.held = if r.accepted { None } else { q };
        Ok(r)
    }
}

#[test]
fn whole_sampler_filter_lod_wrap_masks_ce_credit_and_cache_reuse() {
    const LIMIT: u64 = 300_000;
    let slot = support::slot(6, true);
    let asset = support::asset(slot, support::pattern);
    let mut bytes = vec![0x69; support::BASE as usize];
    bytes.extend(&asset);
    let mut mem = ReadMemory {
        inner: Fixture::new(bytes.clone()),
        held: None,
        requests: 0,
    };
    mem.inner.request_period = 5;
    mem.inner.beat_period = 3;
    mem.inner.ack_delay = 7;
    let mut dut = SamplerEmu::new(vec![slot], mem, LIMIT).unwrap();
    let mut golden_cache = oracle::Cache::new(vec![slot]).unwrap();
    let mut image = support::Image {
        bytes: asset,
        requests: vec![],
    };
    let mut cases = Vec::new();
    for i in 0..48 {
        let mut q = support::input(
            6,
            [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
            [[-0.012, 0.995], [0.4375, 0.1875], [2.03125, -3.0625]][i % 3],
        );
        q.quad_id = (i % 16) as u8;
        q.mask = [15, 5, 10, 1, 0][i % 5];
        q.lod_bias = [-2.0, 0.5, 2.25][i % 3];
        q.uv[1][0] += 1.0 / 64.0;
        q.uv[2][1] += 1.0 / 32.0;
        let expected =
            oracle::sample(&q, &mut golden_cache, &mut image, Config::counted()).unwrap();
        let mut rgb = [[255; 3]; 4];
        for p in expected.pixels {
            rgb[p.lane as usize] = p.rgb;
        }
        cases.push((derivative::Input::capture(&q, slot).unwrap(), rgb));
    }
    let mut sent = 0;
    let mut received = 0;
    let mut held = None;
    for wall in 0..LIMIT {
        let step = dut
            .tick(Tick {
                ce: wall % 13 != 4 && wall % 17 != 8,
                input: cases.get(sent).map(|x| x.0),
                output_ready: wall % 19 > 5,
            })
            .unwrap();
        if let Some(o) = held {
            assert_eq!(step.output, Some(o));
        }
        if step.accepted {
            sent += 1;
        }
        if step.transferred {
            let o = step.output.unwrap();
            let c = &cases[received];
            assert_eq!(o.quad, c.0.header.quad);
            assert_eq!(o.mask, c.0.header.mask);
            assert_eq!(o.colors, c.1, "quad {received}");
            received += 1;
            held = None;
        } else {
            held = step.output;
        }
        if received == cases.len() && dut.idle() {
            break;
        }
        assert!(wall + 1 < LIMIT, "whole sampler bounded wall watchdog");
    }
    assert_eq!(received, cases.len());
    assert!(!dut.faulted());
    assert!(dut.cache().memory().requests > 0);
    assert!(dut.cache().memory().inner.idle());
    assert_eq!(
        dut.cache().memory().inner.bytes,
        bytes,
        "RO cache touched guards or texture"
    );
}

#[test]
fn whole_sampler_input_fault_is_terminal_and_drainable() {
    const LIMIT: u64 = 500;
    let slot = support::slot(3, true);
    let mem = ReadMemory {
        inner: Fixture::new(vec![0; 8192]),
        held: None,
        requests: 0,
    };
    let mut dut = SamplerEmu::new(vec![slot], mem, LIMIT).unwrap();
    let q = support::input(3, Filter::Bilinear, [0.2, 0.3]);
    let mut i = derivative::Input::capture(&q, slot).unwrap();
    i.uv[0] = 1_i64 << 39;
    assert!(dut
        .tick(Tick {
            ce: true,
            input: Some(i),
            output_ready: true
        })
        .is_err());
    assert!(dut.faulted());
    assert!(dut
        .tick(Tick {
            ce: true,
            input: None,
            output_ready: true
        })
        .is_err());
    for n in 0..LIMIT {
        if dut.drain_tick().unwrap() {
            return;
        }
        assert!(n + 1 < LIMIT);
    }
    panic!("bounded explicit drain");
}

#[test]
fn whole_sampler_optimization_matrix_has_exact_colors_and_measured_hot_intervals() {
    use gpu_v2::texture::sim::staged::bound::serial;
    const LIMIT: u64 = 100_000;
    const QUADS: usize = 20;
    let slot = support::slot(6, true);
    let asset = support::asset(slot, support::pattern);
    let mut bytes = vec![0x69; support::BASE as usize];
    bytes.extend(&asset);
    let mut report = String::from(
        "filter,bypass,short,cold_return,hot_quad_edges,requests,packet_groups,color_groups\n",
    );
    for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
        let mut q = support::input(6, filter, [0.999, 0.999]);
        q.uv[1][0] += 1.0 / 64.0;
        q.uv[2][1] += 1.0 / 64.0;
        q.uv[3][0] += 1.0 / 64.0;
        q.uv[3][1] += 1.0 / 64.0;
        if filter == Filter::Trilinear {
            q.lod_bias = 0.5;
        }
        let mut image = support::Image {
            bytes: asset.clone(),
            requests: vec![],
        };
        let mut golden_cache = oracle::Cache::new(vec![slot]).unwrap();
        let pixels = oracle::sample(&q, &mut golden_cache, &mut image, Config::counted())
            .unwrap()
            .pixels;
        let mut expected = [[255; 3]; 4];
        for p in pixels {
            expected[p.lane as usize] = p.rgb;
        }
        let mut previous = [0u64; 4];
        for (configuration, (nearest_bypass, short_alignment)) in
            [(false, false), (true, false), (false, true), (true, true)]
                .into_iter()
                .enumerate()
        {
            let mut memory = ReadMemory {
                inner: Fixture::new(bytes.clone()),
                held: None,
                requests: 0,
            };
            memory.inner.request_period = 1;
            memory.inner.beat_period = 1;
            memory.inner.ack_delay = 3;
            let mut dut = SamplerEmu::with_config(
                vec![slot],
                memory,
                LIMIT,
                serial::Config {
                    nearest_bypass,
                    short_alignment,
                },
            )
            .unwrap();
            let mut sent = 0;
            let mut returned = Vec::new();
            let mut packets = 0;
            let mut colors = 0;
            for wall in 0..LIMIT {
                q.quad_id = (sent % 16) as u8;
                let input = (sent < QUADS).then(|| derivative::Input::capture(&q, slot).unwrap());
                let s = dut
                    .tick(Tick {
                        ce: true,
                        input,
                        output_ready: true,
                    })
                    .unwrap();
                sent += usize::from(s.accepted);
                packets += usize::from(s.packet_accepted);
                colors += usize::from(s.color_accepted);
                if s.transferred {
                    let v = s.output.unwrap();
                    assert_eq!(v.colors, expected);
                    assert_eq!(v.quad, (returned.len() % 16) as u8);
                    returned.push(wall);
                }
                if returned.len() == QUADS && dut.idle() {
                    break;
                }
                assert!(wall + 1 < LIMIT, "bounded optimization matrix");
            }
            assert_eq!(returned.len(), QUADS);
            let hot = returned[QUADS - 1] - returned[QUADS - 2];
            assert!(
                returned[5..].windows(2).all(|p| p[1] - p[0] == hot),
                "hot window must actually be steady"
            );
            previous[configuration] = hot;
            assert_eq!(packets, colors);
            assert_eq!(dut.cache().memory().inner.bytes, bytes);
            report.push_str(&format!(
                "{filter:?},{nearest_bypass},{short_alignment},{},{hot},{},{packets},{colors}\n",
                returned[0],
                dut.cache().memory().requests
            ));
        }
        assert!(previous[2] < previous[0]);
        assert!(previous[3] <= previous[2]);
        if filter == Filter::Nearest {
            assert!(previous[1] < previous[0]);
        } else {
            assert_eq!(previous[1], previous[0]);
        }
    }
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-overnight-20261006");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("sampler-optimization-matrix.csv"), report).unwrap();
}
