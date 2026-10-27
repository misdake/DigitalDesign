//! Modulo-four resource ownership is checked by the actual arithmetic program.
use gpu_v2::lighting::{dsp_sharing, rtl, LightingProfile};

#[test]
fn four_cycle_rom_ports_have_no_phase_conflicts_and_keep_operation_dates() {
    for pool in 7..=11 {
        let (options, original) = dsp_sharing::four_cycle(pool).unwrap();
        let (_, shared) = dsp_sharing::four_cycle_shared_roms(pool).unwrap();
        for (before, after) in original.iter().zip(&shared) {
            assert_eq!(before.slots.len(), after.slots.len());
            for (a, b) in before.slots.iter().zip(&after.slots) {
                assert_eq!((a.id, a.issue), (b.id, b.issue));
            }
        }
        let core =
            rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &shared).unwrap();
        assert_eq!((core.specular_ii, core.latency), (4, 34));
        let calendar =
            rtl::physical_calendar_with_schedule_plans(LightingProfile::Fast, options, &shared)
                .unwrap();
        let mut occupied = std::collections::BTreeSet::new();
        for instruction in calendar.iter().filter(|i| i.kind == "NormalizeRead") {
            assert!(occupied.insert((instruction.full, instruction.lane, instruction.issue % 4)));
        }
        assert_eq!(core.source.matches("lighting_norm_q13 bank").count(), 2);
    }
    assert!(dsp_sharing::four_cycle_shared_roms(6).is_err());
    assert!(dsp_sharing::four_cycle_shared_roms(12).is_err());
}
