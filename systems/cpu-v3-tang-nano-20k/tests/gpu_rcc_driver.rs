//! Compile and execute the formal RCC GPU library; host checks do not render.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, RunOutcome};
use gpu_v2::system::vliw_command::{Command as GpuCommand, DeviceSubmission};
use std::path::PathBuf;
use std::{any::Any, cell::RefCell, rc::Rc};

fn compile_direct(source: &str) -> Vec<u16> {
    compile(source)
}
fn compile(source: &str) -> Vec<u16> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("GPU driver fixture", source, &options, &mut |name| {
            let path = if name == "overlay_gpu" {
                root.join("../examples/gpu_demo/rcc/overlay_gpu.rs")
            } else {
                root.join(format!("{name}.rs"))
            };
            std::fs::read_to_string(path).map_err(|e| e.to_string())
        })
        .unwrap();
    rcc_backend::try_compile(parsed, &options, "main")
        .unwrap()
        .words
}

#[derive(Default)]
struct PeerState {
    status: u16,
    cache_error: bool,
    starts: usize,
    counts: usize,
    maintenance: usize,
    cache_error_after: usize,
}
struct ControlPeer {
    state: Rc<RefCell<PeerState>>,
    kind: u8,
}
impl cpu_v3::Device for ControlPeer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let s = self.state.borrow();
        match (self.kind, channel) {
            (4, 2) => s.status,
            (3, 2) => 0x20,
            (0, 5) => {
                u16::from(
                    s.cache_error
                        || (s.cache_error_after != 0 && s.maintenance >= s.cache_error_after),
                ) * 0x8000
            }
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, channel) {
            (4, 0) => {
                assert_eq!(s.status, 0);
                assert_eq!(value, 9);
                s.counts += 1;
            }
            (4, 1) => {
                assert_eq!(s.status, 0);
                s.status = 1;
                s.starts += 1;
            }
            (0, 1) => s.maintenance += 1,
            (7, 0) => match value {
                0 => s.status = 0,
                1 => s.status = 1,
                2 => s.status = 3,
                3 => s.status = 2,
                4 => s.cache_error = false,
                5 => s.cache_error = true,
                _ => panic!("peer stimulus"),
            },
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn compiled_high_slot_mapping_boundaries_and_restoration() {
    let source = r#"
mod gpu_device;
fn check(ok:bool) { if !ok { halt(0xbad); } }
fn main() {
 unsafe { write_dseg_page(3,17); }
 let mut a:GpuBuffer=gpu_buffer_empty();let mut b:GpuBuffer=gpu_buffer_empty();let mut duplicate:GpuBuffer=gpu_buffer_empty();
 check(gpu_high_slot_init(view_of(&mut a),0));check(!gpu_high_slot_init(view_of(&mut b),1));
 check(!gpu_high_slot_init(view_of(&mut duplicate),0));
 check(!gpu_build_clear(view_of(&mut a),GPU_COLOR_A_SECTOR,GPU_COLOR_A_SECTOR,0,0));
 check(a.state==GPU_CB_BUILDING && a.command_words==0);
 let mut data:Buf<u16,8>=Buf::new([0x2345;8]);let mut out:Buf<u16,8>=Buf::new([0;8]);
 check(gpu_slot_upload(view_of(&mut a),true,32760,data.as_array(),8));
 check(!gpu_slot_upload(view_of(&mut a),true,32764,data.as_array(),8));
 check(gpu_slot_download(view_of(&mut a),true,32760,out.as_array(),8));check(out[7u16]==0x2345);
 // Internal memory primitive under an explicitly owned synthetic asset fixture.
 check(gpu_window_copy(data.as_array(),GPU_ASSET_FIRST_SECTOR,16380,8,false));
 check(gpu_window_copy(out.as_array(),GPU_ASSET_FIRST_SECTOR,16380,8,true));check(out[0u16]==0x2345);
 check(gpu_window_copy(data.as_array(),65535,60,4,false));
 check(!gpu_window_copy(data.as_array(),65535,60,5,false));
 check(read_dseg_page(3)==17 && read_dseg_page(0)==0 && read_dseg_page(1)==1 && read_dseg_page(2)==2);
 check(gpu_build_clear(view_of(&mut a),GPU_COLOR_B_SECTOR,GPU_DEPTH_SECTOR,0x1357,0x2468));
 check(read_dseg_page(3)==17);halt(0x600d);
}
"#;
    let words = compile(source);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&words);
    let state = Rc::new(RefCell::new(PeerState::default()));
    cpu.attach_device(0, Box::new(ControlPeer { state, kind: 0 }));
    let result = cpu.run(2_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    assert_eq!(cpu.data_segments(), [0, 1, 2, 17]);
    use gpu_v2::{
        framebuffer::ports::MaterializedSurface,
        system::vliw_command::{target_params, CommandBuffer},
    };
    let mut cb = CommandBuffer::new();
    for c in [
        GpuCommand::wait(true, true),
        GpuCommand::FLUSH,
        GpuCommand::dma_local(0x4a0000, 276, 32).unwrap(),
        GpuCommand::wait(true, false),
        GpuCommand::set_target(276).unwrap(),
        GpuCommand::clear(false, 0x1357),
        GpuCommand::clear(true, 0x2468),
        GpuCommand::wait(true, true),
        GpuCommand::FLUSH,
        GpuCommand::END,
    ] {
        cb.push(c);
    }
    assert_eq!(bytes(&cpu, 0x490000 / 2, 128), cb.image().unwrap().storage);
    assert_eq!(
        bytes(&cpu, 0x4a0000 / 2, 32),
        target_params(MaterializedSurface {
            width: 400,
            height: 240,
            color_base_bytes: 0x430000,
            depth_base_bytes: 0x460000
        })
        .unwrap()
    );
    assert_eq!(
        bytes(&cpu, 0x7ffff8 / 2, 8),
        [0x45, 0x23, 0x45, 0x23, 0x45, 0x23, 0x45, 0x23]
    );
}

#[test]
fn scoped_maintenance_failure_restores_mapping_and_faults_driver() {
    for fail_on in [1, 2] {
        let source = r#"
mod gpu_device;
fn check(ok:bool) { if !ok { halt(0xbad); } }
fn main() {
 unsafe { write_dseg_page(3,19); }
 let mut a:GpuBuffer=gpu_buffer_empty();check(gpu_high_slot_init(view_of(&mut a),0));
 let mut data:Buf<u16,4>=Buf::new([0x6789;4]);
 check(!gpu_slot_upload(view_of(&mut a),true,0,data.as_array(),4));
 check(read_dseg_page(3)==19);check(read_dseg_page(0)==0 && read_dseg_page(1)==1 && read_dseg_page(2)==2);
 check(gpu_memory_faulted());check(a.state==GPU_CB_BUILDING && a.command_words==0 && a.object_words==0);
 check(gpu_driver_init()==GPU_FAULT);halt(0x600d);
}

"#;
        let words = compile(source);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&words);
        let state = Rc::new(RefCell::new(PeerState {
            cache_error_after: fail_on,
            ..PeerState::default()
        }));
        for kind in [0, 4] {
            cpu.attach_device(
                kind,
                Box::new(ControlPeer {
                    state: state.clone(),
                    kind,
                }),
            );
        }
        let result = cpu.run(1_000_000).unwrap();
        assert!(
            matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
            "fail_on={fail_on} {result:?}"
        );
        assert_eq!(state.borrow().starts, 0);
    }
}
#[test]
fn paired_window_preflights_both_spans_and_crosses_into_tail() {
    let words = compile(
        r#"mod gpu_device;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 unsafe{write_dseg_page(3,19);}
 let mut a:Buf<u16,3>=Buf::new([0x1111,0x2222,0x3333]);
 let mut b:Buf<u16,5>=Buf::new([0x4444,0x5555,0x6666,0x7777,0x8888]);
 check(!gpu_window_upload_pair(a.as_array(),3,unsafe{Ptr::from_addr(0xbfff).as_u16_array()},2,GPU_ASSET_FIRST_SECTOR,16380));
 check(!gpu_window_upload_pair(a.as_array(),3,b.as_array(),32766,GPU_ASSET_FIRST_SECTOR,0));
 check(!gpu_window_upload_pair(a.as_array(),3,b.as_array(),5,65535,60));
 check(!gpu_window_upload_pair(a.as_array(),0,b.as_array(),5,GPU_ASSET_FIRST_SECTOR,0));
 check(!gpu_memory_faulted());
 check(gpu_window_upload_pair(a.as_array(),3,b.as_array(),5,GPU_ASSET_FIRST_SECTOR,16380));
 check(read_dseg_page(3)==19);halt(0x600d);
}"#,
    );
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&words);
    let state = Rc::new(RefCell::new(PeerState::default()));
    cpu.attach_device(
        0,
        Box::new(ControlPeer {
            state: state.clone(),
            kind: 0,
        }),
    );
    let at = 0x4b0000 / 2 + 16380;
    cpu.physical_memory_mut()[at - 1] = 0xabcd;
    cpu.physical_memory_mut()[at + 8] = 0xdcba;
    assert!(matches!(
        cpu.run(1_000_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    for (i, value) in [
        0x1111, 0x2222, 0x3333, 0x4444, 0x5555, 0x6666, 0x7777, 0x8888,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            cpu.physical_memory(PhysicalWordAddress::new((at + i) as u32)),
            value
        );
    }
    assert_eq!(
        cpu.physical_memory(PhysicalWordAddress::new((at - 1) as u32)),
        0xabcd
    );
    assert_eq!(
        cpu.physical_memory(PhysicalWordAddress::new((at + 8) as u32)),
        0xdcba
    );
    assert_eq!(
        state.borrow().maintenance,
        4,
        "Two page scopes; rejected spans must cause no maintenance"
    );
}

#[test]
fn paired_window_failure_on_either_page_restores_mapping() {
    let words = compile(
        r#"mod gpu_device;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){unsafe{write_dseg_page(3,19);}
 let mut a:Buf<u16,3>=Buf::new([1,2,3]);let mut b:Buf<u16,5>=Buf::new([4,5,6,7,8]);
 check(!gpu_window_upload_pair(a.as_array(),3,b.as_array(),5,GPU_ASSET_FIRST_SECTOR,16380));
 check(read_dseg_page(3)==19&&gpu_memory_faulted());check(gpu_driver_init()==GPU_FAULT);halt(0x600d);
}"#,
    );
    for fail_on in 1..=4 {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&words);
        let state = Rc::new(RefCell::new(PeerState {
            cache_error_after: fail_on,
            ..PeerState::default()
        }));
        for kind in [0, 4] {
            cpu.attach_device(
                kind,
                Box::new(ControlPeer {
                    state: state.clone(),
                    kind,
                }),
            );
        }
        assert!(matches!(
            cpu.run(1_000_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        assert_eq!(cpu.data_segments(), [0, 1, 2, 19]);
        assert_eq!(state.borrow().starts, 0);
    }
}
const CONTROL_SETUP: &str = r#"
mod gpu;
fn check(ok:bool) { if !ok { halt(0xbad); } }
fn main() {
"#;
#[test]
fn compiled_driver_control_ownership_faults_and_timeout() {
    // Control-only peers deliberately do not execute or certify rendering.
    let bodies = [
        r#"check(gpu_init()==GPU_IDLE);check(gpu_init()==GPU_REJECTED);
        let cb=gpu_cb_begin();check(cb==1);check(gpu_cb_begin()==0);
        check(gpu_pass_clear(cb,0x1234,0x5678));check(gpu_submit_idle(cb)==GPU_PENDING);
        check(gpu_submit_idle(cb)==GPU_REJECTED);check(gpu_cb_begin()==0);
        check(gpu_wait(cb,1)==GPU_TIMEOUT);check(!gpu_pass_clear(cb,0,0));
        check(gpu_poll(2)==GPU_REJECTED);dev_send(7,0,0);
        check(gpu_poll(cb)==GPU_COMPLETED);check(gpu_poll(cb)==GPU_IDLE);check(gpu_discard_ready(cb));
        let next=gpu_cb_begin();check(next==1);check(gpu_cb_begin()==0);
        check(gpu_pass_clear(next,0xabba,0x4321));check(gpu_submit_idle(next)==GPU_PENDING);
        dev_send(7,0,0);check(gpu_wait(next,4)==GPU_COMPLETED);"#,
        r#"check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();check(gpu_pass_clear(cb,0,0));
        dev_send(7,0,5);check(gpu_submit_idle(cb)==GPU_CACHE_ERROR);
        dev_send(7,0,4);check(gpu_poll(cb)==GPU_FAULT);check(gpu_submit_idle(cb)==GPU_FAULT);
        check(gpu_cb_begin()==0);check(gpu_init()==GPU_REJECTED);"#,
        r#"check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();check(gpu_pass_clear(cb,1,2));
        check(gpu_submit_idle(cb)==GPU_PENDING);dev_send(7,0,2);check(gpu_poll(cb)==GPU_FAULT);
        dev_send(7,0,3);check(gpu_poll(cb)==GPU_FAULT);check(gpu_cb_begin()==0);
        check(!gpu_pass_clear(cb,0,0));check(gpu_submit_idle(cb)==GPU_FAULT);check(gpu_init()==GPU_REJECTED);"#,
        r#"check(gpu_init()==GPU_FAULT);check(gpu_cb_begin()==0);check(gpu_submit_idle(1)==GPU_FAULT);"#,
        r#"check(gpu_init()==GPU_PENDING);check(gpu_cb_begin()==0);check(gpu_poll(1)==GPU_PENDING);
        dev_send(7,0,0);check(gpu_poll(1)==GPU_IDLE);let cb=gpu_cb_begin();check(cb==1);
        check(gpu_pass_clear(cb,0,0));check(gpu_submit_idle(cb)==GPU_PENDING);"#,
        r#"check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();check(gpu_pass_clear(cb,1,2));
        check(gpu_submit_idle(cb)==GPU_PENDING);dev_send(7,0,3);
        check(gpu_poll(cb)==GPU_FAULT);check(gpu_cb_begin()==0);
        check(!gpu_pass_clear(cb,0,0));check(gpu_submit_idle(cb)==GPU_FAULT);"#,
    ];
    for (case, body) in bodies.iter().enumerate() {
        let source = format!("{CONTROL_SETUP}{body}halt(0x600d);}}");
        let words = compile(&source);
        let state = Rc::new(RefCell::new(PeerState {
            status: match case {
                3 => 2,
                4 => 1,
                _ => 0,
            },
            ..PeerState::default()
        }));
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&words);
        for kind in [0, 3, 4, 7] {
            cpu.attach_device(
                kind,
                Box::new(ControlPeer {
                    state: state.clone(),
                    kind,
                }),
            );
        }
        let result = cpu.run(1_000_000).unwrap();
        assert!(
            matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
            "case={case} {result:?}"
        );
        let state = state.borrow();
        assert_eq!(
            state.starts,
            match case {
                0 => 2,
                1 | 3 => 0,
                _ => 1,
            }
        );
        assert_eq!(state.counts, state.starts);
        assert_eq!(
            state.maintenance,
            match case {
                0 => 2,
                1 => 1,
                2 | 4 | 5 => 1,
                3 => 0,
                _ => unreachable!(),
            }
        );
    }
}
fn run(source: &str) -> CpuV3Sim {
    let words = compile(source);
    let mut cpu = CpuV3Sim::with_physical_memory_words(65536);
    cpu.set_boot_window(&words);
    let result = cpu.run(1_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    cpu
}
fn bytes(cpu: &CpuV3Sim, word: u32, count: usize) -> Vec<u8> {
    (0..count / 2)
        .flat_map(|i| {
            cpu.physical_memory(PhysicalWordAddress::new(word + i as u32))
                .to_le_bytes()
        })
        .collect()
}

#[test]
fn compiled_native_encoders_match_host_and_fail_atomically() {
    let source = r#"
mod gpu_device;
fn check(ok: bool) { if !ok { halt(0xbad); } }
fn main() {
 let mut c: GpuBuffer = gpu_buffer_empty();
 let mut command: GpuStorage = GpuStorage { cpu: Ptr::from_addr(0x4000), sector: 256, sectors: 2 };
 let mut objects: GpuStorage = GpuStorage { cpu: Ptr::from_addr(0x4100), sector: 260, sectors: 4 };
 check(gpu_buffer_init(view_of(&mut c),view_of(&mut command),view_of(&mut objects)));
 let mut g: GpuGlobal = GpuGlobal { far_low: 65535, far_high: 199, light_x: 0, light_y: 0, light_z: 16384,
 ray_x: -12288, ray_y: 12288, ray_k: 8192, ambient: 256, directional: 256, mode: 3, shininess: 16, epoch: 65535 };
 check(gpu_record_global(view_of(&mut c),view_of(&mut g)));
 g.far_high=200;
 check(!gpu_record_global(view_of(&mut c),view_of(&mut g))); check(c.object_words==32);
 let mut r: GpuRender = GpuRender { texture_sector: 40000, texture_sectors: 21848, size_log2: 10,
 full_mip: 1, filter: 2, specular_low: 0x9876, specular_high: 0x54,
 depth_func: 7, depth_write: 1, blend: 1, unlit: 1, lod_bias: -128 };
 check(gpu_record_render(view_of(&mut c),view_of(&mut r)));
 r.filter=3; check(!gpu_record_render(view_of(&mut c),view_of(&mut r)));check(c.object_words==48);
 let mut matrix: Buf<u16,42> = Buf::new([0;42]);
 let mut m=matrix.as_array();let mut i:u16=0;while i<42 { m[i]=i*7; i+=1; }
 check(gpu_record_instance(view_of(&mut c),m,0x1234,0x5678));
 m[0u16]=65535;
 check(c.object_words==128);
 let out=Ptr::from_addr(0x4500);
 check(gpu_command_dma(out,0xff80,0x7f,368,128));
 check(!gpu_command_dma(out.add(2),0xffe0,0x7f,264,64));
 check(gpu_command_meshlet(out.add(4),65527,1,8));
 check(!gpu_command_meshlet(out.add(6),65528,1,8));
 check(gpu_command_set(out.add(8),5,312));
 check(!gpu_command_set(out.add(10),5,316));
 check(gpu_command_draw(out.add(12),1));
 check(gpu_command_fixed(out.add(14),8,0xcafe,1));
 check(gpu_build_clear(view_of(&mut c),34304,35840,0x1234,0xabcd));
 halt(0x600d);
}
"#;
    let cpu = run(source);
    let expected_global: Vec<u8> = [
        0x00c7ffffu32,
        0,
        0xd0004000,
        0x20003000,
        0x01000100,
        0xffff0043,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    assert_eq!(bytes(&cpu, 0x4100, 64), expected_global);
    let expected_render: Vec<u8> = [
        gpu_v2::system::vliw_command::TextureDescriptor {
            base_sector: 40000,
            size_log2: 10,
            full_mip: true,
            filter: 2,
        }
        .word(),
        0x549876,
        63,
        128,
        0,
        0,
        0,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    assert_eq!(bytes(&cpu, 0x4120, 32), expected_render);
    let instance = bytes(&cpu, 0x4140, 128);
    for (i, pair) in instance[..84].as_chunks::<2>().0.iter().enumerate() {
        assert_eq!(u16::from_le_bytes(*pair), i as u16 * 7);
    }
    assert_eq!(
        &instance[84..96],
        &[0, 0, 1, 0, 0x34, 0x12, 0x78, 0x56, 0, 0, 0, 0]
    );
    assert!(instance[96..].iter().all(|b| *b == 0));
    for (offset, expected) in [
        (0, GpuCommand::dma_local(0x7fff80, 368, 128).unwrap()),
        (8, GpuCommand::dma_meshlet(65527 * 128, 1, 1024).unwrap()),
        (16, GpuCommand::set_global(312).unwrap()),
        (24, GpuCommand::draw_meshlet(1).unwrap()),
        (28, GpuCommand::clear(true, 0xcafe)),
    ] {
        assert_eq!(bytes(&cpu, 0x4500 + offset / 2, 4), expected.encode());
    }
    for word in [0x4502, 0x4506, 0x450a] {
        assert_eq!(bytes(&cpu, word, 4), [0; 4]);
    }
    assert_eq!(
        DeviceSubmission::new(0x7fff80, 128).unwrap().base_sector,
        65535
    );
    assert!(DeviceSubmission::new(0x7fff80, 132).is_err());
}

#[test]
fn page_intrinsics_reject_invalid_selectors_and_unsafe_calls() {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    for body in [
        "read_dseg_page(4);",
        "unsafe { write_dseg_page(4,1); }",
        "write_dseg_page(3,1);",
        "unsafe { write_dseg_page(3,256); }",
        "let i:u16=3; read_dseg_page(i);",
    ] {
        let source = format!("fn main() {{ {body} halt(0); }}");
        let result =
            rcc::frontend::compile_program_named("invalid page", &source, &options, &mut |_| {
                Err("unexpected module".into())
            });
        assert!(result.is_err(), "accepted {body}");
    }
}

#[test]
fn compiled_submission_address_edges_are_atomic() {
    let mut source=String::from("mod gpu_encode; fn check(ok:bool){if !ok{halt(0xbad);}} fn main(){ let mut s:GpuSubmission=GpuSubmission{count_minus_one:0x55aa,base_sector:0xaa55};");
    for (base, length) in [
        (0, 4),
        (0, 65536),
        (0x7fff80, 128),
        (0x7fff80, 132),
        (0x7f0000, 65536),
        (0x7f0080, 65536),
        (0x800000, 4),
        (0x400001, 4),
        (0x400000, 0),
        (0x400000, 3),
        (0x400000, 65540),
    ] {
        source.push_str("s.count_minus_one=0x55aa;s.base_sector=0xaa55;\n");
        let call = format!(
            "gpu_encode_submission(view_of(&mut s),{},{},{},{})",
            base & 65535,
            base >> 16,
            length & 65535,
            length >> 16
        );
        if let Ok(expected) = DeviceSubmission::new(base, length) {
            source.push_str(&format!(
                "check({call});check(s.count_minus_one=={} && s.base_sector=={});\n",
                expected.count_minus_one, expected.base_sector
            ));
        } else {
            source.push_str(&format!(
                "check(!{call});check(s.count_minus_one==0x55aa && s.base_sector==0xaa55);\n"
            ));
        }
    }
    source.push_str("halt(0x600d);}");
    run(&source);
}

#[test]
fn high_object_snapshots_and_full_capacity_boundaries() {
    let source = r#"
mod gpu_device;
fn check(ok:bool) { if !ok { halt(0xbad); } }
fn main() {
 let mut cb:GpuBuffer=gpu_buffer_empty();check(gpu_high_slot_init(view_of(&mut cb),0));
 let mut g:GpuGlobal=GpuGlobal { far_low: 0, far_high: 10, light_x: 16384, light_y: 181, light_z: 0,
 ray_x: 12288, ray_y: -12288, ray_k: 12288, ambient: 256, directional: 0, mode: 1, shininess: 0, epoch: 2 };
 check(gpu_record_global(view_of(&mut cb),view_of(&mut g)));
 g.light_y=182;check(!gpu_record_global(view_of(&mut cb),view_of(&mut g)));check(cb.object_words==32);
 g.light_x=16383;g.light_y=0;g.far_high=20;check(gpu_record_global(view_of(&mut cb),view_of(&mut g)));
 g.light_x=16382;check(!gpu_record_global(view_of(&mut cb),view_of(&mut g)));check(cb.object_words==64);
 let mut r:GpuRender=GpuRender { texture_sector:GPU_ASSET_FIRST_SECTOR,texture_sectors:1,size_log2:0,full_mip:0,filter:0,
 specular_low:0x9876,specular_high:0x54,depth_func:1,depth_write:1,blend:0,unlit:1,lod_bias:127 };
 check(gpu_record_render(view_of(&mut cb),view_of(&mut r)));check(cb.object_words==80);
 let mut matrix:Buf<u16,42>=Buf::new([0;42]);let mut m=matrix.as_array();m[0u16]=0x1234;
 check(gpu_record_instance(view_of(&mut cb),m,0xcafe,0x9876));m[0u16]=0xffff;
 check(cb.object_words==192);
 let mut words:Buf<u16,8>=Buf::new([0x3535;8]);let mut result:Buf<u16,8>=Buf::new([0;8]);
 check(gpu_slot_upload(view_of(&mut cb),true,16380,words.as_array(),8));
 check(gpu_slot_download(view_of(&mut cb),true,16380,result.as_array(),8));check(result[7u16]==0x3535);
 check(gpu_slot_upload(view_of(&mut cb),false,32760,words.as_array(),8));
 check(!gpu_slot_upload(view_of(&mut cb),false,32761,words.as_array(),8));
 cb.object_words=32752;check(!gpu_record_global(view_of(&mut cb),view_of(&mut g)));check(cb.object_words==32752);
 check(read_dseg_page(3)==3 && read_dseg_page(0)==0 && read_dseg_page(1)==1 && read_dseg_page(2)==2);
 halt(0x600d);
}
"#;
    let program = compile(source);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program);
    let state = Rc::new(RefCell::new(PeerState::default()));
    cpu.attach_device(0, Box::new(ControlPeer { state, kind: 0 }));
    let result = cpu.run(3_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    assert_eq!(bytes(&cpu, 0x4a0000 / 2, 4), [0, 0, 10, 0]);
    assert_eq!(bytes(&cpu, 0x4a0040 / 2, 4), [0, 0, 20, 0]);
    assert_eq!(bytes(&cpu, (0x4a0000 + 256) / 2, 2), [0x34, 0x12]);
    assert_eq!(
        bytes(&cpu, (0x4a0000 + 256 + 84) / 2, 12),
        [0, 0, 1, 0, 0xfe, 0xca, 0x76, 0x98, 0, 0, 0, 0]
    );
    assert_eq!(bytes(&cpu, 0x49fff0 / 2, 16), [0x35; 16]);
    assert_eq!(bytes(&cpu, 0x4a7ff8 / 2, 16), [0x35; 16]);
    assert_eq!(bytes(&cpu, 0x4a7ff6 / 2, 2), [0; 2]);
    assert_eq!(bytes(&cpu, 0x4a8008 / 2, 2), [0; 2]);
}

#[test]
fn window_copy_preserves_bits_offsets_tails_and_guards() {
    run(r#"mod gpu_device;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let mut source:Buf<u16,40>=Buf::new([0;40]);
 let mut target:Buf<u16,40>=Buf::new([0;40]);
 let mut offset:u16=0;
 while offset<8 {
  let mut count:u16=0;
  while count<24 {
   let mut i:u16=0;
   while i<40 {source[i]=(i*17321)^0x8001;target[i]=0xa55a;i+=1;}
   gpu_window_words(target.as_ptr().add((7-offset) as i16),source.as_ptr().add(offset as i16),count);
   i=0;
   while i<40 {
    if i>=7-offset && i<7-offset+count {check(target[i]==source[i-(7-offset)+offset]);}
    else {check(target[i]==0xa55a);}
    i+=1;
   }
   count+=1;
  }
  offset+=1;
 }
 halt(0x600d);
}"#);
}

#[test]
fn builder_directory_cache_boundaries_and_late_rejection() {
    let source = r#"mod gpu_device;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let mut cb:GpuBuffer=gpu_buffer_empty();check(gpu_high_slot_init(view_of(&mut cb),0));
 check(gpu_builder_begin(view_of(&mut cb),GPU_COLOR_B_SECTOR,GPU_DEPTH_SECTOR,0x1234,0xffff));
 let mut g:GpuGlobal=GpuGlobal{far_low:0,far_high:10,light_x:16384,light_y:0,light_z:0,
 ray_x:0,ray_y:0,ray_k:8192,ambient:256,directional:0,mode:1,shininess:0,epoch:0};
 check(gpu_builder_global(view_of(&mut cb),view_of(&mut g)));
 let global_words=cb.object_words;let global_commands=cb.command_words;
 check(gpu_builder_global(view_of(&mut cb),view_of(&mut g)));
 check(cb.object_words==global_words&&cb.command_words==global_commands);
 g.ambient=257;check(!gpu_builder_global(view_of(&mut cb),view_of(&mut g)));g.ambient=256;
 let mut t:GpuTexture=GpuTexture{sector:GPU_ASSET_FIRST_SECTOR+256,sectors:1,size_log2:0,full_mip:0};
 let mut r:GpuRender=GpuRender{texture_sector:0,texture_sectors:0,size_log2:0,full_mip:0,filter:0,
 specular_low:0,specular_high:0,depth_func:1,depth_write:1,blend:0,unlit:1,lod_bias:0};
 check(gpu_builder_render(view_of(&mut cb),view_of(&mut t),view_of(&mut r)));
 let render_words=cb.object_words;let render_commands=cb.command_words;
 t.sectors=2;check(gpu_builder_render(view_of(&mut cb),view_of(&mut t),view_of(&mut r)));
 check(cb.object_words==render_words&&cb.command_words==render_commands);
 let mut matrix:Buf<u16,42>=Buf::new([0x8001;42]);
 let mut mesh:GpuMesh=GpuMesh{directory_sector:GPU_ASSET_FIRST_SECTOR,directory_word:63,count:17,
 first_sector:GPU_ASSET_FIRST_SECTOR,sectors:128};
 check(gpu_builder_draw(view_of(&mut cb),view_of(&mut mesh),matrix.as_array(),0xcafe,0x1234));
 mesh.count=32;matrix[0u16]=0xffff;
 check(gpu_builder_draw(view_of(&mut cb),view_of(&mut mesh),matrix.as_array(),0x5678,0xabcd));
 mesh.count=33;matrix[0u16]=0;
 check(gpu_builder_draw(view_of(&mut cb),view_of(&mut mesh),matrix.as_array(),0,0));
 let used=cb.command_words;let objects=cb.object_words;
 let mut saved:Buf<u16,128>=Buf::new([0;128]);let mut i:u16=0;
 while i<128 {saved[i]=GPU_CODE[i];i+=1;}
 let staged=GPU_BUILD[2u16];let cursor=GPU_BUILD[5u16];
 mesh.directory_sector=GPU_ASSET_FIRST_SECTOR+128;mesh.first_sector=mesh.directory_sector;mesh.count=17;
 check(!gpu_builder_draw(view_of(&mut cb),view_of(&mut mesh),matrix.as_array(),0xffff,0xffff));
 check(cb.command_words==used&&cb.object_words==objects&&GPU_BUILD[2u16]==staged&&GPU_BUILD[5u16]==cursor);
 i=0;while i<128 {check(saved[i]==GPU_CODE[i]);i+=1;}
 check(gpu_builder_end(view_of(&mut cb)));check(gpu_builder_finish(view_of(&mut cb)));
 halt(0x600d);
}"#;
    let words = compile_direct(source);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&words);
    let state = Rc::new(RefCell::new(PeerState::default()));
    cpu.attach_device(0, Box::new(ControlPeer { state, kind: 0 }));
    for (base, count) in [(0x9600usize, 33usize), (0x9680, 17)] {
        for i in 0..count {
            let at = base * 64 + 63 + i * 2;
            cpu.physical_memory_mut()[at] = (base + 4 + i * 2) as u16;
            cpu.physical_memory_mut()[at + 1] = u16::from(base == 0x9600 || i != 16);
        }
    }
    assert!(matches!(
        cpu.run(3_000_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    let arena = bytes(&cpu, 0x490000 / 2, 2048);
    let draws = arena
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .filter(|word| word >> 28 == 3)
        .count();
    assert_eq!(draws, 17 + 32 + 33);
    // The rejected draw must not upload its Instance into the next allocation.
    assert_eq!(bytes(&cpu, (0x4a0000 + 640) / 2, 128), [0; 128]);
}

#[test]
fn direct_append_crosses_window_and_reaches_exact_capacity() {
    for cursor in [16380u32, 32762] {
        let source = format!(
            r#"mod gpu_device;
fn check(ok:bool){{if !ok{{halt(0xbad);}}}}
fn main(){{
 unsafe{{write_dseg_page(3,19);}}
 let mut cb:GpuBuffer=gpu_buffer_empty();check(gpu_high_slot_init(view_of(&mut cb),0));
 check(gpu_builder_begin(view_of(&mut cb),GPU_COLOR_B_SECTOR,GPU_DEPTH_SECTOR,0,0));
 check(read_dseg_page(3)==19);
 GPU_BUILD[5u16]={cursor};cb.command_words={cursor};
 check(gpu_builder_end(view_of(&mut cb)));check(read_dseg_page(3)==19);
 check(gpu_builder_finish(view_of(&mut cb)));check(read_dseg_page(3)==19);
 check(cb.command_words=={cursor}+6);halt(0x600d);
}}"#
        );
        let words = compile_direct(&source);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&words);
        cpu.physical_memory_mut()[0x490000 / 2..0x4a0000 / 2].fill(0xa55a);
        assert!(matches!(
            cpu.run(500_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        assert_eq!(
            bytes(&cpu, 0x490000 / 2 + cursor, 12),
            [3, 0, 0, 0x40, 0, 0, 0, 0x70, 0, 0, 0, 0xf0]
        );
        let padded = (cursor + 6 + 63) & !63;
        assert_eq!(
            bytes(
                &cpu,
                0x490000 / 2 + cursor + 6,
                ((padded - cursor - 6) * 2) as usize
            ),
            vec![0; ((padded - cursor - 6) * 2) as usize]
        );
    }
}

#[test]
fn batched_mesh_commands_preserve_prefetch_order_across_window() {
    let mut expected = Vec::new();
    for i in 0..17u32 {
        let slot = (i & 1) as u8;
        expected.push(GpuCommand::dma_meshlet((0x9700 + i * 2) * 128, slot, 128).unwrap());
        if i != 0 {
            expected.push(GpuCommand::draw_meshlet(slot ^ 1).unwrap());
        }
        expected.push(GpuCommand::wait(true, false));
    }
    expected.push(GpuCommand::draw_meshlet(0).unwrap());
    let expected: Vec<u8> = expected.iter().flat_map(|c| c.0.to_le_bytes()).collect();
    for cursor in [16370u32, 16374, 16378, 16380] {
        let source = format!(
            r#"mod gpu_device;
fn check(ok:bool){{if !ok{{halt(0xbad);}}}}
fn main(){{
 unsafe{{write_dseg_page(3,19);}}
 let mut cb:GpuBuffer=gpu_buffer_empty();check(gpu_high_slot_init(view_of(&mut cb),0));
 check(gpu_builder_begin(view_of(&mut cb),GPU_COLOR_B_SECTOR,GPU_DEPTH_SECTOR,0,0));
 let mut entries:Buf<u16,34>=Buf::new([0;34]);let mut i:u16=0;
 while i<17 {{entries[i*2]=0x9700+i*2;entries[i*2+1]=1;i+=1;}}
 GPU_BUILD[5u16]={cursor};cb.command_words={cursor};
 check(gpu_builder_commands(view_of(&mut cb)));
 check(gpu_builder_mesh_commands(view_of(&mut cb),entries.as_array(),0,16));
 check(gpu_builder_mesh_commands(view_of(&mut cb),unsafe{{entries.as_ptr().add(32).as_u16_array()}},16,1));
 check(gpu_builder_emit(view_of(&mut cb),0,0x3000));
 check(GPU_BUILD[5u16]=={cursor}+102);
 check(gpu_builder_restore(19,true));check(read_dseg_page(3)==19);halt(0x600d);
}}"#
        );
        let words = compile_direct(&source);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&words);
        cpu.physical_memory_mut()[0x490000 / 2..0x4a0000 / 2].fill(0xa55a);
        assert!(matches!(
            cpu.run(500_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        assert_eq!(bytes(&cpu, 0x490000 / 2 + cursor, expected.len()), expected);
        assert_eq!(bytes(&cpu, 0x490000 / 2 + cursor - 1, 2), [0x5a, 0xa5]);
        assert_eq!(bytes(&cpu, 0x490000 / 2 + cursor + 102, 2), [0x5a, 0xa5]);
    }
}

#[test]
fn inplace_ui_crosses_window_preserves_neighbors_and_rejects_atomically() {
    for fill in [255u16, 42] {
        let source = format!(
            r#"mod gpu;
fn check(ok:bool){{if !ok{{halt(0xbad);}}}}
fn main(){{
 unsafe{{write_dseg_page(3,19);}}
 check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();let pass=gpu_pass_begin(cb,0,65535);check(pass==1);
 let mut atlas:GpuUiAtlas=GpuUiAtlas{{sector:0x9800,sectors:512}};
 let mut ids:Buf<u16,4>=Buf::new([49,50,51,256]);
 GPU_CB.object_words=16320;let commands=GPU_CB.command_words;
 check(gpu_pass_ui_begin(0,view_of(&mut atlas),{fill})==0);
 check(gpu_pass_ui_begin(pass,view_of(&mut atlas),256)==0);
 atlas.sector=0x9801;check(gpu_pass_ui_begin(pass,view_of(&mut atlas),{fill})==0);atlas.sector=0x9800;
 GPU_CB.object_words=32368;check(gpu_pass_ui_begin(pass,view_of(&mut atlas),{fill})==0);
 GPU_CB.object_words=16320;GPU_CB.command_words=32760;
 check(gpu_pass_ui_begin(pass,view_of(&mut atlas),{fill})==0);GPU_CB.command_words=commands;
 check(GPU_CB.object_words==16320&&GPU_CB.command_words==commands&&GPU_BUILD[1u16]==1);
 let ui=gpu_pass_ui_begin(pass,view_of(&mut atlas),{fill});check(ui==pass);
 check(read_dseg_page(3)==19);check(GPU_CB.object_words==16320&&GPU_CB.command_words==commands);
 check(gpu_pass_ui_begin(pass,view_of(&mut atlas),{fill})==0);
 check(!gpu_pass_end(pass));check(!gpu_cb_finish(cb));check(!gpu_pass_clear(cb,0,0));
 check(!gpu_pass_ui_write(ui,127,ids.as_array(),4));
 check(!gpu_pass_ui_write(ui,749,ids.as_array(),2));
 check(!gpu_pass_ui_write(ui,751,ids.as_array(),0));
 check(!gpu_pass_ui_write(ui,0,unsafe{{Ptr::from_addr(0xbfff).as_u16_array()}},2));
 check(gpu_pass_ui_write(ui,750,ids.as_array(),0));
 check(gpu_pass_ui_write(ui,127,ids.as_array(),3));
 ids[0u16]=0;check(gpu_pass_ui_write(ui,749,ids.as_array(),1));
 check(read_dseg_page(3)==19);check(gpu_pass_ui_end(ui));
 check(GPU_CB.object_words==16720&&GPU_CB.command_words==commands+10);
 check(!gpu_pass_ui_end(ui));check(!gpu_pass_ui_write(ui,0,ids.as_array(),1));
 check(gpu_pass_end(pass));check(gpu_cb_finish(cb));check(read_dseg_page(3)==19);
 halt(0x600d);
}}"#
        );
        let words = compile(&source);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&words);
        cpu.physical_memory_mut()[0x4a0000 / 2..0x4b0000 / 2].fill(0xa55a);
        let state = Rc::new(RefCell::new(PeerState::default()));
        for kind in [0, 3, 4] {
            cpu.attach_device(
                kind,
                Box::new(ControlPeer {
                    state: state.clone(),
                    kind,
                }),
            );
        }
        let result = cpu.run(2_000_000).unwrap();
        assert!(
            matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
            "fill={fill} {result:?}"
        );
        let at = 0x4a0000 / 2 + 16320;
        let mut expected = vec![fill | (fill << 8); 375];
        for (cell, id) in [(127usize, 49u16), (128, 50), (129, 51), (749, 0)] {
            let old = expected[cell / 2];
            expected[cell / 2] = if cell % 2 == 0 {
                (old & 0xff00) | id
            } else {
                (old & 0xff) | (id << 8)
            };
        }
        expected.extend([0xffff; 9]);
        expected.extend([0x94ff, 0x4c]);
        expected.extend([0; 14]);
        let expected: Vec<u8> = expected.iter().flat_map(|x| x.to_le_bytes()).collect();
        assert_eq!(bytes(&cpu, at, 800), expected);
        assert_eq!(bytes(&cpu, at - 1, 2), [0x5a, 0xa5]);
        assert_eq!(bytes(&cpu, at + 400, 2), [0x5a, 0xa5]);
    }
}

#[test]
fn inplace_ui_cancel_discards_unsealed_object_and_stale_operations() {
    let words = compile(
        r#"mod gpu;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();let pass=gpu_pass_begin(cb,0,0);
 let mut atlas:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};
 let mut ids:Buf<u16,1>=Buf::new([65]);
 let ui=gpu_pass_ui_begin(pass,view_of(&mut atlas),255);check(ui!=0);
 check(gpu_cb_cancel(cb));check(!gpu_pass_ui_end(ui));check(!gpu_pass_ui_write(ui,0,ids.as_array(),1));
 let next=gpu_cb_begin();check(next==1);check(gpu_pass_clear(next,0,0));halt(0x600d);
}"#,
    );
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&words);
    let state = Rc::new(RefCell::new(PeerState::default()));
    for kind in [0, 3, 4] {
        cpu.attach_device(
            kind,
            Box::new(ControlPeer {
                state: state.clone(),
                kind,
            }),
        );
    }
    let result = cpu.run(2_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    assert_eq!(state.borrow().starts, 0);
}

#[test]
fn inplace_demo_numbers_have_no_leading_zeros_and_clear_unused_cells() {
    let words = compile(
        r#"mod overlay_gpu;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();let pass=gpu_pass_begin(cb,0,0);
 let mut atlas:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};
 let mut stats:Buf<u16,4>=Buf::new([0;4]);stats[3u16]=1;
 stats[1u16]=0;check(demo_ui_record(pass,view_of(&mut atlas),stats.as_array()));
 stats[1u16]=9;check(demo_ui_record(pass,view_of(&mut atlas),stats.as_array()));
 stats[1u16]=10;check(demo_ui_record(pass,view_of(&mut atlas),stats.as_array()));
 stats[1u16]=65535;check(demo_ui_record(pass,view_of(&mut atlas),stats.as_array()));
 stats[3u16]=0;check(demo_ui_record(pass,view_of(&mut atlas),stats.as_array()));
 check(gpu_pass_end(pass));check(gpu_cb_finish(cb));halt(0x600d);
}"#,
    );
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&words);
    cpu.physical_memory_mut()[0x4a0000 / 2..0x4b0000 / 2].fill(0xa55a);
    let state = Rc::new(RefCell::new(PeerState::default()));
    for kind in [0, 3, 4] {
        cpu.attach_device(
            kind,
            Box::new(ControlPeer {
                state: state.clone(),
                kind,
            }),
        );
    }
    let result = cpu.run(2_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    for (i, text) in ["0", "9", "10", "65535", ""].iter().enumerate() {
        let mut cells = [255u8; 750];
        cells[52..52 + text.len()].copy_from_slice(text.as_bytes());
        let word = 64 + i as u32 * 448;
        assert_eq!(bytes(&cpu, 0x4a0000 / 2 + word, 750), cells);
        assert_eq!(bytes(&cpu, 0x4a0000 / 2 + word + 375, 18), [255; 18]);
    }
}
