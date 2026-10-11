//! Paid descriptor control capture, independent wide queue and hard-DO loans.
//! This qualifies the isolated lowering and head lifetime, not the whole top.
use gpu_v2::system::pixel::row_store;
#[path = "../src/system/pixel/postquad_descriptor.rs"]
mod descriptor;
use std::{collections::VecDeque, fmt::Write, fs, path::Path, process::Command};

const WALLS: usize = 4096;
const PARENT: &str = r#"module gpu_v2_postquad(input clk,reset,ce,deny,in_valid,
 input[22:0]in_begin,input ack_request,input[1:0]branch_ready);
 wire advance=ce&&!reset&&!deny;
 reg[5:0]publish_ptr,retire_ptr;
 wire[4:0]published_slot=publish_ptr[4:0],retire_slot=retire_ptr[4:0];
 wire published;
 wire[22:0]published_begin=in_begin;
 wire quad_accepted;
 wire light_collision=0,sample_collision=0,basic_collision=0,draw_rebind=0,pipeline_idle=1;
 (* syn_ramstyle="distributed_ram" *)reg[22:0]descriptors[0:31];
 reg[5:0]published_count;
 reg active;reg[22:0]header;
 reg stopped;
 @FRONTIER@
 assign published=advance&&in_valid&&published_count<32;
 assign quad_accepted=advance&&active&&ack_request;
 always @(posedge clk)begin
  if(reset)begin published_count<=0;publish_ptr<=0;retire_ptr<=0;active<=0;stopped<=0;end
  else begin
   if(light_collision||sample_collision||basic_collision||(draw_rebind&&!pipeline_idle))stopped<=1;
   if(advance)begin
    case({published,quad_accepted})
     2'b10:published_count<=published_count+1'b1;
     2'b01:published_count<=published_count-1'b1;
    endcase
    if(published)begin descriptors[published_slot]<=published_begin;publish_ptr<=publish_ptr+1'b1;end
    if(!active&&published_count!=0&&@READY@)begin active<=1;header<=descriptors[retire_slot];end
    if(quad_accepted)begin active<=0;retire_ptr<=retire_ptr+1'b1;end
   end
  end
 end
endmodule
"#;

fn parent(frontier: bool) -> String {
    PARENT
        .replace(
            "@FRONTIER@",
            if frontier {
                "wire[22:0]head_descriptor=descriptors[retire_slot];"
            } else {
                ""
            },
        )
        .replace(
            "@READY@",
            if frontier {
                "(head_descriptor[5]||branch_ready[0])&&(head_descriptor[6]||branch_ready[1])"
            } else {
                "(&branch_ready)"
            },
        )
}

#[test]
fn static_control_lowering_preserves_frozen_factory() {
    for frontier in [false, true] {
        let original = parent(frontier);
        for native in [false, true] {
            let lowered = descriptor::lower_control(&original, native).unwrap();
            assert!(original.contains("reg[22:0]header;"));
            assert!(!lowered.contains("reg[22:0]header;"));
            assert!(!lowered.contains("descriptors["));
            assert!(lowered.contains("if(descriptor_return_valid)head_loaded<=1;"));
            assert!(lowered.contains("published_count!=0&&head_loaded&&"));
            assert!(lowered.contains("output reg [5:0] control"));
            let frozen = descriptor::lower(&original, native).unwrap();
            assert!(frozen.contains("(head_loaded||descriptor_return_valid)"));
            assert!(!frozen.contains("descriptor_control"));
            assert!(
                lowered.contains("/* The already paid descriptor DO owns this active quad. */end")
            );
            if native {
                assert!(lowered.contains("SDPX9B"));
                assert!(!lowered.contains("ram["));
            }
            assert!(descriptor::lower_control(&lowered, native).is_err());
        }
    }
}

fn bench() -> String {
    let mut tb = r#"module tb;
 reg clk=0,reset,ce,deny,in_valid,ack_request;
 reg[22:0]in_begin;reg[1:0]branch_ready;
 gpu_v2_postquad dut(.*);
 initial begin
"#
    .to_owned();
    for wall in 0..WALLS {
        let reset = wall == 0 || wall == 1300 || wall == 2600;
        let ce = wall % 17 != 2 && wall % 17 != 3;
        let deny = wall % 13 == 4 || (700..735).contains(&wall);
        let offer = wall % 5 != 1;
        let ack = wall % 7 < 4;
        let branches = if wall % 31 < 7 {
            ((wall / 9) ^ (wall / 23)) & 3
        } else {
            3
        };
        let data = ((wall * 0x345a7) ^ (wall << 9) ^ 0x71abc) & 0x7fffff | 1;
        writeln!(tb," reset={};ce={};deny={};in_valid={};ack_request={};branch_ready=2'd{};in_begin=23'h{:x};#5;",u8::from(reset),u8::from(ce),u8::from(deny),u8::from(offer),u8::from(ack),branches,data).unwrap();
        writeln!(tb," $display(\"EDGE {wall} %d %d %d %d %h %d %d %d %d %d %d %d %h %d %d %h\",reset,dut.advance,dut.published,dut.published_slot,in_begin,dut.quad_accepted,dut.retire_slot,dut.published_count,dut.active,dut.head_loaded,dut.descriptor_return_valid,dut.descriptor_read,dut.header_do,dut.descriptor_collision,dut.stopped,dut.header);clk=1;#5;clk=0;").unwrap();
    }
    tb.push_str(" $finish;end\n initial begin #50000;$fatal(1,\"wall budget\");end\nendmodule\n");
    tb
}

#[derive(Default)]
struct Coverage {
    published: usize,
    retired: usize,
    returns_ce0: usize,
    held: usize,
    denied: usize,
    full: usize,
    simultaneous: usize,
    live_reset: usize,
}

fn check(trace: &str, frontier: bool) -> Coverage {
    let mut queue: VecDeque<(usize, u32)> = VecDeque::new();
    let mut prior_read = false;
    let mut prior_active = false;
    let mut prior_advance = false;
    let mut prior_ack = false;
    let mut coverage = Coverage::default();
    let mut edges = 0;
    for line in trace.lines().filter(|l| l.starts_with("EDGE ")) {
        let words: Vec<_> = line.split_whitespace().collect();
        let wall: usize = words[1].parse().unwrap();
        let bit = |i: usize| words[i].parse::<usize>().unwrap();
        let reset = bit(2) != 0;
        // Initial pre-reset FFs may be X; reset ownership is checked first.
        if reset {
            coverage.live_reset += usize::from(!queue.is_empty());
            queue.clear();
            prior_read = false;
            prior_active = false;
            prior_advance = false;
            prior_ack = false;
            edges += 1;
            continue;
        }
        let advance = bit(3) != 0;
        let published = bit(4) != 0;
        let slot = bit(5);
        let data = u32::from_str_radix(words[6], 16).unwrap();
        let ack = bit(7) != 0;
        let retire = bit(8);
        let count = bit(9);
        let active = bit(10) != 0;
        let loaded = bit(11) != 0;
        let returned = bit(12) != 0;
        let read = bit(13) != 0;
        // Reset dominates all owners; payload may remain old/unknown before reset.
        assert_eq!(count, queue.len(), "published owner count edge{wall}");
        assert_eq!(returned, prior_read, "one paid wall edge{wall}");
        if prior_active && !prior_ack {
            assert!(active, "active header lost edge{wall}");
        }
        if !prior_advance {
            assert_eq!(active, prior_active, "CE/deny changed active edge{wall}");
        }
        if active || loaded || returned {
            let expected = queue
                .front()
                .expect("paid header requires real published owner");
            assert_eq!(retire, expected.0, "global head identity edge{wall}");
            assert_eq!(
                u32::from_str_radix(words[14], 16).unwrap(),
                expected.1,
                "wide oracle header edge{wall}"
            );
        }
        if active || loaded {
            assert!(loaded, "activation requires captured controls edge{wall}");
            assert_eq!(
                u32::from_str_radix(words[17], 16).unwrap(),
                queue.front().unwrap().1,
                "captured control plus borrowed origin edge{wall}"
            );
        }
        assert_eq!(bit(15), 0, "descriptor collision edge{wall}");
        assert_eq!(bit(16), 0, "terminal edge{wall}");
        if read {
            assert!(advance && !active && !loaded && !returned && !queue.is_empty());
            assert!(!ack, "same-edge retire cannot fund read");
            assert!(
                !published || slot != retire,
                "same-address writer edge{wall}"
            );
        }
        if ack {
            assert!(advance && active && loaded);
            queue.pop_front().unwrap();
            coverage.retired += 1;
        }
        if published {
            assert!(queue.iter().all(|(old, _)| *old != slot));
            queue.push_back((slot, data));
            coverage.published += 1;
        }
        coverage.returns_ce0 += usize::from(returned && !advance);
        coverage.held += usize::from((loaded || active) && !ack);
        coverage.denied += usize::from(!advance);
        coverage.full += usize::from(count == 32);
        coverage.simultaneous += usize::from(ack && published);
        prior_read = read;
        prior_active = active;
        prior_advance = advance;
        prior_ack = ack;
        edges += 1;
    }
    assert_eq!(edges, WALLS);
    assert!(coverage.published > 300 && coverage.retired > 300);
    assert!(coverage.returns_ce0 > 0 && coverage.held > 100 && coverage.denied > 100);
    assert!(coverage.full > 0 && coverage.simultaneous > 0 && coverage.live_reset == 2);
    println!("descriptor frontier={frontier}: pub{} retire{} CE0-return{} held{} deny{} full{} sameedge{} live-reset{}",coverage.published,coverage.retired,coverage.returns_ce0,coverage.held,coverage.denied,coverage.full,coverage.simultaneous,coverage.live_reset);
    coverage
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME native primitive simulation"]
fn actual_paid_control_portable_native() {
    for frontier in [false, true] {
        for native in [false, true] {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../target/postquad-descriptor-control-20261010/{frontier}-{native}"
            ));
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("dut.v"),
                descriptor::lower_control(&parent(frontier), native).unwrap(),
            )
            .unwrap();
            fs::write(dir.join("tb.v"), bench()).unwrap();
            let mut compile =
                Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
            compile
                .current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"]);
            if native {
                compile.arg(
                    Path::new(&std::env::var_os("GOWIN_HOME").expect("native cells"))
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            }
            let c = compile.output().unwrap();
            assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
            let r = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
                .current_dir(&dir)
                .arg("test.vvp")
                .output()
                .unwrap();
            assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stdout));
            fs::write(dir.join("trace.log"), &r.stdout).unwrap();
            check(&String::from_utf8_lossy(&r.stdout), frontier);
        }
    }
}
