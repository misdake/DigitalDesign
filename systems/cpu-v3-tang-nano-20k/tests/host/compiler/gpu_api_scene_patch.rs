//! Persistent typed bindings, with a freshly recorded native payload oracle.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

const SOURCE: &str = r#"mod gpu_api;use gpu_api::*;
fn v(x:i16,y:i16,z:i16)->Vec3{Vec3{x:fix32::from_int(x),y:fix32::from_int(y),z:fix32::from_int(z)}}
fn main(){
 let first:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let second:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0xfc00,512)},push:unsafe{GpuMemory::from_sectors(0xfe00,512)}};
 let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:first,command_b:Some(second)});
 let mut a=gpu.command_buffer();let mut b=gpu.command_buffer();
 let mut cam=Camera::new(CameraDesc{eye:v(0,0,0),angles:v(0,0,0),projection:Projection::from_focal_pixels(fix32::from_int(200),fix32::from_int(120))});
 let model=Model::new(Transform{position:v(0,0,-3),angles:v(0,0,0),scale:v(1,1,1)});
 let material=Material::new(MaterialDesc{uniform_color:Rgba8{r:80,g:160,b:240,a:255},specular:Rgb8{r:10,g:20,b:30},shininess:Shininess::S32,ambient_response:fix32::from_words(32768,0),directional_response:fix32::from_words(16384,0),unlit:false});
 let state=RenderState::new(RenderStateDesc{depth:DepthFunc::Less,depth_write:true,blend:BlendMode::Replace,filter:TextureFilter::Nearest,lighting:Lighting{direction:v(0,0,1),ambient:fix32::from_int(1),directional:fix32::from_int(1)}});
 let color_a=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let color_b=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let mesh=Mesh::from_native(MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:unsafe{GpuMemory::from_sectors(0xa000,16)}});
 {let mut p=a.pass(&color_a,&depth,ColorLoad::clear(Rgb565::from_raw(0x1234)),DepthLoad::clear(65535));p.set_camera(&cam);p.set_state(&state);p.draw_colored(&mesh,&model,&material);p.draw_colored(&mesh,&model,&material);}
 let binding=a.scene_binding();dev_send(7,1,0);
 gpu.submit(a);a=gpu.wait();
 cam.set_pose(v(1,0,0),Vec3{x:fix32::from_int(0),y:fix32::from_words(8192,0),z:fix32::from_int(0)});
 dev_send(7,1,3);
 a.patch_scene(binding,&cam,&model,&color_b,&depth);dev_send(7,1,1);
 gpu.submit(a);a=gpu.wait();
 {let mut p=b.pass(&color_b,&depth,ColorLoad::clear(Rgb565::from_raw(0x1234)),DepthLoad::clear(65535));p.set_camera(&cam);p.set_state(&state);p.draw_colored(&mesh,&model,&material);p.draw_colored(&mesh,&model,&material);}
 let other_binding=b.scene_binding();dev_send(7,1,2);
 a.patch_scene(binding,&cam,&model,&color_a,&depth);dev_send(7,1,4);
 drop(mesh);drop(depth);drop(color_b);drop(color_a);drop(state);drop(material);drop(model);drop(cam);drop(a);drop(b);drop(gpu);halt(0x600d);
}"#;

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    commands: Vec<u16>,
    push: Vec<u16>,
    facts: Vec<u16>,
    control: Vec<u16>,
}
#[derive(Default)]
struct Seen {
    snapshots: Vec<(u16, Snapshot)>,
    starts: usize,
    busy: usize,
}
#[derive(Clone, Copy, Debug)]
enum Inject {
    None,
    Generation,
    TargetSpan,
    Owner(u16),
    Exhaust,
    Carry,
}
struct Peer {
    kind: u8,
    records: usize,
    facts: usize,
    execution: usize,
    generation: usize,
    injection: Inject,
    seen: Rc<RefCell<Seen>>,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.seen.borrow_mut();
        if self.kind == 7 && channel == 2 {
            u16::from(s.snapshots.iter().any(|x| x.0 == 3))
        } else if self.kind == 4 && s.busy != 0 {
            s.busy -= 1;
            1
        } else {
            0
        }
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut s = self.seen.borrow_mut();
        if self.kind == 4 {
            if channel == 1 {
                s.starts += 1;
                s.busy = 2;
            }
        } else if self.kind == 7 {
            assert_eq!(channel, 1);
            let slot = usize::from(value == 2);
            let c = memory[self.records + slot * 224..self.records + slot * 224 + 224].to_vec();
            let command = usize::from(c[1]) * 64;
            let push = usize::from(c[3]) * 64;
            s.snapshots.push((
                value,
                Snapshot {
                    commands: memory[command..command + usize::from(c[7])].to_vec(),
                    push: memory[push..push + usize::from(c[6])].to_vec(),
                    facts: memory
                        [self.facts + slot * 256..self.facts + slot * 256 + usize::from(c[15]) * 4]
                        .to_vec(),
                    control: c,
                },
            ));
            if value == 3 {
                match self.injection {
                    Inject::None => (),
                    Inject::Generation => memory[self.records + 20] ^= 2,
                    Inject::TargetSpan => memory[self.records + 22] = memory[self.records + 6] + 1,
                    Inject::Owner(phase) => {
                        memory[self.execution] = phase;
                        memory[self.execution + 1] = 1;
                    }
                    Inject::Exhaust => {
                        memory[self.generation] = 65535;
                        memory[self.generation + 1] = 65535;
                    }
                    Inject::Carry => {
                        memory[self.generation] = 65535;
                        memory[self.generation + 1] = 5;
                    }
                }
            }
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
pub(super) fn compile(source: &str, debug: bool, label: &str) -> rcc_backend::CpuV3Program {
    use super::shared_build_support_rcc_modules as resolver;
    let mut root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/renderer-stage2/checks"));
    // Parallel consumers may inject different library files under the same label.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let instance = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = output.join(format!(
        "scene-patch-{label}-{debug}-{}-{instance}",
        std::process::id()
    ));
    let local = directory.join("application");
    std::fs::create_dir_all(&local).unwrap();
    if label.starts_with("storage-failure") || label == "submit-modes" {
        // Test-only callee failure. The scene patch itself remains byte-exact
        // production source; this does not qualify a hardware transport error.
        let copy = directory.join("library");
        std::fs::create_dir_all(&copy).unwrap();
        for entry in std::fs::read_dir(&root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|x| x == "rs") {
                std::fs::copy(&path, copy.join(path.file_name().unwrap())).unwrap();
            }
        }
        if label.starts_with("storage-failure") {
            let storage = copy.join("gpu_storage.rs");
            let original = std::fs::read_to_string(&storage).unwrap();
            let needle = "    gpu_storage_copy_owned(src, sector, word, count, false)";
            assert_eq!(original.matches(needle).count(), 1);
            let injected = original.replace(needle,
            "    if dev_recv(7,2)!=0 {TEST_STORAGE_WRITES[0u16]+=1;if TEST_STORAGE_WRITES[0u16]==2{return false;}}\n    gpu_storage_copy_owned(src, sector, word, count, false)");
            std::fs::write(
                storage,
                format!("{injected}\nstatic TEST_STORAGE_WRITES:Buf<u16,1>=Buf::new([0;1]);\n"),
            )
            .unwrap();
            assert_eq!(
                std::fs::read(root.join("gpu_scene_patch.rs")).unwrap(),
                std::fs::read(copy.join("gpu_scene_patch.rs")).unwrap()
            );
        } else {
            std::fs::write(copy.join("profiler_gate.rs"), r#"use crate::dsl_rt::*;mod gpu_capture_abi;use gpu_capture_abi::*;
pub fn profiler_gate(d:Array<u16>)->u16{dev_send(7,2,d[GPU_CAPTURE_H_SUBMISSION]);dev_send(7,3,d[GPU_CAPTURE_H_SUBMISSION+1]);0}"#).unwrap();
        }
        root = copy;
    }
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = debug;
    let configuration = if label.starts_with("submit-") {
        resolver::RendererCaptureConfiguration::with_default_hook()
    } else {
        resolver::RendererCaptureConfiguration::without_capture()
    };
    let ir = resolver::compile_explicit_gpu_program_checked(
        "scene-patch",
        source,
        &options,
        &local,
        &root,
        "",
        &configuration,
        &RustcCheck::new(directory),
        &mut |_| {},
    )
    .unwrap();
    rcc_backend::try_compile(ir, &options, "main").unwrap()
}
pub(super) fn global(code: &rcc_backend::CpuV3Program, name: &str) -> usize {
    match code
        .debug
        .globals
        .iter()
        .find(|g| g.name == name)
        .unwrap()
        .loc
    {
        rcc::VarLoc::Global(at) => usize::from(at),
        _ => unreachable!(),
    }
}
fn run(code: &rcc_backend::CpuV3Program, injection: Inject) -> (u16, Rc<RefCell<Seen>>, CpuV3Sim) {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
    for kind in [0, 4, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                records: global(code, "GPU_CB_RECORDS"),
                facts: global(code, "GPU_CB_CAPTURE_FACTS"),
                execution: global(code, "GPU_EXECUTION"),
                injection,
                seen: seen.clone(),
                generation: global(code, "GPU_RECORD_GENERATION"),
            }),
        );
    }
    for _ in 0..1_000_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            return (signal, seen, cpu);
        }
    }
    panic!("scene patch exceeded 1M instruction bound: {injection:?}");
}
#[test]
fn persistent_scene_patch_matches_fresh_record_and_survives_retirement() {
    let mut profile_gold = None;
    for debug in [false, true] {
        let code = compile(SOURCE, debug, "reuse");
        let (signal, seen, _) = run(&code, Inject::None);
        assert_eq!(signal, 0x600d);
        let s = seen.borrow();
        assert_eq!(s.starts, 2);
        assert_eq!(
            s.snapshots.iter().map(|x| x.0).collect::<Vec<_>>(),
            [0, 3, 1, 2, 4]
        );
        let old = &s.snapshots[0].1;
        let patched = &s.snapshots[2].1;
        let fresh = &s.snapshots[3].1;
        let alternate = &s.snapshots[4].1;
        assert_eq!(
            old.commands, patched.commands,
            "immutable stream including END/padding"
        );
        assert_eq!(old.commands, alternate.commands);
        assert_eq!(
            patched.push, fresh.push,
            "complete native payload vs ordinary recording"
        );
        assert_eq!(patched.facts, fresh.facts, "retarget replaces facts");
        assert_eq!(
            &old.control[20..27],
            &alternate.control[20..27],
            "epoch/positions/counts persist"
        );
        let g = usize::from(old.control[23] - 1);
        let i = usize::from(old.control[11] - 1);
        assert_ne!(&old.push[g + 2..g + 5], &patched.push[g + 2..g + 5]);
        assert_ne!(&old.push[i..i + 42], &patched.push[i..i + 42]);
        assert_eq!(&old.push[i + 42..i + 64], &patched.push[i + 42..i + 64]);
        assert_eq!(
            &old.push[..16],
            &alternate.push[..16],
            "Back target alternates"
        );
        let healthy = (patched.push.clone(), patched.facts.clone());
        if let Some(gold) = &profile_gold {
            assert_eq!(&healthy, gold);
        }
        profile_gold = Some(healthy);
    }
}
#[test]
fn stale_span_and_driver_owned_patch_fail_before_backing_mutation() {
    for debug in [false, true] {
        let code = compile(SOURCE, debug, "reject");
        for injection in [
            Inject::Generation,
            Inject::TargetSpan,
            Inject::Owner(2),
            Inject::Owner(3),
            Inject::Owner(4),
            Inject::Owner(5),
        ] {
            let (signal, seen, mut cpu) = run(&code, injection);
            assert_eq!(signal, 0xe001, "{injection:?}");
            let s = seen.borrow();
            assert_eq!(s.starts, 1, "no START after rejection");
            assert_eq!(s.snapshots.len(), 2);
            let before = &s.snapshots[1].1;
            let memory = cpu.physical_memory_mut();
            let push = usize::from(before.control[3]) * 64;
            let command = usize::from(before.control[1]) * 64;
            assert_eq!(&memory[push..push + before.push.len()], before.push);
            assert_eq!(
                &memory[command..command + before.commands.len()],
                before.commands
            );
        }
    }
}

#[test]
fn reset_generation_carries_and_exhaustion_does_not_mutate_the_record() {
    let prefix = SOURCE
        .split(" a.patch_scene(binding,&cam,&model,&color_b,&depth);")
        .next()
        .unwrap();
    let source = format!(
        "{prefix}a.reset();dev_send(7,1,5);drop(mesh);drop(depth);drop(color_b);drop(color_a);drop(state);drop(material);drop(model);drop(cam);drop(a);drop(b);drop(gpu);halt(0x600d);}}"
    );
    for debug in [false, true] {
        let code = compile(&source, debug, "generation");
        let (signal, seen, _) = run(&code, Inject::Carry);
        assert_eq!(signal, 0x600d);
        let s = seen.borrow();
        assert_eq!(s.snapshots.last().unwrap().0, 5);
        assert_eq!(&s.snapshots.last().unwrap().1.control[20..22], &[0, 6]);
        drop(s);
        let (signal, seen, mut cpu) = run(&code, Inject::Exhaust);
        assert_eq!(signal, 0xe001);
        let s = seen.borrow();
        let before = &s.snapshots.last().unwrap().1;
        assert_eq!(s.starts, 1);
        let at = global(&code, "GPU_CB_RECORDS");
        assert_eq!(&cpu.physical_memory_mut()[at..at + 224], before.control);
    }
}

#[test]
fn partial_storage_failure_poison_preserves_shadows_and_blocks_start() {
    for debug in [false, true] {
        let code = compile(SOURCE, debug, "storage-failure");
        let (signal, seen, mut cpu) = run(&code, Inject::None);
        assert_eq!(signal, 0xe001);
        let s = seen.borrow();
        assert_eq!(s.starts, 1);
        let before = &s.snapshots.last().unwrap().1;
        let records = global(&code, "GPU_CB_RECORDS");
        let memory = cpu.physical_memory_mut();
        let c = &memory[records..records + 224];
        assert_eq!(c[0], 4, "failed storage poisons the CB");
        assert_eq!(
            &c[1..],
            &before.control[1..],
            "no shadow/target commit on partial failure"
        );
        let push = usize::from(c[3]) * 64;
        let g = usize::from(c[23] - 1);
        let i = usize::from(c[11] - 1);
        assert_ne!(
            &memory[push + g + 2..push + g + 5],
            &before.push[g + 2..g + 5],
            "the first actual copy completed"
        );
        assert_eq!(
            &memory[push + i..push + i + 64],
            &before.push[i..i + 64],
            "second copy rejected"
        );
        assert_eq!(
            &memory[push..push + 16],
            &before.push[..16],
            "target not yet written"
        );
    }
}

#[test]
fn foreign_binding_projection_and_actual_reset_reject_before_patch_writes() {
    let call = "a.patch_scene(binding,&cam,&model,&color_b,&depth);";
    let rerecord = r#"a.reset();
 {let mut p=a.pass(&color_b,&depth,ColorLoad::clear(Rgb565::from_raw(0x1234)),DepthLoad::clear(65535));p.set_camera(&cam);p.set_state(&state);p.draw_colored(&mesh,&model,&material);p.draw_colored(&mesh,&model,&material);}
 let current=a.scene_binding();dev_send(7,1,6);
 a.patch_scene(binding,&cam,&model,&color_b,&depth);"#;
    for (label, replacement) in [
        (
            "foreign",
            "b.patch_scene(binding,&cam,&model,&color_b,&depth);",
        ),
        (
            "projection",
            "cam.set_projection(Projection::from_focal_pixels(fix32::from_int(220),fix32::from_int(120)));a.patch_scene(binding,&cam,&model,&color_b,&depth);",
        ),
        (
            "light-range",
            "let mut view=cam.view();view.row0.z=fix32::from_int(2);cam.set_view(view);a.patch_scene(binding,&cam,&model,&color_b,&depth);",
        ),
        ("reset-stale", rerecord),
    ] {
        for debug in [false, true] {
            let code = compile(&SOURCE.replacen(call, replacement, 1), debug, label);
            let (signal, seen, mut cpu) = run(&code, Inject::None);
            assert_eq!(signal, 0xe001, "{label}/{debug}");
            let s = seen.borrow();
            assert_eq!(s.starts, 1);
            let before = &s.snapshots.last().unwrap().1;
            let push = usize::from(before.control[3]) * 64;
            let memory = cpu.physical_memory_mut();
            assert_eq!(&memory[push..push + before.push.len()], before.push);
        }
    }
}

#[test]
fn binding_from_a_dropped_context_cannot_match_reused_slots() {
    let (preamble, body) = SOURCE.split_once("fn main(){").unwrap();
    let prefix = body
        .split(" let binding=a.scene_binding();")
        .next()
        .unwrap();
    let drops = "drop(mesh);drop(depth);drop(color_b);drop(color_a);drop(state);drop(material);drop(model);drop(cam);drop(a);drop(b);drop(gpu);";
    let source = format!(
        "{preamble}fn previous()->SceneBinding{{{prefix}let token=a.scene_binding();{drops}token}}\nfn main(){{let old=previous();{prefix}let current=a.scene_binding();dev_send(7,1,0);a.patch_scene(old,&cam,&model,&color_b,&depth);{drops}halt(0x600d);}}"
    );
    for debug in [false, true] {
        let code = compile(&source, debug, "context-reuse");
        let (signal, seen, mut cpu) = run(&code, Inject::None);
        assert_eq!(signal, 0xe001);
        let s = seen.borrow();
        assert_eq!(s.starts, 0);
        let before = &s.snapshots.last().unwrap().1;
        assert_eq!(
            before.control[20], 3,
            "context init does not rewind the global generation"
        );
        let push = usize::from(before.control[3]) * 64;
        assert_eq!(
            &cpu.physical_memory_mut()[push..push + before.push.len()],
            before.push
        );
    }
}

#[test]
fn numerically_equal_unrelated_models_are_not_a_shared_patch_group() {
    let source = SOURCE.replace("let material=", "let other=Model::new(Transform{position:v(0,0,-3),angles:v(0,0,0),scale:v(1,1,1)});let material=")
        .replacen("p.draw_colored(&mesh,&model,&material);p.draw_colored(&mesh,&model,&material);",
            "p.draw_colored(&mesh,&model,&material);p.draw_colored(&mesh,&other,&material);", 1);
    for debug in [false, true] {
        let code = compile(&source, debug, "equal-foreign-model");
        let (signal, seen, mut cpu) = run(&code, Inject::None);
        assert_eq!(signal, 0xe001);
        assert_eq!(seen.borrow().starts, 0);
        let at = global(&code, "GPU_CB_RECORDS");
        let c = &cpu.physical_memory_mut()[at..at + 224];
        assert_eq!(c[26], 1, "the two equal instances really deduplicated");
        assert_eq!(c[29], 65535, "the distinct models remain distinguishable");
    }
}

#[path = "gpu_api_target_patch.rs"]
mod target_patch;
