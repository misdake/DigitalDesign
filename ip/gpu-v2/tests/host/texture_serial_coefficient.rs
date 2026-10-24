//! Closed Serial old-owner proof; independent overlapping reference stays ready2.
use gpu_v2::texture::{
    emu::coefficient::{
        self as c,
        compact_stage::{CompactCoefficientEmu, SerialCoefficientEmu},
    },
    emu::compact,
    rtl::compact_coefficient as rtl,
};

fn input(key: u8) -> c::Input {
    let raw = (u64::from(key) * 137) | (255 << 8) | (1 << 16) | (7 << 26);
    let o = compact::expand_result(raw, 8);
    c::Input {
        parents: [255, 256],
        fractions: o.fractions,
        nearest: false,
        metadata: c::Metadata {
            coordinates: o.coordinates,
            levels: [8, 7],
            slot: 0,
            key,
            last_fine: false,
        },
    }
}
#[test]
fn single_row_holds_old_owner_and_consumption_cannot_fund_admission() {
    let mut serial = SerialCoefficientEmu::new(500).unwrap();
    let mut overlap = CompactCoefficientEmu::new(500).unwrap();
    let first = input(3);
    let second = input(61);
    let mut accepted = false;
    let mut overlap_extra = false;
    let mut writes = 0;
    let mut published = false;
    let mut last_enabled = 0;
    for wall in 0..40 {
        let ce = wall != 3 && wall != 4;
        let t = c::Tick {
            ce,
            input: Some(if accepted { second } else { first }),
            output_ready: false,
            work_available: 16,
        };
        let old = serial.snapshot();
        let s = serial.tick(t).unwrap();
        let g = overlap.tick(t).unwrap();
        if !ce {
            assert_eq!(s.snapshot.numeric_words, old.numeric_words);
            assert_eq!(s.snapshot.product_registers, old.product_registers);
            assert_eq!(s.snapshot.enabled, last_enabled);
        }
        last_enabled = s.snapshot.enabled;
        if s.accepted {
            assert!(!accepted);
            accepted = true;
        }
        if g.accepted && wall != 0 {
            overlap_extra = true;
        }
        assert_eq!(s.snapshot.ready_pointers, [0, 0]);
        assert!(s.snapshot.cohorts + s.snapshot.queued <= 1);
        if let Some((row, _)) = s.ready_access.write {
            assert_eq!(row, 0);
            writes += 1;
            assert_eq!(s.snapshot.enabled, 5);
        }
        if s.ready_access.publish {
            published = true;
            assert_eq!(s.snapshot.enabled, 6);
        }
        if let Some(o) = s.output {
            assert_eq!(o.metadata.key, first.metadata.key);
            assert!(!s.accepted);
        }
    }
    assert!(accepted && published && overlap_extra);
    assert_eq!(writes, 1);
    let held = serial.output().unwrap();
    let before = serial.snapshot();
    let s = serial
        .tick(c::Tick {
            input: Some(second),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(s.output, Some(held));
    assert!(s.consumed);
    assert!(!s.accepted && !s.input_ready);
    assert_eq!(s.snapshot.queued, 0);
    assert_eq!(s.snapshot.enabled, before.enabled + 1);
    let mut admitted = false;
    for _ in 0..20 {
        let s = serial
            .tick(c::Tick {
                input: (!admitted).then_some(second),
                ..Default::default()
            })
            .unwrap();
        admitted |= s.accepted;
        if let Some(o) = s.output {
            assert_eq!(o.metadata.key, second.metadata.key);
        }
        if admitted && serial.idle() {
            break;
        }
    }
    assert!(admitted && serial.idle());
    // Structural specialization has one actual row and no selector to an absent row.
    let source = rtl::serial_verilog();
    assert!(source.contains("cohorts<1 && queued==0"));
    assert!(source.contains("assign ready_value=ready_ff0;"));
    assert!(!source.contains("ready_ff1"));
    assert!(rtl::verilog().contains("cohorts<3"));
    assert!(rtl::verilog().contains("ready_ff1"));
}
#[test]
fn closed_profile_checks_input_only_when_old_admission_is_available() {
    let mut s = SerialCoefficientEmu::new(100).unwrap();
    assert!(
        s.tick(c::Tick {
            input: Some(input(0)),
            ..Default::default()
        })
        .unwrap()
        .accepted
    );
    let mut bad = input(1);
    bad.metadata.coordinates[0][1] = 99;
    for _ in 0..9 {
        assert!(
            !s.tick(c::Tick {
                input: Some(bad),
                output_ready: false,
                ..Default::default()
            })
            .unwrap()
            .accepted
        );
    }
    assert!(s.output().is_some());
    assert!(
        !s.tick(c::Tick {
            input: Some(bad),
            ..Default::default()
        })
        .unwrap()
        .accepted
    );
    for _ in 0..2 {
        match s.tick(c::Tick {
            input: Some(bad),
            ..Default::default()
        }) {
            Err(c::Fault::Input) => {
                assert!(s.faulted());
                assert_eq!(s.tick(c::Tick::default()).unwrap_err(), c::Fault::Terminal);
                return;
            }
            Ok(v) => assert!(!v.accepted),
            other => panic!("{other:?}"),
        }
    }
    panic!("bad input was not checked at the next eligible admission");
}
