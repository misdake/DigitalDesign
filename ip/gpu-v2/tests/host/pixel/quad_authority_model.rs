use gpu_v2::system::pixel::{quad_authority_model as candidate, quad_ingress as reference};
use std::collections::VecDeque;
#[test]
fn bounded_canonical_private_publish_wrap_ce0() {
    let mut old = reference::Ingress::new(20000);
    let mut new = candidate::CanonicalIngress::new(20000);
    let mut published = VecDeque::new();
    let mut cursor = 0u32;
    let mut completed = 0;
    let mut full = false;
    let mut full_edges = 0;
    let mut wraps = 0;
    for wall in 0..20000 {
        let ce = wall % 17 != 3 && wall % 17 != 4;
        let row = (cursor % 8) as u8;
        let job = cursor / 8;
        let begin = reference::Begin {
            mask: if job % 23 == 22 { 0 } else { 15 },
            draw: job & 1 != 0,
            unlit: job < 40 || job.is_multiple_of(3),
            untextured: job < 40 || job.is_multiple_of(5),
            force_coarsest: job & 2 != 0,
            origin: ((job * 71) & 32767) as u16,
        };
        let input = (job < 320).then_some(reference::Beat {
            begin,
            basic: if row & 1 != 0 {
                (job * 37 + u32::from(row)) & 65535
            } else {
                (job * 537) & 0xffffff
            },
            lighting: u64::from(job * 19 + u32::from(row)),
            uv: u64::from(job * 23 + u32::from(row)),
        });
        let rop = if ce && full {
            published.front().copied()
        } else {
            None
        };
        let read = if ce && wall % 3 == 0 {
            published
                .front()
                .map(|&slot| (slot << 3) | ((wall % 8) as u8))
        } else {
            None
        };
        let tick = reference::Tick {
            ce,
            input,
            light_ready: true,
            sample_ready: true,
            rop_row7: rop,
            basic_read: read,
            ..Default::default()
        };
        let nt = candidate::Tick {
            ce,
            input: input.map(|b| candidate::Beat {
                begin: candidate::Begin {
                    mask: b.begin.mask,
                    draw: b.begin.draw,
                    unlit: b.begin.unlit,
                    untextured: b.begin.untextured,
                    force_coarsest: b.begin.force_coarsest,
                    origin: b.begin.origin,
                },
                basic: b.basic,
                lighting: b.lighting,
                uv: b.uv,
            }),
            light_ready: true,
            sample_ready: true,
            rop_row7: rop,
            basic_read: read,
            ..Default::default()
        };
        let a = old.tick(tick).unwrap();
        let b = new.tick(nt).unwrap();
        assert_eq!(format!("{:?}", a), format!("{:?}", b), "edge {wall}");
        assert!(!a.signals.fault);
        if a.signals.global_used == 32 {
            full = true;
            full_edges += 1;
        }
        if rop.is_some() && a.signals.advance {
            published.pop_front();
            completed += 1;
        }
        if let Some((slot, _)) = a.published {
            if slot == 31 {
                wraps += 1;
            }
            published.push_back(slot);
        }
        if a.accepted {
            cursor += 1;
        }
        if cursor == 320 * 8 && published.is_empty() && a.signals.global_used == 0 {
            assert!(full_edges > 0 && wraps >= 9 && completed > 300);
            return;
        }
    }
    panic!("bounded candidate did not drain");
}
