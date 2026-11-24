//! Independent signed-axis group and actual Model source-matrix qualification.
use super::scenarios;
use super::*;
use cpu_v3::{CpuV3Sim, Device};
use std::any::Any;

struct Idle;
impl Device for Idle {
    fn read(&mut self, _: &mut [u16], _: u8) -> u16 {
        0
    }
    fn write(&mut self, _: &mut [u16], _: u8, _: u16) {
        panic!("matrix test must not submit");
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn all_cube_orientations_preserve_world_and_normal_columns() {
    let module = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/game_bloxorz/rcc/orientation.rs"),
    )
    .unwrap();
    let body = &module[module.find("pub const ROTATION_IDENTITY").unwrap()..];
    let caller = r#"mod gpu_api;use gpu_api::*;
fn v(x:i16,y:i16,z:i16)->Vec3{Vec3{x:fix32::from_int(x),y:fix32::from_int(y),z:fix32::from_int(z)}}
fn save_model(model:&Model,at:u16){
 let mut world=model.world();let mut normal=model.normal();
 let w=unsafe{view_of(&mut world).as_ptr().as_u16_array()};let n=unsafe{view_of(&mut normal).as_ptr().as_u16_array()};
 let out=Ptr::from_addr(at);let mut i:u16=0;while i<24 {unsafe{out.write(i as i16,w[i]);}i+=1;}
 i=0;while i<18 {unsafe{out.write((24+i) as i16,n[i]);}i+=1;}
}

fn main(){
 let output=Ptr::from_addr(0x4000);let mut q:u16=0;
 while q<24 {let mut c:u16=0;while c<3 {unsafe{output.write((q*3+c)as i16,rotation_axis(q,c));}c+=1;}
 let mut d:u16=0;while d<4 {unsafe{output.write((72+q*4+d)as i16,rotation_quarter(q,d));}d+=1;}q+=1;}
 let memory:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;
 let gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:memory,command_b:absent});
 let pose:Transform=Transform{position:v(7,9,-2),angles:Vec3{x:fix32::zero(),y:fix32::zero(),z:fix32::from_words(32768,0)},scale:v(1,2,3)};
 let mut model=Model::new(pose);save_model(&model,0x4200);q=0;
 while q<24 {rotation_pose(&mut model,pose,q);save_model(&model,0x4300+q*42);q+=1;}
 drop(model);drop(gpu);halt(0x600d);
}"#;
    for debug in [false, true] {
        let source = format!("{caller}\n{body}");
        let ir = crate::gpu_api_resources::parse(&source, "cube-orientation", debug).unwrap();
        let options = rcc_backend::CompilerOptions {
            gpu_debug: debug,
            ..rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0)
        };
        let code = rcc_backend::try_compile(ir, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        cpu.attach_device(4, Box::new(Idle));
        let mut halted = false;
        for _ in 0..2_000_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "orientation2M step bound");
        let memory = cpu.physical_memory_slice();
        let axis = |q: usize, c: usize| memory[0x4000 + q * 3 + c];
        let mut unique = std::collections::BTreeSet::new();
        let vector = |a: u16| {
            let mut v = [0i32; 3];
            v[usize::from(a / 2)] = if a & 1 == 0 { 1 } else { -1 };
            v
        };
        for q in 0..24 {
            let basis = [axis(q, 0), axis(q, 1), axis(q, 2)];
            assert!(basis.iter().all(|a| *a < 6));
            assert!(unique.insert(basis));
            let [x, y, z] = basis.map(vector);
            assert_eq!(
                [
                    x[1] * y[2] - x[2] * y[1],
                    x[2] * y[0] - x[0] * y[2],
                    x[0] * y[1] - x[1] * y[0]
                ],
                z
            );
            for d in 0..4 {
                let next = usize::from(memory[0x4000 + 72 + q * 4 + d]);
                assert!(next < 24);
                for c in 0..3 {
                    let [x, y, z] = vector(axis(q, c));
                    let expected = match d {
                        0 => [y, -x, z],
                        1 => [-y, x, z],
                        2 => [x, -z, y],
                        _ => [x, z, -y],
                    };
                    assert_eq!(vector(axis(next, c)), expected, "q{q} d{d} col{c}");
                }
            }
            let baseline = &memory[0x4200..0x4200 + 42];
            let actual = &memory[0x4300 + q * 42..0x4300 + (q + 1) * 42];
            let value = |words: &[u16], at: usize| {
                i32::from(words[at]) | ((i32::from(words[at + 1])) << 16)
            };
            for (start, stride, rows) in [(0, 8, 3), (24, 6, 3)] {
                for row in 0..rows {
                    for c in 0..3 {
                        let a = axis(q, c);
                        let base = value(baseline, start + row * stride + usize::from(a / 2) * 2);
                        assert_eq!(
                            value(actual, start + row * stride + c * 2),
                            if a & 1 == 0 { base } else { -base },
                            "world/normal q{q}"
                        );
                    }
                    if stride == 8 {
                        assert_eq!(
                            &actual[row * 8 + 6..row * 8 + 8],
                            &baseline[row * 8 + 6..row * 8 + 8]
                        );
                    }
                }
            }
        }
        assert_eq!([axis(8, 0), axis(8, 1), axis(8, 2)], [0, 2, 4]);
    }
}

#[test]
fn roll_completion_inverse_and_undo_keep_the_same_local_faces() {
    let body = r#"
let memory:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
let absent:Option<CommandMemory>=None;let gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:memory,command_b:absent});
let mut model=Model::new(Transform::new(Vec3::zero(),Vec3::zero(),Vec3::new(fix32::from_int(1),fix32::from_int(1),fix32::from_int(1))));
GAME_SESSION.begin();GAME_SESSION.level.width=15;GAME_SESSION.level.height=10;GAME_SESSION.level.cell_count=150;GAME_SESSION.level.schema=2;
let mut i:u16=0;while i<150 {GAME_SESSION.level_words()[8+i*2]=1;GAME_SESSION.level_words()[9+i*2]=0;i+=1;}
GAME_SESSION.rules.x=5;GAME_SESSION.rules.z=4;GAME_SESSION.rules.orientation=0;GAME_SESSION.rules.bridge_mask=0;GAME_SESSION.rules.status=0;GAME_SESSION.rules.moves=0;GAME_SESSION.rules.split=0;GAME_SESSION.animation.phase=AnimationPhase::Play;
GAME_SESSION.block_pose(&mut model);world_probe(&model);check(GAME_SESSION.block_orientation()==8);
check(GAME_SESSION.try_move(0));GAME_SESSION.animation.elapsed=220;GAME_SESSION.motion.elapsed=220;
GAME_SESSION.block_pose(&mut model);world_probe(&model);
GAME_SESSION.tick(0,0,0);check(GAME_SESSION.animation.phase==AnimationPhase::Play);check(GAME_SESSION.block_orientation()!=8);
GAME_SESSION.block_pose(&mut model);world_probe(&model);
check(GAME_SESSION.try_move(1));GAME_SESSION.animation.elapsed=220;GAME_SESSION.motion.elapsed=220;
GAME_SESSION.block_pose(&mut model);world_probe(&model);GAME_SESSION.tick(0,0,0);check(GAME_SESSION.block_orientation()==8);
GAME_SESSION.block_pose(&mut model);world_probe(&model);
GAME_SESSION.tick(32768,0,0);check(GAME_SESSION.rules.moves==1);check(GAME_SESSION.block_orientation()!=8);
GAME_SESSION.block_pose(&mut model);world_probe(&model);
GAME_SESSION.reset_level(0);check(GAME_SESSION.block_orientation()==8);GAME_SESSION.end();drop(model);drop(gpu);
"#;
    let (program, resources, out) = application("orientation-roll-continuity", Some(body));
    let mut sim = cpu_v3_tang_nano_20k::system_sim::CpuV3SystemSim::new(
        cpu_v3_tang_nano_20k::system_sim::VBlankMode::PauseOnFrameIndexWait,
    )
    .with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut samples = vec![];
    let mut halted = false;
    for _ in 0..4_000_000 {
        match sim.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 13 => {
                let at = usize::from(e.value);
                let m = sim.cpu().physical_memory_slice();
                samples.push(
                    (0..12)
                        .map(|i| i32::from(m[at + i * 2]) | (i32::from(m[at + i * 2 + 1]) << 16))
                        .collect::<Vec<_>>(),
                );
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "roll continuity4M step bound");
    assert_eq!(samples.len(), 6);
    for (a, b) in [(1, 2), (3, 4)] {
        assert!(
            samples[a]
                .iter()
                .zip(&samples[b])
                .all(|(x, y)| (i64::from(*x) - i64::from(*y)).abs() <= 16),
            "animated endpoint vs resting world matrix: {:?} {:?}",
            samples[a],
            samples[b]
        );
    }
    assert_eq!(
        samples[0], samples[4],
        "inverse move restores position and material frame"
    );
    assert_eq!(
        samples[2], samples[5],
        "undo restores exact object orientation"
    );
    std::fs::write(out.join("rotation-continuity.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "world_q16_samples":samples,"endpoint_tolerance_q16":16,"max_steps":4000000,
        "scope":"same local vertex labels across the animated endpoint and rest; inverse and undo exact"})).unwrap()).unwrap();
}

#[test]
fn rendered_roll_endpoints_and_transparent_bridge_occlusion() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let mut results = vec![];
    for (name, case) in [
        ("web-stage-orientation-continuity", scenarios::orientation()),
        ("web-stage-hint-order-late", scenarios::hint_order(false)),
        ("web-stage-hint-order-early", scenarios::hint_order(true)),
        ("web-goal-close", scenarios::goal_close()),
    ] {
        let (program, resources, out) = application_case(name, None, None, case);
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut frames = vec![];
        let mut depths = vec![];
        let mut halted = false;
        for _ in 0..12_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            match sim.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 12 => {
                    let rgb = sim.render_active_framebuffer();
                    write_display_ppm(&out.join(format!("frame-{}.ppm", frames.len())), &rgb);
                    depths.push(
                        sim.cpu().physical_memory_slice()[0x480000 / 2..0x480000 / 2 + 102400]
                            .to_vec(),
                    );
                    frames.push(rgb);
                }
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d, "{name}");
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(halted, "{name}12M step bound");
        assert_eq!(frames.len(), if name == "web-goal-close" { 5 } else { 4 });
        assert!(sim.gpu().last_error().is_none());
        if name.contains("continuity") {
            for (a, b) in [(0, 1), (2, 3)] {
                // CORDIC's animated quarter turn and exact settled permutation
                // may differ by one texture/filter quantization step. Bound the
                // actual channel error, rather than counting every changed LSB.
                let deltas: Vec<_> = frames[a]
                    .iter()
                    .zip(&frames[b])
                    .map(|(x, y)| {
                        [0, 8, 16].map(|shift| {
                            (((x >> shift) & 255) as i32 - ((y >> shift) & 255) as i32)
                                .unsigned_abs()
                        })
                    })
                    .collect();
                let large = deltas.iter().filter(|v| v.iter().any(|d| *d > 32)).count();
                let total: u32 = deltas.iter().flatten().sum();
                assert!(large<=4 && total<4000,"endpoint texture discontinuity: {large} large pixels, total channel error {total}");
                std::fs::write(out.join(format!("endpoint-{a}-{b}.json")),serde_json::to_vec_pretty(&serde_json::json!({"large_pixels_over32":large,"total_channel_error":total,"scope":"full unmasked400x240 Display RGB; bounded CORDIC/filter quantization"})).unwrap()).unwrap();
            }
        }
        results.push((out, frames, depths));
    }
    assert!(
        results[3].1.windows(2).all(|p| p[0] != p[1]),
        "goal opening must visibly close before fading"
    );
    let (_, late, late_depth) = &results[1];
    let (_, early, early_depth) = &results[2];
    assert_eq!(
        late_depth, early_depth,
        "transparent hints must not change opaque depth"
    );
    let differences: Vec<_> = late
        .iter()
        .zip(early)
        .map(|(a, b)| a.iter().zip(b).filter(|(x, y)| x != y).count())
        .collect();
    assert!(
        differences.iter().skip(1).any(|n| *n > 20),
        "falling block must expose the ordering regression"
    );
    std::fs::write(results[1].0.join("occlusion.json"),serde_json::to_vec_pretty(&serde_json::json!({"early_vs_late_different_pixels":differences,"opaque_depth_identical":true,"scope":"four heights over an inactive level29 bridge; corrected late blending versus explicit early-order control","maximum_steps_per_case":12000000})).unwrap()).unwrap();
}

#[test]
fn split_swap_merge_undo_and_reset_preserve_each_actor_orientation() {
    let body = r#"
GAME_SESSION.begin();GAME_SESSION.reset_level(7);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.animation.held_lock=0;
GAME_SESSION.rules.x=2;GAME_SESSION.rules.z=4;GAME_SESSION.rules.orientation=1;check(GAME_SESSION.try_move(0));
let mut n:u16=0;while n<11 {GAME_SESSION.tick(0,0,50);n+=1;}
let inherited=GAME_SESSION.rotations.current(0);check(inherited==GAME_SESSION.rotations.current(1)&&inherited!=8);
check(GAME_SESSION.try_move(6));check(GAME_SESSION.rotations.current(0)==inherited&&GAME_SESSION.rotations.current(1)==inherited);
GAME_SESSION.tick(32768,0,0);GAME_SESSION.animation.held_lock=0;
check(GAME_SESSION.try_move(2));n=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}
let first=GAME_SESSION.rotations.current(0);check(first==rotation_quarter(inherited,2)&&GAME_SESSION.rotations.current(1)==inherited);
check(GAME_SESSION.try_move(6));check(GAME_SESSION.rotations.current(0)==first&&GAME_SESSION.rotations.current(1)==inherited);
n=0;while n<4 {check(GAME_SESSION.try_move(3));let mut k:u16=0;while k<5 {GAME_SESSION.tick(0,0,50);k+=1;}n+=1;}
check(GAME_SESSION.rules.split==0);check(GAME_SESSION.rotations.current(0)==inherited);
GAME_SESSION.tick(32768,0,0);check(GAME_SESSION.rules.split!=0&&GAME_SESSION.rotations.current(0)==first);
check(GAME_SESSION.rotations.current(1)==rotation_quarter(rotation_quarter(rotation_quarter(inherited,3),3),3));
GAME_SESSION.reset_level(0);check(GAME_SESSION.rotations.current(0)==8&&GAME_SESSION.rotations.current(1)==8&&GAME_SESSION.rotations.animation()==8);GAME_SESSION.end();
"#;
    fn export_orientation(source: &Path, target: &Path) {
        let session = std::fs::read_to_string(source.join("session.rs")).unwrap();
        assert!(session.contains("use orientation::*;"));
        std::fs::write(
            target.join("session.rs"),
            session.replace("use orientation::*;", "pub use orientation::*;"),
        )
        .unwrap();
    }
    let (program, resources, _) = application_case(
        "orientation-split-lifecycle",
        Some(body),
        None,
        ApplicationCase {
            edit_modules: Some(export_orientation),
            ..ApplicationCase::default()
        },
    );
    let mut sim = cpu_v3_tang_nano_20k::system_sim::CpuV3SystemSim::new(
        cpu_v3_tang_nano_20k::system_sim::VBlankMode::Manual,
    );
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut halted = false;
    for _ in 0..2_000_000 {
        if let StepOutcome::Halted { signal } = sim.step().unwrap() {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted, "split lifecycle2M step bound");
}
