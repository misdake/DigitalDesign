//! Live ingress ownership checks with no Program, counted or oracle source.
use super::*;

#[derive(Default)]
struct Memory {
    clocks: u64,
}
impl RefillPort for Memory {
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        self.clocks += 1;
        assert!(self.clocks <= 64, "test wall bound");
        Ok(vec![])
    }
    fn submit_read(&mut self, _: u64, _: usize) -> Result<u64, String> {
        Ok(1)
    }
}
fn machine(mask: u8) -> (Machine, Memory) {
    let mut m = Machine::new(
        vec![
            Slot {
                base_address: 4096,
                max_size_log2: 3,
                has_full_mip: true,
                valid: true
            };
            2
        ],
        Hardware {
            preparation: PreparationMode::BoundStages,
            packet_storage: PacketStorage::Pool64,
            prefetch: false,
            max_cycles: 64,
            ..Default::default()
        },
    )
    .unwrap();
    let mut memory = Memory::default();
    assert!(
        m.step_live(
            &mut memory,
            Some(LiveAdmission {
                quad: 2,
                mask,
                slot: 0
            }),
            Control::default(),
            None,
            vec![],
            vec![]
        )
        .unwrap()
        .accepted
    );
    assert!(m.external_programs.iter().all(Option::is_none));
    assert_eq!(m.external_cursor, [0; 16]);
    (m, memory)
}
fn packet(lane: u8, first: bool, last: bool) -> Group4 {
    Group4 {
        quad_id: 2,
        lane,
        key: TileKey {
            slot: 0,
            n: 3,
            x: 0,
            y: 0,
        },
        top_left_local: [0, 0],
        coefficients: [511, 0, 0, 0],
        first,
        last,
    }
}
fn publish(m: &mut Machine, memory: &mut Memory, g: Group4) -> Result<Step, Error> {
    // The real pipeline reserves before W; do not borrow same-edge credit.
    m.step_live(
        memory,
        None,
        Control::default(),
        None,
        vec![],
        vec![packet::Owner {
            quad: g.quad_id,
            lane: g.lane,
        }],
    )?;
    m.step_live(
        memory,
        None,
        Control::default(),
        Some(g.pack72().unwrap() as i128),
        vec![],
        vec![],
    )
}

#[test]
fn live_completion_follows_actual_last_and_keeps_cache_ownership() {
    let (mut m, mut memory) = machine(0b1010);
    publish(&mut m, &mut memory, packet(1, true, false)).unwrap();
    assert_eq!(m.external_remaining[2], 0b1010);
    assert_eq!(m.external_open, 1 << 2);
    publish(&mut m, &mut memory, packet(1, false, true)).unwrap();
    assert_eq!(m.external_remaining[2], 0b1000);
    publish(&mut m, &mut memory, packet(3, true, true)).unwrap();
    assert_eq!(m.external_remaining[2], 0);
    assert_eq!(m.external_open, 0);
    m.step_live(&mut memory, None, Control::default(), None, vec![2], vec![])
        .unwrap();
    assert_eq!(m.external_live, 0);
    assert!(m.produced[2]);
    assert_eq!(
        m.remaining[2], 0b1010,
        "preparation does not retire cache results"
    );
    assert!(!m.external_ready(2));
    assert!(m.external_programs.iter().all(Option::is_none));
    assert_eq!(m.external_cursor, [0; 16]);
}

#[test]
fn live_empty_mask_completes_without_packet_and_duplicate_end_faults() {
    let (mut m, mut memory) = machine(0);
    m.step_live(&mut memory, None, Control::default(), None, vec![2], vec![])
        .unwrap();
    assert!(m.idle() && m.external_ready(2));
    assert!(m
        .step_live(&mut memory, None, Control::default(), None, vec![2], vec![])
        .is_err());
    assert!(m.faulted());
}

#[test]
fn live_invalid_ownership_faults_without_expected_packet_counts() {
    // No checker predicts how many groups the chosen filter should produce.
    for case in 0..10 {
        let (mut m, mut memory) = machine(0b1010);
        let err = match case {
            0 => m.step_live(&mut memory, None, Control::default(), None, vec![2], vec![]),
            1 => publish(&mut m, &mut memory, packet(3, true, true)),
            2 => publish(&mut m, &mut memory, packet(1, false, true)),
            3 => {
                let mut g = packet(1, true, true);
                g.key.slot = 1;
                publish(&mut m, &mut memory, g)
            }
            4 => {
                let mut g = packet(1, true, true);
                g.quad_id = 3;
                publish(&mut m, &mut memory, g)
            }
            5 => {
                publish(&mut m, &mut memory, packet(1, true, false)).unwrap();
                publish(&mut m, &mut memory, packet(1, true, true))
            }
            6 => {
                publish(&mut m, &mut memory, packet(1, true, true)).unwrap();
                publish(&mut m, &mut memory, packet(1, true, true))
            }
            7 => publish(&mut m, &mut memory, packet(0, true, true)),
            8 => m.step_live(
                &mut memory,
                Some(LiveAdmission {
                    quad: 2,
                    mask: 1,
                    slot: 0,
                }),
                Control::default(),
                None,
                vec![],
                vec![],
            ),
            _ => {
                publish(&mut m, &mut memory, packet(1, true, false)).unwrap();
                m.step_live(&mut memory, None, Control::default(), None, vec![2], vec![])
            }
        };
        assert!(err.is_err(), "case {case}");
        assert!(m.faulted(), "case {case}");
        let clocks = memory.clocks;
        assert!(m
            .step_live(&mut memory, None, Control::default(), None, vec![], vec![])
            .is_err());
        assert_eq!(memory.clocks, clocks, "fault does not pretend to drain MC");
    }
}
