//! Production read/pixel-register projection with controlled Timing boundary inputs.
//! No warm checkpoint, root time, backing-store bypass or complete Display claim.
use cpu_v3_tang_nano_20k::hardware::display_cycle::{
    ColorPrecode, ControlInput, DisplayControl, Edges, LineBuffer, LineBufferInput, PixelPipe,
    PixelRead, PixelReadInput, Precode, RamWord, Timing, TimingOutput,
};
use cpu_v3_tang_nano_20k::{display::ACTIVE_DISPLAY_CONFIG, DisplayLineBuffer};
use digital_design_hardware::Module;
use std::{
    fmt::Write as _,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
struct Sample {
    name: String,
    value: u64,
    known: bool,
    tainted: bool,
}
fn plain(name: &str, value: impl Into<u64>) -> Sample {
    Sample {
        name: name.into(),
        value: value.into(),
        known: true,
        tainted: false,
    }
}
fn word(name: &str, q: RamWord) -> Sample {
    Sample {
        name: name.into(),
        value: q.value.into(),
        known: q.known,
        tainted: q.collision,
    }
}
fn code(name: &str, q: Precode) -> Sample {
    Sample {
        name: name.into(),
        value: q.value.into(),
        known: q.known,
        tainted: q.collision,
    }
}
fn color_samples(name: &str, pixel: ColorPrecode) -> Vec<Sample> {
    pixel
        .channels
        .into_iter()
        .enumerate()
        .map(|(i, c)| code(&format!("{name}{i}"), c))
        .collect()
}
fn decode(c: u16) -> u8 {
    let mut data = c & 1;
    for bit in 1..8 {
        data |= (((c >> bit) ^ (c >> (bit - 1)) ^ u16::from(c & 256 == 0)) & 1) << bit;
    }
    data as u8
}
fn pattern(x: u16, y: u16) -> u16 {
    const FIRST: [u16; 8] = [0, 65535, 0xf800, 0x07e0, 0x001f, 0x0801, 0x1002, 0x8010];
    if x < 8 {
        return FIRST[usize::from(x)];
    }
    (((x * 13 + y * 7) & 31) << 11) | (((x * 19 + y * 11) & 63) << 5) | ((x * 3 + y * 5) & 31)
}
fn source_word(address: u16) -> u64 {
    (0..4)
        .map(|lane| u64::from(pattern((address % 100) * 4 + lane, address / 100)) << (16 * lane))
        .sum()
}
fn expected_rgb(pixel: u16, linear: bool) -> [u8; 3] {
    let r = (pixel >> 11) & 31;
    let g = (pixel >> 5) & 63;
    let b = pixel & 31;
    if linear {
        let table = cpu_v3_tang_nano_20k::display::LINEAR6_TO_SRGB8;
        [
            table[usize::from((r << 1) | (r >> 4))],
            table[usize::from(g)],
            table[usize::from((b << 1) | (b >> 4))],
        ]
    } else {
        [
            ((r << 3) | (r >> 2)) as u8,
            ((g << 2) | (g >> 4)) as u8,
            ((b << 3) | (b >> 2)) as u8,
        ]
    }
}

#[derive(Default)]
struct Fixture {
    ram: LineBuffer,
    read: PixelRead,
    pipe: PixelPipe,
    active_black: usize,
    blank: usize,
    vectors: String,
    expected: Vec<Vec<Sample>>,
    valid: usize,
    tainted: usize,
    unused_unknown: usize,
    pixels: usize,
    rows: u16,
}
impl Fixture {
    fn step(&mut self, edges: Edges, input: PixelReadInput, write: Option<(u16, u64)>) {
        let index = self.expected.len();
        assert!(index < 32768);
        let q = self.ram.outputs();
        let c = ACTIVE_DISPLAY_CONFIG;
        let mut timing = Timing::default().outputs();
        timing.pixel_reset = input.pixel_reset;
        timing.h_count = input.h_count;
        timing.v_count = input.v_count;
        timing.started = input.scan_enabled;
        timing.scan_valid = input.scan_valid;
        timing.scan_line_ready = input.scan_line_ready;
        timing.scan_linear = input.scan_linear;
        timing.slot = input.slot;
        timing.second = input.second;
        timing.hsync = u32::from(input.h_count) < c.h_sync_end;
        timing.vsync = u32::from(input.v_count) < c.v_sync_end;
        timing.active = (c.h_active_start..c.h_active_end).contains(&u32::from(input.h_count))
            && (c.v_active_start..c.v_active_end).contains(&u32::from(input.v_count));
        let adapted = PixelReadInput::from(timing);
        let old = self.read.outputs(&adapted, q);
        assert_eq!(old, self.read.outputs(&adapted, q));
        let mut samples = Vec::new();
        for bank in 0..2 {
            samples.push(Sample {
                name: format!("address{bank}"),
                value: old.read_address[bank].into(),
                known: old.read_address_known[bank],
                tainted: old.read_address_tainted[bank],
            });
        }
        samples.push(word("oldQa", q[0]));
        samples.push(word("oldQb", q[1]));
        samples.extend(color_samples("selected", old.selected));
        let encoder_input = self.pipe.outputs();
        assert_eq!(encoder_input, self.pipe.outputs());
        samples.extend(color_samples("encoder_old", encoder_input.precode));
        samples.extend([
            plain("old_de", u8::from(encoder_input.de)),
            plain("old_fb", u8::from(encoder_input.framebuffer)),
            plain("old_hs", u8::from(encoder_input.hsync)),
            plain("old_vs", u8::from(encoder_input.vsync)),
        ]);
        if edges.pixel_rise {
            self.pixels += 1;
            let c = ACTIVE_DISPLAY_CONFIG;
            if !input.pixel_reset
                && input.scan_enabled
                && input.scan_valid
                && input.scan_line_ready
                && (c.h_active_start..c.h_active_end).contains(&u32::from(input.h_count))
                && (c.v_active_start..c.v_active_end).contains(&u32::from(input.v_count))
            {
                self.valid += 1;
                assert!(
                    old.selected.channels.iter().all(|c| c.known),
                    "undefined valid pixel {input:?}"
                );
                self.tainted += usize::from(old.selected.channels.iter().any(|c| c.collision));
                let x = (input.h_count - c.h_active_start as u16) / 2;
                let y = (input.v_count - c.v_active_start as u16) / c.scale as u16;
                assert_eq!(
                    old.selected.channels.map(|c| decode(c.value)),
                    expected_rgb(pattern(x, y), input.scan_linear),
                    "x={x} {input:?}"
                );
            }
            if input.h_count == 1013 && !old.read_address_known[0] {
                self.unused_unknown += 1;
            }
            self.pipe.clock(&timing, old.selected);
            let video = self.pipe.outputs();
            assert_eq!(video.de, !input.pixel_reset && timing.active);
            assert_eq!(
                video.framebuffer,
                !input.pixel_reset && timing.active && input.scan_enabled && input.scan_valid
            );
            let source = !input.pixel_reset
                && timing.active
                && input.scan_enabled
                && input.scan_valid
                && input.scan_line_ready;
            assert_eq!(video.effective_source, source);
            if source {
                let x = (input.h_count - c.h_active_start as u16) / 2;
                let y = (input.v_count - c.v_active_start as u16) / c.scale as u16;
                assert_eq!(video.rgb888, expected_rgb(pattern(x, y), input.scan_linear));
            } else {
                assert_eq!(video.rgb888, [0; 3]);
                assert!(video
                    .precode
                    .channels
                    .iter()
                    .all(|c| c.known && !c.collision));
                self.active_black += usize::from(video.de);
                self.blank += usize::from(!video.de);
            }
            self.read.clock(&adapted, q);
        }
        self.ram.clock(
            edges,
            &LineBufferInput {
                write_enable: write.is_some(),
                write_address: write.map_or(0, |v| v.0),
                write_data: write.map_or(0, |v| v.1),
                read_address_a: old.read_address[0],
                read_address_b: old.read_address[1],
                read_address_known: old.read_address_known,
                read_address_tainted: old.read_address_tainted,
            },
        );
        let post = self.read.outputs(&adapted, self.ram.outputs());
        samples.extend([
            plain("phase", post.phase),
            plain("fetch", post.fetch_pair),
            plain("bank", u8::from(post.pair_bank)),
            word("raw", post.raw_pair),
        ]);
        for lane in 0..2 {
            samples.push(code(&format!("r{lane}"), post.red[lane]));
        }
        for lane in 0..2 {
            samples.push(code(&format!("g{lane}"), post.green[lane]));
        }
        for lane in 0..2 {
            samples.extend(color_samples(
                &format!("pair{lane}"),
                post.completed_pair[lane],
            ));
        }
        let q = self.ram.outputs();
        samples.push(word("newQa", q[0]));
        samples.push(word("newQb", q[1]));
        let video = self.pipe.outputs();
        samples.extend(color_samples("pipe", video.precode));
        for lane in 0..3 {
            samples.push(Sample {
                name: format!("rgb{lane}"),
                value: video.rgb888[lane].into(),
                known: video.precode.channels[lane].known,
                tainted: video.precode.channels[lane].collision,
            });
        }
        samples.extend([
            plain("de", u8::from(video.de)),
            plain("fb", u8::from(video.framebuffer)),
            plain("hs", u8::from(video.hsync)),
            plain("vs", u8::from(video.vsync)),
        ]);
        writeln!(
            self.vectors,
            "{index} {} {} {} {:x} {:016x} {} {} {} {} {} {} {} {} {}",
            u8::from(edges.logic_rise),
            u8::from(edges.pixel_rise),
            u8::from(write.is_some()),
            write.map_or(0, |v| v.0),
            write.map_or(0, |v| v.1),
            u8::from(input.pixel_reset),
            input.h_count,
            input.v_count,
            u8::from(input.scan_enabled),
            u8::from(input.scan_line_ready),
            u8::from(input.scan_valid),
            u8::from(input.scan_linear),
            u8::from(input.slot),
            u8::from(input.second)
        )
        .unwrap();
        self.expected.push(samples);
    }
    fn row(&mut self, slot: bool, second: bool, linear: bool, collision: bool) {
        for h in 0..1056 {
            let input = PixelReadInput {
                h_count: h,
                v_count: 32 + (u16::from(slot) * 2 + u16::from(second)) * 2 + (self.rows & 1),
                scan_enabled: true,
                scan_line_ready: true,
                scan_valid: true,
                scan_linear: linear,
                slot,
                second,
                ..Default::default()
            };
            let write = if collision && h == 212 {
                let address = u16::from(slot) * 200 + u16::from(second) * 100;
                Some((address, source_word(address)))
            } else {
                None
            };
            self.step(
                Edges {
                    pixel_rise: true,
                    logic_rise: write.is_some(),
                },
                input,
                write,
            );
            if (210..220).contains(&h) {
                self.step(Edges::default(), input, None);
            }
        }
        self.rows += 1;
    }
}
fn vectors() -> Fixture {
    let mut f = Fixture::default();
    for address in 0..400 {
        f.step(
            Edges {
                logic_rise: true,
                pixel_rise: false,
            },
            PixelReadInput::default(),
            Some((address, source_word(address))),
        );
    }
    for (slot, second, linear) in [
        (false, false, false),
        (false, false, false),
        (false, true, true),
        (false, true, true),
        (true, false, false),
        (true, false, false),
        (true, true, true),
        (true, true, true),
    ] {
        f.row(slot, second, linear, false);
    }
    for phase in 0..4 {
        for h in 0..=216 + phase {
            f.step(
                Edges {
                    pixel_rise: true,
                    logic_rise: false,
                },
                PixelReadInput {
                    h_count: h,
                    v_count: 32,
                    scan_enabled: true,
                    scan_line_ready: true,
                    scan_valid: true,
                    ..Default::default()
                },
                None,
            );
        }
        f.step(
            Edges {
                pixel_rise: true,
                logic_rise: false,
            },
            PixelReadInput {
                pixel_reset: true,
                h_count: 217 + phase,
                ..Default::default()
            },
            None,
        );
    }
    f.row(true, true, true, true);
    for (enabled, valid, ready, v) in [
        (false, false, false, 32),
        (true, false, true, 33),
        (true, true, false, 34),
        (true, true, true, 0),
        (true, true, true, 512),
    ] {
        for h in 0..1056 {
            f.step(
                Edges {
                    pixel_rise: true,
                    logic_rise: false,
                },
                PixelReadInput {
                    h_count: h,
                    v_count: v,
                    scan_enabled: enabled,
                    scan_valid: valid,
                    scan_line_ready: ready,
                    ..Default::default()
                },
                None,
            );
        }
    }
    assert!(f.valid > 5000 && f.tainted == 4 && f.unused_unknown >= 2);
    assert!(f.active_black >= 2400 && f.blank >= 2000);
    f
}

// Preserve the production expressions/body. Only project away unrelated Timing
// assignments from its reset/H=0 blocks; record both full source and leaf hashes.
fn assignments(block: &str, registers: &[&str]) -> String {
    let uncommented = block
        .lines()
        .map(|line| line.split("//").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    uncommented
        .split(';')
        .filter_map(|statement| {
            let statement = statement.trim();
            let (lhs, _) = statement.split_once("<=")?;
            registers
                .contains(&lhs.trim())
                .then(|| format!("{statement};\n"))
        })
        .collect()
}
fn leaf() -> String {
    let source = include_str!("../../src/hardware/display/display_hdmi.v");
    let declarations = source
        .split_once("reg [1:0] read_phase=0;")
        .unwrap()
        .1
        .split_once("always @(posedge pixel_clock) begin")
        .unwrap()
        .0;
    let registers = [
        "read_phase",
        "fetch_pair",
        "pair_bank",
        "raw_pair",
        "r0",
        "r1",
        "g0",
        "g1",
        "output_pair",
        "visible_pipe3",
        "framebuffer_pipe3",
        "hsync_pipe3",
        "vsync_pipe3",
        "precode_pipe",
    ];
    let reset = source
        .split_once("    if (pixel_reset) begin")
        .unwrap()
        .1
        .split_once("    end else begin")
        .unwrap()
        .0;
    let h0 = source
        .split_once("        if (h_count==0) begin")
        .unwrap()
        .1
        .split_once("        end else if (conversion_active) begin")
        .unwrap()
        .0;
    let body = source
        .split_once("        end else if (conversion_active) begin")
        .unwrap()
        .1
        .split_once("        if (scan_enabled && (scan_valid || !group_valid")
        .unwrap()
        .0;
    let pixel_declarations = source
        .split_once("reg visible_pipe3=0")
        .unwrap()
        .1
        .split_once("// Independent bank addresses")
        .unwrap()
        .0;
    let pixel_body = source
        .split_once("        visible_pipe3<=active;")
        .unwrap()
        .1
        .split_once("        if (h_count==0) begin")
        .unwrap()
        .0;
    let sync = source
        .split_once("wire hsync =")
        .unwrap()
        .1
        .split_once("wire line_ready =")
        .unwrap()
        .0;
    include_str!("../display_cycle_pixels_tb.v")
        .replace(
            "__PIXEL_DECLARATIONS__",
            &format!("wire hsync ={sync}\nreg visible_pipe3=0{pixel_declarations}"),
        )
        .replace(
            "__PIXEL_BODY__",
            &format!("visible_pipe3<=active;{pixel_body}"),
        )
        .replace("__CONFIG__", &ACTIVE_DISPLAY_CONFIG.verilog_localparams())
        .replace(
            "__DECLARATIONS__",
            &format!("reg [1:0] read_phase=0;{declarations}"),
        )
        .replace("__RESET__", &assignments(reset, &registers))
        .replace("__H0__", &assignments(h0, &registers))
        .replace(
            "__BODY__",
            &format!("        end else if (conversion_active) begin{body}"),
        )
}
fn compare(text: &str, f: &Fixture, poisoned: bool) -> Result<(), String> {
    let mut count = 0;
    for line in text.lines().filter(|line| line.starts_with("R ")) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != f.expected[count].len() + 2 {
            return Err(format!("trace width {count}: {line}"));
        }
        if fields[1].parse::<usize>().unwrap() != count {
            return Err(format!("trace index {count}"));
        }
        for (actual, expected) in fields[2..].iter().zip(&f.expected[count]) {
            let value = u64::from_str_radix(actual, 16);
            let known = expected.known && !(poisoned && expected.tainted);
            if (known && value.as_ref().ok() != Some(&expected.value)) || (!known && value.is_ok())
            {
                return Err(format!("PIXEL_READ row={count} {} actual={actual} expected={:x} known={known} taint={}",expected.name,expected.value,expected.tainted));
            }
        }
        count += 1;
    }
    if count != f.expected.len() {
        return Err(format!("missing rows {count}/{}", f.expected.len()));
    }
    Ok(())
}
fn run(mut command: Command, out: &Path, name: &str) -> (bool, String) {
    let args = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|v| v.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    std::fs::write(
        out.join(format!("{name}.command.json")),
        serde_json::to_string_pretty(&args).unwrap(),
    )
    .unwrap();
    command.stdout(Stdio::from(
        std::fs::File::create(out.join(format!("{name}.stdout"))).unwrap(),
    ));
    command.stderr(Stdio::from(
        std::fs::File::create(out.join(format!("{name}.stderr"))).unwrap(),
    ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("{name}: 30s cap");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    (
        status.success(),
        std::fs::read_to_string(out.join(format!("{name}.stdout"))).unwrap(),
    )
}

#[test]
fn read_calendar_knownness_and_numeric_pixels() {
    let _ = vectors();
}

#[test]
#[ignore = "bounded production pixel-read projection and real DUT mutations"]
fn production_read_calendar_and_real_mutations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/display-cycle-pixels/c2-v3");
    std::fs::create_dir_all(&out).unwrap();
    let f = vectors();
    std::fs::write(out.join("vectors.txt"), &f.vectors).unwrap();
    let source = leaf();
    let ram = DisplayLineBuffer::verilog_source().unwrap();
    let poison=ram.replace("read_data_a <= memory_a[read_address_a];","read_data_a <= (write_clock && write_enable && write_address==read_address_a) ? 32'bx : memory_a[read_address_a];")
        .replace("read_data_b <= memory_b[read_address_b];","read_data_b <= (write_clock && write_enable && write_address==read_address_b) ? 32'bx : memory_b[read_address_b];");
    assert_ne!(poison, ram);
    let cases = [
        ("production", source.clone(), ram.clone(), true, false),
        ("collision_poison", source.clone(), poison, true, true),
        (
            "wrong_bank",
            source.replace(
                "pair_bank ? line_read_data_b : line_read_data_a",
                "pair_bank ? line_read_data_a : line_read_data_b",
            ),
            ram.clone(),
            false,
            false,
        ),
        (
            "skip_old_q",
            source.replace("raw_pair<=source_pair;", "raw_pair<=pair_bank ? tb.ram.memory_b[line_read_address_b] : tb.ram.memory_a[line_read_address_a];"),
            ram.clone(),
            false,
            false,
        ),
        (
            "wrong_repeat",
            source.replace(
                "wire output_second = read_phase[1]",
                "wire output_second = read_phase[0]",
            ),
            ram.clone(),
            false,
            false,
        ),
        (
            "green_low",
            source.replace("[2*PRECODE_BITS-1:PRECODE_BITS]", "[PRECODE_BITS-1:0]"),
            ram.clone(),
            false,
            false,
        ),
        (
            "reset_pair",
            source.replace(
                "if (pixel_reset) begin",
                "if (pixel_reset) begin output_pair<=0;",
            ),
            ram,
            false,
            false,
        ),
    ];
    let mut cases = cases.to_vec();
    cases.extend([
        (
            "de_ready",
            source.replace(
                "visible_pipe3<=active;",
                "visible_pipe3<=active && scan_line_ready;",
            ),
            DisplayLineBuffer::verilog_source().unwrap(),
            false,
            false,
        ),
        (
            "early_hsync",
            source.replace("hsync_pipe3<=hsync;", "hsync_pipe3<=h_count+1<H_SYNC_END;"),
            DisplayLineBuffer::verilog_source().unwrap(),
            false,
            false,
        ),
        (
            "old_pair_emit",
            source.replace("emit_pair ? converted_pair : output_pair", "output_pair"),
            DisplayLineBuffer::verilog_source().unwrap(),
            false,
            false,
        ),
        (
            "hold_blank",
            source.replace(
                "precode_pipe<=BLACK_PRECODE;",
                "precode_pipe<=precode_pipe;",
            ),
            DisplayLineBuffer::verilog_source().unwrap(),
            false,
            false,
        ),
    ]);
    let mut results = Vec::new();
    for (name, source, ram, positive, poisoned) in cases {
        std::fs::write(out.join(format!("{name}.v")), source).unwrap();
        std::fs::write(out.join(format!("{name}-ram.v")), ram).unwrap();
        let exe = out.join(format!("{name}.vvp"));
        let mut compile =
            Command::new(std::env::var("IVERILOG_EXE").unwrap_or_else(|_| "iverilog".into()));
        compile
            .args(["-g2012", "-s", "tb", "-o"])
            .arg(&exe)
            .arg(out.join(format!("{name}.v")))
            .arg(out.join(format!("{name}-ram.v")));
        assert!(
            run(compile, &out, &format!("{name}-compile")).0,
            "compile {name}"
        );
        let mut simulate = Command::new(std::env::var("VVP_EXE").unwrap_or_else(|_| "vvp".into()));
        simulate
            .arg(exe)
            .arg(format!("+vectors={}", out.join("vectors.txt").display()));
        let (success, trace) = run(simulate, &out, name);
        assert!(success && trace.contains("PASS READ"), "{name}: {trace}");
        let comparison = compare(&trace, &f, poisoned);
        std::fs::write(
            out.join(format!("{name}.diff")),
            format!("{comparison:?}\n"),
        )
        .unwrap();
        if positive {
            assert!(comparison.is_ok(), "{name}: {comparison:?}");
        } else {
            assert!(
                comparison
                    .as_ref()
                    .is_err_and(|e| e.starts_with("PIXEL_READ")),
                "undetected {name}: {comparison:?}"
            );
        }
        results.push(serde_json::json!({"case":name,"expected_pass":positive,"first_difference":comparison.err()}));
    }
    let mut hashes = serde_json::Map::new();
    for relative in [
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/cycle/pixels.rs",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/cycle/pixel_pipe.rs",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/cycle/control.rs",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/cycle/line_buffer.rs",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/cycle/mod.rs",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/display_hdmi.v",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/display_line_buffer.v",
        "systems/cpu-v3-tang-nano-20k/src/hardware/display/precode_table.rs",
        "systems/cpu-v3-tang-nano-20k/src/display.rs",
        "systems/cpu-v3-tang-nano-20k/tests/display/display_cycle_pixels.rs",
        "systems/cpu-v3-tang-nano-20k/tests/display_cycle_pixels_tb.v",
    ] {
        let hash = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["hash-object", relative])
            .output()
            .unwrap();
        assert!(hash.status.success());
        hashes.insert(
            relative.into(),
            String::from_utf8(hash.stdout).unwrap().trim().into(),
        );
    }
    for entry in std::fs::read_dir(&out).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|ext| ext == "v" || ext == "txt")
        {
            let hash = Command::new("git")
                .args(["hash-object"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(hash.status.success());
            hashes.insert(
                path.file_name().unwrap().to_string_lossy().into_owned(),
                String::from_utf8(hash.stdout).unwrap().trim().into(),
            );
        }
    }
    std::fs::write(
        out.join("source-hashes.json"),
        serde_json::to_string_pretty(&hashes).unwrap(),
    )
    .unwrap();
    std::fs::write(out.join("results.json"),serde_json::to_string_pretty(&serde_json::json!({"rows":f.expected.len(),"pixels":f.pixels,"valid":f.valid,"tainted_valid":f.tainted,"unused_unknown":f.unused_unknown,"active_black":f.active_black,"blank":f.blank,"results":results})).unwrap()).unwrap();
}

#[test]
fn control_cold_black_uses_old_timing_and_keeps_encoder_input_registered() {
    let mut display = DisplayControl::default();
    let input = ControlInput {
        video_locked: true,
        boot_enable: true,
        ..Default::default()
    };
    let mut active_black = 0;
    let mut coincident = 0;
    for t in 0..2_400_000u32 {
        let edges = Edges {
            logic_rise: t % 37 == 0,
            pixel_rise: t % 60 == 0,
        };
        if !edges.logic_rise && !edges.pixel_rise {
            continue;
        }
        let old = display.outputs(&input);
        assert_eq!(old, display.outputs(&input));
        assert!(!old.logic.memory_request_valid);
        display.clock(edges, &input);
        let video = display.outputs(&input).video;
        assert_eq!(video.rgb888, [0; 3]);
        assert!(!video.effective_source);
        assert!(video
            .precode
            .channels
            .iter()
            .all(|c| c.known && !c.collision));
        if edges.pixel_rise {
            assert_eq!(video.de, !old.pixel.pixel_reset && old.pixel.active);
            assert_eq!(video.hsync, !old.pixel.pixel_reset && old.pixel.hsync);
            assert_eq!(video.vsync, !old.pixel.pixel_reset && old.pixel.vsync);
            active_black += usize::from(video.de);
            coincident += usize::from(edges.logic_rise);
        } else {
            assert_eq!(video, old.video);
        }
    }
    assert!(active_black > 4000 && coincident > 1000);
    let mut timing: TimingOutput = Timing::default().outputs();
    timing.started = true;
    timing.failure_sync = 2;
    assert!(!PixelReadInput::from(timing).scan_enabled);
    timing.failure_sync = 1;
    assert!(PixelReadInput::from(timing).scan_enabled);
}
