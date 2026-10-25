//! Bounded execution of the actual RCC scene preparation on CPU V3.
use super::{compile, CpuV3Sim, RunOutcome};
use crate::geometry::{compose, transform, Matrix};

const PROGRAM: &str = r#"
mod gpu_transform;
mod bloxorz_scene;
static POSE:Buf<u16,18>=Buf::new([0;18]);
static CAMERA:Buf<u16,18>=Buf::new([0;18]);
static VIEW:Buf<u16,24>=Buf::new([0;24]);
static INSTANCE:Buf<u16,42>=Buf::new([0;42]);
fn main(){
 let input=unsafe{Ptr::from_addr(0x5000).as_u16_array()};
 let mut output=unsafe{Ptr::from_addr(0x6000).as_u16_array()};
 if !blox_camera(input[0u16],input[1u16],CAMERA.as_array(),VIEW.as_array()) {halt(1);}
 let mut state:Buf<u16,10>=Buf::new([0;10]);state[0u16]=input[8u16];state[1u16]=input[9u16];state[2u16]=input[2u16];
 if !blox_roll_pose(POSE.as_array(),input,state.as_array(),input[3u16],fix32::from_words(input[4u16],input[5u16]),fix32::from_words(input[6u16],input[7u16])) {halt(2);}
 if !gpu_affine_trs(output,POSE.as_array()) {halt(3);}
 if !blox_instance(INSTANCE.as_array(),POSE.as_array(),CAMERA.as_array()) {halt(4);}
 let mut i:u16=0;while i<24 {output[24+i]=VIEW[i];i+=1;}
 i=0;while i<42 {output[48+i]=INSTANCE[i];i+=1;}
 let mut bounds:Buf<u16,12>=Buf::new([0;12]);
 let half=fix32::from_words(64256,0);
 gpu_bounds_set(bounds.as_array(),vec3::new(-half,-half,-half),vec3::new(half,half,half));
 output[90u16]=if gpu_instance_bounds(INSTANCE.as_array(),bounds.as_array()) {1}else{0};
 halt(0x600d);
}
"#;

fn pair(value: f64) -> [u16; 2] {
    let bits = (value * 65536.0).round() as i32;
    [bits as u16, (bits >> 16) as u16]
}

fn run(code: &[u16], input: &[u16]) -> (Vec<f64>, Vec<i32>, u16, usize) {
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(code);
    cpu.physical_memory_mut()[0x5000..0x5000 + input.len()].copy_from_slice(input);
    let result = cpu.run(2_000_000).unwrap();
    let RunOutcome::Halted {
        signal: 0x600d,
        steps,
    } = result
    else {
        panic!("{input:?}: {result:?}");
    };
    let raw: Vec<i32> = cpu.physical_memory_mut()[0x6000..0x605a]
        .chunks_exact(2)
        .map(|w| (u32::from(w[0]) | (u32::from(w[1]) << 16)) as i32)
        .collect();
    (
        raw.iter().map(|v| f64::from(*v) / 65536.0).collect(),
        raw,
        cpu.physical_memory_mut()[0x605a],
        steps,
    )
}

#[test]
fn rcc_fixed_camera_and_roll_match_original_geometry() {
    let code = compile(PROGRAM);
    let mut cameras = Vec::new();
    let mut poses = Vec::new();
    for line in include_str!("assets/geometry-reference.txt").lines() {
        let mut words = line.split_whitespace();
        match words.next().unwrap() {
            "camera" => {
                let v: Vec<f64> = words.map(|s| s.parse().unwrap()).collect();
                let m: Matrix = std::array::from_fn(|r| std::array::from_fn(|c| v[3 + r * 4 + c]));
                cameras.push((v[0] as u16, v[1] as u16, m));
            }
            "pose" => {
                let v: Vec<f64> = words.map(|s| s.parse().unwrap()).collect();
                let m: Matrix = std::array::from_fn(|r| std::array::from_fn(|c| v[4 + r * 4 + c]));
                poses.push((v[0] as u16, v[1] as u16, v[2], v[3], m));
            }
            _ => {}
        }
    }
    let mut max_matrix_error = 0.0_f64;
    let mut max_pixel_error = 0.0_f64;
    let mut max_normal_error = 0.0_f64;
    let mut max_normal_coefficient = 0.0_f64;
    let mut max_shifted = 0_i64;
    let mut max_steps = 0;
    let mut count = 0;
    let lens = 120.0 / (std::f64::consts::PI / 18.0).tan() / 256.0;
    for (width, depth, camera) in cameras {
        for &(orient, dir, fraction, drop, reference) in &poses {
            for [x, z] in [
                [0i16, 0],
                [(width - 1) as i16, (depth - 1) as i16],
                [-2, (depth + 1) as i16],
            ] {
                let mut input = vec![width, depth, orient, dir];
                input.extend(pair(fraction));
                input.extend(pair(drop));
                input.extend([x as u16, z as u16]);
                let (out, raw, bounded, steps) = run(&code, &input);
                assert_eq!(bounded, 1, "bounded F16 source positions");
                max_steps = max_steps.max(steps);
                let mut world = reference;
                world[0][3] += f64::from(x) - f64::from(width) / 2.0;
                world[2][3] += f64::from(z) - f64::from(depth) / 2.0;
                for row in &mut world {
                    for coefficient in &mut row[..3] {
                        *coefficient *= 0.5;
                    }
                }
                let expected = compose(camera, world);
                for (a, b) in out[24..36].iter().zip(expected.into_iter().flatten()) {
                    max_matrix_error = max_matrix_error.max((a - b).abs());
                }
                for r in 0..3 {
                    for c in 0..4 {
                        let rigid = if r < 2 {
                            camera[r][c] / lens
                        } else {
                            camera[r][c]
                        };
                        max_matrix_error =
                            max_matrix_error.max((out[12 + r * 4 + c] - rigid).abs());
                        max_matrix_error =
                            max_matrix_error.max((out[r * 4 + c] - world[r][c]).abs());
                    }
                }
                let size = match orient {
                    0 => [1.0, 2.0, 1.0],
                    1 => [2.0, 1.0, 1.0],
                    _ => [1.0, 1.0, 2.0],
                };
                for r in 0..3 {
                    for c in 0..3 {
                        let n = (0..3)
                            .map(|k| {
                                camera[r][k] / if r < 2 { lens } else { 1.0 }
                                    * reference[k][c]
                                    * 2.0
                                    / (size[c] * size[c])
                            })
                            .sum::<f64>();
                        max_normal_error = max_normal_error.max((out[36 + r * 3 + c] - n).abs());
                        max_normal_coefficient =
                            max_normal_coefficient.max(out[36 + r * 3 + c].abs());
                        // Box normals are axis-aligned source codes; both extreme
                        // signed normal10 values stay within the native normal18.
                        for n in [-512_i64, 511] {
                            let transformed = (n * 128 * i64::from(raw[36 + r * 3 + c])) >> 16;
                            assert!((-131072..=131071).contains(&transformed));
                        }
                    }
                }
                // Evaluate the native MAC/F16 boundaries using CPU-produced
                // Q16.16 matrices and doubled local positions, without rendering.
                for corner in 0..8 {
                    let p: [f64; 3] =
                        std::array::from_fn(|i| if corner & (1 << i) == 0 { -0.98 } else { 0.98 });
                    let ideal = transform(expected, p);
                    let mut actual = [0.0; 3];
                    for r in 0..3 {
                        let sum = (0..3)
                            // Native candidate: origin=-64256, quant_shift=7,
                            // delta10=0/1004 gives symmetric +/-0.98046875.
                            .map(|c| {
                                i64::from(raw[24 + r * 4 + c])
                                    * if p[c] < 0.0 { -64256 } else { 64256 }
                            })
                            .sum::<i64>()
                            + 65536 * i64::from(raw[24 + r * 4 + 3]);
                        let shifted = sum >> 16;
                        assert!((-(1 << 25)..(1 << 25)).contains(&shifted));
                        if r == 2 {
                            assert!(shifted > 0);
                        }
                        max_shifted = max_shifted.max(shifted.abs());
                        actual[r] = shifted as f64;
                    }
                    for i in 0..2 {
                        max_pixel_error = max_pixel_error.max(
                            (256.0 * actual[i] / actual[2] - 256.0 * ideal[i] / ideal[2]).abs(),
                        );
                    }
                }
                count += 1;
            }
        }
    }
    println!("RCC geometry: {count} cases, max matrix error {max_matrix_error}, max normal error {max_normal_error}, max pixel error after MAC/F16 {max_pixel_error}, max normal coefficient {max_normal_coefficient}, max F16 coordinate {max_shifted}, max emulator instructions {max_steps}");
    assert_eq!(count, 1620);
    assert!(max_matrix_error < 0.004);
    assert!(max_normal_error < 0.0002);
    assert!(max_pixel_error < 0.125);
    assert!(max_normal_coefficient < 2.0);
}

#[test]
fn rcc_scene_invalid_inputs_preserve_destinations() {
    let source = r#"
mod gpu_transform;
mod bloxorz_scene;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn unchanged(words:Array<u16>,count:u16){let mut i:u16=0;while i<count{check(words[i]==23130);i+=1;}}
fn main(){
 let mut pose:Buf<u16,18>=Buf::new([23130;18]);
 let mut camera:Buf<u16,18>=Buf::new([23130;18]);
 let mut view:Buf<u16,24>=Buf::new([23130;24]);
 let mut out:Buf<u16,42>=Buf::new([23130;42]);
 let mut level:Buf<u16,2>=Buf::new([15,10]);
 let mut state:Buf<u16,10>=Buf::new([0;10]);
 let zero=fix32::zero();let one=fix32::from_int(1);
 check(!blox_camera(0,10,camera.as_array(),view.as_array()));
 check(!blox_camera(16,10,camera.as_array(),view.as_array()));
 check(!blox_camera(15,11,camera.as_array(),view.as_array()));
 unchanged(camera.as_array(),18);unchanged(view.as_array(),24);
 check(!blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),4,zero,zero));
 check(!blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),0,-one,zero));
 check(!blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),0,fix32::from_int(2),zero));
 check(!blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),0,zero,one));
 state[0u16]=65533;
 check(!blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),0,zero,zero));
 state[0u16]=17;
 check(!blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),0,zero,zero));
 state[0u16]=0;state[2u16]=3;
 check(!blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),0,zero,zero));
 unchanged(pose.as_array(),18);
 state[2u16]=0;
 state[9u16]=1;
 check(!blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),0,zero,zero));
 unchanged(pose.as_array(),18);
 state[9u16]=0;
 check(blox_camera(15,10,camera.as_array(),view.as_array()));
 check(blox_roll_pose(pose.as_array(),level.as_array(),state.as_array(),0,zero,zero));
 gpu_fstore(pose.as_array(),6,zero);
 check(!blox_instance(out.as_array(),pose.as_array(),camera.as_array()));
 unchanged(out.as_array(),42);
 halt(0x600d);
}
"#;
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&compile(source));
    let result = cpu.run(2_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
}
