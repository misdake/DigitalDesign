//! All-stage rules, input and independent animation/veil clocks.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;

mod boot_resources;
use boot_resources::*;
mod bloxorz_core;
use bloxorz_core::*;
mod gamepad;
use gamepad::*;
mod orientation;
use orientation::*;
pub const PLAY: u16 = 0;
pub const ROLL: u16 = 1;
pub const ENTRY_OPEN: u16 = 2;
pub const ENTRY_RISE: u16 = 3;
pub const GOAL_OPEN: u16 = 4;
pub const GOAL_DROP: u16 = 5;
pub const GOAL_LAND: u16 = 6;
pub const GOAL_CLOSE: u16 = 7;
pub const FALL: u16 = 8;
pub const FADE_OUT: u16 = 9;
pub const HOLD: u16 = 10;
pub const FADE_IN: u16 = 11;
pub const TIP: u16 = 12;
pub const SPLIT_POP: u16 = 13;
pub const CRACK: u16 = 14;
#[derive(Copy, Clone)]
pub struct FallState {
    pub position: Vec3,
    pub velocity: Vec3,
    pub scale: Vec3,
    pub angle: fix32,
    pub spin: fix32,
    pub axis: u16,
}

#[repr(C)]
pub struct MotionState {
    pub from_x: u16,
    pub from_z: u16,
    pub from_orientation: u16,
    pub direction: u16,
    pub elapsed: u16,
    pub moving: u16,
    pub level: u16,
    pub previous_mask: u16,
    pub fall_kind: u16,
    pub release_threshold: u16,
    pub first_support: u16,
    pub from_split: u16,
    pub from_active: u16,
    pub from_other_x: u16,
    pub from_other_z: u16,
    pub landing_x: u16,
    pub landing_z: u16,
}
#[repr(C)]
pub struct AnimationState {
    pub phase: u16,
    pub elapsed: u16,
    pub destination: u16,
    pub held_lock: u16,
    pub captured_frames: u16,
    pub fall_notified: u16,
    pub goal_x: u16,
    pub goal_z: u16,
    pub direction_stack: u16,
}
#[repr(C)]
pub struct TransitionState {
    pub phase: u16,
    pub elapsed: u16,
}
#[repr(C)]
pub struct UndoState {
    pub cursor: u16,
    pub count: u16,
}
#[repr(C)]
pub struct RuleState {
    pub x: u16,
    pub z: u16,
    pub orientation: u16,
    pub bridge_mask: u16,
    pub status: u16,
    pub moves: u16,
    pub other_x: u16,
    pub other_z: u16,
    pub active_cube: u16,
    pub split: u16,
}
#[repr(C)]
pub struct LevelState {
    pub width: u16,
    pub height: u16,
    pub start_x: u16,
    pub start_z: u16,
    pub start_orientation: u16,
    pub initial_mask: u16,
    pub cell_count: u16,
    pub schema: u16,
    pub cells: Buf<u16, 300>,
}
pub struct SwitchEffects {
    pub previous_mask: u16,
    pub ages: Buf<u16, 9>,
}
pub struct BloxorzSession {
    pub generation: u16,
    pub level: LevelState,
    pub rules: RuleState,
    // Packed rule-engine snapshot and 256-entry undo ring stay at the format boundary.
    history_snapshot: Buf<u16, 6>,
    history_state: Buf<u16, 2>,
    undo: Buf<u16, 512>,
    pub undo_state: UndoState,
    pub motion: MotionState,
    pub fracture_cell: u16,
    pub animation: AnimationState,
    pub switches: SwitchEffects,
    pub transition: TransitionState,
    pub fall: FallState,
    pub rotations: BlockOrientations,
}
// One persistent game instance. Methods borrow its storage; no whole-state copies.
pub static GAME_SESSION: BloxorzSession = BloxorzSession {
    generation: 0,
    level: LevelState {
        width: 0,
        height: 0,
        start_x: 0,
        start_z: 0,
        start_orientation: 0,
        initial_mask: 0,
        cell_count: 0,
        schema: 0,
        cells: Buf::new([0; 300]),
    },
    rules: RuleState {
        x: 0,
        z: 0,
        orientation: 0,
        bridge_mask: 0,
        status: 0,
        moves: 0,
        other_x: 0,
        other_z: 0,
        active_cube: 0,
        split: 0,
    },
    history_snapshot: Buf::new([0; 6]),
    history_state: Buf::new([0; 2]),
    undo: Buf::new([0; 512]),
    undo_state: UndoState {
        cursor: 0,
        count: 0,
    },
    motion: MotionState {
        from_x: 0,
        from_z: 0,
        from_orientation: 0,
        direction: 0,
        elapsed: 0,
        moving: 0,
        level: 0,
        previous_mask: 0,
        fall_kind: 0,
        release_threshold: 0,
        first_support: 0,
        from_split: 0,
        from_active: 0,
        from_other_x: 0,
        from_other_z: 0,
        landing_x: 0,
        landing_z: 0,
    },
    fracture_cell: 65535,
    animation: AnimationState {
        phase: 0,
        elapsed: 0,
        destination: 0,
        held_lock: 0,
        captured_frames: 0,
        fall_notified: 0,
        goal_x: 0,
        goal_z: 0,
        direction_stack: 0,
    },
    switches: SwitchEffects {
        previous_mask: 0,
        ages: Buf::new([260; 9]),
    },
    transition: TransitionState {
        phase: 0,
        elapsed: 0,
    },
    fall: FallState {
        position: Vec3::zero(),
        velocity: Vec3::zero(),
        scale: Vec3::zero(),
        angle: fix32::zero(),
        spin: fix32::zero(),
        axis: 0,
    },
    rotations: BlockOrientations {
        actors: Buf::new([8; 2]),
        start: 8,
        history: 0,
    },
};

fn ratio(t: u16, duration: u16) -> fix32 {
    // Exact power-of-two normalization keeps the reciprocal in a useful range.
    fix32::from_words(t << 8, t >> 8) * frcp(fix32::from_words(duration << 8, duration >> 8))
}

pub fn fraction(t: u16, duration: u16) -> fix32 {
    if t >= duration {
        fix32::from_int(1)
    } else {
        ratio(t, duration)
    }
}

fn direction_button(direction: u16) -> u16 {
    if direction == 0 {
        32
    } else {
        if direction == 1 {
            128
        } else {
            if direction == 2 {
                64
            } else {
                16
            }
        }
    }
}

impl BloxorzSession {
    pub fn end(&mut self) {
        self.rotations.end();
    }

    // These two views are the rules engine's stable packed word ABI. Only that
    // boundary and fixture setup use raw arrays; rendering uses named state fields.
    pub fn level_words(&mut self) -> Array<u16> {
        unsafe { addr_of(&self.level.width).as_u16_array() }
    }
    pub fn rule_words(&mut self) -> Array<u16> {
        unsafe { addr_of(&self.rules.x).as_u16_array() }
    }

    pub fn level_field(&self, index: u16, field: u16) -> u16 {
        blox_field(
            unsafe { addr_of(&self.level.width).as_u16_array() },
            index + 1,
            field,
        )
    }

    pub fn reset_level(&mut self, which: u16) {
        self.rotations.reset();
        self.generation += 1;
        self.fracture_cell = 65535;
        self.motion.from_split = 0;
        blox_fixture_level(which, self.level_words());
        blox_reset(
            self.level_words(),
            self.rule_words(),
            self.history_state.as_array(),
        );
        self.undo_state.cursor = 0;
        self.undo_state.count = 0;
        self.switches.previous_mask = self.rules.bridge_mask;
        self.switches.ages[0u16] = 260;
        let mut channel: u16 = 1;
        while channel <= 9 {
            self.switches.ages[(channel) - 1] = 260;
            channel += 1;
        }
        self.motion.elapsed = 220;
        self.motion.moving = 0;
        self.motion.level = which;
        self.animation.fall_notified = 0;
        self.animation.goal_x = 65535;
        self.animation.goal_z = 65535;
        let mut i: u16 = 0;
        while i < self.level.cell_count {
            if self.level_field(i, 0) == 6 {
                self.animation.goal_x = i % self.level.width;
                self.animation.goal_z = div_u16(i, self.level.width);
            }
            i += 1;
        }
    }

    pub fn begin(&mut self) {
        self.rotations.begin();
        self.reset_level(BLOXORZ_INITIAL_LEVEL);
        self.animation.phase = ENTRY_OPEN;
        if BLOXORZ_INITIAL_PLAY != 0 {
            self.animation.phase = PLAY;
        }
        self.animation.elapsed = 0;
        self.animation.held_lock = 0;
        self.animation.captured_frames = 0;
        self.animation.direction_stack = 0;
        self.transition.phase = 0;
        self.transition.elapsed = 0;
    }

    pub fn fading(&self) -> bool {
        self.transition.phase != 0
    }

    pub fn visual_mode(&self) -> u16 {
        self.animation.phase
    }

    pub fn visual_time(&self) -> u16 {
        self.animation.elapsed
    }

    pub fn visual_mask(&self) -> u16 {
        if self.motion.elapsed < 154 {
            self.motion.previous_mask
        } else {
            self.rules.bridge_mask
        }
    }

    pub fn begin_fade(&mut self, destination: u16) {
        if self.fading() {
            return;
        }
        self.animation.destination = destination;
        self.animation.held_lock = 1;
        self.animation.direction_stack = 0;
        self.transition.phase = FADE_OUT;
        self.transition.elapsed = 0;
    }

    pub fn tint(&self) -> u16 {
        if self.transition.phase == FADE_OUT {
            return 255 - div_u16(self.transition.elapsed * 51, 64);
        }
        if self.transition.phase == HOLD {
            return 0;
        }
        if self.transition.phase == FADE_IN {
            return div_u16(self.transition.elapsed * 51, 76);
        }
        255
    }

    pub fn captured(&mut self) {
        self.animation.captured_frames += 1;
    }

    // Called only after consumer WAIT_ALL and actual Display retirement.
    pub fn presented(&mut self) {
        if self.transition.phase == FADE_OUT && self.transition.elapsed == 320 {
            self.reset_level(self.animation.destination);
            self.animation.phase = ENTRY_OPEN;
            self.animation.elapsed = 0;
            self.transition.phase = HOLD;
            self.transition.elapsed = 0;
        } else if self.transition.phase == FADE_IN && self.transition.elapsed == 380 {
            self.transition.phase = 0;
            self.transition.elapsed = 0;
            self.animation.phase = ENTRY_OPEN;
            self.animation.elapsed = 0;
            self.animation.direction_stack = 0;
            gamepad_discard_edges();
            let input = gamepad_take_frame();
            self.animation.held_lock = if input.buttons & 240 != 0 { 1u16 } else { 0u16 };
        }
    }

    fn presentation_support(&self, x: i16, z: i16, mask: u16) -> bool {
        if x < 0 || z < 0 || x as u16 >= self.level.width || z as u16 >= self.level.height {
            return false;
        }
        let at = (z as u16) * self.level.width + x as u16;
        let kind = self.level_field(at, 0);
        kind != 0 && !(kind == 5 && mask & self.level_field(at, 1) == 0)
    }

    pub fn try_move(&mut self, direction: u16) -> bool {
        if self.fading() || self.animation.phase != PLAY || (direction >= 4 && direction != 6) {
            return false;
        }
        self.motion.from_split = self.rules.split;
        self.motion.from_active = self.rules.active_cube;
        self.motion.from_other_x = self.rules.other_x;
        self.motion.from_other_z = self.rules.other_z;
        self.motion.from_x = self.rules.x;
        self.motion.from_z = self.rules.z;
        self.motion.from_orientation = self.rules.orientation;
        self.motion.direction = direction;
        self.motion.previous_mask = self.rules.bridge_mask;
        self.history_state[0u16] = 0;
        self.history_state[1u16] = 0;
        if !blox_dispatch(
            self.level_words(),
            self.rule_words(),
            self.history_snapshot.as_array(),
            self.history_state.as_array(),
            direction,
        ) {
            return false;
        }
        let mode = if self.history_snapshot[5u16] & 32768 != 0 {
            3 + ((self.history_snapshot[5u16] >> 14) & 1)
        } else {
            self.history_snapshot[2u16] & 3
        };
        let mask = (self.history_snapshot[2u16] >> 2) & 511;
        self.rotations.save(self.undo_state.cursor);
        let at = self.undo_state.cursor * 2;
        self.undo[at] = (self.history_snapshot[0u16] & 31)
            | ((self.history_snapshot[1u16] & 31) << 5)
            | (mode << 10)
            | ((mask & 7) << 13);
        self.undo[at + 1] = (mask >> 3)
            | ((self.history_snapshot[4u16] & 31) << 6)
            | ((((self.history_snapshot[4u16] >> 15) | (self.history_snapshot[5u16] << 1)) & 31)
                << 11);
        self.undo_state.cursor = (self.undo_state.cursor + 1) & 255;
        if self.undo_state.count < 256 {
            self.undo_state.count += 1;
        }
        if direction == 6 {
            self.motion.elapsed = 220;
            self.motion.moving = 0;
            if self.rules.status == 2 {
                // A soft-switch swap can remove the other cube's bridge. Preserve
                // the supported cube and drop the actual unsupported one at rest.
                let first = self.presentation_support(
                    self.rules.x as i16,
                    self.rules.z as i16,
                    self.rules.bridge_mask,
                );
                let falling = if first { 1u16 } else { 0u16 };
                let mut pose = self.block_transform();
                if falling != self.rules.active_cube {
                    pose = self.idle_cube_transform();
                }
                self.motion.from_active = falling;
                self.rotations.start(falling);
                self.fall = FallState {
                    position: pose.position,
                    velocity: Vec3::zero(),
                    scale: pose.scale,
                    angle: fix32::zero(),
                    spin: fix32::zero(),
                    axis: 0,
                };
                self.animation.phase = FALL;
                self.animation.elapsed = 0;
                self.animation.fall_notified = 0;
                self.animation.held_lock = 1;
                self.animation.direction_stack = 0;
            }
            return true;
        }
        self.rotations.start(if self.motion.from_split != 0 {
            self.motion.from_active
        } else {
            0u16
        });
        self.animation.phase = ROLL;
        self.animation.elapsed = 0;
        self.animation.fall_notified = 0;
        self.motion.elapsed = 0;
        self.motion.moving = 1;
        self.motion.fall_kind = 0;
        self.motion.release_threshold = 79;
        if self.motion.from_orientation != 0 {
            self.motion.release_threshold = 124;
            if (self.motion.from_orientation == 1 && direction < 2)
                || (self.motion.from_orientation == 2 && direction >= 2)
            {
                self.motion.release_threshold = 169;
            }
        }
        if self.motion.from_split != 0 {
            self.motion.release_threshold = 124;
        }
        if self.rules.status == 2 && self.rules.split == 0 && self.rules.orientation == 0 {
            let x = self.rules.x;
            let z = self.rules.z;
            if x < self.level.width
                && z < self.level.height
                && self.level_field(z * self.level.width + x, 0) == 2
            {
                self.motion.fall_kind = 2;
            }
        }
        if self.motion.from_split == 0 && self.rules.split != 0 {
            self.motion.fall_kind = 3;
        }
        if self.rules.status == 2 && self.rules.orientation != 0 && self.motion.from_split == 0 {
            let x = self.rules.x as i16;
            let z = self.rules.z as i16;
            let first = self.presentation_support(x, z, self.rules.bridge_mask);
            let second = self.presentation_support(
                x + if self.rules.orientation == 1 {
                    1i16
                } else {
                    0i16
                },
                z + if self.rules.orientation == 2 {
                    1i16
                } else {
                    0i16
                },
                self.rules.bridge_mask,
            );
            if (first && !second) || (!first && second) {
                self.motion.fall_kind = 1;
                self.motion.first_support = if first { 1u16 } else { 0u16 };
            }
        }
        if self.rules.status != 0 {
            self.animation.held_lock = 1;
            self.animation.direction_stack = 0;
        }
        true
    }

    fn undo(&mut self) {
        self.animation.held_lock = 1;
        self.animation.direction_stack = 0;
        if self.undo_state.count == 0 {
            return;
        }
        self.undo_state.cursor = (self.undo_state.cursor + 255) & 255;
        self.undo_state.count -= 1;
        self.rotations.undo(self.undo_state.cursor);
        let at = self.undo_state.cursor * 2;
        let first = self.undo[at];
        let second = self.undo[at + 1];
        let mode = (first >> 10) & 7;
        self.rules.x = first & 31;
        self.rules.z = (first >> 5) & 31;
        self.rules.orientation = if mode < 3 { mode } else { 0u16 };
        self.rules.bridge_mask = (first >> 13) | ((second & 63) << 3);
        self.rules.status = 0;
        self.rules.moves -= 1;
        self.rules.other_x = (second >> 6) & 31;
        self.rules.other_z = (second >> 11) & 31;
        self.rules.active_cube = if mode >= 3 { mode - 3 } else { 0u16 };
        self.rules.split = if mode >= 3 { 1u16 } else { 0u16 };
        self.fracture_cell = 65535;
        self.motion.from_split = 0;
        self.animation.phase = PLAY;
        self.animation.elapsed = 0;
        self.animation.fall_notified = 0;
        self.motion.elapsed = 220;
        self.motion.moving = 0;
    }

    fn held_update(&mut self, pressed: u16, held: u16) {
        if self.fading() {
            self.animation.held_lock = 1;
            self.animation.direction_stack = 0;
            return;
        }
        if self.animation.held_lock != 0 {
            if held & 240 == 0 {
                self.animation.held_lock = 0;
            } else {
                return;
            }
        }
        // Remove released keys and old copies of any re-pressed key before pushing.
        // A release/re-press captured within one frame must not evict another held key.
        let mut stack = self.animation.direction_stack;
        let mut compact: u16 = 0;
        let mut shift: u16 = 0;
        let mut seen: u16 = 0;
        while stack != 0 {
            let code = stack & 15;
            let bit = direction_button(code - 1);
            if held & bit != 0 && pressed & bit == 0 && seen & bit == 0 {
                compact |= code << shift;
                shift += 4;
                seen |= bit;
            }
            stack >>= 4;
        }
        self.animation.direction_stack = compact;
        if pressed & 240 != 0 {
            let mut i: u16 = 0;
            while i < 4 {
                let bit = direction_button(i);
                if pressed & held & bit != 0 {
                    self.animation.direction_stack =
                        (self.animation.direction_stack << 4) | (i + 1);
                }
                i += 1;
            }
        }
    }

    pub fn tick(&mut self, pressed: u16, held: u16, elapsed: u16) {
        let dt = if elapsed > 50 { 50u16 } else { elapsed };
        self.held_update(pressed, held);
        if !self.fading() {
            if pressed & 1024 != 0 {
                self.begin_fade((self.motion.level + BLOX_LEVEL_COUNT - 1) % BLOX_LEVEL_COUNT);
            } else if pressed & (4096 | 2048) != 0 {
                self.begin_fade((self.motion.level + 1) % BLOX_LEVEL_COUNT);
            } else if pressed & 8 != 0 {
                self.begin_fade(self.motion.level);
            } else if pressed & 32768 != 0 {
                self.undo();
            } else if pressed & 16384 != 0 {
                self.try_move(6);
            }
        }
        let phase = self.transition.phase;
        if phase == 0 || phase == FADE_OUT {
            self.animation_tick(dt);
        }
        if self.transition.phase == FADE_OUT {
            self.transition.elapsed += dt;
            if self.transition.elapsed > 320 {
                self.transition.elapsed = 320;
            }
        } else if self.transition.phase == HOLD {
            self.transition.elapsed += dt;
            if self.transition.elapsed >= 80 {
                self.transition.phase = FADE_IN;
                self.transition.elapsed = 0;
            }
        } else if self.transition.phase == FADE_IN {
            self.transition.elapsed += dt;
            if self.transition.elapsed > 380 {
                self.transition.elapsed = 380;
            }
        }
        self.switch_tick(dt);
        if !self.fading()
            && self.animation.phase == PLAY
            && self.animation.held_lock == 0
            && self.animation.direction_stack != 0
        {
            self.try_move((self.animation.direction_stack & 15) - 1);
        }
    }

    pub fn switch_age(&self, channel: u16) -> u16 {
        self.switches.ages[channel]
    }

    pub fn switch_fraction(&self, channel: u16) -> fix32 {
        fraction(self.switches.ages[channel], 260)
    }

    fn switch_tick(&mut self, dt: u16) {
        let mask = self.visual_mask();
        let old = self.switches.previous_mask;
        let mut channel: u16 = 0;
        while channel < 9 {
            let bit = 1 << channel;
            if mask & bit != 0 {
                if old & bit == 0 {
                    self.switches.ages[channel] = 0;
                }
                self.switches.ages[channel] += dt;
                if self.switches.ages[channel] > 260 {
                    self.switches.ages[channel] = 260;
                }
            } else {
                self.switches.ages[channel] = 260;
            }
            channel += 1;
        }
        self.switches.previous_mask = mask;
    }

    fn animation_tick(&mut self, dt: u16) {
        let mode = self.animation.phase;
        if mode == PLAY {
            return;
        }
        if mode != ROLL && self.motion.elapsed < 154 {
            self.motion.elapsed += dt;
            if self.motion.elapsed > 154 {
                self.motion.elapsed = 154;
            }
        }
        self.animation.elapsed += dt;
        if mode == ROLL {
            if self.animation.elapsed > 220 {
                self.animation.elapsed = 220;
            }
            self.motion.elapsed = self.animation.elapsed;
            if self.rules.status == 2
                && self.motion.fall_kind == 0
                && self.animation.elapsed >= self.motion.release_threshold
            {
                self.release_pivot();
            } else if self.animation.elapsed == 220 {
                self.rotations.finish(
                    self.motion.direction,
                    self.motion.from_split != 0,
                    self.rules.split != 0,
                    self.motion.from_active,
                );
                if self.motion.from_split == 0 && self.rules.split != 0 {
                    let landing = self.block_transform();
                    let half = fix!(0.5);
                    self.motion.landing_x = (landing.position.x
                        + fix32::from_int(self.level.width as i16) * half
                        - fix32::from_words(16384, 0))
                    .to_int() as u16;
                    self.motion.landing_z = (landing.position.z
                        + fix32::from_int(self.level.height as i16) * half
                        - fix32::from_words(16384, 0))
                    .to_int() as u16;
                    self.animation.phase = SPLIT_POP;
                } else if self.rules.status == 1 {
                    self.animation.phase = GOAL_OPEN;
                } else if self.rules.status == 2 {
                    if self.motion.fall_kind == 2 {
                        self.animation.phase = CRACK;
                        self.fracture_cell = self.rules.z * self.level.width + self.rules.x;
                    } else {
                        self.animation.phase = TIP;
                    }
                } else {
                    self.animation.phase = PLAY;
                }
                self.motion.moving = 0;
                self.animation.elapsed = 0;
            }
        } else if mode == SPLIT_POP && self.animation.elapsed >= 280 {
            self.animation.phase = PLAY;
            self.animation.elapsed = 0;
        } else if mode == CRACK && self.animation.elapsed >= 140 {
            let pose = self.block_transform();
            self.fall = FallState {
                position: pose.position,
                velocity: Vec3::zero(),
                scale: pose.scale,
                angle: fix32::zero(),
                spin: fix32::zero(),
                axis: 0,
            };
            self.animation.phase = FALL;
            self.animation.elapsed = 0;
        } else if mode == TIP && self.animation.elapsed >= 360 {
            self.animation.elapsed = 360;
            self.release_pivot();
        } else if mode == ENTRY_OPEN && self.animation.elapsed >= 200 {
            self.animation.phase = ENTRY_RISE;
            self.animation.elapsed = 0;
        } else if mode == ENTRY_RISE && self.animation.elapsed >= 380 {
            self.animation.phase = PLAY;
            self.animation.elapsed = 0;
        } else if mode == GOAL_OPEN && self.animation.elapsed >= 200 {
            self.animation.phase = GOAL_DROP;
            self.animation.elapsed = 0;
        } else if mode == GOAL_DROP && self.animation.elapsed >= 420 {
            self.animation.phase = GOAL_LAND;
            self.animation.elapsed = 0;
        } else if mode == GOAL_LAND && self.animation.elapsed >= 80 {
            self.animation.phase = GOAL_CLOSE;
            self.animation.elapsed = 0;
        } else if mode == GOAL_CLOSE && self.animation.elapsed >= 200 {
            self.animation.elapsed = 200;
            if !self.fading() {
                self.begin_fade((self.motion.level + 1) % BLOX_LEVEL_COUNT);
            }
        } else if mode == FALL && self.animation.elapsed >= 600 && self.animation.fall_notified == 0
        {
            self.animation.fall_notified = 1;
            self.begin_fade(self.motion.level);
        }
    }

    pub fn block_visible(&self) -> bool {
        self.animation.phase != ENTRY_OPEN
            && !(self.animation.phase == GOAL_CLOSE && self.animation.elapsed >= 200)
    }

    fn release_pivot(&mut self) {
        let pose = self.block_transform();
        let half = fix!(0.5);
        let tipping = self.animation.phase == TIP;
        let axis = if tipping {
            if self.rules.orientation == 1 {
                1u16
            } else {
                0u16
            }
        } else {
            if self.motion.direction < 2 {
                1u16
            } else {
                0u16
            }
        };
        let mut spin = fix32::from_words(9174, 7);
        if tipping {
            spin = fix32::from_words(47621, 8);
            if (axis == 1 && self.motion.first_support != 0)
                || (axis == 0 && self.motion.first_support == 0)
            {
                spin = -spin;
            }
        } else if self.motion.direction == 0 || self.motion.direction == 3 {
            spin = -spin;
        }
        let mut pivot = fix32::zero();
        if axis == 1 {
            if tipping {
                pivot = fix32::from_int(self.rules.x as i16) + fix32::from_int(1);
            } else {
                pivot = fix32::from_int(self.motion_x() as i16);
                if self.motion.direction == 0 {
                    pivot = pivot
                        + if self.motion.from_orientation == 1 {
                            fix32::from_int(2)
                        } else {
                            fix32::from_int(1)
                        };
                }
            }
            pivot = pivot - fix32::from_int(self.level.width as i16) * half;
        } else {
            if tipping {
                pivot = fix32::from_int(self.rules.z as i16) + fix32::from_int(1);
            } else {
                pivot = fix32::from_int(self.motion_z() as i16);
                if self.motion.direction == 2 {
                    pivot = pivot
                        + if self.motion.from_orientation == 2 {
                            fix32::from_int(2)
                        } else {
                            fix32::from_int(1)
                        };
                }
            }
            pivot = pivot - fix32::from_int(self.level.height as i16) * half;
        }
        let mut velocity = Vec3::zero();
        if axis == 1 {
            velocity = Vec3::new(
                -spin * pose.position.y,
                spin * (pose.position.x - pivot),
                fix32::zero(),
            );
        } else {
            velocity = Vec3::new(
                fix32::zero(),
                -spin * (pose.position.z - pivot),
                spin * pose.position.y,
            );
        }
        self.fall = FallState {
            position: pose.position,
            velocity: velocity,
            scale: pose.scale,
            angle: if axis == 1 {
                pose.angles.z
            } else {
                pose.angles.x
            },
            spin: spin,
            axis: axis,
        };
        self.motion.moving = 0;
        self.animation.phase = FALL;
        self.animation.elapsed = 0;
    }

    fn motion_x(&self) -> u16 {
        if self.motion.from_split != 0 && self.motion.from_active != 0 {
            self.motion.from_other_x
        } else {
            self.motion.from_x
        }
    }

    fn motion_z(&self) -> u16 {
        if self.motion.from_split != 0 && self.motion.from_active != 0 {
            self.motion.from_other_z
        } else {
            self.motion.from_z
        }
    }

    pub fn split_visual(&self) -> bool {
        if self.animation.phase == SPLIT_POP {
            return self.animation.elapsed >= 70;
        }
        if self.animation.phase == ROLL {
            return self.motion.from_split != 0;
        }
        if self.animation.phase == FALL && self.motion.from_split != 0 {
            return true;
        }
        self.rules.split != 0
    }

    fn split_pop_scale(&self) -> fix32 {
        if self.animation.phase != SPLIT_POP {
            return fix32::from_int(1);
        }
        let pop = (fraction(self.animation.elapsed, 280) - fix32::from_words(13107, 0))
            * fix32::from_words(16384, 1);
        let remain = pop - fix32::from_int(1);
        let back = fix32::from_int(1)
            + fix32::from_words(45875, 2) * remain * remain * remain
            + fix32::from_words(45875, 1) * remain * remain;
        if back < fix32::from_words(655, 0) {
            fix32::from_words(655, 0)
        } else {
            back
        }
    }

    pub fn idle_cube_transform(&self) -> Transform {
        let old = (self.animation.phase == ROLL || self.animation.phase == FALL)
            && self.motion.from_split != 0;
        let active = if old {
            self.motion.from_active
        } else {
            self.rules.active_cube
        };
        let mut x = if active == 0 {
            self.rules.other_x
        } else {
            self.rules.x
        };
        let mut z = if active == 0 {
            self.rules.other_z
        } else {
            self.rules.z
        };
        if old {
            x = if active == 0 {
                self.motion.from_other_x
            } else {
                self.motion.from_x
            };
            z = if active == 0 {
                self.motion.from_other_z
            } else {
                self.motion.from_z
            };
        }
        let half = fix!(0.5);
        let scale = self.split_pop_scale();
        Transform::new(
            Vec3::new(
                fix32::from_int(x as i16) - fix32::from_int(self.level.width as i16) * half + half,
                half,
                fix32::from_int(z as i16) - fix32::from_int(self.level.height as i16) * half + half,
            ),
            Vec3::zero(),
            Vec3::new(scale, scale, scale),
        )
    }

    pub fn idle_cube_pose(&self, model: &mut Model) {
        let old = (self.animation.phase == ROLL || self.animation.phase == FALL)
            && self.motion.from_split != 0;
        let active = if old {
            self.motion.from_active
        } else {
            self.rules.active_cube
        };
        rotation_pose(
            model,
            self.idle_cube_transform(),
            self.rotations.current(1 - active),
        );
    }

    pub fn fragile_dropped(&self, index: u16) -> bool {
        self.fracture_cell == index
            && (self.animation.phase == CRACK || self.animation.phase == FALL)
    }

    pub fn fragile_height(&self, index: u16) -> fix32 {
        if self.fracture_cell != index {
            return fix32::zero();
        }
        if self.animation.phase == CRACK {
            let f = fraction(self.animation.elapsed, 140);
            let sc = fsincos(f * fix32::from_int(60));
            return sc.x() * fix32::from_words(786, 0) * (fix32::from_int(1) - f);
        }
        if self.animation.phase == FALL {
            let f = ratio(self.animation.elapsed, 600);
            return -fix32::from_int(16) * f * f;
        }
        fix32::zero()
    }

    pub fn block_transform(&self) -> Transform {
        let mode = self.animation.phase;
        let t = self.animation.elapsed;
        let zero = fix32::zero();
        let one = fix32::from_int(1);
        let two = fix32::from_int(2);
        let half = fix!(0.5);
        if mode == FALL {
            let state = self.fall;
            let seconds = ratio(t, 1000);
            let f = ratio(t, 600);
            let angle = state.angle + state.spin * seconds;
            return Transform::new(
                Vec3::new(
                    state.position.x + state.velocity.x * seconds,
                    state.position.y + state.velocity.y * seconds - fix32::from_int(16) * f * f,
                    state.position.z + state.velocity.z * seconds,
                ),
                Vec3::new(
                    if state.axis == 0 { angle } else { zero },
                    zero,
                    if state.axis == 1 { angle } else { zero },
                ),
                state.scale,
            );
        }
        if mode == SPLIT_POP && t < 70 {
            let size = one - fraction(t, 70);
            return Transform::new(
                Vec3::new(
                    fix32::from_int(self.motion.landing_x as i16)
                        - fix32::from_int(self.level.width as i16) * half
                        + half,
                    size,
                    fix32::from_int(self.motion.landing_z as i16)
                        - fix32::from_int(self.level.height as i16) * half
                        + half,
                ),
                Vec3::zero(),
                Vec3::new(one, two * size, one),
            );
        }
        let moving = self.motion.moving != 0;
        let split = self.split_visual();
        let orient = if moving {
            self.motion.from_orientation
        } else {
            self.rules.orientation
        };
        let x = if moving {
            self.motion_x()
        } else {
            if split && self.rules.active_cube != 0 {
                self.rules.other_x
            } else {
                self.rules.x
            }
        };
        let z = if moving {
            self.motion_z()
        } else {
            if split && self.rules.active_cube != 0 {
                self.rules.other_z
            } else {
                self.rules.z
            }
        };
        let sx = if !split && orient == 1 { two } else { one };
        let sy = if !split && orient == 0 { two } else { one };
        let sz = if !split && orient == 2 { two } else { one };
        let mut cx = sx * half;
        let mut cy = sy * half;
        let mut cz = sz * half;
        let mut ax = zero;
        let mut az = zero;
        let mut angle = zero;
        let mut axis: u16 = 0;
        let mut pivot = zero;
        if moving {
            angle = fraction(self.motion.elapsed, 220) * fix32::from_words(37408, 1);
            if self.motion.direction == 0 || self.motion.direction == 3 {
                angle = -angle;
            }
            if self.motion.direction < 2 {
                axis = 1;
                pivot = if self.motion.direction == 0 { sx } else { zero };
            } else {
                pivot = if self.motion.direction == 2 { sz } else { zero };
            }
        } else if mode == TIP {
            let f = fraction(t, 360);
            angle = fix32::from_words(37408, 1) * f * f;
            if orient == 1 {
                axis = 1;
                pivot = one;
                if self.motion.first_support != 0 {
                    angle = -angle;
                }
            } else {
                pivot = one;
                if self.motion.first_support == 0 {
                    angle = -angle;
                }
            }
        }
        if moving || mode == TIP {
            let sc = fsincos(angle);
            let c = sc.y();
            let s = sc.x();
            if axis == 1 {
                az = angle;
                cx = pivot + c * (sx * half - pivot) - s * sy * half;
                cy = s * (sx * half - pivot) + c * sy * half;
            } else {
                ax = angle;
                cy = c * sy * half - s * (sz * half - pivot);
                cz = pivot + s * sy * half + c * (sz * half - pivot);
            }
        }
        let mut vertical = zero;
        let mut squeeze = one;
        if mode == ENTRY_RISE {
            let remain = one - fraction(t, 380);
            let e = remain * fix32::from_words(26214, 0) + remain * remain * fix!(0.6);
            vertical = -fix32::from_words(5243, 2) * e;
            squeeze = one - fix32::from_words(6554, 0) * e;
        }
        if mode == GOAL_DROP || mode == GOAL_LAND || mode == GOAL_CLOSE {
            let f = if mode == GOAL_DROP {
                fraction(t, 420)
            } else {
                one
            };
            vertical =
                -fix32::from_words(5243, 2) * (f * fix32::from_words(26214, 0) + f * f * fix!(0.6));
            squeeze = one - fix32::from_words(6554, 0) * f;
        }
        Transform::new(
            Vec3::new(
                cx + fix32::from_int(x as i16) - fix32::from_int(self.level.width as i16) * half,
                cy + vertical,
                cz + fix32::from_int(z as i16) - fix32::from_int(self.level.height as i16) * half,
            ),
            Vec3::new(ax, zero, az),
            Vec3::new(
                sx * squeeze * self.split_pop_scale(),
                sy * self.split_pop_scale(),
                sz * squeeze * self.split_pop_scale(),
            ),
        )
    }

    pub fn block_pose(&self, model: &mut Model) {
        rotation_pose(model, self.block_transform(), self.block_orientation());
    }

    pub fn block_orientation(&self) -> u16 {
        if self.motion.moving != 0 || self.animation.phase == FALL {
            self.rotations.animation()
        } else {
            self.rotations.current(if self.split_visual() {
                self.rules.active_cube
            } else {
                0u16
            })
        }
    }
}
