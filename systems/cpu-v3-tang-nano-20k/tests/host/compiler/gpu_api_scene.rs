//! Public resource adapters consume the fixed B production helpers.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
pub(super) const SOURCE: &str = r#"mod gpu_api;use gpu_api::*;
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
fn make_model()->Transform{
 let mut model=Transform::new(Pose{position:v(0,0,-3),angles:v(0,0,0),scale:v(1,1,1)});
 model.set_pose(Pose{position:v(0,0,-3),angles:v(0,0,0),scale:v(1,1,1)});let world=model.world();
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
 let texture=Texture::from_native(TextureDesc{
        format: TextureFormat::Rgb565,memory:unsafe{GpuMemory::from_sectors(0xa800,128)},size_log2:6,full_mip:true});
 let texture_view=texture.view();drop(texture);check(texture_view.width()==64&&texture_view.height()==64);
 let color=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let rtt=color.texture_view();check(rtt.width()==400&&rtt.height()==240);
 let mesh=Mesh::from_native(MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:unsafe{GpuMemory::from_sectors(0xa000,16)}});
 let empty=mesh.subset(0,0);
 let atlas=UiAtlas::from_native(unsafe{GpuMemory::from_sectors(0xb000,6)},3);
 let mut grid=UiGrid::from_memory(unsafe{GpuMemory::from_sectors(0xb100,6)},&atlas);drop(atlas);
 grid.fill(255);grid.write_text(48,14,b"\x01\x02");grid.write_glyph(49,14,0);
 grid.write(CellRect{x:1,y:1,width:3,height:1},b"\0\x01\x02");
 {let mut pass=cb.pass(color.ptr(), depth.ptr(), ColorLoad::load(), DepthLoad::load());
  pass.set_camera(camera.ptr());pass.set_state(state.ptr());
  pass.draw(model.ptr(), mesh.ptr(), material.ptr(), &texture_view);pass.draw_colored(model.ptr(), mesh.ptr(), material.ptr());
  pass.draw_colored(model.ptr(), empty.ptr(), material.ptr());
  pass.draw_ui(grid.ptr(), CellRect{x:1,y:1,width:3,height:1});
 }
 check(cb.command_words()>20&&cb.push_words()>80);
 drop(empty);drop(mesh);drop(grid);drop(camera);drop(model);drop(material);drop(state);drop(color);drop(depth);drop(cb);drop(gpu);halt(0x600d);
}"#;

#[test]
fn public_scene_factories_bind_draw_and_live_ui_record_in_both_profiles() {
    for (debug, rgba4444, clamp, atlas_grid) in [
        (false, false, false, 0),
        (true, false, false, 0),
        (false, true, false, 0),
        (true, true, false, 0),
        (false, false, true, 0),
        (true, false, true, 0),
        (false, true, true, 0),
        (true, true, true, 0),
        (false, false, true, 1),
        (true, false, true, 1),
        (false, false, true, 2),
        (true, false, true, 2),
        (false, false, true, 3),
        (true, false, true, 3),
    ] {
        let mut source = if rgba4444 {
            SOURCE.replace("TextureFormat::Rgb565", "TextureFormat::Rgba4444")
        } else {
            SOURCE.to_owned()
        };
        if clamp {
            source = source.replace("let texture_view=texture.view();", "let mut texture_view=texture.view();texture_view.set_address_mode(TextureAddressMode::ClampToEdge);");
        }
        if atlas_grid != 0 {
            source=source.replace("texture_view.set_address_mode(TextureAddressMode::ClampToEdge);",
                &format!("texture_view.set_atlas_grid({atlas_grid});texture_view.set_address_mode(TextureAddressMode::Repeat);texture_view.set_address_mode(TextureAddressMode::ClampToEdge);"));
        }
        if atlas_grid == 3 {
            source = source.replace("let state=make_state();", "let mut state=make_state();state.set_lod_bias(fix!(-0.5));state.set_filter(TextureFilter::Trilinear);let state_ptr=state.ptr();state_ptr.set_filter(TextureFilter::Bilinear);state_ptr.set_lod_bias(fix!(-0.5));");
        }
        let parsed = super::gpu_api_resources::parse(&source, "public-b-scene", debug)
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
        let facts = global("GPU_CB_RESOURCE_FACTS");
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
        let output = crate::checked_flat_fixture::output_root().join(format!(
            "public-scene-budget-{debug}-rgba{rgba4444}-clamp{clamp}-grid{atlas_grid}.json"
        ));
        std::fs::write(output,serde_json::to_vec_pretty(&serde_json::json!({"static_bytes":static_words*2,"observed_stack_bytes":stack_words*2,"max_frame_words":program.debug.functions.iter().map(|function|function.frame_size).max().unwrap_or(0),"fact_words":&memory[facts..facts+24],"commands":&memory[0x9800*64..0x9800*64+usize::from(memory[record+5])],"push":&memory[0x9a00*64..0x9a00*64+usize::from(memory[record+6])]})).unwrap()).unwrap();
        let push = &memory[0x9a00 * 64..0x9a00 * 64 + usize::from(memory[record + 6])];
        let header = 0x36
            | if rgba4444 { 0x100 } else { 0 }
            | if clamp { 0x200 } else { 0 }
            | (atlas_grid << 10);
        assert!(
            push.windows(4).any(|w| w == [0xa800, header, 0xffff, 0xff]),
            "native format and mip/filter bits lost during public recording"
        );
        if atlas_grid == 3 {
            assert!(
                push.windows(8).any(|w| w[0] == 0xa800
                    && w[1] == header
                    && w[4] >> 8 == 0xf8
                    && w[6] == 0
                    && w[7] == 0),
                "signed half-level bias must survive owner/ptr filter updates and native recording"
            );
        }
        let grid = &memory[0xb100 * 64..0xb100 * 64 + 375];
        assert_eq!(grid[25], 0x00ff);
        assert_eq!(grid[26], 0x0201);
        assert_eq!(
            grid[374], 0x0001,
            "single glyph preserves its packed neighbour at the last cell"
        );
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

#[test]
fn unlit_bank_reuses_native_instances_and_matches_ordinary_binding_bytes() {
    const BANK_SOURCE: &str = r#"mod gpu_api;use gpu_api::*;
fn main(){
 let memory:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let other:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0xfc00,512)},push:unsafe{GpuMemory::from_sectors(0xfe00,512)}};
 let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:memory,command_b:Some(other)});
 let mut a=gpu.command_buffer();let mut b=gpu.command_buffer();
 let mut cam=Camera::new(CameraDesc::new(Vec3::zero(),Vec3::zero(),Projection::from_focal_pixels(fix32::from_int(200),fix32::from_int(120))));
 let model=Transform::new(Pose::identity());
 let red=Material::new(MaterialDesc::diffuse(Rgba8{r:220,g:10,b:20,a:255}).with_unlit(true));
 let blue=Material::new(MaterialDesc::diffuse(Rgba8{r:10,g:20,b:220,a:128}).with_unlit(true));
 let state=RenderState::new(RenderStateDesc::opaque(Lighting::ambient(fix32::one())));
 let color=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let mesh=Mesh::from_native(MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:unsafe{GpuMemory::from_sectors(0xa000,16)}});
 let mut bank=UnlitBank::new(mesh.ptr());
 let r=bank.add_colored(state.ptr(),red.ptr());
 let u=bank.add_colored(state.ptr(),blue.ptr());
 let again=bank.add_colored(state.ptr(),red.ptr());if r!=again{halt(0xbad);}
 a.reset();a.prepare_unlit(&mut bank,cam.ptr(),model.ptr());
 {let mut p=a.pass(color.ptr(),depth.ptr(),ColorLoad::load(),DepthLoad::load());p.set_camera(cam.ptr());p.set_state(state.ptr());
 p.draw_unlit(&bank,r,0,1);p.draw_unlit(&bank,u,0,1);p.draw_unlit(&bank,r,0,1);}
 cam.set_pose(Vec3::new(fix32::one(),fix32::zero(),fix32::zero()),Vec3::zero());
 a.patch_unlit(&bank,cam.ptr(),color.ptr(),depth.ptr());
 {let mut p=b.pass(color.ptr(),depth.ptr(),ColorLoad::load(),DepthLoad::load());p.set_camera(cam.ptr());p.set_state(state.ptr());
 p.draw_colored(model.ptr(),mesh.ptr(),red.ptr());p.draw_colored(model.ptr(),mesh.ptr(),blue.ptr());p.draw_colored(model.ptr(),mesh.ptr(),red.ptr());}
 halt(0x600d);
}"#;
    for debug in [false, true] {
        let program = compile_bank_fixture(BANK_SOURCE, debug, "unlit-bank-native");
        let records = match program
            .debug
            .globals
            .iter()
            .find(|g| g.name == "GPU_CB_RECORDS")
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(at) => usize::from(at),
            _ => unreachable!(),
        };
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
        let mut halted = false;
        for _ in 0..5_000_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded unlit-bank timeout");
        let memory = cpu.physical_memory_mut();
        let snapshots = |memory: &[u16], records: usize, slot: usize| {
            let c = &memory[records + slot * 160..records + (slot + 1) * 160];
            let commands =
                &memory[usize::from(c[1]) * 64..usize::from(c[1]) * 64 + usize::from(c[5])];
            let mut view = vec![0; 64];
            let mut world = vec![0; 64];
            let mut result = Vec::new();
            for code in commands.as_chunks::<2>().0 {
                let opcode = code[1] >> 12;
                if opcode == 1 {
                    let sector = (usize::from(code[1] & 4095) << 4) | usize::from(code[0] >> 12);
                    let word = usize::from((code[0] >> 6) & 48);
                    let local = (code[0] >> 3) & 127;
                    let source = sector * 64 + word;
                    let target = match local {
                        92 => &mut view,
                        80 => &mut world,
                        _ => continue,
                    };
                    let count = target.len();
                    target.copy_from_slice(&memory[source..source + count]);
                } else if opcode == 3 {
                    result.push((view.clone(), world.clone(), code[0] >> 1));
                }
            }
            result
        };
        assert_eq!(
            snapshots(memory, records, 0),
            snapshots(memory, records, 1),
            "fixed-bank native binding bytes differ"
        );
        assert_eq!(snapshots(memory, records, 0).len(), 3);
        assert_eq!(
            memory[records + 15],
            3,
            "mesh and two target resource facts"
        );
        let baseline = compile_bank_fixture(
            &BANK_SOURCE.replace(
                "cam.set_pose(Vec3::new(fix32::one(),fix32::zero(),fix32::zero()),Vec3::zero());",
                "",
            ),
            debug,
            "unlit-bank-tail-baseline",
        );
        let baseline_records = match baseline
            .debug
            .globals
            .iter()
            .find(|g| g.name == "GPU_CB_RECORDS")
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(at) => usize::from(at),
            _ => unreachable!(),
        };
        let mut baseline_cpu = CpuV3Sim::default();
        baseline_cpu.set_boot_window(&baseline.words);
        baseline_cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2]
            .copy_from_slice(&[0xa001, 1]);
        let mut finished = false;
        for _ in 0..5_000_000 {
            if let StepOutcome::Halted { signal } = baseline_cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                finished = true;
                break;
            }
        }
        assert!(finished);
        let expected_prefix =
            snapshots(baseline_cpu.physical_memory_mut(), baseline_records, 1)[..2].to_vec();
        for command in ["p.draw_colored(model.ptr(),mesh.ptr(),blue.ptr());"] {
            let mixed = BANK_SOURCE.replace(
                "p.draw_unlit(&bank,r,0,1);p.draw_unlit(&bank,u,0,1);",
                &format!("p.draw_unlit(&bank,r,0,1);{command}p.draw_unlit(&bank,r,0,1);"),
            );
            let rejected = compile_bank_fixture(&mixed, debug, "unlit-bank-exclusive");
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&rejected.words);
            cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
            let mut stopped = false;
            for _ in 0..5_000_000 {
                if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                    assert_ne!(signal, 0x600d, "mixed bank command accepted: {command}");
                    stopped = true;
                    break;
                }
            }
            assert!(stopped, "bounded mixed-bank timeout");
            let memory = cpu.physical_memory_mut();
            let used = usize::from(memory[records + 5]);
            let commands = &memory[0x9800 * 64..0x9800 * 64 + used];
            assert_eq!(
                commands
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .filter(|p| p[1] >> 12 == 3)
                    .count(),
                2,
                "generic tail must append, then bank resumption rejects before its draw"
            );
            assert_eq!(
                snapshots(memory, records, 0),
                expected_prefix,
                "generic tail must preserve its native Render/Instance bytes"
            );
        }
        for replacement in [
            "a.reset();",
            "a.reset();a.prepare_unlit(&mut bank,cam.ptr(),model.ptr());a.reset();",
            "a.reset();a.prepare_unlit(&mut bank,cam.ptr(),model.ptr());bank.add_colored(state.ptr(),red.ptr());",
        ] {
            let invalid = BANK_SOURCE.replace("a.reset();a.prepare_unlit(&mut bank,cam.ptr(),model.ptr());", replacement);
            let rejected = compile_bank_fixture(&invalid, debug, "unlit-bank-lifecycle");
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&rejected.words);
            cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
            let mut stopped = false;
            for _ in 0..5_000_000 {
                if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                    assert_ne!(signal, 0x600d, "invalid bank lifecycle accepted: {replacement}");
                    stopped = true;
                    break;
                }
            }
            assert!(stopped, "bounded rejected-bank timeout");
        }
    }
}

fn compile_bank_fixture(source: &str, debug: bool, label: &str) -> rcc_backend::CpuV3Program {
    let parsed = super::gpu_api_resources::parse(source, label, debug)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = debug;
    rcc_backend::try_compile(parsed, &options, "main").unwrap()
}
