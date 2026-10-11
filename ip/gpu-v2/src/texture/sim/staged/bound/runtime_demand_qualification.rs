//! Actual Runtime/physical-MC traces; descriptor probes are observations only.
use super::{qualification as q, *};
use crate::texture::ports::{Filter, RefillEvent, RefillPort};
use std::fmt::Write;

struct Port {
    physical: q::physical::Physical,
    wall: u64,
    terminal_delay: u64,
    held: Option<(u64, u64)>,
    requests: Vec<(u64, u64, u64)>,
}
impl RefillPort for Port {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        assert!(
            self.held.is_none(),
            "new request preceded old terminal event"
        );
        let id = self.physical.submit_read(address, bytes)?;
        self.requests.push((self.wall, id, address));
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        self.wall += 1;
        assert!(self.wall <= 30_000, "physical owner test watchdog");
        let mut result = vec![];
        if let Some((id, due)) = self.held {
            if self.wall == due {
                result.push(RefillEvent::Complete { id });
                self.held = None;
            }
        }
        for e in self.physical.step()? {
            if let RefillEvent::Complete { id } = e {
                if self.terminal_delay != 0 {
                    assert!(self.held.is_none());
                    self.held = Some((id, self.wall + self.terminal_delay));
                    continue;
                }
            }
            result.push(e);
        }
        Ok(result)
    }
}

#[test]
fn actual_runtime_global_head_has_one_demand_owner() {
    let root = std::env::var_os("GPU_RUNTIME_DEMAND_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/runtime-demand")
        });
    std::fs::create_dir_all(&root).unwrap();
    let mut summary = String::from(
        "case,edges,pixels,requests,peak_descriptors,CE0_active,await_terminal,II2_intervals\n",
    );
    for (name, seam, miss, terminal_delay, stalls, filter) in [
        ("warm-nearest", false, false, 0, false, Filter::Nearest),
        ("warm-trilinear", false, false, 0, false, Filter::Trilinear),
        ("seam", true, false, 0, false, Filter::Trilinear),
        ("miss", true, true, 0, false, Filter::Trilinear),
        ("terminal-delay", true, false, 23, false, Filter::Trilinear),
        ("CE0-result-stall", true, true, 0, true, Filter::Trilinear),
    ] {
        let (mut slots, mut bytes) = q::fixture();
        if miss {
            let first = slots[0];
            let template = bytes.clone();
            for i in 1..16 {
                let mut slot = first;
                slot.base_address += i * template.len() as u32;
                slots.push(slot);
                bytes.extend(&template);
            }
        }
        let mut r = q::runtime(
            &slots,
            super::super::control::Hardware {
                max_cycles: 30_000,
                ..Default::default()
            },
        );
        let allocation = super::super::runtime_inventory::describe(
            &r.preparation.binding,
            super::super::control::Hardware {
                max_cycles: 30_000,
                ..Default::default()
            },
            r.cache.external_context().1,
        )
        .unwrap();
        assert_eq!(
            allocation
                .rows
                .iter()
                .find(|row| row.name == "miss directory/active")
                .unwrap()
                .ff_bits,
            132,
            "one real demand owner replaces the four-descriptor reference ceiling"
        );
        let mut port = Port {
            physical: q::physical::Physical::new(4096, bytes.clone(), true, stalls),
            wall: 0,
            terminal_delay,
            held: None,
            requests: vec![],
        };
        let mut reference = q::runtime(
            &slots,
            super::super::control::Hardware {
                max_cycles: 30_000,
                ..Default::default()
            },
        );
        reference.cache.demand_queued_reference();
        let mut reference_port = Port {
            physical: q::physical::Physical::new(4096, bytes, true, stalls),
            wall: 0,
            terminal_delay,
            held: None,
            requests: vec![],
        };
        let inputs: Vec<_> = (0..48)
            .map(|i| {
                let mut input = q::input(
                    (i % 16) as u8,
                    15,
                    if seam {
                        [-0.03125 + (i % 7) as f64 / 32., 0.46875]
                    } else {
                        [0.1, 0.1]
                    },
                    filter,
                );
                if miss {
                    input.slot = (i % 16) as u8;
                }
                input
            })
            .collect();
        let golden: Vec<_> = inputs.iter().flat_map(q::golden).collect();
        let (mut accepted, mut peak, mut ce0_active, mut await_terminal) = (0, 0, 0, 0);
        let mut pixels = vec![];
        let mut result_edges = vec![];
        let mut trace = String::new();
        for wall in 0..30_000 {
            let control = timed::Control {
                ce: !stalls || wall % 7 > 1,
                result_ready: !stalls || (wall > 600 && wall % 13 > 2),
            };
            let before = r.cache.demand_owner_probe();
            let s = r.step(&mut port, inputs.get(accepted), control).unwrap();
            let old = reference
                .step(&mut reference_port, inputs.get(accepted), control)
                .unwrap();
            let after = r.cache.demand_owner_probe();
            assert_eq!(
                after,
                reference.cache.demand_owner_probe(),
                "{name} actual descriptor owner wall{wall}"
            );
            assert_eq!(
                s.preparation, old.preparation,
                "{name} preparation wall{wall}"
            );
            assert_eq!(s.results, old.results, "{name} pixels wall{wall}");
            assert_eq!(s.snapshot, old.snapshot, "{name} ownership wall{wall}");
            assert_eq!(
                format!("{:?}", s.cache),
                format!("{:?}", old.cache),
                "{name} every cache port/event/state wall{wall}"
            );
            assert_eq!(
                format!("{:?}", s.color),
                format!("{:?}", old.color),
                "{name} Color ports/events wall{wall}"
            );
            assert_eq!(
                (s.accepted, s.input_ready, s.effective_ce),
                (old.accepted, old.input_ready, old.effective_ce)
            );
            assert_eq!(
                port.requests, reference_port.requests,
                "{name} actual transport descriptors"
            );
            assert!(
                before.0 + before.1 <= 1 && after.0 + after.1 <= 1,
                "{name} wall{wall}"
            );
            peak = peak.max(after.0 + after.1);
            ce0_active += usize::from(!control.ce && before.1 == 1);
            await_terminal += usize::from(after.2 && after.3 == 16);
            assert_eq!(s.snapshot.cache.descriptors, after.0 + after.1);
            writeln!(
                trace,
                "{}|{}|{:?}|{:?}|{:?}",
                s.cycle, control.ce, before, after, s.cache.responses
            )
            .unwrap();
            accepted += usize::from(s.accepted);
            result_edges.extend(std::iter::repeat_n(s.cycle, s.results.len()));
            pixels.extend(s.results);
            if accepted == inputs.len() && r.idle() {
                break;
            }
        }
        assert!(
            r.idle() && reference.idle() && port.held.is_none() && reference_port.held.is_none()
        );
        assert_eq!(accepted, inputs.len());
        assert_eq!(pixels, golden);
        assert_eq!(peak, 1);
        assert_eq!(r.cache.stats.peak_descriptors, 1);
        assert_eq!(r.cache.stats.promotions, 0);
        assert_eq!(
            format!("{:?}", r.cache.stats),
            format!("{:?}", reference.cache.stats),
            "{name} all cache statistics"
        );
        if stalls {
            assert!(ce0_active > 0);
        }
        if terminal_delay > 0 {
            assert!(await_terminal > 0);
        }
        let intervals = result_edges[63..128]
            .windows(2)
            .filter(|w| w[1] - w[0] == 2)
            .count();
        if !seam && !stalls {
            assert_eq!(intervals, 64, "{name} warm actual spacing");
        }
        writeln!(
            summary,
            "{name},{},{},{},{peak},{ce0_active},{await_terminal},{intervals}",
            port.wall,
            pixels.len(),
            port.requests.len()
        )
        .unwrap();
        std::fs::write(root.join(format!("{name}.trace.txt")), trace).unwrap();
    }
    std::fs::write(root.join("cases.csv"), summary).unwrap();
}
