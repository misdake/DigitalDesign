//! Context preflight faults leave every preparation owner and bank unchanged.
use super::*;
fn input(quad: u8) -> derivative::Input {
    derivative::Input {
        uv: [0, 0, 2048, 0, 0, 2048, 2048, 2048],
        bias: 512,
        force_coarsest: false,
        header: derivative::Header {
            quad,
            mask: 15,
            slot: 0,
            max_n: 5,
            has_mip: true,
            filter: 2,
        },
    }
}
fn drive(m: &mut Machine, offer: Option<derivative::Input>) -> Result<Step, String> {
    m.step_packet_port(offer, true, true, true)
        .map(|p| p.preparation)
}
#[test]
fn context_storage_runtime_faults_precede_all_preparation_mutation() {
    for fault in [
        "allocate-address",
        "raw-type",
        "D-owner-overflow",
        "D-UV-owner",
        "D-return-type",
        "D-identity",
        "D-owner-address",
        "LOD-identity",
        "LOD-return-type",
        "C-ordinal",
        "C-UV-owner",
        "admission-range",
    ] {
        let mut m = Machine::new(
            Binding::build().unwrap(),
            Hardware {
                max_cycles: 512,
                ..Default::default()
            },
        )
        .unwrap();
        let first = input(0);
        drive(&mut m, Some(first)).unwrap();
        let mut reached = false;
        for _ in 0..256 {
            let at_d = m.derivative.phase() == 0
                && m.frontend.reference_fault_state().raw_count != 0
                && m.uv.derivative().is_some();
            let at_dr = m.derivative.output().unwrap().is_some();
            let at_lr = m.lod.output().unwrap().is_some();
            let at_c = m.frontend.reference_fault_state().lod_count != 0;
            let at_c_uv = at_c && m.coordinate.phase() == 0 && m.uv.coordinate().is_some();
            let trigger = match fault {
                "raw-type" | "D-owner-overflow" | "D-UV-owner" => at_d,
                "D-return-type" | "D-identity" | "D-owner-address" => at_dr,
                "admission-range" => at_dr && m.input_ready(1),
                "allocate-address" => m.input_ready(1),
                "LOD-identity" | "LOD-return-type" => at_lr,
                "C-ordinal" => at_c,
                "C-UV-owner" => at_c_uv,
                _ => unreachable!(),
            };
            if !trigger {
                drive(&mut m, None).unwrap();
                continue;
            }
            let mut offer = None;
            match fault {
                "allocate-address" => {
                    m.frontend.reference_fault_state().allocate = m.hardware.contexts;
                    offer = Some(input(1));
                }
                "raw-type" => {
                    let owner = m.frontend.view().d_head;
                    m.frontend.reference_fault_state().slots[owner]
                        .as_mut()
                        .unwrap()
                        .metadata = Metadata::Ready
                }
                "D-owner-overflow" => {
                    m.frontend.reference_fault_state().d_owner =
                        Some(m.frontend.reference_fault_state().d_head)
                }
                "D-UV-owner" => {
                    let from = m.frontend.reference_fault_state().d_head;
                    m.frontend.reference_fault_state().d_head = 1;
                    m.frontend.reference_fault_state().slots[1] = Some(Context {
                        metadata: Metadata::Raw,
                    });
                    let mut e = context_store::Edge::default();
                    e.writes[0] = Some((1, m.metadata.rows().0[from] as u128));
                    m.metadata.commit(e);
                }
                "D-return-type" => {
                    let owner = m.frontend.view().d_owner.unwrap();
                    m.frontend.reference_fault_state().slots[owner]
                        .as_mut()
                        .unwrap()
                        .metadata = Metadata::Derived
                }
                "D-owner-address" => {
                    m.frontend.reference_fault_state().d_owner = Some(m.hardware.contexts)
                }
                "D-identity" => {
                    let a = m.frontend.reference_fault_state().d_owner.unwrap();
                    let mut e = context_store::Edge::default();
                    e.writes[0] = Some((a, (m.metadata.rows().0[a] ^ (1 << 17)) as u128));
                    m.metadata.commit(e);
                }
                "LOD-identity" => m.derivative.corrupt_runtime_lod_quad(),
                "LOD-return-type" => {
                    let owner = m.frontend.view().l_owner.unwrap();
                    m.frontend.reference_fault_state().slots[owner]
                        .as_mut()
                        .unwrap()
                        .metadata = Metadata::Raw
                }
                "C-ordinal" => m.frontend.reference_fault_state().ordinal = 4,
                "C-UV-owner" => {
                    let from = m.frontend.reference_fault_state().c_head;
                    m.frontend.reference_fault_state().c_head = 1;
                    m.frontend.reference_fault_state().slots[1] = Some(Context {
                        metadata: Metadata::Ready,
                    });
                    let mut e = context_store::Edge::default();
                    e.writes[2] = Some((1, m.metadata.rows().1[from]));
                    m.metadata.commit(e);
                }
                "admission-range" => {
                    let mut v = input(1);
                    v.bias = 8193;
                    offer = Some(v);
                }
                _ => unreachable!(),
            }
            let owners = m.ordered_state();
            let rows = m.metadata.rows();
            let d_lod = m.d_lod_state();
            let c = m.coordinate_state();
            let banks = m.numerical_banks();
            let coefficient = m.coefficient_snapshot();
            let work = m.work_state();
            let counts = m.counts();
            let old_extra = (
                m.live,
                m.frontend.reference_fault_state().occupied,
                m.frontend.reference_fault_state().reclaimable.clone(),
                m.completion
                    .iter()
                    .map(|v| v.as_ref().map(|v| (v.slot, v.remaining)))
                    .collect::<Vec<_>>(),
                m.stats.cycles,
                m.stats.enabled,
            );
            assert!(drive(&mut m, offer).is_err(), "{fault}");
            assert_eq!(m.ordered_state(), owners, "{fault}");
            assert_eq!(m.metadata.rows(), rows, "{fault}");
            assert_eq!(m.d_lod_state(), d_lod, "{fault}");
            assert_eq!(m.coordinate_state(), c, "{fault}");
            assert_eq!(m.numerical_banks(), banks, "{fault}");
            assert_eq!(m.coefficient_snapshot(), coefficient, "{fault}");
            assert_eq!(m.work_state(), work, "{fault}");
            assert_eq!(m.counts(), counts, "{fault}");
            assert_eq!(
                (
                    m.live,
                    m.frontend.reference_fault_state().occupied,
                    m.frontend.reference_fault_state().reclaimable.clone(),
                    m.completion
                        .iter()
                        .map(|v| v.as_ref().map(|v| (v.slot, v.remaining)))
                        .collect::<Vec<_>>(),
                    m.stats.cycles,
                    m.stats.enabled
                ),
                old_extra,
                "{fault}"
            );
            reached = true;
            break;
        }
        assert!(reached, "fault boundary not reached: {fault}");
    }
}
