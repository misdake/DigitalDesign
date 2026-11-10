//! Complete selected sampler; independent coordinate, padded layout and pixel sums.
use gpu_v2::system::vliw_cycle::{postquad::Profile, sampling::pipeline::*, vertex::Bits};
use std::collections::VecDeque;
#[derive(Clone, Copy)]
struct Pixel {
    slot: u8,
    lane: u8,
    mask: u8,
    lod: u8,
    uv: u32,
    first: bool,
}
fn texel(address: u32) -> u16 {
    let n = address / 2;
    (n.wrapping_mul(0x9e37_79b9).rotate_left(11) ^ (n >> 5) ^ 0xa731) as u16
}
fn backing_texel(address: u32, c: Config) -> u16 {
    if c.framebuffer {
        return texel(address);
    }
    let base = u32::from(c.base) * 128;
    let mut prefix = 0;
    for level in 0..=c.n {
        if !c.mip && level != c.n {
            continue;
        }
        let side = 1u32 << level;
        let macros = side.div_ceil(16);
        let bytes = macros * macros * 512;
        if (base + prefix..base + prefix + bytes).contains(&address) {
            let offset = address - base - prefix;
            let tile = offset / 512;
            let x = (tile / macros * 16 + offset % 32 / 2) % side;
            let y = (tile % macros * 16 + offset % 512 / 32) % side;
            let canonical =
                base + prefix + ((x / 16) * macros + y / 16) * 512 + (y % 16) * 32 + (x % 16) * 2;
            return texel(canonical);
        }
        prefix += bytes;
    }
    panic!("outside texture allocation {address:x} {c:?}");
}
fn reference(p: Pixel, c: Config) -> u32 {
    let n = if c.mip {
        c.n.saturating_sub(p.lod / 8)
    } else {
        c.n
    };
    let f = if c.mip && p.lod / 8 < c.n && c.filter == 2 {
        p.lod % 8
    } else {
        0
    };
    let lambda = (u32::from(f) * 511 + 4) / 8;
    let mut origin = [0i64; 2];
    let mut fraction = [0u32; 2];
    for a in 0..2 {
        let side = if c.framebuffer {
            [400, 240][a]
        } else {
            1u32 << if c.clamp { n } else { n.max(1) }
        };
        let uv = (p.uv >> (16 * a)) as u16;
        let q = i64::from(u32::from(uv) * side / 256) - if c.filter == 0 { 0 } else { 128 };
        let q = if c.framebuffer || c.clamp {
            q.clamp(0, i64::from((side - 1) * 256))
        } else {
            q
        };
        origin[a] = q.div_euclid(256).rem_euclid(i64::from(side));
        fraction[a] = q.rem_euclid(256) as u32;
    }
    let mut sums = [0u32; 3];
    let mut alpha_sum = 0u32;
    for plane in 0..2 {
        let parent = if plane == 0 { 511 - lambda } else { lambda };
        if parent == 0 {
            continue;
        }
        let level = n.saturating_sub(plane);
        let side = 1i64 << if c.clamp { level } else { level.max(1) };
        let mut xy = origin;
        let mut uv = if c.filter == 0 { [0, 0] } else { fraction };
        if c.clamp {
            for a in 0..2 {
                let raw = i64::from((p.uv >> (16 * a)) as u16);
                let q = raw * side / 256 - if c.filter == 0 { 0 } else { 128 };
                let q = q.clamp(0, (side - 1) * 256);
                xy[a] = q / 256;
                uv[a] = if c.filter == 0 { 0 } else { (q % 256) as u32 };
            }
        } else if plane == 1 && n > 1 {
            for a in 0..2 {
                let q = (origin[a] * 256 + i64::from(fraction[a]) + 128) / 2 - 128;
                xy[a] = q.div_euclid(256).rem_euclid(side);
                uv[a] = q.rem_euclid(256) as u32;
            }
        }
        let bottom = parent * uv[1] / 256;
        let top = parent - bottom;
        let br = bottom * uv[0] / 256;
        let tr = top * uv[0] / 256;
        let weights = [top - tr, tr, bottom - br, br];
        for (t, &weight) in weights.iter().enumerate() {
            if weight == 0 {
                continue;
            }
            let x = (xy[0] + (t % 2) as i64).rem_euclid(side) as u32;
            let y = (xy[1] + (t / 2) as i64).rem_euclid(side) as u32;
            let prefix = if c.mip {
                (0..level)
                    .map(|l| 2 * (1u32 << l).div_ceil(16).pow(2))
                    .sum()
            } else {
                0
            };
            let pitch = if c.framebuffer {
                16
            } else {
                (1u32 << level).div_ceil(16)
            };
            let line =
                u32::from(c.base) / 2 + prefix + 2 * ((x / 16) * pitch + y / 16) + (y / 8) % 2;
            let raw = backing_texel(line * 256 + ((y % 8) * 16 + x % 16) * 2, c);
            let r = u32::from(raw >> 11);
            let g = u32::from(raw >> 5 & 63);
            let b = u32::from(raw & 31);
            let rgb = if c.rgba4444 {
                std::array::from_fn(|channel| u32::from((raw >> (12 - channel * 4)) & 15) * 17)
            } else {
                [(r << 3 | r >> 2), (g << 2 | g >> 4), (b << 3 | b >> 2)]
            };
            alpha_sum += weight * u32::from(raw & 15);
            for (c, v) in rgb.into_iter().enumerate() {
                sums[c] += (weight * v + 256) / 512;
            }
        }
    }
    let alpha = if c.rgba4444 {
        (alpha_sum * 17 + 255) / 511
    } else {
        255
    };
    (sums[0] & 255) | ((sums[1] & 255) << 8) | ((sums[2] & 255) << 16) | (alpha << 24)
}
fn pixels() -> Vec<Pixel> {
    let mut all = Vec::new();
    for q in 0..40usize {
        let mask = [1u8, 5, 10, 15][q % 4];
        let mut first = true;
        for lane in 0..4 {
            if mask & (1 << lane) == 0 {
                continue;
            }
            let u = [0x3f00u16, 0, 0xffff, 0x12cd, 0x7fff][(q + lane) % 5];
            let v = [0x1f00u16, 0x81ff, 0xfffe, 0x7723][(q + lane) % 4];
            all.push(Pixel {
                slot: (q % 32) as u8,
                lane: lane as u8,
                mask,
                lod: [0, 1, 4, 5, 7, 8, 15, 31, 47, 48][q % 10],
                uv: u32::from(u) | (u32::from(v) << 16),
                first,
            });
            first = false;
        }
    }
    all
}
#[derive(Default)]
struct Transfer {
    address: u32,
    next: u8,
    terminal: usize,
}
fn run(config: Config, profile: Profile) -> (usize, usize, usize) {
    let mut s = Sampler::new(profile);
    let source = pixels();
    let mut index = 0;
    let mut loan = false;
    let mut returns = VecDeque::new();
    let mut transfer: Option<Transfer> = None;
    let mut count = 0;
    let mut groups = 0;
    let mut requests = 0;
    let mut full = 0;
    for wall in 0..50000 {
        let p = source[index.min(source.len() - 1)];
        let mut data = Bits::new(0, 36);
        data.set(0, 16, Bits::new(u64::from(p.uv as u16), 16));
        data.set(18, 16, Bits::new(u64::from((p.uv >> 16) as u16), 16));
        let ce = wall % 7 != 3;
        let allow = wall % 11 != 4;
        let mut i = Input {
            reset: wall < 2,
            ce,
            commit_allow: allow,
            source_valid: wall >= 2 && index < source.len() && !loan,
            source_descriptor: u32::from(p.slot)
                | (u32::from(p.mask) << 5)
                | (u32::from(p.first) << 10)
                | (u32::from(p.lod) << 11),
            source_row: p.lane,
            source_data: data,
            config,
            packet_enable: wall % 17 != 6,
            result_ready: ce && allow,
            memory: Memory {
                request_ready: wall % 5 == 1,
                ok: true,
                ..Default::default()
            },
            ..Input::default()
        };
        if let Some(t) = transfer.as_mut() {
            if t.next < 16 && wall % 5 != 2 {
                i.memory.read = true;
                i.memory.index = t.next;
                i.memory.data = (0..4).fold(0, |v, w| {
                    v | (u64::from(backing_texel(
                        t.address + u32::from(t.next) * 8 + w * 2,
                        config,
                    )) << (w * 16))
                });
                t.next += 1;
                if t.next == 16 {
                    t.terminal = 2;
                }
            } else if t.next == 16 {
                if t.terminal > 0 {
                    t.terminal -= 1;
                } else {
                    i.memory.complete = true;
                }
            }
        }
        let o = s.outputs(i);
        if o.cache.mem_req_valid && i.memory.request_ready {
            assert!(transfer.is_none());
            transfer = Some(Transfer {
                address: o.cache.mem_req_addr,
                ..Default::default()
            });
            requests += 1;
        }
        if i.memory.complete {
            transfer = None;
        }
        if o.source_fire {
            returns.push_back((p, reference(p, config)));
        }
        if o.result.valid {
            let (wanted, rgb) = *returns.front().unwrap();
            assert_eq!(o.result.key, (wanted.slot & 15) * 4 + wanted.lane);
            assert_eq!(
                o.result.rgb,
                Bits::new(u64::from(rgb & 0xffffff), 24),
                "wall{wall} slot{} lane{} lod{} config{config:?}",
                wanted.slot,
                wanted.lane,
                wanted.lod
            );
            assert_eq!(
                o.result.alpha,
                Bits::new(u64::from(rgb >> 24), 8),
                "filtered alpha wall{wall}"
            );
            if o.color.result_fire {
                returns.pop_front();
                count += 1;
            }
        }
        groups += usize::from(o.cache.read_issue);
        full += usize::from(s.reserved == 16);
        s.clock(i)
            .unwrap_or_else(|e| panic!("wall{wall} source{index}: {e}"));
        if !i.reset && ce && allow {
            if loan {
                loan = false;
                index += 1;
            } else if o.source_fire {
                loan = true;
            }
        }
        if index == source.len() && returns.is_empty() && o.drained && s.color.live == 0 {
            assert_eq!(count, source.len());
            assert!(groups >= count && requests > 0);
            println!("SAMPLER profile={profile:?} config={config:?} walls={} pixels={count} groups={groups} sectors={requests} Work16_full={full}",wall+1);
            return (wall + 1, requests, groups);
        }
    }
    panic!(
        "sampler bound index={index} result={count} reserved={} group={}",
        s.reserved, s.packets.group.used
    );
}
#[test]
fn complete_sampler_matches_independent_integer_pixels() {
    for profile in [Profile::Portable, Profile::Native] {
        for filter in 0..3 {
            run(
                Config {
                    base: 0x200,
                    n: 6,
                    mip: true,
                    rgba4444: false,
                    clamp: false,
                    filter,
                    framebuffer: false,
                },
                profile,
            );
        }
        run(
            Config {
                base: 0x400,
                n: 9,
                mip: false,
                rgba4444: false,
                clamp: false,
                filter: 1,
                framebuffer: true,
            },
            profile,
        );
    }
}

#[test]
fn rgba4444_sampler_keeps_cache_and_calendar_with_and_without_mips() {
    for profile in [Profile::Portable, Profile::Native] {
        for mip in [false, true] {
            for filter in 0..3 {
                let original = Config {
                    base: 0x200,
                    n: 6,
                    mip,
                    rgba4444: false,
                    clamp: false,
                    filter,
                    framebuffer: false,
                };
                let baseline = run(original, profile);
                let candidate = run(
                    Config {
                        rgba4444: true,
                        ..original
                    },
                    profile,
                );
                assert_eq!(
                    baseline, candidate,
                    "RGBA4444 added cycles or cache traffic"
                );
            }
        }
    }
}

#[test]
fn clamp_sampler_matches_independent_per_mip_coordinates() {
    for profile in [Profile::Portable, Profile::Native] {
        for n in [0, 1, 2, 6] {
            for filter in 0..3 {
                run(
                    Config {
                        base: 0x200,
                        n,
                        mip: true,
                        rgba4444: true,
                        clamp: true,
                        filter,
                        framebuffer: false,
                    },
                    profile,
                );
            }
        }
    }
}

#[test]
fn clamp_uses_signed_source_uv_before_wrapping() {
    for profile in [Profile::Portable, Profile::Native] {
        let mut sampler = Sampler::new(profile);
        let config = Config {
            n: 6,
            filter: 1,
            clamp: true,
            ..Default::default()
        };
        let data = Bits::new(0x3ffff | (65536u64 << 18), 36);
        let mut seen = false;
        for edge in 0..8 {
            let input = Input {
                reset: edge == 0,
                ce: true,
                commit_allow: true,
                source_valid: edge == 1,
                source_descriptor: (1 << 10) | (1 << 5),
                source_data: data,
                config,
                packet_enable: true,
                result_ready: true,
                ..Default::default()
            };
            let output = sampler.outputs(input);
            if output.direct.valid {
                assert_eq!(output.direct.result.value, 63u64 << 26);
                seen = true;
            }
            sampler.clock(input).unwrap();
        }
        assert!(seen);
    }
}
