//! Independent semantic goldens, stream/CE/backpressure tests and finite
//! resource checks for the `final_stage` leaf. The Icarus co-simulation lives
//! in `pixel_final_stage_rtl.rs`.

use gpu_v2::lighting::ports::LightingOutput;
use gpu_v2::system::pixel::final_rgb;
use gpu_v2::system::pixel::final_stage::{
    emu::FinalEmu,
    math, rtl,
    sim::{counted, credit, timed},
    Input, Output, Tick, KEY_MASK, PIPELINE_LATENCY, RESULT_CAPACITY,
};
use std::collections::VecDeque;

struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
    fn sample(&mut self) -> Input {
        Input {
            key: self.byte() & KEY_MASK,
            tint: std::array::from_fn(|_| self.byte()),
            texture: std::array::from_fn(|_| self.byte()),
            g: (self.next() % 512) as u16,
            h: (self.next() % 257) as u16,
            specular: std::array::from_fn(|_| self.byte()),
        }
    }
}

fn corner_values() -> Vec<u8> {
    vec![0, 1, 2, 127, 128, 129, 168, 254, 255]
}

#[test]
fn base_unorm255_is_exact_over_the_whole_u8_domain() {
    for tint in 0..=255u8 {
        for texture in 0..=255u8 {
            let product = u32::from(tint) * u32::from(texture);
            // Exact nearest for a denominator with no integer half-way value.
            assert_eq!(
                u32::from(math::base_unorm255(tint, texture)),
                (product + 127) / 255,
                "tint={tint} texture={texture}"
            );
        }
    }
}

#[test]
fn rne_div256_is_ties_even_over_the_legal_sum_domain() {
    for sum in 0..=math::MAX_SUM {
        let floor = sum / 256;
        let remainder = sum % 256;
        let expected = floor + u32::from(remainder > 128 || (remainder == 128 && floor & 1 != 0));
        assert_eq!(math::rne_div256(sum), expected, "sum={sum}");
    }
    assert_eq!(math::rne_div256(128), 0, "even floor tie rounds down");
    assert_eq!(math::rne_div256(256 + 128), 2, "odd floor tie rounds up");
}

#[test]
fn endpoint_and_tie_channels_match_true_division() {
    let values = corner_values();
    for &tint in &values {
        for &texture in &values {
            for &specular in &values {
                for g in [0u16, 1, 2, 127, 128, 255, 256, 510, 511] {
                    for h in [0u16, 1, 2, 127, 128, 254, 255, 256] {
                        let fast = math::channel(tint, texture, g, h, specular);
                        let exact = math::channel_true_division(tint, texture, g, h, specular);
                        assert_eq!(fast, exact, "t={tint} x={texture} g={g} h={h} s={specular}");
                    }
                }
            }
        }
    }
}

#[test]
fn reference_matches_existing_final_rgb_including_ties_and_saturation() {
    let mut rng = Lcg::new(0x0F17A12026);
    let mut inputs: Vec<Input> = Vec::new();
    // Directed: all-channel corners and second-rounding ties.
    for &tint in &[0u8, 127, 128, 255] {
        for &texture in &[0u8, 1, 254, 255] {
            for &specular in &[0u8, 128, 255] {
                for g in [0u16, 1, 128, 255, 511] {
                    for h in [0u16, 0, 128, 256] {
                        inputs.push(Input {
                            key: (g as u8) & KEY_MASK,
                            tint: [tint; 3],
                            texture: [texture; 3],
                            g,
                            h,
                            specular: [specular; 3],
                        });
                    }
                }
            }
        }
    }
    // Pseudo-random, including reused keys.
    for i in 0..20_000 {
        let mut sample = rng.sample();
        sample.key = (i % 5) as u8;
        inputs.push(sample);
    }
    for input in &inputs {
        let existing = final_rgb(
            input.tint,
            input.texture,
            LightingOutput {
                g: input.g,
                h: input.h,
            },
            input.specular,
        )
        .unwrap();
        assert_eq!(input.reference_rgb(), existing, "{input:?}");
    }
}

#[test]
fn counted_batch_matches_independent_golden_for_every_pixel() {
    let mut rng = Lcg::new(0xC0FFEE2026);
    let mut inputs = Vec::new();
    inputs.push(Input {
        key: 1,
        tint: [255; 3],
        texture: [255; 3],
        g: 511,
        h: 256,
        specular: [255; 3],
    });
    inputs.push(Input {
        key: 2,
        tint: [0; 3],
        texture: [255; 3],
        g: 0,
        h: 0,
        specular: [0; 3],
    });
    for _ in 0..600 {
        inputs.push(rng.sample());
    }
    let report = counted::run(&inputs).unwrap();
    assert_eq!(report.outputs.len(), inputs.len());
    for (input, output) in inputs.iter().zip(&report.outputs) {
        assert_eq!(output.key, input.key);
        assert_eq!(output.rgb, input.reference_rgb(), "{input:?}");
    }
}

#[test]
fn timed_plan_binds_the_fixed_stage_calendar_and_credit_completion() {
    let mut rng = Lcg::new(0x7E52026);
    let inputs: Vec<Input> = (0..64).map(|_| rng.sample()).collect();
    let plan = timed::run(&inputs, timed::Hardware::default()).unwrap();
    plan.audit().unwrap();
    assert_eq!(plan.initiation_interval, 1);
    assert_eq!(plan.latency, PIPELINE_LATENCY as u64);
    // The plan reports the fixed implementation calendar, not an arbitrary
    // modulo schedule: tint*texture/specular*h at 1, base*g at 4, sum at 5,
    // +127 at 6, +bit at 7, compare at 8, select at 9.
    assert_eq!(
        plan.calendar.class_census(),
        timed::StageCalendar::fixed_pattern()
    );
    assert_eq!(plan.calendar.lanes_used[&timed::LaneKind::Multiply18], 9);
    assert_eq!(plan.calendar.lanes_used[&timed::LaneKind::Add16], 6);
    assert_eq!(plan.calendar.lanes_used[&timed::LaneKind::Add18], 9);
    assert_eq!(plan.calendar.lanes_used[&timed::LaneKind::Compare], 3);
    assert_eq!(plan.calendar.lanes_used[&timed::LaneKind::Select], 3);
    // Arithmetic-only lower bound is separate from finite credit completion.
    assert_eq!(
        plan.arithmetic_lower_bound,
        plan.latency + inputs.len() as u64 - 1
    );
    let n = inputs.len() as u64;
    assert_eq!(
        plan.completion.enabled_edges,
        11 * n.div_ceil(4) + (n - 1) % 4
    );
    assert!(plan.completion.enabled_edges > plan.arithmetic_lower_bound);
    // Literal additions and the +1 step are classified apart from full adds.
    let mut literal = 0;
    let mut variable = 0;
    let mut increment = 0;
    for binding in &plan.calendar.bindings {
        match binding.class {
            Some(timed::OpClass::LiteralAdd) => literal += 1,
            Some(timed::OpClass::VariableAdd) => variable += 1,
            Some(timed::OpClass::Increment) => increment += 1,
            _ => {}
        }
    }
    assert_eq!(literal, 6, "+128 x3 and +127 x3");
    assert_eq!(variable, 9, "+(t>>8), sum and +bit per channel");
    assert_eq!(increment, 0, "host index +1 is not pixel arithmetic");
    // Inputs, slices, resizes and literals are wires and consume no lane.
    assert!(plan
        .calendar
        .bindings
        .iter()
        .filter(|b| !b.registered)
        .all(|b| b.lane.is_none()));
    // Concatenating the same batch must not change the fixed schedule.
    let single = timed::run(&inputs[..1], timed::Hardware::default()).unwrap();
    assert_eq!(single.latency, plan.latency);
    assert_eq!(single.calendar, plan.calendar);
    // Starving a dedicated lane makes II=1 infeasible; failure is explicit.
    let starved = timed::Hardware {
        mul18_lanes: 1,
        ..timed::Hardware::default()
    };
    assert!(timed::run(&inputs, starved).is_err());
}

#[test]
fn credit_calendar_matches_the_emulator_over_a_long_continuous_stream() {
    let n = 1000usize;
    let pixel = Input {
        key: 7,
        tint: [200, 10, 255],
        texture: [3, 254, 1],
        g: 511,
        h: 256,
        specular: [255, 0, 128],
    };
    let mut emu = FinalEmu::new(200_000).unwrap();
    let mut cal = credit::CreditCalendar::leaf();
    let mut offered = 0usize;
    let mut accepted = 0usize;
    let mut retired = 0usize;
    let mut enabled = 0u64;
    for _ in 0..60_000u64 {
        let offer = offered < n;
        let step = emu
            .tick(Tick {
                ce: true,
                input: offer.then_some(pixel),
                output_ready: true,
            })
            .unwrap();
        let calendar = cal.step(offer, true);
        enabled += 1;
        // Occupancy-only calendar agrees with the numerical emulator every edge.
        assert_eq!(step.accepted, calendar.accepted, "accept edge {enabled}");
        assert_eq!(step.consumed, calendar.consumed, "consume edge {enabled}");
        assert_eq!(emu.credits(), cal.credits());
        assert_eq!(emu.queued(), cal.queued());
        if step.accepted {
            offered += 1;
            accepted += 1;
        }
        if step.consumed {
            retired += 1;
        }
        if accepted == n && retired == n && emu.idle() {
            break;
        }
    }
    assert_eq!(accepted, n);
    assert_eq!(retired, n);
    // Observed finite completion matches the credit-aware closed form.
    let n = n as u64;
    assert_eq!(enabled, 11 * n.div_ceil(4) + (n - 1) % 4);
    // Observed steady throughput is capacity-bound, never one per clock.
    assert!(retired < enabled as usize);
    assert!(enabled * 4 >= n * 11, "below the 4/11 capacity band");
    let done = cal.completion(n as usize, 100_000).unwrap();
    assert_eq!(done.enabled_edges, enabled);
    assert_eq!(done.max_in_flight, RESULT_CAPACITY);
}

#[test]
fn emu_streams_with_ce_pauses_backpressure_and_reused_keys() {
    let mut rng = Lcg::new(0xB00D2026);
    let inputs: Vec<Input> = (0..300)
        .map(|i| {
            let mut sample = rng.sample();
            sample.key = (i % 4) as u8; // deliberate key reuse
            if i % 37 == 0 {
                sample.g = 511;
                sample.h = 256;
            }
            sample
        })
        .collect();
    let mut emu = FinalEmu::new(200_000).unwrap();
    let mut expected: VecDeque<Output> = VecDeque::new();
    let mut ptr = 0;
    let mut consumed = 0;
    let mut accepted = 0;
    let mut max_in_flight = 0;
    for wall in 0..20_000u64 {
        if ptr == inputs.len() && expected.is_empty() && emu.idle() {
            break;
        }
        let ce = wall % 11 != 3 && wall % 11 != 4;
        let ready = wall % 7 != 5 && wall % 13 < 9;
        let input = inputs.get(ptr).copied();
        let before = emu.head();
        let before_snapshot = emu.snapshot();
        let step = emu
            .tick(Tick {
                ce,
                input,
                output_ready: ready,
            })
            .unwrap();
        assert_eq!(step.output, before, "pre-edge output identity");
        if let Some(prev) = before {
            if !step.consumed {
                assert_eq!(
                    emu.head(),
                    Some(prev),
                    "held output changed without a transfer"
                );
            }
        }
        if !ce {
            assert_eq!(step.snapshot.pipeline_keys, before_snapshot.pipeline_keys);
            assert_eq!(step.snapshot.credits, before_snapshot.credits);
            assert_eq!(step.snapshot.queued, before_snapshot.queued);
            assert_eq!(step.snapshot.enabled, before_snapshot.enabled);
        }
        if step.consumed {
            let want = expected.pop_front().expect("consume without a reservation");
            assert_eq!(step.output, Some(want));
            consumed += 1;
        }
        if step.accepted {
            let pixel = input.unwrap();
            expected.push_back(Output::new(pixel.key, pixel.reference_rgb()));
            ptr += 1;
            accepted += 1;
        }
        assert_eq!(emu.credits(), emu.in_flight() + emu.queued());
        assert_eq!(emu.credits(), expected.len());
        max_in_flight = max_in_flight.max(emu.in_flight());
        assert!(emu.credits() <= RESULT_CAPACITY);
    }
    assert_eq!(consumed, inputs.len());
    assert_eq!(accepted, inputs.len());
    assert!(expected.is_empty());
    assert!(emu.idle());
    // Pipelining really overlapped more than one accepted pixel.
    assert!(max_in_flight >= 2, "no burst overlap observed");
}

#[test]
fn credit_return_never_funds_a_same_edge_acceptance() {
    let mut emu = FinalEmu::new(1000).unwrap();
    let pixel = Input {
        key: 3,
        tint: [10, 20, 30],
        texture: [40, 50, 60],
        g: 100,
        h: 50,
        specular: [70, 80, 90],
    };
    // Fill the four result credits while withholding output transfers.
    for i in 0..RESULT_CAPACITY {
        let step = emu
            .tick(Tick {
                ce: true,
                input: Some(pixel),
                output_ready: false,
            })
            .unwrap();
        assert!(step.accepted, "accept {i}");
    }
    assert_eq!(emu.credits(), RESULT_CAPACITY);
    // Drain four results so the FIFO holds all four credits.
    for _ in 0..30 {
        emu.tick(Tick {
            ce: true,
            input: None,
            output_ready: false,
        })
        .unwrap();
    }
    assert_eq!(emu.queued(), RESULT_CAPACITY);
    // The edge that transfers one result must not also accept a new pixel.
    let blocked = emu
        .tick(Tick {
            ce: true,
            input: Some(pixel),
            output_ready: true,
        })
        .unwrap();
    assert!(blocked.consumed);
    assert!(!blocked.accepted, "credit returned on the same edge");
    // The freed credit funds acceptance only on the following edge.
    let next = emu
        .tick(Tick {
            ce: true,
            input: Some(pixel),
            output_ready: false,
        })
        .unwrap();
    assert!(next.accepted);
}

#[test]
fn constructor_bounds_and_watchdog_are_explicit() {
    assert!(FinalEmu::new(0).is_err());
    assert!(FinalEmu::new(u64::MAX).is_err());
    let mut emu = FinalEmu::new(3).unwrap();
    for _ in 0..3 {
        emu.tick(Tick::default()).unwrap();
    }
    assert!(emu.tick(Tick::default()).unwrap_err().contains("watchdog"));
    assert!(emu.faulted());
    assert!(emu.tick(Tick::default()).is_err());
}

#[test]
fn illegal_inputs_are_rejected_only_when_offered() {
    let mut emu = FinalEmu::new(100).unwrap();
    let illegal = Input {
        key: 64,
        tint: [0; 3],
        texture: [0; 3],
        g: 0,
        h: 0,
        specular: [0; 3],
    };
    // Parked while `ce=0`: not accepted, not validated.
    let step = emu
        .tick(Tick {
            ce: false,
            input: Some(illegal),
            output_ready: true,
        })
        .unwrap();
    assert!(!step.accepted);
    assert!(!emu.faulted());
    // Offered on an enabled edge: rejected as a terminal fault.
    assert!(emu
        .tick(Tick {
            ce: true,
            input: Some(illegal),
            output_ready: true,
        })
        .is_err());
    assert!(emu.faulted());
}

#[test]
fn rtl_declaration_matches_the_emulated_pipeline() {
    let allocation = rtl::Allocation::default();
    assert_eq!(rtl::latency(), PIPELINE_LATENCY);
    assert_eq!(rtl::initiation_interval(), 1);
    assert_eq!(allocation.mul18_lanes, 9);
    assert_eq!(allocation.add16_lanes, 6);
    assert_eq!(allocation.add18_lanes, 9);
    assert_eq!(allocation.fifo_payload_bits, RESULT_CAPACITY * 30);
    assert_eq!(allocation.pipeline_bits, 777);
    assert!(allocation.portless_storage_bits() < allocation.total_state_bits());
    let source = rtl::source();
    assert!(source.contains("module gpu_v2_final_stage"));
    assert!(source.contains("assign in_ready  = ce && (credits < 3'd4);"));
}
