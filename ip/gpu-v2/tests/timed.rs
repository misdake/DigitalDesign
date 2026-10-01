use gpu_v2::lighting::{ports::*, sim::timed::*};

fn pixels(n: usize) -> Vec<PixelInput> {
    (0..n)
        .map(|i| PixelInput {
            normal: [7123, -519, 13567],
            ndc: [i as i32 * 7919 % 131073 - 65536, 12345],
        })
        .collect()
}
fn run(n: usize, strategy: Strategy, hardware: Hardware, storage: Storage) -> Plan {
    let p = pixels(n);
    let r = plan(
        &p,
        Material::default(),
        Light::default(),
        Projection::default(),
        hardware,
        storage,
        strategy,
    )
    .unwrap();
    r.audit().unwrap();
    r.compare_oracle(
        &p,
        Material::default(),
        Light::default(),
        Projection::default(),
    )
    .unwrap();
    r
}
#[test]
fn interleaved_batches_preserve_outputs_and_fill_bubbles() {
    for n in [1, 2, 4, 8, 16] {
        let s = run(n, Strategy::Serial, Hardware::default(), Storage::Registers);
        let i = run(
            n,
            Strategy::Interleaved,
            Hardware::default(),
            Storage::Registers,
        );
        assert_eq!(s.outputs, i.outputs);
        assert!(i.cycles <= s.cycles);
        if n > 1 {
            assert!(i.cycles < s.cycles);
        }
        assert_eq!(i.work(), s.work());
        println!("batch={n} serial={} interleaved={}", s.cycles, i.cycles);
    }
}
#[test]
fn physical_rows_and_restricted_hardware_are_checked() {
    let h = Hardware {
        small_multiply: 1,
        large_multiply: 1,
        normalize_reads: 1,
        ..Hardware::default()
    };
    for lanes in [1, 2] {
        let r = run(
            8,
            Strategy::Interleaved,
            h,
            Storage::Rows {
                read_lanes: lanes,
                latency: 2,
            },
        );
        assert_eq!(r.source_reads, 28);
        assert_eq!(r.pixel_payload_bits, 672);
        println!("restricted lanes={lanes} cycles={}", r.cycles);
    }
}
#[test]
fn corrupted_reservations_are_rejected_without_panics() {
    let fresh = || {
        run(
            2,
            Strategy::Interleaved,
            Hardware::default(),
            Storage::Rows {
                read_lanes: 1,
                latency: 2,
            },
        )
    };
    let mut r = fresh();
    r.events[0].event = usize::MAX;
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.events.pop();
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.events.iter_mut().find(|r| r.lane.is_some()).unwrap().lane = Some(usize::MAX);
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.reads[0].issue = u64::MAX;
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.reads[1] = r.reads[0].clone();
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.writes[1].pixel = 0;
    assert!(r.audit().is_err());
    let mut r = fresh();
    let id = r.events.iter().position(|r| r.kind.is_some()).unwrap();
    r.events[id].ready += 1;
    assert!(r.audit().is_err());
}
#[test]
fn modes_and_limits_have_explicit_boundaries() {
    for m in [
        Material {
            unlit: true,
            ..Material::default()
        },
        Material {
            specular_color: [0; 3],
            ..Material::default()
        },
    ] {
        let p = pixels(4);
        let r = plan(
            &p,
            m,
            Light::default(),
            Projection::default(),
            Hardware::default(),
            Storage::Registers,
            Strategy::Interleaved,
        )
        .unwrap();
        r.compare_oracle(&p, m, Light::default(), Projection::default())
            .unwrap();
    }
    for (p, h, s) in [
        (pixels(65), Hardware::default(), Storage::Registers),
        (
            pixels(1),
            Hardware {
                max_cycles: 2,
                ..Hardware::default()
            },
            Storage::Registers,
        ),
        (
            pixels(1),
            Hardware {
                large_multiply: 0,
                ..Hardware::default()
            },
            Storage::Registers,
        ),
        (
            pixels(1),
            Hardware::default(),
            Storage::Rows {
                read_lanes: 0,
                latency: 1,
            },
        ),
    ] {
        assert!(plan(
            &p,
            Material::default(),
            Light::default(),
            Projection::default(),
            h,
            s,
            Strategy::Interleaved
        )
        .is_err());
    }
}
#[test]
fn pixel_rows_roundtrip_boundaries_and_reject_unused_bits() {
    for ndc in [-65536, -1, 0, 1, 65536] {
        let p = PixelInput {
            normal: [i16::MIN, i16::MAX, -1],
            ndc: [ndc, -ndc],
        };
        let q = PixelRows::encode(p).unwrap().decode().unwrap();
        assert_eq!(p.normal, q.normal);
        assert_eq!(p.ndc, q.ndc);
    }
    assert!(PixelRows([1 << 32, 0, 0]).decode().is_err());
    assert!(PixelRows([0, 0, 65537]).decode().is_err());
}
#[test]
fn bounded_search_preserves_work_and_independently_checks_candidates() {
    for n in [1, 4, 8, 16] {
        let base = run(
            n,
            Strategy::Interleaved,
            Hardware::default(),
            Storage::Rows {
                read_lanes: 2,
                latency: 2,
            },
        );
        let cycles = base.cycles;
        let work = base.work();
        let outputs = base.outputs.clone();
        let (best, search) = base.optimize(16).unwrap();
        assert_eq!(best.work(), work);
        assert_eq!(best.outputs, outputs);
        assert!(best.cycles <= cycles);
        assert_eq!(search.candidates.len(), 16);
        println!(
            "search batch={n} baseline={cycles} best={} strategy={} pressure={}",
            best.cycles,
            search.best_candidate().label,
            search.best_candidate().live_pressure
        );
    }
}
