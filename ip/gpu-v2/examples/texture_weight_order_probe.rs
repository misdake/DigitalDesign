use gpu_v2::{
    memory::ports::{MemoryPort, Request, Response},
    texture::{
        emu::{
            cache::{tap_map, CacheEmu, CacheTick},
            color::{ColorEmu, Input, Tick},
        },
        ports::{Group4, Slot, TileKey},
    },
};
#[derive(Default)]
struct Memory {
    beat: Option<u8>,
}
impl MemoryPort for Memory {
    fn cycle(&mut self, r: Option<Request>, _: Option<u64>) -> Result<Response, String> {
        let mut v = Response::default();
        if let Some(b) = self.beat {
            let data = (0..4).fold(0u64, |a, k| {
                a | ((((u32::from(b) * 4 + k) * 421 + 0x1234) & 65535) as u64) << (16 * k)
            });
            v.read = Some((b, data));
            if b == 15 {
                v.complete = Some(true);
                self.beat = None
            } else {
                self.beat = Some(b + 1)
            }
        } else if r.is_some() {
            v.accepted = true;
            self.beat = Some(0)
        }
        Ok(v)
    }
}
fn transformed(payload: i128) -> i128 {
    let mut g = Group4::unpack72(payload).unwrap();
    let (banks, _) = tap_map(g.top_left_local);
    let old = g.coefficients;
    for t in 0..4 {
        g.coefficients[banks[t] as usize] = old[t]
    }
    g.pack72().unwrap() as i128
}
fn rtl_map(p: [u8; 2]) -> [usize; 4] {
    let x = p[0] & 1;
    let y0 = (p[1] & 1) ^ ((p[0] >> 1) & 1);
    let y1 = y0 ^ x;
    [
        x as usize + 2 * y0 as usize,
        (x ^ 1) as usize + 2 * y1 as usize,
        x as usize + 2 * (y0 ^ 1) as usize,
        (x ^ 1) as usize + 2 * (y1 ^ 1) as usize,
    ]
}
fn main() {
    if let Some(path) = std::env::args().nth(1) {
        let mut n = 0;
        for line in std::fs::read_to_string(path).unwrap().lines() {
            let x: Vec<u128> = line
                .split_whitespace()
                .map(|s| u128::from_str_radix(s, 16).unwrap())
                .collect();
            assert_eq!(x.len(), 4);
            assert_eq!(transformed(x[0] as i128) as u128, x[2]);
            assert_eq!(x[1], x[3]);
            n += 1;
        }
        assert!(n > 0 && n < 200000);
        println!("PASS actual cache return inverse weights+physical texels groups={n}");
        return;
    }
    for x in 0..8u8 {
        for y in 0..8u8 {
            let (a, _) = tap_map([x, y]);
            assert_eq!(a.map(usize::from), rtl_map([x, y]));
        }
    }
    // Every mask, including an all-zero contribution, preserves all channel tap sums.
    for x in 0..4u8 {
        for y in 0..2u8 {
            for mask in 0..16u8 {
                let map = rtl_map([x, y]);
                let physical = [0x001fu16, 0xf800, 0x07e0, 0xa5c3];
                let base = [31u32, 127, 181, 172];
                let weights: [u32; 4] =
                    std::array::from_fn(|k| if mask & (1 << k) != 0 { base[k] } else { 0 });
                let mut narrow = [0u32; 4];
                for k in 0..4 {
                    narrow[map[k]] = weights[k];
                }
                for c in 0..3 {
                    let expand = |v: u16| -> u32 {
                        let q = match c {
                            0 => (v >> 11) as u32,
                            1 => ((v >> 5) & 63) as u32,
                            _ => (v & 31) as u32,
                        };
                        match c {
                            1 => q * 4 + (q >> 4),
                            _ => q * 8 + (q >> 2),
                        }
                    };
                    let old: u32 = (0..4)
                        .map(|k| (weights[k] * expand(physical[map[k]]) + 256) >> 9)
                        .sum();
                    let new: u32 = (0..4)
                        .map(|k| (narrow[k] * expand(physical[k]) + 256) >> 9)
                        .sum();
                    assert_eq!(old, new);
                }
            }
        }
    }
    let slots = vec![Slot {
        base_address: 0,
        has_full_mip: false,
        max_size_log2: 5,
        valid: true,
    }];
    let mut a = CacheEmu::new(slots.clone(), Memory::default(), 200000).unwrap();
    let mut b = CacheEmu::new_bank_order(slots, Memory::default(), 200000).unwrap();
    let mut ca = ColorEmu::new_integer_filter(200000).unwrap();
    let mut cb = ColorEmu::new_integer_filter(200000).unwrap();
    let mut next = 0;
    let mut pairs = Vec::new();
    let mut taken = 0;
    for wall in 0..20000 {
        let ce = wall % 13 != 0;
        let input = if next < 64 {
            Some(
                Group4 {
                    key: TileKey {
                        slot: 0,
                        n: 5,
                        x: 0,
                        y: 0,
                    },
                    top_left_local: [(next % 8) as u8, (next / 8) as u8],
                    coefficients: if next % 3 == 0 {
                        [0, 0, 511, 0]
                    } else {
                        [31, 127, 181, 172]
                    },
                    first: true,
                    last: true,
                    quad_id: (next / 4 % 16) as u8,
                    lane: (next % 4) as u8,
                }
                .pack72()
                .unwrap() as i128,
            )
        } else {
            None
        };
        let t = CacheTick {
            ce,
            input,
            output_ready: wall % 7 != 0,
        };
        let sa = a.tick(t).unwrap();
        let sb = b.tick(t).unwrap();
        assert_eq!(
            (
                sa.accepted,
                sa.transferred,
                sa.request,
                sa.response.accepted,
                sa.response.read,
                sa.response.complete
            ),
            (
                sb.accepted,
                sb.transferred,
                sb.request,
                sb.response.accepted,
                sb.response.read,
                sb.response.complete
            )
        );
        if sa.accepted {
            next += 1
        }
        if sa.transferred {
            let va = sa.output.unwrap();
            let vb = sb.output.unwrap();
            assert_eq!(transformed(va.payload), vb.payload);
            let map = rtl_map(Group4::unpack72(va.payload).unwrap().top_left_local);
            for (k, &mapped) in map.iter().enumerate() {
                assert_eq!(va.texels[k], vb.texels[mapped])
            }
            pairs.push((
                Input {
                    payload: va.payload,
                    texels: va.texels,
                },
                Input {
                    payload: vb.payload,
                    texels: vb.texels,
                },
            ));
            taken += 1;
        }
        if taken == 64 {
            break;
        }
        assert!(wall < 19999, "bounded cache completion");
    }
    let mut accepted = 0;
    let mut committed = 0;
    for wall in 0..20000 {
        let ce = wall % 5 != 0;
        let p = pairs.get(accepted);
        let sa = ca
            .tick(Tick {
                ce,
                input: p.map(|p| p.0),
                output_ready: wall % 7 != 0,
            })
            .unwrap();
        let sb = cb
            .tick(Tick {
                ce,
                input: p.map(|p| p.1),
                output_ready: wall % 7 != 0,
            })
            .unwrap();
        assert_eq!(
            (sa.accepted, sa.output, sa.snapshot),
            (sb.accepted, sb.output, sb.snapshot)
        );
        if sa.accepted {
            accepted += 1
        }
        if ce && wall % 7 != 0 && sa.output.is_some() {
            committed += 1
        }
        if committed == 64 {
            break;
        }
        assert!(wall < 19999, "bounded Color completion");
    }
    assert_eq!(taken, 64);
    assert_eq!(committed, 64);
    println!("PASS all8 parity mappings/all64 local pairs, actual bank CacheEmu cold/hot+CE0, two-edge ColorEmu products/rounding/output credits bitexact pixels={committed}");
}
