//! Actual Runtime phase/last-read proof. No sampled output is retained by DUT.
use super::{qualification as q, *};
use crate::texture::ports::{RefillEvent, RefillPort};
use crate::texture::{emu::context_store, ports::Filter};
use std::fmt::Write;
struct Port {
    physical: q::physical::Physical,
    wall: u64,
    terminal_delay: u64,
    held: Option<(u64, u64)>,
    requests: Vec<(u64, u64, u64)>,
}
impl RefillPort for Port {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        assert!(
            self.held.is_none(),
            "new request preceded old terminal event"
        );
        let id = self.physical.submit_read(address, bytes)?;
        self.requests.push((self.wall, id, address));
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        self.wall += 1;
        assert!(self.wall <= 30_000, "physical owner test watchdog");
        let mut result = vec![];
        if let Some((id, due)) = self.held {
            if self.wall == due {
                result.push(RefillEvent::Complete { id });
                self.held = None;
            }
        }
        for e in self.physical.step()? {
            if let RefillEvent::Complete { id } = e {
                if self.terminal_delay != 0 {
                    assert!(self.held.is_none());
                    self.held = Some((id, self.wall + self.terminal_delay));
                    continue;
                }
            }
            result.push(e);
        }
        Ok(result)
    }
}

#[path = "runtime_derived_rtl.rs"]
mod rtl_probe;
#[test]
fn actual_runtime_derived_loan_last_reads() {
    run(false);
}
#[test]
#[ignore = "requires Icarus and Gowin primitives"]
fn actual_runtime_derived_loan_rtl_reads_live_banks() {
    run(true);
}
fn run(rtl_check: bool) {
    let root =
        std::path::PathBuf::from(std::env::var_os("GPU_DERIVED_OUTPUT").unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/texture-runtime-derived-qualification")
                .into_os_string()
        }));
    std::fs::create_dir_all(&root).unwrap();
    let mut csv=String::from("profile,edges,accepted,pixels,derived_peak,l_reads,l_identity,expired_reads,CE0_derived,C_full,context_peak,same_edge_Daccept_Lreturn,terminal_held\n");
    let mut profiles = vec![];
    for (contexts, early) in [(1, true), (3, false), (8, true), (8, false)] {
        for paused in [false, true] {
            profiles.push((contexts, early, paused, 0));
        }
    }
    for early in [false, true] {
        for scenario in [1, 2] {
            profiles.push((8, early, true, scenario));
        }
    }
    for (contexts, early, paused, scenario) in profiles {
        let name = if scenario == 0 {
            format!("N{contexts}-E{early}-P{paused}")
        } else {
            format!("N{contexts}-E{early}-P{paused}-S{scenario}")
        };
        let (mut slots, mut bytes) = q::fixture();
        if scenario == 1 {
            let first = slots[0];
            let template = bytes.clone();
            for i in 1..16 {
                let mut slot = first;
                slot.base_address += i * template.len() as u32;
                slots.push(slot);
                bytes.extend(&template);
            }
        }
        let hw = super::super::control::Hardware {
            contexts,
            release_after_capture: early,
            max_cycles: 20_000,
            ..Default::default()
        };
        let mut r = q::runtime(&slots, hw);
        let mut reference = super::super::derived_reference_runtime::Runtime::new(
            &slots,
            hw,
            timed::Hardware {
                prefetch: false,
                max_quads: 1,
                max_cycles: 20_000,
                ..Default::default()
            },
            20_000,
        )
        .unwrap();
        let mut reference_port = Port {
            physical: q::physical::Physical::new(4096, bytes.clone(), true, paused),
            wall: 0,
            terminal_delay: if scenario == 2 { 23 } else { 0 },
            held: None,
            requests: vec![],
        };
        let mut port = Port {
            physical: q::physical::Physical::new(4096, bytes, true, paused),
            wall: 0,
            terminal_delay: if scenario == 2 { 23 } else { 0 },
            held: None,
            requests: vec![],
        };
        let inputs: Vec<_> = (0..96)
            .map(|i| {
                q::input(
                    (i % 16) as u8,
                    (i % 16) as u8,
                    [-0.03125 + (i % 7) as f64 / 32., 0.46875],
                    [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
                )
            })
            .collect();
        let inputs: Vec<_> = inputs
            .into_iter()
            .enumerate()
            .map(|(i, mut v)| {
                if scenario == 1 {
                    v.slot = (i % 16) as u8;
                }
                v
            })
            .collect();
        let expected: Vec<_> = inputs.iter().flat_map(q::golden).collect();
        let mut accepted = 0;
        let mut pixels = vec![];
        let mut counts = [0usize; 9];
        let binding = super::super::Binding::build().unwrap();
        let (dc, lc, _) = super::super::runtime_preparation::calendars(&binding).unwrap();
        let lc = lc.with_issue_offset(4).unwrap();
        let desc = derivative::rtl::describe(&dc, "actual_d");
        let uw = desc
            .inputs
            .iter()
            .find(|i| i.key == "helper_uv")
            .unwrap()
            .width;
        let mut tb=format!("module tb;reg clk=0,reset=1,ce=0,admit=0,d_issue=0,l_issue=0,c_read=0;reg[3:0]allocate=0,d_head=0,l_head=0,c_head=0,d_owner=0,l_owner=0;reg[35:0]raw_data=0;reg[{}:0]uv=0;wire bad,advance,df,lf;wire[2:0]reads,writes,phase;wire[35:0]raw_q;wire[55:0]derived_q;wire[83:0]ready_q;wire[127:0]duv;GSR GSR(.GSRI(1'b1));derived_probe dut(.*);initial begin #5;clk=1;#5;clk=0;reset=0;\n",uw*8-1);
        let mut trace=String::from("wall,CE,Dphase,Lphase,derived_count,Downer,Lowner,Dvalid,Dissue,Lissue,Lreturn,owner,derived_word\n");
        for wall in 0..20_000 {
            r.preparation.membership_ready = !paused || wall >= 200;
            reference.preparation.membership_ready = !paused || wall >= 200;
            let b = r.preparation.frontend_state();
            assert!(b.derived_count <= 1, "{name} wall{wall}");
            let (_, _, dphase, lphase) = r.preparation.d_lod_state();
            let (dvalid, loan) = r.preparation.derived_loan_probe();
            let owner = (b.d_head + contexts - 1) % contexts;
            let ctl = timed::Control {
                ce: !paused || wall % 7 > 1,
                result_ready: !paused || (wall > 700 && wall % 13 > 2),
            };
            let s = r
                .step(
                    &mut port,
                    inputs.get(accepted),
                    timed::Control {
                        ce: !paused || wall % 7 > 1,
                        result_ready: !paused || (wall > 700 && wall % 13 > 2),
                    },
                )
                .unwrap();
            let os = reference
                .step(&mut reference_port, inputs.get(accepted), ctl)
                .unwrap();
            // The frozen atomic executor predates row_accepted and
            // result_quad_last. Compare every shared observable explicitly;
            // those newer transport events must not alter the legacy replay.
            assert!(!s.row_accepted, "atomic transport has no UV row event");
            assert_eq!(
                format!(
                    "{:?}",
                    (
                        s.cycle,
                        s.control,
                        s.effective_ce,
                        s.offered,
                        s.input_ready,
                        s.accepted,
                        &s.results,
                        &s.color,
                        &s.cache,
                        &s.preparation,
                        &s.snapshot
                    )
                ),
                format!(
                    "{:?}",
                    (
                        os.cycle,
                        os.control,
                        os.effective_ce,
                        os.offered,
                        os.input_ready,
                        os.accepted,
                        &os.results,
                        &os.color,
                        &os.cache,
                        &os.preparation,
                        &os.snapshot
                    )
                ),
                "baseline538 per edge {name} wall{wall}"
            );
            assert_eq!(
                r.preparation.d_lod_state(),
                reference.preparation.d_lod_state()
            );
            assert_eq!(
                r.preparation.numerical_banks(),
                reference.preparation.numerical_banks()
            );
            assert_eq!(port.requests, reference_port.requests);
            let mut ns = r.preparation.ordered_state();
            let mut rs = reference.preparation.ordered_state();
            // Expired/non-C-head ordinals are diagnostic-only. Keep all live
            // cursor/owner/payload comparisons and the active C ordinal intact.
            for (i, v) in ns.contexts.iter_mut().enumerate() {
                if ns.cursors[10] == 0 || i != ns.cursors[9] {
                    if let Some((_, n)) = v {
                        *n = 0;
                    }
                }
            }
            for (i, v) in rs.contexts.iter_mut().enumerate() {
                if rs.cursors[10] == 0 || i != rs.cursors[9] {
                    if let Some((_, n)) = v {
                        *n = 0;
                    }
                }
            }
            assert_eq!(ns.cursors, rs.cursors);
            assert_eq!(ns.contexts, rs.contexts);
            assert_eq!(ns.uv, rs.uv);
            assert_eq!(ns.coordinates, rs.coordinates);

            let t = &r.preparation.trace;
            assert_eq!(
                t.metadata_edge.reads,
                reference.preparation.trace.metadata_edge.reads
            );
            for a in [
                t.metadata_tick.derived_issue,
                t.metadata_tick.l_return.map(|x| x.0),
            ]
            .into_iter()
            .flatten()
            {
                assert_eq!(a, owner, "ordered predecessor {name} wall{wall}");
                assert_eq!(
                    t.metadata_edge.reads[1],
                    Some((a, context_store::pack_derived(loan).unwrap() as u128)),
                    "actual bank last read {name} wall{wall}"
                );
                counts[3] += usize::from(!dvalid);
            }
            counts[0] = counts[0].max(b.derived_count);
            counts[1] += usize::from(t.l_input.is_some());
            counts[2] += usize::from(t.metadata_tick.l_return.is_some());
            counts[4] +=
                usize::from(!s.effective_ce && (b.derived_count != 0 || b.l_owner.is_some()));
            counts[5] +=
                usize::from(s.effective_ce && b.lod_count != 0 && !t.frontend_ports.c_credit);
            counts[6] = counts[6].max(b.occupied);
            counts[7] += usize::from(t.d_input.is_some() && t.l_capture.is_some());
            counts[8] += usize::from(port.held.is_some());
            if t.d_input.is_some() {
                assert_eq!(dphase, 0);
                assert!(b.derived_count == 0);
            }
            if t.d_capture.is_some() {
                assert_eq!(dphase, 3);
            }
            if t.l_input.is_some() {
                assert_eq!((dphase, lphase), (4, 0));
                assert!(!dvalid);
            }
            if t.l_capture.is_some() {
                assert_eq!((dphase, lphase), (0, 4));
            }
            writeln!(
                trace,
                "{wall},{},{dphase},{lphase},{},{:?},{:?},{dvalid},{},{},{},{owner},{:x}",
                s.effective_ce,
                b.derived_count,
                b.d_owner,
                b.l_owner,
                t.d_input.is_some(),
                t.l_input.is_some(),
                t.l_capture.is_some(),
                context_store::pack_derived(loan).unwrap()
            )
            .unwrap();
            if rtl_check {
                let bit = |v: bool| u8::from(v);
                let mt = t.metadata_tick;
                writeln!(tb,"ce={};admit={};d_issue={};l_issue={};c_read={};allocate={};d_head={};l_head={};c_head={};d_owner={};l_owner={};raw_data=36'h{:x};",
                bit(s.effective_ce),bit(mt.admission.is_some()),bit(t.d_input.is_some()),bit(t.l_input.is_some()),bit(mt.ready_read.is_some()),b.allocate,b.d_head,b.l_head,b.c_head,b.d_owner.unwrap_or(0),b.l_owner.unwrap_or(0),mt.admission.map_or(0,|(_,v)|v.pack().unwrap())).unwrap();
                let values = t.d_input.map_or([0i64; 8], |v| v.uv);
                for (i, v) in values.into_iter().enumerate() {
                    writeln!(
                        tb,
                        "uv[{}+:{}]={}'h{:x};",
                        i as u32 * uw,
                        uw,
                        uw,
                        (v as u64) & ((1u64 << uw) - 1)
                    )
                    .unwrap();
                }
                writeln!(tb,"#5;if(bad!==0||df!==0||lf!==0||phase!==3'd{dphase})$fatal(1,\"control {wall}\");").unwrap();
                for (bank, port) in ["raw_q", "derived_q", "ready_q"].into_iter().enumerate() {
                    if let Some((_, word)) = t.metadata_edge.reads[bank] {
                        writeln!(
                            tb,
                            "if({port}!=={}'h{word:x})$fatal(1,\"bank{bank} {wall}\");",
                            [36, 56, 84][bank]
                        )
                        .unwrap();
                    }
                }
                let reads = t
                    .metadata_edge
                    .reads
                    .iter()
                    .enumerate()
                    .fold(0, |m, (i, v)| m | (usize::from(v.is_some()) << i));
                let writes = t
                    .metadata_edge
                    .writes
                    .iter()
                    .enumerate()
                    .fold(0, |m, (i, v)| m | (usize::from(v.is_some()) << i));
                writeln!(tb,"if(reads!==3'd{reads}||writes!==3'd{writes})$fatal(1,\"strobes {wall}\");clk=1;#5;clk=0;").unwrap();
            }
            if s.accepted {
                accepted += 1;
            }
            pixels.extend(s.results.iter().copied());
            if accepted == inputs.len() && r.idle() {
                break;
            }
        }
        assert_eq!(accepted, 96);
        assert!(r.idle());
        assert_eq!(pixels, expected);
        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], 96);
        assert_eq!(counts[2], 96);
        assert_eq!(counts[3], 192);
        if paused {
            assert!(counts[4] > 0);
        }
        if contexts > 1 {
            assert!(counts[7] > 0, "same-edge old bank read");
        }
        if scenario == 2 {
            assert!(counts[8] > 0);
        }
        if scenario == 1 {
            assert!(port.requests.len() > 30, "real misses");
        }
        writeln!(
            csv,
            "{name},{},{accepted},{},{},{},{},{},{},{},{},{},{}",
            r.stats.link.wall,
            pixels.len(),
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            counts[4],
            counts[5],
            counts[6],
            counts[7],
            counts[8]
        )
        .unwrap();
        std::fs::write(root.join(format!("{name}.csv")), trace).unwrap();
        if rtl_check {
            let dir = root.join(&name);
            std::fs::create_dir_all(&dir).unwrap();
            writeln!(tb,"$display(\"PASS actual D/LOD loan {name} edges={}\");$finish;end initial begin #2000000;$fatal(1,\"wall bound\");end endmodule",r.stats.link.wall).unwrap();
            std::fs::write(dir.join("tb.v"), tb).unwrap();
            for (filename, native, loan) in [
                ("portable.v", false, true),
                ("native.v", true, true),
                ("baseline.v", true, false),
            ] {
                std::fs::write(
                    dir.join(filename),
                    rtl_probe::source(&dc, &lc, contexts, native, loan),
                )
                .unwrap();
            }
            rtl_probe::run(&dir);
        }
    }
    std::fs::write(root.join("coverage.csv"), csv).unwrap();
}
