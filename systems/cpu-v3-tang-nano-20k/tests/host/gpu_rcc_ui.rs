//! Compiled UI records and ownership controls; peers do not render.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, RunOutcome};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};
fn compile(source: &str) -> Vec<u16> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("D2 builder", source, &options, &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs")))
                .or_else(|_| {
                    std::fs::read_to_string(
                        root.join(format!("../examples/gpu_demo/rcc/{name}.rs")),
                    )
                })
                .map_err(|e| e.to_string())
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
fn run(source: &str) -> (CpuV3Sim, usize) {
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&compile(source));
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
fn packed_helpers_preserve_neighbours_and_reject_bounds() {
    run(r#"mod gpu;static C:Buf<u16,375>=Buf::new([0;375]);
 fn check(x:bool){if !x{halt(0xbad);}}fn main(){
 check(gpu_ui_fill(C.as_array(),255));check(C[0u16]==65535&&C[374u16]==65535);
 check(gpu_ui_set_cell(C.as_array(),0,0,0));check(C[0u16]==65280);
 check(gpu_ui_set_cell(C.as_array(),1,0,254));check(C[0u16]==65024);
 check(gpu_ui_set_cell(C.as_array(),49,14,7));check(C[374u16]==2047);
 check(!gpu_ui_set_cell(C.as_array(),50,0,1));check(!gpu_ui_set_cell(C.as_array(),0,15,1));
 check(!gpu_ui_set_cell(C.as_array(),0,0,256));check(!gpu_ui_fill(C.as_array(),256));
 check(C[0u16]==65024&&C[374u16]==2047);
 check(!gpu_ui_fill(unsafe{Ptr::from_addr(0xc000).as_u16_array()},0));halt(0x600d);}"#);
}
#[test]
fn ui_snapshot_crosses_window_and_matches_host_descriptor_and_commands() {
    let (cpu, _) = run(r#"mod gpu;static C:Buf<u16,375>=Buf::new([0;375]);
 fn check(x:bool){if !x{halt(0xbad);}}fn main(){unsafe{write_dseg_page(3,19);}
 check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();let pass=gpu_pass_begin(cb,1,65535);check(pass==1);
 let mut a:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};let mut i:u16=0;
 while i<375{C[i]=(i&255)|(((374-i)&255)<<8);i+=1;}
 GPU_CB.object_words=16320;
 check(gpu_pass_draw_ui_grid(pass,view_of(&mut a),C.as_array()));check(GPU_CB.object_words==16720);
 check(gpu_ui_fill(C.as_array(),255));check(read_dseg_page(3)==19);
 check(gpu_pass_end(pass));check(gpu_cb_finish(cb));halt(0x600d);}"#);
    let word = |address: u32| cpu.physical_memory(PhysicalWordAddress::new(address / 2));
    let base = 0x4a0000 + 16320 * 2;
    for i in 0..375 {
        assert_eq!(
            word(base + i * 2),
            ((i & 255) | (((374 - i) & 255) << 8)) as u16
        );
    }
    for i in 375..384 {
        assert_eq!(word(base + i * 2), 65535);
    }
    let object: Vec<u8> = (0..16)
        .flat_map(|i| word(base + 768 + i * 2).to_le_bytes())
        .collect();
    assert_eq!(
        object,
        gpu_v2::system::vliw_command::ui_params(base, 0x4c0000).unwrap()
    );
    use gpu_v2::system::vliw_command::Command as C;
    let expected = [
        C::wait(true, true),
        C::FLUSH,
        C::dma_local(0x4a0000, 276, 32).unwrap(),
        C::wait(true, false),
        C::set_target(276).unwrap(),
        C::clear(false, 1),
        C::clear(true, 65535),
        C::wait(true, true),
        C::FLUSH,
        C::dma_local(base + 768, 276, 32).unwrap(),
        C::wait(true, false),
        C::ui_grid(276).unwrap(),
        C::wait(true, true),
        C::FLUSH,
        C::END,
    ];
    let bytes: Vec<u8> = (0..64)
        .flat_map(|i| word(0x490000 + i * 2).to_le_bytes())
        .collect();
    let mut gold: Vec<u8> = expected.iter().flat_map(|c| c.encode()).collect();
    gold.resize(128, 0);
    assert_eq!(bytes, gold);
}
#[test]
fn rejected_ui_preserves_recording_and_exact_object_capacity_is_usable() {
    run(r#"mod gpu;static C:Buf<u16,375>=Buf::new([65535;375]);
 fn check(x:bool){if !x{halt(0xbad);}}fn main(){check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();
 let mut a:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};
 check(!gpu_pass_draw_ui_grid(c,view_of(&mut a),C.as_array()));let p=gpu_pass_begin(c,0,65535);check(p==1);
 let old=GPU_CB.command_words;let object=GPU_CB.object_words;let used=GPU_BUILD.as_array()[2u16];
 a.sector=0x9801;check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));
 a.sector=0xfe00;a.sectors=513;check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));
 a.sectors=511;check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));a.sectors=512;
 check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));a.sector=0xf800;
 check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),unsafe{Ptr::from_addr(0xbfff).as_u16_array()}));
 GPU_CB.command_words=32754;check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));
 check(GPU_CB.command_words==32754&&GPU_CB.object_words==object&&GPU_BUILD.as_array()[2u16]==used);
 GPU_CB.command_words=old;
 GPU_CB.object_words=32321;check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));
 check(GPU_CB.object_words==32321&&GPU_CB.command_words==old&&GPU_BUILD.as_array()[2u16]==used);
 GPU_CB.object_words=32320;check(gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));check(GPU_CB.object_words==32720);
 check(gpu_pass_end(p));check(gpu_cb_finish(c));check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));halt(0x600d);}"#);
}

#[test]
fn ui_cannot_record_after_finish_inflight_ready_or_pending() {
    let (_, maintenance) = run(r#"mod gpu;static C:Buf<u16,375>=Buf::new([65535;375]);
 fn check(x:bool){if !x{halt(0xbad);}}fn main(){check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();let p=gpu_pass_begin(c,0,65535);
 let mut a:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};check(gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));
 check(gpu_pass_end(p));check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));check(gpu_cb_finish(c));
 check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));check(gpu_submit_idle(c)==GPU_PENDING);
 check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));check(gpu_poll(c)==GPU_COMPLETED);
 check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));check(gpu_present(c)==GPU_PENDING);
 check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),C.as_array()));check(!gpu_cb_cancel(c));check(!gpu_discard_ready(c));halt(0x600d);}"#);
    assert!(maintenance > 0);
}

#[test]
fn ranges_limit_initialization_and_snapshot_and_coalesce_only_ui_fences() {
    let (cpu, _) = run(r#"mod gpu;static C:Buf<u16,375>=Buf::new([0x0100;375]);
 fn check(x:bool){if !x{halt(0xbad);}}fn main(){unsafe{write_dseg_page(3,19);}
 check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();let p=gpu_pass_begin(c,1,65535);
 let mut a:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};
 let before=GPU_CB.command_words;let object=GPU_CB.object_words;
 check(gpu_pass_ui_begin_range(p,view_of(&mut a),255,4,4)==GPU_CB_NONE);
 check(!gpu_pass_set_ui_grid_range(p,view_of(&mut a),C.as_array(),1,751));
 check(GPU_CB.command_words==before&&GPU_CB.object_words==object);
 let ui=gpu_pass_ui_begin_range(p,view_of(&mut a),255,1,4);check(ui==1);
 let mut ids:Buf<u16,3>=Buf::new([1,2,3]);
 check(!gpu_pass_ui_write(ui,0,ids.as_array(),1));
 check(!gpu_pass_ui_write(ui,3,ids.as_array(),2));
 check(gpu_pass_ui_write(ui,1,ids.as_array(),3));check(gpu_pass_ui_set(ui));
 check(GPU_CB.command_words==before+8);
 check(!gpu_pass_draw_ui_range(p,0,2));check(!gpu_pass_draw_ui_range(p,3,5));
 check(gpu_pass_draw_ui_range(p,1,2));check(GPU_CB.command_words==before+10);
 check(gpu_pass_draw_ui_range(p,2,4));check(GPU_CB.command_words==before+12);
 check(gpu_pass_set_ui_grid_range(p,view_of(&mut a),C.as_array(),127,130));
 check(gpu_pass_draw_ui_range(p,127,130));
 check(GPU_CB.command_words==before+18);
 let last=gpu_pass_ui_begin_range(p,view_of(&mut a),254,749,750);check(last==1);
 check(gpu_pass_ui_write(last,750,ids.as_array(),0));check(gpu_pass_ui_end(last));
 check(GPU_CB.command_words==before+24);
 check(gpu_pass_end(p));check(gpu_cb_finish(c));check(read_dseg_page(3)==19);halt(0x600d);}"#);
    let word = |address: u32| cpu.physical_memory(PhysicalWordAddress::new(address / 2));
    // Full objects remain allocated/readable, but unselected words were not filled/copied.
    let bases = [0x4a0080, 0x4a0400, 0x4a0780];
    assert_eq!(word(bases[0]), 0x01ff);
    assert_eq!(word(bases[0] + 2), 0x0302);
    assert_eq!(word(bases[0] + 4), 0);
    assert_eq!(word(bases[1] + 62 * 2), 0);
    assert_eq!(word(bases[1] + 63 * 2), 0x0100);
    assert_eq!(word(bases[1] + 64 * 2), 0x0100);
    assert_eq!(word(bases[1] + 65 * 2), 0);
    assert_eq!(word(bases[2] + 373 * 2), 0);
    assert_eq!(word(bases[2] + 374 * 2), 0xfefe);
    use gpu_v2::system::vliw_command::Command as C;
    let expected = [
        C::wait(true, true),
        C::FLUSH,
        C::dma_local(bases[0] + 768, 276, 32).unwrap(),
        C::wait(true, false),
        C::ui_grid_range(276, 1, 2).unwrap(),
        C::ui_grid_range(276, 2, 4).unwrap(),
        C::dma_local(bases[1] + 768, 276, 32).unwrap(),
        C::wait(true, false),
        C::ui_grid_range(276, 127, 130).unwrap(),
        C::dma_local(bases[2] + 768, 276, 32).unwrap(),
        C::wait(true, false),
        C::ui_grid_range(276, 749, 750).unwrap(),
    ];
    let actual: Vec<u8> = (0..expected.len() * 2)
        .flat_map(|i| word(0x490000 + 28 + i as u32 * 2).to_le_bytes())
        .collect();
    assert_eq!(
        actual,
        expected.iter().flat_map(|c| c.encode()).collect::<Vec<_>>()
    );

    // Optional handoff to ui_grid.py: exact compiled-CPU commands and objects.
    // Remove only the two CLEARs so a nonuniform preexisting surface can expose
    // writes outside the UI intervals. All UI/DMA/fence bytes remain untouched.
    if let Some(path) = std::env::var_os("GPU_RCC_UI_RANGE_FIXTURE") {
        let commands: Vec<u32> = (0..22)
            .filter(|i| *i != 5 && *i != 6)
            .map(|i| u32::from(word(0x490000 + i * 4)) | (u32::from(word(0x490002 + i * 4)) << 16))
            .collect();
        assert_eq!(*commands.last().unwrap(), 0xf0000000);
        let objects: Vec<u16> = (0..1360).map(|i| word(0x4a0000 + i * 2)).collect();
        let target = u32::from(word(0x4a0000)) | (u32::from(word(0x4a0002)) << 16);
        let color = (target & 16383) << 9;
        let depth = ((target >> 14) & 16383) << 9;
        let fixture = format!(
            r#"{{"command_base":4784128,"object_base":4849664,
            "color_base":{color},"depth_base":{depth},"atlas_base":4980736,
            "commands":{commands:?},"objects":{objects:?},
            "calls":[{{"first":1,"end":2,"grid":{}}},
                {{"first":2,"end":4,"grid":{}}},
                {{"first":127,"end":130,"grid":{}}},
                {{"first":749,"end":750,"grid":{}}}]}}"#,
            bases[0], bases[0], bases[1], bases[2]
        );
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, fixture).unwrap();
    }
}

#[test]
fn one_ui_install_drives_a_batch_without_descriptor_reload() {
    let (cpu, _) = run(r#"mod gpu;static C:Buf<u16,375>=Buf::new([0x0100;375]);
 fn check(x:bool){if !x{halt(0xbad);}}fn main(){
 check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();let pass=gpu_pass_begin(cb,1,65535);
 let mut a:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};
 check(!gpu_pass_draw_ui_range(pass,1,4));
 C[1u16]=0x0302;C[374u16]=0xfe01;
 let before=GPU_CB.command_words;check(gpu_pass_set_ui_grid(pass,view_of(&mut a),C.as_array()));
 check(GPU_CB.command_words==before+8);
 check(gpu_ui_fill(C.as_array(),255));
 check(gpu_pass_draw_ui_range(pass,1,4));check(GPU_CB.command_words==before+10);
 check(gpu_pass_draw_ui_range(pass,127,130));check(GPU_CB.command_words==before+12);
 check(gpu_pass_draw_ui_range(pass,749,750));check(GPU_CB.command_words==before+14);
 check(gpu_pass_end(pass));check(gpu_cb_finish(cb));halt(0x600d);}"#);
    let word = |address: u32| cpu.physical_memory(PhysicalWordAddress::new(address / 2));
    let commands: Vec<u32> = (0..17)
        .map(|i| u32::from(word(0x490000 + i * 4)) | (u32::from(word(0x490002 + i * 4)) << 16))
        .collect();
    use gpu_v2::system::vliw_command::Command as C;
    assert_eq!(
        commands[9],
        u32::from_le_bytes(C::dma_local(0x4a0380, 276, 32).unwrap().encode())
    );
    assert_eq!(commands[10], 0x40000001);
    for (command, (first, end)) in commands[11..14]
        .iter()
        .zip([(1, 4), (127, 130), (749, 750)])
    {
        assert_eq!(
            *command,
            u32::from_le_bytes(C::ui_grid_range(276, first, end).unwrap().encode())
        );
    }
    assert_eq!(commands.iter().filter(|c| **c >> 28 == 1).count(), 2); // Target + UI only.
    assert_eq!(commands[16], 0xf0000000);
    assert_eq!(word(0x4a0082), 0x0302); // Caller edits cannot alter the installed snapshot.
    if let Some(path) = std::env::var_os("GPU_RCC_UI_BATCH_FIXTURE") {
        let commands: Vec<_> = commands
            .into_iter()
            .enumerate()
            .filter_map(|(i, c)| (i != 5 && i != 6).then_some(c))
            .collect();
        let objects: Vec<_> = (0..464).map(|i| word(0x4a0000 + i * 2)).collect();
        let target = u32::from(objects[0]) | (u32::from(objects[1]) << 16);
        let color = (target & 16383) << 9;
        let depth = ((target >> 14) & 16383) << 9;
        let command_base = 0x490000;
        let object_base = 0x4a0000;
        let atlas_base = 0x4c0000;
        let grid = 0x4a0080;
        let fixture = format!(
            r#"{{"command_base":{command_base},"object_base":{object_base},
            "color_base":{color},"depth_base":{depth},"atlas_base":{atlas_base},
            "single_binding":true,"commands":{commands:?},"objects":{objects:?},
            "calls":[{{"first":1,"end":4,"grid":{grid}}},
                {{"first":127,"end":130,"grid":{grid}}},
                {{"first":749,"end":750,"grid":{grid}}}]}}"#
        );
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, fixture).unwrap();
    }
}

#[test]
fn production_demo_encodes_only_visible_digits_and_skips_invalid_interval() {
    let (cpu, _) = run(r#"mod overlay_gpu;
 fn check(x:bool){if !x{halt(0xbad);}}fn main(){
 check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();let p=gpu_pass_begin(c,1,65535);
 let mut a:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};
 let mut state:Buf<u16,4>=Buf::new([0,0,1,0]);let before=GPU_CB.command_words;
 check(demo_ui_record(p,view_of(&mut a),state.as_array()));check(GPU_CB.command_words==before);
 state[1u16]=17000;state[3u16]=1;check(demo_ui_record(p,view_of(&mut a),state.as_array()));
 check(GPU_CB.command_words==before+10);
 state[1u16]=0;check(demo_ui_record(p,view_of(&mut a),state.as_array()));
 check(GPU_CB.command_words==before+16);
 check(gpu_pass_end(p));check(gpu_cb_finish(c));halt(0x600d);}"#);
    let word = |address: u32| cpu.physical_memory(PhysicalWordAddress::new(address / 2));
    let command =
        |i: u32| u32::from(word(0x490000 + i * 4)) | (u32::from(word(0x490002 + i * 4)) << 16);
    use gpu_v2::system::vliw_command::Command as C;
    assert_eq!(
        command(11),
        u32::from_le_bytes(C::ui_grid_range(276, 52, 56).unwrap().encode())
    );
    assert_eq!(
        command(14),
        u32::from_le_bytes(C::ui_grid_range(276, 52, 55).unwrap().encode())
    );
    assert_eq!(word(0x4a0080 + 52), 0x3731); // "17"
    assert_eq!(word(0x4a0080 + 54), 0x302e); // ".0"
    assert_eq!(word(0x4a0400 + 52), 0x2e30); // "0."
    assert_eq!(word(0x4a0400 + 54), 0xff30); // "0", adjacent byte masked
    assert_eq!(word(0x4a0080 + 50), 0); // no full-grid FF initialization
}
