//! New small geometry; the frozen G3 fixtures and their 45 inputs stay unchanged.
use crate::triangle_support;
use std::collections::BTreeSet;
fn cross(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn data() -> triangle_support::Data {
    data_mode(true, 0)
}
#[allow(dead_code)]
pub fn uv() -> triangle_support::Data {
    data_mode(false, 2048)
}
fn write_word(rows: &mut [(u16, u64)], word: usize, value: u32) {
    let row = rows
        .iter_mut()
        .find(|r| usize::from(r.0) == word / 2)
        .unwrap();
    let shift = (word & 1) * 32;
    row.1 = (row.1 & !(u64::from(u32::MAX) << shift)) | (u64::from(value) << shift);
}
fn data_mode(rgb: bool, x_shift: i128) -> triangle_support::Data {
    let case = triangle_support::Case {
        rgb,
        ..Default::default()
    };
    let mut data = triangle_support::data(case);
    // Keep index/position matrix; use a tiny triangle and constant attributes.
    let metadata = data.rows.iter_mut().find(|r| r.0 == 259).unwrap();
    metadata.1 = (metadata.1 & !(255u64 << 32)) | (3u64 << 32);
    if !rgb {
        let row = data.rows.iter_mut().find(|r| r.0 == 258).unwrap();
        row.1 = (row.1 & !u64::from(u32::MAX)) | u64::from((-65536 + x_shift) as i32 as u32);
        // Raw Z normal256 times normal-matrix scale2 gives exact S12F10 Z1024.
        let row = data.rows.iter_mut().find(|r| r.0 == 330).unwrap();
        row.1 = (row.1 & !u64::from(u32::MAX)) | 131072;
    }
    for v in 0..3 {
        let attr = if rgb { 0xa1b2c3 } else { 0x1f00_3f00 };
        write_word(&mut data.rows, v * 3 + 2, attr);
        data.vertices[v * 5 + 4] = u64::from(attr);
        if !rgb {
            write_word(&mut data.rows, v * 3 + 1, 256 << 20);
            data.vertices[v * 5 + 3] = 1024 << 24;
        }
    }
    let matrix: [i128; 12] = std::array::from_fn(|n| {
        let row = data
            .rows
            .iter()
            .find(|r| usize::from(r.0) == 320 + n / 2)
            .unwrap()
            .1;
        i128::from((row >> ((n & 1) * 32)) as u32 as i32)
    });
    let world = [
        [-65536 + x_shift, -65536, 262144],
        [-61440 + x_shift, -65536, 262144],
        [-65536 + x_shift, -61440, 262144],
    ];
    let p: [[i128; 3]; 3] = world.map(|v| {
        std::array::from_fn(|j| {
            let raw =
                (0..3).map(|k| v[k] * matrix[j * 4 + k]).sum::<i128>() + 65536 * matrix[j * 4 + 3];
            i128::from((raw >> 16) as u32 as i32)
        })
    });
    for (v, p) in p.iter().enumerate() {
        for (k, &n) in p.iter().enumerate() {
            data.vertices[v * 5 + k] = n as u32 as u64;
        }
    }
    let mut n = [cross(p[1], p[2]), cross(p[2], p[0]), cross(p[0], p[1])];
    let mut q = [0; 3];
    for (j, n) in n.iter_mut().enumerate() {
        q[j] = n
            .iter()
            .map(|&v| 128 - (if v < 0 { !v } else { v }).unsigned_abs().leading_zeros())
            .max()
            .unwrap()
            .saturating_sub(26);
        for v in n {
            *v = if *v < 0 { -((-*v) >> q[j]) } else { *v >> q[j] };
        }
    }
    let det = p[0].into_iter().zip(n[0]).map(|(a, b)| a * b).sum::<i128>();
    assert!(det > 0);
    let mut words = [0; 16];
    for j in 0..3 {
        let [x, y, z] = n[j];
        words[3 * j] = (x as u64 & ((1 << 36) - 1)) | (u64::from(q[j]) << 48);
        words[3 * j + 1] = y as u64 & ((1 << 54) - 1);
        words[3 * j + 2] = (-399 * x - 239 * y + 512 * z) as u64 & ((1 << 54) - 1);
    }
    let bit = 127 - det.unsigned_abs().leading_zeros();
    let normalized = if bit >= 30 {
        det.unsigned_abs() >> (bit - 30)
    } else {
        det.unsigned_abs() << (30 - bit)
    };
    words[9] = normalized as u64 | (u64::from(bit + q[0]) << 31);
    for v in 0..3 {
        words[10 + 2 * v] = data.vertices[v * 5 + 3];
        words[11 + 2 * v] = data.vertices[v * 5 + 4];
    }
    words[10] |= u64::from(case.color & ((1 << 18) - 1)) << 36;
    words[11] |= u64::from(case.color >> 18) << 36;
    data.packets[0].1 = words;
    data.per_triangle[0] = Some(words);
    data
}
fn signed(n: u64, w: u32) -> i64 {
    ((n << (64 - w)) as i64) >> (64 - w)
}
pub fn coverage(data: &triangle_support::Data) -> BTreeSet<(usize, usize)> {
    let words = data.packets[0].1;
    let geo: [[i64; 3]; 3] = std::array::from_fn(|j| {
        let row = [
            signed(words[j * 3], 27),
            signed(words[j * 3 + 1], 27),
            signed(words[j * 3 + 2], 38),
        ];
        let shift = row
            .iter()
            .enumerate()
            .map(|(k, n)| {
                (64 - n.unsigned_abs().leading_zeros()).saturating_sub(if k == 2 { 27 } else { 17 })
            })
            .max()
            .unwrap();
        row.map(|v| v / (1i64 << shift))
    });
    (0..240usize)
        .flat_map(|y| (0..400).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            geo.into_iter().all(|n| {
                let h = n[0] * 2 * x as i64 + n[1] * 2 * y as i64 + n[2];
                h > 0 || (h == 0 && (n[0] > 0 || (n[0] == 0 && n[1] > 0)))
            })
        })
        .collect()
}
