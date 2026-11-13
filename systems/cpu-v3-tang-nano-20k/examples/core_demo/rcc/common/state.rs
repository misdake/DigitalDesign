//! Actual gamepad edges and modulo wall time drive the application.
use crate::dsl_rt::*;
mod gamepad;
use gamepad::*;
mod time;
use time::*;

pub const PAGE_COUNT: u16 = 6;
pub const MENU_COUNT: u16 = PAGE_COUNT - 1;

pub const BUTTON_START: u16 = 1 << 3;
pub const BUTTON_UP: u16 = 1 << 4;
pub const BUTTON_DOWN: u16 = 1 << 6;
pub const BUTTON_RIGHT: u16 = 1 << 5;
pub const BUTTON_LEFT: u16 = 1 << 7;
pub const BUTTON_L1: u16 = 1 << 10;
pub const BUTTON_R1: u16 = 1 << 11;
pub const BUTTON_TRIANGLE: u16 = 1 << 12;
pub const BUTTON_CROSS: u16 = 1 << 14;

#[derive(Copy, Clone)]
pub struct DemoState {
    pub page: u16,
    pub selection: u16,
    pub parameter: u16,
    pub paused: bool,
    pub elapsed: u16,
    pub previous: u16,
    pub frames: u16,
    pub color_cursor: u16,
    pub color_mode: u16,
}
pub fn initial_state(page: u16) -> DemoState {
    DemoState {
        page: page,
        selection: 1,
        parameter: 8,
        paused: false,
        elapsed: 0,
        previous: time_milliseconds(),
        frames: 0,
        color_cursor: 0,
        color_mode: 0,
    }
}
pub fn update(mut state: DemoState, pressed: u16, now: u16) -> DemoState {
    let dt = time_elapsed_ms(state.previous, now);
    state.previous = now;
    if pressed & BUTTON_R1 != 0 {
        state.page = (state.page + 1) % PAGE_COUNT;
    }
    if pressed & BUTTON_L1 != 0 {
        state.page = (state.page + PAGE_COUNT - 1) % PAGE_COUNT;
    }
    if pressed & BUTTON_START != 0 {
        state.page = 0;
    }
    if state.page == 0 {
        if pressed & BUTTON_DOWN != 0 {
            state.selection = state.selection % MENU_COUNT + 1;
        }
        if pressed & BUTTON_UP != 0 {
            state.selection = (state.selection + MENU_COUNT - 2) % MENU_COUNT + 1;
        }
    }
    let vertical = pressed & (BUTTON_UP | BUTTON_DOWN);
    if state.page == 4 && (vertical == BUTTON_UP || vertical == BUTTON_DOWN) {
        state.color_cursor = 1 - state.color_cursor;
    }
    if pressed & BUTTON_CROSS != 0 {
        if state.page == 0 {
            state.page = state.selection;
        } else if state.page == 4 {
            state.color_mode = state.color_cursor;
        } else if state.page == 1 || state.page == 2 || state.page == 5 {
            state.paused = !state.paused;
        }
    }
    if state.page != 0 {
        state.selection = state.page;
    }
    if (state.page == 1 || state.page == 2 || state.page == 5) && pressed & BUTTON_RIGHT != 0 {
        state.parameter = (state.parameter + 1) % 32;
    }
    if (state.page == 1 || state.page == 2 || state.page == 5) && pressed & BUTTON_LEFT != 0 {
        state.parameter = (state.parameter + 31) % 32;
    }
    if (state.page == 1 || state.page == 2 || state.page == 5) && pressed & BUTTON_TRIANGLE != 0 {
        state.parameter = 8;
        state.elapsed = 0;
        state.paused = false;
    }
    if !state.paused && pressed & BUTTON_TRIANGLE == 0 {
        state.elapsed += dt;
    }
    state
}

// Nominal full rotation at one radian per second.
const ANIMATION_PERIOD_MS: u16 = 6283;
pub fn animation_phase(state: DemoState) -> fix32 {
    fix32::from_int((state.elapsed % ANIMATION_PERIOD_MS) as i16) * fix!(0.001)
}
