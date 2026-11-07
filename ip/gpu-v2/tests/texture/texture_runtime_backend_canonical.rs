//! Matched actual backend replay plus exhaustive conserving parent-cost domain.
#[path = "texture_runtime_backend.rs"]
mod reference;
use gpu_v2::texture::rtl::runtime_backend;

const COST_TB: &str = r#"module tb;
reg clk=0,reset=1,ce=0,commit_allow=0,abort=0,in_valid=0,packet_enable=0,out_ready=0;
reg[37:0]in_owner=0;reg[35:0]in_result=0;
wire in_ready,in_accept,fault,preflight_bad;wire[1:0]reserved_cost;wire[4:0]work_reserved;
gpu_v2_texture_runtime_backend_canonical dut(.clk(clk),.reset(reset),.ce(ce),.commit_allow(commit_allow),.abort(abort),
 .in_valid(in_valid),.in_owner(in_owner),.in_result(in_result),.packet_enable(packet_enable),.out_ready(out_ready),
 .in_ready(in_ready),.in_accept(in_accept),.fault(fault),.preflight_bad(preflight_bad),.reserved_cost(reserved_cost),.work_reserved(work_reserved));
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
integer p,expected,edges=0;reg[4:0]held;
task tick;begin clk=1;#2;clk=0;#2;edges=edges+1;if(edges>4096)$fatal(1,"edge watchdog");end endtask
initial begin
for(p=0;p<512;p=p+1)begin
 reset=1;ce=0;in_valid=0;commit_allow=0;#2;tick;reset=0;
 in_owner=0;in_owner[8:0]=511-p;in_owner[17:9]=p;in_owner[22:19]=4;in_owner[26:23]=3;in_owner[37]=(p==0);
 expected=((511-p)!=0)+(p!=0);in_valid=1;#2;
 if(dut.coefficient.cost_in!==expected||in_ready||in_accept||reserved_cost||fault||preflight_bad)$fatal(1,"CE0 cost p%0d",p);
 tick;ce=1;#2;
 if(!in_ready||in_accept||reserved_cost||fault||preflight_bad)$fatal(1,"denied cost p%0d",p);
 tick;if(work_reserved!==0)$fatal(1,"denied write p%0d",p);
 commit_allow=1;#2;
 if(!in_ready||!in_accept||reserved_cost!==expected||fault||preflight_bad)$fatal(1,"accepted cost p%0d",p);
 tick;held=work_reserved;ce=0;#2;
 if(held!==expected||in_ready||in_accept||reserved_cost)$fatal(1,"reserved cost p%0d",p);
 tick;if(work_reserved!==held)$fatal(1,"CE0 hold p%0d",p);
end
$display("PASS canonical exhaustive512 conserving costs CE0/denied/actualaccept edges%0d",edges);$finish;
end
initial begin #100000;$fatal(1,"time watchdog");end
endmodule
"#;

#[test]
#[ignore = "requires actual selected arithmetic and Gowin native primitives"]
fn canonical_cost_and_actual_backend_qualify() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/runtime-backend-canonical-cost-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    for native in [false, true] {
        let source = runtime_backend::verilog_canonical(native).unwrap();
        let source = if native {
            "`define GPU_V2_GOWIN_DSP\n".to_owned() + &source
        } else {
            source
        };
        let name = format!("native-{native}.v");
        std::fs::write(dir.join(&name), source).unwrap();
        for (kind, fixture) in [
            ("cost512", COST_TB.to_owned()),
            (
                "actual",
                reference::fixture()
                    .replace(runtime_backend::MODULE, runtime_backend::CANONICAL_MODULE),
            ),
        ] {
            let tb = format!("{kind}.v");
            std::fs::write(dir.join(&tb), fixture).unwrap();
            let mut command = std::process::Command::new(
                std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
            );
            command
                .current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
            if native {
                command.arg(
                    std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            }
            let compiled = command.args([&name, &tb]).output().unwrap();
            assert!(
                compiled.status.success(),
                "{}",
                String::from_utf8_lossy(&compiled.stderr)
            );
            let ran =
                std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
                    .current_dir(&dir)
                    .arg("sim.vvp")
                    .output()
                    .unwrap();
            assert!(
                ran.status.success(),
                "native={native} {kind}: {} {}",
                String::from_utf8_lossy(&ran.stdout),
                String::from_utf8_lossy(&ran.stderr)
            );
            println!(
                "native={native} {kind}: {}",
                String::from_utf8_lossy(&ran.stdout)
            );
        }
    }
}
