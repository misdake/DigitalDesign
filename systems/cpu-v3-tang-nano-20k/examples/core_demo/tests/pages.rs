//! Real exported application, functional GPU, actual Display and gamepad input.
use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::{
    input_device::{ControllerSnapshot, InputMode},
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use std::{path::Path, time::Duration};

#[test]
fn cpu_canvas_clips_visible_pixels_and_restores_the_cpu_window() {
    use cpu_v3::{rcc_backend, CpuV3Sim};
    let source = r#"
use crate::dsl_rt::*;
mod cpu_canvas; use cpu_canvas::*;
fn main() {
 unsafe { write_dseg_page(3,0x45); }
 let canvas=unsafe { canvas_from_sectors(0x8000,1600) };
 canvas_span(canvas, 14,19,3,0x1111);
 canvas_span(canvas, -5,3,239,0x2222);
 canvas_rect(canvas, 397,238,8,5,0x3333);
 canvas_line(canvas, 0,0,3,3,0x4444);
 canvas_circle(canvas, 100,100,1,0x5555);
 canvas_pixel(canvas, 400,240,0xffff); canvas_pixel(canvas, 32767,32767,0xffff);
 canvas_span(canvas, 20,10,5,0xffff); canvas_rect(canvas, 32767,0,20,20,0xffff);
 canvas_line(canvas, -32768,0,32767,0,0xffff); canvas_circle(canvas, 0,0,32767,0xffff);
 halt(0x600d);
}
"#;
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let ir = rcc::frontend::compile_program_named_checked(
        "CPU canvas boundaries",
        source,
        &options,
        &rcc::frontend::rustc_check::RustcCheck::new(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/core-demo/graphical/canvas-rustc"),
        ),
        &mut |name| {
            std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("rcc")
                    .join(format!("{name}.rs")),
            )
            .map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    let base = 0x400000 / 2;
    cpu.physical_memory_mut()[base - 1..base + 102401].fill(0xa55a);
    let mut halted = false;
    for _ in 0..200000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted, "bounded CPU canvas run");
    assert_eq!(cpu.data_segments(), [0, 1, 2, 0x45]);
    let mut expected = vec![0xa55a; 400 * 240];
    for x in 14..19 {
        expected[3 * 400 + x] = 0x1111;
    }
    for x in 0..3 {
        expected[239 * 400 + x] = 0x2222;
    }
    for y in 238..240 {
        for x in 397..400 {
            expected[y * 400 + x] = 0x3333;
        }
    }
    for i in 0..4 {
        expected[i * 400 + i] = 0x4444;
    }
    for (x, y) in [(100, 99), (100, 101), (99, 100), (101, 100)] {
        expected[y * 400 + x] = 0x5555;
    }
    let memory = cpu.physical_memory_slice();
    for y in 0..240 {
        for x in 0..400 {
            let at = cpu_v3_tang_nano_20k::framebuffer_word_at(base as u32, x as u32, y as u32);
            assert_eq!(memory[at as usize], expected[y * 400 + x], "pixel {x},{y}");
        }
    }
    for column in 0..25 {
        assert!(
            memory[base + column * 4096 + 3840..base + (column + 1) * 4096]
                .iter()
                .all(|&v| v == 0xa55a)
        );
    }
    assert_eq!(memory[base - 1], 0xa55a);
    assert_eq!(memory[base + 102400], 0xa55a);
}

struct Demo {
    sim: CpuV3SystemSim,
    presents: usize,
    minimum_sp: u16,
    static_words: usize,
    asset_ids: std::collections::BTreeMap<String, u8>,
    colors: [u32; 2],
    depth: usize,
    ui: usize,
}
impl Demo {
    fn new() -> Self {
        let artifact = std::env::var("CORE_DEMO_ARTIFACT").expect("export core-demo first");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let checked = std::process::Command::new("python")
            .args(["-B"])
            .arg(root.join("tools/application_artifact.py"))
            .args(["check", &artifact])
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{}",
            String::from_utf8_lossy(&checked.stderr)
        );
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(Path::new(&artifact).join("application.build.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest["application"]["source"]["path"],
            "systems/cpu-v3-tang-nano-20k/examples/core_demo/rcc/main.rs"
        );
        let package = std::fs::read(Path::new(&artifact).join("cpu-v3-boot.bin")).unwrap();
        let mut sim =
            CpuV3SystemSim::from_single_flash(&package, 0xa55a, VBlankMode::PauseOnFrameIndexWait)
                .unwrap()
                .with_functional_gpu();
        sim.advance_host_time(Duration::from_millis(65530));
        let static_words = manifest["target"]["static_words"].as_u64().unwrap() as usize;
        let constants =
            std::fs::read_to_string(Path::new(&artifact).join("core-demo-assets/core_assets.rs"))
                .unwrap();
        let asset_ids = constants
            .lines()
            .filter_map(|line| {
                let (name, rest) = line.strip_prefix("pub const ")?.split_once(':')?;
                if !name.starts_with("KEY_") && !name.starts_with("ICON_") {
                    return None;
                }
                let value = rest
                    .split_once('=')?
                    .1
                    .trim_end_matches(';')
                    .parse()
                    .unwrap();
                Some((name.to_string(), value))
            })
            .collect();
        let storage: serde_json::Value = serde_json::from_slice(
            &std::fs::read(Path::new(&artifact).join("application.buffers.json")).unwrap(),
        )
        .unwrap();
        let base = |name: &str| {
            let entry = storage
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["name"] == name)
                .unwrap();
            assert_eq!(entry["bytes"], 204800);
            assert_eq!(entry["alignment"], 65536);
            (entry["address"].as_u64().unwrap() / 2) as u32
        };
        let colors = [base("COLOR_A"), base("COLOR_B")];
        let depth = base("DEPTH") as usize;
        let ui = storage
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"] == "UI_STORAGE")
            .unwrap()["address"]
            .as_u64()
            .unwrap() as usize
            / 2;
        let mut result = Self {
            sim,
            presents: 0,
            minimum_sp: 0xc000,
            static_words,
            asset_ids,
            colors,
            depth,
            ui,
        };
        result.input(0, [128, 128]);
        result
    }
    fn input(&mut self, held: u16, left: [u8; 2]) {
        self.input_pair(held, left, [128, 128]);
    }
    fn input_pair(&mut self, held: u16, left: [u8; 2], right: [u8; 2]) {
        self.sim.publish_input(Some(ControllerSnapshot {
            mode: InputMode::Analog,
            buttons_active_low: !held,
            left,
            right,
        }));
    }
    fn present(&mut self) {
        let front = self.sim.display_state().active_base as usize;
        let saved_front = self.sim.cpu().physical_memory_slice()[front..front + 102400].to_vec();
        for _ in 0..8_000_000 {
            if self.sim.waiting_for_vblank() {
                self.sim.advance_host_time(Duration::from_micros(16667));
                if self.sim.advance_vblank() {
                    let expected = if self.presents.is_multiple_of(2) {
                        self.colors[1]
                    } else {
                        self.colors[0]
                    };
                    assert_eq!(self.sim.display_state().active_base, expected);
                    assert!(self.sim.gpu().last_error().is_none());
                    assert_eq!(
                        self.sim.cpu().physical_memory_slice()[front..front + 102400],
                        saved_front,
                        "CPU/GPU must never modify the current Front plane"
                    );
                    self.presents += 1;
                    return;
                }
            }
            let outcome = self.sim.step().unwrap();
            let sp = self.sim.cpu().registers()[13];
            if sp != 0 {
                self.minimum_sp = self.minimum_sp.min(sp);
            }
            if let StepOutcome::Halted { signal } = outcome {
                panic!(
                    "unexpected halt {signal:#06x} after {} presents",
                    self.presents
                );
            }
        }
        panic!("bounded page timeout after {} presents", self.presents);
    }
    fn click(&mut self, button: u16) {
        self.input(button, [128, 128]);
        self.present();
        self.input(0, [128, 128]);
        self.present();
    }
    fn glyph(&self, x: usize, y: usize) -> u8 {
        let memory = self.sim.cpu().physical_memory_slice();
        let base = self.ui;
        let cell = y * 50 + x;
        (memory[base + cell / 2] >> ((cell & 1) * 8)) as u8
    }
    fn text(&self, x: usize, y: usize, count: usize) -> String {
        (0..count).map(|i| self.glyph(x + i, y) as char).collect()
    }
    fn assert_hint(&self, x: usize, key: &str, action: &str) {
        let first = self.asset_ids[&format!("KEY_{key}")];
        for cell in 0..3 {
            assert_eq!(self.glyph(x + cell, 14), first + cell as u8, "{key} keycap");
        }
        assert_eq!(self.glyph(x + 3, 14), 255, "key/action gap");
        assert_eq!(
            self.glyph(x + 4, 14),
            self.asset_ids[&format!("ICON_{action}")],
            "{key} action"
        );
    }
    fn pixel(&self, x: u32, y: u32) -> u16 {
        let at =
            cpu_v3_tang_nano_20k::framebuffer_word_at(self.sim.display_state().active_base, x, y);
        self.sim.cpu().physical_memory_slice()[at as usize]
    }
    fn region(&self, x: u32, y: u32, w: u32, h: u32) -> Vec<u16> {
        (y..y + h)
            .flat_map(|row| (x..x + w).map(move |column| self.pixel(column, row)))
            .collect()
    }
    fn save(&self, name: &str) {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/core-demo/key-legends/pages");
        std::fs::create_dir_all(&root).unwrap();
        let mut data = b"P6\n400 240\n255\n".to_vec();
        for pixel in self.sim.render_active_framebuffer() {
            data.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
        std::fs::write(root.join(format!("{name}.ppm")), data).unwrap();
    }
}

#[test]
#[ignore = "requires an ordinary core-demo --frames 0 exported as CORE_DEMO_ARTIFACT"]
fn graphical_pages_controls_and_present_from_exported_application() {
    let mut demo = Demo::new();
    demo.present();
    let signatures = [
        (2, 1, "DEMONSTRATIONS"),
        (7, 4, "FILLED"),
        (4, 4, "SIN"),
        (5, 4, "LEFT"),
        (8, 3, "SRGB"),
        (10, 12, "STATIC"),
    ];
    let titles = [
        "DEMONSTRATIONS",
        "TRIANGLES",
        "WAVES",
        "CONTROLLER",
        "COLOR",
        "GEOMETRY",
    ];
    for (page, &(x, y, label)) in signatures.iter().enumerate() {
        assert_eq!(demo.text(2, 1, titles[page].len()), titles[page]);
        assert_eq!(demo.text(x, y, label.len()), label);
        for row in [0, 13] {
            for col in 0..50 {
                assert_eq!(demo.glyph(col, row), 255, "no top bar or page counter");
            }
        }
        for col in 0..50 {
            let glyph = demo.glyph(col, 14);
            assert!(
                glyph == 255 || glyph >= 91,
                "footer uses icon assets, not prose"
            );
        }
        demo.assert_hint(1, "L1", "PREVIOUS");
        demo.assert_hint(8, "R1", "NEXT");
        if page == 0 {
            demo.assert_hint(38, "DPAD_V", "SELECT");
            demo.assert_hint(44, "CROSS", "ACCEPT");
        } else {
            demo.assert_hint(44, "START", "HOME");
            if page == 1 || page == 2 || page == 5 {
                demo.assert_hint(26, "DPAD_H", "ADJUST");
                demo.assert_hint(32, "CROSS", "PAUSE");
                demo.assert_hint(38, "TRIANGLE", "RESET");
            } else if page == 4 {
                demo.assert_hint(32, "DPAD_V", "SELECT");
                demo.assert_hint(38, "CROSS", "ACCEPT");
            }
        }
        demo.save(&format!("page-{page}"));
        match page {
            0 => assert_eq!(demo.pixel(24, 72), 0x0328),
            1 => {
                let filled = demo.region(36, 72, 128, 128);
                assert!(
                    filled.iter().filter(|&&v| v != 0x0843).count() > 2000,
                    "CPU triangle must cover pixels"
                );
                demo.click(1 << 14); // Freeze animation; parameter changes must still rotate it.
                demo.assert_hint(32, "CROSS", "PLAY");
                let before = demo.region(36, 72, 128, 128);
                demo.present();
                assert_eq!(
                    demo.region(36, 72, 128, 128),
                    before,
                    "paused geometry must be stable"
                );
                demo.click(1 << 5);
                assert_ne!(
                    demo.region(36, 72, 128, 128),
                    before,
                    "parameter must rotate actual pixels"
                );
                demo.save("triangle-rotated");
                demo.click(1 << 12);
            }
            2 => {
                let graph = demo.region(32, 70, 344, 120);
                assert!(
                    graph.iter().filter(|&&v| v == 0x07e0).count() > 300,
                    "sine pixels"
                );
                assert!(
                    graph.iter().filter(|&&v| v == 0x07ff).count() > 300,
                    "cosine pixels"
                );
                assert_eq!(
                    graph.iter().filter(|&&v| v == 0xf800).count(),
                    25,
                    "phase marker"
                );
                demo.click(1 << 14);
                let frozen = demo.region(32, 70, 344, 120);
                demo.sim.advance_host_time(Duration::from_millis(1000));
                demo.present();
                assert_eq!(demo.region(32, 70, 344, 120), frozen);
                demo.save("waves-paused");
                demo.click(1 << 14);
                assert_ne!(demo.region(32, 70, 344, 120), frozen);
                demo.click(1 << 12);
            }
            3 => {
                let connected_icon = demo.glyph(2, 12);
                // Match the old diagnostic's shared stick contract: +Y down,
                // normalized direction, maximum-axis travel and radial dead-zone remap.
                let cases = [
                    [128, 128],
                    [128, 0],
                    [128, 255],
                    [0, 128],
                    [255, 128],
                    [0, 0],
                    [0, 255],
                    [255, 0],
                    [255, 255],
                    [135, 128],
                    [128, 135],
                    [192, 128],
                    [192, 192],
                    [128, 64],
                ];
                let mut stick_evidence = Vec::new();
                for left in cases {
                    let right = [255 - left[0], 255 - left[1]];
                    demo.input_pair(0, left, right);
                    demo.present();
                    for (cx, raw) in [(72.0, left), (184.0, right)] {
                        let axis = |value: u8| {
                            if value <= 128 {
                                (f64::from(value) - 128.0) / 128.0
                            } else {
                                (f64::from(value) - 128.0) / 127.0
                            }
                        };
                        let x = axis(raw[0]);
                        let y = axis(raw[1]);
                        let travel = x.abs().max(y.abs());
                        let strength =
                            ((travel - 24.0 / 256.0) / (1.0 - 24.0 / 256.0)).clamp(0.0, 1.0);
                        let length = x.hypot(y);
                        let (dx, dy) = if length == 0.0 {
                            (0.0, 0.0)
                        } else {
                            (
                                (x / length * strength * 28.0).round(),
                                (y / length * strength * 28.0).round(),
                            )
                        };
                        assert_eq!(
                            demo.pixel((cx + dx) as u32, (128.0 + dy) as u32),
                            0x07e0,
                            "native marker {raw:?}"
                        );
                        assert!(
                            dx.hypot(dy) <= 28.5,
                            "diagonal travel must stay on the circle"
                        );
                        let mut points = Vec::new();
                        for y in 96..160 {
                            for x in cx as u32 - 32..cx as u32 + 32 {
                                if demo.pixel(x, y) == 0x07e0 {
                                    points.push((x, y));
                                }
                            }
                        }
                        assert_eq!(points.len(), 25, "only the 5x5 stick marker is green");
                        let actual_x = points.iter().map(|p| p.0).sum::<u32>() / 25;
                        let actual_y = points.iter().map(|p| p.1).sum::<u32>() / 25;
                        assert_eq!(
                            (actual_x, actual_y),
                            ((cx + dx) as u32, (128.0 + dy) as u32),
                            "marker center {raw:?}"
                        );
                        stick_evidence.push(serde_json::json!({"center_x":cx,"raw":raw,
                            "calibrated":[x,y],"travel":travel,"strength":strength,
                            "expected":[cx+dx,128.0+dy],"actual":[actual_x,actual_y]}));
                    }
                    if left == [255, 0] {
                        demo.save("sticks-up-right");
                    }
                    if left == [255, 255] {
                        demo.save("sticks-down-right");
                    }
                    if left == [192, 192] {
                        demo.save("sticks-half-diagonal");
                    }
                }
                std::fs::write(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../target/core-demo/key-legends/pages/stick-oracle.json"),
                    serde_json::to_vec_pretty(&stick_evidence).unwrap(),
                )
                .unwrap();
                demo.input(0, [128, 128]);
                demo.present();

                assert_eq!(demo.pixel(72, 128), 0x07e0);
                let centered = demo.region(40, 96, 64, 64);
                demo.input(1 << 4, [255, 0]);
                demo.present();
                assert_ne!(
                    demo.region(40, 96, 64, 64),
                    centered,
                    "actual calibrated stick moves marker"
                );
                assert_eq!(
                    demo.pixel(266, 118),
                    0x048c,
                    "held UP button lights its panel"
                );
                demo.save("input-held");
                demo.input(0, [128, 128]);
                demo.present();
                assert_eq!(demo.pixel(266, 118), 0x18c3);
                assert_eq!(demo.pixel(72, 128), 0x07e0);
                demo.save("input-released");
                demo.sim.publish_input(None);
                demo.present();
                assert_ne!(demo.glyph(2, 12), connected_icon);
                assert_eq!(demo.pixel(72, 128), 0x07e0);
                demo.input(0, [128, 128]);
                demo.present();
            }
            4 => {
                for step in 0..32u16 {
                    assert_eq!(
                        demo.pixel(35 + u32::from(step) * 8, 120),
                        (step << 11) | (step << 6) | ((step >> 4) << 5) | step,
                        "gray {step}"
                    );
                }
                let sample = 120 * 400 + 35 + 16 * 8;
                let raw = demo.sim.render_active_framebuffer()[sample];
                let active_radio = demo.glyph(5, 3);
                assert_ne!(active_radio, demo.glyph(5, 5));
                demo.click(1 << 6); // Focus the explicit LINEAR option without applying it.
                assert_eq!(demo.sim.render_active_framebuffer()[sample], raw);
                assert_eq!(demo.glyph(5, 3), active_radio);
                demo.save("color-linear-focused");
                demo.click(1 << 14); // Confirm the focused option.
                let linear = demo.sim.render_active_framebuffer()[sample];
                assert_ne!(linear, raw);
                assert_eq!(demo.glyph(5, 5), active_radio);
                demo.save("color-linear");
                demo.click(1 << 5);
                demo.click(1 << 7);
                assert_eq!(
                    demo.sim.render_active_framebuffer()[sample],
                    linear,
                    "left/right does not select color mode"
                );
                demo.click(1 << 4);
                assert_eq!(demo.sim.render_active_framebuffer()[sample], linear);
                demo.click(1 << 14);
                assert_eq!(demo.sim.render_active_framebuffer()[sample], raw);
            }
            5 => {
                for x in [48, 208] {
                    let pixels = demo.region(x, 60, 144, 128);
                    assert!(
                        pixels.iter().filter(|&&p| p != 0x0843).count() > 700,
                        "real GPU cube must cover pixels"
                    );
                }
                demo.click(1 << 14);
                demo.assert_hint(32, "CROSS", "PLAY");
                let frozen = demo.region(32, 55, 336, 130);
                demo.sim.advance_host_time(Duration::from_millis(1000));
                demo.present();
                assert_eq!(demo.region(32, 55, 336, 130), frozen, "paused GPU geometry");
                demo.save("geometry-paused");
                demo.click(1 << 5);
                assert_ne!(
                    demo.region(32, 55, 336, 130),
                    frozen,
                    "focal adjustment changes projection"
                );
                demo.save("geometry-perspective");
                demo.click(1 << 12);
                demo.click(1 << 14);
                let reset = demo.region(32, 55, 336, 130);
                demo.click(1 << 12);
                demo.click(1 << 14);
                assert_eq!(
                    demo.region(32, 55, 336, 130),
                    reset,
                    "reset is deterministic"
                );
                demo.save("geometry-reset");
                // Re-enter the actual page repeatedly; resources are registered once at boot.
                for _ in 0..4 {
                    demo.click(1 << 11);
                    demo.click(1 << 10);
                }
                assert_eq!(demo.text(2, 1, 8), "GEOMETRY");
                let left = demo.region(48, 55, 144, 130);
                let right = demo.region(208, 55, 144, 130);
                demo.click(1 << 14);
                assert_eq!(
                    demo.region(48, 55, 144, 130),
                    left,
                    "STATIC label matches fixed model"
                );
                assert_ne!(
                    demo.region(208, 55, 144, 130),
                    right,
                    "ROTATING label matches animated model"
                );
            }
            _ => {}
        }
        // Clear owns the padded physical plane; CPU canvas must not touch its guard or depth.
        let memory = demo.sim.cpu().physical_memory_slice();
        let active = demo.sim.display_state().active_base as usize;
        for column in 0..25 {
            assert!(
                memory[active + column * 4096 + 3840..active + (column + 1) * 4096]
                    .iter()
                    .all(|&v| v == 0x0843)
            );
        }
        let depth = demo.depth;
        if page == 5 {
            assert!(
                memory[depth..depth + 102400].iter().any(|&v| v != 65535),
                "GPU writes real depth"
            );
        } else {
            assert!(memory[depth..depth + 102400].iter().all(|&v| v == 0));
        }
        demo.click(1 << 11);
    }
    assert_eq!(demo.text(2, 1, 14), "DEMONSTRATIONS");
    demo.click(1 << 10);
    assert_eq!(demo.text(2, 1, 8), "GEOMETRY");
    demo.click(1 << 3);
    demo.click(1 << 6);
    demo.click(1 << 14);
    assert_eq!(demo.text(7, 4, 6), "FILLED");
    demo.input(1 << 11, [128, 128]);
    demo.present();
    demo.present();
    assert_eq!(
        demo.text(4, 4, 3),
        "SIN",
        "holding navigation cannot repeat edges"
    );
    let stack_words = usize::from(0xc000 - demo.minimum_sp);
    assert!(demo.static_words + stack_words <= 8192);
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/core-demo/key-legends/pages/budget.json");
    std::fs::write(out,serde_json::to_vec_pretty(&serde_json::json!({"static_words":demo.static_words,
        "observed_stack_words":stack_words,"presents":demo.presents,"maximum_steps_per_present":8000000,
        "backend":"CPU instruction sim + functional GPU + Display"})).unwrap()).unwrap();
}
