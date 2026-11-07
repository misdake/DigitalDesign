//! Actual Pool-borrowed cache; immutable refill bytes and canonical tap gold.
use gpu_v2::texture::{
    ports::{Group4, Slot, TileKey},
    rtl::runtime_cache,
};
use std::{collections::BTreeSet, fmt::Write, path::Path, process::Command};

const GROUPS: usize = 192;
fn slots() -> Vec<Slot> {
    (0..16)
        .map(|i| Slot {
            base_address: 0x100000 + i * 0x10000,
            has_full_mip: true,
            max_size_log2: 6,
            valid: i != 15,
        })
        .collect()
}
fn word(address: u64) -> u16 {
    let i = address / 2;
    ((i.wrapping_mul(12345) ^ (i >> 9) ^ 0xa563) & 65535) as u16
}
fn group(i: usize) -> Group4 {
    let (slot, n, x, y) = if i < 64 {
        (0, 6, 0, 0)
    } else if i < 100 {
        ((i % 15) as u8, 6, ((i / 15) & 1) as u8 * 4, 0)
    } else {
        let n = (i % 7) as u8;
        let side = 1 << n.saturating_sub(3);
        (
            (i % 15) as u8,
            n,
            ((i * 3) % side) as u8,
            ((i * 5) % side) as u8,
        )
    };
    Group4 {
        key: TileKey { slot, n, x, y },
        top_left_local: [(i & 7) as u8, ((i >> 3) & 7) as u8],
        coefficients: [1, 255, 254, 1],
        first: i & 1 == 0,
        last: i & 1 != 0,
        quad_id: (i % 16) as u8,
        lane: ((i / 16) % 4) as u8,
    }
}
fn bench() -> String {
    let slots = slots();
    let mut init = String::new();
    let mut addresses = BTreeSet::new();
    for i in 0..GROUPS {
        let g = group(i);
        let address = g.key.address(&slots).unwrap();
        addresses.insert(address);
        writeln!(init, "gold[{i}]=72'h{:x};", g.pack72().unwrap()).unwrap();
        for t in 0..4 {
            let x = (u64::from(g.top_left_local[0]) + (t & 1)) & 7;
            let y = (u64::from(g.top_left_local[1]) + (t >> 1)) & 7;
            writeln!(
                init,
                "tex[{i}][{t}]=16'h{:x};",
                word(address + 2 * (y * 8 + x))
            )
            .unwrap();
        }
    }
    let mut cases = String::new();
    for a in addresses {
        for beat in 0..16 {
            let data = (0..4).fold(0u64, |w, j| {
                w | (u64::from(word(a + beat * 8 + j * 2)) << (j * 16))
            });
            writeln!(cases, "36'h{:x}:refill=64'h{data:x};", (a << 4) | beat).unwrap();
        }
    }
    BENCH.replace("@INIT@", &init).replace("@REFILL@", &cases)
}

const BENCH: &str = r#"`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=0,allow=1,abort=0,iv=0,oready=0;
reg[71:0]ip=0;wire pr,ir,bad,advance,ov,fault,req,drained;
wire[71:0]op;wire[15:0]t0,t1,t2,t3;wire[31:0]addr;
reg mr=0,bv=0,complete=0,ok=1;reg[3:0]bi=0;reg[63:0]bd=0;
gpu_v2_texture_runtime_cache dut(.clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(allow),.abort(abort),
 .in_valid(iv),.in_payload(ip),.proposal_ready(pr),.in_ready(ir),.preflight_bad(bad),.advance(advance),
 .out_ready(oready),.out_valid(ov),.out_payload(op),.out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),
 .mem_req_valid(req),.mem_req_addr(addr),.mem_req_ready(mr),.mem_resp_valid(bv),.mem_resp_index(bi),
 .mem_resp_data(bd),.mem_resp_complete(complete),.mem_resp_ok(ok),.drained(drained),.fault(fault));
@GSR@
reg[71:0]gold[0:191];reg[15:0]tex[0:191][0:3];
function[63:0]refill;input[31:0]a;input[3:0]b;begin case({a,b})
@REFILL@
default:begin refill=64'bx;$fatal(1,"unexpected refill address %h beat%0d",a,b);end
endcase end endfunction
integer wall=0,src=0,pop=0,reads=0,bursts=0,next=0,wait_ack=0;
integer hold=0,denied=0,paid0=0,tag0=0,lateack=0,joined=0,hits=0,i,j;
reg active=0,last_read=0,had_output=0,old_rd,old_res,old_advance,got_read;
reg[31:0]mem_addr=0,rng=32'h54159af2;
reg[71:0]held_payload;reg[63:0]held_tex;
task tick_edge;begin clk=1;#5;clk=0;#5;wall=wall+1;if(wall>12000)$fatal(1,"tick_edge watchdog");end endtask
task reset_leaf;begin
 reset=1;ce=0;allow=1;abort=0;iv=0;oready=0;mr=0;bv=0;complete=0;#2;tick_edge;
 reset=0;#2;if(fault||ov||!drained||dut.tag_valid||dut.rd_valid)$fatal(1,"reset ownership");
end endtask
task quiet;begin bv=0;complete=0;mr=0;end endtask
task request_zero;begin
 ce=1;allow=1;iv=1;ip=gold[0];oready=0;quiet;
 for(j=0;j<12&&!req;j=j+1)begin #2;tick_edge;end
 if(!req)$fatal(1,"request watchdog");
 #2;tick_edge; // Offered but unaccepted request must remain presented.
end endtask
task refill_zero;begin
 mr=1;bv=1;bi=0;bd=refill(addr,0);#2;tick_edge;mr=0;
 for(j=1;j<16;j=j+1)begin ce=0;allow=0;bi=j;bd=refill(addr,j);#2;tick_edge;end
 bv=0;for(j=0;j<3;j=j+1)begin #2;tick_edge;if(drained)$fatal(1,"beat15 is not terminal");end
 complete=1;#2;tick_edge;complete=0;if(!drained)$fatal(1,"late terminal drain");
end endtask
initial begin
@INIT@
 #2;tick_edge;reset=0;
 for(i=0;i<10000;i=i+1)begin
  rng=rng^(rng<<13);rng=rng^(rng>>17);rng=rng^(rng<<5);
  ce=(rng[2:0]!=0);allow=(rng[7:4]!=0);oready=(wall>200)&&(rng[11:9]!=0);
  if(last_read)begin ce=0;allow=0;end
  iv=src<192;ip=iv?gold[src]:72'b0;
  mr=(wall%5!=0);bv=0;complete=0;
  #2;
  if(active||(req&&mr))begin
   if(!active)begin active=1;next=0;mem_addr=addr;bursts=bursts+1;wait_ack=5;end
   if(next<16&&(wall%3!=1))begin
    bv=1;bi=next;bd=refill(mem_addr,next);next=next+1;
   end
   if(next==16)begin
    if(bv&&(bursts%2==0))begin complete=1;joined=joined+1;end
    else if(!bv)begin
     if(wait_ack==0)begin complete=1;lateack=lateack+1;end
     else wait_ack=wait_ack-1;
    end
   end
  end
  #2;
  if(fault||bad)$fatal(1,"healthy fault wall%0d src%0d bad%b",wall,src,bad);
  if(!allow&&pr)denied=denied+1;
  if(!ce&&dut.tag_read)tag0=tag0+1;
  if(req&&dut.desc_presented&&addr!==mem_addr&&active)$fatal(1,"held request address");
  if(ov)begin
   if(pop>=reads||op!==gold[pop]||{t3,t2,t1,t0}!=={tex[pop][3],tex[pop][2],tex[pop][1],tex[pop][0]})
    $fatal(1,"canonical captured row%0d wall%0d got%h expected%h tex%h expected%h",pop,wall,op,gold[pop],{t3,t2,t1,t0},{tex[pop][3],tex[pop][2],tex[pop][1],tex[pop][0]});
   if(had_output&&(op!==held_payload||{t3,t2,t1,t0}!==held_tex))$fatal(1,"output under backpressure");
  end
  old_rd=dut.rd_valid;old_res=dut.res_valid;old_advance=advance;
  last_read=ir;had_output=ov&&!(advance&&oready);held_payload=op;held_tex={t3,t2,t1,t0};
  if(ov&&!advance)hold=hold+1;
  if(advance&&ov&&oready)pop=pop+1;
  if(ir)begin if(!advance||!pr)$fatal(1,"consume guard");src=src+1;reads=reads+1;if(src<=64)hits=hits+1;end
  if(!advance&&(ir||dut.tag_write))$fatal(1,"denied mutation");
  tick_edge;
  if(old_rd&&!old_res)begin
   if(!dut.res_valid||dut.rd_valid)$fatal(1,"paid read capture independent of CE");
   if(!old_advance)paid0=paid0+1;
  end
  if(complete)active=0;
  if(pop==192&&!active&&drained)begin
   if(src!=192||reads!=192||hold==0||denied==0||paid0==0||tag0==0||lateack==0||joined==0||hits!=64)$fatal(1,"coverage");
   $display("PASS runtime cache groups%0d reads%0d requests%0d walls%0d held%0d deny%0d paidCE0%0d tagCE0%0d delayedACK%0d joinedACK%0d",pop,reads,bursts,wall,hold,denied,paid0,tag0,lateack,joined);
   i=10000;
  end
 end
 if(pop!=192)$fatal(1,"flow watchdog");
 // SRAM reset masks must hide stale lines; capture one freshly refilled row.
 reset_leaf;request_zero;refill_zero;ce=1;allow=1;
 got_read=0;
 for(j=0;j<12&&!got_read;j=j+1)begin #2;if(ir)got_read=1;tick_edge;end
 if(!got_read)$fatal(1,"post-reset read watchdog");iv=0;ce=0;allow=0;#2;tick_edge;
 if(!ov||op!==gold[0]||t0!==tex[0][0])$fatal(1,"post-reset paid row");
 held_payload=op;held_tex={t3,t2,t1,t0};
 // Bad raw proposal cannot hide under a foreign denial or pop a held row.
 ce=1;allow=0;iv=1;ip=gold[0];ip[3:0]=15;oready=1;#2;
 if(!bad||advance||ir)$fatal(1,"invalid slot preflight");tick_edge;
 if(!fault)$fatal(1,"terminal local fault");
 for(j=0;j<8;j=j+1)begin ce=1;allow=1;iv=0;#2;tick_edge;
  if(advance||ov||op!==held_payload||{t3,t2,t1,t0}!==held_tex)$fatal(1,"fault retained capture");end
 // Level and coordinate guards also see raw attempts before foreign denial.
 for(i=0;i<2;i=i+1)begin
  reset_leaf;ce=1;allow=0;iv=1;ip=gold[0];
  if(i==0)ip[7:4]=11;else begin ip[7:4]=3;ip[14:8]=1;end
  #2;if(!bad||advance||ir)$fatal(1,"malformed TileKey proposal");tick_edge;
  if(!fault||!drained)$fatal(1,"malformed TileKey terminal");
 end
 // A producer violating the borrowed miss head cannot redirect its refill.
 reset_leaf;request_zero;ip[14:8]=1;allow=0;#2;
 if(!bad||advance||ir)$fatal(1,"borrowed address proposal");tick_edge;
 if(!fault)$fatal(1,"borrowed address terminal");ce=0;refill_zero;
 if(!drained||ov)$fatal(1,"borrowed address drain");
 // Abort a presented request: acceptance plus beat0 still drains at CE0.
 reset_leaf;request_zero;abort=1;ce=0;allow=0;refill_zero;
 if(!fault||!drained||ov)$fatal(1,"abort drain publication");
 // Duplicate beat on CE0 is a wall-protocol fault, then correct tail drains.
 reset_leaf;request_zero;mr=1;bv=1;bi=0;bd=refill(addr,0);#2;tick_edge;mr=0;
 ce=0;allow=0;bi=0;#2;if(!bad)$fatal(1,"duplicate beat preflight CE0");tick_edge;
 if(!fault||drained)$fatal(1,"protocol terminal state");
 for(j=1;j<16;j=j+1)begin bi=j;bd=refill(addr,j);#2;tick_edge;end
 bv=0;complete=1;#2;tick_edge;complete=0;if(!drained||ov)$fatal(1,"protocol drain");
 // An early terminal is an error even when no beat shares that wall edge.
 reset_leaf;request_zero;mr=1;bv=1;bi=0;bd=refill(addr,0);#2;tick_edge;
 mr=0;bv=0;complete=1;ce=0;allow=0;#2;
 if(!bad||advance)$fatal(1,"early terminal preflight");tick_edge;complete=0;
 if(!fault||!drained||ov)$fatal(1,"early terminal state");
 reset_leaf;ce=1;allow=1;quiet;iv=0;oready=1;
 for(j=0;j<5;j=j+1)begin #2;tick_edge;if(fault||ov||ir)$fatal(1,"reset clean ownership");end
 $display("PASS runtime cache reset/TileKey/borrow/abort/duplicate-beat/early-terminal drain");$finish;
end
initial begin #1000000;$fatal(1,"time watchdog");end
endmodule
"#;

fn bounded(mut command: Command, dir: &Path, name: &str) {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    command.stdout(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
    ));
    command.stderr(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
    ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("process watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn borrowed_profile_has_one_group_owner_and_validates_slots() {
    let s = runtime_cache::verilog(&slots(), false).unwrap();
    assert!(!s.contains("reg [71:0] head_payload"));
    assert!(!s.contains("rd_payload"));
    assert!(s.contains("reg[71:0]res_payload"));
    assert!(s.contains(".CEB(read_issue),.OCE(1'b1)"));
    assert!(runtime_cache::verilog(&[], false).is_err());
}

#[test]
#[ignore = "requires Icarus and native Gowin SRAM"]
fn actual_borrowed_cache_qualifies() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-runtime-cache-next-20261010");
    for native in [false, true] {
        let dir = base.join(if native { "native" } else { "portable" });
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cache.v"),
            runtime_cache::verilog(&slots(), native).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("tb.v"),
            bench().replace("@GSR@", if native { "GSR GSR(.GSRI(1'b1));" } else { "" }),
        )
        .unwrap();
        let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        command
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "test.vvp", "cache.v", "tb.v"]);
        if native {
            command.arg(
                Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        bounded(command, &dir, "compile");
        let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
        command.current_dir(&dir).arg("test.vvp");
        bounded(command, &dir, "run");
        println!(
            "native={native}: {}",
            std::fs::read_to_string(dir.join("run.out")).unwrap()
        );
    }
}

#[test]
#[ignore = "requires Icarus and native Gowin SRAM"]
fn actual_pipelined_cache_qualifies() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-runtime-cache-pipeline-20261010");
    for native in [false, true] {
        let dir = base.join(if native { "native" } else { "portable" });
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cache.v"),
            runtime_cache::verilog_pipelined(&slots(), native).unwrap(),
        )
        .unwrap();
        let tb = bench()
            .replace("wire[71:0]op;", "wire look_valid;wire[21:0]look_key;wire[71:0]op;")
            .replace(".in_valid(iv),.in_payload(ip),", ".in_valid(iv),.in_payload(ip),.next_valid(look_valid),.next_key(look_key),")
            .replace("integer hold=0", "assign look_valid=src<191;assign look_key=look_valid?gold[src+1][21:0]:22'd0;\ninteger consecutive=0,previous_read=-2;\ninteger hold=0")
            .replace("if(last_read)begin ce=0;allow=0;end", "if(last_read&&src<2)begin ce=0;allow=0;end")
            .replace("if(ir)begin if(!advance", "if(ir)begin if(wall-previous_read==1)consecutive=consecutive+1;previous_read=wall;if(!advance")
            .replace("if(old_rd&&!old_res)begin", "if(old_rd&&(!old_res||(old_advance&&oready)))begin")
            .replace("if(!dut.res_valid||dut.rd_valid)", "if(!dut.res_valid)")
            .replace("if(src!=192||reads!=192", "if(consecutive==0||src!=192||reads!=192")
            .replace("@GSR@", if native { "GSR GSR(.GSRI(1'b1));" } else { "" });
        std::fs::write(dir.join("tb.v"), tb).unwrap();
        let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        command
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "test.vvp", "cache.v", "tb.v"]);
        if native {
            command.arg(
                Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        bounded(command, &dir, "compile");
        let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
        command.current_dir(&dir).arg("test.vvp");
        bounded(command, &dir, "run");
        println!(
            "pipelined native={native}: {}",
            std::fs::read_to_string(dir.join("run.out")).unwrap()
        );
    }
}

#[test]
#[ignore = "requires Icarus and native Gowin SRAM"]
fn actual_pool_closed_matches_checked_legal_flow() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-runtime-cache-next-20261010/pool-closed-experiment");
    for native in [false, true] {
        let dir = base.join(if native { "native" } else { "portable" });
        std::fs::create_dir_all(&dir).unwrap();
        let checked = runtime_cache::verilog(&slots(), native).unwrap();
        let closed = runtime_cache::verilog_pool_closed(&slots(), native).unwrap();
        assert_eq!(
            closed.replace(
                " wire borrow_bad=1'b0; // Actual ordered Pool holds Group until read_issue.",
                " wire borrow_bad=running&&desc_valid&&(!head_valid||hk_addr!=desc_addr);",
            ),
            checked,
            "Closed factory must change only the discharged borrowed-address guard"
        );
        std::fs::write(
            dir.join("checked.v"),
            checked.replace(runtime_cache::MODULE, "cache_checked"),
        )
        .unwrap();
        std::fs::write(
            dir.join("closed.v"),
            closed.replace(runtime_cache::MODULE, "cache_closed"),
        )
        .unwrap();
        let original = bench();
        let legal_end = original.find(" // SRAM reset masks").unwrap();
        let mut matched = original[..legal_end].to_owned();
        matched.push_str(
            " $display(\"PASS checked/pool-closed legal flow public ports every edge\");$finish;\nend\ninitial begin #1000000;$fatal(1,\"time watchdog\");end\nendmodule\n",
        );
        matched = matched
            .replace("gpu_v2_texture_runtime_cache dut(", "cache_checked dut(")
            .replace("@GSR@", if native { "GSR GSR(.GSRI(1'b1));" } else { "" })
            .replace(
                "reg[71:0]gold[0:191];",
                r#"wire cpr,cir,cbad,cadvance,cov,cfault,creq,cdrained;
wire[71:0]cop;wire[15:0]ct0,ct1,ct2,ct3;wire[31:0]caddr;
cache_closed closed(.clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(allow),.abort(abort),
 .in_valid(iv),.in_payload(ip),.proposal_ready(cpr),.in_ready(cir),.preflight_bad(cbad),.advance(cadvance),
 .out_ready(oready),.out_valid(cov),.out_payload(cop),.out_tex0(ct0),.out_tex1(ct1),.out_tex2(ct2),.out_tex3(ct3),
 .mem_req_valid(creq),.mem_req_addr(caddr),.mem_req_ready(mr),.mem_resp_valid(bv),.mem_resp_index(bi),
 .mem_resp_data(bd),.mem_resp_complete(complete),.mem_resp_ok(ok),.drained(cdrained),.fault(cfault));
task compare_public;begin
 if({pr,ir,bad,advance,ov,fault,req,drained,op,t0,t1,t2,t3,addr} !==
    {cpr,cir,cbad,cadvance,cov,cfault,creq,cdrained,cop,ct0,ct1,ct2,ct3,caddr})
  $fatal(1,"checked/closed public mismatch time%0t",$time);
end endtask
always @(posedge clk)begin if(!reset)compare_public;#1;if(!reset)compare_public;end
reg[71:0]gold[0:191];"#,
            );
        std::fs::write(dir.join("tb.v"), matched).unwrap();
        let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        command.current_dir(&dir).args([
            "-g2012",
            "-s",
            "tb",
            "-o",
            "test.vvp",
            "checked.v",
            "closed.v",
            "tb.v",
        ]);
        if native {
            command.arg(
                Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        bounded(command, &dir, "compile");
        let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
        command.current_dir(&dir).arg("test.vvp");
        bounded(command, &dir, "run");
        println!(
            "pool-closed native={native}: {}",
            std::fs::read_to_string(dir.join("run.out")).unwrap()
        );
    }
}
