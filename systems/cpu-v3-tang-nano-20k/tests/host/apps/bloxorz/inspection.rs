use super::*;

pub(super) fn goal_region(catalog: &serde_json::Value) -> (usize, usize, usize, usize) {
    let c = &catalog["cameras"][0];
    let eye: [f64; 3] = std::array::from_fn(|i| c["eye"][i].as_f64().unwrap());
    let target = [0., 0.6, 0.];
    let mut back: [f64; 3] = std::array::from_fn(|i| eye[i] - target[i]);
    let n = back.iter().map(|x| x * x).sum::<f64>().sqrt();
    for x in &mut back {
        *x /= n;
    }
    let mut right = [back[2], 0., -back[0]];
    let n = right.iter().map(|x| x * x).sum::<f64>().sqrt();
    for x in &mut right {
        *x /= n;
    }
    let up = [
        back[1] * right[2] - back[2] * right[1],
        back[2] * right[0] - back[0] * right[2],
        back[0] * right[1] - back[1] * right[0],
    ];
    let f = c["focal"].as_f64().unwrap();
    let mut bounds = [400f64, 240., 0., 0.];
    // Real stage1 goal(7,4) and its adjacent tiles, including well depth.
    for x in [1., 4.] {
        for y in [-2.2, 0.05] {
            for z in [0., 3.] {
                let p: [f64; 3] = std::array::from_fn(|i| [x, y, z][i] - eye[i]);
                let depth = -(0..3).map(|i| back[i] * p[i]).sum::<f64>();
                let sx = 200. + f * (0..3).map(|i| right[i] * p[i]).sum::<f64>() / depth;
                let sy = 120. - f * (0..3).map(|i| up[i] * p[i]).sum::<f64>() / depth;
                bounds[0] = bounds[0].min(sx);
                bounds[1] = bounds[1].min(sy);
                bounds[2] = bounds[2].max(sx);
                bounds[3] = bounds[3].max(sy);
            }
        }
    }
    (
        bounds[0].floor().max(0.) as usize,
        bounds[1].floor().max(0.) as usize,
        bounds[2].ceil().min(400.) as usize,
        bounds[3].ceil().min(240.) as usize,
    )
}

pub(super) fn actual_hud_cells(memory: &[u16], row: usize) -> Vec<u8> {
    (0..50)
        .map(|x| {
            let i = row * 50 + x;
            (memory[0xfbf0 * 64 + i / 2] >> ((i & 1) * 8)) as u8
        })
        .collect()
}

pub(super) fn actual_hud_number(cells: &[u8]) -> u16 {
    let digits = cells
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect::<Vec<_>>();
    std::str::from_utf8(&digits).unwrap().parse().unwrap()
}

pub(super) fn expanded565(c: u16) -> [u32; 3] {
    let r = u32::from(c >> 11);
    let g = u32::from((c >> 5) & 63);
    let b = u32::from(c & 31);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

pub(super) fn submitted_shadow_triangles(
    memory: &[u16],
    catalog: &serde_json::Value,
    command_words: u16,
) -> Vec<[[f64; 2]; 3]> {
    let sector = catalog["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "shadow-triangle")
        .unwrap()["record_sector"]
        .as_u64()
        .unwrap() as usize;
    let camera = &catalog["cameras"][0];
    let eye: [f64; 3] = std::array::from_fn(|i| camera["eye"][i].as_f64().unwrap());
    let target: [f64; 3] = std::array::from_fn(|i| camera["target"][i].as_f64().unwrap());
    let mut back: [f64; 3] = std::array::from_fn(|i| eye[i] - target[i]);
    let length = back.iter().map(|v| v * v).sum::<f64>().sqrt();
    for v in &mut back {
        *v /= length;
    }
    let mut right = [back[2], 0., -back[0]];
    let length = right.iter().map(|v| v * v).sum::<f64>().sqrt();
    for v in &mut right {
        *v /= length;
    }
    let up = [
        back[1] * right[2] - back[2] * right[1],
        back[2] * right[0] - back[0] * right[2],
        back[0] * right[1] - back[1] * right[0],
    ];
    let k = camera["focal"].as_f64().unwrap() / 256.;
    let mut local = vec![0u16; 2048];
    let mut records = [0usize; 2];
    let mut result = vec![];
    let commands = &memory[0x9880 * 64..0x9880 * 64 + usize::from(command_words)];
    for words in commands.as_chunks::<2>().0 {
        let c = u32::from(words[0]) | (u32::from(words[1]) << 16);
        match c >> 28 {
            1 => {
                let src = ((c >> 10) & 0x3ffff) as usize * 16;
                let dst = ((c >> 3) & 127) as usize * 16;
                let n = 16 << ((c >> 1) & 3);
                local[dst..dst + n].copy_from_slice(&memory[src..src + n]);
            }
            2 => records[(c & 1) as usize] = ((c >> 12) & 65535) as usize,
            3 if records[(c & 1) as usize] == sector => {
                let uniform = usize::from(memory[sector * 64 + 1] & 63);
                let at = uniform * 64;
                assert_eq!(&local[at + 44..at + 46], &[0x0d0a, 0x5712]);
                let p: [[f64; 3]; 3] = std::array::from_fn(|vertex| {
                    std::array::from_fn(|r| {
                        let translation = scene::fixed(&local, at + r * 8 + 6);
                        if vertex == 0 {
                            translation
                        } else {
                            translation
                                + scene::fixed(&local, at + r * 8 + if vertex == 1 { 0 } else { 4 })
                        }
                    })
                });
                result.push(std::array::from_fn(|v| {
                    let w: [f64; 3] = std::array::from_fn(|r| {
                        eye[r] + right[r] * p[v][0] / k - up[r] * p[v][1] / k - back[r] * p[v][2]
                    });
                    assert!(
                        (w[1] - 393. / 65536.).abs() < 0.004,
                        "shadow plane y={}",
                        w[1]
                    );
                    [w[0], w[2]]
                }));
            }
            _ => {}
        }
    }
    result
}
