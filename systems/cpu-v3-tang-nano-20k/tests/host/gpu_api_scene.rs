//! Public resource adapters consume the fixed B production helpers.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use std::any::Any;
const SOURCE: &str = r#"mod gpu_api;use gpu_api::*;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn v(x:i16,y:i16,z:i16)->Vec3{Vec3{x:fix32::from_int(x),y:fix32::from_int(y),z:fix32::from_int(z)}}
fn make_camera()->Camera{
 let projection=Projection::from_focal_pixels(fix32::from_int(200),fix32::from_int(100));
 check(projection.focal_pixels()==fix32::from_int(200));
 let mut camera=Camera::new(CameraDesc{eye:v(0,0,0),angles:v(0,0,0),projection:projection});
 camera.set_projection(Projection::from_vertical_fov(fix32::from_int(1),fix32::from_int(100)));
 let projection=Projection::from_horizontal_fov(fix32::from_int(1),fix32::from_int(100));camera.set_projection(projection);
 camera.set_pose(v(0,0,0),v(0,0,0));let view=camera.view();camera.set_view(view);

 camera
}
fn make_model()->Model{
 let mut model=Model::new(Transform{position:v(0,0,-3),angles:v(0,0,0),scale:v(1,1,1)});
 model.set_pose(Transform{position:v(0,0,-3),angles:v(0,0,0),scale:v(1,1,1)});let world=model.world();
 let normal:Mat3x3=Mat3x3{row0:v(1,0,0),row1:v(0,1,0),row2:v(0,0,1)};model.set_matrices(world,normal);

 model
}
fn make_material()->Material{
 let shininess=Shininess::nearest(18);check(shininess.exponent()==16);
 let mut material=Material::new(MaterialDesc{uniform_color:Rgba8{r:180,g:120,b:60,a:255},specular:Rgb8{r:255,g:255,b:255},ambient_response:fix32::from_words(32768,0),directional_response:fix32::from_words(32768,0),shininess:shininess,unlit:false});
 material.set_uniform_color(Rgba8{r:255,g:200,b:100,a:255});material.set_specular(Rgb8{r:255,g:255,b:255},shininess);material.set_response(fix32::from_words(32768,0),fix32::from_words(32768,0));material.set_unlit(false);

 material
}
fn make_state()->RenderState{
 let light:Lighting=Lighting{direction:v(0,0,1),ambient:fix32::from_int(1),directional:fix32::from_int(1)};
 let mut state=RenderState::new(RenderStateDesc{depth:DepthFunc::LessEqual,depth_write:true,blend:BlendMode::Replace,filter:TextureFilter::Bilinear,lighting:light});
 state.set_depth(DepthFunc::LessEqual,true);state.set_blend(BlendMode::Replace);state.set_filter(TextureFilter::Bilinear);state.set_lighting(light);

 state
}
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});
 let mut cb=gpu.command_buffer();
 let camera=make_camera();
 let model=make_model();
 let material=make_material();
 let state=make_state();
 let texture=Texture::from_native(TextureDesc{memory:unsafe{GpuMemory::from_sectors(0xa800,128)},size_log2:6,full_mip:true});
 let texture_view=texture.view();drop(texture);check(texture_view.width()==64&&texture_view.height()==64);
 let color=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let rtt=color.texture_view();check(rtt.width()==400&&rtt.height()==240);drop(rtt);
 let mesh=Mesh::from_native(MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:unsafe{GpuMemory::from_sectors(0xa000,16)}});
 let empty=mesh.subset(0,0);
 let atlas=UiAtlas::from_native(unsafe{GpuMemory::from_sectors(0xb000,6)},3);
 let mut grid=UiGrid::from_memory(unsafe{GpuMemory::from_sectors(0xb100,6)},&atlas);drop(atlas);
 grid.fill(255);grid.write(CellRect{x:1,y:1,width:3,height:1},b"\0\x01\x02");
 {let mut pass=cb.pass(&color,&depth,ColorLoad::load(),DepthLoad::load());
  pass.set_camera(&camera);pass.set_state(&state);
  pass.draw(&mesh,&model,&material,&texture_view);pass.draw_colored(&mesh,&model,&material);
  pass.draw_colored(&empty,&model,&material);
  pass.draw_ui(&grid,CellRect{x:1,y:1,width:3,height:1});
 }
 check(cb.command_words()>20&&cb.push_words()>80);
 drop(empty);drop(mesh);drop(grid);drop(texture_view);drop(camera);drop(model);drop(material);drop(state);drop(color);drop(depth);drop(cb);drop(gpu);halt(0x600d);
}"#;

#[test]
fn public_scene_factories_bind_draw_and_live_ui_record_in_both_profiles() {
    for debug in [false, true] {
        let parsed = super::gpu_api_resources::parse(SOURCE, "public-b-scene", debug)
            .unwrap_or_else(|error| panic!("{error}"));
        let static_words = parsed.debug.static_data_end;
        assert!(static_words <= 8192);
        let global = |name| match parsed
            .debug
            .globals
            .iter()
            .find(|item| item.name == name)
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(address) => usize::from(address),
            _ => unreachable!(),
        };
        let record = global("GPU_CB_RECORDS");
        let facts = global("GPU_CB_CAPTURE_FACTS");
        let fault = match parsed
            .debug
            .globals
            .iter()
            .find(|item| item.name == "GPU_API_FAULT")
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(address) => usize::from(address),
            _ => unreachable!(),
        };
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
        let mut halted = false;
        let mut minimum_sp = 0xc000u16;
        for _ in 0..5_000_000 {
            let sp = cpu.registers()[13];
            if sp != 0 {
                minimum_sp = minimum_sp.min(sp);
            }
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(
                    signal,
                    0x600d,
                    "fault {:?}",
                    &cpu.physical_memory_mut()[fault..fault + 6]
                );
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded public scene adapter timeout");
        let stack_words = usize::from(0xc000 - minimum_sp);
        assert!(
            static_words + stack_words <= 8192,
            "combined static/observed-stack budget"
        );
        let memory = cpu.physical_memory_mut();
        assert_eq!(
            memory[record + 15],
            6,
            "actual Mesh/Texture/Grid/Atlas facts missing or empty draw added a fact"
        );
        let kinds: Vec<_> = memory[facts..facts + 24]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|fact| fact[2])
            .collect();
        assert_eq!(kinds, [5, 6, 1, 2, 4, 3]);
        let output = std::path::PathBuf::from(std::env::var_os("RCC_RUSTC_TEST_DIR").unwrap())
            .join(format!("public-scene-budget-{debug}.json"));
        std::fs::write(output,serde_json::to_vec_pretty(&serde_json::json!({"static_bytes":static_words*2,"observed_stack_bytes":stack_words*2,"max_frame_words":program.debug.functions.iter().map(|function|function.frame_size).max().unwrap_or(0),"fact_words":&memory[facts..facts+24],"commands":&memory[0x9800*64..0x9800*64+usize::from(memory[record+5])],"push":&memory[0x9a00*64..0x9a00*64+usize::from(memory[record+6])]})).unwrap()).unwrap();
        let grid = &memory[0xb100 * 64..0xb100 * 64 + 375];
        assert_eq!(grid[25], 0x00ff);
        assert_eq!(grid[26], 0x0201);
        assert!(grid[..25].iter().all(|&word| word == 0xffff));
        let commands = &memory[0x9800 * 64..0x9800 * 64 + 128];
        assert_eq!(
            commands
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|words| words[1] >> 12 == 3)
                .count(),
            2
        );
        assert_eq!(
            commands
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|words| words[1] >> 12 == 11)
                .count(),
            1
        );
    }
}

struct NoSubmission;
impl cpu_v3::Device for NoSubmission {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        assert_eq!(channel, 2);
        0
    }
    fn write(&mut self, _: &mut [u16], _: u8, _: u16) {
        panic!("malformed captured dependency issued SEND");
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn malformed_native_dependency_fails_before_send_and_preserves_sealed_inputs() {
    let prefix = SOURCE
        .split(" check(cb.command_words()>20&&cb.push_words()>80);")
        .next()
        .unwrap();
    let source = format!("{prefix}gpu.request_snapshot();gpu.submit_capturable(cb);halt(0xbad);}}");
    for debug in [false, true] {
        let parsed = super::gpu_api_resources::parse(&source, "actual-abc-stage-boundary", debug)
            .unwrap_or_else(|error| panic!("{error}"));
        let global = |name| match parsed
            .debug
            .globals
            .iter()
            .find(|item| item.name == name)
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(address) => usize::from(address),
            _ => unreachable!(),
        };
        let execution = global("GPU_EXECUTION");
        let record = global("GPU_CB_RECORDS");
        let facts = global("GPU_CB_CAPTURE_FACTS");
        let capture = global("GPU_CAPTURE_CONTROL");
        let fault = global("GPU_API_FAULT");
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(4, Box::new(NoSubmission));
        cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
        let mut halted = false;
        for _ in 0..5_000_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0xe001);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded actual A/B/C malformed-dependency timeout");
        let memory = cpu.physical_memory_mut();
        assert_eq!(&memory[fault + 1..fault + 3], &[6, 6]);
        assert_eq!(&memory[execution..execution + 2], &[5, 1]);
        assert_eq!(memory[capture], 1);
        assert_ne!(memory[capture + 1], 0);
        assert_eq!(memory[capture + 4], 0);
        let e = &memory[execution..execution + 16];
        let c = &memory[record..record + 32];
        let source = [
            1, 0, e[2], e[8], c[5], e[9], e[10], e[11], e[12], e[13], 4, 2,
        ];
        let command = usize::from(e[2]) * 64;
        let push = usize::from(e[10]) * 64;
        let report = serde_json::json!({"scope":"actual A/B sealed input, malformed native dependency rejected before SEND","source12":source,"fact_words":&memory[facts..facts+usize::from(e[13])*4],"commands":&memory[command..command+usize::from(e[9])],"push":&memory[push..push+usize::from(e[12])]});
        let output = std::path::PathBuf::from(std::env::var_os("RCC_RUSTC_TEST_DIR").unwrap())
            .join(format!("actual-abc-sealed-input-{debug}.json"));
        std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
