//! Real capacity-selected Runtime execution with fixed Public16 and cold service.
use super::{qualification as q, *};
use crate::texture::ports::{Filter, RefillEvent, RefillPort};
struct Held {
    inner: q::physical::Physical,
    wall: u64,
}
impl RefillPort for Held {
    fn submit_read(&mut self, a: u64, n: usize) -> Result<u64, String> {
        self.inner.submit_read(a, n)
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        self.wall += 1;
        if self.wall < 3500 {
            Ok(vec![])
        } else {
            self.inner.step()
        }
    }
}
#[test]
fn group48_runtime_actual_cold_capacity() {
    const LIMIT: u64 = 50_000;
    for early in [false, true] {
        let (slots, bytes) = q::fixture();
        let inputs: Vec<_> = (0..512)
            .map(|i| q::input((i % 16) as u8, 15, [0.5, 0.5], Filter::Trilinear))
            .collect();
        let gold: Vec<_> = inputs.iter().flat_map(q::golden).collect();
        let mut outputs = vec![];
        for profile in [32, 48] {
            let p = PreparationHardware {
                contexts: 8,
                coordinate_credits: 6,
                release_after_capture: early,
                max_cycles: LIMIT,
                ..Default::default()
            };
            let h = timed::Hardware {
                prefetch: false,
                max_quads: 1,
                max_cycles: LIMIT,
                ..Default::default()
            };
            let mut r = if profile == 48 {
                Runtime::new_group48(&slots, p, h, LIMIT).unwrap()
            } else {
                Runtime::new(&slots, p, h, LIMIT).unwrap()
            };
            let mut mc = Held {
                inner: q::physical::Physical::new(4096, bytes.clone(), true, true),
                wall: 0,
            };
            let mut accepted = 0;
            let mut pixels = vec![];
            let (
                mut peakg,
                mut peakp,
                mut peakrows,
                mut maxaddr,
                mut writes,
                mut wraps,
                mut coldcaptures,
            ) = (0, 0, 0, 0, 0, 0, 0);
            let mut producer_pipe = [None; 2];
            for edge in 0..LIMIT {
                let control = timed::Control {
                    ce: edge % 17 != 3,
                    result_ready: edge % 13 > 1,
                };
                let old = r.cache.packet_storage_state().unwrap().0;
                let step = r.step(&mut mc, inputs.get(accepted), control).unwrap();
                let c = r.cache.packet_storage_state().unwrap().0;
                let heads = c.head_valid.iter().filter(|x| **x).count() as u8;
                assert_eq!(c.producer + c.groups, c.rows + heads);
                assert!(heads + u8::from(c.pending) <= 2);
                assert!(c.groups <= profile);
                if step.effective_ce {
                    let (t, e) = r.cache.packet_storage_trace().unwrap();
                    assert_eq!(t.write.map(|v| v as i128), r.preparation.trace.packet);
                    assert_eq!(
                        t.write.is_some(),
                        producer_pipe[1].is_some(),
                        "actual E2 no holes"
                    );
                    producer_pipe = [t.reserve, producer_pipe[0]];
                    if let Some(row) = e.write {
                        writes += 1;
                        maxaddr = maxaddr.max(row);
                        wraps += usize::from(row == 63);
                    }
                    coldcaptures += usize::from(mc.wall < 3500 && e.reserve.is_some());
                } else {
                    assert_eq!(old, c, "CE0 register hold");
                }
                peakg = peakg.max(c.groups);
                peakp = peakp.max(c.producer);
                peakrows = peakrows.max(c.rows);
                accepted += usize::from(step.accepted);
                pixels.extend(step.results);
                if accepted == inputs.len() && r.idle() {
                    break;
                }
            }
            assert_eq!(accepted, inputs.len());
            assert!(r.idle());
            assert_eq!(producer_pipe, [None, None]);
            assert_eq!(pixels, gold);
            if profile != 32 {
                assert_eq!((peakg, peakp, peakrows, coldcaptures), (48, 16, 62, 64));
                assert!(
                    peakg > 32 && maxaddr == 63 && wraps >= 4,
                    "new capacity/wrap actually exercised"
                );
            }
            println!("Group48 production Runtime profile={profile} early={early}: wall={} accepted={accepted} pixels={} gc={peakg} pc={peakp} rows={peakrows} writes={writes} maxaddr={maxaddr} wraps={wraps} coldcaptures={coldcaptures}",r.stats.link.wall,pixels.len());
            outputs.push(pixels);
        }
        assert_eq!(
            outputs[0], outputs[1],
            "ordered numeric outputs with independent acceptance times"
        );
    }
}
