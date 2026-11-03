//! Actual TDP cache, scalar ROP and shared bank; memory transport remains a peer.
#[path = "support/vliw_tdp.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    pixel_math::{SharedBank, SharedInput},
    tdp::{Cache, Input, Output},
    vertex::Bits,
};
#[derive(Clone, Copy, Default)]
struct Probe {
    mt_active: bool,
    mt_write: bool,
    mt_cancel: bool,
    mt_request: bool,
    mt_prefetch: bool,
    mt_line: u8,
    mt_issue: u8,
    mt_retire: u8,
    mt_data: u8,
    mt_remaining: u8,
    mt_target: u16,
    wr_beat_valid: bool,
    flushing: bool,
    cal: u8,
    fill_row: u8,
    tail: bool,
    result_owner: bool,
    return_key: u8,
    depth_pending: bool,
    color_pending: bool,
    hint_count: u8,
    hints: [u16; 2],
    protected: bool,
    protect_line: u8,
    valid: u8,
    color: u8,
    depth: u8,
    pinned: u8,
    tag: [u16; 8],
    aq: [Bits; 4],
    bq: [Bits; 4],
    shared: Bits,
}
struct Edge {
    i: Input,
    o: Output,
    p: Probe,
}
fn probe(c: &Cache, shared: &SharedBank) -> Probe {
    let s = &c.state;
    let flags = |a: [bool; 8]| {
        a.into_iter()
            .enumerate()
            .fold(0, |v, (n, b)| v | (u8::from(b) << n))
    };
    Probe {
        mt_active: s.mt_active,
        mt_write: s.mt_write,
        mt_cancel: s.mt_cancel,
        mt_request: s.mt_request,
        mt_prefetch: s.mt_prefetch,
        mt_line: s.mt_line,
        mt_issue: s.mt_issue,
        mt_retire: s.mt_retire,
        mt_data: s.mt_data,
        mt_remaining: s.mt_remaining,
        mt_target: s.mt_target,
        wr_beat_valid: s.wr_beat_valid,
        flushing: s.flushing,
        cal: s.cal,
        fill_row: s.fill_row,
        tail: s.tail,
        result_owner: s.result_owner,
        return_key: s.return_key,
        depth_pending: s.depth_pending,
        color_pending: s.color_pending,
        hint_count: s.hint_count,
        hints: s.hints,
        protected: s.protect_valid,
        protect_line: s.protect_line,
        valid: flags(s.valid),
        color: flags(s.dirty_color),
        depth: flags(s.dirty_depth),
        pinned: (if s.ctx_valid[0] {
            s.ctx_line_mask[0]
        } else {
            0
        }) | (if s.ctx_valid[1] {
            s.ctx_line_mask[1]
        } else {
            0
        }),
        tag: s.tag,
        aq: std::array::from_fn(|n| c.banks[n].aq),
        bq: std::array::from_fn(|n| c.banks[n].bq),
        shared: shared.rop_product(),
    }
}
fn offset(x: usize, y: usize) -> usize {
    ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16
}
fn pixel(n: usize, lane: usize) -> (u32, u8, u16) {
    let id = n * 4 + lane + 1;
    (
        ((id as u32).wrapping_mul(0x3197d5)) & 0xffffff,
        [0, 1, 2, 127, 128, 254, 255][id % 7],
        (id as u16).wrapping_mul(1237),
    )
}
fn quad(n: usize) -> (usize, usize, u8) {
    let block = n / 3;
    (
        (block % 12) * 16 + (n % 3) * 2,
        (block / 12) * 8,
        [15, 10, 2, 4, 8, 1, 7, 14][n % 8],
    )
}
fn apply(expected: &mut [u16], n: usize, case: usize) {
    let (x, y, mask) = quad(n);
    for lane in 0..4 {
        if mask & (1 << lane) == 0 {
            continue;
        }
        let p = offset(x + (lane & 1), y + (lane >> 1));
        let zindex = 0x140000 / 2 + p;
        let (rgb, alpha, z) = pixel(n, lane);
        if case == 1 && z > expected[zindex] {
            continue;
        }
        let colorindex = 0x100000 / 2 + p;
        let old = expected[colorindex];
        let r = u32::from(old >> 11);
        let g = u32::from(old >> 5 & 63);
        let b = u32::from(old & 31);
        let dst = [r * 8 + r / 4, g * 4 + g / 16, b * 8 + b / 4];
        let weight = if case == 0 {
            64
        } else {
            (u32::from(alpha) + 2) / 4
        };
        let mut packed = 0;
        for c in 0..3 {
            let channel = (((rgb >> (c * 8)) & 255) * weight + dst[c] * (64 - weight) + 32) / 64;
            let max = if c == 1 { 63 } else { 31 };
            packed |= (((channel * max * 2 + 255) / 510) as u16) << [11, 5, 0][c];
        }
        expected[colorindex] = packed;
        if case == 0 {
            expected[zindex] = z;
        }
    }
}
struct Transaction {
    write: bool,
    address: usize,
    word: usize,
    terminal_wait: usize,
}
fn hash_ram(c: &Cache) -> u64 {
    c.banks.iter().flat_map(|b| b.cells).fold(0, |v, x| {
        v.rotate_left(7) ^ x.value ^ x.known.rotate_left(19)
    })
}
fn run(case: usize) -> Vec<Edge> {
    let mut c = Cache::default();
    let mut shared = SharedBank::default();
    let mut phase = false;
    let mut memory: Vec<u16> = (0..0x800000 / 2)
        .map(|i| (i as u16).wrapping_mul(731).wrapping_add(0x1234))
        .collect();
    let mut expected = memory.clone();
    let mut transaction: Option<Transaction> = None;
    let mut rows = Vec::new();
    let mut n = 0;
    let mut row = 0;
    let mut round = 0;
    let mut flush = false;
    let mut reset_next = false;
    let mut warm_hash = None;
    let mut spec_offered = false;
    let mut aborted = false;
    let mut peak_contexts = 0;
    let mut writes_before_flush = false;
    let mut b_during_ce0 = false;
    let mut saw_depth_only = false;
    for wall in 0..45000usize {
        let reset = wall == 0 || reset_next;
        reset_next = false;
        let ce = wall % 17 != 8 && wall % 17 != 9;
        let allow = wall % 23 != 14;
        let (x, y, mask) = quad(n.min(35));
        let lane = row / 2;
        let (rgb, alpha, z) = pixel(n, lane);
        let abort = case == 0
            && !aborted
            && c.state.mt_prefetch
            && c.state.mt_write
            && c.state.mt_issue == 2
            && c.state.mt_retire == 1;
        if abort {
            aborted = true;
        }
        let mut i = Input {
            reset,
            ce,
            commit_allow: allow,
            shade_phase: phase,
            shared_rop_product: shared.rop_product(),
            row_valid: !flush && n < 36 && (row & 1 != 0 || wall % 13 != 5),
            tile: (((y / 16) << 5) | (x / 16)) as u16,
            x: x as u16,
            y: y as u8,
            mask,
            row_index: row as u8,
            data: Bits::new(if row & 1 == 0 { rgb.into() } else { z.into() }, 32),
            alpha: Bits::new(alpha.into(), 8),
            depth_func: if case == 0 { 7 } else { 3 },
            depth_write: case == 0,
            blend: case == 1,
            color_base: 2048,
            depth_base: 2560,
            flush_request: flush,
            prefetch_abort: abort,
            ..Default::default()
        };
        if !flush && n == 36 && !c.state.mt_active && !c.state.ctx_valid[0] && !c.state.ctx_valid[1]
        {
            if round == 0 && case == 0 && !spec_offered {
                i.prefetch_valid = true;
                i.prefetch_tile = 24;
                spec_offered = true;
            } else if round != 0 || case != 0 || aborted && !c.state.mt_active {
                flush = true;
                i.flush_request = true;
            }
        }
        if let Some(t) = &transaction {
            if t.word < 16 {
                if t.write {
                    i.mem_write_ready = wall % 7 != 3;
                } else if wall % 7 != 2 {
                    i.mem_read_valid = true;
                    for h in 0..4 {
                        i.mem_read_data |=
                            u64::from(memory[t.address / 2 + t.word * 4 + h]) << (h * 16);
                    }
                }
            } else {
                i.mem_complete_valid = t.terminal_wait == 3;
                i.mem_complete_ok = true;
            }
        } else {
            i.mem_req_ready = wall % 5 != 2;
        }
        let o = c.outputs(i);
        rows.push(Edge {
            i,
            o,
            p: probe(&c, &shared),
        });
        peak_contexts = peak_contexts.max(c.state.ctx_valid.into_iter().filter(|v| *v).count());
        saw_depth_only |= c.state.mt_write && c.state.mt_remaining == 2;
        if !reset && o.row_accept {
            if row == 0 {
                apply(&mut expected, n, case);
            }
            row += 1;
            if row == 8 {
                row = 0;
                n += 1;
            }
        }
        if let Some(t) = &mut transaction {
            if t.word < 16
                && (i.mem_read_valid || t.write && i.mem_write_ready && o.mem_write_valid)
            {
                if t.write {
                    let value = o.mem_write_data.get(64, "TDP physical beat").unwrap();
                    for h in 0..4 {
                        memory[t.address / 2 + t.word * 4 + h] = (value >> (h * 16)) as u16;
                    }
                    writes_before_flush |= !flush;
                }
                b_during_ce0 |= !ce;
                t.word += 1;
            } else if t.word == 16 {
                t.terminal_wait += 1;
            }
            if i.mem_complete_valid {
                transaction = None;
            }
        }
        if o.mem_req_valid && i.mem_req_ready {
            assert!(transaction.is_none());
            assert_eq!(o.mem_req_address & 127, 0);
            transaction = Some(Transaction {
                write: o.mem_req_write,
                address: o.mem_req_address as usize,
                word: 0,
                terminal_wait: 0,
            });
        }
        let si = SharedInput {
            rop_ce: o.shared_rop_ce,
            rop_a: o.shared_rop_a,
            rop_b: o.shared_rop_b,
            ..Default::default()
        };
        c.clock(i)
            .unwrap_or_else(|e| panic!("wall{wall} row{row} n{n}: {e}"));
        shared.clock(si);
        if reset {
            phase = false;
            if let Some(hash) = warm_hash.take() {
                assert_eq!(hash_ram(&c), hash, "reset changed retained RAM");
            }
        } else if ce && allow {
            phase = !phase;
        }
        if flush && o.flush_complete {
            assert!(transaction.is_none() && o.idle);
            if let Some((at, (actual, want))) = memory
                .iter()
                .zip(&expected)
                .enumerate()
                .find(|(_, (a, b))| a != b)
            {
                panic!(
                    "case{case} round{round}: physical byte{:x}: {actual:04x} != {want:04x}",
                    at * 2
                );
            }
            if round == 0 {
                round = 1;
                n = 0;
                row = 0;
                flush = false;
                warm_hash = Some(hash_ram(&c));
                reset_next = true;
            } else {
                assert_eq!(peak_contexts, 2);
                assert!(writes_before_flush && b_during_ce0);
                assert!(case != 0 || aborted && saw_depth_only);
                return rows;
            }
        }
    }
    panic!(
        "TDP exceeded45000 wall edges case{case}, n{n}, flush{flush} round{round} aborted{aborted}"
    );
}
#[test]
fn tdp_banks_write_back_both_planes_and_flush_with_wall_clock_progress() {
    for case in 0..2 {
        let rows = run(case);
        println!("TDP_CASE {case} walls={}", rows.len());
    }
}
fn root(label: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
#[ignore = "two bounded selected eight-line/four-bank TDP physical-transport profiles"]
fn every_tdp_cache_edge_matches_production() {
    let root = root("tdp");
    for case in 0..2 {
        let rows = run(case);
        let pins = fixture::fields(&rows[0]);
        vertex_support::rtl::replay(
            &root,
            &format!("case{case}"),
            rows.iter().map(fixture::fields).collect(),
            fixture::production(&pins),
            false,
            None,
        );
    }
    println!("TDP_CHECKPOINT {}", root.display());
}

#[test]
#[ignore = "three actual maintenance/dirty/context mutations and two original monitors"]
fn tdp_negative_controls_and_original_monitors() {
    let root = root("tdp-negative");
    let rows = run(0);
    let pins = fixture::fields(&rows[0]);
    for (name, old, new) in [
        ("maintenance_ce", "wire b_en=m_rd_issue||refill_write;wire b_we=refill_write;",
            "wire b_en=ce&&(m_rd_issue||refill_write);wire b_we=refill_write;"),
        ("dirty_before_terminal", "    if(mem_complete_valid)begin",
            "    if(wbeat&&(&mt_data[4:0]))begin if(mt_data[5])dird[mt_line]<=0;else dirc[mt_line]<=0;end\n    if(mem_complete_valid)begin"),
        ("context_at_depth", "    if(color_fire_write)begin", "    if(depth_fire||color_fire_write)begin"),
    ] {
        let source = fixture::production(&pins);
        assert_eq!(source.matches(old).count(), 1, "{name}");
        vertex_support::rtl::replay(&root, name, rows.iter().map(fixture::fields).collect(),
            source.replacen(old,new,1),true,None);
    }
    for (name, at, expected) in [
        (
            "bad_terminal",
            rows.iter().position(|e| e.i.mem_complete_valid).unwrap(),
            "GPU contract: mem_complete_ok",
        ),
        (
            "missing_D",
            rows.iter()
                .position(|e| e.i.ce && e.i.commit_allow && e.i.row_valid && e.i.row_index == 1)
                .unwrap(),
            "paired source D missing after accepted C",
        ),
    ] {
        let mut c = Cache::default();
        for row in &rows[..at] {
            c.clock(row.i).unwrap();
        }
        let mut i = rows[at].i;
        if name == "bad_terminal" {
            i.mem_complete_ok = false;
        } else {
            i.row_valid = false;
        }
        let mut vectors: Vec<_> = rows[..at].iter().map(fixture::fields).collect();
        let last = Edge {
            i,
            o: c.outputs(i),
            p: rows[at].p,
        };
        vectors.push(fixture::fields(&last));
        assert_eq!(c.clock(i).unwrap_err().rule, expected);
        vertex_support::rtl::replay(
            &root,
            name,
            vectors,
            fixture::production(&pins),
            false,
            Some(expected),
        );
    }
    println!("TDP_NEGATIVE_CHECKPOINT {}", root.display());
}
