//! Deterministic, actual triangle meshes for oracle review. No per-pixel fake
//! geometry normals or browser shading implementation.
use super::ports::*;
use crate::{framebuffer::ports as fb, lighting::ports as light};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub scene: u32,
    pub frame: u32,
    pub yaw: f64,
    pub pitch: f64,
    pub shininess: u8,
    pub specular: u8,
    pub ambient: u16,
    pub directional: u16,
    pub width: u16,
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            scene: 0,
            frame: 0,
            yaw: 25.0,
            pitch: -20.0,
            shininess: 8,
            specular: 64,
            ambient: 32,
            directional: 192,
            width: 400,
        }
    }
}
pub fn build(p: Parameters) -> Result<Scene, String> {
    if p.scene > 3
        || p.frame >= 120
        || ![200, 400].contains(&p.width)
        || !p.yaw.is_finite()
        || !p.pitch.is_finite()
        || p.yaw.abs() > 180.0
        || p.pitch.abs() > 80.0
        || p.shininess > 16
        || p.ambient > 256
        || p.directional > 256
    {
        return Err("scene parameter bounds".into());
    }
    let angle = (p.frame as f64 / 120.0 * 2.0 * PI).sin() * 0.35;
    let (s, c) = angle.sin_cos();
    let rotation = [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]];
    let fx = 1.6 * 0.6;
    let distance = if p.scene == 3 { 0.95 } else { 3.0 };
    let mut mvp = [[0.0; 4]; 4];
    for k in 0..3 {
        mvp[0][k] = fx * rotation[0][k];
        mvp[1][k] = 1.6 * rotation[1][k];
        mvp[3][k] = -rotation[2][k];
    }
    mvp[3][3] = distance;
    let (yaw, pitch) = (p.yaw.to_radians(), p.pitch.to_radians());
    let direction = [
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
    .map(|v| (v * 16384.0).round_ties_even() as i16);
    let mut scene = Scene {
        vertices: Vec::new(),
        triangles: Vec::new(),
        mvp,
        normal_matrix: rotation,
        compact_base: [-65536; 3],
        compact_grid_shift: 8,
        light: light::Light {
            direction,
            ambient: p.ambient,
            directional: p.directional,
        },
        material: light::Material {
            shininess_code: p.shininess,
            specular_color: [p.specular; 3],
            unlit: false,
        },
        // View vector points from surface toward the camera; viewport y is inverted
        // exactly once by triangle projection. k/fx <= .75 across the viewport.
        projection: light::Projection {
            ray_scale: [(-0.5 / fx * 16384.0).round_ties_even() as i16, -5120],
            k: 8192,
        },
        rop: fb::Context {
            depth: fb::DepthFunc::Less,
            depth_write: true,
            blend: fb::Blend::Replace,
        },
        width: p.width,
        height: p.width * 3 / 5,
        textured: p.scene == 2,
    };
    if p.scene == 2 {
        let positions = [
            [-1.0, -0.85, 0.65],
            [1.0, -0.85, -0.65],
            [-1.0, 0.85, 0.65],
            [1.0, 0.85, -0.65],
        ];
        let normal = [0.65 / 1.1926860441876563, 0.0, 1.0 / 1.1926860441876563];
        for (i, position) in positions.into_iter().enumerate() {
            scene.vertices.push(MeshVertex {
                position,
                normal,
                uv: [(i % 2) as f64, (i / 2) as f64],
                tint: [0.65; 3],
            });
        }
        scene.triangles = vec![[0, 1, 2], [1, 3, 2]];
    } else {
        let (longitude, latitude) = if p.scene == 1 { (8, 5) } else { (24, 14) };
        // Duplicate seam vertices preserve UV discontinuity. Zero-area pole fans
        // are omitted rather than relying on a floating singular source.
        for y in 0..=latitude {
            let v = y as f64 / latitude as f64;
            let (sy, cy) = (PI * v).sin_cos();
            for x in 0..=longitude {
                let u = x as f64 / longitude as f64;
                let (sx, cx) = (2.0 * PI * u).sin_cos();
                let position = [sy * sx, cy, sy * cx];
                scene.vertices.push(MeshVertex {
                    position,
                    normal: position,
                    uv: [u, v],
                    tint: [0.34, 0.19, 0.10],
                });
            }
        }
        for y in 0..latitude {
            for x in 0..longitude {
                let a = y * (longitude + 1) + x;
                let b = a + longitude + 1;
                if y > 0 {
                    scene.triangles.push([a, a + 1, b]);
                }
                if y + 1 < latitude {
                    scene.triangles.push([a + 1, b + 1, b]);
                }
            }
        }
    }
    Ok(scene)
}
