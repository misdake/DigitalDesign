//! Full two-mip hot flow: a consumed Color link must permit raw admission.
#[path = "support/texture_capture.rs"]
mod capture;
#[path = "support/sdram/physical_texture.rs"]
#[allow(dead_code)]
mod physical;
#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    emu::color::{Output, RESULT_CAPACITY},
    ports::*,
    sim::{oracle, staged::bound, timed},
};

fn quad(id: usize) -> QuadInput {
    let mut q = support::input(5, Filter::Trilinear, [0.13, 0.07]);
    q.quad_id = (id % 16) as u8;
    q.uv[1][0] += 1.0 / 32.0;
    q.uv[2][1] += 1.0 / 32.0;
    q.uv[3][0] += 1.0 / 32.0;
    q.uv[3][1] += 1.0 / 32.0;
    q.lod_bias = 0.5;
    q
}

#[test]
fn two_mip_hot_ingress_consumes_and_refills_without_bubbles_or_credit_borrowing() {
    for paused in [false, true] {
        let mut audit = capture::CaptureAudit::default();
        let slot = support::slot(5, true);
        let bytes = support::asset(slot, support::pattern);
        let mut memory =
            physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, false);
        let mut runtime = bound::runtime::Runtime::new(
            &[slot],
            bound::control::Hardware {
                storage: bound::control::Storage::Packed,
                ..Default::default()
            },
            timed::Hardware {
                prefetch: false,
                max_quads: 1,
                ..Default::default()
            },
            100_000,
        )
        .unwrap();
        let warm = RawQuadInput::capture(&quad(0)).unwrap();
        let first = runtime
            .step_raw(&mut memory, Some(&warm), timed::Control::default())
            .unwrap();
        assert!(first.accepted);
        audit.observe(&first);
        for _ in 0..10_000 {
            let step = runtime
                .step_raw(&mut memory, None, timed::Control::default())
                .unwrap();
            audit.observe(&step);
            if runtime.idle() {
                break;
            }
        }
        assert!(runtime.idle(), "warm-up watchdog");
        let refills = runtime.cache_stats().refills;
        let mut sent = 0;
        let mut got = Vec::new();
        let mut cycles = Vec::new();
        let mut packets = 0;
        let mut levels = [0usize; 11];
        let mut accepted_with_held_link = 0;
        let mut full_credit_stall = false;
        for t in 0..80_000 {
            let input = RawQuadInput::capture(&quad(sent)).unwrap();
            let before = runtime.snapshot();
            let step = runtime
                .step_raw(
                    &mut memory,
                    (sent < 128).then_some(&input),
                    timed::Control {
                        ce: !paused || t % 17 > 3,
                        result_ready: !paused || (t > 1200 && t % 29 > 5),
                    },
                )
                .unwrap();
            audit.observe(&step);
            assert_eq!(
                step.accepted,
                step.offered.is_some() && step.input_ready && step.control.ce
            );
            assert_eq!(step.accepted, step.cache.accepted);
            assert_eq!(step.accepted, step.preparation.accepted);
            if before.color_input && step.accepted {
                assert!(step.color.accepted && step.effective_ce);
                accepted_with_held_link += 1;
            }
            if before.color_input && step.control.ce && !step.color.accepted {
                assert!(!step.accepted && !step.effective_ce);
                assert_eq!(before.color.result_credits, RESULT_CAPACITY);
                full_credit_stall = true;
            }
            for event in &step.preparation.events {
                if let bound::control::Event::Packet { payload, .. } = *event {
                    let word = payload as u128;
                    assert!((0..4).all(|tap| (word >> (28 + tap * 9)) & 511 != 0));
                    levels[((word >> 4) & 15) as usize] += 1;
                    packets += 1;
                }
            }
            cycles.extend(std::iter::repeat_n(step.cycle, step.results.len()));
            got.extend(step.results);
            sent += usize::from(step.accepted);
            if sent == 128 && runtime.idle() {
                break;
            }
        }
        assert!(sent == 128 && runtime.idle(), "flow watchdog");
        assert_eq!(runtime.cache_stats().refills, refills);
        assert_eq!(packets, 1024);
        assert_eq!((levels[4], levels[5]), (512, 512));
        let mut cache = oracle::Cache::new(vec![slot]).unwrap();
        let mut image = support::Image {
            bytes,
            requests: vec![],
        };
        let expected: Vec<_> = (0..128)
            .flat_map(|id| {
                let q = quad(id);
                oracle::sample(&q, &mut cache, &mut image, Config::counted())
                    .unwrap()
                    .pixels
                    .into_iter()
                    .map(move |p| Output {
                        key: q.quad_id * 4 + p.lane,
                        rgb: p.rgb,
                    })
            })
            .collect();
        assert_eq!(got, expected);
        if paused {
            assert!(audit.frozen_pending > 0 && audit.longest_link_hold > 1);
            assert!(
                full_credit_stall,
                "must exercise exhausted old-state result credit"
            );
        } else {
            assert!(
                accepted_with_held_link > 32,
                "normal consumed Color traffic must allow raw ingress"
            );
            assert!(
                cycles.windows(2).all(|w| w[1] - w[0] == 2),
                "all 511 result intervals must sustain trilinear II2"
            );
        }
        audit.finish(if paused { "hot-paused" } else { "hot-open" });
    }
}
