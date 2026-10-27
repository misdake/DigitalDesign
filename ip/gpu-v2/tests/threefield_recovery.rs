//! Finite independent wide-integer arithmetic and actual registered portable/native playback.
use gpu_v2::triangle::{
    basecolor_threefield::{PreparedBasis, Profile},
    basecolor_uv12,
    recovery::{self, Delta, Descriptor, Emu, Inputs, Outputs, Params, Pixel, Registers, Weight},
};
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[path = "common/basecolor_uv12_fixture.rs"]
mod common;
const MAX_TRACE: usize = 60000;
fn weight(w: Weight) -> u128 {
    u128::from(w.normal)
        | (u128::from(w.uv_high) << 9)
        | (u128::from(w.uv_low) << 18)
        | (u128::from(w.negative) << 21)
        | (u128::from(w.exponent) << 22)
        | (u128::from(w.normal_endpoint) << 26)
        | (u128::from(w.uv_endpoint) << 27)
}
fn compute_weight(w: Weight, p: Params) -> u128 {
    (weight(w) & ((1 << 22) - 1))
        | (u128::from(w.exponent) << 22)
        | (u128::from(w.normal_endpoint) << (22 + p.exponent_bits()))
        | (u128::from(w.uv_endpoint) << (23 + p.exponent_bits()))
}
fn base(d: Descriptor) -> u128 {
    d.base
        .into_iter()
        .enumerate()
        .fold(0, |v, (g, b)| v | (((b as u16 & 4095) as u128) << (g * 12)))
}
fn delta(d: Descriptor, j: usize, p: Params) -> u128 {
    d.delta[j].into_iter().enumerate().fold(0, |v, (g, b)| {
        let m = if g < 3 || (d.texture && !p.exact_uv) {
            b.magnitude | (u16::from(b.exponent) << 9)
        } else {
            b.magnitude
        };
        v | ((u128::from(m) | (u128::from(b.negative) << 12)) << (g * 13))
    })
}
fn mask(a: [bool; 6]) -> u128 {
    a.into_iter()
        .enumerate()
        .fold(0, |v, (g, b)| v | (u128::from(b) << g))
}
fn packed(items: &[(u128, usize)]) -> String {
    let mut bits = String::new();
    for (v, n) in items {
        for bit in (0..*n).rev() {
            bits.push(if (v >> bit) & 1 == 1 { '1' } else { '0' });
        }
    }
    while !bits.len().is_multiple_of(4) {
        bits.insert(0, '0');
    }
    bits.as_bytes()
        .chunks(4)
        .map(|b| {
            char::from_digit(b.iter().fold(0, |v, b| v * 2 + u32::from(*b == b'1')), 16).unwrap()
        })
        .collect()
}
fn ins(i: Inputs, p: Params) -> Vec<(u128, usize)> {
    let d = i.config.unwrap_or_default();
    let x = i.pixel.unwrap_or_default();
    vec![
        (i.reset as u128, 1),
        (i.ce as u128, 1),
        (i.config.is_some() as u128, 1),
        (d.texture as u128, 1),
        (base(d), 72),
        (delta(d, 0, p), 78),
        (delta(d, 1, p), 78),
        (mask(d.active), 6),
        (i.pixel.is_some() as u128, 1),
        (weight(x.beta[0]), 28),
        (weight(x.beta[1]), 28),
        (x.token as u128, p.token_bits as usize),
        (x.fallback as u128, 1),
        (i.result_ready as u128, 1),
    ]
}
fn offers(o: Outputs, p: Params) -> Vec<(u128, usize)> {
    let mut v = vec![
        (o.config_ready as u128, 1),
        (o.config_error as u128, 1),
        (o.pixel_ready as u128, 1),
        (o.result_valid as u128, 1),
        (o.token as u128, p.token_bits as usize),
        (o.fallback as u128, 1),
    ];
    v.extend(
        o.data
            .into_iter()
            .rev()
            .map(|d| (d as u128, p.width() as usize)),
    );
    v
}
fn registers(r: &Registers, p: Params) -> Vec<(u128, usize)> {
    let d = r.descriptor;
    let mut v = vec![
        (r.phase as u128, 1),
        (r.loaded as u128, 1),
        (r.config_error as u128, 1),
        (d.texture as u128, 1),
        (base(d), 72),
        (delta(d, 0, p), 78),
        (delta(d, 1, p), 78),
        (mask(d.active), 6),
        (compute_weight(r.held, p), 24 + p.exponent_bits() as usize),
        (r.ret_valid as u128, 1),
        (r.ret_last as u128, 1),
        (r.ret_token as u128, p.token_bits as usize),
        (r.ret_fallback as u128, 1),
        (r.ret_normal_endpoint as u128, 1),
        (r.ret_uv_endpoint as u128, 1),
        (mask(r.ret_sign), 6),
    ];
    v.extend(r.ret_shift.into_iter().rev().map(|x| (x as u128, 4)));
    v.extend(r.products.into_iter().rev().map(|x| (x as u128, 18)));
    v.extend(r.correction.into_iter().rev().map(|x| (x as u128, 24)));
    v.extend(
        r.accumulator
            .into_iter()
            .rev()
            .map(|x| (x as u128, p.width() as usize)),
    );
    v.extend([
        (r.result_valid as u128, 1),
        (r.result_token as u128, p.token_bits as usize),
        (r.result_fallback as u128, 1),
    ]);
    v.extend(
        r.result
            .into_iter()
            .rev()
            .map(|x| (x as u128, p.width() as usize)),
    );
    v
}
#[derive(Clone, Debug)]
struct Gold {
    token: u16,
    fallback: bool,
    data: [i64; 6],
    admitted: usize,
}
fn reference(d: Descriptor, x: Pixel, p: Params, admitted: usize) -> Gold {
    // Full signed integer products, no DSP split/shift-add helper or emulator step is called.
    let fallback = x.fallback
        || x.beta.iter().any(|w| {
            w.exponent > p.max_weight_exp
                || w.normal_endpoint && w.normal != 0
                || d.texture
                    && (w.uv_low >= 1 << (p.uv_bits - 9)
                        || w.uv_endpoint && (w.uv_high != 0 || w.uv_low != 0))
        });
    let mut data = [0i64; 6];
    if !fallback {
        for (g, destination) in data.iter_mut().enumerate() {
            let mut sum = i128::from(d.base[g]) << 16;
            if d.active[g] {
                for j in 0..2 {
                    let uv = d.texture && (g == 3 || g == 4);
                    let bits = if uv { p.uv_bits } else { 9 };
                    let w = x.beta[j];
                    let a = d.delta[j][g];
                    let endpoint = if uv { w.uv_endpoint } else { w.normal_endpoint };
                    let code = if endpoint {
                        1u32 << bits
                    } else if uv {
                        (u32::from(w.uv_high) << (bits - 9)) | u32::from(w.uv_low)
                    } else {
                        u32::from(w.normal)
                    };
                    let signed_delta = if a.negative {
                        -i128::from(a.magnitude)
                    } else {
                        i128::from(a.magnitude)
                    };
                    let signed_code = if w.negative {
                        -i128::from(code)
                    } else {
                        i128::from(code)
                    };
                    sum += (signed_delta * signed_code) << (16 - bits + a.exponent + w.exponent);
                }
            }
            // Independent F16 gold has no bits below F(UV_BITS): no rounding is needed.
            assert_eq!(sum & ((1 << (16 - p.uv_bits)) - 1), 0);
            *destination = i64::try_from(sum).unwrap();
        }
    }
    Gold {
        token: x.token,
        fallback,
        data,
        admitted,
    }
}
struct Driver {
    emu: Emu,
    descriptor: Descriptor,
    gold: VecDeque<Gold>,
    trace: String,
    wall: usize,
    accepted: usize,
    returned: usize,
    fallbacks: usize,
    ce0: [usize; 4],
    blocked: usize,
    max_latency: usize,
}
impl Driver {
    fn new(p: Params) -> Self {
        Self {
            emu: Emu::new(p),
            descriptor: Descriptor::default(),
            gold: VecDeque::new(),
            trace: String::new(),
            wall: 0,
            accepted: 0,
            returned: 0,
            fallbacks: 0,
            ce0: [0; 4],
            blocked: 0,
            max_latency: 0,
        }
    }
    fn step(&mut self, i: Inputs) -> Outputs {
        assert!(self.wall < MAX_TRACE);
        let p = self.emu.params;
        let before = self.emu.regs.clone();
        let o = self.emu.outputs(&i);
        if o.result_valid {
            let g = self.gold.front().expect("unowned result");
            let restored_f16 = o.data.map(|v| v << (16 - p.uv_bits));
            assert_eq!(
                (o.token, o.fallback, restored_f16),
                (g.token, g.fallback, g.data)
            );
        }
        if i.reset {
            self.gold.clear();
            self.descriptor = Descriptor::default();
        } else if i.ce {
            if o.result_valid && i.result_ready {
                let g = self.gold.pop_front().unwrap();
                self.max_latency = self.max_latency.max(self.wall - g.admitted);
                self.returned += 1;
                self.fallbacks += usize::from(g.fallback);
            }
            if let Some(d) = i
                .config
                .filter(|_| o.config_ready && d_valid(i.config.unwrap(), p))
            {
                self.descriptor = d;
            }
            if let Some(x) = i.pixel.filter(|_| o.pixel_ready) {
                self.gold
                    .push_back(reference(self.descriptor, x, p, self.wall));
                self.accepted += 1;
            }
        }
        let actual = self.emu.tick(i);
        assert_eq!(actual, o);
        if !i.reset && (!i.ce || o.result_valid && !i.result_ready) {
            assert_eq!(before, self.emu.regs);
            if !i.ce {
                self.ce0[usize::from(before.phase) + 2 * usize::from(before.ret_valid)] += 1;
            } else {
                self.blocked += 1;
            }
        }
        self.trace.push_str(&format!(
            "{} {} {}\n",
            packed(&ins(i, p)),
            packed(&offers(o, p)),
            packed(&registers(&self.emu.regs, p))
        ));
        self.wall += 1;
        o
    }
    fn drain(&mut self) {
        for _ in 0..20 {
            if self.emu.outputs(&Inputs::default()).config_ready {
                assert!(self.gold.is_empty());
                return;
            }
            self.step(Inputs {
                ce: true,
                result_ready: true,
                ..Default::default()
            });
        }
        panic!("drain budget");
    }
    fn bind(&mut self, d: Descriptor) {
        self.drain();
        self.step(Inputs {
            ce: true,
            config: Some(d),
            ..Default::default()
        });
        assert!(self.emu.regs.loaded);
    }
    fn stream(&mut self, pixels: &[Pixel], sparse: bool) -> (Vec<usize>, Vec<usize>) {
        let mut at = 0;
        let (mut ai, mut oi) = (Vec::new(), Vec::new());
        let start = self.wall;
        for turn in 0..pixels.len() * 8 + 100 {
            let ce = !sparse || turn % 11 != 2 && turn % 11 != 3;
            let ready = !sparse || turn % 13 < 9;
            let offer = at < pixels.len() && (!sparse || turn % 7 != 0);
            let i = Inputs {
                ce,
                pixel: offer.then(|| pixels[at]),
                result_ready: ready,
                ..Default::default()
            };
            let o = self.step(i);
            if ce && o.pixel_ready && offer {
                ai.push(self.wall - 1 - start);
                at += 1;
            }
            if ce && o.result_valid && ready {
                oi.push(self.wall - 1 - start);
            }
            if at == pixels.len() && self.emu.outputs(&Inputs::default()).config_ready {
                assert!(self.gold.is_empty());
                return (ai, oi);
            }
        }
        panic!("stream budget");
    }
}
fn d_valid(d: Descriptor, p: Params) -> bool {
    d.valid(p)
}
fn run(mut cmd: Command, dir: &Path, label: &str) {
    let log = fs::File::create(dir.join(format!("{label}.log"))).unwrap();
    cmd.current_dir(dir)
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log));
    let mut child = cmd.spawn().unwrap();
    let t = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            assert!(
                s.success(),
                "{}",
                fs::read_to_string(dir.join(format!("{label}.log"))).unwrap()
            );
            break;
        }
        if t.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("simulator watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn hdl(dir: &Path, d: &Driver, native: bool) {
    let p = d.emu.params;
    let label = if native { "native" } else { "portable" };
    let source = if native {
        recovery::native_rtl(p)
    } else {
        recovery::portable_rtl(p)
    };
    fs::write(dir.join(format!("{label}.v")), source).unwrap();
    let iw = ins(Inputs::default(), p).iter().map(|x| x.1).sum::<usize>();
    let ow = offers(Outputs::default(), p)
        .iter()
        .map(|x| x.1)
        .sum::<usize>();
    let rw = registers(&Registers::default(), p)
        .iter()
        .map(|x| x.1)
        .sum::<usize>();
    let lane_concat = |name: &str| {
        (0..6)
            .rev()
            .map(|g| format!("dut.{name}[{g}]"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let regs = format!(
        "dut.phase,dut.loaded,dut.config_error,dut.texture_q,dut.bases,dut.deltas1,dut.deltas2,dut.active,dut.held_beta,dut.ret_valid,dut.ret_last,dut.ret_token,dut.ret_fallback,dut.ret_normal_endpoint,dut.ret_uv_endpoint,dut.ret_sign,{},{},{},{},dut.result_valid,dut.result_token,dut.result_fallback,dut.result_data",
        lane_concat("ret_shift"),
        lane_concat("products"),
        lane_concat("correction"),
        lane_concat("accumulator")
    );
    fs::write(dir.join(format!("tb-{label}.v")),format!(r#"`timescale 1ns/1ps
module tb;
reg clk=0,reset,ce,config_valid,config_texture,pixel_valid,pixel_fallback,result_ready;
reg [71:0]config_base;reg [77:0]config_delta1,config_delta2;reg[5:0]config_active;reg[27:0]beta1,beta2;reg[{tw}:0]pixel_token;
wire config_ready,config_error,pixel_ready,result_valid,result_fallback;wire[{tw}:0]result_token;wire[{dw}:0]result_data;
gpu_v2_threefield_recovery dut(.*);{gsr}
reg[{iw}:0]ins;reg[{ow}:0]expected_offer;reg[{rw}:0]expected_regs;
wire[{ow}:0]actual_offer={{config_ready,config_error,pixel_ready,result_valid,result_token,result_fallback,result_data}};
wire[{rw}:0]actual_regs={{{regs}}};
integer fd,n,k;initial begin fd=$fopen("trace.txt","r");for(k=0;k<{cycles};k=k+1)begin
n=$fscanf(fd,"%h %h %h\n",ins,expected_offer,expected_regs);if(n!=3)$fatal(1,"trace");
{{reset,ce,config_valid,config_texture,config_base,config_delta1,config_delta2,config_active,pixel_valid,beta1,beta2,pixel_token,pixel_fallback,result_ready}}=ins;
#4;if(k>0&&actual_offer!==expected_offer)begin $display("offer k=%0d actual=%h expected=%h",k,actual_offer,expected_offer);$fatal(1);end
#1;clk=1;#5;if(actual_regs!==expected_regs)begin $display("regs k=%0d actual=%h expected=%h",k,actual_regs,expected_regs);$fatal(1);end clk=0;#1;
end $display("PASS {label} cycles={cycles} trace_bits={bits}");$finish;end
initial begin #1000000;$fatal(1,"global watchdog");end
endmodule
"#,tw=p.token_bits-1,dw=6*p.width() as usize-1,iw=iw-1,ow=ow-1,rw=rw-1,regs=regs,cycles=d.wall,bits=rw,gsr=if native{"GSR GSR(.GSRI(1'b1));"}else{""})).unwrap();
    let mut cmd = Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()));
    cmd.args([
        "-g2012",
        "-s",
        "tb",
        "-o",
        &format!("{label}.vvp"),
        &format!("tb-{label}.v"),
        &format!("{label}.v"),
    ]);
    if native {
        let home = std::env::var_os("GOWIN_HOME")
            .map(std::path::PathBuf::from)
            .expect("GOWIN_HOME is required for native primitive simulation");
        cmd.arg(home.join("IDE/simlib/gw2a/prim_sim.v"));
    }
    run(cmd, dir, &format!("compile-{label}"));
    let mut cmd = Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()));
    cmd.arg(format!("{label}.vvp"));
    run(cmd, dir, &format!("simulate-{label}"));
}
#[test]
fn parameterized_fraction_and_decoded_delta_bound() {
    for uv_bits in 9..=12 {
        for max_weight_exp in 0..=3 {
            for exact_uv in [true, false] {
                let p = Params {
                    uv_bits,
                    max_weight_exp,
                    exact_uv,
                    token_bits: 16,
                };
                let bound = (4095i128 + 2 * 4096 * (1 << max_weight_exp)) << uv_bits;
                assert!(bound < 1i128 << (p.width() - 1));
                assert!(bound >= 1i128 << (p.width() - 2));
                let mut desc = Descriptor {
                    texture: true,
                    base: [2047, -2048, 0, 4095, 0, 0],
                    active: [true, true, true, true, true, false],
                    ..Default::default()
                };
                for g in 0..5 {
                    for j in 0..2 {
                        desc.delta[j][g] = if g >= 3 && exact_uv {
                            Delta {
                                magnitude: 4095,
                                negative: true,
                                exponent: 0,
                            }
                        } else {
                            Delta {
                                magnitude: 256,
                                negative: true,
                                exponent: 4,
                            }
                        };
                    }
                }
                let mut d = Driver::new(p);
                d.step(Inputs {
                    reset: true,
                    ..Default::default()
                });
                d.bind(desc);
                let endpoint = (1u32 << max_weight_exp) as f64;
                let vectors = [
                    recovery::packet([0.0, -endpoint, -endpoint], 65535, p),
                    recovery::packet([0.0, endpoint, endpoint], 0, p),
                    recovery::packet([0.0, 511.0 / 512.0, 4095.0 / 4096.0], 32768, p),
                ];
                d.stream(&vectors, true);
                assert_eq!(d.fallbacks, 0);
                for exponent in 0..=7 {
                    for magnitude in 0..=511 {
                        let mut candidate = desc;
                        candidate.delta[0][0] = Delta {
                            exponent,
                            magnitude,
                            negative: false,
                        };
                        let legal = exponent <= 4 && ((magnitude as u32) << exponent) <= 4096;
                        assert_eq!(candidate.valid(p), legal);
                    }
                }
            }
        }
    }
}
#[test]
fn bounded_fixed_lane_recovery_steady_ii2_and_dual_rtl() {
    let root = std::env::var_os("RECOVERY_QUAL_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or("target/recovery-qualification".into());
    {
        let exact_uv = true;
        let p = Params {
            exact_uv,
            ..Default::default()
        };
        let mut d = Driver::new(p);
        d.step(Inputs {
            reset: true,
            ce: false,
            ..Default::default()
        });
        let mut texture = Descriptor {
            texture: true,
            base: [2047, -2048, 1000, 4095, 0, 0],
            active: [true, true, true, true, true, false],
            ..Default::default()
        };
        for j in 0..2 {
            for g in 0..5 {
                texture.delta[j][g] = if g < 3 || !exact_uv {
                    Delta {
                        magnitude: 256,
                        negative: true,
                        exponent: 4,
                    }
                } else {
                    Delta {
                        magnitude: 4095,
                        negative: true,
                        exponent: 0,
                    }
                };
            }
        }
        d.bind(texture);
        let headroom = recovery::packet([17.0, -8.0, -8.0], 1, p);
        let gold = reference(texture, headroom, p, 0);
        assert!(gold.data[0] > (1i64 << 32));
        assert!(gold.data[3] > (1i64 << 32));
        // F12 cannot fit signed29; signed30 is derived rather than fitted to examples.
        assert!((gold.data[0] >> 4) > (1i64 << 28));
        assert_eq!(p.width(), 30);
        d.stream(&[headroom], false);
        let dense = (0..1024)
            .map(|n| {
                recovery::packet(
                    [
                        0.0,
                        if n % 9 == 0 { -8.0 } else { n as f64 / 128.0 },
                        if n % 7 == 0 { 2.0 } else { -(n as f64 / 256.0) },
                    ],
                    ((n + 10) & 127) as u16,
                    p,
                )
            })
            .collect::<Vec<_>>();
        let (ai, oi) = d.stream(&dense, false);
        assert_eq!(ai.len(), 1024);
        assert_eq!(oi.len(), 1024);
        assert!(ai.windows(2).all(|a| a[1] - a[0] == 2));
        assert!(oi.windows(2).all(|a| a[1] - a[0] == 2));
        assert_eq!(oi[0] - ai[0], 3);
        let mut helper_points = 0;
        let mut negative = 0;
        let mut max_exponent = 0;
        for scene in 0..common::SCENES {
            for frame in 0..3 {
                let (r, a, m) = common::fixture(scene, frame as f64 / 3.0);
                let b = PreparedBasis::prepare(&r, a, m).unwrap();
                d.bind(recovery::prepare(&b, p));
                let mut pixels = Vec::new();
                for y in (16..224).step_by(42) {
                    for x in (16..384).step_by(62) {
                        let mut gold_uv = Vec::new();
                        let mut numeric_uv = Vec::new();
                        for lane in 0..4 {
                            let point = [
                                x as f64 + 0.5 + (lane % 2) as f64,
                                y as f64 + 0.5 + (lane / 2) as f64,
                            ];
                            let Ok(exact) = b.recover(point, Profile::Exact, false) else {
                                continue;
                            };
                            let beta = exact.beta;
                            let x =
                                recovery::packet(beta, ((3000 + helper_points) & 127) as u16, p);
                            let wide = reference(d.descriptor, x, p, 0);
                            if !wide.fallback {
                                let n = basecolor_uv12::recover(&b, point, Some(12), true).unwrap();
                                let normal = n.normal(m);
                                for (g, expected) in normal.into_iter().enumerate() {
                                    assert!(
                                        (wide.data[g] as f64 / 65536.0 / 1024.0 - expected).abs()
                                            < 1e-10
                                    );
                                }
                                if d.descriptor.texture {
                                    let u = basecolor_uv12::recover(&b, point, Some(12), !exact_uv)
                                        .unwrap();
                                    let actual = [
                                        wide.data[3] as f64 / 65536.0 / 4095.0,
                                        wide.data[4] as f64 / 65536.0 / 4095.0,
                                    ];
                                    for (actual, expected) in actual.into_iter().zip(u.uv()) {
                                        assert!((actual - expected).abs() < 1e-10);
                                    }
                                    gold_uv.push(actual);
                                    numeric_uv.push(u.uv());
                                } else {
                                    for (g, expected) in n.rgb().into_iter().enumerate() {
                                        assert!(
                                            (wide.data[g + 3] as f64 / 65536.0 / 255.0 - expected)
                                                .abs()
                                                < 1e-10
                                        );
                                    }
                                }
                            }
                            negative += usize::from(beta.iter().any(|b| *b < 0.0));
                            max_exponent =
                                max_exponent.max(x.beta.iter().map(|w| w.exponent).max().unwrap());
                            pixels.push(x);
                            helper_points += 1;
                        }
                        if gold_uv.len() == 4 {
                            assert!(
                                (common::lod(gold_uv.try_into().unwrap(), 256.0)
                                    - common::lod(numeric_uv.try_into().unwrap(), 256.0))
                                .abs()
                                    < 1e-12
                            );
                        }
                    }
                }
                d.stream(&pixels, true);
            }
        }
        // Original legal thin-source helper weights beyond the local exponent bound require fallback.
        let fallback = recovery::packet([-63.0, 32.0, 32.0], 0, p);
        d.stream(&[fallback], true);
        let external_fallback = recovery::packet([-131071.0, 65536.0, 65536.0], 127, p);
        assert!(external_fallback.fallback);
        d.stream(&[external_fallback], true);
        // Full transport exponent must classify before low-bit compute storage.
        // In particular exponent4 has low2==0 yet must never become an exponent0 answer.
        let unsupported = (4..=15)
            .map(|exponent| Pixel {
                beta: [Weight {
                    exponent,
                    normal: 1,
                    uv_high: 1,
                    ..Default::default()
                }; 2],
                token: exponent as u16,
                fallback: false,
            })
            .collect::<Vec<_>>();
        let before_fallbacks = d.fallbacks;
        d.stream(&unsupported, true);
        assert_eq!(d.fallbacks - before_fallbacks, 12);
        let mut invalid = texture;
        invalid.delta[0][0].exponent = 7;
        d.drain();
        d.step(Inputs {
            ce: true,
            config: Some(invalid),
            ..Default::default()
        });
        assert!(!d.emu.regs.loaded && d.emu.regs.config_error);
        invalid.delta[0][0] = Delta {
            magnitude: 257,
            exponent: 4,
            negative: false,
        };
        d.step(Inputs {
            ce: true,
            config: Some(invalid),
            ..Default::default()
        });
        assert!(!d.emu.regs.loaded && d.emu.regs.config_error);
        d.bind(Descriptor {
            base: [512, -256, 900, 7, 13, 251],
            ..Default::default()
        });
        d.stream(&[recovery::packet([0.0, -4.0, 2.0], 126, p)], true);
        // Reset abandons live ownership; independent expected queue is cleared at the same edge.
        d.bind(texture);
        // A blocked descriptor offer cannot rebind an already admitted pixel.
        d.step(Inputs {
            ce: true,
            pixel: Some(headroom),
            result_ready: true,
            ..Default::default()
        });
        let before = d.descriptor;
        let o = d.step(Inputs {
            ce: true,
            config: Some(Descriptor::default()),
            result_ready: true,
            ..Default::default()
        });
        assert!(!o.config_ready);
        assert_eq!(d.descriptor, before);
        d.drain();
        // Reset while holding a completed output also abandons that token explicitly.
        d.step(Inputs {
            ce: true,
            pixel: Some(headroom),
            result_ready: true,
            ..Default::default()
        });
        d.step(Inputs {
            ce: true,
            result_ready: true,
            ..Default::default()
        });
        d.step(Inputs {
            ce: true,
            result_ready: false,
            ..Default::default()
        });
        assert!(d.emu.regs.result_valid);
        d.step(Inputs {
            reset: true,
            ce: false,
            result_ready: false,
            ..Default::default()
        });
        d.bind(texture);
        d.step(Inputs {
            ce: true,
            pixel: Some(headroom),
            result_ready: true,
            ..Default::default()
        });
        d.step(Inputs {
            reset: true,
            ce: false,
            ..Default::default()
        });
        d.bind(texture);
        d.step(Inputs {
            ce: true,
            pixel: Some(headroom),
            result_ready: true,
            ..Default::default()
        });
        d.step(Inputs {
            ce: true,
            result_ready: true,
            ..Default::default()
        });
        d.step(Inputs {
            reset: true,
            ce: true,
            ..Default::default()
        });
        d.bind(texture);
        let end = [
            recovery::packet([0.0, 511.0 / 512.0, 4095.0 / 4096.0], 125, p),
            recovery::packet([0.0, -1.0, 1.0], 124, p),
        ];
        d.stream(&end, true);
        d.drain();
        assert!(d.ce0[0] > 0 && d.ce0[2] > 0 && d.ce0[3] > 0 && d.blocked > 0);
        assert_eq!(d.accepted - d.returned, 3);
        let dir = root.join(if exact_uv { "exact-uv" } else { "delta9-uv" });
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("trace.txt"), &d.trace).unwrap();
        hdl(&dir, &d, false);
        hdl(&dir, &d, true);
        fs::write(dir.join("qualification.json"),format!("{{\"status\":\"PASS\",\"cycles\":{},\"accepted\":{},\"returned\":{},\"fallback_returns\":{},\"helper_points\":{helper_points},\"negative_helpers\":{negative},\"max_sampled_weight_exponent\":{max_exponent},\"ce0_phase_return\":{:?},\"output_blocked_edges\":{},\"dense_pixels\":1024,\"accepted_spacing\":2,\"result_spacing\":2,\"first_result_ack_latency\":3,\"acc_width\":{},\"fraction_bits\":{},\"token_bits\":{},\"compute_exponent_bits\":{},\"f16_low4_zero\":true,\"all_transport_exponents4_to15_fallback\":true,\"trace_limit\":{MAX_TRACE},\"watchdog_seconds\":60}}\n",d.wall,d.accepted,d.returned,d.fallbacks,d.ce0,d.blocked,p.width(),p.uv_bits,p.token_bits,p.exponent_bits())).unwrap();
        println!(
            "PASS recovery exactUV={exact_uv} pixels={} returned={} cycles={} helper-points={helper_points} negative={negative} fallback={} sustainedII2=1024",
            d.accepted, d.returned, d.wall, d.fallbacks
        );
    }
}
