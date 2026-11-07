//! Independent division gold and enabled-time/ordered-credit scoreboard.
use gpu_v2::system::pixel::final_stage::rtl::ordered_compact::{self, Profile};
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
    // Every u8*u8 product actually travels through each RTL profile.
    for i in 0..65536_usize.div_ceil(3) {
        result.push(Pixel {
            tint: std::array::from_fn(|c| (((i * 3 + c) % 65536) >> 8) as u8),
            tex: std::array::from_fn(|c| ((i * 3 + c) % 256) as u8),
            spec: [0; 3],
            g: 256,
            h: 0,
        });
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
fn exact_arithmetic_bounds() {
    for sum in 0..=255 * (511 + 256) {
        let floor = sum / 256;
        let remainder = sum % 256;
        let independent =
            (floor + u32::from(remainder > 128 || (remainder == 128 && floor % 2 == 1))).min(255);
        let increment = (sum & 128) != 0 && ((sum & 127) != 0 || (sum & 256) != 0);
        let compact = if sum >> 16 != 0 || ((sum >> 8) & 255) == 255 {
            255
        } else {
            ((sum >> 8) & 255) + u32::from(increment)
        };
        assert_eq!(independent, compact, "sum {sum}");
    }
}

fn fixture() -> String {
    let pixels = pixels();
    let mut init = String::new();
    for (i, p) in pixels.iter().copied().enumerate() {
        writeln!(init, "tints[{i}]=24'h{:06x};textures[{i}]=24'h{:06x};specs[{i}]=24'h{:06x};gs[{i}]=9'd{};hs[{i}]=9'd{};gold[{i}]=24'h{:06x};", packed(p.tint), packed(p.tex), packed(p.spec), p.g, p.h, packed(rgb(p))).unwrap();
    }
    TB.replace("INPUTS", &pixels.len().to_string())
        .replace("// GOLD", &init)
}

const TB: &str = r#"module tb;
reg clk=0,reset=1,ce=0,commit_allow=1,abort=0,in_valid=0,out_ready=0;
reg[23:0]in_tint=0,in_tex=0,in_spec=0;reg[8:0]in_g=0,in_h=0;
wire in_ready,in_accept,advance,preflight_bad,fault,out_valid,out_fire;
wire[23:0]out_rgb;wire[3:0]credits,queued;
gpu_v2_final_ordered_compact dut(.*);
`ifdef NATIVE
GSR GSR(.GSRI(1'b1));
`endif
reg[23:0]tints[0:INPUTS-1],textures[0:INPUTS-1],specs[0:INPUTS-1],gold[0:INPUTS-1];
reg[8:0]gs[0:INPUTS-1],hs[0:INPUTS-1];
reg[23:0]expected[0:7];integer due[0:7];
integer ptr=0,head=0,tail=0,used=0,enabled=0,wall,k,q,index;
integer accepted=0,consumed=0,discarded=0,full=0,held=0,denied=0,frozen=0,simultaneous=0;
reg er,ev,ea,ep,ef,eb;reg[3:0]before_credits,before_queued;
task tick;begin clk=1;#2;clk=0;#2;end endtask
initial begin
// GOLD
#2;tick;reset=0;
for(wall=0;wall<100000;wall=wall+1)begin
 reset=(wall==117);ce=(wall%7!=3)&&(wall%17!=13);commit_allow=(wall%31!=6)&&(wall%31!=7);
 out_ready=(wall>90)&&(wall%11!=5)&&(wall%29<22);in_valid=(ptr<INPUTS)&&(wall%13!=10);
 if(in_valid)begin in_tint=tints[ptr];in_tex=textures[ptr];in_spec=specs[ptr];in_g=gs[ptr];in_h=hs[ptr];end
 #2;
 q=0;for(k=0;k<used;k=k+1)begin index=(head+k)%8;if(due[index]<=enabled)q=q+1;end
 er=ce&&!reset&&(used<8);ev=!reset&&(used!=0)&&(due[head]<=enabled);ea=ce&&!reset&&commit_allow;
 ep=ea&&in_valid&&(used<8);ef=ea&&out_ready&&ev;
 if(fault||preflight_bad||in_ready!==er||out_valid!==ev||advance!==ea||in_accept!==ep||out_fire!==ef||credits!==used||queued!==q)$fatal(1,"ordered controls wall%0d used%0d q%0d enable%0d",wall,used,q,enabled);
 if(ev&&out_rgb!==expected[head])$fatal(1,"RGB wall%0d got%h want%h",wall,out_rgb,expected[head]);
 if(used==8)full=full+1;if(ev&&!ef)held=held+1;if(!ce&&used>0)frozen=frozen+1;if(ce&&!commit_allow&&used>0)denied=denied+1;if(ep&&ef)simultaneous=simultaneous+1;
 if(reset)begin
  if(used==0)$fatal(1,"reset not active");discarded=discarded+used;used=0;head=0;tail=0;enabled=0;ptr=0;
 end else if(ea)begin
  if(ef)begin head=(head+1)%8;used=used-1;consumed=consumed+1;end
  if(ep)begin expected[tail]=gold[ptr];due[tail]=enabled+5;tail=(tail+1)%8;used=used+1;ptr=ptr+1;accepted=accepted+1;end
  enabled=enabled+1;
 end
 tick;
 if(ptr==INPUTS&&used==0)begin
  if(full==0||held==0||denied==0||frozen==0||simultaneous==0||consumed+discarded!=accepted)$fatal(1,"coverage/accounting");
  $display("PASS ordered Final wall%0d inputs%0d outputs%0d discarded%0d full%0d held%0d CE0%0d denied%0d simultaneous%0d",wall,accepted,consumed,discarded,full,held,frozen,denied,simultaneous);
  ce=1;commit_allow=0;in_valid=1;in_h=9'd257;#2;
  if(!preflight_bad||advance||in_accept||out_fire)$fatal(1,"bad-domain raw preflight with denial");
  tick;if(!fault||credits!=0||queued!=0)$fatal(1,"bad-domain terminal");
  reset=1;#2;tick;reset=0;in_valid=1;in_h=0;commit_allow=1;#2;tick;
  before_credits=credits;before_queued=queued;abort=1;#2;
  if(!fault||advance||in_accept||out_valid||out_fire)$fatal(1,"abort preflight");
  repeat(8)begin tick;if(credits!==before_credits||queued!==before_queued)$fatal(1,"abort hold");end
  $display("PASS Final malformed-domain/reset/abort hold8");$finish;
 end
end
$fatal(1,"edge watchdog");end
initial begin #1000000;$fatal(1,"time watchdog");end
endmodule
"#;

#[test]
#[ignore = "requires Icarus and Gowin primitive simulation library"]
fn compact_final_actual_portable_native() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-final-compact-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), fixture()).unwrap();
    for (profile_name, profile) in [
        ("folded", Profile::Folded),
        ("threshold", Profile::Threshold),
    ] {
        for native in [false, true] {
            let name = format!("{profile_name}-native-{native}.v");
            std::fs::write(dir.join(&name), ordered_compact::source(native, profile)).unwrap();
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
                "{profile_name} native={native}: {} {}",
                String::from_utf8_lossy(&ran.stdout),
                String::from_utf8_lossy(&ran.stderr)
            );
            println!(
                "{profile_name} native={native}: {}",
                String::from_utf8_lossy(&ran.stdout)
            );
        }
    }
}
