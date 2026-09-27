//! GPU transaction-level trace differential: the Rust model
//! (`GpuCore` + `HostGpuMemory`, driven through the `gpu_tb.v` scenario
//! sequence) and the Icarus RTL run of `gpu.v` + `gpu_tb.v` must emit the same
//! ordered per-port transaction trace and the same global completion points.
//!
//! The testbench responder and the host memory model intentionally differ in
//! cycle timing (recovery cycles vs. write-data backpressure), so only the
//! transaction content sequence is compared, never the cycle-by-cycle timing.
//! The RTL side enforces its own 4M-cycle global bound; the Rust scenario
//! driver bounds every submission wait at 500k cycles.

use cpu_v3_tang_nano_20k::hardware::trace::{compare_traces, rust_cosim_trace, rust_raster_trace};
use cpu_v3_tang_nano_20k::hardware::trace::{
    raster_suite_scenes, rust_raster_suite_trace, RasterSuiteScene,
};
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::Module;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_GPU_RUN: AtomicU64 = AtomicU64::new(0);

const RASTER_IMAGE_WIDTH: usize = 32;
const RASTER_IMAGE_PIXELS: usize = RASTER_IMAGE_WIDTH * RASTER_IMAGE_WIDTH;

struct RtlRun {
    stdout: String,
    raster_frame: Option<String>,
    suite_frames: Vec<String>,
}

#[derive(Default)]
struct FramebufferFixture {
    baseline: bool,
    vendor: bool,
    skip_depth_init: bool,
}

/// Compiles `gpu.v` + `gpu_tb.v` with Icarus and captures the raster image.
fn run_gpu_rtl(raster: bool) -> RtlRun {
    run_gpu_fixture(raster, None, false)
}

fn run_gpu_fixture(raster: bool, suite: Option<&[RasterSuiteScene]>, faults: bool) -> RtlRun {
    run_gpu_source(raster, suite, faults, false)
}

fn run_gpu_source(
    raster: bool,
    suite: Option<&[RasterSuiteScene]>,
    faults: bool,
    baseline: bool,
) -> RtlRun {
    run_gpu_candidate(raster, suite, faults, baseline, 2, false)
}

fn run_gpu_candidate(
    raster: bool,
    suite: Option<&[RasterSuiteScene]>,
    faults: bool,
    baseline: bool,
    pixels: u8,
    scanline: bool,
) -> RtlRun {
    run_gpu_candidate_vectors(raster, suite, faults, baseline, pixels, scanline, None)
}

fn run_gpu_candidate_vectors(
    raster: bool,
    suite: Option<&[RasterSuiteScene]>,
    faults: bool,
    baseline: bool,
    pixels: u8,
    scanline: bool,
    vectors: Option<&str>,
) -> RtlRun {
    run_gpu_candidate_impl(
        raster,
        suite,
        faults,
        FramebufferFixture {
            baseline,
            ..FramebufferFixture::default()
        },
        pixels,
        scanline,
        vectors,
    )
}

fn run_gpu_candidate_impl(
    raster: bool,
    suite: Option<&[RasterSuiteScene]>,
    faults: bool,
    fixture: FramebufferFixture,
    pixels: u8,
    scanline: bool,
    vectors: Option<&str>,
) -> RtlRun {
    let baseline = fixture.baseline;
    let run = NEXT_GPU_RUN.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "gpu-trace-cosim-{}-{run}-{}",
        std::process::id(),
        if raster { "raster" } else { "legacy" }
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let module_path = directory.join("module.v");
    let testbench_path = directory.join("testbench.v");
    let mut module = if baseline {
        ["raster/raster.v", "raster/raster_pixel.v", "gpu.v"]
            .map(|file| {
                let output = std::process::Command::new("git")
                    .args([
                        "show",
                        &format!("54950b6:systems/cpu-v3-tang-nano-20k/src/hardware/gpu/{file}"),
                    ])
                    .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
                    .output()
                    .unwrap();
                assert!(output.status.success(), "baseline GPU source unavailable");
                String::from_utf8(output.stdout).unwrap()
            })
            .join("\n")
    } else {
        CpuV3Gpu::verilog_source().unwrap()
    };
    if !baseline {
        let old = "CpuV3GpuRasterPixel #(.PREFETCH_LIMIT(1), .QUAD_MODE(1)) raster";
        assert!(module.contains(old));
        module = module.replace(old, &format!(
            "CpuV3GpuRasterPixel #(.PREFETCH_LIMIT(1), .QUAD_MODE(1), .PIXELS_PER_CYCLE({pixels}), .SCANLINE({})) raster",
            u8::from(scanline),
        ));
    }
    if fixture.skip_depth_init {
        let initialization = "phase <= PH_DEPTH_CLEAR;";
        assert_eq!(module.matches(initialization).count(), 2);
        module = module.replace(initialization, "render_tile_valid <= raster_tile_access;\n                    phase <= raster_tile_access ? (raster_prefetch_access ? PH_RASTER_RUN : PH_PIXEL_WRITE) : PH_DRAW_APPLY;");
    }
    std::fs::write(&module_path, module).unwrap();
    let mut testbench = CpuV3Gpu::verilog_testbench().unwrap();
    if scanline {
        // Scanline revisits tile rows and can produce more memory traffic.
        // Keep a finite bound sized for the complete candidate suite.
        testbench = testbench.replace("4000000", "32000000");
    }
    if let Some(scenes) = suite {
        testbench = testbench.replace("__RASTER_SUITE__", &suite_vectors(scenes));
    } else if let Some(vectors) = vectors {
        testbench = testbench.replace("__RASTER_SUITE__", vectors);
    }
    if baseline {
        testbench = format!(
            "`define GPU_LEGACY_REQUEST_BEAT\n`define GPU_FRAMEBUFFER_SERIAL_TRACE\n{testbench}"
        );
        testbench = testbench.replace("dut.retire_ack_valid", "(dut.phase == 5'd19 && dut.raster_pixel_valid && dut.raster_pixel_marker)")
            .replace("dut.retire_ack_epoch", "baseline_epoch")
            .replace("dut.retire_ack_tri", "dut.raster_pixel_tri")
            .replace("dut.render_write", "(dut.phase == 5'd21)")
            .replace("dut.draw_epoch", "baseline_epoch")
            .replace("integer tseq = 0;", "integer tseq = 0; integer baseline_epoch = 0;\nalways @(posedge clk) begin if(reset) baseline_epoch <= 0; else if(dut.phase==17 && dut.pending_opcode==8'he2) baseline_epoch <= baseline_epoch+1; end");
    }
    std::fs::write(&testbench_path, testbench).unwrap();

    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let output_path = directory.join("sim.vvp");
    let mut compiler = std::process::Command::new(&iverilog);
    compiler
        .current_dir(&directory)
        .args(["-g2005", "-s", "tb"]);
    if faults {
        compiler.arg("-DGPU_RASTER_FAULTS");
    }
    if suite.is_some() || vectors.is_some() {
        compiler.arg("-DGPU_RASTER_SUITE");
    }
    if raster {
        compiler.arg("-DGPU_RASTER_TEST");
    }
    if fixture.vendor {
        compiler.arg("-DGPU_FRAMEBUFFER_VENDOR");
    }
    compiler
        .arg("-o")
        .arg(&output_path)
        .arg(&module_path)
        .arg(&testbench_path);
    if fixture.vendor {
        let gowin = std::env::var_os("GOWIN_HOME").expect("vendor integration requires GOWIN_HOME");
        compiler.arg(Path::new(&gowin).join("IDE/simlib/gw2a/prim_sim.v"));
    }
    let compile = compiler.output().unwrap();
    assert!(
        compile.status.success(),
        "iverilog compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let simulation = std::process::Command::new(&vvp)
        .current_dir(&directory)
        .arg(&output_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&simulation.stdout).into_owned();
    if fixture.skip_depth_init {
        assert!(
            !simulation.status.success()
                && stdout.contains("integrated depth initialization mismatch"),
            "depth-init mutation did not fail at its semantic audit:\n{stdout}"
        );
    } else {
        assert!(
            simulation.status.success(),
            "RTL simulation failed:\n{stdout}"
        );
    }
    let raster_frame = raster
        .then(|| std::fs::read_to_string(directory.join("raster-frame.hex")).ok())
        .flatten();
    let suite_frames = suite
        .map(|scenes| {
            (0..scenes.len())
                .map(|index| {
                    std::fs::read_to_string(directory.join(format!("suite-{index}.hex")))
                        .unwrap_or_else(|_| panic!("missing scene image {index}:\n{stdout}"))
                })
                .collect()
        })
        .unwrap_or_default();
    std::fs::remove_dir_all(&directory).ok();
    RtlRun {
        stdout,
        raster_frame,
        suite_frames,
    }
}

/// Exact pixel-center edge test for the testbench's (0,0),(32,0),(0,32)
/// triangle. This oracle does not call the GPU model or raster simulator.
fn raster_reference_frame() -> Vec<u16> {
    let vertices = [(0i32, 0i32), (32 * 16, 0), (0, 32 * 16)];
    let mut frame = vec![0x5a5a; RASTER_IMAGE_PIXELS];
    for y in 0..RASTER_IMAGE_WIDTH {
        for x in 0..RASTER_IMAGE_WIDTH {
            let px = (x * 16 + 8) as i32;
            let py = (y * 16 + 8) as i32;
            let covered = (0..3).all(|edge| {
                let (x0, y0) = vertices[edge];
                let (x1, y1) = vertices[(edge + 1) % 3];
                let dx = x1 - x0;
                let dy = y1 - y0;
                let value = -dy * (px - x0) + dx * (py - y0);
                if dy < 0 || (dy == 0 && dx > 0) {
                    value >= 0
                } else {
                    value > 0
                }
            });
            if covered {
                frame[y * RASTER_IMAGE_WIDTH + x] =
                    (((x >> 3) as u16) << 11) | (((y >> 2) as u16) << 5) | ((x >> 4) as u16);
            }
        }
    }
    frame
}

fn write_rgb565(path: &Path, frame: &[u16]) {
    let bytes: Vec<u8> = frame.iter().flat_map(|pixel| pixel.to_le_bytes()).collect();
    std::fs::write(path, bytes).unwrap();
}

fn append_png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc = !0u32;
    for &byte in kind.iter().chain(data) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    png.extend_from_slice(&(!crc).to_be_bytes());
}

/// Minimal RGB PNG writer: uncompressed DEFLATE keeps this test dependency-free.
fn write_png(path: &Path, frame: &[u16]) {
    write_png_sized(path, frame, RASTER_IMAGE_WIDTH, RASTER_IMAGE_WIDTH);
}

fn write_png_sized(path: &Path, frame: &[u16], width: usize, height: usize) {
    assert_eq!(frame.len(), width * height);
    let mut scanlines = Vec::with_capacity(height * (1 + width * 3));
    for row in frame.chunks(width) {
        scanlines.push(0); // PNG filter: none.
        for &pixel in row {
            let r = ((pixel >> 11) & 0x1f) as u8;
            let g = ((pixel >> 5) & 0x3f) as u8;
            let b = (pixel & 0x1f) as u8;
            scanlines.extend_from_slice(&[
                (r << 3) | (r >> 2),
                (g << 2) | (g >> 4),
                (b << 3) | (b >> 2),
            ]);
        }
    }
    let mut zlib = vec![0x78, 0x01];
    for (index, block) in scanlines.chunks(65_535).enumerate() {
        let final_block = (index + 1) * 65_535 >= scanlines.len();
        zlib.push(u8::from(final_block));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &scanlines {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    append_png_chunk(&mut png, b"IHDR", &header);
    append_png_chunk(&mut png, b"IDAT", &zlib);
    append_png_chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).unwrap();
}

#[test]
#[ignore = "explicit GPU transaction trace co-simulation against Icarus RTL"]
fn gpu_trace_matches_rtl() {
    let rust_trace = rust_cosim_trace();
    let RtlRun { stdout, .. } = run_gpu_rtl(false);
    assert!(
        stdout
            .lines()
            .any(|line| line.trim() == "DIGITAL_DESIGN_PASS"),
        "RTL run did not pass:\n{stdout}"
    );
    let rtl_trace: Vec<String> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("GPU "))
        .map(|line| line.trim().to_string())
        .collect();
    if let Err(message) = compare_traces(&rust_trace, &rtl_trace) {
        panic!("GPU transaction trace mismatch:\n{message}");
    }
    println!(
        "PASS gpu trace: {} events, {} rtl lines",
        rust_trace.len(),
        rtl_trace.len()
    );
}

#[test]
#[ignore = "explicit inline-triangle GPU transaction differential"]
fn gpu_raster_trace_matches_rtl() {
    let rust_trace = rust_raster_trace();
    let RtlRun {
        stdout,
        raster_frame,
        ..
    } = run_gpu_rtl(true);
    let actual: Vec<u16> = raster_frame
        .unwrap_or_else(|| panic!("RTL raster image was not written:\n{stdout}"))
        .lines()
        .map(|word| u16::from_str_radix(word.trim(), 16).expect("invalid RTL RGB565 word"))
        .collect();
    assert_eq!(
        actual.len(),
        RASTER_IMAGE_PIXELS,
        "incomplete RTL raster image"
    );
    let expected = raster_reference_frame();
    let diff: Vec<u16> = actual
        .iter()
        .zip(&expected)
        .map(|(actual, expected)| if actual == expected { 0 } else { 0xf800 })
        .collect();
    let artifact_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-raster-image-diff");
    std::fs::create_dir_all(&artifact_dir).unwrap();
    write_rgb565(&artifact_dir.join("actual.rgb565"), &actual);
    write_rgb565(&artifact_dir.join("reference.rgb565"), &expected);
    write_png(&artifact_dir.join("actual.png"), &actual);
    write_png(&artifact_dir.join("reference.png"), &expected);
    write_png(&artifact_dir.join("diff.png"), &diff);
    let mismatches = diff.iter().filter(|&&pixel| pixel != 0).count();
    if let Some(index) = diff.iter().position(|&pixel| pixel != 0) {
        let x = index % RASTER_IMAGE_WIDTH;
        let y = index / RASTER_IMAGE_WIDTH;
        let tile = (y / 16) * 25 + x / 16;
        panic!(
            "RTL raster image: {mismatches} pixel mismatches; first at triangle 0, ({x},{y}), tile {tile}: actual {:04x}, reference {:04x}; images in {}",
            actual[index],
            expected[index],
            artifact_dir.display()
        );
    }
    assert!(
        stdout
            .lines()
            .any(|line| line.trim() == "DIGITAL_DESIGN_RASTER_PASS"),
        "RTL raster run did not pass:\n{stdout}"
    );
    let rtl_trace: Vec<String> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("GPU "))
        .map(|line| line.trim().to_string())
        .collect();
    if let Err(message) = compare_traces(&rust_trace, &rtl_trace) {
        panic!("GPU raster transaction trace mismatch:\n{message}");
    }
    println!(
        "PASS raster RGB565 image: {} pixels, diff={mismatches}",
        actual.len()
    );
    println!("PASS raster GPU trace: {} events", rust_trace.len());
}

#[test]
#[ignore = "explicit eight-DPB vendor primitive production integration"]
fn gpu_framebuffer_vendor_matches_reference() {
    let run = run_gpu_candidate_impl(
        true,
        None,
        false,
        FramebufferFixture {
            vendor: true,
            ..FramebufferFixture::default()
        },
        2,
        false,
        None,
    );
    let frame: Vec<u16> = run
        .raster_frame
        .expect("vendor raster frame missing")
        .lines()
        .map(|word| u16::from_str_radix(word.trim(), 16).unwrap())
        .collect();
    assert_eq!(frame, raster_reference_frame());
    let trace: Vec<String> = run
        .stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("GPU "))
        .map(|line| line.trim().to_string())
        .collect();
    compare_traces(&rust_raster_trace(), &trace).unwrap();
    assert!(run.stdout.contains("DIGITAL_DESIGN_RASTER_PASS"));
}

#[test]
#[ignore = "explicit production depth initialization negative regression"]
fn gpu_framebuffer_missing_depth_init_is_detected() {
    let run = run_gpu_candidate_impl(
        true,
        None,
        false,
        FramebufferFixture {
            skip_depth_init: true,
            ..FramebufferFixture::default()
        },
        2,
        false,
        None,
    );
    assert!(run
        .stdout
        .contains("integrated depth initialization mismatch"));
}

fn suite_vectors(scenes: &[RasterSuiteScene]) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        let base = scene.target;
        writeln!(
            text,
            "$display(\"GPU %0d SCENE {}\", tseq); tseq = tseq + 1; do_reset();",
            scene.name
        )
        .unwrap();
        writeln!(text, "for(i=0;i<96000;i=i+1) mem[22'd{base}+i]=16'h5a5a;").unwrap();
        writeln!(
            text,
            "mem[22'd{}]=16'hbeef; mem[22'd{}]=16'hbeef;",
            base - 1,
            base + 96000
        )
        .unwrap();
        writeln!(text, "for(j=0;j<375;j=j+1) mem[LIST_BASE+j]=j;").unwrap();
        let commands = scene.commands();
        for (qword, value) in commands.iter().enumerate() {
            writeln!(text, "put_qword(CMD_BASE+{},64'h{value:016x});", qword * 4).unwrap();
        }
        writeln!(
            text,
            "scene_start_cycle=cycles; expected_exec=1; run_ok_case(16'd{});",
            commands.len() * 4
        )
        .unwrap();
        writeln!(
            text,
            "$display(\"PERF {} %0d\", cycles-scene_start_cycle);",
            scene.name
        )
        .unwrap();
        writeln!(text, "if(mem[22'd{}]!==16'hbeef || mem[22'd{}]!==16'hbeef) $fatal(1,\"scene {index} guard\");",base-1,base+96000).unwrap();
        writeln!(
            text,
            "for(i=0;i<8;i=i+1) if(dut.cache_dirty[i]) $fatal(1,\"END retained dirty entry\");"
        )
        .unwrap();
        writeln!(text, "raster_image=$fopen(\"suite-{index}.hex\",\"w\");").unwrap();
        writeln!(text, "for(i=0;i<240;i=i+1) for(j=0;j<400;j=j+1) $fdisplay(raster_image,\"%04x\",mem[tile_word(22'd{base},(i/16)*25+j/16,i%16,j%16)]); $fclose(raster_image);").unwrap();
    }
    text.push_str("$display(\"DIGITAL_DESIGN_RASTER_SUITE_PASS\"); $finish;\n");
    text
}

type PixelWrites = BTreeSet<(u16, usize, usize, u16)>;

fn scene_lines<'a>(stdout: &'a str, name: &str) -> Vec<&'a str> {
    let mut active = false;
    stdout
        .lines()
        .filter(|line| {
            if let Some(rest) = line.strip_prefix("GPU ") {
                if let Some((_, scene)) = rest.split_once(" SCENE ") {
                    active = scene == name;
                }
            }
            active
        })
        .collect()
}

fn scene_error(scene: &RasterSuiteScene, frame: &[u16], lines: &[&str]) -> Option<String> {
    let (expected, mut remaining) = suite_oracle(scene);
    let mut acknowledged = 0;
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() == Some(&"PIXEL") {
            let epoch = fields[1].parse::<u16>().unwrap();
            let x = fields[3].parse::<usize>().unwrap();
            let y = fields[4].parse::<usize>().unwrap();
            let color = u16::from_str_radix(fields[5], 16).unwrap();
            if fields[2] != "0" || epoch <= acknowledged || !remaining.remove(&(epoch, x, y, color))
            {
                return Some(format!("unexpected/duplicate/post-ACK pixel: {line}"));
            }
        }
        if fields.len() >= 5 && fields[0] == "GPU" && fields[2] == "RACK" {
            let epoch = fields[3].parse::<u16>().unwrap();
            if epoch != acknowledged + 1
                || fields[4] != "0"
                || remaining.iter().any(|pixel| pixel.0 == epoch)
            {
                return Some(format!(
                    "early, duplicate or out-of-order marker ACK: {line}"
                ));
            }
            acknowledged = epoch;
        }
    }
    if !remaining.is_empty() || usize::from(acknowledged) != scene.triangles.len() {
        return Some(format!(
            "missing pixels/markers: first {:?}, ACKs {acknowledged}",
            remaining.first()
        ));
    }
    if frame.len() != expected.len() {
        return Some("incomplete image".into());
    }
    frame
        .iter()
        .zip(&expected)
        .enumerate()
        .find(|(_, (a, e))| a != e)
        .map(|(index, (a, e))| {
            let x = index % 400;
            let y = index / 400;
            format!(
                "first pixel mismatch ({x},{y}) tile {}: {a:04x} != {e:04x}",
                (y / 16) * 25 + x / 16
            )
        })
}

/// Bounded greedy reduction on failure only. The persisted command stream is
/// reconstructed from the reduced scene, retaining CLEAR/LOAD and target.
fn minimize_failure(scene: &RasterSuiteScene) -> RasterSuiteScene {
    let mut minimal = scene.clone();
    let mut index = 0;
    while index < minimal.triangles.len() {
        let mut candidate = minimal.clone();
        candidate.triangles.remove(index);
        let rtl = run_gpu_fixture(false, Some(std::slice::from_ref(&candidate)), false);
        let image: Vec<u16> = rtl.suite_frames[0]
            .lines()
            .map(|word| u16::from_str_radix(word, 16).unwrap())
            .collect();
        if scene_error(
            &candidate,
            &image,
            &scene_lines(&rtl.stdout, &candidate.name),
        )
        .is_some()
        {
            minimal = candidate;
        } else {
            index += 1;
        }
    }
    minimal
}

/// Direct integer half-plane oracle; no GPU setup/traversal/model is called.
fn suite_oracle(scene: &RasterSuiteScene) -> (Vec<u16>, PixelWrites) {
    let mut frame = vec![scene.clear.unwrap_or(0x5a5a); 400 * 240];
    let mut writes = BTreeSet::new();
    for (triangle, vertices) in scene.triangles.iter().enumerate() {
        let v = vertices.map(|p| p.map(i64::from));
        let area =
            (v[1][0] - v[0][0]) * (v[2][1] - v[0][1]) - (v[1][1] - v[0][1]) * (v[2][0] - v[0][0]);
        if area <= 0 {
            continue;
        }
        for y in 0..240 {
            for x in 0..400 {
                let point = [x as i64 * 16 + 8, y as i64 * 16 + 8];
                let covered = (0..3).all(|i| {
                    let a = v[i];
                    let b = v[(i + 1) % 3];
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let e = dx * (point[1] - a[1]) - dy * (point[0] - a[0]);
                    e > 0 || (e == 0 && (dy < 0 || (dy == 0 && dx > 0)))
                });
                if covered {
                    let color = (((x >> 3) & 31) as u16) << 11
                        | (((y >> 2) & 63) as u16) << 5
                        | ((x >> 4) & 31) as u16;
                    frame[y * 400 + x] = color;
                    writes.insert(((triangle + 1) as u16, x, y, color));
                }
            }
        }
    }
    (frame, writes)
}

#[test]
#[ignore = "explicit GPU/cache image and retirement integration differential"]
fn gpu_raster_boundary_suite_matches_rtl() {
    let scenes = raster_suite_scenes();
    let expected_trace = rust_raster_suite_trace(&scenes);
    let rtl = run_gpu_fixture(false, Some(&scenes), false);
    assert!(
        rtl.stdout.contains("DIGITAL_DESIGN_RASTER_SUITE_PASS"),
        "{}",
        rtl.stdout
    );
    let actual_trace: Vec<String> = rtl
        .stdout
        .lines()
        .filter(|line| line.starts_with("GPU "))
        .map(str::to_owned)
        .collect();
    compare_traces(&expected_trace, &actual_trace).unwrap_or_else(|error| panic!("{error}"));
    let artifact_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-raster-boundary-suite");
    std::fs::create_dir_all(&artifact_dir).unwrap();
    for (scene, image) in scenes.iter().zip(&rtl.suite_frames) {
        let actual: Vec<u16> = image
            .lines()
            .map(|word| u16::from_str_radix(word, 16).unwrap())
            .collect();
        let expected = suite_oracle(scene).0;
        assert_eq!(actual.len(), expected.len());
        let diff: Vec<u16> = actual
            .iter()
            .zip(&expected)
            .map(|(a, e)| if a == e { 0 } else { 0xf800 })
            .collect();
        let dir = artifact_dir.join(&scene.name);
        std::fs::create_dir_all(&dir).unwrap();
        write_rgb565(&dir.join("actual.rgb565"), &actual);
        write_rgb565(&dir.join("reference.rgb565"), &expected);
        write_png_sized(&dir.join("actual.png"), &actual, 400, 240);
        write_png_sized(&dir.join("reference.png"), &expected, 400, 240);
        write_png_sized(&dir.join("diff.png"), &diff, 400, 240);
        std::fs::write(
            dir.join("commands.hex"),
            scene
                .commands()
                .iter()
                .map(|q| format!("{q:016x}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let scene_output = scene_lines(&rtl.stdout, &scene.name);
        if let Some(error) = scene_error(scene, &actual, &scene_output) {
            let minimized = minimize_failure(scene);
            std::fs::write(
                dir.join("minimized-commands.hex"),
                minimized
                    .commands()
                    .iter()
                    .map(|q| format!("{q:016x}\n"))
                    .collect::<String>(),
            )
            .unwrap();
            std::fs::write(dir.join("minimized-scene.txt"), format!("{minimized:#?}")).unwrap();
            panic!(
                "{}: {error}; replay and images in {}",
                scene.name,
                dir.display()
            );
        }
        if let Some(index) = diff.iter().position(|&p| p != 0) {
            let x = index % 400;
            let y = index / 400;
            panic!(
                "{} first mismatch: ({x},{y}), tile {}, actual {:04x} expected {:04x}; {}",
                scene.name,
                (y / 16) * 25 + x / 16,
                actual[index],
                expected[index],
                dir.display()
            );
        }
    }
    println!("PASS {} integration scenes: {} pixels, complete pixel/ACK conservation and transaction parity",scenes.len(),scenes.len()*400*240);
}

#[test]
#[ignore = "explicit four-candidate full-image/cache/marker validation and throughput"]
fn gpu_raster_candidate_images_and_throughput() {
    let scenes = raster_suite_scenes();
    let mut csv = String::from(
        "traversal,pixels_per_cycle,scene,cycles,fb_read_requests,fb_write_requests\n",
    );
    for scanline in [false, true] {
        for pixels in [1, 2] {
            let rtl = run_gpu_candidate(false, Some(&scenes), false, false, pixels, scanline);
            assert_eq!(rtl.suite_frames.len(), scenes.len());
            for (scene, frame) in scenes.iter().zip(&rtl.suite_frames) {
                let image: Vec<u16> = frame
                    .lines()
                    .map(|s| u16::from_str_radix(s, 16).unwrap())
                    .collect();
                let output = scene_lines(&rtl.stdout, &scene.name);
                if let Some(error) = scene_error(scene, &image, &output) {
                    panic!(
                        "candidate scanline={scanline} pixels={pixels}, {}: {error}",
                        scene.name
                    );
                }
                assert_eq!(
                    image,
                    suite_oracle(scene).0,
                    "candidate image {}",
                    scene.name
                );
            }
            let cycles: Vec<u64> = rtl
                .stdout
                .lines()
                .filter_map(|s| s.strip_prefix("PERF "))
                .map(|s| s.split_once(' ').unwrap().1.parse().unwrap())
                .collect();
            assert_eq!(cycles.len(), scenes.len());
            let traversal = if scanline { "scanline" } else { "tile" };
            for (scene, cycles) in scenes.iter().zip(cycles) {
                let output = scene_lines(&rtl.stdout, &scene.name);
                let reads = output.iter().filter(|s| s.contains("REQ fb_r R")).count();
                let writes = output.iter().filter(|s| s.contains("REQ fb_w W")).count();
                csv.push_str(&format!(
                    "{traversal},{pixels},{},{cycles},{reads},{writes}\n",
                    scene.name
                ));
            }
            // Every read/write error beat must terminate instead of leaking
            // pixels or retiring an uncommitted marker, in every candidate.
            let faults = run_gpu_candidate(false, None, true, false, pixels, scanline);
            assert_eq!(
                faults
                    .stdout
                    .lines()
                    .filter(|s| s.starts_with("FAULT_PASS"))
                    .count(),
                36
            );
        }
    }
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-raster-candidates");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("cache-throughput.csv"), csv).unwrap();
}

#[test]
#[ignore = "explicit exhaustive reserved payload bits and stale tag reset regression"]
fn gpu_reserved_payload_bits_and_tag_reset() {
    // Each reserved bit must reject independently, with no framebuffer request.
    // This checks the external command contract rather than the packed storage.
    let mut vectors = String::from("    do_reset(); expected_exec=1;\n");
    let mut count = 0;
    for slot in 0..3 {
        for bit in 32..64 {
            vectors.push_str(&format!(
                "    $display(\"GPU %0d SCENE reserved-tri-{slot}-{bit}\",tseq); tseq=tseq+1;\n\
                 put_set_target(CMD_BASE,FB_A); put_qword(CMD_BASE+4,64'h00000000000004e2);\n\
                 put_qword(CMD_BASE+8,0); put_qword(CMD_BASE+12,64'h100); put_qword(CMD_BASE+16,64'h01000000);\n\
                 put_qword(CMD_BASE+{},64'h{:016x}); run_error_case(20);\n",
                8 + slot * 4, 1u64 << bit,
            ));
            count += 1;
        }
    }
    for (slot, first_bit) in [(0, 32), (1, 49)] {
        for bit in first_bit..64 {
            vectors.push_str(&format!(
                "    $display(\"GPU %0d SCENE reserved-fake-{slot}-{bit}\",tseq); tseq=tseq+1;\n\
                 put_set_target(CMD_BASE,FB_A); put_fake_draw(CMD_BASE+4,LIST_BASE,0,0,0,0,0);\n\
                 put_qword(CMD_BASE+{},64'h{:016x}); run_error_case(16);\n",
                8 + slot * 4,
                1u64 << bit,
            ));
            count += 1;
        }
    }
    // Populate a nonzero tag, soft reset, change target and load the same index.
    // A stale tag must neither suppress the refill nor write back old data.
    vectors.push_str(
        "\
    $display(\"GPU %0d SCENE stale-tag-before\",tseq); tseq=tseq+1;\n\
    put_set_target(CMD_BASE,FB_A); put_fake_draw(CMD_BASE+4,LIST_BASE,1,1,16'h1234,0,0);\n\
    mem[LIST_BASE]=8; put_end(CMD_BASE+16); run_ok_case(20);\n\
    device_write(5,1);\n\
    if(dut.cache_valid[0]!==0) $fatal(1,\"soft reset retained a valid cache entry\");\n\
    for(i=0;i<256;i=i+1) mem[FB_B+2048+i]=16'h9876;\n\
    $display(\"GPU %0d SCENE stale-tag-after\",tseq); tseq=tseq+1;\n\
    put_set_target(CMD_BASE,FB_B); put_fake_draw(CMD_BASE+4,LIST_BASE,1,0,0,0,0);\n\
    put_end(CMD_BASE+16); run_ok_case(20); check_uniform_tile(8,FB_B,16'h9876);\n\
    $display(\"RESERVED_TAG_PASS\"); $finish;\n",
    );
    let rtl = run_gpu_candidate_vectors(false, None, false, false, 2, false, Some(&vectors));
    assert!(rtl.stdout.contains("RESERVED_TAG_PASS"));
    let reserved: Vec<_> = rtl
        .stdout
        .lines()
        .filter(|s| s.contains("SCENE reserved-"))
        .collect();
    assert_eq!(reserved.len(), count);
    for line in reserved {
        let name = line.split("SCENE ").nth(1).unwrap();
        let output = scene_lines(&rtl.stdout, name);
        assert!(
            !output.iter().any(|s| s.contains("REQ fb_")),
            "reserved bit reached framebuffer: {name}"
        );
    }
    let output = scene_lines(&rtl.stdout, "stale-tag-after");
    assert_eq!(
        output.iter().filter(|s| s.contains("REQ fb_r R")).count(),
        4
    );
    assert_eq!(
        output.iter().filter(|s| s.contains("REQ fb_w W")).count(),
        4
    );
}

#[test]
#[ignore = "explicit maximum command count and 32-bit list address rejection"]
fn gpu_command_count_and_address_boundaries() {
    let mut vectors = String::from(
        "\
    do_reset(); expected_exec=1;\n\
    $display(\"GPU %0d SCENE maximum-count\",tseq); tseq=tseq+1;\n\
    for(i=0;i<16382;i=i+1) put_set_target(CMD_BASE+i*4,FB_A);\n\
    put_end(CMD_BASE+65528); run_ok_case(16'd65532);\n\
    $display(\"GPU %0d SCENE terminal-payload-overrun\",tseq); tseq=tseq+1;\n\
    put_qword(CMD_BASE+65528,64'h00000000000003e1); run_error_case(16'd65532);\n",
    );
    for bit in 22..32 {
        for (count, run) in [(0, "run_ok_case"), (1, "run_error_case")] {
            vectors.push_str(&format!(
                "    $display(\"GPU %0d SCENE address-{count}-{bit}\",tseq); tseq=tseq+1;\n\
                 put_set_target(CMD_BASE,FB_A); put_fake_draw(CMD_BASE+4,LIST_BASE,{count},0,0,0,0);\n\
                 put_qword(CMD_BASE+8,64'h{:016x}); put_end(CMD_BASE+16); {run}(20);\n",
                1u64 << bit,
            ));
        }
    }
    vectors.push_str("    $display(\"COMMAND_BOUNDARY_PASS\"); $finish;\n");
    let rtl = run_gpu_candidate_vectors(false, None, false, false, 2, false, Some(&vectors));
    assert!(rtl.stdout.contains("COMMAND_BOUNDARY_PASS"));
    for name in ["maximum-count", "terminal-payload-overrun"] {
        let output = scene_lines(&rtl.stdout, name);
        let requests: Vec<_> = output.iter().filter(|s| s.contains("REQ ro R")).collect();
        assert_eq!(requests.len(), 4096, "{name}: command line count/wrap");
        assert!(
            requests.last().unwrap().contains("0100f0"),
            "{name}: terminal address"
        );
        assert!(!output.iter().any(|s| s.contains("REQ fb_")));
    }
    for bit in 22..32 {
        for count in [0, 1] {
            let name = format!("address-{count}-{bit}");
            let output = scene_lines(&rtl.stdout, &name);
            let requests = output.iter().filter(|s| s.contains("REQ ro R")).count();
            assert_eq!(
                requests,
                if count == 0 { 2 } else { 1 },
                "{name}: unexpected list access"
            );
            assert!(!output.iter().any(|s| s.contains("REQ fb_")), "{name}");
        }
    }
}

#[test]
#[ignore = "explicit GPU/cache arbitrary-beat error retirement RTL test"]
fn gpu_raster_errors_retire_once_without_committing_failed_transactions() {
    let rtl = run_gpu_fixture(false, None, true);
    assert!(
        rtl.stdout.contains("DIGITAL_DESIGN_RASTER_FAULTS_PASS"),
        "{}",
        rtl.stdout
    );
    assert_eq!(
        rtl.stdout
            .lines()
            .filter(|line| line.starts_with("FAULT_PASS"))
            .count(),
        36
    );
}

#[test]
#[ignore = "explicit isolated raster/cache throughput comparison against 54950b6"]
fn gpu_raster_cache_throughput_comparison() {
    let scenes: Vec<_> = raster_suite_scenes()
        .into_iter()
        .filter(|scene| {
            ["shared_edge", "alias_eviction", "wide_triangle"].contains(&scene.name.as_str())
        })
        .collect();
    let before = run_gpu_source(false, Some(&scenes), false, true);
    let after = run_gpu_fixture(false, Some(&scenes), false);
    assert_eq!(
        before.suite_frames, after.suite_frames,
        "optimization changed output image"
    );
    let cycles = |stdout: &str| {
        stdout
            .lines()
            .filter_map(|line| line.strip_prefix("PERF "))
            .map(|line| line.split_once(' ').unwrap().1.parse::<u64>().unwrap())
            .collect::<Vec<_>>()
    };
    let old = cycles(&before.stdout);
    let new = cycles(&after.stdout);
    assert_eq!(old.len(), scenes.len());
    assert_eq!(new.len(), scenes.len());
    let mut csv = String::from("scene,baseline_commit,baseline_cycles,current_cycles,ratio\n");
    for ((scene, before), after) in scenes.iter().zip(old).zip(new) {
        assert!(
            after < before,
            "renderer did not improve: {} {before} -> {after}",
            scene.name
        );
        let ratio = after as f64 / before as f64;
        csv.push_str(&format!(
            "{},54950b6,{before},{after},{ratio:.6}\n",
            scene.name
        ));
        println!("{}: {before} -> {after} cycles ({ratio:.3}x)", scene.name);
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-raster-boundary-suite");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("throughput.csv"), csv).unwrap();
}
