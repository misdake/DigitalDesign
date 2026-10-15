use gpu_v2::texture::emu::unique_weights::{self as model, Event, Store};
fn hex(s: &str) -> Option<u128> {
    u128::from_str_radix(s, 16).ok()
}
fn main() {
    let compact = std::env::args().nth(2).as_deref() == Some("34");
    if let Some(dir) = std::env::args().nth(1) {
        let dir = std::path::Path::new(&dir);
        let mut store = Store::default();
        let mut edges = 0;
        for line in std::fs::read_to_string(dir.join("weight-store-events.txt"))
            .unwrap()
            .lines()
        {
            if line == "Z" {
                store
                    .tick(Event {
                        reset: true,
                        ..Default::default()
                    })
                    .unwrap();
                continue;
            }
            let s: Vec<_> = line.split_whitespace().collect();
            assert_eq!(s.len(), if compact { 16 } else { 15 });
            let v: Vec<_> = s[1..].iter().map(|s| hex(s)).collect();
            assert_eq!(
                store.paid.is_some(),
                v[8].unwrap() != 0,
                "paid at edge{edges}"
            );
            assert_eq!(
                store.result.is_some(),
                v[9].unwrap() != 0,
                "result at edge{edges}"
            );
            if store.paid.is_some() {
                assert_eq!(
                    u128::from(store.data_out),
                    v[10].unwrap(),
                    "DO at edge{edges}"
                );
            }
            if let Some((g, w)) = store.result {
                assert_eq!(
                    u128::from(g),
                    if compact {
                        u128::from(model::decode34(v[11].unwrap() as u64, v[14].unwrap() as u8))
                    } else {
                        v[11].unwrap()
                    }
                );
                assert_eq!(u128::from(w), v[12].unwrap());
                assert_eq!(
                    store.output().unwrap().unwrap(),
                    v[13].unwrap(),
                    "masked/inverse weights at edge{edges}"
                );
            }
            let e = Event {
                publish: v[0].filter(|&a| a != 255).map(|a| a as u8),
                free: v[1].filter(|&a| a != 255).map(|a| a as u8),
                write: v[2]
                    .filter(|&a| a != 255)
                    .map(|a| (a as usize, v[3].unwrap() as u64)),
                read: (v[4].unwrap() != 0).then(|| {
                    if compact {
                        model::decode34(v[5].unwrap() as u64, v[14].unwrap() as u8)
                    } else {
                        v[5].unwrap() as u64
                    }
                }),
                pop: v[6].unwrap() != 0,
                ..Default::default()
            };
            assert_eq!(
                store.tick(e).unwrap(),
                v[7].unwrap() != 0,
                "paid capture at edge{edges}"
            );
            edges += 1;
            assert!(edges < 200000);
        }
        let mut packets = 0;
        for line in std::fs::read_to_string(dir.join("packet-codec-events.txt"))
            .unwrap()
            .lines()
        {
            let s: Vec<_> = line.split_whitespace().map(|s| hex(s).unwrap()).collect();
            assert_eq!(s.len(), 3);
            assert_eq!(
                u128::from(if compact {
                    model::packet34(s[0], s[1] as usize).unwrap()
                } else {
                    model::packet40(s[0], s[1] as usize).unwrap()
                }),
                s[2]
            );
            packets += 1;
            assert!(packets < 200000);
        }
        // A first-refill fault can legally finish before any texel/weight read.
        assert!(edges > 0 && store.writes > 0 && packets > 0);
        println!("PASS actual physical weights old-state DO/result leases edges={edges} writes={} reads={} captures={} packet40={packets}",store.writes,store.reads,store.captures);
        return;
    }
    let mut checked = 0;
    for level in 0..=10 {
        for x in 0..8 {
            for y in 0..8 {
                for tap in 0..4 {
                    for mask in 1..16u128 {
                        if mask & (1 << tap) == 0 {
                            continue;
                        }
                        let m = (1u128 << 75)
                            | (1u128 << 74)
                            | (3u128 << 72)
                            | (15u128 << 68)
                            | ((level as u128) << 64)
                            | (2u128 << 60)
                            | ((y as u128) << 57)
                            | ((x as u128) << 54)
                            | (127u128 << 47)
                            | (127u128 << 40)
                            | (mask << 36)
                            | 0x7fffffff;
                        let g = model::packet40(m, tap).unwrap();
                        assert_eq!(model::decode34(model::packet34(m, tap).unwrap(), 2), g);
                        assert_eq!(model::member_address(m), model::group_address(g));
                        let restored = model::restore(g, (m & ((1 << 36) - 1)) as u64).unwrap();
                        for t in 0..4 {
                            assert_eq!(
                                (restored >> (28 + 9 * t)) & 511,
                                if g & (1 << (36 + t)) != 0 {
                                    (m >> (9 * t)) & 511
                                } else {
                                    0
                                }
                            );
                        }
                        checked += 1;
                    }
                }
            }
        }
    }
    let mut store = Store::default();
    for q in 0..16 {
        store
            .tick(Event {
                publish: Some(q),
                ..Default::default()
            })
            .unwrap();
    }
    for a in 0..128 {
        store
            .tick(Event {
                write: Some((a, (a as u64) * 12345)),
                ..Default::default()
            })
            .unwrap();
    }
    // Full128 independent RAM rows; no queue-count tautology. Rotate all addresses.
    let mut reads = 0;
    let mut outputs = 0;
    for wall in 0..3000 {
        let pop = store.result.is_some() && wall % 7 != 0;
        let capture = store.paid.is_some() && (store.result.is_none() || pop);
        let read =
            (reads < 512 && wall % 13 != 0 && (store.paid.is_none() || capture)).then(|| {
                let a = reads % 128;
                reads += 1;
                let g = (((a / 8) as u64) << 30)
                    | ((((a / 2) % 4) as u64) << 34)
                    | (((a % 2) as u64) << 4)
                    | (15u64 << 36);
                g
            });
        if pop {
            let (g, w) = store.result.unwrap();
            assert_eq!(w, (model::group_address(g) as u64) * 12345);
            outputs += 1;
        }
        store
            .tick(Event {
                pop,
                read,
                ..Default::default()
            })
            .unwrap();
        if reads == 512 && store.paid.is_none() && store.result.is_none() {
            break;
        }
    }
    assert_eq!(outputs, 512);
    assert_eq!(store.reads, 512);
    assert_eq!(store.captures, 512);
    for q in 0..16 {
        store
            .tick(Event {
                free: Some(q),
                ..Default::default()
            })
            .unwrap();
        store
            .tick(Event {
                publish: Some(q),
                ..Default::default()
            })
            .unwrap();
    }
    assert!(store
        .tick(Event {
            read: Some(15u64 << 36),
            ..Default::default()
        })
        .is_err());
    println!("PASS independent Group40 codec {checked} cases, full128 bank/DO/result holds512 reads, old-state public wrap/read-before-write rejected");
}
