//! Selected arithmetic/transport closure, with generic Coefficient numerical
//! gold and independent tile grouping rather than a copied backend controller.
use gpu_v2::texture::{
    emu::{
        coefficient::{self, CoefficientEmu},
        compact,
        coordinate_store::Owner,
    },
    ports::{Group4, TileKey},
    rtl::runtime_backend,
};
use std::{collections::BTreeSet, fmt::Write};
const INPUTS: usize = 160;
const MAX_EDGES: usize = 20000;

pub(crate) fn fixture() -> String {
    let mut init = String::new();
    let mut members = 0;
    let mut groups = 0;
    let mut emits = BTreeSet::new();
    for q in 0..INPUTS {
        let n = if q < 7 { 5 } else { (q % 11) as u8 };
        let mask = compact::side_mask(n);
        let base = if q < 7 {
            if q == 4 {
                [7, 6]
            } else {
                [7, 7]
            }
        } else {
            [
                [7, 7],
                [15, 8],
                [8, 15],
                [0, 0],
                [1023, 1023],
                [6, 6],
                [31, 31],
                [3, 7],
            ][q % 8]
        }
        .map(|v| v & mask);
        let fractions: [u8; 2] = if q < 7 {
            [
                [0, 0],
                [128, 0],
                [0, 128],
                [1, 1],
                [255, 255],
                [255, 255],
                [128, 128],
            ][q]
        } else {
            [
                [0, 0],
                [255, 255],
                [1, 255],
                [255, 1],
                [128, 128],
                [0, 255],
                [255, 0],
                [37, 211],
            ][q % 8]
        };
        let result = u64::from(fractions[0])
            | (u64::from(fractions[1]) << 8)
            | (u64::from(base[0]) << 16)
            | (u64::from(base[1]) << 26);
        let nearest = q % 7 == 0;
        let parents = if q < 7 {
            if q == 4 || q == 5 {
                [255, 256]
            } else {
                [511, 0]
            }
        } else if q == 10 {
            [1, 510]
        } else if nearest {
            [511, 0]
        } else {
            [
                [511, 0],
                [510, 1],
                [1, 510],
                [0, 511],
                [255, 256],
                [256, 255],
                [509, 2],
            ][q % 7]
        };
        let owner = Owner {
            parents,
            nearest,
            levels: [n, n.saturating_sub(1)],
            slot: (q % 16) as u8,
            key: (q % 64) as u8,
            last_fine: parents[1] == 0,
        };
        let input = owner.finish(compact::expand_result(result, n));
        let mut core = CoefficientEmu::new(64).unwrap();
        for edge in 0..6 {
            core.tick(coefficient::Tick {
                ce: true,
                input: (edge == 0).then_some(input),
                output_ready: false,
                work_available: 16,
            })
            .unwrap();
        }
        let out = core.output().unwrap();
        let mut weight_word = 0u128;
        for (p, &parent) in parents.iter().enumerate() {
            assert_eq!(out.weights[p].iter().sum::<u16>(), parent);
            assert_eq!(out.weights[p][0] != 0, parent != 0);
            for t in 0..4 {
                weight_word |= u128::from(out.weights[p][t]) << (p * 36 + t * 9);
            }
        }
        let meta = compact::Metadata::capture(input).unwrap().0;
        writeln!(init,"owners[{q}]=38'h{:x};results[{q}]=36'h{result:x};weights[{q}]=72'h{weight_word:x};metas[{q}]=36'h{meta:x};costs[{q}]={};",owner.pack().unwrap(),parents.iter().filter(|&&v|v!=0).count()).unwrap();
        for (plane, &parent) in parents.iter().enumerate() {
            if parent == 0 {
                continue;
            }
            let c = out.metadata.coordinates[plane];
            let level = out.metadata.levels[plane];
            let taps = [[c[0], c[2]], [c[1], c[2]], [c[0], c[3]], [c[1], c[3]]];
            let mut unique = Vec::<([u16; 2], usize)>::new();
            for (tap, xy) in taps.iter().enumerate() {
                let tile = xy.map(|v| v / 8);
                if out.weights[plane][tap] != 0 && !unique.iter().any(|(key, _)| *key == tile) {
                    unique.push((tile, tap));
                }
            }
            let emit = unique.iter().fold(0u8, |m, (_, tap)| m | (1 << tap));
            emits.insert(emit);
            let mut member = 0u128;
            for (t, w) in out.weights[plane].iter().enumerate() {
                member |= u128::from(*w) << (t * 9);
            }
            member |= u128::from(emit) << 36
                | u128::from(c[0] >> 3) << 40
                | u128::from(c[2] >> 3) << 47
                | u128::from(c[0] & 7) << 54
                | u128::from(c[2] & 7) << 57
                | u128::from(owner.slot) << 60
                | u128::from(level) << 64
                | u128::from(owner.key / 4) << 68
                | u128::from(owner.key % 4) << 72
                | u128::from(plane == 0) << 74
                | u128::from(plane == 1 || owner.last_fine) << 75;
            writeln!(init, "member_gold[{members}]=76'h{member:x};").unwrap();
            members += 1;
            for (ordinal, (tile, tap)) in unique.iter().enumerate() {
                let row = Group4 {
                    key: TileKey {
                        slot: owner.slot,
                        n: level,
                        x: tile[0] as u8,
                        y: tile[1] as u8,
                    },
                    top_left_local: [(c[0] & 7) as u8, (c[2] & 7) as u8],
                    coefficients: std::array::from_fn(|t| {
                        if taps[t].map(|v| v / 8) == *tile {
                            u32::from(out.weights[plane][t])
                        } else {
                            0
                        }
                    }),
                    first: plane == 0 && *tap == 0,
                    last: (plane == 1 || owner.last_fine) && ordinal + 1 == unique.len(),
                    quad_id: owner.key / 4,
                    lane: owner.key % 4,
                };
                writeln!(
                    init,
                    "group_gold[{groups}]=72'h{:x};",
                    row.pack72().unwrap()
                )
                .unwrap();
                groups += 1;
            }
        }
    }
    // A positive bottom-right conserves a positive bottom-left, excluding11.
    // Every other reachable adjacent-tile emit mask is present in this sweep.
    assert_eq!(emits, BTreeSet::from([1, 3, 5, 7, 9, 13, 15]));
    println!(
        "gold inputs={INPUTS} members={members} groups={groups} reachable emit masks={emits:?}"
    );
    TESTBENCH
        .replace("// GOLDEN", &init)
        .replace("INPUTS", &INPUTS.to_string())
        .replace("MAX_EDGES", &MAX_EDGES.to_string())
        .replace("MEMBERS", &members.to_string())
        .replace("GROUPS", &groups.to_string())
}

const TESTBENCH: &str = r#"module tb;
reg clk=0,reset=1,ce=0,commit_allow=1,abort=0,in_valid=0,packet_enable=1,out_ready=0;
reg[37:0]in_owner=0;reg[35:0]in_result=0;
wire in_ready,in_accept,advance,preflight_bad,fault,out_valid;
wire[71:0]out_payload;wire[4:0]work_reserved,work_count,producer_count,unwritten_count;
wire[5:0]group_count,row_count,pool_strobes;
wire coefficient_valid,coefficient_consume,member_issue,member_return,packet_capture,work_ack;
wire[1:0]reserved_cost;wire[71:0]coefficient_weights;wire[35:0]coefficient_meta;wire[75:0]member_payload;
gpu_v2_texture_runtime_backend dut(.*);
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
reg[37:0]owners[0:INPUTS-1];reg[35:0]results[0:INPUTS-1],metas[0:INPUTS-1];reg[71:0]weights[0:INPUTS-1];integer costs[0:INPUTS-1];
reg[75:0]member_gold[0:MEMBERS-1];reg[71:0]group_gold[0:GROUPS-1];
integer source=0,coef=0,mi=0,mr=0,pc=0,pop=0,acked=0,reservations=0,wall,i,j;
integer work_full=0,p_full=0,g_full=0,reserved_full=0,paid_ce0=0,denied=0,held=0,pending_work=0,cohorts3=0,zero_fine=0,zero_coarse=0;
reg previous_read=0,previous_meta=0;
reg before_advance;reg[4:0]before_reserved,before_work,before_producer,before_unwritten;reg[5:0]before_group,before_rows;
reg[71:0]before_payload;reg before_valid;
task tick_edge;begin clk=1;#2;clk=0;#2;end endtask
initial begin
// GOLDEN
#2;tick_edge;reset=0;
for(wall=0;wall<MAX_EDGES;wall=wall+1)begin
 ce=wall%7!=3&&!previous_read&&!previous_meta;
 commit_allow=wall%31!=17&&wall%31!=18;
 packet_enable=wall>=180&&!(350<=wall&&wall<470);
 out_ready=wall>850&&wall%9!=2&&wall%9!=3;
 in_valid=source<INPUTS;
 if(in_valid)begin in_owner=owners[source];in_result=results[source];end
 #2;
 if(fault||preflight_bad)$fatal(1,"healthy backend fault wall%0d coef%b packet%b credit%b",wall,dut.coefficient_bad,dut.packet_bad,dut.credit_bad);
 if(advance!==(ce&&commit_allow))$fatal(1,"advance");
 if(work_reserved!==reservations-acked||work_count>work_reserved||work_reserved>16||producer_count>16||group_count>32||unwritten_count>producer_count||row_count>48)$fatal(1,"bounded credits");
 if(work_count==16)work_full=work_full+1;if(work_reserved==16)reserved_full=reserved_full+1;
 if(producer_count==16)p_full=p_full+1;if(group_count==32)g_full=g_full+1;
 if(work_reserved>work_count)pending_work=pending_work+1;
 if(dut.coefficient.cohorts==3)cohorts3=cohorts3+1;
 if(!ce&&(previous_read||previous_meta))paid_ce0=paid_ce0+1;
 if(ce&&!commit_allow)denied=denied+1;
 if(coefficient_valid)begin
  if(coef>=source||coefficient_weights!==weights[coef]||coefficient_meta!==metas[coef])$fatal(1,"independent Coefficient result %0d",wall);
  if(coefficient_weights[35:0]==0)zero_fine=zero_fine+1;
  if(coefficient_weights[71:36]==0)zero_coarse=zero_coarse+1;
 end
 if(member_return)begin if(mr>=mi||member_payload!==member_gold[mr])$fatal(1,"independent Member result %0d row%0d got%h expect%h",wall,mr,member_payload,member_gold[mr]);end
 if(out_valid)begin
  if(pop>=pc||out_payload!==group_gold[pop])$fatal(1,"independent Group72 %0d row%0d got%h expect%h",wall,pop,out_payload,group_gold[pop]);
  if(!advance||!out_ready)held=held+1;
 end
 if(in_accept)begin if(!advance||reserved_cost!=costs[source])$fatal(1,"C6 accepted credit");reservations=reservations+costs[source];source=source+1;end
 if(member_issue)mi=mi+1;if(advance&&member_return)mr=mr+1;
 if(coefficient_consume)coef=coef+1;
 if(packet_capture)begin
  if(dut.packets.member[39:36]>>({1'b0,dut.packets.tap}+3'd1)==0 && !work_ack)$fatal(1,"last Work ACK");
  pc=pc+1;
 end
 if(work_ack)acked=acked+1;
 if(advance&&out_valid&&out_ready)pop=pop+1;
 if(!advance&&(in_accept||member_issue||coefficient_consume||packet_capture||work_ack||pool_strobes))$fatal(1,"denied partial commit");
 previous_read=pool_strobes[3];previous_meta=dut.coefficient.meta_re;
 before_advance=advance;before_reserved=work_reserved;before_work=work_count;before_producer=producer_count;before_unwritten=unwritten_count;before_group=group_count;before_rows=row_count;before_payload=out_payload;before_valid=out_valid;
 tick_edge;
 if(!before_advance)begin
  if(work_reserved!==before_reserved||work_count!==before_work||producer_count!==before_producer||unwritten_count!==before_unwritten||group_count!==before_group||row_count!==before_rows)$fatal(1,"held credit controls");
  if(before_valid&&out_payload!==before_payload)$fatal(1,"held Group72");
 end
 if(source==INPUTS&&coef==source&&mi==MEMBERS&&mr==mi&&acked==mi&&pc==GROUPS&&pop==pc&&work_reserved==0&&work_count==0&&producer_count==0&&group_count==0&&row_count==0)begin
  if(work_full==0||p_full==0||g_full==0||reserved_full==0||paid_ce0==0||denied==0||held==0||pending_work==0||cohorts3==0||zero_fine==0||zero_coarse==0)$fatal(1,"saturation coverage W%0d P%0d G%0d cohorts%0d",work_full,p_full,g_full,cohorts3);
  $display("PASS backend wall=%0d inputs=%0d members=%0d groups=%0d Work16=%0d P16=%0d G32=%0d reserved16=%0d paidCE0=%0d denied=%0d held=%0d",wall+1,source,mi,pop,work_full,p_full,g_full,reserved_full,paid_ce0,denied,held);
  // Reset during a live arithmetic/transport cohort discards ownership once.
  reset=1;ce=0;in_valid=0;#2;tick_edge;reset=0;commit_allow=1;packet_enable=1;out_ready=0;
  source=0;
  for(j=0;j<15;j=j+1)begin ce=1;in_valid=source<4;in_owner=owners[source];in_result=results[source];#2;if(in_accept)source=source+1;tick_edge;end
  reset=1;#2;tick_edge;reset=0;in_valid=0;
  for(j=0;j<12;j=j+1)begin #2;tick_edge;if(fault||work_reserved||work_count||producer_count||group_count||row_count||out_valid||coefficient_valid||member_return)$fatal(1,"reset ownership drain");end
  // Bad scalar input while a paid Group head exists must deny all writers.
  source=0;
  for(j=0;j<160;j=j+1)begin
   in_valid=source<1;in_owner=owners[0];in_result=results[0];#2;if(in_accept)source=source+1;
   if(out_valid&&dut.coefficient.proposal_ready)begin
    before_reserved=work_reserved;before_work=work_count;before_producer=producer_count;before_unwritten=unwritten_count;before_group=group_count;before_rows=row_count;before_payload=out_payload;
    in_valid=1;in_owner=owners[0];in_owner[17:0]={9'd511,9'd511};#2;
    if(!preflight_bad||advance||in_accept||member_issue||packet_capture||work_ack||pool_strobes)$fatal(1,"invalid scalar preflight");
    tick_edge;if(!fault)$fatal(1,"terminal fault");
    for(i=0;i<8;i=i+1)begin out_ready=1;#2;tick_edge;
     if(advance||work_reserved!==before_reserved||work_count!==before_work||producer_count!==before_producer||unwritten_count!==before_unwritten||group_count!==before_group||row_count!==before_rows||out_payload!==before_payload)$fatal(1,"fault held paid head");
    end
    $display("PASS backend live reset and fault paid-head hold8edges");$finish;
   end
   tick_edge;
  end
  $fatal(1,"fault watchdog");
 end
end
$fatal(1,"wall watchdog");end
initial begin #400000;$fatal(1,"time watchdog");end
endmodule
"#;

#[test]
#[ignore = "requires selected actual arithmetic and native Gowin primitives"]
fn actual_coefficient_member_work_packet_pool_qualify() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/runtime-backend-closed-maskbits-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), fixture()).unwrap();
    for native in [false, true] {
        let source = runtime_backend::verilog(native).unwrap();
        let source = if native {
            "`define GPU_V2_GOWIN_DSP\n".to_owned() + &source
        } else {
            source
        };
        let name = format!("native-{native}.v");
        std::fs::write(dir.join(&name), source).unwrap();
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
        let compiled = command.args([&name, "tb.v"]).output().unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let ran = std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
            .current_dir(&dir)
            .arg("sim.vvp")
            .output()
            .unwrap();
        assert!(
            ran.status.success(),
            "native={native}: {} {}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        println!("native={native}: {}", String::from_utf8_lossy(&ran.stdout));
    }
}
