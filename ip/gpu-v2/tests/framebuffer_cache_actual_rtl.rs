//! Independent bounded Icarus protocol tests for the corrected framebuffer
//! cache controller (`rtl/framebuffer_cache.v`).
//!
//! The external memory model lives in `fixtures/framebuffer_cache_actual_tb.v`
//! and checks the single-outstanding 128-byte contract on every edge: request
//! valid/ready on the old edge with a stable held request under backpressure,
//! continuous non-bubbling write data after admission, blocked-beat stability,
//! read-last-beat plus terminal-success on the same edge, a delayed write ACK,
//! selected read/write terminal failure, an injected compute header fault, and a
//! finite watchdog. The DUT is driven only from the stimulus row file and the
//! initial memory image; no DUT answer is replayed and no hierarchy is forced.
//!
//! The expected final memory is an independent whole-byte golden that applies
//! a separate real-number blend/quantizer per quad with its captured context.
//! That golden is independent of both the RTL and the register-driven scalar
//! numerical `FramebufferEmu`; a single-context case is additionally
//! cross-checked against that emulator. The bundled accepted `rtl/rop_leaf.v`
//! is the only leaf used.
//!
//! The task directory is `target/opencode/cache-rtl-round4`; the emulator is a
//! whole registered model. Cache calendar edges differ; image/commit equivalence
//! does not claim cache edge equivalence.

use gpu_v2::framebuffer::{
    emu::FramebufferEmu,
    ports::*,
    rtl,
    sim::{bounded::OutputRow, fixture::Fixture},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

const MAX_RUN: Duration = Duration::from_secs(120);
const TASK_DIR: &str = "../../target/opencode/cache-rtl-round4";

fn evidence_root(case: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(TASK_DIR)
        .join("actual")
        .join(case);
    fs::create_dir_all(&root).unwrap();
    root
}

/// The bundled accepted leaf. The test never substitutes a local probe and
/// never returns early: a missing CE/key interface is a hard failure.
fn leaf_source() -> String {
    let source = rtl::ROP_LEAF.to_string();
    let has_ce = source.contains("in_valid")
        && source.contains("in_key")
        && source.contains("in_ready")
        && source.contains("ce");
    assert!(
        has_ce,
        "bundled rtl/rop_leaf.v lacks the accepted CE/key interface"
    );
    source
}

fn wait_reap(child: &mut Child, limit: Duration, label: &str, root: &Path) {
    let start = Instant::now();
    loop {
        match child.try_wait().unwrap() {
            Some(status) => {
                assert!(
                    status.success(),
                    "{label} failed:\n{}\n{}",
                    fs::read_to_string(root.join("run.out")).unwrap_or_default(),
                    fs::read_to_string(root.join("run.err")).unwrap_or_default()
                );
                return;
            }
            None => {
                if start.elapsed() >= limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("{label} exceeded {limit:?}, killed");
                }
                sleep(Duration::from_millis(10));
            }
        }
    }
}

fn run_iverilog(root: &Path, sources: &[(&str, String)]) {
    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    fs::write(root.join("run.out"), b"").unwrap();
    fs::write(root.join("run.err"), b"").unwrap();
    let mut compile = Command::new(iverilog);
    compile
        .current_dir(root)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp"])
        .stdout(Stdio::from(
            fs::File::create(root.join("compile.out")).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(root.join("compile.err")).unwrap(),
        ));
    for (name, text) in sources {
        fs::write(root.join(name), text).unwrap();
        if name.ends_with(".v") {
            compile.arg(name);
        }
    }
    let mut compile_child = compile.spawn().unwrap();
    wait_reap(&mut compile_child, MAX_RUN, "iverilog compile", root);
    let mut run = Command::new(vvp);
    run.current_dir(root)
        .arg("test.vvp")
        .stdout(Stdio::from(fs::File::create(root.join("run.out")).unwrap()))
        .stderr(Stdio::from(fs::File::create(root.join("run.err")).unwrap()));
    let mut child = run.spawn().unwrap();
    wait_reap(&mut child, MAX_RUN, "vvp run", root);
    let out = fs::read_to_string(root.join("run.out")).unwrap_or_default();
    assert!(out.contains("PASS"), "no PASS marker:\n{out}");
}

fn hex_bytes(path: &Path) -> Vec<u8> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .map(|l| u8::from_str_radix(l.trim(), 16).unwrap())
        .collect()
}

fn hex_values(path: &Path) -> Vec<u64> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .map(|l| u64::from_str_radix(l.trim(), 16).unwrap())
        .collect()
}

/// A quad with per-lane distinct RGB/depth so a lane/bank slice error shows up.
fn quad(x: u16, y: u8, mask: u8, n: u8) -> Quad {
    Quad {
        header: Header { x, y, mask },
        pixels: std::array::from_fn(|i| Fragment {
            rgba: [
                n.wrapping_mul(37).wrapping_add((i as u8) * 53),
                80 + (i as u8) * 31 + n,
                255u8.wrapping_sub(n).wrapping_sub((i as u8) * 17),
                97 + (i as u8) * 29,
            ],
            depth: 1000 + u16::from(n) * 7 + i as u16,
        }),
    }
}

fn surface(width: u16, height: u16, depth_base: u32) -> MaterializedSurface {
    MaterializedSurface {
        color_base_bytes: 0,
        depth_base_bytes: depth_base,
        width,
        height,
    }
}

/// Distinct guard bytes: no two long runs, every byte pattern exercised.
fn guard_image(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i as u8).wrapping_mul(37).wrapping_add(11)) ^ ((i >> 3) as u8))
        .collect()
}

/// Independent whole-byte golden: apply each quad in order with its own
/// captured context. This models the cache as a serial read-modify-write over
/// the materialized framebuffer and is independent of the RTL and emulator.
fn golden(s: MaterializedSurface, cases: &[(Quad, Context)], mut bytes: Vec<u8>) -> Vec<u8> {
    for (q, c) in cases {
        bytes = golden_one(bytes, std::slice::from_ref(q), s, *c);
    }
    bytes
}

fn golden_one(mut bytes: Vec<u8>, qs: &[Quad], s: MaterializedSurface, c: Context) -> Vec<u8> {
    for q in qs {
        for lane in 0..4 {
            if q.header.mask & (1 << lane) == 0 {
                continue;
            }
            let x = q.header.x as usize + lane % 2;
            let y = q.header.y as usize + lane / 2;
            let offset =
                (((y / 16) * (s.width as usize / 16) + x / 16) * 256 + (y % 16) * 16 + x % 16) * 2;
            let ca = s.color_base_bytes as usize + offset;
            let da = s.depth_base_bytes as usize + offset;
            let old = u16::from_le_bytes([bytes[ca], bytes[ca + 1]]);
            let depth = u16::from_le_bytes([bytes[da], bytes[da + 1]]);
            let f = q.pixels[lane];
            let comparisons = [
                false,
                f.depth < depth,
                f.depth == depth,
                f.depth <= depth,
                f.depth > depth,
                f.depth != depth,
                f.depth >= depth,
                true,
            ];
            if !comparisons[c.depth as usize] {
                continue;
            }
            let codes = [
                (old / 2048) as u32,
                ((old / 32) % 64) as u32,
                (old % 32) as u32,
            ];
            let mut out = 0u16;
            for i in 0..3 {
                let bits = if i == 1 { 6 } else { 5 };
                let m = (1u32 << bits) - 1;
                let d = ((codes[i] << (8 - bits)) + (codes[i] >> (2 * bits - 8))) as f64;
                let v = if c.blend == Blend::Replace {
                    f.rgba[i] as f64
                } else {
                    (f.rgba[3] as f64 / 255.0 * f.rgba[i] as f64
                        + (1.0 - f.rgba[3] as f64 / 255.0) * d)
                        .round()
                };
                let quant = (v * m as f64 / 255.0).round() as u16;
                out |= quant << [11, 5, 0][i];
            }
            bytes[ca..ca + 2].copy_from_slice(&out.to_le_bytes());
            if c.depth_write {
                bytes[da..da + 2].copy_from_slice(&f.depth.to_le_bytes());
            }
        }
    }
    bytes
}

/// Run the register-driven scalar numerical emulator; return (final bytes,
/// fault, committed words).
fn run_emu(
    surface: MaterializedSurface,
    context: Context,
    quads: &[Quad],
    initial: Vec<u8>,
    ce_period: u64,
    max_cycles: u64,
    fail_first: bool,
) -> (Vec<u8>, bool, Vec<u32>) {
    let mut model = FramebufferEmu::new(surface, context, max_cycles).unwrap();
    let mut memory = Fixture::new(initial);
    if fail_first {
        memory.fail_request = Some(1);
    }
    let mut cursor = 0usize;
    let mut flushing = false;
    let mut commits = Vec::new();
    for cycle in 0..max_cycles {
        let input = quads.get(cursor / 8).map(|q| OutputRow {
            header: q.header,
            row: (cursor % 8) as u8,
            data: q.rows()[cursor % 8],
        });
        let t = model
            .step(ce_period == 0 || cycle % ce_period != 0, input, &mut memory)
            .unwrap();
        if let Some(h) = t.committed {
            commits.push(((h.x as u32) << 12) | ((h.y as u32) << 4) | u32::from(h.mask));
        }
        if t.input_accepted {
            cursor += 1;
        }
        if cursor == quads.len() * 8 && !flushing && !model.fault {
            model.request_flush().unwrap();
            flushing = true;
        }
        if model.flush_complete || model.drained() {
            break;
        }
    }
    (memory.bytes, model.fault, commits)
}

#[allow(clippy::too_many_arguments)]
fn run_cache_rtl(
    case: &str,
    width: u16,
    height: u16,
    depth_base: u32,
    cases: &[(Quad, Context)],
    initial: &[u8],
    ce_period: u64,
    stall: bool,
    fail_read: bool,
    fail_write: bool,
    bad_rows: &[usize],
) -> (Vec<u8>, bool, Vec<u32>) {
    let root = evidence_root(case);
    let bytes = initial.len();
    let mut rows = String::new();
    for (qi, (q, c)) in cases.iter().enumerate() {
        for (r, data) in (*q).rows().iter().enumerate() {
            let bad = bad_rows.contains(&(qi * 8 + r));
            let word = u64::from(*data)
                | (u64::from(r as u8) << 32)
                | (u64::from(q.header.mask) << 35)
                | (u64::from(q.header.y) << 39)
                | (u64::from(q.header.x) << 47)
                | ((c.depth as u64) << 56)
                | (u64::from(c.blend == Blend::SrcOver) << 59)
                | (u64::from(c.depth_write) << 60)
                | (u64::from(bad) << 61);
            rows.push_str(&format!("{word:016x}\n"));
        }
    }
    let stim = cases.len() * 8;
    let maxc = cases.len() + 8;
    let watchdog = (stim as u64) * 64 + 500_000;
    let tb = include_str!("fixtures/framebuffer_cache_actual_tb.v")
        .replace("__STIM__", &stim.to_string())
        .replace("__BYTES__", &bytes.to_string())
        .replace("__MAXC__", &maxc.to_string())
        .replace("__COLOR__", "0")
        .replace("__DEPTH__", &depth_base.to_string())
        .replace("__WIDTH__", &width.to_string())
        .replace("__HEIGHT__", &height.to_string())
        .replace("__CE__", &ce_period.to_string())
        .replace("__FAIL_READ__", if fail_read { "1" } else { "0" })
        .replace("__FAIL_WRITE__", if fail_write { "1" } else { "0" })
        .replace("__STALL__", if stall { "1" } else { "0" })
        .replace(
            "__READ_ON_ACCEPT__",
            if case.contains("accept_read") {
                "1"
            } else {
                "0"
            },
        )
        .replace("__WATCH__", &watchdog.to_string());
    let mem_hex: String = initial.iter().map(|b| format!("{b:02x}\n")).collect();
    run_iverilog(
        &root,
        &[
            ("framebuffer_cache.v", rtl::CACHE.to_string()),
            ("rop_leaf.v", leaf_source()),
            ("tb.v", tb),
            ("rows.hex", rows),
            ("mem.hex", mem_hex),
        ],
    );
    let run = fs::read_to_string(root.join("run.out")).unwrap();
    let fault = run.contains("fault=1");
    // The testbench dumps the whole commit array; unused entries are zero and a
    // real commit always has a nonzero mask.
    let commits = hex_values(&root.join("commits.hex"))
        .into_iter()
        .filter(|w| *w != 0)
        .map(|w| w as u32)
        .collect();
    (hex_bytes(&root.join("cache_out.hex")), fault, commits)
}

/// Assert a run faulted only after at least one memory request was accepted.
fn asserted_fault_after_transport(case: &str) {
    let run = fs::read_to_string(evidence_root(case).join("run.out")).unwrap();
    assert!(run.contains("fault=1"), "{case}: no fault:\n{run}");
    let line = run
        .lines()
        .find(|l| l.starts_with("FAULT reqs="))
        .unwrap_or_else(|| panic!("{case}: no FAULT reqs report:\n{run}"));
    let reqs: u64 = line
        .trim_start_matches("FAULT reqs=")
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        reqs >= 1,
        "{case}: fault before any accepted transport: {line}"
    );
}

#[test]
fn cache_actual_rtl_whole_byte_golden() {
    // 10x3 = 30 tiles (> 8 ways) with 34 quads forces > 20 dirty evictions.
    let width = 160u16;
    let height = 48u16;
    let depth_base = u32::from(width / 16) * u32::from(height / 16) * 512;
    let s = surface(width, height, depth_base);
    let initial = guard_image(depth_base as usize * 2 + 256);
    let cases: Vec<(Quad, Context)> = (0..34u16)
        .map(|i| {
            let context = match i % 4 {
                0 => Context {
                    depth: DepthFunc::LessEqual,
                    depth_write: true,
                    blend: Blend::Replace,
                },
                1 => Context {
                    depth: DepthFunc::Greater,
                    depth_write: true,
                    blend: Blend::SrcOver,
                },
                2 => Context {
                    depth: DepthFunc::LessEqual,
                    depth_write: false,
                    blend: Blend::SrcOver,
                },
                _ => Context {
                    depth: DepthFunc::Always,
                    depth_write: false,
                    blend: Blend::Replace,
                },
            };
            let mask = match i % 5 {
                0 => 15,
                1 => 7,
                2 => 5,
                3 => 3,
                _ => 1,
            };
            let tile = i % 30;
            (
                quad((tile % 10) * 16, (tile / 10 * 16) as u8, mask, i as u8),
                context,
            )
        })
        .collect();
    // Every quad writes, so with 8 ways the first `distinct - 8` misses must evict
    // a dirty line. This guarantees the required > 20 dirty evictions.
    let distinct: std::collections::HashSet<u16> =
        cases.iter().map(|(q, _)| s.tile(q.header)).collect();
    assert!(distinct.len() > 8, "need more tiles than ways");
    assert!(distinct.len() - 8 >= 20, "need >20 dirty evictions");
    let expected = golden(s, &cases, initial.clone());
    // CE pauses at row capture/leaf admission, request backpressure and stalled
    // beats are all enabled in this run.
    let (got, fault, commits) = run_cache_rtl(
        "whole_byte",
        width,
        height,
        depth_base,
        &cases,
        &initial,
        3,
        true,
        false,
        false,
        &[],
    );
    assert!(!fault, "unexpected fault in whole-byte golden");
    assert_eq!(got, expected, "whole-byte memory diverges from golden");
    assert_eq!(commits.len(), cases.len(), "one commit per quad");
    assert!(!commits.is_empty(), "nonzero commits expected");
    let run = fs::read_to_string(evidence_root("whole_byte").join("run.out")).unwrap();
    assert!(run.contains("PASS"), "missing success marker:\n{run}");
}

#[test]
fn cache_actual_rtl_first_read_on_admission_all_depth_modes_and_zero_alpha() {
    let s = surface(64, 32, 4096);
    let initial = guard_image(8448);
    let modes = [
        DepthFunc::Never,
        DepthFunc::Less,
        DepthFunc::Equal,
        DepthFunc::LessEqual,
        DepthFunc::Greater,
        DepthFunc::NotEqual,
        DepthFunc::GreaterEqual,
        DepthFunc::Always,
    ];
    let cases: Vec<_> = modes
        .into_iter()
        .enumerate()
        .map(|(i, depth)| {
            let mut q = quad((i as u16 % 4) * 16, (i as u8 / 4) * 16, 15, i as u8);
            for p in &mut q.pixels {
                p.rgba[3] = 0;
            }
            (
                q,
                Context {
                    depth,
                    depth_write: i % 2 == 0,
                    blend: Blend::SrcOver,
                },
            )
        })
        .collect();
    let expected = golden(s, &cases, initial.clone());
    let (got, fault, commits) = run_cache_rtl(
        "accept_read_depth",
        64,
        32,
        4096,
        &cases,
        &initial,
        7,
        true,
        false,
        false,
        &[],
    );
    assert!(!fault);
    assert_eq!(got, expected);
    assert_eq!(commits.len(), cases.len());
}

#[test]
fn cache_actual_rtl_matches_registered_emu_single_context() {
    let width = 64u16;
    let height = 32u16;
    let depth_base = u32::from(width / 16) * u32::from(height / 16) * 512;
    let s = surface(width, height, depth_base);
    let initial = guard_image(depth_base as usize * 2 + 256);
    let context = Context {
        depth: DepthFunc::Always,
        depth_write: true,
        blend: Blend::SrcOver,
    };
    let quads: Vec<Quad> = (0..12u8)
        .map(|n| quad((n as u16 % 4) * 16, ((n / 4) % 2) * 16, 15, n))
        .collect();
    let cases: Vec<(Quad, Context)> = quads.iter().map(|q| (*q, context)).collect();
    let (emu_bytes, emu_fault, emu_commits) =
        run_emu(s, context, &quads, initial.clone(), 0, 400_000, false);
    assert!(!emu_fault);
    assert_eq!(
        golden(s, &cases, initial.clone()),
        emu_bytes,
        "golden and scalar emu disagree"
    );
    // Continuous write-ready (no beat stalls) and no CE pauses.
    let (got, fault, commits) = run_cache_rtl(
        "emu_cross",
        width,
        height,
        depth_base,
        &cases,
        &initial,
        0,
        false,
        false,
        false,
        &[],
    );
    assert!(!fault);
    assert_eq!(got, emu_bytes, "RTL memory diverges from scalar emu");
    assert_eq!(commits, emu_commits, "RTL commit sequence diverges");
}

#[test]
fn cache_actual_rtl_read_failure_faults_after_transport() {
    let width = 16u16;
    let height = 16u16;
    let depth_base = 512u32;
    let initial = guard_image(depth_base as usize * 2 + 256);
    let context = Context {
        depth: DepthFunc::Always,
        depth_write: true,
        blend: Blend::Replace,
    };
    let cases = vec![(quad(0, 0, 15, 1), context)];
    let (got, fault, commits) = run_cache_rtl(
        "read_fail",
        width,
        height,
        depth_base,
        &cases,
        &initial,
        0,
        false,
        true,
        false,
        &[],
    );
    assert!(fault, "read terminal failure must fault");
    asserted_fault_after_transport("read_fail");
    assert_eq!(got, initial, "failed refill must not modify memory");
    assert!(commits.is_empty(), "failed refill must not commit");
}

#[test]
fn cache_actual_rtl_write_failure_faults_after_transport() {
    // 10 tiles over 8 ways: the 9th miss evicts a dirty line and issues the first
    // write request, which is the one selected to fail.
    let width = 160u16;
    let height = 16u16;
    let depth_base = u32::from(width / 16) * u32::from(height / 16) * 512;
    let initial = guard_image(depth_base as usize * 2 + 256);
    let context = Context {
        depth: DepthFunc::Always,
        depth_write: true,
        blend: Blend::Replace,
    };
    let cases: Vec<(Quad, Context)> = (0..10u16)
        .map(|tile| {
            (
                quad((tile % 10) * 16, (tile / 10 * 16) as u8, 15, tile as u8),
                context,
            )
        })
        .collect();
    let (_, fault, _) = run_cache_rtl(
        "write_fail",
        width,
        height,
        depth_base,
        &cases,
        &initial,
        0,
        false,
        false,
        true,
        &[],
    );
    assert!(fault, "write terminal failure must fault");
    asserted_fault_after_transport("write_fail");
}

#[test]
fn cache_actual_rtl_compute_fault_after_accepted_transport() {
    // Quad 0 misses and starts a refill; while that accepted transport drains,
    // quad 1 presents an odd row with a set reserved bit and must fault.
    let width = 64u16;
    let height = 32u16;
    let depth_base = u32::from(width / 16) * u32::from(height / 16) * 512;
    let initial = guard_image(depth_base as usize * 2 + 256);
    let context = Context {
        depth: DepthFunc::Always,
        depth_write: true,
        blend: Blend::Replace,
    };
    let cases = vec![(quad(0, 0, 15, 1), context), (quad(16, 0, 15, 2), context)];
    // Quad 1's last (odd) row carries the reserved-bit violation, presented
    // several edges after quad 0's refill request has been accepted.
    let bad_rows = [15usize];
    let (_, fault, commits) = run_cache_rtl(
        "compute_fault",
        width,
        height,
        depth_base,
        &cases,
        &initial,
        0,
        false,
        false,
        false,
        &bad_rows,
    );
    assert!(fault, "reserved-bit header violation must fault");
    asserted_fault_after_transport("compute_fault");
    assert!(commits.is_empty(), "faulted quad must not commit");
}
