use super::*;
#[test]
#[ignore = "requires Icarus and GOWIN_HOME"]
fn q13_native_tdp_reads_both_ports_and_freezes_on_ce() {
    use std::process::Command;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lighting-rsqrt-production-20261007/primitive-audit");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("rom.v"), normalization_tdp_q13()).unwrap();
    let mut tb=String::from("module tb;reg clk=0,ce=1;reg [9:0] a=0,b=0;wire [17:0] qa,qb;lighting_norm_q13 dut(.*);\n`ifdef GPU_V2_GOWIN_DSP\nGSR GSR(.GSRI(1'b1));\n`endif\nreg [17:0] expected[0:1023];integer i;reg [17:0] held_a,held_b;initial begin\n");
    for i in 0..1024 {
        let value = if i < 256 {
            let a = if i >= 128 { i as i64 - 256 } else { i as i64 };
            (a * a) as u64
        } else if i < 384 {
            let index = i - 256;
            let parity = index / 64;
            let segment = index % 64;
            let endpoint = |s: usize| {
                (8192.0 / ((1.0 + s as f64 / 64.0) * (1_u32 << parity) as f64).sqrt())
                    .round_ties_even() as u64
            };
            let bias = (segment / 8 * 8..segment / 8 * 8 + 8)
                .map(|s| endpoint(s) - endpoint(s + 1))
                .min()
                .unwrap();
            endpoint(segment) | ((endpoint(segment) - endpoint(segment + 1) - bias) << 14)
        } else {
            0
        };
        writeln!(tb, "expected[{i}]=18'h{value:x};").unwrap();
    }
    tb.push_str("for(i=0;i<1024;i=i+1) begin a=i;b=1023-i;ce=1;#2;clk=1;#2;clk=0;#2;if(qa!==expected[i]||qb!==expected[1023-i]) $fatal(1,\"TDP row %0d\",i);held_a=qa;held_b=qb;ce=0;a=~a;b=~b;#2;clk=1;#2;clk=0;#2;if(qa!==held_a||qb!==held_b) $fatal(1,\"CE hold\");end $display(\"PASS both ports 1024 rows CE freeze\");$finish;end initial begin #20000;$fatal(1,\"bounded timeout\");end endmodule\n");
    std::fs::write(dir.join("tb.v"), tb).unwrap();
    for vendor in [false, true] {
        let mut command =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        command
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "test.vvp"]);
        if vendor {
            command.arg("-DGPU_V2_GOWIN_DSP").arg(
                std::path::Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        let compiled = command.args(["rom.v", "tb.v"]).output().unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let result = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
            .current_dir(&dir)
            .arg("test.vvp")
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&result.stdout);
        assert!(
            result.status.success() && out.contains("PASS"),
            "vendor={vendor} {out}"
        );
        println!("vendor={vendor} {out}");
    }
}
