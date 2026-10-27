//! Actual RCC execution. This peer observes control only; it does not render.
use super::shared_support_gpu_rcc_d2 as fixture;
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, RunOutcome};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};
fn compile(source: &str) -> Vec<u16> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("D2 builder", source, &options, &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        })
        .unwrap();
    rcc_backend::try_compile(parsed, &options, "main")
        .unwrap()
        .words
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<(usize, usize)>>,
}
impl cpu_v3::Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        if self.kind == 3 && channel == 2 {
            0x20
        } else {
            0
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, _: u16) {
        if self.kind == 0 && channel == 1 {
            self.state.borrow_mut().0 += 1;
        }
        if self.kind == 4 && channel == 1 {
            self.state.borrow_mut().1 += 1;
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(source: &str, assets: bool) -> (CpuV3Sim, usize) {
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&compile(source));
    if assets {
        let mut data = fixture::assets();
        // A directory spanning both a 128-B boundary and the DSEG page end.
        for i in 0..12 {
            data.insert(fixture::ASSET + 0x7ffc + i, data[&(fixture::ASSET + i)]);
        }
        for i in 0..5454u32 {
            for j in 0..4 {
                data.insert(
                    fixture::ASSET + 0x8000 + i * 4 + j,
                    data[&(fixture::ASSET + j)],
                );
            }
        }
        for (address, b) in data {
            let w = &mut cpu.physical_memory_mut()[address as usize / 2];
            *w = (*w & !(255 << ((address % 2) * 8))) | (u16::from(b) << ((address % 2) * 8));
        }
    }
    let count = Rc::new(RefCell::new((0, 0)));
    for kind in [0, 3, 4] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                state: count.clone(),
            }),
        );
    }
    let r = cpu.run(8_000_000).unwrap();
    assert!(
        matches!(r, RunOutcome::Halted { signal: 0x600d, .. }),
        "{r:?}"
    );
    let n = count.borrow().0;
    (cpu, n)
}
#[test]
fn compiled_builder_reserves_end_at_full_command_allocation() {
    let setup = fixture::program();
    let prefix = setup.split(" let mut iteration:u16=0;").next().unwrap();
    let source = format!(
        r#"{prefix}
 let cb=gpu_cb_begin();let pass=gpu_pass_begin(cb,0,65535);
 check(gpu_pass_set_global(pass,view_of(&mut g)));check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
 gpu_affine_identity(MATRIX.as_array());
 mesh.directory_sector=0x9700;mesh.directory_word=0;mesh.count=5455;mesh.sectors=512;
 check(!gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),0,0));
 mesh.count=5454;check(gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),65535,65535));
 check(GPU_CB.command_words==32758);
 mesh.count=1;check(!gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),0,0));
 check(GPU_CB.command_words==32758);
 check(gpu_pass_end(pass));check(gpu_cb_finish(cb));check(GPU_CB.command_words==32764);
 check(read_dseg_page(3)==19);halt(0x600d);
}}
"#
    );
    let (cpu, maintenance) = run(&source, true);
    let code = bytes(&cpu, 0x490000, 65536);
    assert_eq!(
        u32::from_le_bytes(code[65524..65528].try_into().unwrap()),
        0xf0000000
    );
    assert!(code[65528..].iter().all(|v| *v == 0));
    assert_eq!(
        code.as_chunks::<4>()
            .0
            .iter()
            .filter(|w| u32::from_le_bytes(**w) >> 28 == 3)
            .count(),
        5454
    );
    assert!(
        maintenance < 2000,
        "directory/CB batching regressed: {maintenance}"
    );
}
#[test]
fn compiled_directory_crossing_and_capacity_failures_preserve_prefix() {
    let setup = fixture::program();
    let prefix = setup.split(" let mut iteration:u16=0;").next().unwrap();
    let source = format!(
        r#"{prefix}
 let cb=gpu_cb_begin();let pass=gpu_pass_begin(cb,0,65535);
 check(gpu_pass_set_global(pass,view_of(&mut g)));check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
 gpu_affine_identity(MATRIX.as_array());
 mesh.directory_sector=0x96ff;mesh.directory_word=62;mesh.sectors=512;
 check(gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),65535,65535));
 let commands=GPU_CB.command_words;let objects=GPU_CB.object_words;
 let staged=GPU_BUILD.as_array()[2u16];
 // Capacity failures must neither flush/modify staging nor publish cursors.
 GPU_CB.command_words=32760;
 check(!gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),0,0));
 check(GPU_CB.command_words==32760 && GPU_CB.object_words==objects && GPU_BUILD.as_array()[2u16]==staged);
 GPU_CB.command_words=commands;GPU_CB.object_words=32740;
 check(!gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),0,0));
 check(GPU_CB.command_words==commands && GPU_CB.object_words==32740 && GPU_BUILD.as_array()[2u16]==staged);
 GPU_CB.object_words=objects;
 let mut bad:GpuMesh=GpuMesh{{directory_sector:0x9600,directory_word:4,count:4,first_sector:0x9600,sectors:512}};
 check(!gpu_pass_draw_mesh(pass,view_of(&mut bad),MATRIX.as_array(),0,0));
 check(GPU_CB.command_words==commands && GPU_CB.object_words==objects && GPU_BUILD.as_array()[2u16]==staged);
 mesh.directory_sector=65535;mesh.directory_word=63;mesh.count=2;mesh.first_sector=65535;mesh.sectors=1;
 check(!gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),0,0));
 check(gpu_pass_end(pass));check(gpu_cb_finish(cb));check(read_dseg_page(3)==19);
 halt(0x600d);
}}
"#
    );
    let (cpu, _) = run(&source, true);
    let code = bytes(&cpu, 0x490000, 256);
    assert_eq!(
        code.as_chunks::<4>()
            .0
            .iter()
            .filter(|w| u32::from_le_bytes(**w) >> 28 == 3)
            .count(),
        3
    );
}

#[test]
fn draw_between_ui_calls_restores_the_cache_publication_fence() {
    let setup = fixture::program();
    let prefix = setup.split(" let mut iteration:u16=0;").next().unwrap();
    let source = format!(
        r#"{prefix}
 let cb=gpu_cb_begin();let pass=gpu_pass_begin(cb,0,65535);
 check(gpu_pass_set_global(pass,view_of(&mut g)));
 check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
 gpu_affine_identity(MATRIX.as_array());
 let mut a:GpuUiAtlas=GpuUiAtlas{{sector:0x9800,sectors:512}};
 let mut cursor=GPU_CB.command_words;
 let ui=gpu_pass_ui_begin_range(pass,view_of(&mut a),1,1,2);check(ui==1);
 check(gpu_pass_ui_end(ui));check(GPU_CB.command_words==cursor+10);
 cursor=GPU_CB.command_words;
 let ui2=gpu_pass_ui_begin_range(pass,view_of(&mut a),2,2,3);check(ui2==1);
 check(gpu_pass_ui_end(ui2));check(GPU_CB.command_words==cursor+6);
 check(gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),65535,65535));
 cursor=GPU_CB.command_words;
 // The installed descriptor survives a 3D draw; only the fence is needed.
 check(gpu_pass_draw_ui_range(pass,2,3));check(GPU_CB.command_words==cursor+6);
 check(gpu_pass_end(pass));check(gpu_cb_finish(cb));halt(0x600d);
}}"#
    );
    let (cpu, _) = run(&source, true);
    let code = bytes(&cpu, 0x490000, 256);
    let commands: Vec<u32> = code
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .take_while(|w| *w != 0xf0000000)
        .collect();
    let ui: Vec<usize> = commands
        .iter()
        .enumerate()
        .filter_map(|(i, w)| (*w >> 28 == 11).then_some(i))
        .collect();
    assert_eq!(ui.len(), 3);
    assert_eq!(&commands[ui[0] - 4..ui[0] - 2], &[0x40000003, 0x70000000]);
    assert_eq!(ui[1] - ui[0], 3);
    assert_eq!(&commands[ui[2] - 2..ui[2]], &[0x40000003, 0x70000000]);
}
fn bytes(cpu: &CpuV3Sim, address: u32, count: usize) -> Vec<u8> {
    (0..count / 2)
        .flat_map(|i| {
            cpu.physical_memory(PhysicalWordAddress::new(address / 2 + i as u32))
                .to_le_bytes()
        })
        .collect()
}
#[test]
fn compiled_builder_records_ordered_draws_and_snapshots() {
    let (cpu, maintenance) = run(&fixture::program(), true);
    assert_eq!(cpu.data_segments(), [0, 1, 2, 19]);
    assert!(fixture::surface(1).iter().any(|v| *v != 1));
    let code = bytes(&cpu, 0x490000, 512);
    assert_eq!(&code[..384], &fixture::expected_commands(16368));
    let commands: Vec<u32> = code
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .take_while(|w| *w != 0xf0000000)
        .collect();
    assert!(commands.len() > 32);
    assert_eq!(commands.iter().filter(|w| **w >> 28 == 3).count(), 9);
    assert_eq!(commands.iter().filter(|w| **w >> 28 == 5).count(), 2);
    assert_eq!(commands.iter().filter(|w| **w >> 28 == 6).count(), 3);
    assert_eq!(commands.iter().filter(|w| **w >> 28 == 9).count(), 0);
    let state_sources = |block: u32| -> Vec<u32> {
        commands
            .iter()
            .filter(|w| **w >> 28 == 1 && (**w >> 3) & 127 == block)
            .map(|w| ((w >> 10) & 0x3ffff) * 32)
            .collect()
    };
    let global_sources = state_sources(66);
    assert_eq!(bytes(&cpu, global_sources[0] + 22, 2), 1u16.to_le_bytes());
    assert_eq!(bytes(&cpu, global_sources[1] + 22, 2), 2u16.to_le_bytes());
    for (source, flags) in state_sources(68).into_iter().zip([47u16, 15, 55]) {
        assert_eq!(bytes(&cpu, source + 8, 2), flags.to_le_bytes());
        assert_eq!(bytes(&cpu, source, 2), 0x9620u16.to_le_bytes());
    }
    let instances: Vec<u32> = commands
        .iter()
        .filter(|w| **w >> 28 == 1 && (**w >> 3) & 127 == 80)
        .map(|w| ((w >> 10) & 0x3ffff) * 32)
        .collect();
    assert_eq!(instances.len(), 4);
    let first = bytes(&cpu, instances[0], 128);
    let second = bytes(&cpu, instances[1], 128);
    assert_eq!(
        i32::from_le_bytes(first[12..16].try_into().unwrap()),
        -32768
    );
    assert_eq!(i32::from_le_bytes(second[12..16].try_into().unwrap()), 0);
    assert_eq!(&first[84..92], &[0, 0, 1, 0, 255, 255, 255, 255]);
    assert!(
        maintenance < 100,
        "unexpected per-word window maintenance: {maintenance}"
    );
    println!(
        "D2 model commands={} maintenance={maintenance}",
        commands.len() + 1
    );
}
#[test]
fn compiled_affine_camera_normal_and_bounds() {
    let source = r#"
mod gpu_transform;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let mut p:Buf<u16,18>=Buf::new([0;18]);let mut c:Buf<u16,18>=Buf::new([0;18]);
 gpu_pose_set(p.as_array(),vec3::new(fix32::from_int(2),fix32::from_int(3),fix32::from_int(4)),vec3::zero(),vec3::new(fix32::from_int(2),fix32::from_int(1),fix32::from_words(32768,0)));
 gpu_pose_set(c.as_array(),vec3::new(fix32::from_int(1),fix32::from_int(1),fix32::from_int(1)),vec3::zero(),vec3::new(fix32::from_int(1),fix32::from_int(1),fix32::from_int(1)));
 let mut m:Buf<u16,42>=Buf::new([0;42]);
 check(gpu_instance_trs(m.as_array(),p.as_array(),c.as_array()));
 check((gpu_fload(m.as_array(),3)-fix32::from_int(1)).abs()<fix32::from_words(64,0));
 check((gpu_fload(m.as_array(),7)-fix32::from_int(2)).abs()<fix32::from_words(64,0));
 check((gpu_fload(m.as_array(),12)-fix32::from_words(32768,0)).abs()<fix32::from_words(64,0));
 check((gpu_fload(m.as_array(),20)-fix32::from_int(2)).abs()<fix32::from_words(64,0));
 let mut b:Buf<u16,12>=Buf::new([0;12]);gpu_bounds_set(b.as_array(),vec3::zero(),vec3::new(fix32::from_words(16384,0),fix32::from_words(16384,0),fix32::from_words(16384,0)));
 check(gpu_instance_bounds(m.as_array(),b.as_array()));
 gpu_fstore(m.as_array(),0,fix32::from_int(4));gpu_fstore(m.as_array(),1,fix32::from_int(4));gpu_fstore(m.as_array(),2,fix32::zero());gpu_fstore(m.as_array(),3,fix32::zero());
 gpu_bounds_set(b.as_array(),vec3::zero(),vec3::new(fix32::from_int(64),fix32::from_int(64),fix32::from_words(16384,0)));
 check(!gpu_instance_bounds(m.as_array(),b.as_array()));
 gpu_fstore(m.as_array(),0,-fix32::from_int(4));gpu_fstore(m.as_array(),1,-fix32::from_int(4));check(gpu_instance_bounds(m.as_array(),b.as_array()));
 gpu_fstore(b.as_array(),4,fix32::from_words(1,64));check(!gpu_instance_bounds(m.as_array(),b.as_array()));
 gpu_fstore(p.as_array(),5,fix32::from_words(37408,1));
 check(gpu_instance_trs(m.as_array(),p.as_array(),c.as_array()));
 check(gpu_fload(m.as_array(),0).abs()<fix32::from_words(128,0));
 check((gpu_fload(m.as_array(),1)+fix32::from_int(1)).abs()<fix32::from_words(128,0));
 check((gpu_fload(m.as_array(),4)-fix32::from_int(2)).abs()<fix32::from_words(128,0));
 let saved=gpu_fload(m.as_array(),4);
 gpu_fstore(p.as_array(),6,fix32::zero());check(!gpu_instance_trs(m.as_array(),p.as_array(),c.as_array()));
 check(gpu_fload(m.as_array(),4)==saved);
 gpu_fstore(p.as_array(),6,fix32::from_words(0,32768));check(!gpu_affine_trs(m.as_array(),p.as_array()));
 halt(0x600d);
}
"#;
    run(source, false);
}
