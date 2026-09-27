//! Independent native tests of the accepted eight-DPB storage/ownership component.

use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires Icarus and GOWIN_HOME with GW2A DPB simulation models"]
fn fused_framebuffer_vendor_geometry_masks_stalls_and_bandwidth() {
    for (sector, half_capacity, delay, bank_order, fold) in [
        (0, 1, 0, 0, 0),
        (0, 1, 0, 1, 0),
        (1, 1, 0, 1, 0),
        (0, 0, 0, 1, 0),
        (0, 1, 4, 1, 0),
        (0, 1, 6, 1, 0),
        (0, 1, 0, 1, 1),
    ] {
        let stdout = run_vendor_order(sector, half_capacity, delay, bank_order, fold, None);
        assert!(stdout.contains("PASS fused framebuffer:"), "{stdout}");
    }
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME with GW2A DPB simulation models"]
fn fused_framebuffer_rejects_missing_depth_write() {
    run_vendor(0, 1, 0, Some(("skip_depth", "final depth mismatch")));
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME with GW2A DPB simulation models"]
fn fused_framebuffer_rejects_wrong_row_bank() {
    run_vendor(0, 1, 0, Some(("row_bank", "old quad mismatch")));
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME with GW2A DPB simulation models"]
fn fused_framebuffer_rejects_zero_mask_stealing_read_address() {
    run_vendor(0, 1, 0, Some(("zero_mask_address", "memory mismatch")));
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME with GW2A DPB simulation models"]
fn fused_framebuffer_rejects_source_release_before_commit() {
    run_vendor(
        0,
        1,
        0,
        Some((
            "source_release",
            "fused source/lease changed before actual commit",
        )),
    );
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME with GW2A DPB simulation models"]
fn fused_framebuffer_rejects_wrong_execution_lane_order() {
    run_vendor(
        0,
        1,
        0,
        Some(("lane_order", "execution source bits/order mismatch")),
    );
}

fn run_vendor(sector: u32, half_capacity: u32, delay: u32, fault: Option<(&str, &str)>) -> String {
    run_vendor_order(sector, half_capacity, delay, 1, 0, fault)
}

fn run_vendor_order(
    sector: u32,
    half_capacity: u32,
    delay: u32,
    bank_order: u32,
    fold: u32,
    fault: Option<(&str, &str)>,
) -> String {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/hardware/gpu");
    let gowin = std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME is required for the DPB model");
    let primitive = Path::new(&gowin).join("IDE/simlib/gw2a/prim_sim.v");
    assert!(primitive.is_file(), "missing {}", primitive.display());
    let output = std::env::temp_dir().join(format!(
        "gpu-fused-{}-{sector}-{half_capacity}-{delay}-{bank_order}-{fold}-{}",
        std::process::id(),
        fault.map_or("native", |(name, _)| name)
    ));
    std::fs::create_dir_all(&output).unwrap();
    for file in [
        "framebuffer_lane_array.v",
        "fused_framebuffer_pipe.v",
        "fused_framebuffer_tb.v",
    ] {
        let mut rtl = std::fs::read_to_string(source.join(file)).unwrap();
        let mutation = match (file, fault.map(|(name, _)| name)) {
            ("fused_framebuffer_pipe.v", Some("skip_depth")) => Some((
                "wire [3:0] zm = depth_enable && depth_write ? cm : 4'b0;",
                "wire [3:0] zm = 4'b0;",
            )),
            ("fused_framebuffer_pipe.v", Some("lane_order")) => {
                Some(("reorder <= BANK_ORDER && input_x[1];", "reorder <= 1'b0;"))
            }
            ("framebuffer_lane_array.v", Some("row_bank")) => {
                Some(("render_read_x[1]!=(bank/2)", "render_read_x[1]==(bank/2)"))
            }
            ("framebuffer_lane_array.v", Some("zero_mask_address")) => Some((
                "wire mw_here=mw && wp==plane && memory_write_mask!=0;",
                "wire mw_here=mw && wp==plane;",
            )),
            ("fused_framebuffer_tb.v", Some("source_release")) => Some((
                "repeat(7) @(negedge clk);reset=1;",
                "repeat(2) @(negedge clk);colors[0]=!colors[0];repeat(5) @(negedge clk);reset=1;",
            )),
            _ => None,
        };
        if let Some((old, new)) = mutation {
            assert_eq!(rtl.matches(old).count(), 1, "mutation must execute: {file}");
            rtl = rtl.replace(old, new);
        }
        std::fs::write(output.join(file), rtl).unwrap();
    }
    let executable = output.join("test.vvp");
    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let mut command = Command::new(iverilog);
    command
        .args(["-g2012", "-s", "fused_framebuffer_tb", "-o"])
        .arg(&executable);
    command.args([
        format!("-Pfused_framebuffer_tb.SECTOR={sector}"),
        format!("-Pfused_framebuffer_tb.HALF_CAPACITY={half_capacity}"),
        format!("-Pfused_framebuffer_tb.EXEC_DELAY={delay}"),
        format!("-Pfused_framebuffer_tb.BANK_ORDER={bank_order}"),
        format!("-Pfused_framebuffer_tb.EXEC_FOLD={fold}"),
    ]);
    for file in [
        "framebuffer_lane_array.v",
        "fused_framebuffer_pipe.v",
        "fused_framebuffer_tb.v",
    ] {
        command.arg(output.join(file));
    }
    let compile = command.arg(primitive).output().unwrap();
    assert!(
        compile.status.success(),
        "Icarus compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    // The bench has a global 200,000-clock watchdog and bounded transaction waits.
    let simulation = Command::new(vvp).arg(executable).output().unwrap();
    let stdout = String::from_utf8_lossy(&simulation.stdout).into_owned();
    if let Some((name, expected)) = fault {
        assert!(
            !simulation.status.success() && stdout.contains(expected),
            "negative {name} failed to reproduce:\n{stdout}"
        );
    } else {
        assert!(
            simulation.status.success(),
            "native test failed:\n{stdout}\n{}",
            String::from_utf8_lossy(&simulation.stderr)
        );
    }
    println!("{stdout}");
    stdout
}
