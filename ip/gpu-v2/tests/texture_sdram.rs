#[path = "support/sdram/mod.rs"]
mod sdram;
#[path = "support/texture.rs"]
mod support;
use digital_design_hardware_gowin::sdram_memory_controller::{
    ports::{Event, OracleImage, Service},
    sim::{average, oracle, traffic::*},
};
use gpu_v2::texture::{ports::*, sim::oracle as sampler};
use support::*;

fn service_image(bytes: &[u8]) -> OracleImage {
    // SAFETY: independent external texture asset, not an audited intermediate.
    unsafe {
        OracleImage::from_host(u64::from(BASE), bytes.to_vec(), "RAW565 texture test asset")
            .unwrap()
    }
}
fn verify<S: Service>(service: S, bytes: &[u8], expected: &[[u8; 3]]) -> u64 {
    let mut source = sdram::Adapter {
        service,
        max_cycles: 100_000,
        events: vec![],
    };
    let s = slot(5, true);
    let mut cache = sampler::Cache::new(vec![s]).unwrap();
    let mut q = input(5, Filter::Trilinear, [0.0; 2]);
    q.uv[1][0] = 1.0 / 32.0;
    q.lod_bias = 0.5;
    let prepared = sampler::prepare(&q, &[s], Config::default()).unwrap();
    let hint = prepared.pixels[0].groups[0].key;
    let mut events = cache.prefetch(hint, &mut source).unwrap();
    let out = sampler::sample(&q, &mut cache, &mut source, Config::default()).unwrap();
    assert_eq!(
        out.pixels.iter().map(|p| p.rgb).collect::<Vec<_>>(),
        expected
    );
    assert!(out
        .cache_events
        .iter()
        .any(|e| matches!(e,sampler::CacheEvent::Hit{key,..} if *key==hint)));
    let count = source.events.len();
    sampler::sample(&q, &mut cache, &mut source, Config::default()).unwrap();
    assert_eq!(source.events.len(), count); // warmed demand does no memory traffic
    assert_eq!(
        source
            .events
            .iter()
            .filter(|e| matches!(e, Event::ReadBeat { .. }))
            .count(),
        cache.stats.refills * 16
    );
    assert_eq!(
        source
            .events
            .iter()
            .filter(|e| matches!(e, Event::Complete { .. }))
            .count(),
        cache.stats.refills
    );
    // Check every refill word against the independent asset, including prefetch.
    events.extend(out.cache_events);
    let mut base = 0;
    let mut written = 0;
    for event in &events {
        match event {
            sampler::CacheEvent::Allocate { key, address, .. } => {
                assert_eq!(*address, key.address(&[s]).unwrap());
                base = (*address - u64::from(BASE)) as usize;
                written = 0;
            }
            sampler::CacheEvent::Beat { index, data, .. } => {
                assert_eq!(*index, written);
                let at = base + index * 8;
                assert_eq!(
                    *data,
                    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
                );
                written += 1;
            }
            sampler::CacheEvent::Ready { .. } => assert_eq!(written, 16),
            _ => {}
        }
    }
    assert!(cache.stats.refills >= 8);
    source.service.cycle()
}

#[test]
fn real_sdram_payloads_match_direct_image_in_average_and_loaded_modes() {
    let s = slot(5, true);
    let bytes = asset(s, pattern);
    let mut direct = Image {
        bytes: bytes.clone(),
        requests: vec![],
    };
    let mut cache = sampler::Cache::new(vec![s]).unwrap();
    let mut q = input(5, Filter::Trilinear, [0.0; 2]);
    q.uv[1][0] = 1.0 / 32.0;
    q.lod_bias = 0.5;
    let out = sampler::sample(&q, &mut cache, &mut direct, Config::default()).unwrap();
    let expected = out.pixels.iter().map(|p| p.rgb).collect::<Vec<_>>();
    let stable = average::Memory::new(
        service_image(&bytes),
        average::Profile::gpu_default().unwrap(),
        Default::default(),
    )
    .unwrap();
    verify(stable, &bytes, &expected);
    for chain in [
        ChainPolicy::ExistingUnchained,
        ChainPolicy::ChainedCandidate,
    ] {
        for load in [
            Load::solo(),
            Load::display(1),
            Load::cpu(),
            Load::display_and_cpu(50),
        ] {
            let model = oracle::Memory::new(
                service_image(&bytes),
                oracle::Config {
                    load,
                    chain,
                    ..Default::default()
                },
            )
            .unwrap();
            verify(model, &bytes, &expected);
        }
    }
    // Inspect the service's actual image as well as color output.
    let model = oracle::Memory::new(service_image(&bytes), Default::default()).unwrap();
    let mut adapter = sdram::Adapter {
        service: model,
        max_cycles: 100_000,
        events: vec![],
    };
    let mut cache = sampler::Cache::new(vec![s]).unwrap();
    sampler::sample(&q, &mut cache, &mut adapter, Config::default()).unwrap();
    assert_eq!(adapter.service.bytes(), bytes);
}

#[test]
fn missing_tile_and_exhausted_service_budget_do_not_publish_ready() {
    let s = slot(5, true);
    let bytes = asset(s, pattern);
    let model = oracle::Memory::new(service_image(&bytes[..128]), Default::default()).unwrap();
    let mut source = sdram::Adapter {
        service: model,
        max_cycles: 100_000,
        events: vec![],
    };
    let mut cache = sampler::Cache::new(vec![s]).unwrap();
    let q = input(5, Filter::Nearest, [0.0; 2]);
    assert!(sampler::sample(&q, &mut cache, &mut source, Config::default()).is_err());
    assert_eq!(
        cache.lookup(TileKey {
            slot: 0,
            n: 5,
            x: 0,
            y: 0
        }),
        Some((0, sampler::State::Filling))
    );
    assert_eq!(cache.stats.refills, 0);
    let model = oracle::Memory::new(service_image(&bytes), Default::default()).unwrap();
    let mut source = sdram::Adapter {
        service: model,
        max_cycles: 1,
        events: vec![],
    };
    let mut cache = sampler::Cache::new(vec![s]).unwrap();
    assert!(sampler::sample(&q, &mut cache, &mut source, Config::default()).is_err());
    assert_eq!(
        cache.lookup(TileKey {
            slot: 0,
            n: 5,
            x: 0,
            y: 0
        }),
        Some((0, sampler::State::Filling))
    );
}

#[test]
fn counted_uses_real_sdram_beats_and_warmed_prefetch_without_another_interface() {
    use gpu_v2::texture::sim::counted;
    let s = slot(9, true);
    let bytes = asset(s, pattern);
    let mut q = input(9, Filter::Trilinear, [0.0; 2]);
    q.uv[1][0] = 1.0 / 512.0;
    q.lod_bias = 0.5;
    let mut direct = Image {
        bytes: bytes.clone(),
        requests: vec![],
    };
    let mut reference = sampler::Cache::new(vec![s]).unwrap();
    let expected = sampler::sample(&q, &mut reference, &mut direct, Config::counted()).unwrap();
    for loaded in [false, true] {
        let service = oracle::Memory::new(
            service_image(&bytes),
            oracle::Config {
                load: if loaded {
                    Load::display_and_cpu(50)
                } else {
                    Load::solo()
                },
                ..Default::default()
            },
        )
        .unwrap();
        let mut source = sdram::Adapter {
            service,
            max_cycles: 100_000,
            events: vec![],
        };
        let mut cache = sampler::Cache::new(vec![s]).unwrap();
        let hint = counted::prepare(&q, &[s]).unwrap().groups[0].key;
        cache.prefetch(hint, &mut source).unwrap();
        let actual = counted::sample(&q, &mut cache, &mut source).unwrap();
        assert_eq!(
            actual.pixels.iter().map(|p| p.rgb).collect::<Vec<_>>(),
            expected.pixels.iter().map(|p| p.rgb).collect::<Vec<_>>()
        );
        actual.preparation.frame.audit().unwrap();
        for p in &actual.pixels {
            p.frame.audit().unwrap();
        }
        assert!(actual
            .cache_events
            .iter()
            .any(|e| matches!(e,sampler::CacheEvent::Hit{key,..} if *key==hint)));
        let before = source.events.len();
        counted::sample(&q, &mut cache, &mut source).unwrap();
        assert_eq!(source.events.len(), before);
        assert_eq!(
            source
                .events
                .iter()
                .filter(|e| matches!(e, Event::ReadBeat { .. }))
                .count(),
            cache.stats.refills * 16
        );
        assert_eq!(source.service.bytes(), bytes);
    }
}
