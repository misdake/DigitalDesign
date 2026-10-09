//! Test-only composition of the production display sources and vendor MC.
//! No system crate dependency or replacement display/CDC model is introduced.
use digital_design_hardware_gowin::sdram_memory_controller::{
    arbiter::{CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput},
    combination, emu,
    ports::OracleImage,
};
use digital_design_hardware_gowin::ModuleIo;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const DISPLAY_CONFIG: &str = include_str!("../../../systems/cpu-v3-tang-nano-20k/src/display.rs");
const LAYOUT: &str = include_str!("../../../systems/cpu-v3-tang-nano-20k/src/layout.rs");
const DISPLAY: &str =
    include_str!("../../../systems/cpu-v3-tang-nano-20k/src/hardware/display/display_hdmi.v");
const BUFFER: &str = include_str!(
    "../../../systems/cpu-v3-tang-nano-20k/src/hardware/display/display_line_buffer.v"
);
const FIFO: &str =
    include_str!("../../../systems/cpu-v3-tang-nano-20k/src/hardware/display/display_pair_fifo.v");

// Read literal fields from the authoritative production configuration. Fail
// closed if its representation changes, rather than silently retaining a copy.
fn literal(source: &str, key: &str, separator: char) -> u64 {
    let line = source
        .lines()
        .find(|line| line.trim_start().starts_with(key))
        .unwrap();
    line.split_once(separator)
        .unwrap()
        .1
        .trim()
        .trim_end_matches([',', ';'])
        .replace('_', "")
        .parse()
        .unwrap()
}

fn display_params() -> (String, f64) {
    let config = DISPLAY_CONFIG
        .split_once("pub const VGA_800X480_2X: DisplayConfig = DisplayConfig {")
        .unwrap()
        .1
        .split_once("};")
        .unwrap()
        .0;
    let mut params = String::new();
    for (name, field, width) in [
        ("H_TOTAL", "h_total:", 10),
        ("H_SYNC_END", "h_sync_end:", 10),
        ("H_ACTIVE_START", "h_active_start:", 10),
        ("H_ACTIVE_END", "h_active_end:", 10),
        ("V_TOTAL", "v_total:", 9),
        ("V_SYNC_END", "v_sync_end:", 9),
        ("V_ACTIVE_START", "v_active_start:", 9),
        ("V_ACTIVE_END", "v_active_end:", 9),
        ("SIDE_BORDER", "side_border:", 8),
        ("SCALE", "scale:", 1),
        ("LAST_REPEAT", "vertical_repeat_last:", 1),
    ] {
        params.push_str(&format!(
            "localparam [{width}:0] {name}={};\n",
            literal(config, field, ':')
        ));
    }
    for (name, key) in [
        ("FB_WIDTH", "pub const FRAMEBUFFER_WIDTH:"),
        ("FB_HEIGHT", "pub const FRAMEBUFFER_HEIGHT:"),
    ] {
        params.push_str(&format!(
            "localparam [9:0] {name}={};\n",
            literal(LAYOUT, key, '=')
        ));
    }
    params.push_str("localparam [9:0] LINE_SLOT_WORDS=FB_WIDTH/2; localparam [4:0] BURSTS_PER_LINE=FB_WIDTH/16;\nlocalparam [4:0] LAST_BURST=BURSTS_PER_LINE-1; localparam [7:0] LAST_FILL_Y=FB_HEIGHT-1;\nlocalparam [12:0] TILE_ROW_STRIDE=FB_WIDTH*16;\n");
    (params, literal(config, "pixel_clock_hz:", ':') as f64)
}

fn bounded(mut command: Command, root: &Path, name: &str) {
    let mut child = command
        .stdout(Stdio::from(
            fs::File::create(root.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(root.join(format!("{name}.err"))).unwrap(),
        ))
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
            break;
        }
        if start.elapsed() > Duration::from_secs(180) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "{name} wall watchdog: {}",
                fs::read_to_string(root.join(format!("{name}.out"))).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn run_case(name: &str, early: bool, group: bool, phase: u32, starve: bool) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-v2-sdram/display-integration")
        .join(name);
    fs::create_dir_all(&root).unwrap();
    // Same model-safe related clocks as the existing connected pin fixture.
    // Preserve the exact nominal 54 MHz / 33.3 MHz ratio for real 2x scanout.
    let (params, pixel_hz) = display_params();
    let mut sources = combination::rtl_sources_with_options(21600, early, group).unwrap();
    sources.insert(
        "display.v".into(),
        DISPLAY
            .replace("__DISPLAY_CONFIG__", &params)
            .replace("__LINE_BUFFER__", "DisplayLineBuffer")
            .replace("__PAIR_FIFO__", "DisplayPairFifo"),
    );
    let table = DISPLAY_CONFIG
        .split_once("pub const LINEAR6_TO_SRGB8: [u8; 64] = [")
        .unwrap()
        .1
        .split_once("];")
        .unwrap()
        .0;
    let values = table
        .split(',')
        .filter_map(|v| {
            let v = v.trim();
            (!v.is_empty()).then(|| v.parse::<u32>().unwrap())
        })
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 64);
    let init = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            format!(
                "memory_a[{}]=32'd{v}; memory_b[{}]=32'd{v};\n",
                448 + i,
                448 + i
            )
        })
        .collect::<String>();
    sources.insert("buffer.v".into(), BUFFER.replace("__SRGB_INIT__", &init));
    sources.insert("fifo.v".into(), FIFO.into());
    let mut i = emu::idle_inputs();
    i.display_response_ready = true;
    i.instruction_response_ready = true;
    i.data_response_ready = true;
    i.dma_response_ready = true;
    i.data_line = true;
    let c = emu::Combination::new(OracleImage::filled::<0>(0, 4096).unwrap(), 32).unwrap();
    let o = c.output(&i);
    let mut declarations = String::new();
    for (values, dir) in [
        (CpuV3MemoryArbiterInput::verilog_values(&i), "reg"),
        (CpuV3MemoryArbiterOutput::verilog_values(&o), "wire"),
    ] {
        for v in values {
            if v.name == "reset" || v.name == "lookahead_enable" || v.name.starts_with("memory_") {
                continue;
            }
            let width = if v.width == 1 {
                String::new()
            } else {
                format!("[{}:0] ", v.width - 1)
            };
            let display = if dir == "reg" {
                match v.name {
                    "display_request_valid" => {
                        Some("!reset && memory_request_valid && !(STARVE && published_groups>=6)")
                    }
                    "display_address" => Some("memory_address"),
                    "display_urgent" => Some("memory_urgent"),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(value) = display {
                declarations.push_str(&format!("wire {width}{} = {value};\n", v.name));
            } else if dir == "reg" {
                declarations.push_str(&format!(
                    "reg {width}{} = {}'h{:x};\n",
                    v.name, v.width, v.value
                ));
            } else {
                declarations.push_str(&format!("wire {width}{};\n", v.name));
            }
        }
    }
    let tb = include_str!("fixtures/sdram_display_integration.v")
        .replace("__PARAMS__", &params)
        .replace("__PORTS__", &declarations)
        .replace(
            "__PIXEL_HALF__",
            &(10.102 * 54_000_000.0 / pixel_hz).to_string(),
        )
        .replace("__PHASE__", &phase.to_string())
        .replace("__FB_COUNT__", if group { "2" } else { "3" })
        .replace("__STARVE__", if starve { "1" } else { "0" });
    sources.insert("tb.v".into(), tb);
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&root)
        .args(["-g2012", "-D__ICARUS__", "-s", "tb", "-o", "test.vvp"]);
    for (path, text) in sources {
        let file = path.file_name().unwrap();
        fs::write(root.join(file), text).unwrap();
        compile.arg(file);
    }
    bounded(compile, &root, "compile");
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    run.current_dir(&root).arg("test.vvp");
    bounded(run, &root, "run");
    let output = fs::read_to_string(root.join("run.out")).unwrap();
    assert!(output.contains(if starve {
        "PASS expected production underflow"
    } else {
        "PASS production display integration"
    }));
    println!("{name}: {}", output.trim());
}

#[test]
#[ignore = "explicit bounded production display/CDC and connected SDRAM pin simulation"]
fn real_display_buffer_cdc_under_cpu_and_framebuffer_load() {
    for phase in [0, 73] {
        for (profile, early, group) in [
            ("serial", false, false),
            ("early", true, false),
            ("group", true, true),
        ] {
            run_case(
                &format!("{profile}-phase{phase}"),
                early,
                group,
                phase,
                false,
            );
        }
    }
    run_case("group-starvation-control", true, true, 0, true);
}
