//! Target-only retargeting against fresh native bytes and strict owner guards.
use super::{compile, global};
use cpu_v3::{CpuV3Sim, Device, StepOutcome};
use std::{any::Any, cell::RefCell, rc::Rc};
const PREFIX: &str = r#"mod gpu_api;use gpu_api::*;
fn v(x:i16,y:i16,z:i16)->Vec3{Vec3{x:fix32::from_int(x),y:fix32::from_int(y),z:fix32::from_int(z)}}
fn main(){
 let first:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let second:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9c00,512)},push:unsafe{GpuMemory::from_sectors(0x9e00,512)}};
 let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:first,command_b:Some(second)});
 let mut a=gpu.command_buffer();let mut b=gpu.command_buffer();let mut display=Display::open();
 let cam=Camera::new(CameraDesc{eye:v(0,0,0),angles:v(0,0,0),projection:Projection::from_focal_pixels(fix32::from_int(200),fix32::from_int(120))});
 let model=Model::new(Transform{position:v(0,0,-3),angles:v(0,0,0),scale:v(1,1,1)});
 let model2=Model::new(Transform{position:v(1,0,-4),angles:v(0,0,0),scale:v(1,1,1)});
 let material=Material::new(MaterialDesc{uniform_color:Rgba8{r:80,g:160,b:240,a:255},specular:Rgb8{r:10,g:20,b:30},shininess:Shininess::S32,ambient_response:fix32::from_words(32768,0),directional_response:fix32::from_words(16384,0),unlit:false});
 let material2=Material::new(MaterialDesc{uniform_color:Rgba8{r:240,g:80,b:160,a:255},specular:Rgb8{r:10,g:20,b:30},shininess:Shininess::S32,ambient_response:fix32::from_words(32768,0),directional_response:fix32::from_words(16384,0),unlit:false});
 let state=RenderState::new(RenderStateDesc{depth:DepthFunc::Less,depth_write:true,blend:BlendMode::Replace,filter:TextureFilter::Nearest,lighting:Lighting{direction:v(0,0,1),ambient:fix32::from_int(1),directional:fix32::from_int(1)}});
 let color_a=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let color_b=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,2048)});
 let background=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0xb000,2048)});
 let asset_alias=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0xa000,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let depth_alias=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,2048)});
 let mesh=Mesh::from_native(MeshDesc{directory_sector:0xa000,directory_word:0,meshlet_count:1,storage:unsafe{GpuMemory::from_sectors(0xa000,16)}});
"#;
fn source(copy: bool) -> String {
    let record = |buffer: &str, target: &str| {
        format!(
            "{}{{let mut p={buffer}.pass(&{target},&depth,ColorLoad::load(),DepthLoad::clear(65535));p.set_camera(&cam);p.set_state(&state);p.draw_colored(&mesh,&model,&material);p.draw_colored(&mesh,&model2,&material2);}}",
            if copy {
                format!("{buffer}.copy_color(&background,&{target});")
            } else {
                String::new()
            }
        )
    };
    format!(
        "{PREFIX}{} let binding=a.target_binding();dev_send(7,1,0);gpu.submit(a);a=gpu.wait();let choice=dev_recv(7,2);dev_send(7,1,9);\nif choice==1 {{a.retarget_target(binding,&background,&depth);}}else if choice==2 {{a.retarget_target(binding,&asset_alias,&depth);}}else if choice==3 {{a.retarget_target(binding,&color_b,&depth_alias);}}else{{a.retarget_target(binding,&color_b,&depth);}}\ndev_send(7,1,1);gpu.submit(a);a=gpu.wait();{} let fresh=b.target_binding();dev_send(7,1,2);\ndrop(model);drop(model2);drop(material);drop(material2);display.set_next_swap(&color_b);display.wait_vblank();a.retarget_target(binding,&color_a,&depth);dev_send(7,1,3);drop(a);drop(b);halt(0x600d);}}",
        record("a", "color_a"),
        record("b", "color_b")
    )
}
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    code: Vec<u16>,
    push: Vec<u16>,
    facts: Vec<u16>,
    control: Vec<u16>,
}
#[derive(Clone, Copy, Debug)]
enum Fault {
    None,
    Generation,
    Span,
    CopySpan,
    CopyDestination,
    CopyCount,
    Owner(u16),
    SourceAlias,
    AssetAlias,
    SamePlanes,
    Front,
    Pending,
    Transport,
}
struct Seen {
    snapshots: Vec<(u16, Snapshot)>,
    starts: usize,
    busy: u16,
    front: u16,
    staged: u16,
    pending: u16,
    patch: bool,
    retired: bool,
}
impl Default for Seen {
    fn default() -> Self {
        Self {
            snapshots: vec![],
            starts: 0,
            busy: 0,
            front: 0xc000,
            staged: 0xc000,
            pending: 0,
            patch: false,
            retired: false,
        }
    }
}
struct Peer {
    kind: u8,
    records: usize,
    facts: usize,
    execution: usize,
    present: usize,
    seen: Rc<RefCell<Seen>>,
    fault: Fault,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.seen.borrow_mut();
        if self.kind == 4 {
            if s.busy != 0 {
                s.busy -= 1;
                return 1;
            }
            return 0;
        }
        if self.kind == 3 {
            return match channel {
                1 => s.staged << 6,
                2 => s.staged >> 10,
                3 if s.pending != 0 => {
                    s.pending -= 1;
                    if s.pending == 0 {
                        s.front = s.staged;
                        s.retired = true;
                    }
                    1
                }
                _ => 0,
            };
        }
        if self.kind == 7 && channel == 2 {
            return match self.fault {
                Fault::SourceAlias => 1,
                Fault::AssetAlias => 2,
                Fault::SamePlanes => 3,
                Fault::Transport if s.patch => 1,
                _ => 0,
            };
        }
        0
    }
    fn write(&mut self, m: &mut [u16], channel: u8, value: u16) {
        let mut s = self.seen.borrow_mut();
        if self.kind == 4 {
            if channel == 1 {
                s.starts += 1;
                s.busy = 2;
            }
            return;
        }
        if self.kind == 3 {
            match channel {
                1 => s.staged = (s.staged & 0xfc00) | (value >> 6),
                2 => s.staged = (s.staged & 1023) | (value << 10),
                3 => {
                    assert_eq!(value, 1);
                    s.pending = 3;
                }
                _ => {}
            }
            return;
        }
        if self.kind != 7 {
            return;
        }
        assert_eq!(channel, 1);
        if value == 9 {
            s.patch = true;
            match self.fault {
                Fault::Generation => m[self.records + 20] ^= 2,
                Fault::Span => m[self.records + 22] = m[self.records + 6] + 1,
                Fault::CopySpan => m[self.records + 128] = m[self.records + 22],
                Fault::CopyDestination => m[self.records + 130] ^= 512,
                Fault::CopyCount => m[self.records + 131] = 2,
                Fault::Owner(phase) => {
                    m[self.execution] = phase;
                    m[self.execution + 1] = 1;
                }
                Fault::Front => {
                    s.front = 0x8800;
                    s.staged = 0x8800;
                }
                Fault::Pending => {
                    m[self.present + 1] = 1;
                    s.pending = 1000;
                }
                _ => {}
            }
        }
        let slot = usize::from(value == 2);
        let c = m[self.records + slot * 224..self.records + slot * 224 + 224].to_vec();
        let cb = usize::from(c[1]) * 64;
        let pb = usize::from(c[3]) * 64;
        s.snapshots.push((
            value,
            Snapshot {
                code: m[cb..cb + usize::from(c[7])].to_vec(),
                push: m[pb..pb + usize::from(c[6])].to_vec(),
                facts: m[self.facts + slot * 256..self.facts + slot * 256 + usize::from(c[15]) * 4]
                    .to_vec(),
                control: c,
            },
        ));
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(
    program: &cpu_v3::rcc_backend::CpuV3Program,
    fault: Fault,
) -> (u16, Rc<RefCell<Seen>>, CpuV3Sim) {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    cpu.physical_memory_mut()[0xa000 * 64..0xa000 * 64 + 2].copy_from_slice(&[0xa001, 1]);
    for kind in [0, 3, 4, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                records: global(program, "GPU_CB_RECORDS"),
                facts: global(program, "GPU_CB_CAPTURE_FACTS"),
                execution: global(program, "GPU_EXECUTION"),
                present: global(program, "GPU_PRESENT"),
                seen: seen.clone(),
                fault,
            }),
        );
    }
    for _ in 0..2_000_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            if signal != 0x600d {
                let at = global(program, "GPU_API_FAULT");
                eprintln!(
                    "fault={fault:?}, first_fault={:?}, snapshots={:?}",
                    &cpu.physical_memory_slice()[at..at + 6],
                    seen.borrow()
                        .snapshots
                        .iter()
                        .map(|x| x.0)
                        .collect::<Vec<_>>()
                );
            }
            if signal != 0x600d {
                let r = global(program, "GPU_CB_RECORDS");
                let f = global(program, "GPU_CB_CAPTURE_FACTS");
                eprintln!(
                    "record={:?}, facts={:?}",
                    &cpu.physical_memory_slice()[r..r + 32],
                    &cpu.physical_memory_slice()[f..f + 32]
                );
            }
            return (signal, seen, cpu);
        }
    }
    panic!("bounded2M target patch timeout: {fault:?}");
}
#[test]
fn target_only_multigroup_copy_matches_fresh_bytes_after_retirement() {
    for copy in [false, true] {
        let program = compile(&source(copy), false, &format!("target-copy-{copy}"));
        let (signal, seen, _) = run(&program, Fault::None);
        assert_eq!(signal, 0x600d);
        let s = seen.borrow();
        assert_eq!(s.starts, 2);
        assert!(s.retired);
        let get = |id| &s.snapshots.iter().find(|x| x.0 == id).unwrap().1;
        let old = get(0);
        let patched = get(1);
        let fresh = get(2);
        let restored = get(3);
        assert_eq!(old.code, patched.code);
        assert_eq!(old.code, restored.code);
        assert_eq!(
            patched.push, fresh.push,
            "all native payload bytes versus ordinary recording"
        );
        assert_eq!(
            patched.facts, fresh.facts,
            "capture facts retarget together"
        );
        assert_eq!(old.push, restored.push);
        assert_eq!(old.facts, restored.facts);
        assert_eq!(patched.control[9], 0x8800);
        assert_eq!(restored.control[9], 0x8000);
        if copy {
            assert_eq!(patched.control[129], 0xb000);
            assert_eq!(patched.control[130], 0x8800);
            assert_eq!(restored.control[130], 0x8000);
        }
    }
}
#[test]
fn target_only_rejections_preserve_all_command_payload_and_fact_bytes() {
    let program = compile(&source(true), false, "target-guards");
    for fault in [
        Fault::Generation,
        Fault::Span,
        Fault::CopySpan,
        Fault::CopyDestination,
        Fault::CopyCount,
        Fault::Owner(1),
        Fault::Owner(2),
        Fault::Owner(3),
        Fault::Owner(4),
        Fault::SourceAlias,
        Fault::AssetAlias,
        Fault::SamePlanes,
        Fault::Front,
        Fault::Pending,
    ] {
        let (signal, seen, cpu) = run(&program, fault);
        assert_ne!(signal, 0x600d, "{fault:?}");
        let s = seen.borrow();
        assert_eq!(s.starts, 1, "{fault:?}");
        assert!(!s.snapshots.iter().any(|x| x.0 == 1));
        let old = &s.snapshots.iter().find(|x| x.0 == 9).unwrap().1;
        let m = cpu.physical_memory_slice();
        let cb = usize::from(old.control[1]) * 64;
        let pb = usize::from(old.control[3]) * 64;
        assert_eq!(&m[cb..cb + old.code.len()], old.code, "{fault:?} commands");
        assert_eq!(&m[pb..pb + old.push.len()], old.push, "{fault:?} payload");
        let facts = global(&program, "GPU_CB_CAPTURE_FACTS");
        assert_eq!(
            &m[facts..facts + old.facts.len()],
            old.facts,
            "{fault:?} facts"
        );
    }
}
#[test]
fn target_only_copy_transport_failure_poison_blocks_submission() {
    let program = compile(&source(true), false, "storage-failure-target");
    let (signal, seen, cpu) = run(&program, Fault::Transport);
    assert_ne!(signal, 0x600d);
    let s = seen.borrow();
    assert_eq!(s.starts, 1);
    let m = cpu.physical_memory_slice();
    let r = global(&program, "GPU_CB_RECORDS");
    assert_eq!(m[r], 4, "partial I/O must poison the CB");
    let old = &s.snapshots.iter().find(|x| x.0 == 9).unwrap().1;
    let p = usize::from(old.control[3]) * 64;
    assert_ne!(
        &m[p..p + old.push.len()],
        old.push,
        "first write happened before injected second-write failure"
    );
    let f = global(&program, "GPU_CB_CAPTURE_FACTS");
    assert_eq!(&m[f..f + old.facts.len()], old.facts);
}
