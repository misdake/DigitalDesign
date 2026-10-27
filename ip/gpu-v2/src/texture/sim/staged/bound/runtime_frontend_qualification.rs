//! Live Runtime against frozen 0e91db6 execution and independent queue RTL.
//! Test stimulus comes from live leaf ports; RTL contains no trace/calendar ROM.
use super::super::{control, frontend_reference_runtime as old};
use super::{qualification as q, *};
use crate::texture::{
    emu::runtime_frontend::{Controller, Metadata},
    ports::Filter,
    rtl,
};
use std::{
    fmt::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const LIMIT: u64 = 20_000;
fn packed(c: &Controller) -> u128 {
    let mut bits = 0u128;
    let mut shift = 0;
    let mut push = |value: usize, width: usize| {
        bits |= (value as u128) << shift;
        shift += width;
    };
    push(c.allocate, 3);
    push(c.reclaim, 3);
    push(c.occupied, 4);
    push(c.d_head, 3);
    push(c.raw_count, 4);
    push(usize::from(c.d_owner.is_some()), 1);
    push(c.d_owner.unwrap_or(0), 3);
    push(c.l_head, 3);
    push(c.derived_count, 4);
    push(usize::from(c.l_owner.is_some()), 1);
    push(c.l_owner.unwrap_or(0), 3);
    push(c.c_head, 3);
    push(c.lod_count, 4);
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
    for v in &c.slots {
        push(
            v.map_or(0, |v| match v.metadata {
                Metadata::Raw => 0,
                Metadata::Derived => 1,
                Metadata::Ready => 2,
            }),
            2,
        );
    }
    for _ in c.slots.len()..8 {
        push(0, 2);
    }
    push(c.ordinal, 2);
    assert_eq!(shift, 73);
    bits
}
fn bounded(mut cmd: Command, dir: &Path, name: &str) {
    let mut child = cmd
        .stdout(Stdio::from(
            std::fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            std::fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            assert!(
                s.success(),
                "{} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("frontend RTL watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
// Project only expired C ordinals in the frozen executor. All other
// owners, IDs, payload/storage fields, stages, events and counters are exact.
fn projected_old(r: &old::Runtime) -> String {
    let mut state = r.preparation.ordered_state();
    for (slot, context) in state.contexts.iter_mut().enumerate() {
        if state.cursors[10] == 0 || slot != state.cursors[9] {
            if let Some((_, next)) = context {
                *next = 0;
            }
        }
    }
    format!("{state:?}")
}
fn run(rtl_check: bool) {
    let root = std::env::var_os("GPU_V2_FRONTEND_QUALIFICATION_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/texture-runtime-frontend-qualification")
        });
    std::fs::create_dir_all(&root).unwrap();
    let mut csv=String::from("profile,wall,accepted,results,ce0,uv_return_ce0,D_held,LOD_held,C_full,context_peak,early_context_reuse\n");
    let mut totals = [0usize; 7];
    for (contexts, early) in [(1, true), (3, false), (8, true), (8, false)] {
        for paused in [false, true] {
            let name = format!("N{contexts}-E{early}-P{paused}");
            let (slots, bytes) = q::fixture();
            let p = control::Hardware {
                contexts,
                release_after_capture: early,
                max_cycles: LIMIT,
                ..Default::default()
            };
            let cache = timed::Hardware {
                prefetch: false,
                max_quads: 1,
                max_cycles: LIMIT,
                ..Default::default()
            };
            let mut new = Runtime::new(&slots, p, cache.clone(), LIMIT).unwrap();
            let allocation = super::super::runtime_inventory::describe(
                &new.preparation.binding,
                p,
                new.cache.external_context().1,
            )
            .unwrap();
            let ca = (usize::BITS - (contexts - 1).leading_zeros()) as u64;
            let cq = (usize::BITS - contexts.leading_zeros()) as u64;
            assert_eq!(
                allocation
                    .rows
                    .iter()
                    .find(|r| r.name == "ordered context control")
                    .unwrap()
                    .ff_bits,
                5 * ca + 4 * cq + 2 * (ca + 1) + 4 * contexts as u64 + 2 + 1
            );
            if contexts == 8 {
                assert_eq!(
                    allocation
                        .rows
                        .iter()
                        .find(|r| r.name == "ordered context control")
                        .unwrap()
                        .ff_bits,
                    74
                );
            }
            let mut old = old::Runtime::new(&slots, p, cache, LIMIT).unwrap();
            let mut mc = q::physical::Physical::new(4096, bytes.clone(), true, paused);
            let mut omc = q::physical::Physical::new(4096, bytes, true, paused);
            let inputs: Vec<_> = (0..96)
                .map(|i| {
                    let mut input = q::input(
                        (i % 16) as u8,
                        (i % 16) as u8,
                        [-0.03125 + (i % 7) as f64 / 32.0, 0.46875],
                        [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
                    );
                    input.lod_bias = 0.5;
                    input
                })
                .collect();
            let raw: Vec<_> = inputs
                .iter()
                .map(RawQuadInput::capture)
                .collect::<Result<_, _>>()
                .unwrap();
            let expected: Vec<_> = inputs.iter().flat_map(q::golden).collect();
            let mut got = vec![];
            let mut accepted = 0;
            let mut rng = 0x439ad851u32;
            let mut coverage = [0usize; 7];
            let mut slot_id = vec![None; contexts];
            let mut tb=format!("`timescale 1ns/1ps\nmodule tb;reg clk=0,reset=1,ce=0,admit=0,dz=0,lz=0,dr=0,lr=0,ud=0,odd=0,empty=0,uc=0,cz=0,credit=0;reg[2:0]uds=0,ucs=0;reg[1:0]ucl=0;reg[3:0]mask=0;reg[7:0]retire=0;wire ready,di,li,cr,request,take,zero,rf,fault;wire[2:0]a,ds,ls,cs,dro,lro,rs,ordinal;wire[1:0]lane;wire[72:0]state;\ngpu_v2_texture_runtime_frontend #(.CONTEXTS({contexts}),.EARLY({}))dut(.clk(clk),.reset(reset),.ce(ce),.admit(admit),.d_phase_zero(dz),.l_phase_zero(lz),.d_return(dr),.l_return(lr),.uv_d_valid(ud),.uv_d_slot(uds),.c_mask(mask),.uv_phase_odd(odd),.uv_c_empty(empty),.uv_c_valid(uc),.uv_c_slot(ucs),.uv_c_lane(ucl),.c_phase_zero(cz),.c_credit(credit),.late_retire(retire),.input_ready(ready),.raw_issue(di),.derived_issue(li),.ready_read(cr),.uv_request(request),.c_take(take),.c_empty(zero),.admit_slot(a),.d_slot(ds),.l_slot(ls),.c_slot(cs),.d_return_slot(dro),.l_return_slot(lro),.c_lane(lane),.c_ordinal(ordinal),.reclaim_fire(rf),.reclaim_slot(rs),.fault(fault),.state(state));\ninitial begin #5;clk=1;#5;clk=0;reset=0;\n",u8::from(early));
            for wall in 0..LIMIT {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let control = timed::Control {
                    ce: !paused || rng & 7 > 1,
                    result_ready: (!paused || wall > 700) && rng % 13 > 2,
                };
                // Hold the actual Member consumer ready port to force C6 saturation;
                // no arithmetic output, queue age or ownership is fabricated.
                new.preparation.membership_ready = !paused || wall >= 200;
                old.preparation.membership_ready = !paused || wall >= 200;
                let before = new.preparation.ordered_state();
                assert_eq!(format!("{:?}", before), projected_old(&old));
                let s = new.step_raw(&mut mc, raw.get(accepted), control).unwrap();
                let os = old.step_raw(&mut omc, raw.get(accepted), control).unwrap();
                // Compare the complete common atomic contract by field. The
                // production row API adds diagnostics outside this frozen
                // reference's Step type; formatting is not the contract.
                assert!(!s.row_accepted, "atomic source has no row handshake");
                assert_eq!(
                    (
                        s.cycle,
                        s.control,
                        s.effective_ce,
                        s.offered,
                        s.input_ready,
                        s.accepted
                    ),
                    (
                        os.cycle,
                        os.control,
                        os.effective_ce,
                        os.offered,
                        os.input_ready,
                        os.accepted
                    ),
                    "{name} handshake edge {wall}"
                );
                assert_eq!(s.results, os.results, "{name} result edge {wall}");
                assert_eq!(s.cache, os.cache, "{name} cache edge {wall}");
                assert_eq!(
                    s.preparation, os.preparation,
                    "{name} preparation edge {wall}"
                );
                assert_eq!(s.color.result_access, os.color.result_access);
                assert_eq!(
                    (s.color.input_ready, s.color.accepted, s.color.output),
                    (os.color.input_ready, os.color.accepted, os.color.output)
                );
                assert_eq!(s.color.events, os.color.events);
                assert_eq!(s.color.snapshot, os.color.snapshot);
                assert_eq!(
                    (
                        s.snapshot.wall,
                        s.snapshot.color_input,
                        s.snapshot.result_lanes,
                        s.snapshot.preparation_live
                    ),
                    (
                        os.snapshot.wall,
                        os.snapshot.color_input,
                        os.snapshot.result_lanes,
                        os.snapshot.preparation_live
                    )
                );
                assert_eq!(s.snapshot.color, os.snapshot.color);
                assert_eq!(s.snapshot.cache, os.snapshot.cache);
                assert_eq!(
                    new.preparation.numerical_banks(),
                    old.preparation.numerical_banks()
                );
                assert_eq!(new.preparation.d_lod_state(), old.preparation.d_lod_state());
                assert_eq!(
                    new.preparation.coordinate_state(),
                    old.preparation.coordinate_state()
                );
                assert_eq!(
                    new.preparation.coefficient_snapshot(),
                    old.preparation.coefficient_snapshot()
                );
                assert_eq!(new.preparation.work_state(), old.preparation.work_state());
                let tr = &new.preparation.trace;
                let otr = &old.preparation.trace;
                assert_eq!(tr.metadata_edge.reads, otr.metadata_edge.reads);
                assert_eq!(tr.metadata_edge.writes[0], otr.metadata_edge.writes[0]);
                assert_eq!(tr.metadata_edge.writes[2], otr.metadata_edge.writes[2]);
                assert!(tr.metadata_edge.writes[1].is_none());
                assert_eq!(tr.uv, otr.uv);
                assert_eq!(
                    format!("{:?}", new.preparation.ordered_state()),
                    projected_old(&old)
                );
                let b = tr.frontend_before.as_ref().unwrap();
                let after = new.preparation.frontend_state();
                assert!(after.ordinal < 4, "only a2-bit live ordinal is retained");
                if tr.frontend_tick.c_empty
                    || tr.frontend_tick.c_take
                        && b.lane(tr.frontend_tick.c_mask).unwrap().unwrap().last
                {
                    assert_eq!(after.ordinal, 0, "done/empty clears the only C cursor");
                }
                assert_eq!(after, b.prepare(tr.frontend_tick).unwrap().0);
                let ports = tr.frontend_ports;
                assert_eq!(b.commands(ports).unwrap(), tr.frontend_tick);
                coverage[0] += usize::from(!s.effective_ce);
                coverage[1] += usize::from(!s.effective_ce && tr.uv.returned.is_some());
                coverage[2] += usize::from(!s.effective_ce && b.d_owner.is_some());
                coverage[3] += usize::from(!s.effective_ce && b.l_owner.is_some());
                coverage[4] += usize::from(s.effective_ce && b.lod_count != 0 && !ports.c_credit);
                coverage[5] = coverage[5].max(after.occupied);
                if s.accepted {
                    assert!(b.occupied < contexts, "same-edge Context reclaim borrowed");
                    let slot = b.allocate;
                    if early
                        && slot_id[slot].is_some_and(|id: usize| s.snapshot.result_lanes[id] != 0)
                    {
                        coverage[6] += 1;
                    }
                    slot_id[slot] = Some(raw[accepted].quad_id as usize);
                    accepted += 1;
                }
                got.extend(s.results.iter().copied());
                if !s.effective_ce {
                    assert_eq!(*b, after);
                }
                if rtl_check {
                    let bit = |b: bool| u8::from(b);
                    writeln!(tb,"ce={};admit={};dz={};lz={};dr={};lr={};ud={};uds={};mask={};odd={};empty={};uc={};ucs={};ucl={};cz={};credit={};retire={};#5;",
                bit(ports.ce),bit(ports.admit),bit(ports.d_zero),bit(ports.l_zero),bit(ports.d_return),bit(ports.l_return),bit(ports.uv_d.is_some()),ports.uv_d.unwrap_or(0),ports.c_mask,bit(ports.uv_odd),bit(ports.uv_empty),bit(ports.uv_c.is_some()),ports.uv_c.map_or(0,|v|v.0),ports.uv_c.map_or(0,|v|v.1),bit(ports.c_zero),bit(ports.c_credit),ports.late_retire).unwrap();
                    writeln!(
                        tb,
                        "if(state!==73'h{:x}||fault!==0||ready!==1'b{})$fatal(1,\"pre {wall}\");",
                        packed(b),
                        bit(b.input_ready())
                    )
                    .unwrap();
                    let released = s
                        .preparation
                        .events
                        .iter()
                        .any(|e| matches!(e, PreparationEvent::SharedRelease { .. }));
                    let signals = [
                        tr.d_input.is_some(),
                        tr.l_input.is_some(),
                        tr.metadata_tick.ready_read.is_some(),
                        tr.uv_tick.coordinate.is_some(),
                        tr.c_input.is_some(),
                        tr.frontend_tick.c_empty,
                        released,
                    ];
                    writeln!(
                        tb,
                        "if({{di,li,cr,request,take,zero,rf}}!==7'b{})$fatal(1,\"ports {wall}\");",
                        signals
                            .iter()
                            .map(|v| if *v { '1' } else { '0' })
                            .collect::<String>()
                    )
                    .unwrap();
                    if let Some(l) = b.lane(ports.c_mask).unwrap() {
                        writeln!(tb,"if(lane!==2'd{}||ordinal!==3'd{}||cs!==3'd{})$fatal(1,\"lane {wall}\");",l.lane,l.ordinal,l.slot).unwrap();
                    }
                    writeln!(
                        tb,
                        "clk=1;#5;if(state!==73'h{:x}||fault!==0)$fatal(1,\"post {wall}\");clk=0;",
                        packed(&after)
                    )
                    .unwrap();
                }
                if accepted == raw.len() && new.idle() {
                    break;
                }
            }
            assert_eq!(accepted, raw.len());
            assert!(new.idle());
            assert!(old.idle());
            assert_eq!(got, expected);
            for i in 0..coverage.len() {
                totals[i] += coverage[i];
            }
            if rtl_check {
                writeln!(tb,"$display(\"PASS {name} edges={} raw={} results={}\");$finish;end initial begin #2000000;$fatal(1,\"wall bound\");end endmodule",new.stats.link.wall,accepted,got.len()).unwrap();
                let dir = root.join(&name);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("controller.v"), rtl::runtime_frontend::verilog()).unwrap();
                std::fs::write(dir.join("tb.v"), tb).unwrap();
                let mut compile = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
                compile.current_dir(&dir).args([
                    "-g2012",
                    "-s",
                    "tb",
                    "-o",
                    "test.vvp",
                    "controller.v",
                    "tb.v",
                ]);
                bounded(compile, &dir, "compile");
                let mut exec = Command::new(std::env::var_os("VVP_EXE").unwrap());
                exec.current_dir(&dir).arg("test.vvp");
                bounded(exec, &dir, "run");
            }
            writeln!(
                csv,
                "{name},{},{accepted},{},{},{},{},{},{},{},{}",
                new.stats.link.wall,
                got.len(),
                coverage[0],
                coverage[1],
                coverage[2],
                coverage[3],
                coverage[4],
                coverage[5],
                coverage[6]
            )
            .unwrap();
        }
    }
    assert!(
        totals[0] > 0
            && totals[1] > 0
            && totals[2] > 0
            && totals[3] > 0
            && totals[4] > 0
            && totals[6] > 0,
        "coverage {totals:?}"
    );
    std::fs::write(
        root.join(if rtl_check {
            "rtl-coverage.csv"
        } else {
            "equivalence-coverage.csv"
        }),
        csv,
    )
    .unwrap();
}
#[test]
fn frontend_live_raw_runtime_matches_frozen_executor_every_edge() {
    run(false);
}
#[test]
#[ignore = "requires Icarus"]
fn frontend_queue_rtl_matches_live_runtime_leaf_ports() {
    run(true);
}
