//! Actual row Runtime RTL against independent immutable arithmetic calendars
//! and a counted ownership/handshake scoreboard, with no copied FE controller.
use gpu_v2::texture::{
    emu::{
        compact,
        context_store::{self, Raw},
        coordinate::{self, CoordinateEmu},
        coordinate_store::Owner,
        derivative::{self, DerivativeEmu},
        lod::LodEmu,
    },
    rtl::runtime_rows,
    sim::staged::bound::serial,
};
use std::fmt::Write;

const QUADS: usize = 48;
const MAX_EDGES: usize = 5000;

fn fixture() -> String {
    let (dc, lc, cc) = serial::structural_calendars().unwrap();
    let mut init = String::new();
    for q in 0..QUADS {
        let codes = [
            -131072, -65537, -32769, -1, 0, 1, 32767, 65535, 65536, 131071,
        ];
        let uv = std::array::from_fn(|i| match q % 3 {
            0 => -65537 + (i as i64 / 2) * (13 + q as i64 * 7) + (i as i64 % 2) * 311,
            1 => codes[q % codes.len()],
            _ => codes[(q * 3 + i * 7) % codes.len()],
        });
        let raw = Raw {
            bias: [-8192, -377, 0, 213, 8192][q % 5],
            force_coarsest: q % 7 == 0,
            header: derivative::Header {
                quad: (q % 16) as u8,
                mask: (q % 16) as u8,
                slot: (q % 11) as u8,
                max_n: (q % 11) as u8,
                has_mip: q % 5 != 0,
                filter: (q / 16) as u8,
            },
        };
        writeln!(
            init,
            "raws[{q}]=36'h{:x};masks[{q}]=4'h{:x};",
            raw.pack().unwrap(),
            raw.header.mask
        )
        .unwrap();
        for lane in 0..4 {
            let word =
                (uv[lane * 2] as u64 & 0x3ffff) | ((uv[lane * 2 + 1] as u64 & 0x3ffff) << 18);
            writeln!(init, "uvs[{}]=36'h{word:x};", q * 4 + lane).unwrap();
        }
        let mut d = DerivativeEmu::new(dc.clone()).unwrap();
        d.tick(
            true,
            Some(derivative::Input {
                uv,
                bias: raw.bias,
                force_coarsest: raw.force_coarsest,
                header: raw.header,
            }),
        )
        .unwrap();
        for _ in 0..2 {
            d.tick(true, None).unwrap();
        }
        let derived = d.output().unwrap().unwrap();
        writeln!(
            init,
            "derived[{q}]=53'h{:x};",
            context_store::pack_derived(derived.into()).unwrap()
        )
        .unwrap();
        let mut l = LodEmu::new(lc.clone().with_issue_offset(0).unwrap()).unwrap();
        l.tick(true, Some(derived.into())).unwrap();
        for _ in 0..3 {
            l.tick(true, None).unwrap();
        }
        let ready = l.output().unwrap().unwrap();
        writeln!(
            init,
            "ready_rows[{q}]=83'h{:x};",
            context_store::pack_ready(ready).unwrap()
        )
        .unwrap();
        for lane in 0..4 {
            let mut c = CoordinateEmu::new(cc.clone()).unwrap();
            c.tick(
                true,
                Some(coordinate::Input {
                    uv: [derived.uv[lane * 2], derived.uv[lane * 2 + 1]],
                    shift: ready.context.shift,
                    nearest: ready.context.nearest,
                    halve: ready.context.halve,
                    side: ready.context.side,
                }),
            )
            .unwrap();
            c.tick(true, None).unwrap();
            let result = c.output().unwrap().unwrap();
            let owner = Owner {
                parents: ready.context.parents,
                nearest: ready.context.nearest,
                levels: ready.context.levels,
                slot: ready.slot,
                key: ready.quad * 4 + lane as u8,
                last_fine: ready.context.last_fine,
            };
            writeln!(
                init,
                "owners[{}]=38'h{:x};results[{}]=36'h{:x};",
                q * 4 + lane,
                owner.pack().unwrap(),
                q * 4 + lane,
                compact::pack_result(result)
            )
            .unwrap();
        }
    }
    TESTBENCH
        .replace("// GOLDEN", &init)
        .replace("MAX_EDGES", &MAX_EDGES.to_string())
        .replace("QUADS", &QUADS.to_string())
}

const TESTBENCH: &str = r#"module tb;
reg clk=0,reset=1,ce=0,commit_allow=1,admission_allow=1,in_valid=0,out_ready=0;
reg[35:0]in_raw=0,in_uv=0;
wire in_ready,advance,preflight_bad,fault,raw_write,admitted,reclaim_fire,out_valid;
wire[1:0]in_row,c_lane;wire[2:0]in_slot,reclaim_slot,phase,d_slot,l_slot,c_slot,d_return_slot,l_return_slot;
wire[37:0]out_owner;wire[35:0]out_result;wire[72:0]context_state;
wire d_issue,l_issue,c_issue,c_empty,d_return,l_return,c_return,uv_write,uv_read;
wire[4:0]c_occupied,c_completed,uv_write_address,uv_read_address;
wire[2:0]store_reads,store_writes;
gpu_v2_texture_runtime_rows dut(.*);
reg[35:0]raws[0:QUADS-1],uvs[0:QUADS*4-1],results[0:QUADS*4-1];
reg[3:0]masks[0:QUADS-1];reg[52:0]derived[0:QUADS-1];reg[82:0]ready_rows[0:QUADS-1];reg[37:0]owners[0:QUADS*4-1];
integer owner_ctx[0:7],seen_lane[0:QUADS-1],public_left[0:15],expected[0:QUADS*4-1];
integer q=0,row=0,tail=0,head=0,wall,ctx,job,i,j,di=0,li=0,dr=0,lr=0,ci=0,cr=0,empties=0,raws_written=0,admissions=0;
integer paid_ce0=0,denied=0,full_cycles=0,context_full=0,same_d_reuse=0,peak_context=0,peak_c=0,held_output=0;
reg previous_uv_read=0;reg[72:0]before_context;reg[2:0]before_phase;reg[4:0]before_count,before_ready;
reg[37:0]before_owner;reg[35:0]before_result;reg before_valid;
task tick_edge;begin clk=1;#2;clk=0;#2;end endtask
initial begin
// GOLDEN
for(i=0;i<8;i=i+1)owner_ctx[i]=-1;
for(i=0;i<16;i=i+1)public_left[i]=0;
for(i=0;i<QUADS;i=i+1)seen_lane[i]=0;
#2;tick_edge;reset=0;
for(wall=0;wall<MAX_EDGES;wall=wall+1)begin
 ce=(wall%7!=3)&&!previous_uv_read;
 commit_allow=(wall%31!=17)&&(wall%31!=18);
 admission_allow=wall%13!=4;
 out_ready=wall>240&&wall%9!=2&&wall%9!=3;
 in_valid=q<QUADS&&(row!=0||public_left[q%16]==0);
 if(q<QUADS)begin in_raw=raws[q];in_uv=uvs[q*4+row];end
 #2;
 if(fault||preflight_bad)$fatal(1,"healthy fault edge %0d phase %0d fe %b store %b queue %b",wall,phase,dut.fe_bad,dut.store_bad,dut.queue_bad);
 if(advance!==(ce&&commit_allow))$fatal(1,"advance");
 if(in_row!==row[1:0] && in_ready)$fatal(1,"row cursor %0d %0d",in_row,row);
 if(c_occupied>6||c_completed>c_occupied)$fatal(1,"C credit");
 if(c_occupied>peak_c)peak_c=c_occupied;
 if(context_state[9:6]>peak_context)peak_context=context_state[9:6];
 if(context_state[9:6]==8)context_full=context_full+1;
 if(c_occupied==6)full_cycles=full_cycles+1;
 if(previous_uv_read&&!ce)paid_ce0=paid_ce0+1;
 if(ce&&!commit_allow)denied=denied+1;
 if(out_valid)begin
  if(head>=tail)$fatal(1,"unreserved output");
  if(out_owner!==owners[expected[head]]||out_result!==results[expected[head]])$fatal(1,"numeric output edge %0d key %0d owner %h/%h result %h/%h",wall,expected[head],out_owner,owners[expected[head]],out_result,results[expected[head]]);
  if(!advance||!out_ready)held_output=held_output+1;
 end
 // Scoreboard observes old data before releasing any same-edge ownership.
 if(advance&&out_valid&&out_ready)begin
  job=expected[head]/4;public_left[job%16]=public_left[job%16]-1;head=head+1;
 end
 if(advance&&d_return)begin
  job=owner_ctx[d_return_slot];
  if(phase!=3||job<0||dut.held_derived!==derived[job])$fatal(1,"D numeric/owner %0d",wall);
  dr=dr+1;
 end
 if(advance&&l_return)begin
  job=owner_ctx[l_return_slot];
  if(phase!=0||job<0||dut.held_derived!==derived[job]||dut.ready_data!==ready_rows[job])$fatal(1,"LOD numeric/loan %0d",wall);
  lr=lr+1;if(d_issue)same_d_reuse=same_d_reuse+1;
 end
 if(d_issue)begin
  if(phase!=0||owner_ctx[d_slot]<0)$fatal(1,"D issue owner");di=di+1;
 end
 if(l_issue)begin
  job=owner_ctx[l_slot];if(phase!=4||dut.derived_q[52:0]!==derived[job])$fatal(1,"LOD issue loan");li=li+1;
 end
 if(c_issue)begin
  job=owner_ctx[c_slot];
  if(phase[0]||job<0||!masks[job][c_lane]||seen_lane[job][c_lane])$fatal(1,"C duplicate/owner");
  if(dut.c_owner!==owners[job*4+c_lane])$fatal(1,"C owner capture");
  seen_lane[job]=seen_lane[job]|(1<<c_lane);expected[tail]=job*4+c_lane;tail=tail+1;ci=ci+1;
 end
 if(advance&&c_return)cr=cr+1;
 if(c_empty)begin
  job=owner_ctx[c_slot];if(job<0||masks[job]!=0)$fatal(1,"empty owner");public_left[job%16]=0;empties=empties+1;
 end
 if(reclaim_fire)begin
  job=owner_ctx[reclaim_slot];if(job<0||seen_lane[job]!=masks[job])$fatal(1,"early reclaim coverage");owner_ctx[reclaim_slot]=-1;
 end
 if(advance&&in_valid&&in_ready)begin
  if(row==0)begin
   if(!raw_write||store_writes[0]!=1||owner_ctx[in_slot]>=0)$fatal(1,"row0 canonical Raw");
   owner_ctx[in_slot]=q;raws_written=raws_written+1;
   public_left[q%16]=masks[q][0]+masks[q][1]+masks[q][2]+masks[q][3];
   if(masks[q]==0)public_left[q%16]=1;
  end else if(raw_write||store_writes[0])$fatal(1,"Raw shadow");
  if(row==3)begin if(!admitted)$fatal(1,"row3 publication");row=0;q=q+1;admissions=admissions+1;end
  else begin if(admitted)$fatal(1,"early publication");row=row+1;end
 end else if(raw_write||admitted)$fatal(1,"unaccepted row write");
 if(!advance&&(uv_write||uv_read||store_reads||store_writes||d_issue||l_issue||c_issue||reclaim_fire))$fatal(1,"denied commit");
 previous_uv_read=uv_read;
 before_context=context_state;before_phase=phase;before_count=c_occupied;before_ready=c_completed;
 before_valid=out_valid;before_owner=out_owner;before_result=out_result;
 tick_edge;
 if(!advance)begin
  if(context_state!==before_context||phase!==before_phase||c_occupied!==before_count||c_completed!==before_ready)$fatal(1,"held controls");
  if(before_valid&&(out_owner!==before_owner||out_result!==before_result))$fatal(1,"held result");
 end
 if(q==QUADS&&head==tail&&context_state[9:6]==0&&c_occupied==0&&dr==QUADS&&lr==QUADS)begin
  if(di!=QUADS||li!=QUADS||raws_written!=QUADS||admissions!=QUADS||ci!=cr||empties!=3||paid_ce0==0||denied==0||full_cycles==0||context_full==0||same_d_reuse==0||held_output==0)$fatal(1,"coverage");
  $display("PASS rows wall=%0d quads=%0d C=%0d CE0paid=%0d denied=%0d Cfull=%0d Contextfull=%0d sameDreuse=%0d held=%0d peakContext=%0d peakC=%0d",wall+1,q,ci,paid_ce0,denied,full_cycles,context_full,same_d_reuse,held_output,peak_context,peak_c);
  // A bad row0 proposal suppresses every participant, then holds a paid head.
  reset=1;ce=0;in_valid=0;#2;tick_edge;reset=0;commit_allow=1;admission_allow=1;out_ready=0;
  row=0;
  for(j=0;j<200;j=j+1)begin
   ce=1;in_valid=row<4;in_raw=raws[15];if(row<4)in_uv=uvs[60+row];#2;
   if(advance&&in_valid&&in_ready)row=row+1;
   if(out_valid&&in_ready&&in_row==0)begin
    before_context=context_state;before_phase=phase;before_count=c_occupied;before_ready=c_completed;before_owner=out_owner;before_result=out_result;
    in_valid=1;in_raw=raws[15];in_raw[32:29]=11;#2;
    if(!preflight_bad||advance||raw_write||uv_write||store_writes||d_issue||l_issue||c_issue)$fatal(1,"bad row shared preflight");
    tick_edge;if(!fault)$fatal(1,"terminal shared fault");
    for(i=0;i<8;i=i+1)begin out_ready=1;#2;tick_edge;
     if(advance||context_state!==before_context||phase!==before_phase||c_occupied!==before_count||c_completed!==before_ready||!out_valid||out_owner!==before_owner||out_result!==before_result)$fatal(1,"terminal head hold");
    end
    $display("PASS fault held paid result 8 wall edges");$finish;
   end
   tick_edge;
  end
  $fatal(1,"fault profile watchdog");
 end
end
$fatal(1,"wall watchdog");
end
initial begin #100000;$fatal(1,"time watchdog");end
endmodule
"#;

#[test]
fn independent_numeric_fixture_is_bounded() {
    assert!(fixture().contains("owners[191]"));
    for native in [false, true] {
        runtime_rows::verilog(native).unwrap();
    }
}

#[test]
#[ignore = "requires Icarus and actual Gowin SRAM/BSRAM primitives"]
fn actual_rows_context_d_lod_c_qualify() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/texture-runtime-rows-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), fixture()).unwrap();
    for native in [false, true] {
        let name = format!("native-{native}.v");
        std::fs::write(dir.join(&name), runtime_rows::verilog(native).unwrap()).unwrap();
        let mut command = std::process::Command::new(
            std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
        );
        command
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
        if native {
            command.arg("-DGPU_V2_GOWIN_DSP").arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        let compiled = command.args([&name, "tb.v"]).output().unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let result =
            std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
                .current_dir(&dir)
                .arg("sim.vvp")
                .output()
                .unwrap();
        assert!(
            result.status.success(),
            "native={native}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        println!(
            "native={native}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}
