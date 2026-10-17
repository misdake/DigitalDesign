// This candidate remains outside the active factory until Stage3 integration.
mod frontend {
    pub use gpu_v2::frontend::*;
}
use gpu_v2::system::pixel::{quad_ingress, vliw_backend as backend};
use gpu_v2::{framebuffer, texture};
#[path = "../src/system/vliw_memory.rs"]
mod memory_boundary;
use gpu_v2::frontend::vliw_dma::{self as dma, Descriptor, Segment};
use memory_boundary::*;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Edge {
    i: Input,
    o: Output,
}
fn edge(b: &mut Boundary, v: &mut Vec<Edge>, i: Input) -> Output {
    let o = b.step(i);
    v.push(Edge { i, o });
    o
}
fn pattern(address_bytes: u32) -> u64 {
    0xfedc_1234_98ba_7650 ^ u64::from(address_bytes).wrapping_mul(0x00f0_0001)
}
fn write_pattern(transaction: usize, beat: u8) -> u64 {
    0x17ec_daba_0034_0090 ^ (transaction as u64 * 0x1223) ^ (u64::from(beat) * 0x5319)
}
fn descriptor(n: usize) -> Descriptor {
    Descriptor {
        page: n & 1 != 0,
        tag: n as u8,
        config: Segment {
            source_halfword: 0x100,
            destination_word: 300,
            bytes: if n & 1 == 0 { 72 } else { 0 },
        },
        metadata_source_halfword: 0x180,
        metadata_destination_word: 280,
        payload: Segment {
            source_halfword: 0x200,
            destination_word: 0,
            bytes: [8, 40, 72, 128, 136, 1024][n],
        },
    }
}
fn expected(d: Descriptor) -> BTreeMap<u16, u64> {
    let mut x = BTreeMap::new();
    for s in [d.config, d.metadata(), d.payload] {
        for n in 0..s.bytes / 8 {
            assert!(x
                .insert(
                    s.destination_word + n,
                    pattern((s.source_halfword + u32::from(n) * 4) * 2)
                )
                .is_none());
        }
    }
    x
}

fn physical() -> Vec<Edge> {
    use digital_design_hardware_gowin::sdram_memory_controller::{
        emu::{idle_inputs, Combination},
        ports::OracleImage,
    };
    let mut bytes = vec![0; 65536];
    for (n, x) in bytes.as_chunks_mut::<8>().0.iter_mut().enumerate() {
        x.copy_from_slice(&pattern(n as u32 * 8).to_le_bytes());
    }
    // Safety: external SDRAM input fixture, never a computed GPU intermediate.
    let image =
        unsafe { OracleImage::from_host(0, bytes, "VLIW three-master external SDRAM fixture") }
            .unwrap();
    let mut mc = Combination::with_early_grant(image, 32, true).unwrap();
    let mut b = Boundary::default();
    let mut v = Vec::new();
    let (mut job, mut ga, mut ti, mut ta, mut fi, mut fa, mut wb, mut rb) =
        (0usize, false, 0usize, false, 0usize, false, 0u8, 0u8);
    let mut writes = BTreeMap::new();
    let mut tex_beats = 0u8;
    let mut held = [None; 3];
    let mut occupied = 0u8;
    let mut max_occupied = 0u32;
    let mut paused_returns = 0usize;
    let mut classes = [0usize; 4];
    let mut read_count = 0;
    let mut write_count = 0;
    for wall in 0..100_000 {
        let fb_write = fi & 1 == 0;
        let mut i = Input {
            reset: wall == 0,
            geometry: dma::Input {
                descriptor: if job < 6 && !ga {
                    Some(descriptor(job))
                } else {
                    None
                },
                page_writable: true,
                gpu_drained: true,
                completion_ready: wall % 29 > 9,
                ..dma::Input::default()
            },
            texture: if ti < 20 && !ta {
                Some(0x2000 + (ti % 8) as u32 * 128)
            } else {
                None
            },
            framebuffer: if fi < 16 && !fa {
                Some(BackendRequest {
                    address_bytes: 0x4000 + (fi / 2) as u32 * 128,
                    write: fb_write,
                })
            } else {
                None
            },
            framebuffer_write: if fi < 16 && fb_write && wb < 16 {
                Some(write_pattern(fi / 2, wb))
            } else {
                None
            },
            ..Input::default()
        };
        let p = b.output(&i);
        let mut mi = idle_inputs();
        mi.reset = i.reset;
        if let Some(r) = p.physical[0] {
            mi.gpu_ro_request_valid = true;
            mi.gpu_ro_address = u64::from(r.address_halfword);
            mi.gpu_ro_line_count_minus_1 = u64::from(r.line_count_minus_1);
        }
        if let Some(r) = p.physical[1] {
            mi.gpu_fb_r_request_valid = true;
            mi.gpu_fb_r_address = u64::from(r.address_halfword);
            mi.gpu_fb_r_line_count_minus_1 = u64::from(r.line_count_minus_1);
        }
        if let Some(r) = p.physical[2] {
            mi.gpu_fb_w_request_valid = true;
            mi.gpu_fb_w_write = true;
            mi.gpu_fb_w_address = u64::from(r.address_halfword);
            mi.gpu_fb_w_line_count_minus_1 = u64::from(r.line_count_minus_1);
        }
        mi.gpu_fb_w_write_data = p.write_data;
        let mo = mc.output(&mi);
        i.physical = [
            PhysicalFeedback {
                request_ready: mo.gpu_ro_request_ready,
                response: mo.gpu_ro_response_valid.then_some(dma::Response {
                    data: mo.gpu_ro_read_data,
                    last: mo.gpu_ro_response_last,
                    error: mo.gpu_ro_error,
                }),
                write_ready: false,
            },
            PhysicalFeedback {
                request_ready: mo.gpu_fb_r_request_ready,
                response: mo.gpu_fb_r_response_valid.then_some(dma::Response {
                    data: mo.gpu_fb_r_read_data,
                    last: mo.gpu_fb_r_response_last,
                    error: mo.gpu_fb_r_error,
                }),
                write_ready: false,
            },
            PhysicalFeedback {
                request_ready: mo.gpu_fb_w_request_ready,
                write_ready: mo.gpu_fb_w_write_data_ready,
                response: mo.gpu_fb_w_response_valid.then_some(dma::Response {
                    data: mo.gpu_fb_w_read_data,
                    last: mo.gpu_fb_w_response_last,
                    error: mo.gpu_fb_w_error,
                }),
            },
        ];
        let o = edge(&mut b, &mut v, i);
        assert!(!o.fault, "physical boundary fault at{wall}");
        for (k, h) in held.iter_mut().enumerate() {
            if let Some(r) = *h {
                assert_eq!(o.physical[k], Some(r), "presented physical request changed");
            }
            *h = o.physical[k].filter(|_| !i.physical[k].request_ready);
            if let Some(r) = o.physical[k].filter(|_| i.physical[k].request_ready) {
                assert!(r.valid());
                occupied |= 1 << k;
                if k == 0 {
                    classes[r.line_count_minus_1 as usize] += 1;
                }
            }
            if i.physical[k].response.is_some_and(|r| r.last || r.error) {
                occupied &= !(1 << k);
            }
        }
        max_occupied = max_occupied.max(occupied.count_ones());
        if wall % 17 < 14 {
            paused_returns += usize::from(o.geometry.write.is_some())
                + usize::from(o.texture.read.is_some())
                + usize::from(o.framebuffer.read.is_some());
        }
        ga |= o.geometry.descriptor_ready && i.geometry.descriptor.is_some();
        if let Some((a, d)) = o.geometry.write {
            assert!(writes.insert(a, d).is_none());
        }
        if let Some(c) = o
            .geometry
            .completion
            .filter(|_| i.geometry.completion_ready)
        {
            assert!(c.publish());
            assert_eq!(writes, expected(descriptor(job)));
            writes.clear();
            job += 1;
            ga = false;
        }
        if o.texture.request_ready {
            assert!(i.texture.is_some());
            ta = true;
        }
        if let Some((n, d)) = o.texture.read {
            assert_eq!(n, tex_beats);
            assert_eq!(
                d,
                pattern(0x2000 + (ti % 8) as u32 * 128 + u32::from(n) * 8)
            );
            tex_beats += 1;
        }
        if let Some(ok) = o.texture.complete {
            assert!(ok);
            assert_eq!(tex_beats, 16);
            tex_beats = 0;
            ti += 1;
            ta = false;
        }
        if o.framebuffer.request_ready {
            assert!(i.framebuffer.is_some());
            fa = true;
        }
        if o.framebuffer.write_ready {
            assert!(i.framebuffer_write.is_some());
            wb += 1;
            write_count += 1;
        }
        if let Some((n, d)) = o.framebuffer.read {
            assert_eq!(n, rb);
            assert_eq!(d, write_pattern(fi / 2, n));
            rb += 1;
            read_count += 1;
        }
        if let Some(ok) = o.framebuffer.complete {
            assert!(ok);
            assert_eq!(if fb_write { wb } else { rb }, 16);
            fi += 1;
            fa = false;
            wb = 0;
            rb = 0;
        }
        mc.tick(&mi).unwrap();
        if job == 6 && ti == 20 && fi == 16 && !o.busy {
            assert!(max_occupied >= 2);
            assert!(paused_returns > 100);
            assert!(classes[0] > 0 && classes[1] > 0 && classes[3] > 0);
            for q in 0..8 {
                for n in 0..16 {
                    let a = 0x4000 + q * 128 + n * 8;
                    assert_eq!(
                        &mc.bridge.pins.bytes()[a..a + 8],
                        &write_pattern(q, n as u8).to_le_bytes()
                    );
                }
            }
            println!("physical jobs6 texture20 FB16 edges{} max_outstanding{max_occupied} CE0returns{paused_returns} FBread{read_count} FBwrite{write_count} classes{classes:?}",v.len());
            return v;
        }
    }
    panic!("three-master physical bounded timeout job{job} ti{ti} fi{fi}");
}

#[test]
fn address_contract_and_real_shared_memory_three_masters() {
    for a in [0, 128, 0x7fff80] {
        assert!(valid_sector(a));
    }
    for a in [1, 127, 0x7fffff, 0x800000, 0x80000000, u32::MAX] {
        assert!(!valid_sector(a));
    }
    assert_eq!(MODULE, "gpu_vliw_memory");
    let _ = physical();
}

fn synthetic() -> Vec<Edge> {
    let mut vectors = Vec::new();
    // Each source owns its own terminal event. No timeout fabricates a LAST.
    for case in 0..18 {
        let mut b = Boundary::default();
        let mut active: [Option<(PhysicalRequest, u8)>; 3] = [None; 3];
        let mut held = [None; 3];
        let mut admitted = false;
        let mut source_done = false;
        let mut abort_seen = false;
        let mut writes = 0usize;
        let mut read_beats = 0usize;
        let mut write_beats = 0u8;
        let mut memory_requests = 0usize;
        let mut terminal_seen = false;
        let geometry = case >= 14;
        let fb = (6..14).contains(&case);
        let writing = (10..14).contains(&case);
        let d = descriptor(1);
        let address = match case {
            4 => 129,
            5 => 0x800000,
            13 => 0xffff_ff80,
            _ => 0x1000,
        };
        edge(
            &mut b,
            &mut vectors,
            Input {
                reset: true,
                ..Input::default()
            },
        );
        for wall in 0..2000 {
            let mut i = Input {
                geometry: dma::Input {
                    descriptor: (!admitted && geometry).then_some(d),
                    page_writable: true,
                    gpu_drained: true,
                    completion_ready: wall % 19 >= 11,
                    ..dma::Input::default()
                },
                texture: (!geometry && !fb && !admitted).then_some(address),
                framebuffer: (fb && !admitted).then_some(BackendRequest {
                    address_bytes: address,
                    write: writing,
                }),
                framebuffer_write: (writing
                    && write_beats < 16
                    && !(case == 12 && write_beats == 4))
                    .then_some(write_pattern(case, write_beats)),
                ..Input::default()
            };
            let mut bus_terminal = [false; 3];
            for (k, slot) in active.iter_mut().enumerate() {
                i.physical[k].request_ready = wall >= 18 && wall % 3 != 0;
                if let Some((request, n)) = *slot {
                    if k == 2 {
                        if (case == 11 && n == 2) || n == 16 {
                            i.physical[k].response = Some(dma::Response {
                                last: true,
                                ..dma::Response::default()
                            });
                            bus_terminal[k] = true;
                        } else if wall % 3 != 0 {
                            i.physical[k].write_ready = true;
                            *slot = Some((request, n + 1));
                        }
                    } else if wall % 4 != 0 {
                        let early = matches!(case, 1 | 7) && n == 1;
                        let late = matches!(case, 2 | 8 | 16);
                        let error = matches!(case, 3 | 9 | 15) && n == 1;
                        let last = early
                            || (!late && n + 1 == request.beats())
                            || (late && n == request.beats());
                        i.physical[k].response = Some(dma::Response {
                            data: pattern(request.address_halfword * 2 + u32::from(n) * 8),
                            last,
                            error,
                        });
                        bus_terminal[k] = last || error;
                        *slot = Some((request, n + 1));
                    }
                }
            }
            if case == 14
                && !abort_seen
                && b.output(&i).physical[0].is_some()
                && !i.physical[0].request_ready
            {
                i.geometry.abort = true;
                abort_seen = true;
            }
            let o = edge(&mut b, &mut vectors, i);
            for (k, h) in held.iter_mut().enumerate() {
                if let Some(r) = *h {
                    assert_eq!(o.physical[k], Some(r), "case{case} blocked request changed");
                }
                *h = o.physical[k].filter(|_| !i.physical[k].request_ready);
                if bus_terminal[k] {
                    active[k] = None;
                    terminal_seen = true;
                }
                if let Some(r) = o.physical[k].filter(|_| i.physical[k].request_ready) {
                    assert!(active[k].is_none());
                    active[k] = Some((r, 0));
                    memory_requests += 1;
                }
            }
            admitted |= if geometry {
                o.geometry.descriptor_ready && i.geometry.descriptor.is_some()
            } else if fb {
                o.framebuffer.request_ready
            } else {
                o.texture.request_ready
            };
            writes += usize::from(o.geometry.write.is_some());
            read_beats +=
                usize::from(o.texture.read.is_some()) + usize::from(o.framebuffer.read.is_some());
            if o.framebuffer.write_ready {
                write_beats += 1;
            }
            let done = if geometry {
                o.geometry.completion.map(|c| c.publish())
            } else if fb {
                o.framebuffer.complete
            } else {
                o.texture.complete
            };
            if let Some(ok) = done {
                if geometry && !i.geometry.completion_ready {
                    continue;
                }
                assert!(active.iter().all(Option::is_none));
                let success = matches!(case, 0 | 6 | 10 | 17);
                assert_eq!(ok, success, "case{case} success classification");
                if matches!(case, 4 | 5 | 13) {
                    assert_eq!(memory_requests, 0);
                    assert!(!terminal_seen);
                } else {
                    assert!(terminal_seen);
                }
                if success {
                    if writing {
                        assert_eq!(write_beats, 16);
                    } else if !geometry {
                        assert_eq!(read_beats, 16);
                    }
                }
                if case == 14 {
                    assert!(abort_seen);
                    assert_eq!(writes, 0);
                    assert_eq!(o.geometry.completion.unwrap().error, dma::ERROR_ABORT);
                }
                if case == 15 {
                    assert_eq!(o.geometry.completion.unwrap().error, dma::ERROR_MEMORY);
                }
                if case == 16 {
                    assert_eq!(o.geometry.completion.unwrap().error, dma::ERROR_PROTOCOL);
                }
                source_done = true;
                break;
            }
        }
        assert!(source_done, "synthetic bounded timeout case{case}");
        // Global reset is legal only together with the physical owner/transport.
        edge(
            &mut b,
            &mut vectors,
            Input {
                reset: true,
                ..Input::default()
            },
        );
    }
    // Reset in the middle of an accepted read, then prove a clean new request.
    let mut b = Boundary::default();
    edge(
        &mut b,
        &mut vectors,
        Input {
            reset: true,
            ..Input::default()
        },
    );
    for _ in 0..3 {
        edge(
            &mut b,
            &mut vectors,
            Input {
                texture: Some(0x1000),
                physical: [PhysicalFeedback {
                    request_ready: true,
                    ..PhysicalFeedback::default()
                }; 3],
                ..Input::default()
            },
        );
    }
    edge(
        &mut b,
        &mut vectors,
        Input {
            physical: [
                PhysicalFeedback {
                    response: Some(dma::Response {
                        data: 7,
                        last: false,
                        error: false,
                    }),
                    ..PhysicalFeedback::default()
                },
                PhysicalFeedback::default(),
                PhysicalFeedback::default(),
            ],
            ..Input::default()
        },
    );
    edge(
        &mut b,
        &mut vectors,
        Input {
            reset: true,
            ..Input::default()
        },
    );
    let o = edge(&mut b, &mut vectors, Input::default());
    assert!(!o.busy && !o.fault);
    assert!(o.texture.complete.is_none());
    println!(
        "synthetic cases18 + in-flight global reset, edges{}",
        vectors.len()
    );
    vectors
}

#[test]
fn malformed_abort_and_global_reset_terminate_only_their_owner() {
    let _ = synthetic();
}

// Fields are serialized by name; the RTL harness checks every active payload and
// every control output immediately before the same physical rising edge.
struct Field {
    name: &'static str,
    width: usize,
    value: u64,
    output: bool,
    guard: &'static str,
}
fn fields(i: Input, o: Output) -> Vec<Field> {
    let mut v = Vec::new();
    macro_rules! f {
        ($name:literal,$w:expr,$value:expr) => {
            v.push(Field {
                name: $name,
                width: $w,
                value: ($value) as u64,
                output: false,
                guard: "1'b1",
            })
        };
    }
    macro_rules! e {
        ($name:literal,$w:expr,$value:expr,$guard:literal) => {
            v.push(Field {
                name: $name,
                width: $w,
                value: ($value) as u64,
                output: true,
                guard: $guard,
            })
        };
    }
    let d = i.geometry.descriptor.unwrap_or_default();
    f!("reset", 1, i.reset);
    f!("geometry_abort", 1, i.geometry.abort);
    f!("descriptor_valid", 1, i.geometry.descriptor.is_some());
    f!("page_writable", 1, i.geometry.page_writable);
    f!("gpu_drained", 1, i.geometry.gpu_drained);
    f!("descriptor_page", 1, d.page);
    f!("descriptor_tag", 8, d.tag);
    f!("config_source", 22, d.config.source_halfword);
    f!("metadata_source", 22, d.metadata_source_halfword);
    f!("payload_source", 22, d.payload.source_halfword);
    f!("config_destination", 9, d.config.destination_word);
    f!("metadata_destination", 9, d.metadata_destination_word);
    f!("payload_destination", 9, d.payload.destination_word);
    f!("config_bytes", 11, d.config.bytes);
    f!("payload_bytes", 11, d.payload.bytes);
    f!("completion_ready", 1, i.geometry.completion_ready);
    f!("texture_request_valid", 1, i.texture.is_some());
    f!("texture_request_address", 32, i.texture.unwrap_or(0));
    f!("framebuffer_request_valid", 1, i.framebuffer.is_some());
    f!(
        "framebuffer_request_write",
        1,
        i.framebuffer.is_some_and(|r| r.write)
    );
    f!(
        "framebuffer_request_address",
        32,
        i.framebuffer.unwrap_or_default().address_bytes
    );
    f!("framebuffer_write_valid", 1, i.framebuffer_write.is_some());
    f!(
        "framebuffer_write_data",
        64,
        i.framebuffer_write.unwrap_or(0)
    );
    macro_rules! physical {
        ($k:expr,$q:literal,$v:literal,$l:literal,$e:literal,$d:literal) => {
            let p = i.physical[$k];
            let r = p.response.unwrap_or_default();
            f!($q, 1, p.request_ready);
            f!($v, 1, p.response.is_some());
            f!($l, 1, r.last);
            f!($e, 1, r.error);
            f!($d, 64, r.data);
        };
    }
    physical!(
        0,
        "ro_request_ready",
        "ro_response_valid",
        "ro_response_last",
        "ro_response_error",
        "ro_data"
    );
    physical!(
        1,
        "fb_r_request_ready",
        "fb_r_response_valid",
        "fb_r_response_last",
        "fb_r_response_error",
        "fb_r_data"
    );
    let p = i.physical[2];
    let r = p.response.unwrap_or_default();
    f!("fb_w_request_ready", 1, p.request_ready);
    f!("fb_w_data_ready", 1, p.write_ready);
    f!("fb_w_response_valid", 1, p.response.is_some());
    f!("fb_w_response_last", 1, r.last);
    f!("fb_w_response_error", 1, r.error);
    e!("descriptor_ready", 1, o.geometry.descriptor_ready, "1'b1");
    e!("dma_we", 1, o.geometry.write.is_some(), "1'b1");
    let w = o.geometry.write.unwrap_or_default();
    e!("dma_addr", 10, w.0, "dma_we");
    e!("dma_data", 64, w.1, "dma_we");
    let c = o.geometry.completion.unwrap_or_default();
    e!(
        "completion_valid",
        1,
        o.geometry.completion.is_some(),
        "1'b1"
    );
    e!("completion_publish", 1, c.publish(), "completion_valid");
    e!("completion_page", 1, c.page, "completion_valid");
    e!("completion_tag", 8, c.tag, "completion_valid");
    e!("completion_error", 4, c.error, "completion_valid");
    macro_rules! response {
        ($r:expr,$ready:literal,$valid:literal,$data:literal,$done:literal,$ok:literal) => {
            let r = $r;
            e!($ready, 1, r.request_ready, "1'b1");
            e!($valid, 1, r.read.is_some(), "1'b1");
            e!($data, 64, r.read.unwrap_or_default().1, $valid);
            e!($done, 1, r.complete.is_some(), "1'b1");
            e!($ok, 1, r.complete.unwrap_or(false), $done);
        };
    }
    response!(
        o.texture,
        "texture_request_ready",
        "texture_read_valid",
        "texture_read_data",
        "texture_complete",
        "texture_ok"
    );
    e!(
        "texture_read_index",
        4,
        o.texture.read.unwrap_or_default().0,
        "texture_read_valid"
    );
    response!(
        o.framebuffer,
        "framebuffer_request_ready",
        "framebuffer_read_valid",
        "framebuffer_read_data",
        "framebuffer_complete",
        "framebuffer_ok"
    );
    e!(
        "framebuffer_write_ready",
        1,
        o.framebuffer.write_ready,
        "1'b1"
    );
    macro_rules! request {
        ($k:expr,$valid:literal,$a:literal,$c:literal) => {
            let r = o.physical[$k].unwrap_or_default();
            e!($valid, 1, o.physical[$k].is_some(), "1'b1");
            e!($a, 22, r.address_halfword, $valid);
            e!($c, 2, r.line_count_minus_1, $valid);
        };
    }
    request!(0, "ro_request_valid", "ro_address", "ro_count");
    request!(1, "fb_r_request_valid", "fb_r_address", "fb_r_count");
    request!(2, "fb_w_request_valid", "fb_w_address", "fb_w_count");
    e!("fb_w_data", 64, o.write_data, "1'b1");
    e!("busy", 1, o.busy, "1'b1");
    e!("fault", 1, o.fault, "1'b1");
    v
}
fn bounded(mut cmd: std::process::Command, dir: &Path, label: &str) {
    let mut c = cmd
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{label}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{label}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(s) = c.try_wait().unwrap() {
            assert!(
                s.success(),
                "{label}: {} {}",
                fs::read_to_string(dir.join(format!("{label}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{label}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = c.kill();
            let _ = c.wait();
            panic!("bounded {label} timeout");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "requires Icarus syntax/dependency validation"]
fn selected_backend_removes_second_arbitration_in_new_factory() {
    let slots = [texture::ports::Slot {
        base_address: 0,
        max_size_log2: 5,
        has_full_mip: true,
        valid: true,
    }];
    let surface = framebuffer::ports::MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x200000,
        width: 400,
        height: 240,
    };
    let old =
        backend::verilog_selected(&slots, surface, quad_ingress::rtl::Backend::Portable).unwrap();
    assert!(old.contains(backend::MODULE));
    let new = split_backend(&old).unwrap();
    let top = new.split("endmodule").next().unwrap();
    assert!(top.contains(BACKEND_MODULE));
    assert!(!top.contains("arb_state"));
    assert!(!top.split(");").next().unwrap().contains("mem_req_addr"));
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-vliw-integration-20261018/stage3/memory");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("backend-unified.v"), &old).unwrap();
    fs::write(dir.join("backend-three-port.v"), new).unwrap();
    let mut cmd = std::process::Command::new(
        std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
    );
    cmd.current_dir(&dir).args([
        "-g2012",
        "-s",
        BACKEND_MODULE,
        "-o",
        "backend.vvp",
        "backend-three-port.v",
    ]);
    bounded(cmd, &dir, "backend-compile");
    let bad = old.replacen("reg[1:0]arb_state;", "reg[2:0]arb_state;", 1);
    assert!(split_backend(&bad).is_err());
}
#[test]
#[ignore = "requires Icarus RTL simulation"]
fn rtl_matches_real_memory_wall_edges() {
    let mut v = synthetic();
    v.extend(physical());
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-vliw-integration-20261018/stage3/memory");
    fs::create_dir_all(&dir).unwrap();
    let schema = fields(v[0].i, v[0].o);
    let mut tb = String::from("`timescale 1ns/1ps\nmodule tb;reg clk=0;always #5 clk=~clk;\n");
    let mut names = String::new();
    let mut fmt = String::new();
    let mut checks = String::new();
    for f in &schema {
        if f.output {
            writeln!(
                tb,
                "wire[{}:0]{};reg[{}:0]e_{};",
                f.width - 1,
                f.name,
                f.width - 1,
                f.name
            )
            .unwrap();
            write!(names, ",e_{}", f.name).unwrap();
            writeln!(
                checks,
                "if(({})&&{}!==e_{})$fatal(1,\"{} edge%0d got%h expected%h\",n,{},e_{});",
                f.guard, f.name, f.name, f.name, f.name, f.name
            )
            .unwrap();
        } else {
            writeln!(tb, "reg[{}:0]{};", f.width - 1, f.name).unwrap();
            write!(names, ",{}", f.name).unwrap();
        }
        fmt.push_str("%h ");
    }
    tb.push_str("gpu_vliw_memory dut(.clk(clk)");
    for f in &schema {
        write!(tb, ",.{}({})", f.name, f.name).unwrap();
    }
    tb.push_str(
        ");\ninteger fd,n,k;initial begin fd=$fopen(\"vectors.txt\",\"r\");if(!fd)$fatal;\n",
    );
    writeln!(tb,"for(n=0;n<{};n=n+1)begin @(negedge clk);k=$fscanf(fd,\"{fmt}\\n\"{names});if(k!={})$fatal;#1;if(n>0)begin\n{checks}\nend @(posedge clk);#1;end $display(\"PASS MEMORY edges={}\");$finish;end\ninitial begin #{};$fatal(1,\"absolute watchdog\");end\nendmodule",v.len(),schema.len(),v.len(),v.len()*10+1000).unwrap();
    let mut vectors = String::new();
    for x in &v {
        for f in fields(x.i, x.o) {
            write!(vectors, "{:x} ", f.value).unwrap();
        }
        vectors.push('\n');
    }
    fs::write(dir.join("tb.v"), tb).unwrap();
    fs::write(dir.join("vectors.txt"), vectors).unwrap();
    fs::write(dir.join("memory.v"), verilog()).unwrap();
    let mut cmd = std::process::Command::new(
        std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
    );
    cmd.current_dir(&dir)
        .args(["-g2005", "-s", "tb", "-o", "test.vvp", "memory.v", "tb.v"]);
    bounded(cmd, &dir, "compile");
    let mut cmd =
        std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    cmd.current_dir(&dir).arg("test.vvp");
    bounded(cmd, &dir, "run");
    println!("{}", fs::read_to_string(dir.join("run.out")).unwrap());
}
