//! Shared-RNG blast response, including force edges and health error prefixes.
use super::*;
use tankracer_game::blast;
pub(super) fn append(cases: &mut Vec<Value>) {
    for variant in 0..16 {
        for x in [
            0,
            100,
            5119,
            5120,
            7167,
            7168,
            20479,
            20480,
            40960,
            41472,
            i32::MAX,
            i32::MIN,
        ] {
            for mode in [1, 4] {
                for seed in [0, u32::MAX] {
                    let mut state = blast::State {
                        health: if variant == 15 {
                            i32::MIN.wrapping_add(if mode == 1 { 10240 } else { 3200 })
                        } else {
                            1000
                        },
                        health_timer: 27,
                        health_color: 23,
                        protection_timer: 100,
                        angular_enabled: variant & 8 != 0,
                        hit_axes: [77, 88],
                    };
                    let input = blast::Input {
                        position: [x, 0, 0],
                        source: [0; 3],
                        mode,
                        damage: variant & 1 != 0,
                        max_health: if variant == 7 {
                            0
                        } else if variant == 15 {
                            -2
                        } else {
                            40000
                        },
                        record_axes: variant & 2 != 0,
                        mark: variant & 4 != 0,
                    };
                    let mut velocity = [123, i32::MAX - 100, -999];
                    let mut angular = [-7, i32::MIN + 333, 17];
                    let mut marker = 99;
                    let mut random = seed;
                    let mut values = row(&state);
                    values.extend(velocity);
                    values.extend(angular);
                    values.extend([marker as i32, seed as i32]);
                    values.extend(input.position);
                    values.extend(input.source);
                    values.extend([
                        input.mode as i32,
                        i32::from(input.damage),
                        input.max_health,
                        i32::from(input.record_axes),
                        i32::from(input.mark),
                    ]);
                    assert_eq!(values.len(), 26);
                    let mut expected = Vec::new();
                    for _ in 0..3 {
                        let result = blast::apply(
                            blast::Target {
                                state: &mut state,
                                velocity: &mut velocity,
                                angular: &mut angular,
                                marker: &mut marker,
                            },
                            input,
                            &mut random,
                        );
                        expected.push(match result {
                            Ok(false) => 0,
                            Ok(true) => 1,
                            Err(-2) => 2,
                            other => panic!("blast {other:?}"),
                        });
                        expected.extend(row(&state));
                        expected.extend(velocity);
                        expected.extend(angular);
                        expected.extend([marker as i32, random as i32]);
                    }
                    assert_eq!(expected.len(), 48);
                    cases.push(json!({"kind":22,"input":values,"status":0,"expected":expected}));
                }
            }
        }
    }
}
fn row(s: &blast::State) -> Vec<i32> {
    vec![
        s.health,
        i32::from(s.health_timer),
        i32::from(s.health_color),
        s.protection_timer,
        i32::from(s.angular_enabled),
        i32::from(s.hit_axes[0]),
        i32::from(s.hit_axes[1]),
    ]
}
