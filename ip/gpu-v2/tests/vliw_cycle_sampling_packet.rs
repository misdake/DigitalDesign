//! Real Member/Work/Packet/Group state, with an independent geometric partition.
#[allow(dead_code)]
#[path = "support/vliw_pixel_source.rs"]
mod actual;
#[path = "support/vliw_sampling_packet.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    sampling::{member, packet_backend as backend},
    vertex::Bits,
};
use std::collections::VecDeque;
#[derive(Clone)]
struct Edge {
    mi: member::Input,
    mo: member::Output,
    bi: backend::Input,
    bo: backend::Output,
    m: member::Member,
    b: backend::Backend,
}
fn request(n: usize, view: bool) -> member::Input {
    let level = if view {
        9
    } else {
        [0u64, 1, 2, 4, 5, 6, 10][n % 7]
    };
    let side = 1u64 << level.max(1);
    let xy = if view {
        [[0, 0], [15, 7], [16, 8], [398, 238], [31, 15]][n % 5]
    } else {
        [[0, 0], [15, 7], [31, 15], [side - 1, side - 1], [16, 8]][n % 5].map(|v| v % side)
    };
    let flag_mask = [1u8, 3, 5, 9, 15][n / 3 % 5];
    let mut flags = 0;
    for c in 0..4 {
        flags |= u64::from(flag_mask >> c & 1) << (c * 9);
    }
    member::Input {
        flags: Bits::new(flags, 36),
        meta: Bits::new(
            xy[0]
                | (xy[1] << 10)
                | (u64::from(xy[0].is_multiple_of(2)) << 20)
                | (u64::from(xy[1].is_multiple_of(2)) << 21)
                | (level << 22)
                | (((n % 64) as u64) << 30),
            36,
        ),
        coarse: !view && n % 6 == 3,
        last_fine: true,
        ..Default::default()
    }
}
fn golden(i: member::Input, base: u16, mip: bool, view: bool) -> Vec<u64> {
    let meta = i.meta.value;
    let n0 = meta >> 22 & 15;
    let n = if i.coarse { n0.saturating_sub(1) } else { n0 };
    let side = 1u64 << n.max(1);
    let mut origin = [meta & 1023, meta >> 10 & 1023];
    if i.coarse && n0 > 1 {
        origin = std::array::from_fn(|a| {
            ((origin[a] / 2) as i64 - ((meta >> (20 + a)) & 1) as i64).rem_euclid(side as i64)
                as u64
        });
    }
    let key = meta >> 30 & 63;
    let mut groups: Vec<([u64; 2], usize)> = Vec::new();
    let corner = |tap: usize| {
        [
            (origin[0] + (tap % 2) as u64) % side,
            (origin[1] + (tap / 2) as u64) % side,
        ]
    };
    for tap in 0..4 {
        if i.flags.value >> (tap * 9) & 511 == 0 {
            continue;
        }
        let c = corner(tap);
        let tile = [c[0] / 16, c[1] / 8];
        if !groups.iter().any(|(t, _)| *t == tile) {
            groups.push((tile, tap));
        }
    }
    let prefix = if mip {
        (0..n)
            .map(|l| {
                let s = (1u64 << l).div_ceil(16);
                2 * s * s
            })
            .sum()
    } else {
        0
    };
    groups
        .iter()
        .enumerate()
        .map(|(rank, (tile, _))| {
            let pitch = if view { 16 } else { side.div_ceil(16) };
            let half =
                u64::from(base) / 2 + prefix + 2 * (tile[0] * pitch + tile[1] / 2) + tile[1] % 2;
            let mut mask = 0;
            for tap in 0..4 {
                let c = corner(tap);
                if [tile[0], tile[1]] == [c[0] / 16, c[1] / 8] {
                    mask |= 1 << tap;
                }
            }
            half | ((origin[0] % 16) << 15)
                | ((origin[1] % 8) << 19)
                | (u64::from(rank == 0 && !i.coarse) << 22)
                | (u64::from(rank + 1 == groups.len()) << 23)
                | ((key / 4) << 24)
                | ((key % 4) << 28)
                | ((n % 2) << 30)
                | (mask << 31)
        })
        .collect()
}
fn run(view: bool) -> Vec<Edge> {
    let mut m = member::Member::default();
    let mut b = backend::Backend::default();
    let mut expected = VecDeque::new();
    let mut rows = Vec::new();
    let mut n = 0;
    let mut reserved = 0usize;
    let mut consumed = 0;
    let mut generated = 0;
    let mut reset_discarded = 0;
    let mut max_work = 0;
    let mut max_group = 0;
    for wall in 0..4000 {
        let reset = wall == 0 || wall == 439;
        let ce = !reset && wall % 7 != 2;
        let allow = wall % 11 != 5;
        let mi = member::Input {
            reset,
            ce: ce && allow,
            valid: n < 192 && reserved < 16,
            ..request(n, view)
        };
        let mo = m.outputs(mi);
        let bi = backend::Input {
            reset,
            proposal_ce: ce,
            commit_allow: allow,
            valid: mo.valid,
            member: mo.member,
            packet_enable: wall > 35 && wall % 19 > 2,
            out_ready: wall > 230 && wall % 13 > 3,
            texture_base: 0x0800,
            mip: !view,
            framebuffer: view,
            ..Default::default()
        };
        let bo = b.outputs(bi);
        if !reset {
            assert!(
                !(bo.advance && mo.valid && !bo.work.ready),
                "reserved member refused at wall{wall}"
            );
            if bo.valid {
                assert_eq!(
                    bo.payload,
                    Bits::new(*expected.front().unwrap(), 35),
                    "Group35 wall{wall}"
                );
                if bo.advance && bi.out_ready {
                    expected.pop_front();
                    consumed += 1;
                }
            }
            if mi.ce && mi.valid {
                let groups = golden(mi, bi.texture_base, bi.mip, view);
                generated += groups.len();
                expected.extend(groups);
                n += 1;
                reserved += 1;
            }
            reserved -= usize::from(bo.work.ack);
        }
        rows.push(Edge {
            mi,
            mo,
            bi,
            bo,
            m: m.clone(),
            b: b.clone(),
        });
        let member_result = m.clock(mi);
        let backend_result = b.clock(bi);
        member_result.unwrap_or_else(|e| panic!("member wall{wall}: {e}"));
        backend_result.unwrap_or_else(|e| panic!("backend wall{wall}: {e}"));
        if reset {
            reset_discarded += expected.len();
            expected.clear();
            reserved = 0;
        }
        max_work = max_work.max(b.work.count);
        max_group = max_group.max(b.group.used);
        if n == 192 && reserved == 0 && b.group.used == 0 && m.valid & 3 == 0 && b.packet.valid == 0
        {
            break;
        }
    }
    assert!(
        n == 192
            && reserved == 0
            && consumed == generated - reset_discarded
            && consumed > 150
            && max_work == 16
            && max_group == 48
            && expected.is_empty(),
        "n={n} reserved={reserved} consumed={consumed} Work{max_work} Group{max_group}"
    );
    println!("PACKET_NATIVE view={view} walls={} members={n} groups={consumed} work_peak={max_work} group_peak={max_group}",rows.len());
    rows
}
#[test]
fn actual_member_and_packet_rows_match_geometric_half_lines() {
    run(false);
    run(true);
}
fn root(label: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
#[ignore = "two finite actual Member/Work16/Packet/native Group35 DUTs"]
fn member_work_packet_group_match_actual_command_modules() {
    let root = root("packet-group");
    println!("PACKET_GROUP_ROOT {}", root.display());
    for view in [false, true] {
        let rows = run(view);
        let p = fixture::fields(&rows[0]);
        vertex_support::rtl::replay(
            &root,
            if view { "rtt" } else { "pot" },
            rows.iter().map(fixture::fields).collect(),
            fixture::production(&p),
            false,
            None,
        );
    }
}
#[test]
#[ignore = "one expanded actual POT trace including selected coarse members"]
fn coarse_member_and_half_line_addresses_match_actual_root() {
    let rows = run(false);
    let root = root("packet-coarse");
    println!("PACKET_COARSE_ROOT {}", root.display());
    let p = fixture::fields(&rows[0]);
    vertex_support::rtl::replay(
        &root,
        "pot_coarse",
        rows.iter().map(fixture::fields).collect(),
        fixture::production(&p),
        false,
        None,
    );
}
#[test]
#[ignore = "four actual lifetime/address/credit mutations and two original monitors"]
fn packet_group_lifetimes_and_physical_peers_reject_counterexamples() {
    let rows = run(false);
    let root = root("packet-group-negative");
    println!("PACKET_GROUP_NEGATIVE_ROOT {}", root.display());
    let p = fixture::fields(&rows[0]);
    let source = fixture::production(&p);
    for (name, old, new) in [
        (
            "last_tap_ack",
            "wire last = (later == 0);",
            "wire last = 1'b1;",
        ),
        ("paid_native_Q", ".OCE(load)", ".OCE(pr)"),
        (
            "mip_half_line_prefix",
            "level_base<=texture_base[15:1]+prefix;",
            "level_base<=texture_base[15:1];",
        ),
        ("old_Group48_credit", "used<48", "used<49"),
    ] {
        assert_eq!(source.matches(old).count(), 1, "{name}");
        vertex_support::rtl::replay(
            &root,
            name,
            rows.iter().map(fixture::fields).collect(),
            source.replacen(old, new, 1),
            true,
            None,
        );
    }
    for bad_member in [true, false] {
        let mut m = member::Member::default();
        let mut b = backend::Backend::default();
        let mut bad_rows = Vec::new();
        let expected = if bad_member {
            "GPU contract: texture membership"
        } else {
            "Texture16x8 base must be256B aligned"
        };
        let mut rejected = false;
        for wall in 0..30 {
            let mi = member::Input {
                reset: wall == 0,
                ce: wall != 0,
                valid: wall == 1,
                flags: Bits::new(u64::from(!bad_member), 36),
                ..request(0, false)
            };
            let mo = m.outputs(mi);
            let bi = backend::Input {
                reset: wall == 0,
                proposal_ce: wall != 0,
                commit_allow: true,
                valid: mo.valid,
                member: mo.member,
                packet_enable: true,
                out_ready: true,
                texture_base: if bad_member { 0x800 } else { 0x801 },
                ..Default::default()
            };
            let bo = b.outputs(bi);
            bad_rows.push(Edge {
                mi,
                mo,
                bi,
                bo,
                m: m.clone(),
                b: b.clone(),
            });
            let mr = m.clock(mi);
            let br = b.clock(bi);
            if let Some(e) = mr.err().or_else(|| br.err()) {
                assert_eq!(e.rule, expected);
                vertex_support::rtl::replay(
                    &root,
                    if bad_member {
                        "empty_member"
                    } else {
                        "unaligned_base"
                    },
                    bad_rows.iter().map(fixture::fields).collect(),
                    source.clone(),
                    false,
                    Some(expected),
                );
                rejected = true;
                break;
            }
        }
        assert!(rejected);
    }
}
