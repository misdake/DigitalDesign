//! Static World packets, view dependencies and descriptor reuse.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use std::{any::Any, cell::RefCell, rc::Rc};
const SOURCE: &str = r#"mod gpu_api;use gpu_api::*;
fn v(x:i16,y:i16,z:i16)->Vec3{Vec3{x:fix32::from_int(x),y:fix32::from_int(y),z:fix32::from_int(z)}}
fn pose(x:i16)->Pose{Pose{position:v(x,0,-3),angles:v(0,0,0),scale:v(1,1,1)}}
fn projection(f:i16,far:i16)->Projection{Projection::from_focal_pixels(fix32::from_int(f),fix32::from_int(far))}
fn camera()->Camera{Camera::new(CameraDesc{eye:v(0,0,0),angles:v(0,0,0),projection:projection(200,100)})}
fn make_model(x:i16)->Transform{Transform::new(pose(x))}
fn set_model(model:&mut Transform,x:i16){let mut world=model.world();world.row0.w=fix32::from_int(x);let normal:Mat3x3=Mat3x3{row0:v(1,0,0),row1:v(0,1,0),row2:v(0,0,1)};model.set_matrices(world,normal);}
fn set_normal(model:&mut Transform){let world=model.world();let normal:Mat3x3=Mat3x3{row0:v(1,0,0),row1:v(0,1,0),row2:v(0,0,1)};model.set_matrices(world,normal);}
fn change_normal_tail(model:&mut Transform){let world=model.world();let normal:Mat3x3=Mat3x3{row0:v(1,0,0),row1:v(0,1,0),row2:v(0,0,2)};model.set_matrices(world,normal);}
fn move_camera(cam:&mut Camera){cam.set_pose(v(1,0,0),v(0,0,0));}
fn reproject(cam:&mut Camera,f:i16,far:i16){cam.set_projection(projection(f,far));}
fn relight(state:&mut RenderState){state.set_lighting(Lighting{direction:v(1,0,0),ambient:fix32::from_int(1),directional:fix32::from_int(1)});}
fn look(cam:&mut Camera,kind:u16){if kind==0{cam.set_look_at(v(0,0,0),v(0,0,-1),v(0,1,0));}else if kind==1{cam.set_look_at_y_up(v(0,0,0),v(0,0,-1));}else{cam.set_look_to(v(0,0,0),v(0,0,-1),v(0,1,0));}}
fn note(count:u16){dev_send(7,1,count);}
fn main(){let first:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let second:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0xfc00,512)},push:unsafe{GpuMemory::from_sectors(0xfe00,512)}};
 let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:first,command_b:Some(second)});
 let mut a=gpu.command_buffer();let mut b=gpu.command_buffer();let mut cam=camera();
 let other=Camera::look_at(v(0,0,0),v(0,0,-1),v(0,1,0),projection(200,100));
 let mut one=make_model(0);let two=make_model(2);
 let mut material=Material::new(MaterialDesc{uniform_color:Rgba8{r:255,g:200,b:100,a:255},specular:Rgb8{r:160,g:160,b:160},ambient_response:fix32::from_words(16384,0),directional_response:fix32::from_words(49152,0),shininess:Shininess::S32,unlit:false});
 let mut state=RenderState::new(RenderStateDesc{depth:DepthFunc::Less,depth_write:true,blend:BlendMode::Replace,filter:TextureFilter::Nearest,lighting:Lighting{direction:v(0,0,1),ambient:fix32::from_int(1),directional:fix32::from_int(1)}});
 let color=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let mesh=Mesh::from_native(MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:unsafe{GpuMemory::from_sectors(0xa000,16)}});
 {let mut p=a.pass(color.ptr(), depth.ptr(), ColorLoad::load(), DepthLoad::load());p.set_camera(cam.ptr());p.set_state(state.ptr());
  p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(1);p.draw_colored(two.ptr(), mesh.ptr(), material.ptr());note(2);p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(2);
  set_model(&mut one,1);p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(3);p.draw_colored(two.ptr(), mesh.ptr(), material.ptr());note(3);
  set_normal(&mut one);p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(3);
  change_normal_tail(&mut one);p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(4);
  material.set_uniform_color(Rgba8{r:80,g:160,b:240,a:255});relight(&mut state);p.set_state(state.ptr());p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(4);
  reproject(&mut cam,200,120);p.set_camera(cam.ptr());p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(4);
  reproject(&mut cam,220,120);p.set_camera(cam.ptr());p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(5);
  move_camera(&mut cam);p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(5);
  p.set_camera(cam.ptr());p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(6);
  let view=cam.view();cam.set_view(view);p.set_camera(cam.ptr());p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(7);
  p.set_camera(other.ptr());p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(8);
  p.set_camera(cam.ptr());p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(9);p.draw_colored(two.ptr(), mesh.ptr(), material.ptr());note(10);
 }
 {let mut p=a.pass(color.ptr(), depth.ptr(), ColorLoad::load(), DepthLoad::load());p.set_camera(cam.ptr());p.set_state(state.ptr());
  let mut q=b.pass(color.ptr(), depth.ptr(), ColorLoad::load(), DepthLoad::load());q.set_camera(other.ptr());q.set_state(state.ptr());
  p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(10);q.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(11);p.draw_colored(one.ptr(), mesh.ptr(), material.ptr());note(12);p.draw_colored(two.ptr(), mesh.ptr(), material.ptr());note(12);
 }
 drop(one);let replacement=make_model(7);
 {let mut p=a.pass(color.ptr(), depth.ptr(), ColorLoad::load(), DepthLoad::load());p.set_camera(cam.ptr());p.set_state(state.ptr());p.draw_colored(replacement.ptr(), mesh.ptr(), material.ptr());note(13);}
 drop(cam);let mut reused=camera();
 {let mut p=a.pass(color.ptr(), depth.ptr(), ColorLoad::load(), DepthLoad::load());p.set_camera(reused.ptr());p.set_state(state.ptr());p.draw_colored(replacement.ptr(), mesh.ptr(), material.ptr());note(14);
  look(&mut reused,0);p.set_camera(reused.ptr());p.draw_colored(replacement.ptr(), mesh.ptr(), material.ptr());note(15);
  look(&mut reused,1);p.set_camera(reused.ptr());p.draw_colored(replacement.ptr(), mesh.ptr(), material.ptr());note(16);
  look(&mut reused,2);p.set_camera(reused.ptr());p.draw_colored(replacement.ptr(), mesh.ptr(), material.ptr());note(17);
 }
 drop(replacement);drop(two);drop(reused);drop(other);drop(mesh);drop(material);drop(state);drop(color);drop(depth);drop(a);drop(b);drop(gpu);halt(0x600d);
}"#;
#[derive(Default)]
struct Seen {
    calls: usize,
    markers: Vec<usize>,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<Seen>>,
    force: bool,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], _: u8) -> u16 {
        0
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        assert_eq!(self.kind, 7, "recording must not submit");
        assert_eq!(channel, 1);
        let mut s = self.state.borrow_mut();
        if !self.force {
            assert_eq!(
                s.calls,
                usize::from(value),
                "dirty/revision recomputation count"
            );
        }
        let calls = s.calls;
        s.markers.push(calls);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[test]
fn mesh_and_billboard_keep_world_independent_of_camera_changes() {
    for (debug, legacy) in [(false, false), (false, true), (true, true)] {
        let source = if legacy {
            SOURCE.replace("unlit:false", "unlit:true")
                .replace("let mesh=", "let legacy_texture=Texture::from_native(TextureDesc{format:TextureFormat::Rgb565,memory:unsafe{GpuMemory::from_sectors(0xa200,128)},size_log2:6,full_mip:false});let legacy_texture_view=legacy_texture.view();let mesh=")
                .replace(".draw_colored(", ".draw_billboard(")
                .replace("material.ptr());", "material.ptr(),&legacy_texture_view,BillboardFacing::CameraPlane);")
                .replace("drop(replacement);", "drop(legacy_texture);drop(replacement);")
        } else {
            SOURCE.to_owned()
        };
        let parsed = super::gpu_api_resources::parse(&source, "model-owned-dirty", debug).unwrap();
        let record_words = parsed
            .debug
            .consts
            .iter()
            .find(|x| x.0 == "GPU_API_CB_WORDS")
            .unwrap()
            .2 as usize;
        let record = match parsed
            .debug
            .globals
            .iter()
            .find(|g| g.name == "GPU_CB_RECORDS")
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(a) => usize::from(a),
            _ => unreachable!(),
        };
        let options = rcc_backend::CompilerOptions {
            gpu_debug: debug,
            ..rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0)
        };
        let code = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        for old in [
            "gpu_scene_matrix_prepare",
            "gpu_model_matrix_prepare",
            "gpu_model_cache_init",
        ] {
            assert!(
                !code.debug.functions.iter().any(|f| f.name == old),
                "no per-model CPU view derivation: {old}"
            );
        }
        {
            let state = Rc::new(RefCell::new(Seen::default()));
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&code.words);
            cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
            for kind in [4, 7] {
                cpu.attach_device(
                    kind,
                    Box::new(Peer {
                        kind,
                        state: state.clone(),
                        force: true,
                    }),
                );
            }
            let mut halted = false;
            for _ in 0..2_000_000 {
                if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                    assert_eq!(signal, 0x600d);
                    halted = true;
                    break;
                }
            }
            assert!(halted, "model dirty2M step bound");
            assert_eq!(state.borrow().markers.len(), 25);
            let memory = cpu.physical_memory_mut();
            let snapshots: Vec<Vec<u16>> = (0..2)
                .map(|slot| {
                    let c =
                        &memory[record + slot * record_words..record + slot * record_words + 16];
                    let command = usize::from(c[1]) * 64;
                    let push = usize::from(c[3]) * 64;
                    let mut out = memory[command..command + usize::from(c[5])].to_vec();
                    out.extend_from_slice(&memory[push..push + usize::from(c[6])]);
                    out
                })
                .collect();
            assert_eq!(
                state.borrow().calls,
                0,
                "Mesh and Billboard never compose MV on CPU"
            );
            assert!(snapshots.iter().all(|s| !s.is_empty()));
        }
    }
}

#[test]
fn model_storage_failure_and_camera_revision_carry_preserve_boundaries() {
    for debug in [false, true] {
        for (bytes, signal) in [(64, 0xe001), (128, 0x600d)] {
            let source = format!(
                r#"mod gpu_api;use gpu_api::*;
fn main(){{let memory:CommandMemory=CommandMemory{{commands:unsafe{{GpuMemory::from_sectors(0x9800,512)}},push:unsafe{{GpuMemory::from_sectors(0x9a00,512)}}}};
let absent:Option<CommandMemory>=None;let gpu=Gpu::init(GpuConfig{{object_arena_bytes:{bytes},command_a:memory,command_b:absent}});
let z:Vec3=Vec3{{x:fix32::zero(),y:fix32::zero(),z:fix32::zero()}};let one:Vec3=Vec3{{x:fix32::from_int(1),y:fix32::from_int(1),z:fix32::from_int(1)}};
let model=Transform::new(Pose{{position:z,angles:z,scale:one}});drop(model);drop(gpu);halt(0x600d);}}"#
            );
            let parsed =
                super::gpu_api_resources::parse(&source, &format!("model-capacity-{bytes}"), debug)
                    .unwrap();
            let global = |name| match parsed
                .debug
                .globals
                .iter()
                .find(|g| g.name == name)
                .unwrap()
                .loc
            {
                rcc::VarLoc::Global(a) => usize::from(a),
                _ => unreachable!(),
            };
            let control = global("GPU_OBJECT_CONTROL");
            let fault = global("GPU_API_FAULT");
            let options = rcc_backend::CompilerOptions {
                gpu_debug: debug,
                ..rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0)
            };
            let code = rcc_backend::try_compile(parsed, &options, "main").unwrap();
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&code.words);
            cpu.attach_device(
                4,
                Box::new(Peer {
                    kind: 4,
                    state: Rc::new(RefCell::new(Seen::default())),
                    force: false,
                }),
            );
            let mut halted = false;
            for _ in 0..250_000 {
                if let StepOutcome::Halted { signal: actual } = cpu.step().unwrap() {
                    assert_eq!(actual, signal);
                    halted = true;
                    break;
                }
            }
            assert!(halted, "model allocation250K step bound");
            let m = cpu.physical_memory_mut();
            if bytes == 64 {
                assert_eq!(&m[fault..fault + 5], &[0xfa17, 7, 1, 2, 0]);
                assert_eq!(m[control + 3], 0);
            } else {
                assert_eq!(m[fault], 0);
                assert_eq!(m[control + 3], 1);
                assert_ne!(
                    m[control + 5],
                    0,
                    "the single source allocation is released"
                );
            }
        }
        let source = r#"mod gpu_source_revision;static CAMERA:Buf<u16,48>=Buf::new([0;48]);
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){GPU_SOURCE_REVISION_COUNTER[0u16]=65535;GPU_SOURCE_REVISION_COUNTER[1u16]=65535;
check(gpu_camera_revision(CAMERA.as_array()));check(CAMERA[32u16]==0&&CAMERA[33u16]==0&&CAMERA[34u16]==1&&CAMERA[35u16]==0);
let mut i:u16=0;while i<4{GPU_SOURCE_REVISION_COUNTER[i]=65535;i+=1;}
check(!gpu_camera_revision(CAMERA.as_array()));check(CAMERA[32u16]==0&&CAMERA[33u16]==0&&CAMERA[34u16]==1&&CAMERA[35u16]==0);
i=0;while i<4{check(GPU_SOURCE_REVISION_COUNTER[i]==65535);i+=1;}halt(0x600d);}"#;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = std::env::var_os("RCC_RUSTC_TEST_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root.join("../../target/model-matrix-dirty/checks"));
        let options = rcc_backend::CompilerOptions {
            gpu_debug: debug,
            ..rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0)
        };
        let parsed = crate::checked_flat_fixture::compile(
            "camera-revision",
            source,
            &options,
            &rcc::frontend::rustc_check::RustcCheck::new(
                out.join(format!("camera-revision-{debug}")),
            ),
            &mut |name| {
                std::fs::read_to_string(root.join(format!("rcc/{name}.rs")))
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let code = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let mut halted = false;
        for _ in 0..250_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "revision250K step bound");
    }
}
