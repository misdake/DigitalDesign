//! Compiled application/UI data and bounded control peers; no pixel/serial RTL claim.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, StepOutcome};
use std::{
    any::Any,
    cell::RefCell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

#[derive(Default)]
struct State {
    us: u64,
    clear: u64,
    active: u32,
    shadow: u32,
    format: u16,
    shadow_format: u16,
    gpu_until: u64,
    present_until: u64,
    pending: bool,
    starts: usize,
    swaps: usize,
    packet_until: u64,
    changed: bool,
    valid: bool,
    packets: usize,
    during_gpu: usize,
    during_present: usize,
    counts: [u16; 2],
    wire_header: [u16; 3],
    wire_monitor: u16,
}
struct Peer {
    kind: u8,
    s: Rc<RefCell<State>>,
}
impl cpu_v3::Device for Peer {
    fn read(&mut self, _: &mut [u16], ch: u8) -> u16 {
        let mut s = self.s.borrow_mut();
        if self.kind == 3 && s.pending && s.us >= s.present_until {
            s.active = s.shadow;
            s.format = s.shadow_format;
            s.pending = false;
        }
        match (self.kind, ch) {
            (0, 10) => (s.us / 1000) as u16,
            (0, 11) => (s.us - s.clear).min(65535) as u16,
            (3, 0) => (s.us / 16667) as u16,
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 3) => u16::from(s.pending),
            (3, 4) => s.format,
            (4, 2) => u16::from(s.us < s.gpu_until),
            (5, 9..=11) => s.wire_header[usize::from(ch - 9)],
            (5, 12) => s.wire_monitor,
            (5, 8..=15) => u16::from(s.valid) | (u16::from(s.changed) << 1),
            (5, 0..=7) => {
                let response = [0x73u16, 0x5a, 0xa5, 0x5a, 0, 255, 128, 64];
                response[(usize::from(ch) + 6) & 7]
            }
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], ch: u8, value: u16) {
        let mut s = self.s.borrow_mut();
        match (self.kind, ch) {
            (0, 11) if value & 1 != 0 => s.clear = s.us,
            (4, 1) => {
                s.starts += 1;
                s.gpu_until = s.us + 50000;
            }
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(value),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(value) << 16),
            (3, 4) => s.shadow_format = value,
            (3, 3) => {
                assert!(!s.pending);
                s.pending = true;
                s.present_until = s.us + 35000;
                s.swaps += 1;
            }
            (5, 8..=15) => s.changed = false,
            (7, 0) => s.counts[0] = value,
            (7, 1) => s.counts[1] = value,
            (7, 2) => {
                s.valid = value & 1 != 0;
                s.changed = value & 2 != 0;
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn assets(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("../../target/ds2-ui-validation").join(name);
    let r = Command::new("python")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("examples/dualshock2_ui/tools/assets.py"))
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    out
}
fn run(source: &str, entry: &'static str, out: &Path) -> (CpuV3Sim, Rc<RefCell<State>>) {
    run_wire(source, entry, out, None)
}
fn run_wire(
    source: &str,
    entry: &'static str,
    out: &Path,
    wire: Option<[u16; 4]>,
) -> (CpuV3Sim, Rc<RefCell<State>>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let generated = std::fs::read_to_string(out.join("ui_layout.rs")).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = rcc::frontend::compile_program_named("DS2 UI", source, &options, &mut |name| {
        if name == "boot_resources" {
            return Ok(generated.clone());
        }
        std::fs::read_to_string(root.join(format!("examples/dualshock2_ui/rcc/{name}.rs")))
            .or_else(|_| std::fs::read_to_string(root.join(format!("rcc/{name}.rs"))))
            .map_err(|e| e.to_string())
    })
    .unwrap();
    let code = rcc_backend::try_compile(parsed, &options, entry).unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    let s = Rc::new(RefCell::new(State {
        active: 0x200000,
        packet_until: if wire.is_some() { u64::MAX } else { 9709 },
        wire_header: wire.map(|v| [v[0], v[1], v[2]]).unwrap_or([0; 3]),
        wire_monitor: wire.map(|v| v[3]).unwrap_or(0),
        ..State::default()
    }));
    for kind in [0, 3, 4, 5, 7] {
        cpu.attach_device(kind, Box::new(Peer { kind, s: s.clone() }));
    }
    // Host-controlled logical time; not physical CPU-cycle timing evidence.
    let mut halted = false;
    for step in 0..4_000_000 {
        {
            let mut s = s.borrow_mut();
            s.us = step / 10;
            if s.us >= s.packet_until {
                s.packet_until += 9709;
                s.valid = true;
                s.changed = true;
                s.packets += 1;
                if s.us < s.gpu_until {
                    s.during_gpu += 1;
                }
                if s.pending {
                    s.during_present += 1;
                }
            }
        }
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted, "bounded DS2 UI run did not finish");
    (cpu, s)
}
fn byte(cpu: &CpuV3Sim, address: u32) -> u8 {
    (cpu.physical_memory(PhysicalWordAddress::new(address / 2)) >> ((address & 1) * 8)) as u8
}
fn text(cpu: &CpuV3Sim, grid: u32, first: u32, count: u32) -> String {
    (0..count)
        .map(|i| char::from(byte(cpu, grid + first + i)))
        .collect()
}
#[test]
fn actual_recorded_panel_has_axes_buttons_and_bounded_stick_overlays() {
    let out = assets("layout");
    for (name, connected, axes, buttons) in [
        ("center", 1, [128, 128, 128, 128], 0x5aa5u16),
        ("edges", 1, [255, 0, 0, 255], 65535),
        ("deadzone", 1, [139, 128, 128, 139], 0),
        ("disconnected", 0, [128, 128, 128, 128], 0),
    ] {
        let source = format!(
            r#"mod gpu;mod panel;mod stick_controls;static S:Buf<u16,16>=Buf::new([0;16]);
fn main(){{dui_controls_reset();if gpu_init()!=GPU_IDLE{{halt(1);}}
S[0u16]={connected};S[1u16]=115;S[2u16]={buttons};
S[3u16]={rx};S[4u16]={ry};S[5u16]={lx};S[6u16]={ly};
if dui_record(S.as_array(),18,256,52)==0{{halt(2);}}
dev_send(7,0,GPU_CB.command_words);dev_send(7,1,GPU_CB.object_words);halt(0x600d);}}"#,
            rx = axes[0],
            ry = axes[1],
            lx = axes[2],
            ly = axes[3]
        );
        let (cpu, s) = run(&source, "main", &out);
        let grid = 0x4a0080;
        assert_eq!(
            text(&cpu, grid, 57, if connected == 0 { 12 } else { 9 }),
            if connected == 0 {
                "DISCONNECTED"
            } else {
                "CONNECTED"
            }
        );
        for (at, value) in [
            (158, axes[2]),
            (167, axes[3]),
            (184, axes[0]),
            (193, axes[1]),
        ] {
            assert_eq!(text(&cpu, grid, at, 3), format!("{value:03}"));
        }
        assert_eq!(text(&cpu, grid, 110, 4), format!("{buttons:04X}"));
        assert_eq!(text(&cpu, grid, 124, 4), "0012");
        assert_eq!(text(&cpu, grid, 142, 4), "0100");
        for i in 0..16 {
            let at = (12 + (i / 8) * 2) * 50 + 2 + (i % 8) * 6;
            assert_eq!(
                byte(&cpu, grid + at),
                if buttons & (1 << i) == 0 { 1 } else { 2 }
            );
        }
        let positions = [(6, axes[2], axes[3]), (32, axes[0], axes[1])];
        let ranges: Vec<u32> = (0..u32::from(s.borrow().counts[0]) / 2)
            .map(|i| {
                u32::from(byte(&cpu, 0x490000 + i * 4))
                    | (u32::from(byte(&cpu, 0x490001 + i * 4)) << 8)
                    | (u32::from(byte(&cpu, 0x490002 + i * 4)) << 16)
                    | (u32::from(byte(&cpu, 0x490003 + i * 4)) << 24)
            })
            .filter(|c| c >> 28 == 11)
            .collect();
        assert_eq!(ranges.len(), 5);
        let mut layers = vec![format!(
            "{{\"first\":0,\"end\":750,\"cells\":{:?}}}",
            (0..750).map(|i| byte(&cpu, grid + i)).collect::<Vec<_>>()
        )];
        for processed in 0..2 {
            let overlay = if processed == 0 { 0x4a0400 } else { 0x4a0780 };
            for (i, (x, rx, ry)) in positions.into_iter().enumerate() {
                let axis = |raw: u32| {
                    if raw < 128 {
                        (f64::from(raw) - 128.0) / 128.0
                    } else {
                        (f64::from(raw) - 128.0) / 127.0
                    }
                };
                let (mut dx, mut dy) = if connected == 0 {
                    (0.0, 0.0)
                } else {
                    (axis(rx), axis(ry))
                };
                if processed != 0 {
                    let length = dx.hypot(dy);
                    let strength = ((dx.abs().max(dy.abs()) - 24.0 / 256.0) / (1.0 - 24.0 / 256.0))
                        .clamp(0.0, 1.0);
                    if length == 0.0 {
                        dx = 0.0;
                        dy = 0.0;
                    } else {
                        dx = dx / length * strength;
                        dy = dy / length * strength;
                    }
                }
                let plot = |v: f64| {
                    let d = (v.abs() * 14.0).round() as i32 * 2;
                    (32 + if v < 0.0 { -d } else { d }) as u32
                };
                let px = plot(dx);
                let py = plot(dy);
                let cell = (5 + py / 16) * 50 + x + px / 8;
                let id = if processed == 0 { 96 } else { 128 } + ((py % 16) / 2) * 4 + (px % 8) / 2;
                assert_eq!(u32::from(byte(&cpu, overlay + cell)), id);
                assert_eq!(
                    ranges[1 + processed * 2 + i],
                    gpu_v2::system::vliw_command::Command::ui_grid_range(
                        276,
                        cell as u16,
                        cell as u16 + 1
                    )
                    .unwrap()
                    .0
                );
                layers.push(format!(
                    "{{\"first\":{cell},\"end\":{},\"cells\":{:?}}}",
                    cell + 1,
                    (0..750)
                        .map(|j| byte(&cpu, overlay + j))
                        .collect::<Vec<_>>()
                ));
            }
        }
        std::fs::write(
            out.join(format!("{name}.json")),
            format!("{{\"clear\":2113,\"layers\":[{}]}}", layers.join(",")),
        )
        .unwrap();
    }
}
#[test]
fn diagnostic_controls_clamp_deadzone_and_calibrate_only_on_explicit_edges() {
    let out = assets("controls");
    run(
        r#"mod stick_controls;static S:Buf<u16,16>=Buf::new([0;16]);
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){dui_controls_reset();check(dui_deadzone()==24);S[0u16]=1;
S[5u16]=140;S[6u16]=128;let edge=dui_left(S.as_array());check(edge.magnitude>fix32::zero());
S[8u16]=16;dui_controls(S.as_array());let dead=dui_left(S.as_array());check(dead.magnitude==fix32::zero());
S[8u16]=64;dui_controls(S.as_array());let restored=dui_left(S.as_array());check(restored.magnitude>fix32::zero());
S[3u16]=126;S[4u16]=130;S[5u16]=124;S[6u16]=132;
S[8u16]=16;dui_controls(S.as_array());check(dui_deadzone()==28&&S[8u16]==16);
S[8u16]=0;dui_controls(S.as_array());check(dui_deadzone()==28);
S[8u16]=80;dui_controls(S.as_array());check(dui_deadzone()==28);
let mut i:u16=0;S[8u16]=16;while i<30{dui_controls(S.as_array());i+=1;}check(dui_deadzone()==60);
i=0;S[8u16]=64;while i<30{dui_controls(S.as_array());i+=1;}check(dui_deadzone()==0);
S[8u16]=1;dui_controls(S.as_array());let mut l=dui_left(S.as_array());let r=dui_right(S.as_array());check(l.cal_x==fix32::zero()&&l.cal_y==fix32::zero()&&r.cal_x==fix32::zero()&&r.cal_y==fix32::zero());
S[5u16]=255;dui_controls(S.as_array());S[5u16]=124;l=dui_left(S.as_array());check(l.cal_x==fix32::zero());
S[8u16]=8;dui_controls(S.as_array());check(dui_deadzone()==24);l=dui_left(S.as_array());check(l.cal_x==fix32::from_words(63488,65535)&&l.cal_y>fix32::from_words(2060,0)&&l.cal_y<fix32::from_words(2070,0));
S[8u16]=16;dui_controls(S.as_array());S[0u16]=0;S[8u16]=8;dui_controls(S.as_array());check(dui_deadzone()==28);
halt(0x600d);}"#,
        "main",
        &out,
    );
}
#[test]
fn complete_one_frame_services_ds2_during_render_and_present_waits() {
    let out = assets("loop");
    let (_, s) = run(
        include_str!("../../examples/dualshock2_ui/rcc/main.rs"),
        "dui_one_frame_main",
        &out,
    );
    let s = s.borrow();
    assert_eq!((s.starts, s.swaps), (1, 1));
    assert!(!s.pending);
    assert_eq!(s.active, 0x218000);
    assert!(
        s.during_gpu > 0 && s.during_present > 0,
        "polling stopped during waits"
    );
    std::fs::write(
        out.join("result.json"),
        format!(
            "{{\"frames\":1,\"packets\":{},\"during_gpu\":{},\"during_present\":{}}}",
            s.packets, s.during_gpu, s.during_present
        ),
    )
    .unwrap();
}

#[test]
fn one_call_per_frame_reads_latest_without_controlling_acquisition() {
    let out = assets("cadence");
    let (_, s) = run(
        r#"mod device_abi;mod dualshock2;mod time;
static S:Buf<u16,16>=Buf::new([0;16]);static RX:Buf<u16,21>=Buf::new([0;21]);
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){ds2_init(S.as_array());check(!ds2_service(S.as_array(),RX.as_array()));
let mut frame:u16=0;while frame<3{
let start=time_milliseconds();while time_elapsed_ms(start,time_milliseconds())<17{}
time_clear_microseconds();check(ds2_service(S.as_array(),RX.as_array()));
check(S[0u16]==1);frame+=1;}halt(0x600d);}"#,
        "main",
        &out,
    );
    assert!(s.borrow().packets >= 5);
    assert!(s.borrow().packet_until != 0);
}

#[test]
fn failed_headers_are_visible_without_promoting_invalid_input() {
    let out = assets("wire-diagnosis");
    let source = r#"mod gpu;mod panel;mod stick_controls;mod dualshock2;
static S:Buf<u16,10>=Buf::new([0;10]);
fn main(){dui_controls_reset();ds2_init(S.as_array());if gpu_init()!=GPU_IDLE{halt(1);}
dui_probe_wire();if dui_record(S.as_array(),0,0,0)==0{halt(2);}halt(0x600d);}"#;
    for (header, stage) in [
        ([0, 0, 0], 1),
        ([255, 255, 255], 2),
        ([255, 115, 165], 3),
        ([255, 243, 90], 4),
        ([255, 115, 90], 5),
    ] {
        let monitor = (3 << 11) | (32 << 5) | 1;
        let (cpu, _) = run_wire(
            source,
            "main",
            &out,
            Some([header[0], header[1], header[2], monitor]),
        );
        let grid = 0x4a0080;
        assert_eq!(text(&cpu, grid, 57, 12), "DISCONNECTED");
        assert_eq!(text(&cpu, grid, 76, 2), "00");
        for (at, byte) in [(511, header[0]), (517, header[1]), (523, header[2])] {
            assert_eq!(text(&cpu, grid, at, 2), format!("{byte:02X}"));
        }
        assert_eq!(text(&cpu, grid, 529, 4), format!("{monitor:04X}"));
        assert_eq!(text(&cpu, grid, 537, 1), format!("{stage:X}"));
        assert_eq!(text(&cpu, grid, 541, 1), "0");
    }
    // A software-disconnected state with a valid hardware page is a different
    // diagnosis from no wire reply; V must expose it without fabricating input.
    let source = source.replace("dui_probe_wire();", "dev_send(7,2,3);dui_probe_wire();");
    let (cpu, _) = run_wire(&source, "main", &out, Some([0, 0, 0, 0]));
    assert_eq!(text(&cpu, 0x4a0080, 57, 12), "DISCONNECTED");
    assert_eq!(text(&cpu, 0x4a0080, 537, 1), "0");
    assert_eq!(text(&cpu, 0x4a0080, 541, 1), "3");
}
