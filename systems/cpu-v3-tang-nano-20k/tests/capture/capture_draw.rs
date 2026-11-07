//! Actual three-submission DRAW producer. Peers do not rasterize or scan out.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use resolver::capture_profile::abi;
use serde_json::{json, Value};
use std::{
    any::Any,
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};
#[path = "../support/capture_uart.rs"]
mod capture_uart;
#[path = "../../build_support/rcc_modules.rs"]
#[allow(dead_code)]
mod resolver;
const APP: &str = r#"mod gpu_pipeline;use gpu_pipeline::*;use crate::dsl_rt::*;
fn main(){let mut r=renderer_open();let zero=vec3::zero();let one=fix32::from_int(1);
let mut camera=renderer_camera(&mut r,zero,zero,fix32::from_int(160),fix32::from_int(32));
let mut mesh=renderer_mesh(&mut r);
let mut model=renderer_model(&mut r,vec3::new(fix32::zero(),fix32::zero(),fix32::from_int(-4)),zero,vec3::new(one,one,one));
let mut material=renderer_material(&mut r,255,0,0,false);
let mut i:u16=0;while i<3{
 let mut x=fix32::zero();if i==1{x=fix32::from_words(32768,0);}if i==2{x=-fix32::from_words(32768,0);}
 model_pose(&mut model,vec3::new(x,fix32::zero(),fix32::from_int(-4)),zero,vec3::new(one,one,one));
 let mut red:u16=0;let mut green:u16=0;let mut blue:u16=0;
 if i==0{red=255;}if i==1{green=255;}if i==2{blue=255;}
 material_color(&mut material,red,green,blue,255);
 let mut f=renderer_frame(&mut r,0x0841+i);let mut p=frame_pass(&mut f,&mut camera);
 pass_draw(&mut p,&mut mesh,&mut model,&mut material);pass_complete(p);frame_complete(f);i+=1;
}renderer_close(r);if gpu_capture_error()!=1{halt(0xbad);}halt(0x600d);}"#;
const GATE: &str = r#"use crate::dsl_rt::*;mod gpu_pipeline;use gpu_pipeline::*;mod gpu_capture_abi;use gpu_capture_abi::*;
pub fn profiler_gate(d:Array<u16>)->u16{
let mut i:u16=0;while i<GPU_CAPTURE_WORDS{signal(2,d[i]);i+=1;}
let mut r:u16=0;while r<d[GPU_CAPTURE_H_RANGE_COUNT]{
 i=0;while i<GPU_CAPTURE_RANGE_WORDS{signal(3,gpu_capture_range_word(r,i));i+=1;}
 signal(4,gpu_capture_read_word(r,0,0));
 let low=gpu_capture_range_word(r,GPU_CAPTURE_R_BYTES);let high=gpu_capture_range_word(r,GPU_CAPTURE_R_BYTES+1);
 let mut last=(low>>1)|(high<<15);let mut upper=high>>1;
 if last==0{last=65535;upper-=1;}else{last-=1;}
 signal(5,gpu_capture_read_word(r,last,upper));r+=1;
}signal(6,gpu_capture_error());0}"#;
#[derive(Default)]
struct State {
    active: u32,
    shadow: u32,
    format: u16,
    shadow_format: u16,
    pending: bool,
    gpu: u16,
    counts: usize,
    starts: usize,
    swaps: usize,
    full_scene: bool,
    cycles: u64,
    us_origin: u64,
    input_reads: usize,
    timer_clears: usize,
    publications: usize,
    gate_publications: Vec<usize>,
    start_publications: Vec<usize>,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<State>>,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], c: u8) -> u16 {
        let mut s = self.state.borrow_mut();
        match (self.kind, c) {
            (0, 10) if s.full_scene => (s.cycles / 54_000) as u16,
            (0, 11) if s.full_scene => ((s.cycles - s.us_origin) / 54).min(65535) as u16,
            (5, 8) if s.full_scene => 1,
            (5, 2) if s.full_scene => 0x73,
            (5, 3) if s.full_scene => 0x5a,
            (5, 4 | 5) if s.full_scene => 255,
            (5, 0 | 1 | 6 | 7) if s.full_scene => 128,
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 4) => s.format,
            (3, 3) => {
                if s.pending {
                    s.pending = false;
                    s.active = s.shadow;
                    s.format = s.shadow_format;
                    1
                } else {
                    0
                }
            }
            (4, 2) => {
                let value = s.gpu;
                s.gpu = 0;
                value
            }
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], c: u8, v: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, c) {
            (0, 1) => s.publications += 1,
            (0, 11) if s.full_scene => {
                s.us_origin = s.cycles;
                s.timer_clears += 1;
            }
            (5, 8) if s.full_scene => s.input_reads += 1,
            (4, 0) => {
                assert_eq!(s.gpu, 0);
                s.counts += 1;
            }
            (4, 1) => {
                assert!(!s.pending);
                s.starts += 1;
                let publication = s.publications;
                s.start_publications.push(publication);
                s.gpu = 1;
            }
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(v),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(v) << 16),
            (3, 4) => {
                if s.full_scene {
                    assert_eq!(v, 1, "canonical scene requires linear RGB565 presentation");
                }
                s.shadow_format = v;
            }
            (3, 3) => {
                assert_eq!(s.gpu, 0);
                assert!(!s.pending);
                s.pending = true;
                s.swaps += 1;
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn call(root: &Path, args: &[&str]) {
    let p = std::process::Command::new("python")
        .arg("-B")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        p.status.success(),
        "{}{}",
        String::from_utf8_lossy(&p.stdout),
        String::from_utf8_lossy(&p.stderr)
    );
}
fn global(cpu: &mut CpuV3Sim, p: &rcc_backend::CpuV3Program, name: &str, n: usize) -> Vec<u16> {
    let a = p
        .debug
        .globals
        .iter()
        .find_map(|g| match g.loc {
            rcc::VarLoc::Global(a) if g.name == name => Some(usize::from(a)),
            _ => None,
        })
        .unwrap();
    cpu.physical_memory_mut()[a..a + n].to_vec()
}

fn pair(words: &[u16], at: u16) -> u32 {
    let i = usize::from(at);
    u32::from(words[i]) | (u32::from(words[i + 1]) << 16)
}
fn bytes(cpu: &mut CpuV3Sim, base: u32, size: u32) -> Vec<u8> {
    cpu.physical_memory_mut()[base as usize / 2..(base + size) as usize / 2]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
}
fn words(data: &[u8]) -> Vec<u16> {
    data.as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_le_bytes(*p))
        .collect()
}
fn inspect_draw(
    commands: &[u8],
    objects: &[u8],
    object_base: u32,
    frame: usize,
    texture_base: u32,
) -> Value {
    let objects = words(objects);
    let mut scratch = vec![0u16; 1536];
    let mut render = vec![];
    let mut matrix = vec![];
    let mut global = vec![];
    let mut draws = 0;
    let commands: Vec<_> = commands
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    for command in &commands {
        match command >> 28 {
            1 => {
                let source = ((command >> 10) & 0x3ffff) * 32;
                let dst = (((command >> 3) & 127) * 16) as usize;
                let count = (32usize << ((command >> 1) & 3)) / 2;
                assert!(source >= object_base);
                let at = ((source - object_base) / 2) as usize;
                assert!(at + count <= objects.len());
                scratch[dst..dst + count].copy_from_slice(&objects[at..at + count]);
            }
            5 => {
                let at = ((command & 127) * 16) as usize;
                global = scratch[at..at + 32].to_vec();
            }
            6 => {
                let at = ((command & 127) * 16) as usize;
                render = scratch[at..at + 16].to_vec();
            }
            3 => {
                draws += 1;
                matrix = scratch[1280..1344].to_vec();
            }
            15 => break,
            _ => {}
        }
    }
    assert_eq!(draws, 1);
    assert_eq!(commands.last().unwrap() >> 28, 15);
    assert!(commands.iter().any(|c| c >> 28 == 8));
    assert_eq!(u32::from(render[0]) * 128, texture_base);
    assert_eq!(render[4] & 63, 1 | 8 | 32, "LESS/write/unlit, no blend");
    let raw: Vec<i32> = matrix
        .as_chunks::<2>()
        .0
        .iter()
        .map(|w| pair(w, 0) as i32)
        .collect();
    let offsets = [0, -20480, 20480];
    let mut expected = [0i32; 12];
    expected[0] = -40960;
    expected[3] = offsets[frame];
    expected[5] = -40960;
    expected[10] = -65536;
    expected[11] = 4 * 65536;
    for (a, b) in raw.iter().zip(expected) {
        assert!(
            (i64::from(*a) - i64::from(b)).abs() <= 4,
            "matrix {a} vs {b}"
        );
    }
    let rgba = [0xff0000ff, 0xff00ff00, 0xffff0000];
    assert_eq!(raw[22] as u32, rgba[frame]);
    assert_eq!(pair(&global, 0), 32 * 65536);
    let points = [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]];
    let view: Vec<[f64; 3]> = points
        .iter()
        .map(|p| {
            std::array::from_fn(|r| {
                (f64::from(raw[4 * r]) * p[0]
                    + f64::from(raw[4 * r + 1]) * p[1]
                    + f64::from(raw[4 * r + 2]) * p[2]
                    + f64::from(raw[4 * r + 3]))
                    / 65536.0
            })
        })
        .collect();
    let screen: Vec<_> = view
        .iter()
        .map(|p| {
            assert!((3.9..4.1).contains(&p[2]));
            let s = [200.0 + 256.0 * p[0] / p[2], 120.0 + 256.0 * p[1] / p[2]];
            assert!((32.0..368.0).contains(&s[0]) && (32.0..208.0).contains(&s[1]));
            s
        })
        .collect();
    json!({"draws":draws,"command_count":commands.len(),"render_words":render,"global_words":global,"instance_i32":raw,"native_vertices":view,"ideal_projection_check":screen,
        "scene":{"vertices":[[-1,-1,0],[1,-1,0],[0,1,0]],"model_x":([0.0,0.5,-0.5][frame]),"model_z":-4,"focal":160,"far":32,"rgba_word":rgba[frame],"flat_rgb565":([0xf800,0x07e0,0x001f][frame]),"clear_rgb565":0x0841+frame},
        "boundary":"CPU recording and conservative positive-Z/interior checks only. GPU owner must independently derive raster coverage/depth from actual input; these screen values are not a GPU oracle."})
}
fn inspect_full_scene(
    commands: &[u8],
    objects: &[u8],
    object_base: u32,
    ui: &[u8],
    ui_base: u32,
    catalog: &Value,
    mesh: &[u8],
) -> Value {
    let commands: Vec<_> = commands
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    let mut scratch = vec![0u16; 1536];
    let mut pages = [0u32; 2];
    let mut renders = vec![];
    let mut globals: Vec<Vec<u16>> = vec![];
    let mut draws = vec![];
    let mut render = vec![];
    let mut ui_commands = vec![];
    let mesh_base = catalog["mesh"]["first_sector"].as_u64().unwrap() as u32 * 128;
    for command in &commands {
        match command >> 28 {
            1 => {
                let source = ((command >> 10) & 0x3ffff) * 32;
                let dst = (((command >> 3) & 127) * 16) as usize;
                let count = (32usize << ((command >> 1) & 3)) / 2;
                let (base, data) = if source >= object_base
                    && u64::from(source) + count as u64 * 2
                        <= u64::from(object_base) + objects.len() as u64
                {
                    (object_base, objects)
                } else {
                    (ui_base, ui)
                };
                assert!(source >= base);
                let at = (source - base) as usize;
                let source_words = words(&data[at..at + count * 2]);
                scratch[dst..dst + count].copy_from_slice(&source_words);
            }
            2 => {
                pages[(command & 1) as usize] = ((command >> 12) & 65535) * 128;
            }
            3 => {
                let index = draws.len();
                let expected = &catalog["mesh"]["entries"][index];
                let address = pages[(command & 1) as usize];
                assert_eq!(u64::from(address), expected["address"].as_u64().unwrap());
                let at = (address - mesh_base) as usize;
                let info = u32::from_le_bytes(mesh[at..at + 4].try_into().unwrap());
                assert_eq!(
                    render[4] & 63,
                    if index < 7 { 41 } else { 9 },
                    "historical LESS/depth-write, unlit room/RGB then lit rabbit"
                );
                // Historical native contract, independently decoded from source96eb7283:
                // ordinary 64x64 full-mip view, trilinear; RGB-only draw has no view.
                if index != 6 {
                    assert_eq!(render[1], 0x56, "historical exp6/full-mip/trilinear");
                }
                if index >= 7 {
                    assert_eq!(
                        globals.last().unwrap()[10],
                        0x33,
                        "historical mode3/shininess code12 (exponent32)"
                    );
                }
                draws.push(json!({"index":index,"name":expected["name"],"meshlet_address":address,"vertices":info&127,"triangles":(info>>8)&127,"render_words":render,"instance_words":scratch[1280..1344]}));
            }
            5 => {
                let at = ((command & 127) * 16) as usize;
                globals.push(scratch[at..at + 32].to_vec());
            }
            6 => {
                let at = ((command & 127) * 16) as usize;
                render = scratch[at..at + 16].to_vec();
                renders.push(render.clone());
            }
            11 => ui_commands.push(*command),
            15 => break,
            _ => {}
        }
    }
    assert_eq!(draws.len(), 24);
    let triangles: u64 = draws.iter().map(|d| d["triangles"].as_u64().unwrap()).sum();
    assert_eq!(triangles, 1141);
    assert_eq!(
        draws[7..]
            .iter()
            .map(|d| d["triangles"].as_u64().unwrap())
            .sum::<u64>(),
        948
    );
    assert_eq!(commands.last().unwrap() >> 28, 15);
    assert_eq!(commands.iter().filter(|c| *c >> 28 == 8).count(), 2);
    assert_eq!(ui_commands.len(), 1);
    let glyphs = ui[..8].to_vec();
    assert!(glyphs
        .iter()
        .take_while(|&&b| b != 255)
        .all(u8::is_ascii_digit));
    json!({"draws":draws,"triangle_count":triangles,"command_count":commands.len(),"render_bindings":renders,"global_bindings":globals,"ui_commands":ui_commands,"ui_glyphs":glyphs,"ui_cells_prefix":&ui[..16],"boundary":"Actual recorded DRAW topology, bindings and UI only; no raster/depth oracle or GPU execution."})
}
fn run(profile: &str, full_scene: bool, system_frontend: bool) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = root.join("../..");
    let base = std::env::var_os(if full_scene {
        "GPU_CAPTURE_FULL_SCENE_DIR"
    } else {
        "GPU_CAPTURE_DRAW_DIR"
    })
    .map(PathBuf::from)
    .unwrap_or_else(|| {
        repo.join("target/gpu-capture-fixtures").join(format!(
            "gpu-capture-{}-{}",
            if full_scene { "full-scene" } else { "draw" },
            std::process::id()
        ))
    });
    let out = base.join(if system_frontend {
        "system-frontend"
    } else {
        profile
    });
    let assets = out.join("assets");
    std::fs::create_dir_all(&out).unwrap();
    call(
        &repo,
        &[
            if full_scene {
                "systems/cpu-v3-tang-nano-20k/tools/capture_full_assets.py"
            } else {
                "systems/cpu-v3-tang-nano-20k/examples/gpu_renderer/tools/assets.py"
            },
            "--output",
            assets.to_str().unwrap(),
        ],
    );
    let mut app = if full_scene {
        include_str!("../../examples/gpu_profiler_board/rcc/full_scene.rs").to_owned()
            + "\nfn main(){profiler_full_three_frames();}\n"
    } else {
        APP.to_owned()
    };
    if system_frontend && !full_scene {
        app = app
            .replace(
                "let zero=vec3::zero();",
                "let mut label=renderer_text(&mut r,24,10,8);let zero=vec3::zero();",
            )
            .replace(
                "pass_complete(p);",
                "pass_observed_time(&mut p,&mut label);pass_complete(p);",
            );
        app = app.replace("pass_draw(&mut p,&mut mesh,&mut model,&mut material);",
            "pass_draw(&mut p,&mut mesh,&mut model,&mut material);material_color(&mut material,0,0,255,255);model_pose(&mut model,vec3::new(fix32::from_words(32768,0),fix32::zero(),fix32::from_int(-4)),zero,vec3::new(one,one,one));pass_draw(&mut p,&mut mesh,&mut model,&mut material);");
        if std::env::var_os("GPU_PROFILER_RTT").is_some() {
            app = app.replace("let zero=vec3::zero();", "let mut memory:RenderBufferMemory=RenderBufferMemory{color_base_low:0,color_base_high:0x60,color_capacity_low:0,color_capacity_high:4,depth_base_low:0,depth_base_high:0x64,depth_capacity_low:0,depth_capacity_high:4};let mut buffer=renderer_render_buffer(&mut r,view_of(&mut memory));let zero=vec3::zero();")
                .replace("renderer_frame(&mut r,0x0841+i)", "renderer_frame_to(&mut r,&mut buffer,0x0841+i)");
        }
    }
    let uart = system_frontend && std::env::var_os("GPU_PROFILER_UART").is_some();
    let native = system_frontend && std::env::var_os("GPU_PROFILER_NATIVE_COPY").is_some();
    let reserved = native && std::env::var("GPU_PROFILER_NATIVE_COPY").as_deref() == Ok("reserved");
    let gate = if uart {
        include_str!("../../rcc/profiler_gate.rs")
    } else if full_scene {
        include_str!("../fixtures/capture_full_scene/profiler_gate.rs")
    } else {
        GATE
    };
    std::fs::write(out.join("main.rs"), &app).unwrap();
    std::fs::write(out.join("profiler_gate.rs"), gate).unwrap();
    let mut fixture_args = vec![
        "systems/cpu-v3-tang-nano-20k/tools/capture_fixture.py",
        "--output",
        out.to_str().unwrap(),
        "--assets",
        assets.to_str().unwrap(),
        "--version",
        "2",
    ];
    let rtl = std::env::var_os("GPU_PROFILER_RTL");
    if native {
        fixture_args.push("--native-tail");
    }
    if reserved {
        fixture_args.push("--native-reserved");
    }
    if system_frontend {
        if let Some(rtl) = &rtl {
            fixture_args.extend(["--rtl", rtl.to_str().unwrap()]);
        }
    }
    call(&repo, &fixture_args);
    let catalog: Value =
        serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap()).unwrap();
    let mut config = String::from("format 2\ngpu-api\napplication main.rs\n");
    for r in catalog["resources"].as_array().unwrap() {
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {} {}\n",
            r["file"]
                .as_str()
                .unwrap()
                .trim_end_matches(".bin")
                .to_uppercase(),
            r["address"].as_u64().unwrap(),
            r["file"].as_str().unwrap()
        ));
    }
    let constants = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config)
        .unwrap()
        .constants()
        + &std::fs::read_to_string(assets.join("renderer_assets.rs")).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let selection = resolver::RendererCaptureConfiguration::from_manifest(
        out.join("profiler_gate.rs"),
        &out.join("build.json"),
    )
    .unwrap();
    let mut origins = vec![];
    let parsed = if full_scene {
        resolver::compile_explicit_gpu_program_checked(
            "actual public full-scene captures",
            &app,
            &options,
            &out,
            &root.join("rcc"),
            &constants,
            &selection,
            &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
            &mut |p| origins.push(std::fs::canonicalize(p).unwrap()),
        )
        .unwrap()
    } else {
        resolver::compile_renderer_program_with_capture_profile(
            "three DRAW captures",
            &app,
            &options,
            &out,
            &if native {
                out.join("fixture-rcc")
            } else {
                root.join("rcc")
            },
            &constants,
            &[],
            &selection,
            &out.join("profile-rcc"),
            &mut |p| origins.push(std::fs::canonicalize(p).unwrap()),
        )
        .unwrap()
    };
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    std::fs::write(
        out.join("producer.v3bin"),
        program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(out.join("producer.lst"), &program.listing).unwrap();
    if system_frontend {
        use cpu_v3_tang_nano_20k::{
            boot,
            profiling_capture::{capture_flash, CapturePoint},
        };
        let code: Vec<u8> = program.words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let resources: Vec<_> = catalog["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                boot::BootResource::copy(
                    r["address"].as_u64().unwrap() as usize,
                    std::fs::read(assets.join(r["file"].as_str().unwrap())).unwrap(),
                )
            })
            .collect();
        let package =
            boot::pack_application_resources(&code, boot::SINGLE_BOOT_STATIC_WORDS, &resources)
                .unwrap();
        std::fs::write(out.join("compiled-boot.bin"), &package).unwrap();
        let point = CapturePoint::from_program(&program).unwrap();
        if reserved {
            capture_uart::reject_dependency(&package, &program, point, &out);
            return out;
        }
        let frozen = capture_flash(&package, point, 0, 5_000_000).unwrap();
        assert!(
            matches!(capture_flash(&package, point, 0, 1), Err(message) if message.contains("incomplete"))
        );
        let raw = out.join("system-raw");
        std::fs::create_dir_all(&raw).unwrap();
        for (name, values) in [
            ("header.bin", &frozen.header),
            ("ranges.bin", &frozen.ranges),
        ] {
            std::fs::write(
                raw.join(name),
                values
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        for (index, data) in frozen.payloads.iter().enumerate() {
            std::fs::write(raw.join(format!("range-{index:03}.bin")), data).unwrap();
        }
        std::fs::write(raw.join("origin.json"), serde_json::to_vec_pretty(&json!({"schema":1,"source":"system-simulation","boundary":"before-profiler-gate","gpu_consumed":false,"range_count":frozen.payloads.len(),"cpu_steps":frozen.cpu_steps,"vblank_events":frozen.vblank_events,"memory_seed":0})).unwrap()).unwrap();
        std::fs::write(out.join("capture-point.json"), serde_json::to_vec_pretty(&json!({"gate_pc":point.gate_pc,"header_word":point.header_word,"ranges_word":point.ranges_word})).unwrap()).unwrap();
        assert_eq!(
            frozen.header[abi::GPU_CAPTURE_H_COMMAND_ABI as usize],
            abi::GPU_CAPTURE_RTT400_COMMAND_ABI
        );
        assert!(!frozen.payloads.is_empty());
        if uart {
            capture_uart::exercise(&package, &program, point, &frozen, &out);
        }
        return out;
    }
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for at in 0x400000usize / 2..0x4e0000 / 2 {
        cpu.physical_memory_mut()[at] = ((at as u32 * 17) ^ (at as u32 >> 8)) as u16;
    }
    for r in catalog["resources"].as_array().unwrap() {
        let at = r["address"].as_u64().unwrap() as usize / 2;
        let data = words(&std::fs::read(assets.join(r["file"].as_str().unwrap())).unwrap());
        cpu.physical_memory_mut()[at..at + data.len()].copy_from_slice(&data);
    }
    let state = Rc::new(RefCell::new(State {
        active: 0x200000,
        full_scene,
        ..Default::default()
    }));
    for kind in [0, 3, 4, 5, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                state: state.clone(),
            }),
        );
    }
    let texture = catalog["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "texture")
        .unwrap()["address"]
        .as_u64()
        .unwrap() as u32;
    let mut headers = vec![];
    let mut ranges = vec![];
    let mut first = vec![];
    let mut last = vec![];
    let mut frames = vec![];
    let mut signal = 0;
    let mut steps = 0;
    for step in 0..5_000_000 {
        steps = step + 1;
        state.borrow_mut().cycles += 1;
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) => match e.signal_type {
                2 => headers.push(e.value),
                3 => ranges.push(e.value),
                4 => first.push(e.value),
                5 => last.push(e.value),
                6 => {
                    let frame = frames.len();
                    {
                        let mut s = state.borrow_mut();
                        let publication = s.publications;
                        s.gate_publications.push(publication);
                    }
                    assert!(frame < 3);
                    assert_eq!(e.value, 0);
                    assert_eq!(
                        (
                            state.borrow().counts,
                            state.borrow().starts,
                            state.borrow().swaps
                        ),
                        (frame, frame, frame)
                    );
                    assert!(!state.borrow().pending);
                    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
                    assert_eq!(headers.len(), abi::GPU_CAPTURE_WORDS as usize);
                    assert_eq!(
                        headers[abi::GPU_CAPTURE_H_LAYOUT_ABI as usize],
                        abi::GPU_CAPTURE_LAYOUT_COLUMN_ABI
                    );
                    assert_eq!(
                        headers[abi::GPU_CAPTURE_H_COMMAND_ABI as usize],
                        abi::GPU_CAPTURE_RTT400_COMMAND_ABI
                    );
                    assert_eq!(
                        ranges.len(),
                        usize::from(headers[abi::GPU_CAPTURE_H_RANGE_COUNT as usize]) * 8
                    );
                    assert_eq!(first.len(), ranges.len() / 8);
                    assert_eq!(last.len(), first.len());
                    let dir = out.join(format!("frame-{frame}"));
                    std::fs::create_dir_all(&dir).unwrap();
                    let mut range_meta = vec![];
                    for (i, r) in ranges.as_chunks::<8>().0.iter().enumerate() {
                        let base = pair(r, abi::GPU_CAPTURE_R_BASE);
                        let size = pair(r, abi::GPU_CAPTURE_R_BYTES);
                        if [abi::GPU_CAPTURE_TAG_COMMANDS, abi::GPU_CAPTURE_TAG_OBJECTS]
                            .contains(&r[1])
                        {
                            assert_eq!(
                                pair(r, abi::GPU_CAPTURE_R_GENERATION),
                                pair(&headers, abi::GPU_CAPTURE_H_TOKEN)
                            );
                        }
                        let payload = bytes(&mut cpu, base, size);
                        let ws = words(&payload);
                        assert_eq!(first[i], ws[0]);
                        assert_eq!(last[i], *ws.last().unwrap());
                        if [
                            abi::GPU_CAPTURE_TAG_COLOR,
                            abi::GPU_CAPTURE_TAG_DEPTH,
                            abi::GPU_CAPTURE_TAG_FRONT,
                        ]
                        .contains(&r[1])
                        {
                            if full_scene
                                && r[0] == (abi::GPU_CAPTURE_READ | abi::GPU_CAPTURE_PREIMAGE)
                            {
                                assert_eq!(size, 57344, "read-only allocation tail");
                            } else {
                                assert_eq!(size, 204800, "physical target span");
                            }
                        }
                        std::fs::write(dir.join(format!("range-{i}.bin")), payload).unwrap();
                        range_meta.push(json!({"file":format!("range-{i}.bin"),"index":i,"flags":r[0],"tag":r[1],"base":base,"bytes":size,"generation":pair(r,6)}));
                    }
                    if full_scene {
                        // Every declared 256KiB allocation is covered exactly:
                        // 200KiB writable physical plane plus 56KiB read-only tail.
                        let rows = ranges.as_chunks::<8>().0;
                        for (tag, field) in [
                            (abi::GPU_CAPTURE_TAG_COLOR, abi::GPU_CAPTURE_H_COLOR_BASE),
                            (abi::GPU_CAPTURE_TAG_DEPTH, abi::GPU_CAPTURE_H_DEPTH_BASE),
                        ] {
                            let base = pair(&headers, field);
                            let selected = rows.iter().filter(|r| r[1] == tag).collect::<Vec<_>>();
                            assert_eq!(selected.len(), 2, "physical plane and tail required");
                            let physical = selected
                                .iter()
                                .find(|r| pair(r.as_slice(), abi::GPU_CAPTURE_R_BASE) == base)
                                .unwrap();
                            assert_eq!(pair(physical.as_slice(), abi::GPU_CAPTURE_R_BYTES), 204800);
                            assert_eq!(
                                physical[0],
                                abi::GPU_CAPTURE_WRITE
                                    | abi::GPU_CAPTURE_PREIMAGE
                                    | abi::GPU_CAPTURE_FULL_CLEAR
                            );
                            let tail = selected
                                .iter()
                                .find(|r| {
                                    pair(r.as_slice(), abi::GPU_CAPTURE_R_BASE) == base + 204800
                                })
                                .unwrap();
                            assert_eq!(pair(tail.as_slice(), abi::GPU_CAPTURE_R_BYTES), 57344);
                            assert_eq!(tail[0], abi::GPU_CAPTURE_READ | abi::GPU_CAPTURE_PREIMAGE);
                            assert_eq!(
                                selected
                                    .iter()
                                    .map(|r| pair(r.as_slice(), abi::GPU_CAPTURE_R_BYTES))
                                    .sum::<u32>(),
                                262144
                            );
                        }
                    }
                    let cb_base = pair(&headers, abi::GPU_CAPTURE_H_COMMAND_BASE);
                    let cb_len = pair(&headers, abi::GPU_CAPTURE_H_COMMAND_BYTES);
                    let object_base = pair(&headers, abi::GPU_CAPTURE_H_OBJECT_BASE);
                    let object_len = pair(&headers, abi::GPU_CAPTURE_H_OBJECT_BYTES);
                    let command_bytes = bytes(&mut cpu, cb_base, cb_len);
                    let object_bytes = bytes(&mut cpu, object_base, object_len);
                    let ui_base = pair(&headers, abi::GPU_CAPTURE_H_UI_BASE);
                    let ui_bytes = bytes(
                        &mut cpu,
                        ui_base,
                        pair(&headers, abi::GPU_CAPTURE_H_UI_BYTES),
                    );
                    let observed = if full_scene {
                        let mesh_file = catalog["resources"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|r| r["kind"] == "mesh")
                            .unwrap()["file"]
                            .as_str()
                            .unwrap();
                        inspect_full_scene(
                            &command_bytes,
                            &object_bytes,
                            object_base,
                            &ui_bytes,
                            ui_base,
                            &catalog,
                            &std::fs::read(assets.join(mesh_file)).unwrap(),
                        )
                    } else {
                        inspect_draw(&command_bytes, &object_bytes, object_base, frame, texture)
                    };
                    assert_eq!(
                        observed["command_count"].as_u64().unwrap(),
                        u64::from(headers[abi::GPU_CAPTURE_H_COUNT as usize]) + 1
                    );
                    std::fs::write(dir.join("commands.bin"), command_bytes).unwrap();
                    std::fs::write(dir.join("objects.bin"), object_bytes).unwrap();
                    if full_scene {
                        std::fs::write(dir.join("ui.bin"), &ui_bytes).unwrap();
                    }
                    std::fs::write(
                        dir.join("header.bin"),
                        headers
                            .iter()
                            .flat_map(|w| w.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                    std::fs::write(
                        dir.join("ranges.bin"),
                        ranges
                            .iter()
                            .flat_map(|w| w.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                    let clock = if full_scene {
                        global(&mut cpu, &program, "PROFILER_CLOCK", 2)
                    } else {
                        vec![]
                    };
                    if full_scene {
                        assert_eq!(
                            clock[1],
                            u16::from(frame == 2),
                            "one resumed gate changes the ordinary clock epoch"
                        );
                        let glyphs = observed["ui_glyphs"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_u64().unwrap() as u8)
                            .collect::<Vec<_>>();
                        if frame == 0 || frame == 2 {
                            assert_eq!(
                                &glyphs[..5],
                                b"65535",
                                "unavailable observation after startup/resume"
                            );
                        } else {
                            assert_ne!(
                                &glyphs[..5],
                                b"65535",
                                "completed observation remains available"
                            );
                        }
                    }
                    let info = json!({"status":"pass","frame_index":frame,"header_words":headers,"range_words":ranges,"ranges":range_meta,"cb_address":cb_base,
                        "token":pair(&headers,abi::GPU_CAPTURE_H_TOKEN),"publication":u32::from(headers[11])|(u32::from(headers[abi::GPU_CAPTURE_H_PUBLICATION_HIGH as usize])<<16),"slot":headers[abi::GPU_CAPTURE_H_SLOT as usize],"observed":observed,
                        "profiler_clock_words":clock,"gate_result":if full_scene && frame==1 {1}else{0},
                        "origins":origins,"build_manifest":"../build.json","catalog":"../assets/assets.json","boundary":"Actual RCC/CPU Prepared gate. Public read bridges sample first/last words; host copies full frozen ranges. GPU/Display peers only, no rasterization/scanout/UART or board proof."});
                    std::fs::write(
                        dir.join("producer.json"),
                        serde_json::to_vec_pretty(&info).unwrap(),
                    )
                    .unwrap();
                    frames.push(info);
                    headers.clear();
                    ranges.clear();
                    first.clear();
                    last.clear();
                    if full_scene && frame == 1 {
                        // Advance wall time while the CPU is inside the real gate; no instruction runs.
                        state.borrow_mut().cycles += 40_000 * 54_000;
                    }
                }
                _ => {}
            },
            StepOutcome::Halted { signal: s } => {
                signal = s;
                break;
            }
            _ => {}
        }
    }
    assert_eq!(signal, 0x600d, "watchdog/fault after {steps} steps");
    assert_eq!(frames.len(), 3);
    assert_eq!(
        (
            state.borrow().counts,
            state.borrow().starts,
            state.borrow().swaps
        ),
        (3, 3, 3)
    );
    assert_eq!(global(&mut cpu, &program, "GPU_CAPTURE_CONTROL", 13)[0], 0);
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    assert_eq!(frames[0]["cb_address"], frames[2]["cb_address"]);
    if full_scene {
        assert_eq!(frames[0]["cb_address"], frames[1]["cb_address"]);
    } else {
        assert_ne!(frames[0]["cb_address"], frames[1]["cb_address"]);
    }
    assert_eq!(frames[0]["slot"], frames[2]["slot"]);
    if full_scene {
        assert_eq!(frames[0]["slot"], frames[1]["slot"]);
    } else {
        assert_ne!(frames[0]["slot"], frames[1]["slot"]);
    }
    assert_eq!(
        frames
            .iter()
            .map(|f| f["token"].as_u64().unwrap())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    if full_scene {
        // Zero means the legacy diagnostic counter is unavailable in source12.
        // Observe the real publication commands instead of inventing an epoch.
        assert!(frames.iter().all(|f| f["publication"] == 0));
        assert_eq!(state.borrow().gate_publications, [1, 2, 4]);
        assert_eq!(state.borrow().start_publications, [1, 3, 4]);
    } else {
        assert_eq!(
            frames
                .iter()
                .map(|f| f["publication"].as_u64().unwrap())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3
        );
    }
    if full_scene {
        assert_eq!(
            frames[0]["observed"]["ui_glyphs"],
            frames[2]["observed"]["ui_glyphs"]
        );
        assert!(state.borrow().timer_clears >= 3);
        let clock = global(&mut cpu, &program, "PROFILER_CLOCK", 2);
        assert_eq!(clock[1], 1);
        let raw_ms = (state.borrow().cycles / 54000) as u16;
        assert!(
            raw_ms.wrapping_sub(clock[0]) < 1000,
            "ordinary active clock excludes the 40000ms pause"
        );
    } else {
        assert_ne!(
            std::fs::read(out.join("frame-0/objects.bin")).unwrap(),
            std::fs::read(out.join("frame-2/objects.bin")).unwrap()
        );
    }
    for r in catalog["resources"].as_array().unwrap() {
        assert_eq!(
            bytes(
                &mut cpu,
                r["address"].as_u64().unwrap() as u32,
                r["bytes"].as_u64().unwrap() as u32
            ),
            std::fs::read(assets.join(r["file"].as_str().unwrap())).unwrap()
        );
    }
    std::fs::write(out.join("sequence.json"),serde_json::to_vec_pretty(&json!({"status":"pass","profile":profile,"full_scene":full_scene,"frames":frames,"cpu_steps":steps,"bound":5000000,"signal":signal,"counts":3,"starts":3,"presents":3,"cb_sequence":if full_scene {"A/A/A"} else {"A/B/A"},"pause_ms":if full_scene{40000}else{0},"final_profiler_clock_words":if full_scene {global(&mut cpu,&program,"PROFILER_CLOCK",2)} else {vec![]},"input_peer":"application-owned input; no physical serial qualification","clock_peer":"one modeled 54MHz cycle per CPU instruction, not CPU performance evidence","timer_clears":state.borrow().timer_clears,"boundary":"Independent frozen inputs; peers never render. GPU owner must cold replay and independently verify each DRAW, including address reuse."})).unwrap()).unwrap();
    out
}
#[test]
fn real_draw_three_gates_reuse_slots_with_distinct_inputs() {
    run("column16", false, false);
}
#[test]
fn real_full_scene_three_gates_resume_and_restore_stats() {
    run("column16", true, false);
}

#[test]
fn compiled_program_system_frontend_stops_before_actual_gate() {
    run(
        "column16",
        std::env::var_os("GPU_PROFILER_FULL_SCENE").is_some(),
        true,
    );
}
