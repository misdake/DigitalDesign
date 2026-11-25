//! Public G2 checks, compiled with a fixed actual System loader by the script.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use std::{any::Any, path::Path};

struct NoSend;
impl Device for NoSend {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        assert_eq!(channel, 2);
        0
    }
    fn write(&mut self, _: &mut [u16], _: u8, _: u16) {
        panic!("recording issued SEND");
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn address(debug: &rcc::DebugInfo, name: &str) -> usize {
    match debug.globals.iter().find(|g| g.name == name).unwrap().loc {
        rcc::VarLoc::Global(address) => usize::from(address),
        _ => unreachable!(),
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stream {
    commands: Vec<u16>,
    push: Vec<u16>,
    facts: Vec<u16>,
}
fn streams(cpu: &mut CpuV3Sim, debug: &rcc::DebugInfo) -> Vec<Stream> {
    let record = address(debug, "GPU_CB_RECORDS");
    let facts = address(debug, "GPU_CB_RESOURCE_FACTS");
    let record_words = debug
        .consts
        .iter()
        .find_map(|(name, _, value)| (name == "GPU_API_CB_WORDS").then_some(usize::from(*value)))
        .expect("compiled command-record stride");
    let memory = cpu.physical_memory_mut();
    (0..2)
        .map(|slot| {
            let c = &memory[record + slot * record_words..record + slot * record_words + 32];
            let cb = usize::from(c[1]) * 64;
            let push = usize::from(c[3]) * 64;
            Stream {
                commands: memory[cb..cb + usize::from(c[5])].to_vec(),
                push: memory[push..push + usize::from(c[6])].to_vec(),
                facts: memory[facts + slot * 256..facts + slot * 256 + usize::from(c[15]) * 4]
                    .to_vec(),
            }
        })
        .collect()
}
fn draws(stream: &Stream, push_sector: u32) -> Vec<Vec<u16>> {
    let mut global = vec![];
    let mut render = vec![];
    let mut instance = vec![];
    let mut result = vec![];
    let commands = stream
        .commands
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16))
        .collect::<Vec<_>>();
    let mut drained = false;
    for (index, &code) in commands.iter().enumerate() {
        if code == 0x4000_0003 {
            drained = true;
        }
        if code >> 28 == 1 || code >> 28 == 2 {
            assert_eq!(
                commands.get(index + 1),
                Some(&0x4000_0001),
                "DMA completion WAIT missing"
            );
        }
        if code >> 28 == 1 {
            let local = (code >> 3) & 127;
            if local == 66 || local == 68 {
                assert!(
                    drained,
                    "Global/Render staging overwritten without full WAIT"
                );
            }
            let word = (((code >> 10) & 0x3ffff) * 16 - push_sector * 64) as usize;
            match local {
                66 => global = stream.push[word..word + 32].to_vec(),
                68 => render = stream.push[word..word + 16].to_vec(),
                80 => instance = stream.push[word..word + 64].to_vec(),
                _ => {}
            }
        }
        if code >> 28 == 3 {
            assert_eq!((global.len(), render.len(), instance.len()), (32, 16, 64));
            result.push(
                global
                    .iter()
                    .chain(&render)
                    .chain(&instance)
                    .copied()
                    .collect(),
            );
            drained = false;
        }
        if code >> 28 == 11 {
            drained = false;
        }
    }
    result
}
type Matrix = [[f64; 3]; 3];
fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}
fn rotation([x, y, z]: [f64; 3]) -> Matrix {
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    product(
        product(
            [[cz, -sz, 0.], [sz, cz, 0.], [0., 0., 1.]],
            [[cy, 0., sy], [0., 1., 0.], [-sy, 0., cy]],
        ),
        [[1., 0., 0.], [0., cx, -sx], [0., sx, cx]],
    )
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}
fn unit(v: [f64; 3]) -> [f64; 3] {
    let len = dot(v, v).sqrt();
    v.map(|x| x / len)
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn near(words: &[u16], at: usize, expected: f64, tolerance: i64) -> i64 {
    let actual = (u32::from(words[at]) | (u32::from(words[at + 1]) << 16)) as i32;
    let error = (i64::from(actual) - (expected * 65536.).round() as i64).abs();
    assert!(
        error <= tolerance,
        "coefficient {at}: {actual}, expected {expected}, error {error}"
    );
    error
}
fn native_oracle(native: &[u16], view: Matrix) -> i64 {
    assert_eq!(native.len(), 112);
    let world = rotation([0.125, -0.375, 0.625]);
    let scale = [2., -4., 0.5];
    let mut error = 0;
    for (row, basis) in view.iter().enumerate() {
        let position_factor = if row == 2 { -1. } else { -1.5 };
        for (col, &axis_scale) in scale.iter().enumerate() {
            error = error.max(near(
                native,
                48 + row * 8 + col * 2,
                dot(*basis, std::array::from_fn(|k| world[k][col] * axis_scale)) * position_factor,
                128,
            ));
            error = error.max(near(
                native,
                72 + (row * 3 + col) * 2,
                dot(*basis, std::array::from_fn(|k| world[k][col] / axis_scale))
                    * if row == 0 { -1. } else { 1. },
                64,
            ));
        }
        error = error.max(near(
            native,
            48 + row * 8 + 6,
            dot(*basis, [7., -2., -17.]) * position_factor,
            128,
        ));
        let light = (dot(*basis, unit([1., 2., -3.])) * if row == 0 { -16384. } else { 16384. })
            .round() as i64;
        assert!((i64::from(native[2 + row] as i16) - light).abs() <= 4);
    }
    let squared: i64 = native[2..5]
        .iter()
        .map(|&v| i64::from(v as i16).pow(2))
        .sum();
    assert!(((1 << 28) - 32768..=(1 << 28) + 32768).contains(&squared));
    let mut global = [0u16; 32];
    global[1] = 128;
    global[2..5].copy_from_slice(&native[2..5]);
    global[5] = (-4267i16) as u16;
    global[6] = (-2560i16) as u16;
    global[7] = 8192;
    global[8] = 96;
    global[9] = 32;
    global[10] = 35;
    assert_eq!(&native[..32], &global);
    let mut render = [0u16; 16];
    render[2] = 0x6655;
    render[3] = 0x77;
    render[4] = 27;
    assert_eq!(&native[32..48], &render);
    assert_eq!(&native[90..94], &[0, 1, 0x2211, 0x4433]);
    assert!(native[94..].iter().all(|&w| w == 0));
    error
}
pub fn run(
    mut compile: impl FnMut(
        &str,
        &str,
        &rcc_backend::CompilerOptions,
        &Path,
    ) -> Result<rcc::frontend::Program, String>,
    root: &Path,
) {
    let source = include_str!("../../fixtures/graphics_api_b_public.rs");
    let mut previous = None;
    let mut reports = vec![];
    for enabled in [false, true] {
        let profile = if enabled { "debug" } else { "release" };
        let folder = root.join(profile);
        std::fs::create_dir_all(&folder).unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let ir = compile("public-g2", source, &options, &folder).unwrap();
        let static_bytes = ir.debug.static_data_end * 2;
        let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
        let phase = address(&program.debug, "G2_PHASE");
        let control = address(&program.debug, "GPU_OBJECT_CONTROL");
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(4, Box::new(NoSend));
        cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
        cpu.physical_memory_mut()[0xb0ff * 64..0xb0ff * 64 + 384].fill(0xa55a);
        let mut stages = vec![];
        let mut arena = vec![];
        let mut min_sp = 0xc000;
        let mut halted = false;
        let mut steps = 0;
        for _ in 0..5_000_000 {
            steps += 1;
            let outcome = cpu.step().unwrap();
            let sp = cpu.registers()[13];
            if sp != 0 {
                min_sp = min_sp.min(sp);
            }
            if usize::from(cpu.physical_memory_mut()[phase]) > stages.len() {
                stages.push(streams(&mut cpu, &program.debug));
                arena.push(cpu.physical_memory_mut()[control..control + 6].to_vec());
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted);
        assert_eq!(stages.len(), 4);
        assert_eq!(
            stages[0], stages[1],
            "setters/Drop/reuse changed old recording"
        );
        assert_eq!(
            stages[2], stages[3],
            "resource Drop changed recorded inputs"
        );
        assert_eq!(
            stages[0][1], stages[2][1],
            "interleaved first CB changed second CB"
        );
        assert!(stages[2][0].commands.starts_with(&stages[0][0].commands));
        assert!(stages[2][0].push.starts_with(&stages[0][0].push));
        assert_eq!(
            arena[0][3], arena[1][3],
            "replacement allocated a new large slot instead of reusing freed slot"
        );
        assert_eq!(arena[0][5], arena[1][5]);
        assert!(arena[0][3] >= 8, "six Models plus two Cameras must coexist");
        let a = draws(&stages[0][0], 0x9a00);
        let b = draws(&stages[0][1], 0xfe00);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0], a[1], "equal-value Transform changed native binding");
        assert_eq!(b.len(), 1);
        let camera = rotation([-0.375, 0.625, -0.25]);
        let euler = std::array::from_fn(|r| std::array::from_fn(|c| camera[c][r]));
        let backward = unit([4., -2.5, 8.]);
        let right = unit(cross(unit([0.5, 1., 0.25]), backward));
        let error = native_oracle(&a[0], euler).max(native_oracle(
            &b[0],
            [right, cross(backward, right), backward],
        ));
        let final_draws = draws(&stages[2][0], 0x9a00);
        assert_eq!(final_draws.len(), 6);
        assert_eq!(&final_draws[2][92..94], &[0x0201, 0x0403]);
        assert_eq!(&final_draws[3][92..94], &[0x0201, 0x0403]);
        assert_eq!(final_draws[2][32], 0xa800);
        assert_eq!(final_draws[2][33], 54);
        assert_eq!(&final_draws[3][32..34], &[0x8800, 169]);
        assert_eq!(&final_draws[4][32..34], &[0, 0]);
        assert_eq!(
            &final_draws[4][..92],
            &final_draws[5][..92],
            "RGBA-only change affected other native state"
        );
        assert_eq!(&final_draws[5][92..94], &[0x0201, 0x0803]);
        let commands = &stages[2][0].commands;
        let draw_indices = commands
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, w)| w[1] >> 12 == 3)
            .map(|(i, _)| i * 2)
            .collect::<Vec<_>>();
        assert!(
            commands[draw_indices[4] + 2..draw_indices[5]]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|w| w[1] >> 12 != 5 && w[1] >> 12 != 6 && *w != [3, 0x4000]),
            "RGBA-only change caused SET or full WAIT"
        );
        for stream in &stages[0] {
            let globals = stream
                .commands
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|w| w[1] >> 12 == 5)
                .count();
            let renders = stream
                .commands
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|w| w[1] >> 12 == 6)
                .count();
            assert_eq!(
                (globals, renders),
                (1, 1),
                "equal-value reuse emitted redundant SET"
            );
        }
        let memory = cpu.physical_memory_mut();
        let grid = &memory[0xb0ff * 64..0xb0ff * 64 + 384];
        let mut expected_grid = vec![0xffff; 384];
        expected_grid[375..].fill(0xa55a);
        expected_grid[25] = 0x00ff;
        expected_grid[26] = 0xff01;
        expected_grid[62] = 0x00ff;
        expected_grid[63] = 0x0201;
        assert_eq!(
            grid, expected_grid,
            "full public UI allocation, neighbors and tail"
        );
        let stack_bytes = usize::from(0xc000 - min_sp) * 2;
        assert!(static_bytes + stack_bytes <= 16384);
        if let Some(previous) = &previous {
            assert_eq!(&stages, previous, "healthy profile output differs");
        }
        previous = Some(stages.clone());
        let data = serde_json::json!({"profile":profile,"static_bytes":static_bytes,"observed_stack_bytes":stack_bytes,"max_frame_words":program.debug.functions.iter().map(|f|f.frame_size).max().unwrap_or(0),"steps":steps,"maximum_steps":5000000,"max_q16_error":error,"stages":stages.iter().map(|s|s.iter().map(|v|serde_json::json!({"commands":v.commands,"push":v.push,"facts":v.facts})).collect::<Vec<_>>()).collect::<Vec<_>>(),"qualification":"Actual fixed A public root plus B/C recording; source identities are in the input manifest. No capture/dump stack, final caller composition, GPU rendering or G3 qualification."});
        std::fs::write(
            folder.join("qualification.json"),
            serde_json::to_vec_pretty(&data).unwrap(),
        )
        .unwrap();
        reports.push(data);
        for (label,body,expected) in [
            ("projection-private","let p=Projection::from_focal_pixels(fix32::from_int(384),fix32::from_int(128));halt(p.far.to_int() as u16);","E0616"),
            ("model-use-after-drop","let m=Transform::new(Pose{position:Vec3{x:fix32::zero(),y:fix32::zero(),z:fix32::zero()},angles:Vec3{x:fix32::zero(),y:fix32::zero(),z:fix32::zero()},scale:Vec3{x:fix32::from_int(1),y:fix32::from_int(1),z:fix32::from_int(1)}});drop(m);let w=m.world();","moved"),
            ("helper-private","let s=Shininess::nearest(18);halt(gpu_scene_shininess_exponent(s));","private"),
        ] {
            let input=format!("mod gpu_api;fn main(){{{body}}}");let output=folder.join(label);
            let error=compile(label,&input,&options,&output).err().expect("invalid public boundary accepted");assert!(error.contains(expected),"{label}: {error}");
            std::fs::write(output.join("diagnostic.txt"),error).unwrap();
        }
    }
    std::fs::write(
        root.join("qualification.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    println!("Public G2 recording/numerics/lifetimes and six boundary negatives passed");
}
