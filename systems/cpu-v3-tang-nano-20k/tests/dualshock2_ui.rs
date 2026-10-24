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
    selected: bool,
    valid: bool,
    byte: usize,
    result: u16,
    packets: usize,
    during_gpu: usize,
    during_present: usize,
    counts: [u16; 2],
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
            (5, 0) => u16::from(s.selected) * 4 + u16::from(s.valid) * 2,
            (5, 2) => s.result,
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
            (5, 0) => match value {
                1 => {
                    assert!(!s.selected);
                    s.selected = true;
                    s.byte = 0;
                }
                2 => {
                    assert!(s.selected && !s.valid);
                    s.selected = false;
                    s.packets += 1;
                    if s.us < s.gpu_until {
                        s.during_gpu += 1;
                    }
                    if s.pending {
                        s.during_present += 1;
                    }
                }
                3 => {
                    s.selected = false;
                    s.valid = false;
                }
                _ => panic!("unexpected DS2 command"),
            },
            (5, 1) => {
                assert!(s.selected && !s.valid);
                let response = [255, 0x73, 0x5a, 0xa5, 0x5a, 0, 255, 128, 64];
                assert!(s.byte < response.len());
                s.result = response[s.byte];
                s.byte += 1;
                s.valid = true;
            }
            (5, 2) => {
                assert!(s.valid);
                s.valid = false;
            }
            (7, 0) => s.counts[0] = value,
            (7, 1) => s.counts[1] = value,
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
        ..State::default()
    }));
    for kind in [0, 3, 4, 5, 7] {
        cpu.attach_device(kind, Box::new(Peer { kind, s: s.clone() }));
    }
    // Host-controlled logical time; not physical CPU-cycle timing evidence.
    let mut halted = false;
    for step in 0..4_000_000 {
        s.borrow_mut().us = step / 10;
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
        ("disconnected", 0, [128, 128, 128, 128], 0),
    ] {
        let source = format!(
            r#"mod gpu;mod panel;static S:Buf<u16,16>=Buf::new([0;16]);
fn main(){{if gpu_init()!=GPU_IDLE{{halt(1);}}
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
        let overlay = 0x4a0400;
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
        assert_eq!(ranges.len(), 3);
        let mut layers = vec![format!(
            "{{\"first\":0,\"end\":750,\"cells\":{:?}}}",
            (0..750).map(|i| byte(&cpu, grid + i)).collect::<Vec<_>>()
        )];
        for (i, (x, rx, ry)) in positions.into_iter().enumerate() {
            let cell = (5 + ry / 64) * 50 + x + rx / 32;
            let id = 128 + ((ry / 8) % 8) * 4 + (rx / 8) % 4;
            assert_eq!(u32::from(byte(&cpu, overlay + cell)), id);
            assert_eq!(
                ranges[i + 1],
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
        std::fs::write(
            out.join(format!("{name}.json")),
            format!("{{\"clear\":2113,\"layers\":[{}]}}", layers.join(",")),
        )
        .unwrap();
    }
}
#[test]
fn complete_one_frame_services_ds2_during_render_and_present_waits() {
    let out = assets("loop");
    let (_, s) = run(
        include_str!("../examples/dualshock2_ui/rcc/main.rs"),
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
