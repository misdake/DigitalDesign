//! Device-0 wall time: functional host model, production RTL, and real CPU/cache.
use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache, CpuV3Sim, RunOutcome};
use cpu_v3_tang_nano_20k::{boot, hardware::SystemControlDevice};
use digital_design_hardware::{GeneratedVerilogProject, HardwareIdentity, VerilogProject};
use std::{collections::BTreeMap, path::PathBuf, process::Command};

#[test]
fn rcc_time_reads_host_scheduled_functional_time_without_advancing_it() {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = rcc::frontend::compile_program_named(
        "time",
        r#"
        mod device_abi; mod time;
        fn main() {
            let first = time_milliseconds();
            dev_send(SYSTEM_CONTROL_DEVICE,SYSCTL_MILLISECONDS,999);
            let second = time_milliseconds();
            if first != 17 || second != 17 || time_elapsed_ms(65530,11) != 17 { halt(0xbad); }
            halt(0x600d);
        }
    "#,
        &options,
        &mut |name| match name {
            "device_abi" => Ok(include_str!("../../rcc/device_abi.rs").to_owned()),
            "time" => Ok(include_str!("../../rcc/time.rs").to_owned()),
            _ => Err(name.to_owned()),
        },
    )
    .unwrap();
    let code = rcc_backend::try_compile(parsed, &options, "main")
        .unwrap()
        .words;
    let mut cpu = CpuV3Sim::default();
    cpu.attach_device(0, Box::<boot::SystemControlDevice>::default());
    let clock = cpu.device::<boot::SystemControlDevice>(0).unwrap();
    clock.advance_milliseconds(65530);
    clock.advance_milliseconds(23);
    cpu.set_boot_window(&code);
    assert!(matches!(
        cpu.run(10000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn rcc_microsecond_clear_and_decimal_boundaries() {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = rcc::frontend::compile_program_named(
        "microseconds",
        r#"mod device_abi; mod time; mod frame_stats;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 check(time_microseconds()==16789 && time_milliseconds()==16);
 dev_send(SYSTEM_CONTROL_DEVICE,SYSCTL_MICROSECONDS,2);
 check(time_microseconds()==16789);
 time_clear_microseconds();check(time_microseconds()==0 && time_milliseconds()==16);
 let mut s:Buf<u16,4>=Buf::new([0;4]);let mut d:Buf<u16,4>=Buf::new([0;4]);
 frame_stats_elapsed_begin(s.as_array(),100,true);check(s[3u16]==0);
 frame_stats_elapsed_begin(s.as_array(),16789,true);check(s[3u16]==1);
 frame_stats_tenths_digits(s.as_array(),d.as_array());
 check(d[0u16]==49&&d[1u16]==54&&d[2u16]==46&&d[3u16]==55);
 let mut v:u16=0;
 while v<65500{
  frame_stats_elapsed_begin(s.as_array(),v,true);frame_stats_tenths_digits(s.as_array(),d.as_array());
  check(d[0u16]==48+v/10000 && d[1u16]==48+(v/1000)%10 && d[2u16]==46 && d[3u16]==48+(v/100)%10);
  v+=137;
 }
 frame_stats_elapsed_begin(s.as_array(),65534,true);frame_stats_tenths_digits(s.as_array(),d.as_array());
 check(d[0u16]==54&&d[1u16]==53&&d[3u16]==53);
 frame_stats_elapsed_begin(s.as_array(),65535,true);check(s[3u16]==0);
 frame_stats_elapsed_begin(s.as_array(),17000,false);check(s[3u16]==0);
 frame_stats_elapsed_begin(s.as_array(),1000,true);check(s[3u16]==1);
 halt(0x600d);
}"#,
        &options,
        &mut |name| match name {
            "device_abi" => Ok(include_str!("../../rcc/device_abi.rs").to_owned()),
            "time" => Ok(include_str!("../../rcc/time.rs").to_owned()),
            "frame_stats" => Ok(include_str!("../../rcc/frame_stats.rs").to_owned()),
            _ => Err(name.to_owned()),
        },
    ).unwrap();
    let words = rcc_backend::try_compile(parsed, &options, "main")
        .unwrap()
        .words;
    let mut cpu = CpuV3Sim::default();
    let device = boot::SystemControlDevice::default();
    device.advance_microseconds(16789);
    cpu.attach_device(0, Box::new(device));
    cpu.set_boot_window(&words);
    assert!(matches!(
        cpu.run(2_000_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    cpu.device::<boot::SystemControlDevice>(0)
        .unwrap()
        .advance_microseconds(100_000);
    // Host time advances once, including while CPU execution is stopped.
    use cpu_v3::Device;
    let mut saturated = boot::SystemControlDevice::default();
    saturated.advance_microseconds(100_000);
    assert_eq!(saturated.read(&mut [], boot::SYSCTL_MICROSECONDS), 65535);
    saturated.write(&mut [], boot::SYSCTL_MICROSECONDS, 1);
    saturated.advance_microseconds(23);
    assert_eq!(saturated.read(&mut [], boot::SYSCTL_MICROSECONDS), 23);
    assert_eq!(saturated.read(&mut [], boot::SYSCTL_MILLISECONDS), 100);
}

fn run_rtl(name: &str, projects: Vec<GeneratedVerilogProject>, testbench: String) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/frame-stats-rtl")
        .join(name);
    std::fs::create_dir_all(&root).unwrap();
    let mut files = BTreeMap::new();
    for project in projects {
        for (path, source) in project.files {
            if !source.contains("module tb") {
                if let Some(previous) = files.insert(path, source.clone()) {
                    assert_eq!(previous, source);
                }
            }
        }
    }
    let mut paths = Vec::new();
    for (i, source) in files.values().enumerate() {
        let path = root.join(format!("source_{i}.v"));
        std::fs::write(&path, source).unwrap();
        paths.push(path);
    }
    let tb = root.join("tb.v");
    std::fs::write(&tb, testbench).unwrap();
    let image = root.join("tb.vvp");
    let output =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()))
            .args(["-g2012", "-s", "tb", "-o"])
            .arg(&image)
            .args(paths)
            .arg(tb)
            .output()
            .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
        .arg(image)
        .output()
        .unwrap();
    std::fs::write(root.join("result.log"), &output.stdout).unwrap();
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("TIME_PASS"),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "explicit Icarus validation of production SystemControl timer"]
fn actual_system_control_rtl_exact_division_wrap_and_hold() {
    run_rtl(
        "counter",
        vec![
            VerilogProject::generate::<SystemControlDevice<4, 54000>>().unwrap(),
            VerilogProject::generate::<SystemControlDevice<4, 2>>().unwrap(),
        ],
        include_str!("../time_counter_tb.v")
            .replace(
                "__EXACT__",
                &SystemControlDevice::<4, 54000>::verilog_identity().module_name(),
            )
            .replace(
                "__FAST__",
                &SystemControlDevice::<4, 2>::verilog_identity().module_name(),
            ),
    );
}

#[test]
#[ignore = "bounded production RTL microsecond clear/saturation/hold test"]
fn actual_microsecond_counter_clear_saturate_and_hold() {
    run_rtl(
        "microseconds",
        vec![
            VerilogProject::generate::<SystemControlDevice<4, 54000>>().unwrap(),
            VerilogProject::generate::<SystemControlDevice<4, 1000>>().unwrap(),
        ],
        include_str!("../microsecond_counter_tb.v")
            .replace(
                "__EXACT__",
                &SystemControlDevice::<4, 54000>::verilog_identity().module_name(),
            )
            .replace(
                "__FAST__",
                &SystemControlDevice::<4, 1000>::verilog_identity().module_name(),
            ),
    );
}

#[test]
#[ignore = "bounded real CPU + cache + SystemControl wall-clock test"]
fn real_cpu_recv_dwait_maintenance_and_watch_use_wall_clock() {
    run_cpu_wall_clock(false);
}

#[test]
#[ignore = "bounded real CPU/cache microsecond clear/RECV/hold/watch test"]
fn real_cpu_uses_microseconds_through_device() {
    run_cpu_wall_clock(true);
}

fn run_cpu_wall_clock(microseconds: bool) {
    // Reuse the existing cache test's independent, backpressured memory peer.
    // The CPU program and time/completion assertions below are independent.
    let fixture = include_str!("../cache_maintenance_protocol_tb.v");
    let prefix = fixture
        .split_once("integer commands=")
        .expect("memory-peer boundary")
        .0;
    let mut tb = prefix.replace("__CASE__", "0").replace("__RESULT__", "0");
    tb = tb.replace(".watch_read_data(16'd0),", ".watch_read_data(device_read_data),.watch_device_index(watch_index),.watch_device_channel(watch_channel),.watch_read_enable(watch_enable),");
    tb = tb.replace(
        "__SYSCTL__ sysctl(",
        "wire [2:0] watch_index; wire [3:0] watch_channel; wire watch_enable;\n__SYSCTL__ sysctl(",
    );
    let start = tb.find("__SYSCTL__ sysctl(").unwrap();
    let end = tb[start..].find("__CACHE__ cache(").unwrap() + start;
    let instance = tb[start..end]
        .replace(
            ".device_index(device_index)",
            ".device_index(watch_enable?watch_index:device_index)",
        )
        .replace(
            ".device_channel(device_channel)",
            ".device_channel(watch_enable?watch_channel:device_channel)",
        )
        .replace(
            ".device_read_enable(device_read_enable)",
            ".device_read_enable(device_read_enable||watch_enable)",
        )
        .replace(
            ".device_write_enable(device_write_enable)",
            ".device_write_enable(device_write_enable&&!watch_enable)",
        );
    tb.replace_range(start..end, &instance);
    let channel = if microseconds { 11 } else { 10 };
    let mut p = Vec::new();
    if microseconds {
        p.extend(cpu_v3::load_immediate16(0, 1));
        p.push(cpu_v3::device_send(0, 0, channel));
    }
    p.push(cpu_v3::device_receive(2, 0, channel));
    p.extend(cpu_v3::load_immediate16(1, 0x4000));
    p.extend(cpu_v3::load_immediate16(0, 0x1234));
    p.push(cpu_v3::store(0, 1, 0));
    p.extend(cpu_v3::load_immediate16(3, 2));
    p.push(cpu_v3::line_copy(1, 3));
    p.push(cpu_v3::wait_data_cache());
    p.push(cpu_v3::device_receive(2, 0, channel));
    p.push(cpu_v3::device_send(0, 0, 4));
    p.push(cpu_v3::device_receive(2, 0, channel));
    p.extend(cpu_v3::load_immediate16(
        0,
        boot::sysctl_watch_target(0, channel),
    ));
    p.push(cpu_v3::device_send(0, 0, 6));
    p.push(cpu_v3::device_receive(2, 0, channel));
    p.push(cpu_v3::device_send(2, 0, 7));
    p.push(cpu_v3::device_receive(2, 0, channel));
    p.push(cpu_v3::signal(2, 0));
    let init = p
        .iter()
        .enumerate()
        .map(|(i, w)| format!("rom[{i}]=16'h{w:04x};\n"))
        .collect::<String>();
    tb.push_str(&include_str!("../time_cpu_checks.vh").replace("__PROGRAM__", &init));
    if microseconds {
        tb = tb
            .replace("integer previous_ms=0;", "integer previous_ms=0,clear_edge=0;")
            .replace("sysctl.milliseconds", "sysctl.microseconds")
            .replace("RECV10 reads=", "RECV11 reads=")
            .replace("((active_edges/32)&65535)", "(active_edges-clear_edge)")
            .replace("device_channel==10", "device_channel==11")
            .replace("  active_edges=active_edges+1;", "  if(device_write_enable && device_index==0 && device_channel==11 && device_write_data[0])clear_edge=active_edges+1;\n  active_edges=active_edges+1;");
    }
    tb = tb
        .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
        .replace(
            "__CACHE__",
            &CpuV3DataCache::verilog_identity().module_name(),
        )
        .replace(
            "__SYSCTL__",
            &SystemControlDevice::<4, 32>::verilog_identity().module_name(),
        );
    run_rtl(
        if microseconds {
            "cpu-microseconds"
        } else {
            "cpu"
        },
        vec![
            VerilogProject::generate::<CpuV3Core>().unwrap(),
            VerilogProject::generate::<CpuV3DataCache>().unwrap(),
            VerilogProject::generate::<SystemControlDevice<4, 32>>().unwrap(),
        ],
        tb,
    );
}
