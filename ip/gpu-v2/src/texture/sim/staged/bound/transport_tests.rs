use super::*;
use std::collections::VecDeque;

#[test]
fn bounded_ram_single_head_wrap_pause_and_last_use() {
    for capacity in [2, 3, 16, 17, 32] {
        let mut work = Work::new(capacity).unwrap();
        assert_eq!(work.rows.len(), capacity.next_power_of_two().max(16));
        let mut owners = VecDeque::new();
        let total = capacity * 4 + 3;
        let mut submitted = 0;
        let mut completed = 0;
        let mut next_tap = 0;
        let mut pause = false;
        let mut counts = [0; 7]; // W,R,C,ACK,partial,pause-pending,pause-valid
        for _ in 0..20_000 {
            let old = work.snapshot();
            let ce = !pause;
            let write = (ce && old.materialized < capacity && submitted < total).then(|| {
                // Equal weight data under distinct owners. Odd rows have four
                // groups; even rows one. Goldens use literal field offsets.
                let weights = if submitted % 2 == 0 {
                    511
                } else {
                    128 | 128 << 9 | 128 << 18 | 127 << 27
                };
                Member(
                    weights
                        | (if submitted % 2 == 0 { 1 } else { 15 }) << 36
                        | ((submitted % 16) as u128) << 84
                        | ((submitted / 16 % 4) as u128) << 88,
                )
            });
            let consume = ce && old.valid;
            let edge = work.tick(ce, write, consume).unwrap();
            if !ce {
                assert_eq!(work.snapshot(), old);
                assert!(edge.read.is_none() && edge.write.is_none() && !edge.returned && !edge.ack);
                counts[5] += usize::from(old.pending);
                counts[6] += usize::from(old.valid);
                pause = false;
                continue;
            }
            if edge.read.is_some() {
                assert!(!old.valid && !old.pending && old.materialized > 0);
                assert_ne!(edge.read, edge.write);
                counts[1] += 1;
            }
            if edge.returned {
                assert!(old.pending && !consume);
                counts[2] += 1;
            }
            if let Some((member, tap)) = edge.capture {
                assert!(old.valid && !edge.returned);
                assert_eq!(Some(&member), owners.front());
                assert_eq!(tap, next_tap);
                if member.emit() == 15 && tap != 3 {
                    next_tap += 1;
                    counts[4] += 1;
                    assert!(!edge.ack);
                } else {
                    assert!(edge.ack);
                    owners.pop_front();
                    completed += 1;
                    next_tap = 0;
                }
            }
            if edge.ack {
                assert!(edge.read.is_none());
                counts[3] += 1;
            }
            if let Some(member) = write {
                owners.push_back(member);
                submitted += 1;
                counts[0] += 1;
                if old.materialized == 0 {
                    assert!(edge.read.is_none());
                }
            }
            assert!(work.snapshot().materialized <= capacity);
            assert_eq!(work.snapshot().materialized, owners.len());
            pause = edge.read.is_some() || edge.returned || edge.ack;
            if completed == total {
                break;
            }
        }
        assert_eq!(submitted, total);
        assert_eq!(completed, total);
        assert_eq!(&counts[..4], &[total; 4]);
        assert!(counts[4..].iter().all(|&n| n > 0));
        println!(
            "WORK W{capacity}: totals={counts:?} logical_wraps={} source_row_held_until_ACK=true",
            total / capacity
        );
        // C is not consumable on its own edge, and failed transport use has no
        // recoverable public API: Runtime owns the enclosing terminal fault.
        let mut work = Work::new(capacity).unwrap();
        let row = Member(511 | 1 << 36);
        work.tick(true, Some(row), false).unwrap();
        work.tick(true, None, false).unwrap();
        assert!(work.tick(true, None, true).is_err());
        assert!(work.snapshot().pending);
    }
}
