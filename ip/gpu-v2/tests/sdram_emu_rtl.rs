use digital_design_hardware_gowin::sdram_memory_controller::{arbiter::*, combination};
use digital_design_hardware_gowin::sdram_memory_controller::{
    emu::{self, Combination},
    ports::*,
};
use digital_design_hardware_gowin::{ModuleIo, VerilogIoValue, VerilogProject};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn data(beat: usize) -> u64 {
    0x9876000012340000u64 ^ (beat as u64 * 0x100000001)
}
fn image() -> OracleImage {
    OracleImage::filled::<0xa5>(0, 16384).unwrap()
}
fn ready_inputs(
) -> digital_design_hardware_gowin::sdram_memory_controller::arbiter::CpuV3MemoryArbiterInputValue {
    let mut i = emu::idle_inputs();
    i.display_response_ready = true;
    i.instruction_response_ready = true;
    i.data_response_ready = true;
    i.dma_response_ready = true;
    i
}
#[test]
fn independent_cycle_combination_roundtrip_and_guards() {
    let mut c = Combination::new(image(), 32).unwrap();
    let mut i = ready_inputs();
    i.reset = true;
    c.tick(&i).unwrap();
    i.reset = false;
    for bytes in [32, 64, 128] {
        let mut accepted = false;
        let mut fed = 0;
        let mut complete = false;
        for _ in 0..2000 {
            i.gpu_fb_w_request_valid = !accepted;
            i.gpu_fb_w_write = true;
            i.gpu_fb_w_address = 0x1000 / 2;
            i.gpu_fb_w_line_count_minus_1 = (bytes / 32 - 1) as u64;
            i.gpu_fb_w_write_data = data(fed);
            let o = c.output(&i);
            if o.gpu_fb_w_request_ready {
                accepted = true;
            }
            if o.gpu_fb_w_write_data_ready {
                fed += 1;
            }
            if o.gpu_fb_w_response_valid {
                assert!(!o.gpu_fb_w_error);
                complete = true;
            }
            c.tick(&i).unwrap();
            if complete {
                break;
            }
        }
        assert!(accepted && complete);
        assert_eq!(fed, bytes / 8);
        i.gpu_fb_w_request_valid = false;
        let mut accepted = false;
        let mut got = Vec::new();
        let mut complete = false;
        for _ in 0..2000 {
            i.gpu_fb_r_request_valid = !accepted;
            i.gpu_fb_r_address = 0x1000 / 2;
            i.gpu_fb_r_line_count_minus_1 = (bytes / 32 - 1) as u64;
            let o = c.output(&i);
            if o.gpu_fb_r_request_ready {
                accepted = true;
            }
            if o.gpu_fb_r_response_valid {
                assert!(!o.gpu_fb_r_error);
                got.push(o.gpu_fb_r_read_data);
                complete = o.gpu_fb_r_response_last;
            }
            c.tick(&i).unwrap();
            if complete {
                break;
            }
        }
        assert!(complete);
        assert_eq!(got, (0..bytes / 8).map(data).collect::<Vec<_>>());
        i.gpu_fb_r_request_valid = false;
        assert!(c.bridge.pins.bytes()[..4096].iter().all(|&b| b == 0xa5));
        assert!(c.bridge.pins.bytes()[4096 + 128..]
            .iter()
            .all(|&b| b == 0xa5));
    }
}

fn observe(
    c: &mut Combination,
    i: &CpuV3MemoryArbiterInputValue,
    rows: &mut Vec<(Vec<VerilogIoValue>, Vec<VerilogIoValue>)>,
) -> CpuV3MemoryArbiterOutputValue {
    let o = c.output(i);
    rows.push((
        CpuV3MemoryArbiterInput::verilog_values(i),
        CpuV3MemoryArbiterOutput::verilog_values(&o),
    ));
    c.tick(i).unwrap();
    o
}
fn transcript() -> Vec<(Vec<VerilogIoValue>, Vec<VerilogIoValue>)> {
    let mut c = Combination::new(image(), 32).unwrap();
    let mut i = ready_inputs();
    let mut rows = Vec::new();
    i.reset = true;
    for _ in 0..3 {
        observe(&mut c, &i, &mut rows);
    }
    i.reset = false;
    for address in [0x1000, 0x1080, 0x2000, 0x1000] {
        for bytes in [32, 64, 128] {
            let mut accepted = false;
            let mut fed = 0;
            let mut done = false;
            for _ in 0..2000 {
                i.gpu_fb_w_request_valid = !accepted;
                i.gpu_fb_w_write = true;
                i.gpu_fb_w_address = address / 2;
                i.gpu_fb_w_line_count_minus_1 = bytes / 32 - 1;
                i.gpu_fb_w_write_data = data(fed);
                let o = observe(&mut c, &i, &mut rows);
                accepted |= o.gpu_fb_w_request_ready;
                if o.gpu_fb_w_write_data_ready {
                    fed += 1;
                }
                done |= o.gpu_fb_w_response_valid;
                if done {
                    break;
                }
            }
            assert!(done);
            assert_eq!(fed, bytes as usize / 8);
            i.gpu_fb_w_request_valid = false;
            let mut accepted = false;
            let mut got = Vec::new();
            let mut done = false;
            for _ in 0..2000 {
                i.gpu_fb_r_request_valid = !accepted;
                i.gpu_fb_r_address = address / 2;
                i.gpu_fb_r_line_count_minus_1 = bytes / 32 - 1;
                let o = observe(&mut c, &i, &mut rows);
                accepted |= o.gpu_fb_r_request_ready;
                if o.gpu_fb_r_response_valid {
                    got.push(o.gpu_fb_r_read_data);
                    done = o.gpu_fb_r_response_last;
                }
                if done {
                    break;
                }
            }
            assert!(done);
            assert_eq!(got, (0..bytes as usize / 8).map(data).collect::<Vec<_>>());
            i.gpu_fb_r_request_valid = false;
        }
    }
    // Invalid length is a real error response, not a host pre-validation shortcut.
    i.gpu_ro_request_valid = true;
    i.gpu_ro_line_count_minus_1 = 2;
    let mut accepted = false;
    let mut error = false;
    for _ in 0..200 {
        i.gpu_ro_request_valid = !accepted;
        let o = observe(&mut c, &i, &mut rows);
        accepted |= o.gpu_ro_request_ready;
        error |= o.gpu_ro_error;
        if error {
            break;
        }
    }
    assert!(error);
    i.gpu_ro_request_valid = false;
    // Scalar halfword lanes, fixed-length CPU/display reads, and final sink stall.
    for lane in 0..2 {
        i.dma_request_valid = true;
        i.dma_write = true;
        i.dma_address = 0x3000 / 2 + lane;
        i.dma_write_data = 0x1357 + lane;
        let mut accepted = false;
        let mut done = false;
        for _ in 0..2000 {
            i.dma_request_valid = !accepted;
            let o = observe(&mut c, &i, &mut rows);
            accepted |= o.dma_request_ready;
            done |= o.dma_response_valid;
            if done {
                break;
            }
        }
        assert!(done);
        i.dma_request_valid = false;
        let mut accepted = false;
        let mut done = false;
        i.dma_write = false;
        i.dma_response_ready = false;
        for n in 0..2000 {
            i.dma_request_valid = !accepted;
            i.dma_response_ready = n > 80;
            let o = observe(&mut c, &i, &mut rows);
            accepted |= o.dma_request_ready;
            if o.dma_response_valid {
                assert_eq!(o.dma_read_data, 0x1357 + lane);
                done = i.dma_response_ready;
            }
            if done {
                break;
            }
        }
        assert!(done);
        i.dma_request_valid = false;
    }
    for _ in 0..650 {
        observe(&mut c, &i, &mut rows);
    } // crosses refresh threshold
    assert!(c.bridge.pins.refreshes > 2);
    // Reset aborts outstanding ownership; subsequent initialization preserves image.
    i.gpu_ro_request_valid = true;
    i.gpu_ro_line_count_minus_1 = 0;
    i.gpu_ro_address = 0x1000 / 2;
    for _ in 0..4 {
        observe(&mut c, &i, &mut rows);
    }
    i.gpu_ro_request_valid = false;
    i.reset = true;
    for _ in 0..4 {
        observe(&mut c, &i, &mut rows);
    }
    i.reset = false;
    for _ in 0..50 {
        let o = observe(&mut c, &i, &mut rows);
        assert!(!o.gpu_ro_response_valid);
    }
    rows
}
fn bounded(mut command: Command, root: &Path, name: &str) {
    let stdout = fs::File::create(root.join(format!("{name}.out"))).unwrap();
    let stderr = fs::File::create(root.join(format!("{name}.err"))).unwrap();
    let mut child = command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                fs::read_to_string(root.join(format!("{name}.out"))).unwrap(),
                fs::read_to_string(root.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{name} wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
#[test]
fn cycle_transcript_has_scalar_mask_refresh_reset_and_bounded_work() {
    assert!(transcript().len() < 10000);
}
#[test]
fn facade_uses_real_arbitration_and_four_sector_data() {
    use emu::service::{Config, Memory};
    let mut m = Memory::new(
        image(),
        Config {
            init_cycles: 32,
            ..Default::default()
        },
    )
    .unwrap();
    // Reject unsupported masks before accepting an ID, rather than dropping enables.
    assert!(m
        .submit(
            Client::FramebufferWrite,
            Request::Write {
                address: 0,
                data: vec![OracleWord::constant::<0>(); 4],
                enables: vec![1; 4]
            }
        )
        .is_err());
    let display = m
        .submit(
            Client::Display,
            Request::Read {
                address: 0,
                bytes: 32,
            },
        )
        .unwrap();
    let write = m
        .submit(
            Client::FramebufferWrite,
            Request::Write {
                address: 4096,
                data: (0..64)
                    .map(|n| unsafe { OracleWord::from_host(data(n), "pin source").unwrap() })
                    .collect(),
                enables: vec![255; 64],
            },
        )
        .unwrap();
    let mut order = Vec::new();
    let mut done = Vec::new();
    for _ in 0..20000 {
        for e in m.step().unwrap() {
            match e {
                Event::Started { id, .. } => order.push(id),
                Event::Complete { id, .. } => done.push(id),
                _ => {}
            }
        }
        if m.idle() {
            break;
        }
    }
    assert_eq!(order, vec![display, write]);
    assert_eq!(done.len(), 2);
    for client in Client::ALL {
        let id = m
            .submit(
                client,
                Request::Read {
                    address: 4096,
                    bytes: 512,
                },
            )
            .unwrap();
        let mut got = Vec::new();
        let mut completed = false;
        for _ in 0..20000 {
            for e in m.step().unwrap() {
                match e {
                    Event::ReadBeat {
                        id: who,
                        data,
                        index,
                        last,
                        ..
                    } => {
                        assert_eq!(who, id);
                        assert_eq!(index, got.len());
                        got.push(data.bits());
                        assert_eq!(last, index == 63);
                    }
                    Event::Complete { .. } => completed = true,
                    _ => {}
                }
            }
            if m.idle() {
                break;
            }
        }
        assert!(completed);
        assert_eq!(got, (0..64).map(data).collect::<Vec<_>>());
    }
    assert!(m.bytes()[..4096].iter().all(|&b| b == 0xa5));
    assert!(m.bytes()[4608..].iter().all(|&b| b == 0xa5));
}
#[test]
fn display_arrival_between_sector_requests_and_reset_abort_are_live() {
    use emu::service::{Config, Memory};
    let mut m = Memory::new(
        image(),
        Config {
            init_cycles: 32,
            ..Default::default()
        },
    )
    .unwrap();
    let gpu = m
        .submit(
            Client::FramebufferRead,
            Request::Read {
                address: 4096,
                bytes: 512,
            },
        )
        .unwrap();
    let mut display = None;
    let mut history = Vec::new();
    for _ in 0..10000 {
        let events = m.step().unwrap();
        if display.is_none()
            && events
                .iter()
                .any(|e| matches!(e,Event::ReadBeat{id,index:2,..}if *id==gpu))
        {
            display = Some(
                m.submit(
                    Client::Display,
                    Request::Read {
                        address: 0,
                        bytes: 32,
                    },
                )
                .unwrap(),
            );
        }
        history.extend(events);
        if m.idle() {
            break;
        }
    }
    let display = display.unwrap();
    let first_sector_end = history
        .iter()
        .position(|e| matches!(e,Event::ReadBeat{id,index:15,..}if *id==gpu))
        .unwrap();
    let display_start = history
        .iter()
        .position(|e| matches!(e,Event::Started{id,..}if *id==display))
        .unwrap();
    let second_sector_first = history
        .iter()
        .position(|e| matches!(e,Event::ReadBeat{id,index:16,..}if *id==gpu))
        .unwrap();
    assert!(first_sector_end < display_start && display_start < second_sector_first);
    let doomed = m
        .submit(
            Client::FramebufferRead,
            Request::Read {
                address: 4096,
                bytes: 512,
            },
        )
        .unwrap();
    for _ in 0..5 {
        m.step().unwrap();
    }
    m.reset().unwrap();
    assert!(m.idle());
    let live = m
        .submit(
            Client::Display,
            Request::Read {
                address: 0,
                bytes: 32,
            },
        )
        .unwrap();
    assert!(live > doomed);
    let mut done = false;
    for _ in 0..1000 {
        for e in m.step().unwrap() {
            if let Event::Complete { id, .. } = e {
                assert_eq!(id, live);
                done = true;
            }
        }
        if m.idle() {
            break;
        }
    }
    assert!(done);
}

#[test]
fn gearbox_waits_for_initial_payload_and_detects_stream_underrun() {
    let mut e = emu::bridge::Engine::new(image(), 32).unwrap();
    let mut i = emu::bridge::Input {
        valid: true,
        writing: true,
        address: 1024,
        words: 32,
        ..Default::default()
    };
    let mut accepted = false;
    for _ in 0..100 {
        accepted |= e.output(false).ready;
        e.tick(i).unwrap();
        if accepted {
            break;
        }
    }
    assert!(accepted);
    i.valid = false;
    for _ in 0..20 {
        e.tick(i).unwrap();
        assert!(!e.output(false).done);
    }
    i.data_valid = true;
    i.data = data(0);
    assert!(e.output(false).data_ready);
    e.tick(i).unwrap();
    i.data_valid = false;
    let mut failed = false;
    for _ in 0..100 {
        match e.tick(i) {
            Ok(()) => assert!(!e.output(false).done),
            Err(s) => {
                assert!(s.contains("underrun"));
                failed = true;
                break;
            }
        }
    }
    assert!(failed);
}
#[test]
#[ignore = "requires explicit Icarus pin-level emu/RTL validation"]
fn connected_combination_matches_every_logic_edge_in_iverilog() {
    let rows = transcript();
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-v2-sdram/cycle-cosim");
    fs::create_dir_all(&root).unwrap();
    let mut sources = combination::rtl_sources(32).unwrap();
    let ins = rows[0]
        .0
        .iter()
        .filter(|v| !v.name.starts_with("memory_"))
        .cloned()
        .collect::<Vec<_>>();
    let outs = rows[0]
        .1
        .iter()
        .filter(|v| !v.name.starts_with("memory_"))
        .cloned()
        .collect::<Vec<_>>();
    let mut vectors = String::new();
    for (inputs, outputs) in &rows {
        for v in inputs
            .iter()
            .filter(|v| !v.name.starts_with("memory_"))
            .chain(outputs.iter().filter(|v| !v.name.starts_with("memory_")))
        {
            vectors.push_str(&format!("{:x} ", v.value));
        }
        vectors.push('\n');
    }
    fs::write(root.join("vectors.txt"), vectors).unwrap();
    let mut tb=String::from("`timescale 1ns/1ps\nmodule tb;\nreg controller_clk=0,logic_clk=0,phy_clk=0;wire sdram_clk=phy_clk;\nalways #5.051 controller_clk=!controller_clk;always @(posedge controller_clk) logic_clk=!logic_clk;initial begin #13.259;forever begin phy_clk=!phy_clk;#5.051;end end\n");
    for (values, dir) in [(&ins, "reg"), (&outs, "wire")] {
        for v in values {
            let w = if v.width == 1 {
                String::new()
            } else {
                format!("[{}:0] ", v.width - 1)
            };
            tb.push_str(&format!("{dir} {w}{};\n", v.name));
            if dir == "wire" {
                tb.push_str(&format!("reg {w}expected_{};\n", v.name));
            }
        }
    }
    tb.push_str("wire O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n;wire [3:0] O_sdram_dqm;wire [10:0] O_sdram_addr;wire [1:0] O_sdram_ba;wire [31:0] IO_sdram_dq;\nGowinSdramCombination dut(.*);\nLabPinModel #(.PERIOD_NS(10.102),.RETURN_MIN(1),.RETURN_MAX(4)) pin(.sclk(O_sdram_clk),.reset(reset),.cke(O_sdram_cke),.cs(O_sdram_cs_n),.ras(O_sdram_ras_n),.cas(O_sdram_cas_n),.we(O_sdram_wen_n),.dqm(O_sdram_dqm),.a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes());\ninteger f,n,r;initial begin\n");
    for address in (0..16384).step_by(4) {
        tb.push_str(&format!(
            "pin.seed_word(21'h{:x},32'ha5a5a5a5);\n",
            emu::pins::Pins::native_word(address)
        ));
    }
    let scan_names = ins
        .iter()
        .map(|v| v.name.to_string())
        .chain(outs.iter().map(|v| format!("expected_{}", v.name)))
        .collect::<Vec<_>>();
    tb.push_str(&format!("f=$fopen(\"vectors.txt\",\"r\");for(n=0;n<{};n=n+1)begin\nr=$fscanf(f,\"{}\\n\",{});if(r!={})$fatal(1,\"vector parse\");\n@(posedge logic_clk);\n",rows.len(),vec!["%h";scan_names.len()].join(" "),scan_names.join(","),scan_names.len()));
    for v in &outs {
        let condition = if v.name.ends_with("read_data") {
            format!(
                "{}_response_valid && ",
                v.name.trim_end_matches("_read_data")
            )
        } else {
            String::new()
        };
        tb.push_str(&format!("if({condition}{}!==expected_{})$fatal(1,\"row %0d {} got %h expected %h corephase %h\",n,{},expected_{},dut.bridge.u_controller.phase);\n",v.name,v.name,v.name,v.name,v.name));
    }
    tb.push_str("@(negedge logic_clk);end\n$display(\"PASS connected SDRAM cycle comparison\");$finish;end\ninitial begin #3000000;$fatal(1,\"fixture cycle watchdog\");end\nendmodule\n");
    sources.insert("tb.v".into(), tb);
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&root)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp"]);
    for (path, text) in sources {
        let name = path.file_name().unwrap();
        fs::write(root.join(name), text).unwrap();
        compile.arg(name);
    }
    bounded(compile, &root, "compile");
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    run.current_dir(&root).arg("test.vvp");
    bounded(run, &root, "run");
    assert!(fs::read_to_string(root.join("run.out"))
        .unwrap()
        .contains("PASS connected SDRAM cycle comparison"));
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffffffffu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xedb88320
            } else {
                c >> 1
            };
        }
    }
    !c
}
#[test]
fn board_probe_exports_one_memory_pll_without_cpu_flash_or_video() {
    use digital_design_hardware_gowin::sdram_memory_controller::probe::SdramTrafficProbe;
    let p = SdramTrafficProbe::project().generate().unwrap();
    let all = p.files.values().cloned().collect::<Vec<_>>().join("\n");
    assert!(all.contains("TangNano20KSdramPll108M54M u_sdram_pll"));
    assert!(!all.contains("TangNano20KVideoPll u_video_pll"));
    assert!(all.contains("108") && all.contains("54"));
    assert!(all.contains("CpuV3MemoryArbiter"));
    assert!(all.contains("SharedSdramPort"));
}
#[test]
#[ignore = "requires Icarus pin-level traffic workload and physical UART decoding"]
fn board_probe_runs_nine_workloads_and_checks_uart_crc() {
    use digital_design_hardware_gowin::sdram_memory_controller::probe::SdramTrafficProbe;
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-v2-sdram/board-cosim");
    fs::create_dir_all(&root).unwrap();
    let mut p = VerilogProject::generate::<SdramTrafficProbe>().unwrap();
    let tb = <SdramTrafficProbe as digital_design_hardware_gowin::Module>::verilog_testbench()
        .unwrap()
        .replace("SdramTrafficProbe dut", &format!("{} dut", p.top_module));
    p.files.insert("tb.v".into(), tb);
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&root)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp"]);
    for (path, text) in p.files {
        let name = path.file_name().unwrap();
        let text = text
            .replace("WINDOW=1048576, UART_DIV=469", "WINDOW=3000, UART_DIV=4")
            .replace(
                ".BANK_BIT(BANK_BIT)",
                ".BANK_BIT(BANK_BIT),.INIT_CYCLES(32)",
            );
        fs::write(root.join(name), text).unwrap();
        compile.arg(name);
    }
    bounded(compile, &root, "compile");
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    run.current_dir(&root).arg("test.vvp");
    bounded(run, &root, "run");
    let frames = fs::read_to_string(root.join("uart.hex")).unwrap();
    assert_eq!(frames.lines().count(), 9);
    for (mode, line) in frames.lines().enumerate() {
        let bytes = (0..line.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(bytes.len(), 420);
        let words = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| u32::from_le_bytes(*v))
            .collect::<Vec<_>>();
        assert_eq!(words[0], 0x434d4453);
        assert_eq!(words[1], 1 | ((mode as u32) << 8));
        assert_eq!(words[2], 54_000_000);
        assert_eq!(crc32(&bytes[..416]), words[104]);
        assert!(words[4] >= 3000);
        assert!(words[5] <= words[4]);
        assert!((words[6] + words[7]) as u64 <= u64::from(words[4]) * 8);
        assert!(words[8] > 0);
        for client in 0..7 {
            let start = 13 + client * 13;
            assert_eq!(words[start], words[start + 1]);
        }
    }
}
