//! Bounded field pipeline, independently computed with integer operations. Input
//! words/issue opportunities come from the audited native cache/MC trace. This
//! is a retimed storage study, not a replacement cache controller.
use super::*;
#[derive(Clone, Copy)]
struct Owned<T> {
    serial: usize,
    data: T,
}
struct Input {
    program: usize,
    issue: u64,
    payload: i128,
    words: [u16; 4],
    key: usize,
    first: bool,
    last: bool,
    expected_partial: [u32; 3],
    expected_sum: [u32; 3],
    expected_rgb: Option<[u8; 3]>,
}
fn inputs(r: &bound::system::Report) -> Vec<Input> {
    let mut source = VecDeque::new();
    let mut waiting = VecDeque::new();
    let mut partial = 0;
    let mut acc = 0;
    let mut last = VecDeque::new();
    let mut xs = vec![];
    let mut t = 0;
    for (p, c) in r.preparation.iter().zip(&r.cache.steps) {
        for e in &p.events {
            if let bound::control::Event::Packet { program, payload } = *e {
                source.push_back((program, payload));
            }
        }
        for e in &c.events {
            match e {
                timed::Event::Read { group, payload, .. } => {
                    let (program, word) = source.pop_front().unwrap();
                    assert_eq!(word, *payload);
                    waiting.push_back(xs.len());
                    xs.push(Input {
                        program,
                        issue: t,
                        payload: *payload,
                        words: [0; 4],
                        key: usize::from(group.quad_id) * 4 + usize::from(group.lane),
                        first: group.first,
                        last: group.last,
                        expected_partial: [0; 3],
                        expected_sum: [0; 3],
                        expected_rgb: None,
                    });
                }
                timed::Event::Captured { words, .. } => {
                    xs[waiting.pop_front().unwrap()].words = *words
                }
                timed::Event::Partial { value, .. } => {
                    xs[partial].expected_partial = *value;
                    partial += 1;
                }
                timed::Event::Accumulate { value, .. } => {
                    xs[acc].expected_sum = *value;
                    if xs[acc].last {
                        last.push_back(acc);
                    }
                    acc += 1;
                }
                timed::Event::Result { pixel } => {
                    xs[last.pop_front().unwrap()].expected_rgb = Some(pixel.rgb)
                }
                _ => {}
            }
        }
        if c.control.ce {
            t += 1;
        }
    }
    assert_eq!((partial, acc), (xs.len(), xs.len()));
    assert!(source.is_empty() && waiting.is_empty() && last.is_empty());
    xs
}
fn take<T: Copy>(x: Option<Owned<T>>, serial: usize) -> Result<T, String> {
    let x = x.ok_or("color missing field")?;
    if x.serial != serial {
        return Err("color stale field owner".into());
    }
    Ok(x.data)
}
fn expanded(word: u16) -> [u32; 3] {
    let r = u32::from(word >> 11);
    let g = u32::from((word >> 5) & 63);
    let b = u32::from(word & 31);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}
#[derive(Default)]
struct Stats {
    wall: u64,
    enabled: u64,
    writes: usize,
    done: usize,
    published: usize,
    peak_slots: usize,
    stalls: u64,
}
fn run(
    r: &bound::system::Report,
    stress: bool,
    bad: bool,
    dense: bool,
    out: &mut impl Write,
) -> Result<Stats, String> {
    let mut xs = inputs(r);
    if dense {
        for (serial, i) in xs.iter_mut().enumerate() {
            i.issue = serial as u64;
        }
    }
    let mut words = None::<Owned<[u16; 4]>>;
    let mut weights = [None::<Owned<[u32; 4]>>; 2];
    let mut dsp = [None::<Owned<[[u32; 3]; 4]>>; 3];
    let mut tree = None::<Owned<[[u32; 3]; 2]>>;
    let mut partial = None::<Owned<[u32; 3]>>;
    let mut feedback = None::<Owned<[u32; 3]>>;
    let mut feedback_key = None;
    let mut fold = None::<Owned<[(u16, u8, bool); 3]>>;
    let mut increment = None::<Owned<[(u8, bool); 3]>>;
    let mut rgb = None::<Owned<[u8; 3]>>;
    let mut key_pipe = [None::<Owned<usize>>; 11];
    let mut first_pipe = [None::<Owned<bool>>; 7];
    let mut last_pipe = [None::<Owned<bool>>; 11];
    let mut jobs = Vec::<(usize, u64)>::new(); // Checker serial/age; hardware tags are above.
    let mut store = [None::<Owned<[u8; 3]>>; 64];
    let mut published = [false; 64];
    let mut quad = [None::<(usize, u8)>; 16]; // Existing global owner/remaining mask, outside sampling.
    let mut done = VecDeque::<(usize, usize, u64)>::new(); // External final's completion metadata.
    let mut reply = None::<(usize, usize, [u8; 3])>;
    let mut s = Stats::default();
    let mut next = 0;
    let mut injected = false;
    while next < xs.len() || !jobs.is_empty() || !done.is_empty() || reply.is_some() {
        if s.wall > 200_000 {
            return Err("color storage watchdog".into());
        }
        s.wall += 1;
        if stress && s.wall % 23 <= 7 {
            continue;
        }
        let t = s.enabled;
        s.enabled += 1;
        let mut write_row = None;
        let mut read_row = None;
        if let Some((serial, key, value)) = reply.take() {
            let i = &xs[serial];
            if value != i.expected_rgb.unwrap() || !published[key] {
                return Err("public result R ownership/value".into());
            }
            store[key] = None;
            published[key] = false;
            let q = quad[key / 4].as_mut().ok_or("public result unowned quad")?;
            if q.0 != i.program || q.1 & (1 << (key % 4)) == 0 {
                return Err("global output release owner".into());
            }
            q.1 &= !(1 << (key % 4));
            if q.1 == 0 {
                quad[key / 4] = None;
            }
            s.published += 1;
            writeln!(out, "{stress},{t},final captured,{serial},{key}").unwrap();
        }
        if (!stress || (t > 500 && t % 13 > 3))
            && done.front().is_some_and(|&(_, _, edge)| edge < t)
        {
            let (serial, key, _) = done.pop_front().unwrap();
            let word = store[key].ok_or("final read before actual result W")?;
            if word.serial != serial {
                return Err("public result stale row".into());
            }
            reply = Some((serial, key, word.data));
            read_row = Some(key);
            writeln!(out, "{stress},{t},final R,{serial},{key}").unwrap();
        }
        // Reads of old FF values precede same-edge writes from younger tokens.
        // Each register below is a distinct field bank; there is no hidden RAM
        // read port or whole252-bit record kept through the color stages.
        for &(serial, start) in &jobs {
            let age = t - start;
            let i = &xs[serial];
            if (1..=11).contains(&age) && take(key_pipe[age as usize - 1], serial)? != i.key {
                return Err("color key pipeline".into());
            }
            match age {
                12 if i.last => {
                    let v = store[i.key].ok_or("done before store")?;
                    if v.serial != serial {
                        return Err("done wrong stored owner".into());
                    }
                    s.done += 1;
                    done.push_back((serial, i.key, t));
                    writeln!(out, "{stress},{t},done,{serial},{}", i.key).unwrap();
                }
                11 if i.last => {
                    let v = take(rgb, serial)?;
                    if store[i.key].is_some() {
                        return Err("public result overwrite before output capture".into());
                    }
                    store[i.key] = Some(Owned { serial, data: v });
                    published[i.key] = true;
                    write_row = Some(i.key);
                    s.writes += 1;
                    writeln!(out, "{stress},{t},result W,{serial},{}", i.key).unwrap();
                }
                10 if i.last => {
                    let v = take(increment, serial)?;
                    let v = v.map(|(h, inc)| u8::try_from(u16::from(h) + u16::from(inc)).unwrap());
                    if Some(v) != i.expected_rgb {
                        return Err("color integer normalize vs closed golden".into());
                    }
                    rgb = Some(Owned { serial, data: v });
                }
                9 if i.last => {
                    let v = take(fold, serial)?;
                    increment = Some(Owned {
                        serial,
                        data: v.map(|(sum, h, hi)| (h, hi || sum >> 8 != 0)),
                    });
                }
                8 if i.last => {
                    let v = take(feedback, serial)?;
                    fold = Some(Owned {
                        serial,
                        data: v.map(|s| {
                            let h = (s >> 9) as u8;
                            ((u16::from(h) + (s & 255) as u16), h, s & 256 != 0)
                        }),
                    });
                }
                7 => {
                    let p = take(partial, serial)?;
                    let first = take(first_pipe[6], serial)?;
                    let old = if first {
                        if feedback_key.is_some() {
                            return Err("color first while feedback owned".into());
                        }
                        [0; 3]
                    } else {
                        if feedback_key != Some(i.key) {
                            return Err("color feedback sample owner".into());
                        }
                        feedback.ok_or("missing feedback")?.data
                    };
                    let v = std::array::from_fn(|c| old[c] + p[c]);
                    if v != i.expected_sum || v.iter().any(|&x| x > 130305) {
                        return Err("color accumulated integer bound/golden".into());
                    }
                    feedback = Some(Owned { serial, data: v });
                    feedback_key = if i.last { None } else { Some(i.key) };
                }
                6 => {
                    let v = take(tree, serial)?;
                    let v = std::array::from_fn(|c| v[0][c] + v[1][c]);
                    if v != i.expected_partial {
                        return Err("color integer partial vs closed golden".into());
                    }
                    partial = Some(Owned { serial, data: v });
                }
                5 => {
                    let v = take(dsp[(start + 2) as usize % 3], serial)?;
                    tree = Some(Owned {
                        serial,
                        data: std::array::from_fn(|pair| {
                            std::array::from_fn(|c| v[pair * 2][c] + v[pair * 2 + 1][c])
                        }),
                    });
                }
                2 => {
                    let w = take(words, serial)?;
                    let k = take(weights[start as usize % 2], serial)?;
                    dsp[(t as usize) % 3] = Some(Owned {
                        serial,
                        data: std::array::from_fn(|tap| expanded(w[tap]).map(|c| c * k[tap])),
                    });
                }
                1 => {
                    words = Some(Owned {
                        serial,
                        data: i.words,
                    });
                    if bad && !injected {
                        words.as_mut().unwrap().serial = serial + 1;
                        injected = true;
                    }
                }
                _ => {}
            }
            if (1..=11).contains(&age) && take(last_pipe[age as usize - 1], serial)? != i.last {
                return Err("color last pipeline".into());
            }
        }
        jobs.retain(|&(serial, start)| t - start < if xs[serial].last { 12 } else { 7 });
        key_pipe.copy_within(0..10, 1);
        key_pipe[0] = None;
        last_pipe.copy_within(0..10, 1);
        last_pipe[0] = None;
        first_pipe.copy_within(0..6, 1);
        first_pipe[0] = None;
        if next < xs.len() && xs[next].issue <= t {
            let i = &xs[next];
            let owner = quad[i.key / 4];
            if owner.is_none_or(|(program, _)| program == i.program) {
                if owner.is_none() {
                    quad[i.key / 4] = Some((i.program, r.programs[i.program].input().mask));
                }
                key_pipe[0] = Some(Owned {
                    serial: next,
                    data: i.key,
                });
                first_pipe[0] = Some(Owned {
                    serial: next,
                    data: i.first,
                });
                last_pipe[0] = Some(Owned {
                    serial: next,
                    data: i.last,
                });
                weights[t as usize % 2] = Some(Owned {
                    serial: next,
                    data: std::array::from_fn(|tap| ((i.payload >> (28 + 9 * tap)) & 511) as u32),
                });
                jobs.push((next, t));
                next += 1;
            } else {
                s.stalls += 1;
            }
        }
        if read_row.is_some() && read_row == write_row {
            return Err("public result same-row R/W".into());
        }
        s.peak_slots = s
            .peak_slots
            .max(quad.iter().filter(|q| q.is_some()).count());
        if jobs.len() > 13 {
            return Err("color fixed valid-delay capacity".into());
        }
    }
    let pixels = xs.iter().filter(|x| x.last).count();
    if (s.writes, s.done, s.published) != (pixels, pixels, pixels)
        || quad.iter().any(Option::is_some)
    {
        return Err("public color result incomplete drain".into());
    }
    Ok(s)
}
pub fn probe(root: &Path, profile: &str, r: &bound::system::Report, stress: bool) {
    let path = root.join(format!("color_{profile}_{stress}.csv"));
    let mut edges = fs::File::create(path).unwrap();
    writeln!(edges, "stress,enabled,event,serial,key").unwrap();
    let s = run(r, stress, false, false, &mut edges).unwrap();
    assert!(
        run(r, false, true, false, &mut std::io::sink()).is_err(),
        "color equal-value stale owner must fail"
    );
    println!("color {profile}/{stress}: {} enabled writes={} done={} published={} peakGlobalSlots={} globalAdmissionStalls={}",s.enabled,s.writes,s.done,s.published,s.peak_slots,s.stalls);
    if !stress {
        let mut edges = fs::File::create(root.join("color_dense_stall.csv")).unwrap();
        writeln!(edges, "stress,enabled,event,serial,key").unwrap();
        let s = run(r, true, false, true, &mut edges).unwrap();
        assert_eq!(s.peak_slots, 16);
        assert!(s.stalls > 0);
        println!("color dense/final-stall: {} enabled writes={} done={} published={} peakGlobalSlots={} globalAdmissionStalls={}",s.enabled,s.writes,s.done,s.published,s.peak_slots,s.stalls);
    }
}
