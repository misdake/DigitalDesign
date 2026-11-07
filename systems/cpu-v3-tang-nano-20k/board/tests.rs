use super::*;
use cpu_v3_tang_nano_20k::boot::pack_single_application;
use digital_design_hardware::{ResourceKind, VerilogProject};
#[test]
#[ignore = "bounded CPU-only application, real SPI, caches, arbiter, gear, native controller, SDRAM pins and Display"]
fn extended_board_application_with_native_sdram_in_verilog() {
    BOARD_APPLICATION_TEST.with(|x| x.set(true));
    let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
    BOARD_APPLICATION_TEST.with(|x| x.set(false));
    result.unwrap();
}

#[test]
#[ignore = "bounded cold SPI/code/static load, virtual-black Display, actual CPU Device4 and native GPU"]
fn extended_cold_boot_cpu_submits_gpu_commands_in_verilog() {
    let source = r#"
            fn main() {
                if dev_recv(4,2)!=0 { halt(0xff01); }
                dev_send(4,0,3);
                dev_send(4,1,0x400);
                let mut status=dev_recv(4,2);
                while (status&1)!=0 { status=dev_recv(4,2); }
                if status!=0 { halt(0xff02); }
                halt(0x5a36);
            }
        "#;
    let mut options = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = 0xc000;
    let parsed = rcc::frontend::compile_program_named("gpu-cold.rs", source, &options, &mut |_| {
        Err("unexpected module".into())
    })
    .unwrap();
    let static_words = parsed.debug.static_data_end;
    let program = cpu_v3::rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let code: Vec<u8> = program.words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut commands: Vec<u8> = [0u32, 0x40000003, 0x70000000, 0xf0000000]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    commands.resize(128, 0);
    let package = cpu_v3_tang_nano_20k::boot::pack_application_resources(
        &code,
        static_words,
        &[cpu_v3_tang_nano_20k::boot::BootResource::copy(
            0x20000, commands,
        )],
    )
    .unwrap();
    // Keep the production packer's complete byte stream. Virtual-black
    // startup leaves both framebuffer slots poisoned until first present.
    TEST_IMAGE.with(|x| *x.borrow_mut() = Some((package, false, 0x5a36)));
    GPU_COLD_TEST.with(|x| x.set(true));
    let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
    GPU_COLD_TEST.with(|x| x.set(false));
    TEST_IMAGE.with(|x| *x.borrow_mut() = None);
    result.unwrap();
}
#[test]
#[ignore = "bounded native SDRAM source-dirty LCOPY followed by full clean/invalidate"]
fn extended_native_sdram_maintenance_probe_in_verilog() {
    let source = r#"
            fn fail(status: u16) { dev_send(0,2,status); halt(0xff01); }
            fn invalidate() {
                unsafe { dcache_clean_line(Ptr::from_addr(0x4000)); } dcache_wait();
                if dcache_invalidate_all()!=0 { fail(1); }
            }
            fn main() {
                unsafe { write_dseg_page(3,128); }
                let mut line: u16=0;
                while line<256 {
                    let p=Ptr::from_addr(0x4000+(line<<4));
                    let mut i: u16=0;
                    while i<16 { unsafe { p.write(i as i16,0x1234); } i+=1; }
                    unsafe { dcache_line_copy(p,Ptr::from_addr(0xc000+(line<<4))); } dcache_wait(); line+=1;
                }
                invalidate();
                halt(0x5a35);
            }
        "#;
    let options = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = rcc::frontend::compile_program_named(
        "native-maintenance-probe.rs",
        source,
        &options,
        &mut |_| Err("unexpected module".into()),
    )
    .unwrap();
    let static_words = parsed.debug.static_data_end;
    let program = cpu_v3::rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/system-board-ready/audit");
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(
        evidence.join("native-maintenance-probe.lst"),
        &program.listing,
    )
    .unwrap();
    std::fs::write(evidence.join("native-maintenance-probe.rcc"), source).unwrap();
    let bytes: Vec<_> = program.words.iter().flat_map(|x| x.to_le_bytes()).collect();
    let package = pack_single_application(&bytes, static_words).unwrap();
    TEST_IMAGE.with(|x| *x.borrow_mut() = Some((package, false, 0x5a35)));
    BOARD_APPLICATION_TEST.with(|x| x.set(true));
    let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
    BOARD_APPLICATION_TEST.with(|x| x.set(false));
    TEST_IMAGE.with(|x| *x.borrow_mut() = None);
    result.unwrap();
}
fn fixture() -> Vec<u8> {
    let source = include_str!("../rcc/boot-contract.rs");
    let mut options = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = 0xc000;
    let program =
        rcc::frontend::compile_program_named("boot-contract.rs", source, &options, &mut |name| {
            Err(format!("unexpected module {name}"))
        })
        .unwrap();
    let static_words = program.debug.static_data_end;
    let program = cpu_v3::rcc_backend::try_compile(program, &options, "main").unwrap();
    let bytes: Vec<u8> = program.words.iter().flat_map(|x| x.to_le_bytes()).collect();
    pack_single_application(&bytes, static_words).unwrap()
}
#[test]
fn single_application_assets_repack_exactly() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = cpu_v3_tang_nano_20k::boot::parse_boot_project(
        &std::fs::read_to_string(root.join("boot-applications.conf")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        cpu_v3_tang_nano_20k::boot::pack_application_resources(
            cpu_v3_application::asset("application.v3bin").unwrap(),
            std::str::from_utf8(cpu_v3_application::asset("application.static-words").unwrap())
                .unwrap()
                .trim()
                .parse()
                .unwrap(),
            &project.resources(root).unwrap()
        )
        .unwrap(),
        cpu_v3_application::package()
    );
}
#[test]
fn application_updates_do_not_change_gateware() {
    let before = VerilogProject::generate::<CpuV3System>().unwrap().files;
    let synthesis = VerilogProject::generate_for_synthesis::<CpuV3System>().unwrap();
    assert_eq!(before, synthesis.files);
    TEST_IMAGE.with(|x| *x.borrow_mut() = Some((fixture(), false, 0x5a35)));
    let after = VerilogProject::generate::<CpuV3System>().unwrap().files;
    TEST_IMAGE.with(|x| *x.borrow_mut() = None);
    assert_eq!(before, after);
}
#[test]
fn full_system_omits_rom_overlay_dma_and_runtime_reset() {
    let project = VerilogProject::generate::<CpuV3System>().unwrap();
    let text = project
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !text.contains("BootDmaEngine")
            && !text.contains("ResetController")
            && !text.contains("instruction_source_count")
            && !text.contains("u_boot (")
    );
    let board = gowin_project().generate().unwrap();
    assert_eq!(board.resources.claimed[&ResourceKind::Bsram18K], 46);
    assert_eq!(board.resources.claimed[&ResourceKind::Multiplier18x18], 42);
    assert_eq!(board.resources.claimed[&ResourceKind::UserLed], 2);
    assert_eq!(board.resources.claimed[&ResourceKind::Pll], 2);
    assert_eq!(board.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
    assert_eq!(board.resources.claimed[&ResourceKind::SpiFlashDevice], 1);
    let cst = &board.files[std::path::Path::new("src/generated/board.cst")];
    let mut pins = std::collections::BTreeMap::new();
    for line in cst.lines().filter(|line| line.starts_with("IO_LOC ")) {
        let parts: Vec<_> = line.split_whitespace().collect();
        let pin = parts[2].trim_end_matches(';').parse::<u16>().unwrap();
        assert!(
            pins.insert(pin, parts[1].trim_matches('"')).is_none(),
            "duplicate CST pin {pin}"
        );
    }
    for (pin, name) in [
        (15, "leds[0]"),
        (16, "leds[1]"),
        (17, "ds2_clk"),
        (20, "ds2_cmd"),
        (19, "ds2_dat"),
        (18, "ds2_att"),
    ] {
        assert_eq!(pins[&pin], name);
    }
    assert!(!cst.contains("leds[2]") && !cst.contains("leds[5]"));
    assert!(cst.contains("IO_PORT \"ds2_dat\" IO_TYPE=LVCMOS33 PULL_MODE=UP;"));
    assert!(text.contains(".dat(ds2_dat),.ds2_clk(ds2_clk),.cmd(ds2_cmd),.att(ds2_att)"));
    assert!(text.contains("gpu_read_data | ds2_read_data"));
    let top = &board.files[std::path::Path::new("src/generated/board_top.v")];
    assert!(top.contains("input wire ds2_dat") && !top.contains("output wire ds2_dat"));
    assert!(top.contains("#(.COLD_START_ONLY(1)) u_sdram_bridge"));
    assert!(top.contains(".PHASE_SEL(\"0111\")"));
    assert!(top.contains(".request_continuation(sdram_request_continuation)"));
    assert!(top.contains(".write_done(sdram_write_done)"));
    assert!(text.contains(".cpu_chain_class(memory_chain_class)"));
    assert!(text.contains("controller_request_continuation"));
    assert!(text.contains("Ordered two-descriptor endpoint"));
    assert!(!text.contains("EARLY_GRANT") && !text.contains("CHAIN_GROUP_FOUR"));
    let bridge = &board.files
        [std::path::Path::new("src/generated/target/tang_nano_20k/sdram/native_bridge_108m_54m.v")];
    assert!(bridge.contains(".CONTINUE(1),.NEXT_TAGGED(1)"));
    assert!(bridge.contains("REGISTER_READ_RETURN=1, DQ_INPUT_DELAY_TAPS=32"));
    assert!(bridge.contains("CAS=3, RCD_CYCLES=3, RP_CYCLES=3, CAPTURE_LATENCY=3"));
    assert!(bridge.contains(".DQ_INPUT_DELAY_TAPS(DQ_INPUT_DELAY_TAPS)"));
    let sdc = &board.files[std::path::Path::new("src/generated/board.sdc")];
    assert!(sdc.contains("-phase 157.5"));
    assert!(sdc.contains("-source -early 3.75"));
    assert!(sdc.contains("-source -late 6.25"));
    let build = &board.files[std::path::Path::new("build.tcl")];
    assert!(build.contains("set_option -route_option 2"));
    assert!(build.contains("set_option -gen_sdf 1"));
}
#[test]
#[ignore = "complete real-SPI production application cold boot"]
fn extended_flash_boot_executes_in_verilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>().unwrap();
}
#[test]
#[ignore = "complete real-SPI nonzero memory / zero-and-nonzero-static fixture"]
fn extended_flash_boot_statics_in_verilog() {
    TEST_IMAGE.with(|x| *x.borrow_mut() = Some((fixture(), false, 0x5a35)));
    let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
    TEST_IMAGE.with(|x| *x.borrow_mut() = None);
    result.unwrap();
}
#[test]
#[ignore = "bounded real SPI full-system 2MiB resource load and byte comparison"]
fn extended_flash_boot_large_resources_in_verilog() {
    let base = fixture();
    let code = cpu_v3_tang_nano_20k::boot::single_boot_code(&base).unwrap();
    let data = (0..0x200000).map(|n| (n ^ (n >> 8) ^ 0x53) as u8).collect();
    let package = cpu_v3_tang_nano_20k::boot::pack_application_resources(
        code,
        cpu_v3_tang_nano_20k::boot::single_boot_commands(&base).unwrap()[0].length / 2,
        &[cpu_v3_tang_nano_20k::boot::BootResource::copy(
            0x100000, data,
        )],
    )
    .unwrap();
    TEST_IMAGE.with(|x| *x.borrow_mut() = Some((package, false, 0x5a35)));
    let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
    TEST_IMAGE.with(|x| *x.borrow_mut() = None);
    result.unwrap();
}
#[test]
#[ignore = "malformed image must never release CPU or write a device"]
fn flash_boot_rejects_header_in_verilog() {
    let mut bad = fixture();
    bad[0] ^= 1;
    TEST_IMAGE.with(|x| *x.borrow_mut() = Some((bad, true, 0)));
    let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
    TEST_IMAGE.with(|x| *x.borrow_mut() = None);
    result.unwrap();
}
#[test]
#[ignore = "real SPI must reject an overflowing ZERO before any aliased DRAM write"]
fn flash_boot_rejects_wrapping_zero_in_verilog() {
    let mut bad = fixture();
    bad[12..16].copy_from_slice(&0x7ffffeu32.to_le_bytes());
    bad[16..20].copy_from_slice(&0x80c00002u32.to_le_bytes());
    TEST_IMAGE.with(|x| *x.borrow_mut() = Some((bad, true, 0)));
    TEST_ERROR_CODE.with(|x| x.set(2));
    let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
    TEST_IMAGE.with(|x| *x.borrow_mut() = None);
    TEST_ERROR_CODE.with(|x| x.set(1));
    result.unwrap();
}
