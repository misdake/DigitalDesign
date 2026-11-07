//! Real 32-line/two-sector cache and sole weights, checked against memory geometry.
#[allow(dead_code)]
#[path = "../support/vliw_pixel_source.rs"]
mod actual;
#[path = "../support/vliw_sampling_cache.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    sampling::{cache::*, word::Word},
    vertex::Bits,
};
use std::collections::VecDeque;
use vertex_support::rtl::Pin;

fn texel(address: u32) -> u16 {
    let n = address / 2;
    (n.wrapping_mul(0x9e37_79b9).rotate_left(11) ^ (n >> 5) ^ 0xa731) as u16
}
fn weights(a: usize) -> Bits {
    let mut v = Bits::new(0, 36);
    for t in 0..4 {
        v.set(
            t * 9,
            9,
            Bits::new(((a * 71 + t as usize * 113 + 17) % 512) as u64, 9),
        );
    }
    v
}
fn group(n: usize) -> Word {
    // Six colliding tags exceed four ways, interspersed repeated hits and other sets.
    let h = 0x30 + 8 * ((n / 3) % 6) + (if n % 11 == 7 { 3 } else { 0 });
    let x = (n * 7) % 16;
    let y = (n * 3) % 8;
    let mut g = Word::new(h as u128 | ((x as u128) << 15) | ((y as u128) << 19), 35);
    g.set(22, 8, Bits::new(((n & 63) << 2 | 3) as u64, 8));
    g.set(30, 1, Bits::new((n % 2) as u64, 1));
    g.set(31, 4, Bits::new([1, 3, 5, 9, 15][n % 5], 4));
    g
}
fn expected(g: Word) -> (Word, [Bits; 4]) {
    let h = g.slice(0, 15).value as u32;
    let x = g.slice(15, 4).value as u32;
    let y = g.slice(19, 3).value as u32;
    let quad = g.slice(24, 4).value as usize;
    let lane = g.slice(28, 2).value as usize;
    let w = weights(quad * 8 + lane * 2 + g.slice(30, 1).value as usize);
    let mut out = Word::new(0, 72);
    out.set(64, 8, g.slice(22, 8));
    let mut tex = [Bits::default(); 4];
    for dy in 0..2 {
        for dx in 0..2 {
            let tx = (x + dx) % 16;
            let ty = (y + dy) % 8;
            let b = (tx % 2 + 2 * ((ty + tx / 2) % 2)) as usize;
            tex[b] = Bits::new(u64::from(texel(h * 256 + (ty * 16 + tx) * 2)), 16);
            let t = dy * 2 + dx;
            out.set(
                28 + 9 * b as u32,
                9,
                if g.slice(31 + t, 1).value != 0 {
                    w.slice(t * 9, 9)
                } else {
                    Bits::new(0, 9)
                },
            );
        }
    }
    (out, tex)
}
#[derive(Default)]
struct Transfer {
    address: u32,
    index: u8,
    delay: usize,
    terminal: usize,
}
fn run(native: bool) -> Vec<Vec<Pin>> {
    let mut c = if native {
        Cache::native()
    } else {
        Cache::default()
    };
    let mut tape = Vec::new();
    let mut issued = VecDeque::new();
    let mut transfer: Option<Transfer> = None;
    let mut head = 0;
    let mut epoch = 0;
    let mut returned = 0;
    let mut fills = 0;
    let mut hits = 0;
    let mut ce0_capture = 0;
    let mut two_slots = 0;
    let mut first_terminal = 0;
    let mut terminal_gap = 0;
    let mut held = None;
    let mut invalidate_sent = false;
    for wall in 0..12000 {
        let warm = wall == 1207;
        if warm {
            head = 0;
            epoch = 1;
            issued.clear();
            transfer = None;
            held = None;
            invalidate_sent = false;
        }
        let local = if epoch == 0 { wall } else { wall - 1207 };
        let reset = local < 2;
        let mut i = Input {
            reset,
            proposal_ce: wall % 9 != 4 && wall % 13 != 6,
            commit_allow: wall % 17 != 10,
            valid: !reset && local >= 132 && head < 100,
            payload: if head < 100 {
                group(head)
            } else {
                Word::new(0, 35)
            },
            next_valid: head + 1 < 100,
            next_key: if head + 1 < 100 {
                group(head + 1).slice(0, 15).value as u16
            } else {
                0
            },
            out_ready: wall % 31 >= 12,
            mem_req_ready: wall % 7 == 3,
            mem_resp_ok: true,
            ..Input::default()
        };
        if (2..130).contains(&local) {
            i.weight_write = true;
            i.weight_address = (local - 2) as u8;
            i.weight_value = weights(local - 2);
        }
        if epoch == 1
            && head == 63
            && transfer.is_none()
            && !c.desc_valid
            && !c.rd_valid
            && !c.res_valid
            && !invalidate_sent
        {
            i.texture_invalidate = true;
            invalidate_sent = true;
            i.valid = false;
        }
        if let Some(t) = transfer.as_mut() {
            if t.delay > 0 {
                t.delay -= 1;
            } else if t.index < 16 && wall % 5 != 1 {
                i.mem_resp_valid = true;
                i.mem_resp_index = t.index;
                i.mem_resp_data = (0..4).fold(0, |v, w| {
                    v | (u64::from(texel(t.address + u32::from(t.index) * 8 + w * 2)) << (w * 16))
                });
                t.index += 1;
                if t.index == 16 {
                    t.terminal = 3;
                }
            } else if t.index == 16 {
                if t.terminal > 0 {
                    t.terminal -= 1;
                    terminal_gap += 1;
                } else {
                    i.mem_resp_complete = true;
                }
            }
        }
        let o = c.outputs(i);
        if o.mem_req_valid {
            if let Some(a) = held {
                assert_eq!(a, o.mem_req_addr);
            }
            held = Some(o.mem_req_addr);
            if i.mem_req_ready {
                assert!(transfer.is_none());
                transfer = Some(Transfer {
                    address: o.mem_req_addr,
                    delay: 2,
                    ..Transfer::default()
                });
                held = None;
                fills += 1;
            }
        }
        if i.mem_resp_complete {
            if !c.desc_sector {
                first_terminal += 1;
                assert!(!o.drained);
            }
            transfer = None;
        }
        if o.read_issue {
            issued.push_back(group(head));
            head += 1;
            hits += 1;
        }
        if o.valid {
            let g = *issued.front().unwrap();
            let (p, t) = expected(g);
            assert_eq!(o.payload, p, "payload wall{wall}");
            assert_eq!(o.texels, t, "DO wall{wall}");
            if o.result_pop {
                issued.pop_front();
                returned += 1;
            }
        }
        ce0_capture += usize::from(o.return_capture && !o.advance);
        two_slots += usize::from(c.rd_valid && c.res_valid);
        tape.push(fixture::fields(i, o, &c));
        c.clock(i).unwrap();
        if epoch == 1
            && head == 100
            && issued.is_empty()
            && !c.rd_valid
            && !c.res_valid
            && o.drained
        {
            assert!(
                fills > 60
                    && hits > 100
                    && returned > 100
                    && ce0_capture > 0
                    && two_slots > 0
                    && first_terminal > 25
                    && terminal_gap > 100
            );
            assert!(invalidate_sent);
            println!("CACHE32 native={native} walls={} requests={fills} hits={hits} returns={returned} ce0_capture={ce0_capture} two_slots={two_slots}",tape.len());
            return tape;
        }
    }
    panic!(
        "cache bound head={head} epoch={epoch} issued={} desc={}",
        issued.len(),
        c.desc_valid
    );
}
#[test]
fn cache_returns_independent_physical_texels_and_sole_weights() {
    run(false);
    run(true);
}
fn root(label: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
#[ignore = "actual portable/native32-line cache and sole weight bank"]
fn paid_pin_and_wall_returns_match_actual_cache() {
    let root = root("cache32");
    println!("CACHE32_ROOT {}", root.display());
    for native in [false, true] {
        let tape = run(native);
        let source = fixture::production(&tape[0], native);
        vertex_support::rtl::replay(
            &root,
            if native { "native" } else { "portable" },
            tape,
            source,
            false,
            None,
        );
    }
}
fn abort_tape(case: usize) -> Vec<Vec<Pin>> {
    let mut c = Cache::default();
    let mut tape = Vec::new();
    let mut transfer: Option<Transfer> = None;
    let mut stop = None;
    let mut requests = 0;
    let mut writes = 0;
    let mut written = [None; 32];
    for wall in 0..2000 {
        let trigger = match case {
            0 => c.desc_valid && !c.desc_presented,
            1 => c.desc_presented && !c.desc_started,
            2 => c.desc_started && !c.desc_sector && c.desc_next == 5,
            3 => c.desc_valid && c.desc_sector && !c.desc_presented,
            4 => c.desc_started && c.desc_sector && c.desc_next == 5,
            _ => unreachable!(),
        };
        if stop.is_none() && trigger {
            stop = Some(wall);
        }
        let mut i = Input {
            reset: wall < 2,
            proposal_ce: stop.is_none(),
            commit_allow: true,
            abort: stop.is_some(),
            valid: wall >= 2,
            payload: group(0),
            mem_req_ready: case != 1 || stop.is_some_and(|at| wall >= at + 3),
            mem_resp_ok: true,
            ..Input::default()
        };
        if let Some(t) = transfer.as_mut() {
            if t.index < 16 && wall % 4 != 2 {
                i.mem_resp_valid = true;
                i.mem_resp_index = t.index;
                i.mem_resp_data = (0..4).fold(0, |v, w| {
                    v | (u64::from(texel(t.address + u32::from(t.index) * 8 + w * 2)) << (w * 16))
                });
                t.index += 1;
            } else if t.index == 16 {
                i.mem_resp_complete = true;
            }
        }
        let o = c.outputs(i);
        if o.mem_req_valid && i.mem_req_ready {
            assert!(transfer.is_none());
            transfer = Some(Transfer {
                address: o.mem_req_addr,
                ..Transfer::default()
            });
            requests += 1;
        }
        if o.data_write {
            let row = usize::from(i.mem_resp_index) + usize::from(c.desc_sector) * 16;
            written[row] = Some(std::array::from_fn::<_, 4, _>(|b| {
                texel(0x3000 + row as u32 * 8 + (b ^ (if row & 4 != 0 { 2 } else { 0 })) as u32 * 2)
            }));
            writes += 1;
        }
        if i.mem_resp_complete {
            transfer = None;
        }
        tape.push(fixture::fields(i, o, &c));
        c.clock(i).unwrap();
        if let Some(at) = stop {
            if wall > at + 2 && o.drained && transfer.is_none() {
                assert!(c.fault);
                assert_eq!(requests, [0, 1, 1, 1, 2][case]);
                assert_eq!(writes, [0, 0, 5, 16, 21][case]);
                for (r, expected) in written.iter().enumerate() {
                    for b in 0..4 {
                        assert_eq!(
                            c.banks[b][r],
                            expected.map_or_else(Bits::default, |v| Bits::new(u64::from(v[b]), 16))
                        );
                    }
                }
                println!(
                    "CACHE32_ABORT case={case} walls={} requests={requests} writes={writes}",
                    tape.len()
                );
                return tape;
            }
        }
    }
    panic!("abort bound case={case}");
}
#[test]
fn abort_drains_only_presented_children_and_retains_written_prefix() {
    for case in 0..5 {
        abort_tape(case);
    }
}
#[test]
#[ignore = "five bounded original cache descriptor stop phases, CE0 wall drain"]
fn ce0_abort_phases_match_actual_cache() {
    let root = root("cache32-abort");
    println!("CACHE32_ABORT_ROOT {}", root.display());
    for case in 0..5 {
        let tape = abort_tape(case);
        let source = fixture::production(&tape[0], false);
        vertex_support::rtl::replay(&root, &format!("phase{case}"), tape, source, false, None);
    }
}
fn monitor_tape(case: usize) -> (Vec<Vec<Pin>>, &'static str) {
    let mut c = Cache::default();
    let mut tape = Vec::new();
    let rule = [
        "GPU contract: texture return framing",
        "GPU contract: mem_resp_ok",
        "Group sector changed before its descriptor drained",
    ][case];
    for wall in 0..200 {
        let mut i = Input {
            reset: wall < 2,
            proposal_ce: true,
            commit_allow: true,
            valid: wall >= 2,
            payload: group(0),
            mem_req_ready: true,
            mem_resp_ok: true,
            ..Input::default()
        };
        if c.desc_started {
            if c.desc_next < 16 {
                i.mem_resp_valid = true;
                i.mem_resp_index = c.desc_next;
            } else {
                i.mem_resp_complete = true;
            }
        }
        let trigger = match case {
            0 => c.desc_started && c.desc_next == 0,
            1 => i.mem_resp_complete,
            2 => c.desc_started && c.desc_next == 3,
            _ => unreachable!(),
        };
        if trigger {
            match case {
                0 => i.mem_resp_index = 1,
                1 => i.mem_resp_ok = false,
                2 => i.payload.set(0, 15, Bits::new(0x38, 15)),
                _ => unreachable!(),
            }
        }
        tape.push(fixture::fields(i, c.outputs(i), &c));
        let result = c.clock(i);
        if trigger {
            assert_eq!(result.unwrap_err().rule, rule);
            return (tape, rule);
        }
        result.unwrap();
    }
    panic!("monitor bound case={case}");
}
#[test]
#[ignore = "four actual cache mutations and three exact original monitors"]
fn paid_capture_banks_and_parent_terminal_detect_counterexamples() {
    let root = root("cache32-negative");
    println!("CACHE32_NEGATIVE_ROOT {}", root.display());
    let tape = run(false);
    let source = fixture::production(&tape[0], false);
    for (name, old, new) in [
        (
            "wall_capture",
            "wire return_capture=rd_valid&&(!res_valid||result_pop);",
            "wire return_capture=advance&&rd_valid&&(!res_valid||result_pop);",
        ),
        (
            "weight_owner_address",
            "wire[6:0]weight_ra={head_payload[27:24],head_payload[29:28],head_payload[30]};",
            "wire[6:0]weight_ra={head_payload[29:28],head_payload[27:24],head_payload[30]};",
        ),
        (
            "physical_bank_swizzle",
            "assign data_di[0]=mem_resp_index[2]?mem_resp_data[32+:16]:mem_resp_data[0+:16];",
            "assign data_di[0]=mem_resp_data[0+:16];",
        ),
    ] {
        assert_eq!(source.matches(old).count(), 1, "{name}");
        vertex_support::rtl::replay(
            &root,
            name,
            tape.iter()
                .map(|row| {
                    row.iter()
                        .map(|p| Pin {
                            name: p.name.clone(),
                            width: p.width,
                            value: p.value,
                            known: p.known,
                            output: p.output,
                        })
                        .collect()
                })
                .collect(),
            source.replacen(old, new, 1),
            true,
            None,
        );
    }
    let first = "wire publish=terminal&&desc_sector&&mem_resp_ok&&!reset&&!fault_eff&&term_ok;";
    let second = "(terminal&&(desc_sector||!term_ok||!mem_resp_ok))";
    assert_eq!(source.matches(first).count(), 1);
    assert_eq!(source.matches(second).count(), 1);
    let early = source
        .replacen(
            first,
            "wire publish=terminal&&mem_resp_ok&&!reset&&!fault_eff&&term_ok;",
            1,
        )
        .replacen(second, "terminal", 1);
    vertex_support::rtl::replay(&root, "first_sector_parent_ready", tape, early, true, None);
    for case in 0..3 {
        let (t, rule) = monitor_tape(case);
        vertex_support::rtl::replay(
            &root,
            &format!("monitor{case}"),
            t,
            source.clone(),
            false,
            Some(rule),
        );
    }
}
fn pin_tape() -> Vec<Vec<Pin>> {
    let mut c = Cache::default();
    let mut tape = Vec::new();
    let mut transfer: Option<Transfer> = None;
    let mut head = 0;
    let mut pending = VecDeque::new();
    let mut invalidated = false;
    let mut full_lines = 0;
    let mut protected = false;
    for wall in 0..500 {
        let mut g = group(0);
        if head == 2 {
            g.set(0, 15, Bits::new(0x38, 15));
        }
        let invalidate = head == 2 && c.rd_valid && c.res_valid && !c.desc_valid && !invalidated;
        let mut i = Input {
            reset: wall < 2,
            proposal_ce: !invalidate,
            commit_allow: true,
            valid: wall >= 2 && head < 3,
            payload: g,
            texture_invalidate: invalidate,
            next_valid: head < 2,
            next_key: if head == 1 { 0x38 } else { 0x30 },
            out_ready: full_lines >= 2,
            mem_req_ready: true,
            mem_resp_ok: true,
            ..Input::default()
        };
        if invalidate {
            invalidated = true;
        }
        if let Some(t) = transfer.as_mut() {
            if t.index < 16 {
                i.mem_resp_valid = true;
                i.mem_resp_index = t.index;
                i.mem_resp_data = (0..4).fold(0, |v, w| {
                    v | (u64::from(texel(t.address + u32::from(t.index) * 8 + w * 2)) << (w * 16))
                });
                t.index += 1;
            } else {
                i.mem_resp_complete = true;
            }
        }
        let o = c.outputs(i);
        if o.mem_req_valid && i.mem_req_ready {
            assert!(transfer.is_none());
            transfer = Some(Transfer {
                address: o.mem_req_addr,
                ..Transfer::default()
            });
        }
        if i.mem_resp_complete {
            transfer = None;
            if c.desc_sector {
                full_lines += 1;
            }
        }
        if invalidated && o.tag_write && c.rd_valid {
            assert_eq!(c.pin_line.value, 0);
            assert_eq!(o.victim_line, 1);
            protected = true;
        }
        if o.read_issue {
            pending.push_back(g);
            head += 1;
        }
        if o.valid {
            assert_eq!(o.texels, expected(*pending.front().unwrap()).1);
            if o.result_pop {
                pending.pop_front();
            }
        }
        tape.push(fixture::fields(i, o, &c));
        c.clock(i).unwrap();
        if head == 3 && pending.is_empty() && !c.rd_valid && !c.res_valid {
            assert!(protected);
            println!("CACHE32_PIN walls={}", tape.len());
            return tape;
        }
    }
    panic!("pin bound head={head} lines={full_lines}");
}
#[test]
fn invalidation_does_not_evict_a_paid_pin() {
    pin_tape();
}
#[test]
#[ignore = "paid pin excludes a newly invalidated victim, plus actual mutation"]
fn paid_pin_survives_invalidation_in_actual_cache() {
    let root = root("cache32-pin");
    println!("CACHE32_PIN_ROOT {}", root.display());
    let tape = pin_tape();
    let source = fixture::production(&tape[0], false);
    let old = "!(pin_valid && (pin_line == head_set*4+wv))";
    assert_eq!(source.matches(old).count(), 1);
    let mutant = source.replacen(old, "1'b1", 1);
    let mut copy = Vec::new();
    for row in &tape {
        copy.push(
            row.iter()
                .map(|p| Pin {
                    name: p.name.clone(),
                    width: p.width,
                    value: p.value,
                    known: p.known,
                    output: p.output,
                })
                .collect(),
        );
    }
    vertex_support::rtl::replay(&root, "original", copy, source, false, None);
    vertex_support::rtl::replay(&root, "evict_paid_pin", tape, mutant, true, None);
}
