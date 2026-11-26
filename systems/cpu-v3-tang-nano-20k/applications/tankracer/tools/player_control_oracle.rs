//! Original player input, turret, brake and collision prefix on shared fields.
use super::ints;
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{drive, player_brake, player_collision, player_input, turret, vehicle};

struct State {
    input: player_input::State,
    shared: player_input::Shared,
    countdown: i32,
    turret: turret::State,
    angle: i16,
    brake: player_brake::State,
    max_turn: i32,
    yaw_lock: i32,
    collision: vehicle::CollisionState,
}
impl State {
    fn load(v: &[i32; 32]) -> Self {
        Self {
            input: player_input::State {
                drive: drive::Input {
                    throttle: v[0],
                    brake: v[1],
                    steering: v[2],
                    action_flags: v[3] as u32,
                    alternate: v[4] != 0,
                    alternate_lateral: v[5] as i16,
                    alternate_rotation: v[6] as i16,
                },
                start_held: v[7] as u8,
                fast_start: v[8] as u8,
            },
            countdown: v[9],
            shared: player_input::Shared {
                steering_analog_latch: v[10] != 0,
            },
            turret: turret::State {
                velocity: v[11],
                cooldown: v[12],
                centering: v[13] != 0,
                moving: v[14] != 0,
                center_lock: v[15] as i16,
                pressed: [v[16] != 0, v[17] != 0],
            },
            angle: v[18] as i16,
            brake: player_brake::State {
                saved_turn: v[19] as i16,
                reverse_braking: v[20] != 0,
            },
            max_turn: v[21],
            yaw_lock: v[22],
            collision: vehicle::CollisionState {
                impact_accumulator: v[23],
                collision_size: v[24] as u32,
                ground_delta: [v[25], v[26], v[27]],
                tank_delta: [v[28], v[29], v[30]],
                obstacle_impulse: v[31],
                ..Default::default()
            },
        }
    }
    fn snapshot(&self) -> Vec<u16> {
        let d = self.input.drive;
        let t = &self.turret;
        let c = &self.collision;
        let mut out = Vec::new();
        ints(
            &mut out,
            [
                d.throttle,
                d.brake,
                d.steering,
                d.action_flags as i32,
                i32::from(d.alternate),
                i32::from(d.alternate_lateral),
                i32::from(d.alternate_rotation),
                i32::from(self.input.start_held),
                i32::from(self.input.fast_start),
                self.countdown,
                i32::from(self.shared.steering_analog_latch),
                t.velocity,
                t.cooldown,
                i32::from(t.centering),
                i32::from(t.moving),
                i32::from(t.center_lock),
                i32::from(t.pressed[0]),
                i32::from(t.pressed[1]),
                i32::from(self.angle),
                i32::from(self.brake.saved_turn),
                i32::from(self.brake.reverse_braking),
                self.max_turn,
                self.yaw_lock,
                c.impact_accumulator,
                c.collision_size as i32,
                c.ground_delta[0],
                c.ground_delta[1],
                c.ground_delta[2],
                c.tank_delta[0],
                c.tank_delta[1],
                c.tank_delta[2],
                c.obstacle_impulse,
            ],
        );
        assert_eq!(out.len(), 64);
        out
    }
    fn input(&mut self, c: &[i32; 20], tuning: player_input::Tuning) -> u16 {
        let mut device = Sample {
            keys: c[11],
            axes: [c[12], c[13]],
        };
        let result = self
            .input
            .step(
                &mut self.countdown,
                &mut self.shared,
                player_input::Frame {
                    dt: c[1],
                    driving: c[2] != 0,
                    player: c[3] as i8,
                    speed: c[4],
                    start_stage: c[5],
                    analog: c[6] != 0,
                    analog_longitudinal: c[7] != 0,
                    twin_tracks: c[8] != 0,
                    track_values: [c[9], c[10]],
                },
                tuning,
                &mut device,
            )
            .unwrap();
        u16::from(result.driving) | (u16::from(result.auxiliary_down) << 1)
    }
    fn turret(&mut self, c: &[i32; 20]) {
        self.turret.tick(
            &mut self.angle,
            turret::Keys {
                left: c[11] & 32 != 0,
                right: c[11] & 64 != 0,
            },
            c[1],
        );
    }
    fn brake(&mut self, c: &[i32; 20], restore: bool) {
        let vehicle = player_brake::Vehicle {
            input: &mut self.input.drive,
            max_turn: &mut self.max_turn,
            yaw_lock: &mut self.yaw_lock,
        };
        let reference = player_brake::Reference {
            slide: c[17] as u16,
            rotation: c[18] as i16,
        };
        if restore {
            self.brake.restore(vehicle, c[1], c[15], reference);
        } else {
            self.brake.step(
                vehicle,
                player_brake::Frame {
                    dt: c[1],
                    speed: c[4],
                    mode: c[15],
                    temporary_timer: c[16] as i16,
                    down: c[11] & 16 != 0,
                },
                reference,
            );
        }
    }
}
struct Sample {
    keys: i32,
    axes: [i32; 2],
}
impl player_input::Device for Sample {
    fn down(&mut self, action: player_input::Action) -> Result<bool, i32> {
        let bit = match action {
            player_input::Action::Left => 1,
            player_input::Action::Right => 2,
            player_input::Action::Throttle => 4,
            player_input::Action::Brake => 8,
            player_input::Action::Auxiliary => 16,
        };
        Ok(self.keys & bit != 0)
    }
    fn axis(&mut self, axis: player_input::Axis) -> Result<i32, i32> {
        Ok(self.axes[usize::from(matches!(axis, player_input::Axis::Longitudinal))])
    }
}
fn control(op: i32) -> [i32; 20] {
    [
        op, 40, 1, 0, 300001, 4, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 220, 23, 0,
    ]
}
fn initial() -> [i32; 32] {
    let mut v = [0; 32];
    v[0] = 20000;
    v[1] = 1000;
    v[2] = -1000;
    v[3] = 0x87654321u32 as i32;
    v[5] = 220;
    v[6] = 23;
    v[9] = 24000;
    v[18] = 1000;
    v[19] = 250;
    v[21] = 250;
    v[23] = 120;
    v[24] = 150;
    v[25] = 40000;
    v[29] = -50000;
    v[31] = 70000;
    v
}
pub fn export(out: &Path) {
    let mut cases = Vec::new();
    let mut add = |tag: String, v: [i32; 32], controls: Vec<[i32; 20]>, t: [i32; 6]| {
        let mut s = State::load(&v);
        let input = s.snapshot();
        let mut frames = Vec::new();
        let tuning = player_input::Tuning {
            push_throttle: t[0],
            push_brake: t[1],
            push_steer: t[2],
            return_throttle: t[3] as i16,
            return_brake: t[4] as i16,
            return_steer: t[5],
        };
        for c in &controls {
            let mut result = 0;
            match c[0] {
                0 => result = s.input(c, tuning),
                1 => s.turret(c),
                2 => s.brake(c, false),
                3 => s.brake(c, true),
                4 => player_collision::step(&mut s.collision, c[1], c[14] != 0),
                5 => {
                    result = s.input(c, tuning);
                    if result & 1 != 0 {
                        player_collision::step(&mut s.collision, c[1], c[14] != 0);
                        s.turret.center_lock = c[19] as i16;
                        s.turret(c);
                        // The sampled auxiliary result, rather than another device read.
                        let mut brake = *c;
                        brake[11] &= !16;
                        if result & 2 != 0 {
                            brake[11] |= 16;
                        }
                        s.brake(&brake, false);
                    }
                }
                _ => panic!("operation"),
            }
            let mut frame = Vec::new();
            ints(&mut frame, [i32::from(result)]);
            frame.extend(s.snapshot());
            frames.push(frame);
        }
        cases.push(json!({"tag":tag,"input":input,"tuning":t,"controls":controls,"frames":frames}));
    };
    let tuning = [800, 70, 800, 720, 720, 700];
    for slot in [-128, -1, 1, 127] {
        let mut c = control(0);
        c[3] = slot;
        c[11] = 127;
        let mut analog = c;
        analog[6] = 1;
        analog[12] = 12345;
        let mut countdown = c;
        countdown[2] = 0;
        countdown[5] = 2;
        add(
            format!("slot-{slot}"),
            initial(),
            vec![c, analog, countdown],
            tuning,
        );
    }
    let mut restored = initial();
    restored[4] = 1;
    let mut c = control(3);
    c[15] = 4;
    add("restore-complete".into(), restored, vec![c, c], tuning);
    let mut wrapped_tuning = control(0);
    wrapped_tuning[11] = 31;
    add(
        "signed-tuning".into(),
        initial(),
        vec![wrapped_tuning; 3],
        [-800, i32::MAX, -800, -32768, -1, i32::MIN],
    );
    for keys in 0..32 {
        let mut c = control(0);
        c[11] = keys;
        c[4] = [-1, 0, 1][keys as usize % 3];
        let mut release = c;
        release[11] = 0;
        add(
            format!("keys-{keys}"),
            initial(),
            vec![c, c, release],
            tuning,
        );
    }
    for stage in -1..7 {
        for held in 0..2 {
            let mut c = control(0);
            c[2] = 0;
            c[5] = stage;
            c[11] = held * 4;
            let mut release = c;
            release[11] = 0;
            let mut late = c;
            late[5] = 2;
            add(
                format!("countdown-{stage}-{held}"),
                initial(),
                vec![c, release, late],
                tuning,
            );
        }
    }
    for flags in 0..8 {
        for direction in -1..2 {
            let mut c = control(0);
            c[6] = 1;
            c[7] = flags & 1;
            c[8] = (flags >> 1) & 1;
            c[2] = (flags >> 2) & 1;
            c[9] = direction * 27000;
            c[10] = -16000;
            c[12] = direction * 24000;
            c[13] = direction * 20000;
            c[5] = 2;
            let mut zero = c;
            zero[12] = 0;
            zero[13] = 0;
            let mut override_key = zero;
            override_key[11] = 3;
            add(
                format!("analog-{flags}-{direction}"),
                initial(),
                vec![c, zero, override_key],
                tuning,
            );
        }
    }
    for branch in 0..24 {
        let mut v = initial();
        v[11] = [0, 2000, -2000, i32::MAX][branch % 4];
        v[12] = [0, 40, 41, -1][branch % 4];
        v[13] = i32::from(branch & 4 != 0);
        v[15] = i32::from(branch & 8 != 0);
        v[18] = [-32768, -2000, -1, 0, 1, 32767][branch % 6];
        let mut c = control(1);
        c[11] = [0, 32, 64, 96][branch % 4];
        c[1] = [40, 0, -40, i32::MAX, i32::MIN, 600][branch % 6];
        let mut release = c;
        release[11] = 0;
        add(format!("turret-{branch}"), v, vec![c, c, release], tuning);
    }
    for branch in 0..24 {
        let mut v = initial();
        v[4] = i32::from(branch & 1 != 0);
        v[5] = [-32768, -1, 0, 10, 32000, 220][branch % 6];
        v[6] = [-32768, 23, 600, 0][branch % 4];
        v[19] = -123;
        v[20] = 1;
        v[21] = 310;
        let mut c = control(2);
        c[11] = if branch & 2 != 0 { 16 } else { 0 };
        c[4] = [250000, 250001, -250001, i32::MIN][branch % 4];
        c[15] = [0, 4, 5][branch % 3];
        c[16] = if branch & 4 != 0 { 5 } else { 0 };
        c[17] = if branch & 8 != 0 { 40000 } else { 220 };
        c[18] = [-32768, 23, 32767][branch % 3];
        c[1] = [40, 0, -40, 32767][branch % 4];
        let mut restore = c;
        restore[0] = 3;
        add(format!("brake-{branch}"), v, vec![c, restore, c], tuning);
    }
    for branch in 0..16 {
        let mut v = initial();
        v[23] = [0, 40, 41, -1][branch % 4];
        v[24] = [0, 1300, i32::MIN, -1][branch % 4];
        v[25] = [30000, 30001, i32::MIN, i32::MAX][branch % 4];
        v[26] = v[25];
        v[27] = if branch & 4 != 0 { v[25] } else { 0 };
        v[28] = i32::MIN;
        v[29] = -1;
        v[30] = 200;
        v[31] = [30000, 30001, i32::MAX, i32::MIN][branch % 4];
        let mut c = control(4);
        c[14] = i32::from(branch & 8 == 0);
        c[1] = [40, 0, -40, i32::MIN][branch % 4];
        add(format!("collision-{branch}"), v, vec![c, c, c], tuning);
    }
    for branch in 0..4 {
        let mut controls = Vec::new();
        for step in 0..6 {
            let mut c = control(5);
            c[2] = i32::from(step >= 2);
            c[5] = step;
            c[11] = [4, 4, 100, 116, 0, 8][step as usize];
            c[4] = [-1, 0, 300001, 500000][branch];
            c[16] = if step == 4 { 5 } else { 0 };
            controls.push(c);
        }
        add(format!("prefix-{branch}"), initial(), controls, tuning);
    }
    let mut seed = 0x74616e6bu32;
    for branch in 0..32 {
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed as i32
        };
        let mut v = [0; 32];
        for x in &mut v {
            *x = next();
        }
        let mut controls = Vec::new();
        for op in [0, 1, 2, 4, 5] {
            let mut c = control(op);
            c[1] = next();
            c[4] = next();
            c[6] = branch & 1;
            c[7] = (branch >> 1) & 1;
            c[11] = next() & 127;
            c[12] = next();
            c[13] = next();
            c[15] = branch % 6;
            c[16] = next() as i16 as i32;
            c[17] = next() as u16 as i32;
            c[18] = next() as i16 as i32;
            controls.push(c);
        }
        add(format!("wrapped-{branch}"), v, controls, tuning);
    }
    fs::write(out.join("player-control.json"),serde_json::to_vec(&json!({"cases":cases,
        "scope":"Original sampled input/countdown, turret, auxiliary brake/restoration and collision indication; isolated branches and ordered shared-field short prefixes. Device sampling is external; race route/fire and parent Tank are separate boundaries."})).unwrap()).unwrap();
}
