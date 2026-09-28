//! Render a 400x240 RGB565 meshlet scene with the functional geometry sim.
//! Usage: cargo run -p cpu-v3-tang-nano-20k --example gpu_microcore_sim -- <output-dir>
use cpu_v3_tang_nano_20k::hardware::geometry_raster::microcore_sim::{
    run_meshlet, Meshlet, RawVertex, SimRecord,
};
use std::io::Write;
use std::path::PathBuf;

const WIDTH: usize = 400;
const HEIGHT: usize = 240;
const Q: i32 = 65_536;

fn vertex(x: i32, y: i32, z: i32, rgb565: u16) -> RawVertex {
    RawVertex {
        position: [x, y, z, Q],
        rgb565,
    }
}

fn identity() -> [[i32; 4]; 4] {
    let mut matrix = [[0; 4]; 4];
    for (axis, row) in matrix.iter_mut().enumerate() {
        row[axis] = Q;
    }
    matrix
}

fn rgb888(code: u16) -> [u8; 3] {
    let r = (code >> 11) as u8 & 31;
    let g = (code >> 5) as u8 & 63;
    let b = code as u8 & 31;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/gpu-microcore-sim"));
    std::fs::create_dir_all(&output)?;
    let scenes = [
        Meshlet {
            matrix: identity(),
            vertices: vec![
                vertex(-45_875, -36_045, Q / 2, 0xf800),
                vertex(0, 49_152, Q / 2, 0x07e0),
                vertex(45_875, -36_045, Q / 2, 0x001f),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Meshlet {
            matrix: identity(),
            vertices: vec![
                vertex(-19_661, -19_661, -Q / 2, 0xffe0),
                vertex(22_938, -16_384, Q / 2, 0xffff),
                vertex(0, 19_661, Q / 2, 0xf81f),
            ],
            triangles: vec![[0, 2, 1], [0, 1, 63]],
        },
    ];
    let mut pixels = vec![[18, 22, 38]; WIDTH * HEIGHT];
    let mut summary = String::new();
    let mut trace = std::io::BufWriter::new(std::fs::File::create(output.join("trace.txt"))?);
    for (slot, meshlet) in scenes.iter().enumerate() {
        // Exercise the exact word image consumed by the raw-slot RTL interface.
        let words = meshlet.words().expect("meshlet fits raw slot");
        let meshlet = Meshlet::from_words(&words, meshlet.vertices.len(), meshlet.triangles.len())
            .expect("raw slot round trip");
        let records = run_meshlet(slot as u8, &meshlet, 100_000).expect("bounded functional sim");
        let mut fans = 0;
        let mut quads = 0;
        let mut sources = 0;
        let mut faults = 0;
        for record in records {
            writeln!(trace, "{record:?}")?;
            match record {
                SimRecord::Fan { .. } => fans += 1,
                SimRecord::Quad {
                    x, y, mask, color, ..
                } => {
                    quads += 1;
                    for (lane, code) in color.iter().enumerate() {
                        if mask & (1 << lane) == 0 {
                            continue;
                        }
                        let px = usize::from(x) + lane % 2;
                        let py = usize::from(y) + lane / 2;
                        if px < WIDTH && py < HEIGHT {
                            pixels[py * WIDTH + px] = rgb888(*code);
                        }
                    }
                }
                SimRecord::SourceEnd { fault, .. } => {
                    sources += 1;
                    faults += usize::from(fault.is_some());
                }
                SimRecord::TileEnd { .. } => {}
                SimRecord::PrimitiveEnd { .. } => {}
            }
        }
        summary.push_str(&format!(
            "slot {slot}: sources={sources}, fan_triangles={fans}, covered_quads={quads}, source_faults={faults}\n"
        ));
    }
    let mut ppm = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
    for pixel in pixels {
        ppm.extend_from_slice(&pixel);
    }
    std::fs::write(output.join("scene.ppm"), &ppm)?;
    // BMP is directly viewable on Windows without an extra conversion tool.
    let row_stride = (WIDTH * 3 + 3) & !3;
    let mut bmp = Vec::with_capacity(54 + row_stride * HEIGHT);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((54 + row_stride * HEIGHT) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(WIDTH as i32).to_le_bytes());
    bmp.extend_from_slice(&(HEIGHT as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&((row_stride * HEIGHT) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 16]);
    let raw = &ppm[format!("P6\n{WIDTH} {HEIGHT}\n255\n").len()..];
    for y in (0..HEIGHT).rev() {
        for x in 0..WIDTH {
            let at = (y * WIDTH + x) * 3;
            bmp.extend_from_slice(&[raw[at + 2], raw[at + 1], raw[at]]);
        }
        bmp.resize(bmp.len() + row_stride - WIDTH * 3, 0);
    }
    std::fs::write(output.join("scene.bmp"), bmp)?;
    std::fs::write(output.join("summary.txt"), &summary)?;
    trace.flush()?;
    print!("{summary}");
    println!("scene: {}", output.join("scene.bmp").display());
    println!("trace: {}", output.join("trace.txt").display());
    Ok(())
}
