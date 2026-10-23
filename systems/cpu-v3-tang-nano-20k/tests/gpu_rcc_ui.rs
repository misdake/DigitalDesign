//! Compiled UI records and ownership controls; peers do not render.
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
