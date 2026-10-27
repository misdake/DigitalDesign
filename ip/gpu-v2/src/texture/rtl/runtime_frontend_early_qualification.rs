//! Fixed EARLY controller versus the frozen generic model and actual SRAM.
use crate::texture::emu::runtime_frontend::*;
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
        assert_eq!(self.c.reclaim, self.c.c_head);
        assert!(self.c.reclaimable.iter().all(|bit| !*bit));
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
        if p.late_retire == 0 {
            assert_eq!(rf, done, "ordered rf=done");
        }
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
        assert_eq!(self.c.reclaim, self.c.c_head);
        assert!(self.c.reclaimable.iter().all(|bit| !*bit));
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

fn canonical(v: lod::Input) -> lod::Output {
    lod::Output {
        quad: v.header.quad,
        mask: v.header.mask,
        slot: v.header.slot,
        context: lod::CoordinateContext {
            shift: -5,
            nearest: false,
            halve: true,
            side: [8, 4],
            parents: [479, 32],
            levels: [3, 2],
            last_fine: false,
        },
    }
}

fn fixture() -> Replay {
    let mut r = Replay::new(8);
    let mut phase = 0;
    let mut admitted = 0;
    let mut row = 0;
    let mut partial = false;
    let mut held: Option<(usize, lod::Input)> = None;
    let mut l_hold = None;
    let mut witness = [0usize; 7];
    let mut rng = 0x5e112e3au32;
    for wall in 0..4000 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let ce = rng & 7 != 0;
        let deny = rng >> 4 & 15 == 0;
        let reserve = admitted < 96 && !partial && r.c.input_ready();
        let continuation = partial;
        let p_admit = continuation && row == 3;
        let incoming = raw(admitted, [0, 1, 6, 15][admitted % 4]);
        let mask = if r.c.lod_count != 0 {
            bank::unpack_ready(r.s.rows().1[r.c.c_head]).mask
        } else {
            0
        };
        let credit = wall > 180 && rng >> 9 & 7 > 1;
        let lane = r.c.lane(mask).unwrap();
        let d = r.c.d_owner.filter(|_| phase == 3).map(|slot| {
            let q = bank::Raw::unpack(r.s.rows().0[slot]);
            lod::Input {
                slope: ((wall * 17) & 0x3ffff) as u64,
                bias: q.bias,
                header: q.header,
            }
        });
        let l =
            r.c.l_owner
                .filter(|_| phase == 0)
                .map(|_| canonical(l_hold.unwrap()));
        let p = Ports {
            ce,
            admit: p_admit,
            d_zero: phase == 0,
            l_zero: phase == 4,
            d_return: d.is_some(),
            l_return: l.is_some(),
            uv_d: (r.c.raw_count != 0).then_some(r.c.d_head),
            c_mask: mask,
            uv_odd: phase & 1 != 0,
            uv_empty: true,
            uv_c: lane.map(|v| (v.slot, usize::from(v.lane))),
            c_zero: phase == 0,
            c_credit: credit,
            ..Default::default()
        };
        let t = r.c.preflight(p, reserve).unwrap();
        let done = t.c_empty || t.c_take && lane.unwrap().last;
        witness[0] += usize::from(r.c.occupied == 8);
        witness[1] += usize::from(r.c.occupied == 8 && done);
        witness[2] += usize::from(t.d_issue && t.l_return && t.c_take);
        witness[3] += usize::from(r.c.lod_count != 0 && !credit);
        witness[4] += usize::from(!ce && partial);
        witness[5] += usize::from(deny && partial);
        witness[6] += usize::from(partial && !p_admit && r.c.raw_count == 0);
        let slot = r.c.allocate;
        if p_admit {
            assert_eq!(bank::Raw::unpack(r.s.rows().0[slot]), incoming);
        }
        let old = r.c.clone();
        let d_owner = r.c.d_owner;
        r.edge(Input {
            p,
            reserve,
            row: reserve.then_some(incoming),
            phase,
            loan: held,
            d,
            l,
            deny,
        });
        assert!(!r.fault);
        if ce && !deny {
            if reserve {
                partial = true;
                row = 1;
                assert_eq!(old.allocate, r.c.allocate);
                assert!(r.c.slots[slot].is_none(), "row0 must remain private");
            } else if continuation {
                row += 1;
                if row == 4 {
                    row = 0;
                    partial = false;
                    admitted += 1;
                }
            }
            if let Some(d) = d {
                held = Some((d_owner.unwrap(), d));
            }
            if t.l_issue {
                l_hold = Some(held.unwrap().1);
            }
            phase = (phase + 1) & 7;
        } else {
            assert_eq!(old, r.c);
        }
        if admitted == 96 && r.c.occupied == 0 {
            break;
        }
        assert!(wall < 3999, "bounded ordered frontend drain");
    }
    assert_eq!(admitted, 96);
    assert_eq!(r.c.occupied, 0);
    assert!(witness.iter().all(|v| *v != 0), "coverage {witness:?}");
    // Faults are checked before a concurrent otherwise valid row admission.
    for kind in 0..5 {
        r.reset();
        let mut p = Ports {
            ce: true,
            admit: true,
            ..Default::default()
        };
        let mut q = raw(3, 15);
        match kind {
            0 => p.d_return = true,
            1 => p.l_return = true,
            2 => p.late_retire = 1,
            3 => q.bias = 8193,
            _ => {
                p.uv_d = Some(1);
                p.d_zero = true;
            }
        }
        if kind == 4 {
            r.admit(4, 15);
        }
        r.edge(Input {
            p,
            reserve: kind != 4,
            row: Some(q),
            d: p.d_return.then_some(bank::unpack_derived(0)),
            l: p.l_return.then_some(canonical(bank::unpack_derived(0))),
            loan: p.l_return.then_some((0, bank::unpack_derived(0))),
            deny: true,
            ..Default::default()
        });
        assert!(r.fault);
        r.empty();
        r.reset();
    }
    for i in 0..8 {
        r.admit(i, 15);
    }
    r.edge(Input {
        p: Ports {
            ce: true,
            admit: true,
            ..Default::default()
        },
        reserve: true,
        row: Some(raw(9, 15)),
        ..Default::default()
    });
    assert!(r.fault);
    r.reset();
    // Leave a known Ready head for combinational unknown-port comparisons.
    r.admit(7, 15);
    let d = lod::Input {
        slope: 7,
        bias: raw(7, 15).bias,
        header: raw(7, 15).header,
    };
    r.edge(Input {
        p: Ports {
            ce: true,
            d_zero: true,
            uv_d: Some(0),
            ..Default::default()
        },
        ..Default::default()
    });
    r.edge(Input {
        p: Ports {
            ce: true,
            d_return: true,
            ..Default::default()
        },
        phase: 3,
        d: Some(d),
        ..Default::default()
    });
    r.edge(Input {
        p: Ports {
            ce: true,
            l_zero: true,
            ..Default::default()
        },
        phase: 4,
        loan: Some((0, d)),
        ..Default::default()
    });
    r.edge(Input {
        p: Ports {
            ce: true,
            l_return: true,
            ..Default::default()
        },
        loan: Some((0, d)),
        l: Some(canonical(d)),
        ..Default::default()
    });
    std::fs::create_dir_all(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/frontend-early-ordered-20261010"),
    )
    .unwrap();
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/frontend-early-ordered-20261010/model.json"),format!("{{\"edges\":{},\"admissions\":96,\"coverage\":{:?},\"invariant\":\"reclaim=C_head,reclaimable=0,rf=done\"}}",r.edges,witness)).unwrap();
    r
}

fn equality() -> &'static str {
    "if({fb,advance,reserve_ready,proposal_cmds,ready,di,li,cr,request,take,zero,a,ds,ls,cs,dro,lro,lane,ordinal,rf,rs,fault,state}!=={ref_fb,ref_advance,ref_reserve_ready,ref_proposal_cmds,ref_ready,ref_di,ref_li,ref_cr,ref_request,ref_take,ref_zero,ref_a,ref_ds,ref_ls,ref_cs,ref_dro,ref_lro,ref_lane,ref_ordinal,ref_rf,ref_rs,ref_fault,ref_state})$fatal(1,\"early/generic projection\");\n"
}

#[test]
fn ordered_generic_invariant_and_profile_storage() {
    let r = fixture();
    assert_eq!(r.c.reclaim, r.c.c_head);
    let v = super::verilog_shared();
    assert!(!v.contains("reg[7:0]valid,reclaimable"));
    assert!(!v.contains("reg[2:0]allocate,reclaim"));
    assert!(v.contains("ss,8'b0,valid"));
}

#[test]
#[ignore = "requires Icarus and native SRAM simulation"]
fn ordered_actual_frontend_and_native_banks_match_frozen_generic() {
    let mut r = fixture();
    let start =
        r.tb.find("gpu_v2_texture_runtime_frontend_shared #")
            .unwrap();
    let end = r.tb[start..].find("gpu_v2_context_store_shared").unwrap() + start;
    let mut reference = r.tb[start..end].replace(")fe(", ")ref_fe(");
    for field in [
        "fb",
        "advance",
        "reserve_ready",
        "proposal_cmds",
        "ready",
        "di",
        "li",
        "cr",
        "request",
        "take",
        "zero",
        "a",
        "ds",
        "ls",
        "cs",
        "dro",
        "lro",
        "lane",
        "ordinal",
        "rf",
        "rs",
        "fault",
        "state",
    ] {
        reference = reference.replace(&format!("({field})"), &format!("(ref_{field})"));
    }
    let declarations="wire ref_fb,ref_advance,ref_reserve_ready,ref_ready,ref_di,ref_li,ref_cr,ref_request,ref_take,ref_zero,ref_rf,ref_fault;wire[6:0]ref_proposal_cmds;wire[2:0]ref_a,ref_ds,ref_ls,ref_cs,ref_dro,ref_lro,ref_ordinal,ref_rs;wire[1:0]ref_lane;wire[72:0]ref_state;integer four;\n";
    // Declare reference wires after the shared inputs, before either instance.
    r.tb = r.tb.replacen(
        "gpu_v2_texture_runtime_frontend_shared #",
        &format!("{declarations}{reference}gpu_v2_texture_runtime_frontend_early_shared #"),
        1,
    );
    let mut tb = String::new();
    for line in r.tb.lines() {
        if line.starts_with("clk=1;") {
            tb.push_str(equality());
        }
        tb.push_str(line);
        tb.push('\n');
    }
    tb.push_str("external_allow=0;ce=1;admit=0;reserve_check=0;row_write=0;dr=0;lr=0;dz=0;lz=0;ud=0;mask=15;odd=1;empty=1;uc=1;ucs=0;ucl=0;cz=1;credit=1;retire=0;\n");
    // Probe unknown live controls without committing malformed partial data.
    tb.push_str("for(four=0;four<16;four=four+1)begin mask=four;#10;\n");
    tb.push_str(equality());
    tb.push_str("end\n");
    for (field, unknowns) in [
        ("mask", vec!["4'bxxxx", "4'bzzzz", "4'b0x01", "4'b1z00"]),
        ("dz", vec!["1'bx", "1'bz"]),
        ("dr", vec!["1'bx", "1'bz"]),
        ("lr", vec!["1'bx", "1'bz"]),
        ("cz", vec!["1'bx", "1'bz"]),
        ("credit", vec!["1'bx", "1'bz"]),
        ("uc", vec!["1'bx", "1'bz"]),
    ] {
        for value in unknowns {
            writeln!(tb, "{field}={value};#10;").unwrap();
            tb.push_str(equality());
        }
        writeln!(tb, "mask=15;dz=0;dr=0;lr=0;cz=1;credit=1;uc=1;").unwrap();
    }
    writeln!(
        tb,
        "$display(\"PASS ordered frontend edges={} fourstate=32\");$finish;end endmodule",
        r.edges
    )
    .unwrap();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/frontend-early-ordered-20261010");
    for native in [false, true] {
        let name = if native { "native" } else { "portable" };
        let dir = path.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tb.v"), &tb).unwrap();
        std::fs::write(
            dir.join("fe.v"),
            format!(
                "{}\n{}",
                super::verilog_shared(),
                rtl::runtime_frontend::verilog_shared()
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("banks.v"),
            rtl::context_store::runtime_source_shared(8, native).unwrap(),
        )
        .unwrap();
        let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        if native {
            command.arg("-DSHARED_NATIVE");
        }
        command.current_dir(&dir).args([
            "-g2012", "-s", "tb", "-o", "test.vvp", "fe.v", "banks.v", "tb.v",
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
        assert!(std::fs::read_to_string(dir.join("run.out"))
            .unwrap()
            .contains("PASS ordered frontend"));
    }
}
