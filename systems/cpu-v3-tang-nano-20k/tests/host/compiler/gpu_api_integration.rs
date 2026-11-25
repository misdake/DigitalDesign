//! Actual approved A facade and fixed C implementation. Device protocol only;
//! production GPU pixel execution has a separate bounded qualification.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use std::{any::Any, cell::RefCell, rc::Rc};

pub(super) const APPLICATION: &str = r#"mod gpu_api;use gpu_api::*;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let b:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0xfc00,512)},push:unsafe{GpuMemory::from_sectors(0xfe00,512)}};
 let mut gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:Some(b)});
 let mut first:CommandBuffer=gpu.command_buffer();let mut second:CommandBuffer=gpu.command_buffer();
 let color:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth:DepthBuffer=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 {let pass=first.pass(color.ptr(), depth.ptr(), ColorLoad::clear(Rgb565::from_raw(0x1357)), DepthLoad::load());}
 {let pass=second.pass(color.ptr(), depth.ptr(), ColorLoad::load(), DepthLoad::clear(0x2468));}
 gpu.submit(first);first=gpu.wait();let sealed=first.command_words();
 let absent:Option<CommandBuffer>=gpu.try_wait();if let Some(unexpected)=absent{drop(unexpected);halt(0xbad);}
 gpu.submit(second);second=gpu.wait();gpu.submit(first);first=gpu.wait();
 check(first.command_words()==sealed);first.reset();check(first.command_words()==0&&first.push_words()==0);
 drop(first);drop(second);drop(color);drop(depth);drop(gpu);halt(0x600d);
}"#;

#[derive(Default)]
pub(super) struct Trace {
    events: Vec<(u8, u8, u16)>,
    busy: u8,
    count: u16,
    images: Vec<Vec<u16>>,
    pub(super) rtl: Option<std::path::PathBuf>,
}
pub(super) struct Peer(pub(super) u8, pub(super) Rc<RefCell<Trace>>);
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        if self.0 == 4 {
            assert_eq!(channel, 2);
            let mut state = self.1.borrow_mut();
            if state.busy != 0 {
                state.busy -= 1;
                return 1;
            }
        }
        0
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut state = self.1.borrow_mut();
        if (self.0 == 0 && channel == 1) || self.0 == 4 {
            state.events.push((self.0, channel, value));
        }
        if self.0 == 4 && channel == 0 {
            assert_eq!(state.busy, 0);
            state.count = value;
        }
        if self.0 == 4 && channel == 1 {
            assert_eq!(state.busy, 0);
            state.busy = 3;
            let base = usize::from(value) * 64;
            let padded = if state.count & 0x8000 != 0 {
                64
            } else {
                let words = (usize::from(state.count) + 1) * 2;
                (words + 63) & !63
            };
            state.images.push(memory[base..base + padded].to_vec());
            if let Some(directory) = &state.rtl {
                super::gpu_api_pixels::execute(directory, memory, value, state.count);
            }
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn prepared_command_marks_select_only_the_requested_draw() {
    use gpu_v2::system::vliw_command::{CommandSlice, DeviceSubmission};

    let application = r#"mod gpu_api;use gpu_api::*;
fn main(){
 let a=CommandMemory::new(unsafe{GpuMemory::from_sectors(0x9800,8)},unsafe{GpuMemory::from_sectors(0x9820,8)});
 let b=CommandMemory::new(unsafe{GpuMemory::from_sectors(0xba00,8)},unsafe{GpuMemory::from_sectors(0xba20,8)});
 let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:Some(b)});
 let commands=unsafe{GpuMemory::from_sectors(0xb800,8)};
 let push=unsafe{GpuMemory::from_sectors(0xb900,32)};
 let mut prepared=PreparedDraws::new(PreparedMemory::new(commands,push));
 let model=Transform::new(Pose::identity());
 let material=Material::new(MaterialDesc{uniform_color:Rgba8::white(),specular:Rgb8{r:0,g:0,b:0},ambient_response:fix!(1),directional_response:fix!(1),shininess:Shininess::S16,unlit:true});
 let light=Lighting{direction:Vec3::new(fix!(0),fix!(0),fix!(1)),ambient:fix!(0.25),directional:fix!(0.5)};
 let state=RenderState::new(RenderStateDesc{depth:DepthFunc::Less,depth_write:true,blend:BlendMode::Replace,filter:TextureFilter::Nearest,lighting:light});
 let mesh=Mesh::from_native(MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:unsafe{GpuMemory::from_sectors(0xa000,16)}});
 let camera=Camera::new(CameraDesc::new(Vec3::zero(),Vec3::zero(),Projection::from_focal_pixels(fix!(200),fix!(199))));
 let color=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let mut part=prepared.record();
 part.set_state(state.ptr());
 let start=part.mark_slice();
 if start!=0 {halt(0xbad);}
 part.draw_colored(model.ptr(),mesh.ptr(),material.ptr());
 let first=part.mark_slice();
 part.draw_colored(model.ptr(),mesh.ptr(),material.ptr());
 let end=part.mark_slice();
 part.draw_colored(model.ptr(),mesh.ptr(),material.ptr());
 let total=part.mark_slice();
 part.finish();
 let draws=prepared.ptr();
 signal(7,first);signal(8,end);signal(9,total);
 let mut setup=gpu.command_buffer();
 {
  let mut pass=setup.pass(color.ptr(),depth.ptr(),ColorLoad::load(),DepthLoad::load());
  pass.set_camera(camera.ptr());pass.set_state(state.ptr());
  pass.draw_colored(model.ptr(),mesh.ptr(),material.ptr());
 }
 let mut tail=gpu.command_buffer();
 {let pass=tail.pass(color.ptr(),depth.ptr(),ColorLoad::load(),DepthLoad::load());}
 let mut list=CommandList::new(4);
 list.push(setup);
 list.push_prepared_range(prepared.ptr(),first,end);
 list.push_prepared_range(prepared.ptr(),start,first);
 list.push(tail);
 RETIRE
 draws.invalidate();
 halt(0x600d);
}"#;
    let run = "gpu.submit_list(list);if gpu.try_wait_list(){halt(0xbad);}gpu.wait_list();";
    let selection = "list.push_prepared_range(prepared.ptr(),first,end);";
    let lighting = application
        .replace("unlit:true", "unlit:false")
        .replace(
            "let light=",
            "let strong=Material::new(MaterialDesc::diffuse(Rgba8::white()).with_response(fix!(2),fix!(1)));let light=",
        )
        .replace("let state=RenderState", "let mut state=RenderState")
        .replacen(
            "part.draw_colored(model.ptr(),mesh.ptr(),material.ptr());",
            "part.draw_colored(model.ptr(),mesh.ptr(),strong.ptr());",
            1,
        )
        .replace(
            "let draws=prepared.ptr();",
            "let draws=prepared.ptr();state.set_lighting(Lighting::directional(Vec3::new(fix!(0),fix!(0),fix!(1)),fix!(1),fix!(0.5)));",
        )
        .replace("CommandList::new(4)", "CommandList::new(3)")
        .replace(" list.push_prepared_range(prepared.ptr(),start,first);\n", "");
    let cases = [
        ("valid", application.to_owned(), run.to_owned(), 0x600d, 1),
        (
            "lighting-outside-range",
            lighting.clone(),
            run.into(),
            0x600d,
            1,
        ),
        (
            "lighting-inside-range",
            lighting.replace(
                selection,
                "list.push_prepared_range(prepared.ptr(),start,end);",
            ),
            run.into(),
            0xe001,
            0,
        ),
        (
            "abandoned",
            application.to_owned(),
            "drop(list);".into(),
            0x600d,
            0,
        ),
        (
            "padding",
            application.replace(
                selection,
                "list.push_prepared_range(prepared.ptr(),total,total+1);",
            ),
            run.into(),
            0xe001,
            0,
        ),
        (
            "unmarked-start",
            application.replace(
                selection,
                "list.push_prepared_range(prepared.ptr(),first+1,end);",
            ),
            run.into(),
            0xe001,
            0,
        ),
        (
            "unmarked-end",
            application.replace(
                selection,
                "list.push_prepared_range(prepared.ptr(),first,end-1);",
            ),
            run.into(),
            0xe001,
            0,
        ),
        (
            "unmarked-final-end",
            application
                .replace("let total=part.mark_slice();", "let total=end+(end-first);")
                .replace(
                    selection,
                    "list.push_prepared_range(prepared.ptr(),end,total);",
                ),
            run.into(),
            0xe001,
            0,
        ),
        (
            "reversed-range",
            application.replace(
                selection,
                "list.push_prepared_range(prepared.ptr(),end,first);",
            ),
            run.into(),
            0xe001,
            0,
        ),
        (
            "missing-scene",
            application.replace(
                "pass.draw_colored(model.ptr(),mesh.ptr(),material.ptr());",
                "",
            ),
            run.into(),
            0xe001,
            0,
        ),
        (
            "missing-terminal",
            application.replace("CommandList::new(4)", "CommandList::new(2)"),
            run.into(),
            0xe001,
            0,
        ),
        (
            "building-list-pin",
            application.to_owned(),
            "draws.invalidate();drop(list);".into(),
            0xe001,
            0,
        ),
        (
            "running-list-pin",
            application.to_owned(),
            "gpu.submit_list(list);draws.invalidate();gpu.wait_list();".into(),
            0xe001,
            1,
        ),
    ];
    for (name, source, retire, expected_halt, submissions) in cases {
        let source = source.replace("RETIRE", &retire);
        let debug = name == "valid" || name.starts_with("lighting-");
        let ir = super::gpu_api_resources::parse(&source, name, debug).unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
        let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
        let trace = Rc::new(RefCell::new(Trace::default()));
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.physical_memory_mut()[0xa000 * 64..][..2].copy_from_slice(&[0xa001, 1]);
        for kind in [0, 4] {
            cpu.attach_device(kind, Box::new(Peer(kind, trace.clone())));
        }
        let mut marks = [0usize; 3];
        let halt = (0..1_000_000).find_map(|_| match cpu.step().unwrap() {
            StepOutcome::Signaled(event) if (7..=9).contains(&event.signal_type) => {
                marks[usize::from(event.signal_type - 7)] = usize::from(event.value);
                None
            }
            StepOutcome::Halted { signal } => Some(signal),
            _ => None,
        });
        let [first, end, total] = marks;
        assert!(first > 0 && first < end && end < total, "{name}");
        let trace = trace.borrow();
        assert_eq!(halt, Some(expected_halt), "{name}");
        assert_eq!(trace.images.len(), submissions, "{name}");
        if name != "valid" {
            continue;
        }
        assert_eq!(trace.count, 0x8003);
        let selected = CommandSlice::new(
            0xb800 * 128 + (first * 4) as u32,
            ((end - first) * 4) as u32,
        )
        .unwrap();
        let head = CommandSlice::new(0xb800 * 128, (first * 4) as u32).unwrap();
        let (_, expected) = DeviceSubmission::segment_table(0, &[selected, head]).unwrap();
        let actual = trace.images[0][58..62]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        assert_eq!(actual, &expected[120..]);
        let commands = &cpu.physical_memory_mut()[0xb800 * 64..][first * 2..end * 2];
        assert_eq!(
            commands
                .chunks_exact(2)
                .filter(|command| command[1] >> 12 == 3)
                .count(),
            1
        );
    }
}

#[test]
fn actual_a_c_public_submit_wait_and_interleaved_replay() {
    for debug in [false, true] {
        let parsed = super::gpu_api_resources::parse(APPLICATION, "actual-a-c-replay", debug)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            parsed.debug.static_data_end <= 8192,
            "static prefix exceeds 16 KiB"
        );
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        {
            let trace = Rc::new(RefCell::new(Trace::default()));
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&program.words);
            for kind in [0, 1, 4] {
                cpu.attach_device(kind, Box::new(Peer(kind, trace.clone())));
            }
            let mut halted = false;
            for _ in 0..3_000_000 {
                if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                    assert_eq!(signal, 0x600d);
                    halted = true;
                    break;
                }
            }
            assert!(halted, "bounded public A/C replay timeout");
            let trace = trace.borrow();
            assert_eq!(trace.images.len(), 3);
            assert_eq!(
                trace.images[0], trace.images[2],
                "replay changed native stream"
            );
            assert_ne!(trace.images[0], trace.images[1]);
            let starts: Vec<_> = trace
                .events
                .iter()
                .filter(|event| event.0 == 4 && event.1 == 1)
                .map(|event| event.2)
                .collect();
            assert_eq!(starts, [0x9800, 0xfc00, 0x9800]);
            let order: Vec<_> = trace
                .events
                .iter()
                .map(|event| {
                    if event.0 == 0 {
                        'P'
                    } else if event.1 == 0 {
                        'C'
                    } else {
                        'S'
                    }
                })
                .collect();
            assert_eq!(order, ['P', 'C', 'S', 'P', 'C', 'S', 'P', 'C', 'S']);
        }
    }
}
