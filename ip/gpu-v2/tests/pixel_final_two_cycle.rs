//! Independent full-recipe gold and real two-edge reissue/loan scoreboard.
use gpu_v2::system::pixel::final_stage::rtl::{
    ordered_compact::{self, Profile},
    ordered_shared,
};
use std::fmt::Write;

#[derive(Clone, Copy)]
struct Pixel {
    tint: [u8; 3],
    tex: [u8; 3],
    spec: [u8; 3],
    g: u16,
    h: u16,
}

fn rgb(p: Pixel) -> [u8; 3] {
    std::array::from_fn(|c| {
        let product = u32::from(p.tint[c]) * u32::from(p.tex[c]);
        let base = (product + 127) / 255;
        let sum = base * u32::from(p.g) + u32::from(p.spec[c]) * u32::from(p.h);
        let floor = sum / 256;
        let remainder = sum % 256;
        (floor + u32::from(remainder > 128 || (remainder == 128 && floor % 2 == 1))).min(255) as u8
    })
}

fn packed(rgb: [u8; 3]) -> u32 {
    u32::from(rgb[0]) | (u32::from(rgb[1]) << 8) | (u32::from(rgb[2]) << 16)
}

fn pixels() -> Vec<Pixel> {
    let mut result = Vec::new();
    // /255 threshold neighbors and feature combinations; exhaustive base
    // identity is checked separately without repeating the large RTL sweep.
    for tint in [0, 1, 2, 3, 63, 64, 127, 128, 129, 253, 254, 255] {
        for tex in 0..=255 {
            result.push(Pixel {
                tint: [tint, 255 - tint, tint],
                tex: [tex, tex, 255 - tex],
                spec: [0, 255, 128],
                g: 256,
                h: 0,
            });
        }
    }
    for b in 0..=255 {
        result.push(Pixel {
            tint: [255; 3],
            tex: [b, 255 - b, b],
            spec: [0; 3],
            g: 128,
            h: 0,
        });
    }
    for g in [0, 1, 127, 128, 255, 256, 510, 511] {
        for h in [0, 1, 127, 128, 255, 256] {
            for tint in [[0; 3], [255; 3], [1, 127, 128], [254, 255, 129]] {
                result.push(Pixel {
                    tint,
                    tex: [0, 255, 128],
                    spec: [255, 0, 128],
                    g,
                    h,
                });
                result.push(Pixel {
                    tint,
                    tex: [255; 3],
                    spec: [255; 3],
                    g,
                    h,
                });
            }
        }
    }
    let mut rng = 0x371d_890a_u32;
    for _ in 0..2048 {
        let mut next = || {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            rng >> 16
        };
        result.push(Pixel {
            tint: std::array::from_fn(|_| next() as u8),
            tex: std::array::from_fn(|_| next() as u8),
            spec: std::array::from_fn(|_| next() as u8),
            g: (next() % 512) as u16,
            h: (next() % 257) as u16,
        });
    }
    result
}

#[test]
fn exhaustive_exact_base_identity() {
    for tint in 0..=255_u32 {
        for tex in 0..=255_u32 {
            let product = tint * tex;
            let t = product + 128;
            assert_eq!((t + (t >> 8)) >> 8, (product + 127) / 255);
        }
    }
}

fn fixture(borrow: bool) -> String {
    let pixels = pixels();
    let mut init = String::new();
    for (i, p) in pixels.iter().copied().enumerate() {
        writeln!(init, "tints[{i}]=24'h{:06x};textures[{i}]=24'h{:06x};specs[{i}]=24'h{:06x};gs[{i}]=9'd{};hs[{i}]=9'd{};gold[{i}]=24'h{:06x};", packed(p.tint), packed(p.tex), packed(p.spec), p.g, p.h, packed(rgb(p))).unwrap();
    }
    TB.replace("BORROW_INPUTS", if borrow { "1" } else { "0" })
        .replace("INPUTS", &pixels.len().to_string())
        .replace("// GOLD", &init)
}

const TB: &str = r#"module tb;
reg clk=0,reset=1,ce=0,commit_allow=1,abort=0,in_valid=0,out_ready=0;
reg[23:0]in_tint=0,in_tex=0,in_spec=0;reg[8:0]in_g=0,in_h=0;
wire in_ready,proposal_ready,in_accept,advance,preflight_bad,fault,out_valid,out_fire;
wire[23:0]out_rgb;wire[3:0]credits,queued;
localparam BORROW=BORROW_INPUTS;
gpu_v2_final_ordered_two_cycle dut(.*);
wire ref_ready,ref_accept,ref_advance,ref_bad,ref_fault,ref_valid,ref_fire;
wire[23:0]ref_rgb;wire[3:0]ref_credits,ref_queued;
reg match_reference=1;
gpu_v2_final_ordered_compact reference(
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(commit_allow),.abort(abort),
 .in_valid(in_accept&&match_reference),.in_tint(in_tint),.in_tex(in_tex),.in_spec(in_spec),.in_g(in_g),.in_h(in_h),
 .in_ready(ref_ready),.in_accept(ref_accept),.advance(ref_advance),.preflight_bad(ref_bad),.fault(ref_fault),
 .out_ready(1'b1),.out_valid(ref_valid),.out_fire(ref_fire),.out_rgb(ref_rgb),.credits(ref_credits),.queued(ref_queued));
`ifdef NATIVE
GSR GSR(.GSRI(1'b1));
`endif
reg[23:0]tints[0:INPUTS-1],textures[0:INPUTS-1],specs[0:INPUTS-1],gold[0:INPUTS-1];
reg[8:0]gs[0:INPUTS-1],hs[0:INPUTS-1];
reg[23:0]expected[0:7],ref_expected[0:7];integer due[0:7];
integer ptr=0,head=0,tail=0,used=0,enabled=0,next_accept=0,wall,k,q,index;
reg[8:0]loan_g,loan_h;reg[23:0]loan_spec;
integer rh=0,rt=0,ru=0,reference_outputs=0;
integer accepted=0,consumed=0,discarded=0,full=0,held=0,denied=0,frozen=0,simultaneous=0,directpop=0,fallbackpop=0,ii2=0,last_accept=-99;
reg er,ev,ea,ep,ef;reg[3:0]before_credits,before_queued;
task tick;begin clk=1;#2;clk=0;#2;end endtask
initial begin
// GOLD
#2;tick;reset=0;
for(wall=0;wall<100000;wall=wall+1)begin
 reset=(wall==1);ce=(wall%7!=3)&&(wall%17!=13);commit_allow=(wall%31!=6)&&(wall%31!=7);
 out_ready=(wall>90)&&(wall%101<65);in_valid=(ptr<INPUTS)&&(wall%13!=10);
 // The source changes all presented payloads every wall edge, even when the
 // arithmetic engine is busy; only an actual acceptance creates ownership.
 if(in_valid)begin in_tint=tints[ptr];in_tex=textures[ptr];in_spec=specs[ptr];in_g=gs[ptr];in_h=hs[ptr];end
 else begin in_tint=wall;in_tex=~wall;in_spec=wall*17;in_g=wall%512;in_h=wall%257;end
 // Loan held over wall-clock pauses through the first following enabled edge.
 // After that the same ports may change before result publication/consumption.
 if(BORROW&&used!=0&&enabled<next_accept)begin in_g=loan_g;in_h=loan_h;in_spec=loan_spec;end
 #2;
 q=0;for(k=0;k<used;k=k+1)begin index=(head+k)%1;if(due[index]<=enabled)q=q+1;end
 ev=!reset&&(used!=0)&&(due[head]<=enabled);er=ce&&!reset&&((used<1)||(ev&&out_ready))&&(enabled>=next_accept);ea=ce&&!reset&&commit_allow;
 ep=ea&&in_valid&&((used<1)||(ev&&out_ready))&&(enabled>=next_accept);ef=ea&&out_ready&&ev;
 if(fault||preflight_bad||in_ready!==er||out_valid!==ev||advance!==ea||in_accept!==ep||out_fire!==ef||credits!==used||queued!==q)$fatal(1,"single controls wall%0d used%0d q%0d enable%0d next%0d",wall,used,q,enabled,next_accept);
 if(ev&&out_rgb!==expected[head])$fatal(1,"RGB wall%0d got%h want%h",wall,out_rgb,expected[head]);
 if(ref_bad||ref_fault||ref_accept!==ep)$fatal(1,"matched acceptance wall%0d",wall);
 if(ref_valid&&(ru==0||ref_rgb!==ref_expected[rh]))$fatal(1,"matched compact numeric wall%0d got%h want%h",wall,ref_rgb,ref_expected[rh]);
 if(used==1)full=full+1;if(ev&&!ef)held=held+1;if(!ce&&used>0)frozen=frozen+1;if(ce&&!commit_allow&&used>0)denied=denied+1;if(ep&&ef)simultaneous=simultaneous+1;if(ef&&enabled==due[head])directpop=directpop+1;if(ef&&enabled>due[head])fallbackpop=fallbackpop+1;
 if(reset)begin
  if(used==0)$fatal(1,"reset not active");discarded=discarded+used;used=0;head=0;tail=0;enabled=0;next_accept=0;ptr=0;rh=0;rt=0;ru=0;last_accept=-99;
 end else if(ea)begin
  if(ref_fire)begin rh=(rh+1)%8;ru=ru-1;reference_outputs=reference_outputs+1;end
  if(ef)begin head=(head+1)%1;used=used-1;consumed=consumed+1;end
  if(ep)begin
   if(enabled-last_accept<2)$fatal(1,"overlap interval");if(enabled-last_accept==2)ii2=ii2+1;last_accept=enabled;
   loan_g=in_g;loan_h=in_h;loan_spec=in_spec;
   expected[tail]=gold[ptr];due[tail]=enabled+2;tail=(tail+1)%1;used=used+1;
   ref_expected[rt]=gold[ptr];rt=(rt+1)%8;ru=ru+1;
   next_accept=enabled+2;ptr=ptr+1;accepted=accepted+1;
  end
  enabled=enabled+1;
 end
 tick;
 if(ptr==INPUTS&&used==0&&ru==0)begin
  if(full==0||held==0||denied==0||frozen==0||simultaneous==0||directpop==0||fallbackpop==0||ii2==0||consumed+discarded!=accepted)$fatal(1,"coverage/accounting");
  $display("PASS two_cycle matched Final wall%0d inputs%0d outputs%0d discarded%0d full%0d held%0d CE0%0d denied%0d simultaneous%0d II2%0d directpop%0d fallbackpop%0d reference%0d",wall,accepted,consumed,discarded,full,held,frozen,denied,simultaneous,ii2,directpop,fallbackpop,reference_outputs);
  match_reference=0;ce=1;commit_allow=0;in_valid=1;in_h=9'd257;#2;
  if(!preflight_bad||advance||in_accept||out_fire)$fatal(1,"bad-domain raw preflight with denial");
  tick;if(!fault||credits!=0||queued!=0)$fatal(1,"bad-domain terminal");
  reset=1;#2;tick;reset=0;in_valid=1;in_h=0;commit_allow=1;#2;tick;
  before_credits=credits;before_queued=queued;abort=1;#2;
  if(!fault||advance||in_accept||out_valid||out_fire)$fatal(1,"abort preflight");
  repeat(8)begin tick;if(credits!==before_credits||queued!==before_queued)$fatal(1,"abort hold");end
  $display("PASS two_cycle malformed-domain/reset/abort hold8");$finish;
 end
end
$fatal(1,"edge watchdog");end
initial begin #1000000;$fatal(1,"time watchdog");end
endmodule
"#;

#[test]
#[ignore = "requires Icarus and Gowin primitive simulation library"]
fn two_cycle_final_matched_actual_portable_native() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-final-two_cycle-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    for native in [false, true] {
        std::fs::write(
            dir.join(format!("unchanged-shared-native-{native}.v")),
            ordered_shared::source(native),
        )
        .unwrap();
        for borrow in [false, true] {
            std::fs::write(
                dir.join(format!(
                    "unchanged-single-borrow-{borrow}-native-{native}.v"
                )),
                ordered_shared::source_single(native, borrow),
            )
            .unwrap();
        }
    }
    for borrow in [false, true] {
        std::fs::write(dir.join("tb.v"), fixture(borrow)).unwrap();
        std::fs::write(dir.join(format!("tb-borrow-{borrow}.v")), fixture(borrow)).unwrap();
        for native in [false, true] {
            let name = format!("borrow-{borrow}-native-{native}.v");
            let leaf = ordered_shared::source_two_cycle(native, borrow);
            let source = format!(
                "{}\n{}",
                leaf,
                ordered_compact::source(native, Profile::Folded)
            );
            std::fs::write(dir.join(&name), &source).unwrap();
            std::fs::write(
                dir.join(format!("leaf-borrow-{borrow}-native-{native}.v")),
                &leaf,
            )
            .unwrap();
            let mut compile = std::process::Command::new(
                std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
            );
            compile
                .current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
            if native {
                compile.arg("-DNATIVE").arg(
                    std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            }
            let compiled = compile.args([&name, "tb.v"]).output().unwrap();
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
                "two_cycle borrow={borrow} native={native}: {} {}",
                String::from_utf8_lossy(&ran.stdout),
                String::from_utf8_lossy(&ran.stderr)
            );
            println!(
                "two_cycle borrow={borrow} native={native}: {}",
                String::from_utf8_lossy(&ran.stdout)
            );
        }
    }
}
