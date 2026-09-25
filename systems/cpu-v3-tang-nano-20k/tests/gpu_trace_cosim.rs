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
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::Module;
use std::path::Path;

const RASTER_IMAGE_WIDTH: usize = 32;
const RASTER_IMAGE_PIXELS: usize = RASTER_IMAGE_WIDTH * RASTER_IMAGE_WIDTH;

struct RtlRun {
    stdout: String,
    raster_frame: Option<String>,
}

/// Compiles `gpu.v` + `gpu_tb.v` with Icarus and captures the raster image.
fn run_gpu_rtl(raster: bool) -> RtlRun {
    let directory = std::env::temp_dir().join(format!(
        "gpu-trace-cosim-{}-{}",
        std::process::id(),
        if raster { "raster" } else { "legacy" }
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let module_path = directory.join("module.v");
    let testbench_path = directory.join("testbench.v");
    std::fs::write(&module_path, CpuV3Gpu::verilog_source().unwrap()).unwrap();
    std::fs::write(&testbench_path, CpuV3Gpu::verilog_testbench().unwrap()).unwrap();

    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let output_path = directory.join("sim.vvp");
    let mut compiler = std::process::Command::new(&iverilog);
    compiler
        .current_dir(&directory)
        .args(["-g2005", "-s", "tb"]);
    if raster {
        compiler.arg("-DGPU_RASTER_TEST");
    }
    let compile = compiler
        .arg("-o")
        .arg(&output_path)
        .arg(&module_path)
        .arg(&testbench_path)
        .output()
        .unwrap();
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
    let raster_frame = raster
        .then(|| std::fs::read_to_string(directory.join("raster-frame.hex")).ok())
        .flatten();
    std::fs::remove_dir_all(&directory).ok();
    RtlRun {
        stdout,
        raster_frame,
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
    assert_eq!(frame.len(), RASTER_IMAGE_PIXELS);
    let mut scanlines = Vec::with_capacity(RASTER_IMAGE_WIDTH * (1 + RASTER_IMAGE_WIDTH * 3));
    for row in frame.chunks(RASTER_IMAGE_WIDTH) {
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
    header.extend_from_slice(&(RASTER_IMAGE_WIDTH as u32).to_be_bytes());
    header.extend_from_slice(&(RASTER_IMAGE_WIDTH as u32).to_be_bytes());
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
