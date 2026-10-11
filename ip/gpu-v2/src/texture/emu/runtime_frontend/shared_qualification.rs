//! Bounded raw-proposal/common-commit replay; no Runtime calendar or UV shadow.
use super::*;
use crate::texture::{
    emu::{context_store as bank, derivative::Header, lod},
    rtl,
};
use std::{
    fmt::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn raw(i: usize, mask: u8) -> bank::Raw {
    bank::Raw {
        bias: [-8192, -1, 0, 8192][i % 4],
        force_coarsest: i & 1 != 0,
        header: Header {
            quad: (i % 16) as u8,
            mask,
            slot: (i % 16) as u8,
            max_n: 5,
            has_mip: true,
            filter: 1,
        },
    }
}
// Independent concatenation also permits malformed values for RTL preflight.
fn raw_bits(v: bank::Raw) -> u64 {
    let h = v.header;
    (v.bias as u16 as u64)
        | ((v.force_coarsest as u64) << 16)
        | ((h.quad as u64) << 17)
        | ((h.mask as u64) << 21)
        | ((h.slot as u64) << 25)
        | ((h.max_n as u64) << 29)
        | ((h.has_mip as u64) << 33)
        | ((h.filter as u64) << 34)
}
fn packed(c: &Controller) -> u128 {
    let mut word = 0;
    let mut shift = 0;
    let mut push = |v: usize, w: usize| {
        word |= (v as u128) << shift;
        shift += w;
    };
    for (v, w) in [
        (c.allocate, 3),
        (c.reclaim, 3),
        (c.occupied, 4),
        (c.d_head, 3),
        (c.raw_count, 4),
        (usize::from(c.d_owner.is_some()), 1),
        (c.d_owner.unwrap_or(0), 3),
        (c.l_head, 3),
        (c.derived_count, 4),
        (usize::from(c.l_owner.is_some()), 1),
        (c.l_owner.unwrap_or(0), 3),
        (c.c_head, 3),
        (c.lod_count, 4),
    ] {
        push(v, w);
    }
    push(
        c.slots
            .iter()
            .enumerate()
            .fold(0, |m, (i, v)| m | (usize::from(v.is_some()) << i)),
        8,
    );
    push(
        c.reclaimable
            .iter()
            .enumerate()
            .fold(0, |m, (i, v)| m | (usize::from(*v) << i)),
        8,
    );
    for i in 0..8 {
        push(
            c.slots
                .get(i)
                .and_then(|x| *x)
                .map_or(0, |x| usize::from(x.metadata.diagnostic_stage())),
            2,
        );
    }
    push(c.ordinal, 2);
    assert_eq!(shift, 73);
    word
}

#[test]
fn reservation_and_denial_are_stateless() {
    let mut c = Controller::new(1, true).unwrap();
    let p = Ports {
        ce: true,
        ..Ports::default()
    };
    let proposal = c.preflight(p, true).unwrap();
    assert_eq!(c.prepare_shared(proposal, true).unwrap().0, c);
    let mut s = bank::runtime::Store::new(1).unwrap();
    let rows = s.rows();
    let t = bank::Tick {
        ce: true,
        admission: Some((0, raw(0, 15))),
        ..Default::default()
    };
    s.commit(s.prepare_shared(t, None, false).unwrap());
    assert_eq!(s.rows(), rows);
    s.commit(s.prepare_shared(t, None, true).unwrap());
    assert_eq!(c.occupied, 0);
    assert_eq!(c.raw_count, 0);
    let publication = c
        .preflight(
            Ports {
                ce: true,
                admit: true,
                ..Default::default()
            },
            false,
        )
        .unwrap();
    assert_eq!(c.prepare_shared(publication, false).unwrap().0, c);
    c = c.prepare_shared(publication, true).unwrap().0;
    assert_eq!((c.occupied, c.raw_count), (1, 1));
    assert!(c.preflight(p, true).is_err());
    let mut malformed = t;
    malformed.admission.as_mut().unwrap().1.bias = 8193;
    assert!(s.prepare_shared(malformed, None, false).is_err());
    let mut collision = t;
    collision.raw_issue = Some(0);
    assert!(s.prepare_shared(collision, None, false).is_err());
    let g = bank::Store::new(1).unwrap();
    assert_eq!(g.prepare_shared(t, false).unwrap(), bank::Edge::default());
    assert!(g.prepare_shared(malformed, false).is_err());
}

#[test]
fn count_precompare_matches_every_encoded_counter_combination() {
    let mut cases = 0;
    for raw in 0..16u8 {
        for derived in 0..16u8 {
            for lod in 0..16u8 {
                for dv in 0..2u8 {
                    for lv in 0..2u8 {
                        let tracked = raw + derived + lod + dv + lv;
                        assert!(tracked <= 47);
                        for occupied in 0..16u8 {
                            for rf in 0..2u8 {
                                for done in 0..2u8 {
                                    // Both operands have the original six-bit
                                    // HDL width; encoded counters never overflow.
                                    let original = ((tracked + rf) & 63) > occupied + done;
                                    let parallel = match (rf, done) {
                                        (0, 0) | (1, 1) => tracked > occupied,
                                        (0, 1) => tracked > occupied + 1,
                                        (1, 0) => tracked >= occupied,
                                        _ => unreachable!(),
                                    };
                                    assert_eq!(original, parallel,
                                        "counts={raw}/{derived}/{lod}+{dv}+{lv} occupied={occupied} rf={rf} done={done}");
                                    cases += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 1_048_576);
}

#[test]
fn retire_predecode_matches_all_binary_masks_and_selected_row_flags() {
    let mut retire_cases = 0;
    for capacity in 1..=8 {
        for head in 0..8 {
            let onehot = 1u8 << head;
            for late in 0..=255u8 {
                for invalid in 0..=255u8 {
                    for early in [false, true] {
                        let retired = late | if early { onehot } else { 0 };
                        let old = (u16::from(retired) >> capacity) != 0
                            || (0..8).any(|row| retired >> row & 1 != 0 && invalid >> row & 1 != 0);
                        let split = (u16::from(late) >> capacity) != 0
                            || (late & invalid) != 0
                            || early
                                && ((u16::from(onehot) >> capacity) != 0
                                    || (onehot & invalid) != 0);
                        assert_eq!(old,split,"capacity={capacity} head={head} late={late} invalid={invalid} early={early}");
                        retire_cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(retire_cases, 8_388_608);
    let mut reclaim_cases = 0;
    for head in 0..8 {
        for reclaim in 0..8 {
            for late_bit in [false, true] {
                for reclaimed_bit in [false, true] {
                    for early in [false, true] {
                        for running in [false, true] {
                            for occupied in [false, true] {
                                let late = if late_bit { 1u8 << reclaim } else { 0 };
                                let reclaimed = if reclaimed_bit { 1u8 << reclaim } else { 0 };
                                let retired = late | if early { 1u8 << head } else { 0 };
                                let old = running
                                    && occupied
                                    && (reclaimed | retired) >> reclaim & 1 != 0;
                                let split = running
                                    && occupied
                                    && (reclaimed_bit || late_bit || early && head == reclaim);
                                assert_eq!(old, split);
                                reclaim_cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(reclaim_cases, 2048);
}

#[derive(Default)]
struct Input {
    p: Ports,
    reserve: bool,
    row: Option<bank::Raw>,
    phase: u8,
    loan: Option<(usize, lod::Input)>,
    d: Option<lod::Input>,
    l: Option<lod::Output>,
    deny: bool,
}
struct Replay {
    tb: String,
    c: Controller,
    s: bank::runtime::Store,
    fault: bool,
    fe_fault: bool,
    known: [Option<u64>; 8],
    edges: usize,
    capacity: usize,
}
impl Replay {
    fn new(capacity: usize) -> Self {
        let tb = format!(
            r#"`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=0,external_allow=1,reserve_check=0;
reg admit=0,dz=0,lz=0,dr=0,lr=0,ud=0,odd=0,empty=0,uc=0,cz=0,credit=0;
reg[2:0]uds=0,ucs=0,phase=0;reg[1:0]ucl=0;reg[3:0]mask=0;reg[7:0]retire=0;
reg row_write=0;reg[35:0]raw_data=0;reg[52:0]derived_data=0,held_derived=0;
reg[82:0]ready_data=0;reg[3:0]loan_owner=0;
reg terminal=0;wire proposal_ce=ce&&!terminal;
wire fb,sb,advance,sa,reserve_ready,ready,di,li,cr,request,take,zero,rf,fault;
wire allow=external_allow&&!fb&&!sb;
wire[6:0]proposal_cmds;wire[2:0]proposal_reads,proposal_writes,reads,writes;
wire[2:0]a,ds,ls,cs,dro,lro,rs,ordinal;wire[1:0]lane;wire[72:0]state;
wire[3:0]raw_ra,derived_ra,ready_ra;wire[35:0]raw_q;wire[55:0]derived_q;wire[83:0]ready_q;
gpu_v2_texture_runtime_frontend_shared #(.CONTEXTS({capacity}),.EARLY(1))fe(
 .clk(clk),.reset(reset),.proposal_ce(proposal_ce),.commit_allow(allow),.reserve_check(reserve_check),
 .admit(admit),.d_phase_zero(dz),.l_phase_zero(lz),.d_return(dr),.l_return(lr),
 .uv_d_valid(ud),.uv_d_slot(uds),.c_mask(mask),.uv_phase_odd(odd),.uv_c_empty(empty),
 .uv_c_valid(uc),.uv_c_slot(ucs),.uv_c_lane(ucl),.c_phase_zero(cz),.c_credit(credit),.late_retire(retire),
 .preflight_bad(fb),.advance(advance),.reserve_ready(reserve_ready),.proposal_cmds(proposal_cmds),
 .input_ready(ready),.raw_issue(di),.derived_issue(li),.ready_read(cr),.uv_request(request),
 .c_take(take),.c_empty(zero),.admit_slot(a),.d_slot(ds),.l_slot(ls),.c_slot(cs),
 .d_return_slot(dro),.l_return_slot(lro),.c_lane(lane),.c_ordinal(ordinal),
 .reclaim_fire(rf),.reclaim_slot(rs),.fault(fault),.state(state));
gpu_v2_context_store_shared banks(.clk(clk),.reset(reset),.proposal_ce(proposal_ce),
 .commit_allow(allow),.fault(terminal),.control_bad(1'b0),.phase(phase),.admit(row_write),
 .d_issue(proposal_cmds[0]),.l_issue(proposal_cmds[1]),.c_read(proposal_cmds[2]),
 .d_return(dr),.l_return(lr),.allocate({{1'b0,a}}),.d_head({{1'b0,ds}}),.l_head({{1'b0,ls}}),
 .c_head({{1'b0,cs}}),.d_owner({{1'b0,dro}}),.l_owner({{1'b0,lro}}),
 .raw_data(raw_data),.derived_data(derived_data),.held_derived(held_derived),.loan_owner(loan_owner),
 .ready_data(ready_data),.bad(sb),.advance(sa),.preflight_bad(),.proposal_reads(proposal_reads),
 .proposal_writes(proposal_writes),.reads(reads),.writes(writes),.raw_ra(raw_ra),
 .derived_ra(derived_ra),.ready_ra(ready_ra),.raw_q(raw_q),.derived_q(derived_q),.ready_q(ready_q));
always@(posedge clk)if(reset)terminal<=0;else if(proposal_ce&&(fb||sb))terminal<=1;
`ifdef SHARED_NATIVE
GSR GSR(.GSRI(1'b1));
`endif
initial begin #2000000;$fatal(1,"watchdog");end
initial begin #10;clk=1;#10;clk=0;reset=0;#10;
"#
        );
        Self {
            tb,
            c: Controller::new(capacity, true).unwrap(),
            s: bank::runtime::Store::new(capacity).unwrap(),
            fault: false,
            fe_fault: false,
            known: [None; 8],
            edges: 0,
            capacity,
        }
    }
    fn reset(&mut self) {
        self.tb
            .push_str("reset=1;ce=0;#10;clk=1;#10;clk=0;reset=0;#10;\n");
        self.c = Controller::new(self.capacity, true).unwrap();
        self.fault = false;
        self.fe_fault = false;
        // SRAM retains its physical contents across control reset.
    }
    fn edge(&mut self, mut input: Input) {
        let offered = input.p;
        input.p.ce &= !self.fault;
        let p = input.p;
        let reads = self.c.reads(p.ce, p.d_zero, p.l_zero, p.uv_d);
        let cr = reads.ready.is_some();
        let ct = cr && p.c_mask != 0 && p.c_zero && p.c_credit && p.uv_c.is_some();
        let empty = cr && p.c_mask == 0;
        let done = empty || (ct && self.c.ordinal + 1 == p.c_mask.count_ones() as usize);
        let retired = if p.ce {
            p.late_retire | if done { 1 << self.c.c_head } else { 0 }
        } else {
            0
        };
        let rf = p.ce
            && self.c.occupied != 0
            && (self.c.reclaimable[self.c.reclaim] || retired >> self.c.reclaim & 1 != 0);
        let commands = (usize::from(rf) << 6)
            | (usize::from(empty) << 5)
            | (usize::from(ct) << 4)
            | (usize::from(cr && p.c_mask != 0 && p.uv_odd && p.uv_empty) << 3)
            | (usize::from(cr) << 2)
            | (usize::from(reads.derived.is_some()) << 1)
            | usize::from(reads.raw.is_some());
        let ft = self.c.preflight(p, input.reserve);
        let st = bank::Tick {
            ce: p.ce,
            phase: input.phase,
            raw_issue: reads.raw,
            derived_issue: reads.derived,
            ready_read: reads.ready,
            admission: input.row.map(|v| (self.c.allocate, v)),
            d_return: input.d.map(|v| (self.c.d_owner.unwrap_or(0), v)),
            l_return: input.l.map(|v| (self.c.l_owner.unwrap_or(0), v)),
            ..Default::default()
        };
        let se = self.s.prepare(st, input.loan);
        let fb = ft.is_err();
        let sb = se.is_err();
        let allow = !input.deny && !fb && !sb;
        let advance = p.ce && allow;
        let b = |v| usize::from(v);
        let rp = (b(st.ready_read.is_some()) << 2)
            | (b(st.derived_issue.is_some() || st.l_return.is_some()) << 1)
            | b(st.raw_issue.is_some() || st.d_return.is_some());
        let wp = (b(st.l_return.is_some()) << 2) | b(st.admission.is_some());
        writeln!(self.tb,"ce={};external_allow={};reserve_check={};admit={};dz={};lz={};dr={};lr={};ud={};uds={};mask={};odd={};empty={};uc={};ucs={};ucl={};cz={};credit={};retire={};phase={};row_write={};raw_data=36'h{:x};derived_data=53'h{:x};held_derived=53'h{:x};loan_owner={};ready_data=83'h{:x};#10;",b(offered.ce),b(!input.deny),b(input.reserve),b(offered.admit),b(offered.d_zero),b(offered.l_zero),b(offered.d_return),b(offered.l_return),b(offered.uv_d.is_some()),offered.uv_d.unwrap_or(0),offered.c_mask,b(offered.uv_odd),b(offered.uv_empty),b(offered.uv_c.is_some()),offered.uv_c.map_or(0,|x|x.0),offered.uv_c.map_or(0,|x|x.1),b(offered.c_zero),b(offered.c_credit),offered.late_retire,input.phase,b(input.row.is_some()),input.row.map_or(0,raw_bits),input.d.map_or(0,|v|bank::pack_derived(v).unwrap()),input.loan.map_or(0,|(_,v)|bank::pack_derived(v).unwrap()),input.loan.map_or(0,|v|v.0),input.l.map_or(0,|v|bank::pack_ready(v).unwrap())).unwrap();
        writeln!(self.tb,"if(fb!=={}||sb!=={}||advance!=={}||sa!=={}||proposal_cmds!==7'd{}||proposal_reads!==3'd{}||proposal_writes!==3'd{}||reads!==3'd{}||writes!==3'd{})$fatal(1,\"raw/commit edge{} fb=%b sb=%b pc=%b pr=%b pw=%b\",fb,sb,proposal_cmds,proposal_reads,proposal_writes);",b(fb),b(sb),b(advance),b(advance),commands,if p.ce{rp}else{0},if p.ce{wp}else{0},if advance{rp}else{0},if advance{wp}else{0},self.edges).unwrap();
        // Physical Raw address remains live even when the read/commit is denied.
        let ra = st.d_return.map_or(self.c.d_head, |v| v.0);
        if let Some(word) = self.known[ra] {
            writeln!(
                self.tb,
                "if(raw_q!==36'h{word:x})$fatal(1,\"retained Raw edge{}\");",
                self.edges
            )
            .unwrap();
        }
        if advance {
            for (i, q) in ["raw_q", "derived_q", "ready_q"].iter().enumerate() {
                if let Some((_, word)) = se.as_ref().unwrap().reads[i] {
                    writeln!(
                        self.tb,
                        "if({q}!=={}'h{word:x})$fatal(1,\"bank read edge{}\");",
                        [36, 56, 84][i],
                        self.edges
                    )
                    .unwrap();
                }
            }
            let next = self.c.prepare_shared(ft.unwrap(), true).unwrap().0;
            let e = self.s.prepare_shared(st, input.loan, true).unwrap();
            if let Some((a, w)) = e.writes[0] {
                self.known[a] = Some(w as u64);
            }
            self.s.commit(e);
            self.c = next;
        } else {
            if let Ok(t) = ft {
                assert_eq!(self.c.prepare_shared(t, false).unwrap().0, self.c);
            }
            if se.is_ok() {
                assert_eq!(
                    self.s.prepare_shared(st, input.loan, false).unwrap(),
                    bank::Edge::default()
                );
            }
        }
        self.fe_fault |= fb;
        self.fault |= fb || sb;
        writeln!(self.tb,"clk=1;#10;clk=0;#10;if(state!==73'h{:x}||fault!=={}||terminal!=={})$fatal(1,\"state edge{} got=%h\",state);",packed(&self.c),b(self.fe_fault),b(self.fault),self.edges).unwrap();
        self.edges += 1;
        assert!(self.edges <= 5000);
    }
    fn empty(&mut self) {
        self.edge(Input {
            p: Ports {
                ce: true,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    fn admit(&mut self, i: usize, mask: u8) {
        self.edge(Input {
            p: Ports {
                ce: true,
                ..Default::default()
            },
            reserve: true,
            row: Some(raw(i, mask)),
            ..Default::default()
        });
        let snapshot = self.c.clone();
        self.empty();
        self.empty();
        assert_eq!(snapshot, self.c);
        self.edge(Input {
            p: Ports {
                ce: true,
                admit: true,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    fn finish(&mut self, i: usize, mask: u8) {
        let d = lod::Input {
            slope: 217,
            bias: raw(i, mask).bias,
            header: raw(i, mask).header,
        };
        let l = lod::Output {
            quad: d.header.quad,
            mask: d.header.mask,
            slot: d.header.slot,
            context: lod::CoordinateContext {
                shift: -5,
                nearest: false,
                halve: true,
                side: [8, 4],
                parents: [479, 32],
                levels: [3, 2],
                last_fine: false,
            },
        };
        bank::ReadyDecision::capture(l).unwrap();
        self.edge(Input {
            p: Ports {
                ce: true,
                d_zero: true,
                uv_d: Some(self.c.d_head),
                ..Default::default()
            },
            ..Default::default()
        });
        self.edge(Input {
            p: Ports {
                ce: true,
                d_return: true,
                ..Default::default()
            },
            phase: 3,
            d: Some(d),
            ..Default::default()
        });
        let owner = self.c.l_head;
        self.edge(Input {
            p: Ports {
                ce: true,
                l_zero: true,
                ..Default::default()
            },
            phase: 4,
            loan: Some((owner, d)),
            ..Default::default()
        });
        self.edge(Input {
            p: Ports {
                ce: true,
                l_return: true,
                ..Default::default()
            },
            loan: Some((owner, d)),
            l: Some(l),
            ..Default::default()
        });
        if mask == 0 {
            self.empty();
        } else {
            for lane in (0..4).filter(|lane| mask >> lane & 1 != 0) {
                self.edge(Input {
                    p: Ports {
                        ce: true,
                        c_mask: mask,
                        c_zero: true,
                        c_credit: true,
                        uv_c: Some((self.c.c_head, lane)),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            }
        }
    }
}

fn bounded(mut command: Command, path: &Path, name: &str) {
    command
        .stdout(Stdio::from(
            std::fs::File::create(path.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            std::fs::File::create(path.join(format!("{name}.err"))).unwrap(),
        ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                std::fs::read_to_string(path.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(path.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("shared preflight process watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires Icarus"]
fn count_precompare_actual_rtl_four_state() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/frontend-count-precompare-20261010");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("dut.v"), rtl::runtime_frontend::verilog()).unwrap();
    std::fs::write(path.join("tb.v"),r#"module tb;
reg[3:0]rawcnt=0,dcnt=0,lcnt=0,ocnt=0;reg dv=0,lv=0,r=0,d=0;
gpu_v2_texture_runtime_frontend dut(.clk(1'b0),.reset(1'b1),.ce(1'b0));
wire original=(dut.tracked+{5'b0,dut.rf}>{2'b0,dut.occupied}+{5'b0,dut.done});
integer a,b,k,j,known=0,unknown=0;
initial begin #200000;$fatal(1,"count precompare watchdog");end
task check;begin
 #1;if(dut.tracked_bad!==original)$fatal(1,"count precompare A=%d B=%d rf=%b done=%b old=%b new=%b",dut.tracked,dut.occupied,r,d,original,dut.tracked_bad);
 if((r===1'b0||r===1'b1)&&(d===1'b0||d===1'b1))known=known+1;else unknown=unknown+1;
end endtask
initial begin
 force dut.raw_count=rawcnt;force dut.derived_count=dcnt;force dut.lod_count=lcnt;
 force dut.occupied=ocnt;force dut.d_valid=dv;force dut.l_valid=lv;
 force dut.rf=r;force dut.done=d;
 for(a=0;a<=47;a=a+1)begin
  rawcnt=0;dcnt=0;lcnt=0;dv=0;lv=0;
  if(a<=15)rawcnt=a;
  else if(a<=30)begin rawcnt=15;dcnt=a-15;end
  else if(a<=45)begin rawcnt=15;dcnt=15;lcnt=a-30;end
  else begin rawcnt=15;dcnt=15;lcnt=15;dv=1;lv=a-46;end
  for(b=0;b<16;b=b+1)begin
   ocnt=b;
   for(k=0;k<4;k=k+1)begin
    case(k)0:r=0;1:r=1;2:r=1'bx;3:r=1'bz;endcase
    for(j=0;j<4;j=j+1)begin
     case(j)0:d=0;1:d=1;2:d=1'bx;3:d=1'bz;endcase
     check;
    end
   end
  end
 end
 // Unknown count operands must retain the arithmetic comparison's X result.
 for(k=0;k<4;k=k+1)begin
  r=k[1];d=k[0];rawcnt=4'b0x00;dcnt=0;lcnt=0;dv=0;lv=0;ocnt=0;check;
  rawcnt=4'bz000;ocnt=15;check;
  rawcnt=15;ocnt=4'b00x0;check;
  ocnt=4'bz001;check;
 end
 if(known!=3088||unknown!=9216)$fatal(1,"coverage counts");
 $display("PASS precompare known=%0d unknown=%0d",known,unknown);$finish;
end endmodule
"#).unwrap();
    let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
    command
        .current_dir(&path)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"]);
    bounded(command, &path, "compile");
    let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
    command.current_dir(&path).arg("test.vvp");
    bounded(command, &path, "run");
    assert!(std::fs::read_to_string(path.join("run.out"))
        .unwrap()
        .contains("PASS precompare known=3088 unknown=9216"));
}

#[test]
#[ignore = "requires Icarus"]
fn retire_predecode_actual_rtl_four_state() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/frontend-retire-precompare-20261010");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("dut.v"), rtl::runtime_frontend::verilog()).unwrap();
    let mut tb = String::from(
        r#"module tb;
reg[2:0]head=0,reclaim=0;reg[7:0]v=0,q=0,late=0;reg done=0,run=0;reg[3:0]occ=0;
reg[2:0]heads[0:15];reg[7:0]words[0:15];
integer a,b,k,j,u,w,z,checks=0;
initial begin #2000000;$fatal(1,"retire predecode watchdog");end
"#,
    );
    for (i, (capacity, early)) in [(1, true), (2, true), (8, true), (8, false)]
        .into_iter()
        .enumerate()
    {
        writeln!(tb,r#"gpu_v2_texture_runtime_frontend #(.CONTEXTS({capacity}),.EARLY({early}))d{i}(.clk(1'b0),.reset(1'b0),.ce(1'b0),.late_retire(late));
wire[7:0]retired{i}=late|((done&&{early})?(8'b1<<head):8'b0);
wire[7:0]marked{i}=q|retired{i};
wire rf{i}=run&&occ!=0&&marked{i}[reclaim];
reg old_bad{i},new_bad{i};integer row{i};
always@*begin
 old_bad{i}=0;
 if((retired{i}>>{capacity})!=0)old_bad{i}=1;
 for(row{i}=0;row{i}<8;row{i}=row{i}+1)
  if(retired{i}[row{i}]&&(!v[row{i}]||q[row{i}]))old_bad{i}=1;
 new_bad{i}=0;
 if(d{i}.late_retire_bad)new_bad{i}=1;
 if(d{i}.early_done&&d{i}.early_retire_bad)new_bad{i}=1;
end
"#,early=usize::from(early)).unwrap();
    }
    tb.push_str("task check;begin #1;\n");
    for i in 0..4 {
        writeln!(tb,"if(d{i}.retired!==retired{i}||d{i}.rf!==rf{i}||new_bad{i}!==old_bad{i})$fatal(1,\"retire split{i} check%0d head=%b rec=%b valid=%b reclaimed=%b late=%b done=%b oldrf=%b newrf=%b oldbad=%b newbad=%b\",checks,head,reclaim,v,q,late,done,rf{i},d{i}.rf,old_bad{i},new_bad{i});").unwrap();
    }
    tb.push_str("checks=checks+1;end endtask\ninitial begin\n");
    for i in 0..4 {
        writeln!(tb,"force d{i}.c_head=head;force d{i}.reclaim=reclaim;force d{i}.valid=v;force d{i}.reclaimable=q;force d{i}.done=done;force d{i}.running=run;force d{i}.occupied=occ;").unwrap();
    }
    tb.push_str(
        r#"for(a=0;a<8;a=a+1)heads[a]=a;
heads[8]=3'b0x0;heads[9]=3'bx00;heads[10]=3'bxx0;heads[11]=3'b0z1;
heads[12]=3'bzz0;heads[13]=3'bz01;heads[14]=3'bxxx;heads[15]=3'bzzz;
words[0]=0;words[1]=255;words[2]=8'h55;words[3]=8'haa;
words[4]=1;words[5]=128;words[6]=15;words[7]=240;
words[8]=8'bxxxxxxxx;words[9]=8'bzzzzzzzz;
words[10]=8'b0000x000;words[11]=8'b1x01z0x1;
words[12]=8'b0000z000;words[13]=8'bz1100x01;words[14]=8'bxx001100;words[15]=8'b01zz10xx;
for(a=0;a<16;a=a+1)begin
 head=heads[a];
 for(b=0;b<16;b=b+1)begin
  reclaim=heads[b];
  for(k=0;k<4;k=k+1)begin
   case(k)0:done=0;1:done=1;2:done=1'bx;3:done=1'bz;endcase
   run=1;occ=1;
   for(j=0;j<16;j=j+1)begin
    v=words[j];q=words[(j+3)%16];late=words[(j*5+1)%16];check;
   end
   for(u=0;u<4;u=u+1)begin
    case(u)0:v=0;1:v=255;2:v=8'bxxxxxxxx;3:v=8'bzzzzzzzz;endcase
    for(w=0;w<4;w=w+1)begin
     case(w)0:q=0;1:q=255;2:q=8'bxxxxxxxx;3:q=8'bzzzzzzzz;endcase
     for(z=0;z<4;z=z+1)begin
      case(z)0:late=0;1:late=255;2:late=8'bxxxxxxxx;3:late=8'bzzzzzzzz;endcase
      check;
     end
    end
   end
   // Running/occupied masking is unchanged even with ambiguous row ports.
   run=0;occ=1;check;run=1;occ=0;check;run=1;occ=4'bx001;check;run=1'bx;occ=1;check;
  end
 end
end
if(checks!=86016)$fatal(1,"retire coverage count");
$display("PASS retire predecode vectors=%0d instances=4",checks);$finish;
end endmodule
"#,
    );
    std::fs::write(path.join("tb.v"), tb).unwrap();
    let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
    command
        .current_dir(&path)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"]);
    bounded(command, &path, "compile");
    let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
    command.current_dir(&path).arg("test.vvp");
    bounded(command, &path, "run");
    assert!(std::fs::read_to_string(path.join("run.out"))
        .unwrap()
        .contains("PASS retire predecode vectors=86016 instances=4"));
}

#[test]
#[ignore = "requires Icarus and Gowin native SRAM simulation"]
fn shared_preflight_portable_native() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/runtime-shared-preflight-20261010");
    std::fs::create_dir_all(&path).unwrap();
    let mut total = 0;
    for capacity in [1, 2, 8] {
        let mut r = Replay::new(capacity);
        // Raw row0 is canonical but invisible to Context/D until row3.
        // Completion, covered-lane retirement and private-tail reuse wrap.
        for i in 0..24 {
            let mask = [0, 1, 10, 15][i % 4];
            r.admit(i, mask);
            r.finish(i, mask);
        }
        // Healthy external denial retains proposal and retries without mutation.
        r.edge(Input {
            p: Ports {
                ce: true,
                ..Default::default()
            },
            reserve: true,
            row: Some(raw(3, 0)),
            deny: true,
            ..Default::default()
        });
        r.edge(Input {
            p: Ports {
                ce: false,
                ..Default::default()
            },
            reserve: true,
            row: Some(raw(3, 0)),
            ..Default::default()
        });
        r.admit(3, 0);
        r.finish(3, 0);
        // Malformed canonical row cannot perform a concurrent healthy FE admit.
        let mut malformed = raw(2, 0);
        malformed.bias = 8193;
        r.edge(Input {
            p: Ports {
                ce: true,
                admit: true,
                ..Default::default()
            },
            reserve: true,
            row: Some(malformed),
            deny: true,
            ..Default::default()
        });
        assert!(r.fault);
        r.admit(4, 0);
        r.reset();
        // Invalid FE retirement cannot commit an otherwise legal Raw row0.
        r.edge(Input {
            p: Ports {
                ce: true,
                late_retire: 1,
                ..Default::default()
            },
            reserve: true,
            row: Some(raw(4, 0)),
            ..Default::default()
        });
        assert!(r.fault);
        r.empty();
        r.reset();
        // Wrong UV owner must preserve the concurrently offered next tail Raw.
        r.admit(5, 0);
        r.edge(Input {
            p: Ports {
                ce: true,
                d_zero: true,
                uv_d: Some((r.c.d_head + 1) % 8),
                ..Default::default()
            },
            row: Some(raw(6, 0)),
            ..Default::default()
        });
        assert!(r.fault);
        r.empty();
        r.reset();
        // Correct controller D-return, wrong bank identity, and Raw write together.
        r.admit(7, 0);
        r.edge(Input {
            p: Ports {
                ce: true,
                d_zero: true,
                uv_d: Some(r.c.d_head),
                ..Default::default()
            },
            ..Default::default()
        });
        let d = lod::Input {
            slope: 1,
            bias: 0,
            header: raw(8, 0).header,
        };
        r.edge(Input {
            p: Ports {
                ce: true,
                d_return: true,
                ..Default::default()
            },
            phase: 3,
            d: Some(d),
            row: Some(raw(9, 0)),
            ..Default::default()
        });
        assert!(r.fault);
        r.empty();
        r.reset();
        // Old full credit and same-address Raw read/write cannot self-fund.
        for i in 0..capacity {
            r.admit(i, 0);
        }
        r.edge(Input {
            p: Ports {
                ce: true,
                d_zero: true,
                uv_d: Some(r.c.d_head),
                ..Default::default()
            },
            reserve: true,
            row: Some(raw(11, 0)),
            ..Default::default()
        });
        assert!(r.fault);
        r.empty();
        r.reset();
        r.admit(12, 0);
        r.finish(12, 0);
        writeln!(
            r.tb,
            "$display(\"PASS shared preflight capacity{capacity} {} edges\");$finish;end endmodule",
            r.edges
        )
        .unwrap();
        total += r.edges;
        let dir = path.join(format!("capacity{capacity}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tb.v"), r.tb).unwrap();
        std::fs::write(dir.join("fe.v"), rtl::runtime_frontend::verilog_shared()).unwrap();
        for native in [false, true] {
            let name = if native { "native" } else { "portable" };
            std::fs::write(
                dir.join(format!("{name}.v")),
                rtl::context_store::runtime_source_shared(capacity, native).unwrap(),
            )
            .unwrap();
            let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
            if native {
                command.arg("-DSHARED_NATIVE");
            }
            command.current_dir(&dir).args([
                "-g2012",
                "-s",
                "tb",
                "-o",
                &format!("{name}.vvp"),
                "fe.v",
                &format!("{name}.v"),
                "tb.v",
            ]);
            if native {
                command.arg(
                    Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            }
            bounded(command, &dir, &format!("compile-{name}"));
            let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
            command.current_dir(&dir).arg(format!("{name}.vvp"));
            bounded(command, &dir, &format!("run-{name}"));
            assert!(std::fs::read_to_string(dir.join(format!("run-{name}.out")))
                .unwrap()
                .contains("PASS shared preflight"));
        }
    }
    std::fs::write(path.join("receipt.json"),format!("{{\"capacities\":[1,2,8],\"model_edges\":{total},\"rtl_edges\":{},\"backends\":[\"portable\",\"native\"],\"scope\":\"shared FE and selected Raw/Ready port coalition only\"}}",total*2)).unwrap();
}
