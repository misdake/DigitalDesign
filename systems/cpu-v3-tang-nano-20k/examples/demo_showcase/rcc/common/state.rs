//! App-owned page identity, navigation and independent animation clocks.
use crate::dsl_rt::*;

pub const PAGE_COUNT: u16 = 7;
pub const MENU_COUNT: u16 = PAGE_COUNT - 1;

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum PageId {
    Home,
    Triangles,
    Waves,
    Controller,
    Color,
    Geometry,
    GpuLab,
}
impl PageId {
    pub fn from_index(index: u16) -> Self {
        if index >= PAGE_COUNT {
            return Self::Home;
        }
        // All values in the guarded range are declared variants.
        unsafe { enum_from_u16_unchecked::<Self>(index) }
    }
    pub fn next(self) -> Self {
        Self::from_index((self as u16 + 1) % PAGE_COUNT)
    }
    pub fn previous(self) -> Self {
        Self::from_index((self as u16 + PAGE_COUNT - 1) % PAGE_COUNT)
    }
    pub fn is_animated(self) -> bool {
        self == Self::Triangles || self == Self::Waves || self == Self::Geometry
    }
    pub fn uses_gpu_scene(self) -> bool {
        self == Self::Geometry || self == Self::GpuLab
    }
}

pub const BUTTON_START: u16 = 1 << 3;
pub const BUTTON_UP: u16 = 1 << 4;
pub const BUTTON_DOWN: u16 = 1 << 6;
pub const BUTTON_RIGHT: u16 = 1 << 5;
pub const BUTTON_LEFT: u16 = 1 << 7;
pub const BUTTON_L1: u16 = 1 << 10;
pub const BUTTON_R1: u16 = 1 << 11;
pub const BUTTON_L2: u16 = 1 << 8;
pub const BUTTON_R2: u16 = 1 << 9;
pub const BUTTON_TRIANGLE: u16 = 1 << 12;
pub const BUTTON_CROSS: u16 = 1 << 14;
pub const BUTTON_SQUARE: u16 = 1 << 15;

#[derive(Copy, Clone)]
pub struct AnimationState {
    pub parameter: u16,
    pub paused: bool,
    pub elapsed: u16,
}
impl AnimationState {
    pub const fn new() -> Self {
        Self {
            parameter: 8,
            paused: false,
            elapsed: 0,
        }
    }
    pub fn advance(&mut self, dt: u16) {
        if !self.paused {
            self.elapsed += dt;
        }
    }
    pub fn controls(&mut self, pressed: u16) {
        if pressed & BUTTON_CROSS != 0 {
            self.paused = !self.paused;
        }
        if pressed & BUTTON_RIGHT != 0 {
            self.parameter = (self.parameter + 1) % 32;
        }
        if pressed & BUTTON_LEFT != 0 {
            self.parameter = (self.parameter + 31) % 32;
        }
        if pressed & BUTTON_TRIANGLE != 0 {
            *self = Self::new();
        }
    }
}

#[derive(Copy, Clone)]
pub struct DemoState {
    pub page: PageId,
    pub selection: u16,
    pub triangles: AnimationState,
    pub waves: AnimationState,
    pub geometry: AnimationState,
    pub previous: u16,
    pub frames: u16,
    pub color_cursor: u16,
    pub color_mode: u16,
}
pub fn initial_state(page: u16, now: u16) -> DemoState {
    DemoState {
        page: PageId::from_index(page),
        selection: 1,
        triangles: AnimationState::new(),
        waves: AnimationState::new(),
        geometry: AnimationState::new(),
        previous: now,
        frames: 0,
        color_cursor: 0,
        color_mode: 0,
    }
}
pub fn update(mut state: DemoState, pressed: u16, now: u16) -> DemoState {
    let dt = now - state.previous;
    state.previous = now;
    // Charge the elapsed interval to the page visible during that interval.
    if state.page == PageId::Triangles {
        state.triangles.advance(dt);
    } else if state.page == PageId::Waves {
        state.waves.advance(dt);
    } else if state.page == PageId::Geometry {
        state.geometry.advance(dt);
    }
    if pressed & BUTTON_R1 != 0 {
        state.page = state.page.next();
    }
    if pressed & BUTTON_L1 != 0 {
        state.page = state.page.previous();
    }
    if pressed & BUTTON_START != 0 {
        state.page = PageId::Home;
    }
    if state.page == PageId::Home {
        if pressed & BUTTON_DOWN != 0 {
            state.selection = state.selection % MENU_COUNT + 1;
        }
        if pressed & BUTTON_UP != 0 {
            state.selection = (state.selection + MENU_COUNT - 2) % MENU_COUNT + 1;
        }
    }
    let vertical = pressed & (BUTTON_UP | BUTTON_DOWN);
    if state.page == PageId::Color && (vertical == BUTTON_UP || vertical == BUTTON_DOWN) {
        state.color_cursor = 1 - state.color_cursor;
    }
    if pressed & BUTTON_CROSS != 0 {
        if state.page == PageId::Home {
            state.page = PageId::from_index(state.selection);
            // Opening a page consumes Accept; it must not also pause that page.
            return state;
        } else if state.page == PageId::Color {
            state.color_mode = state.color_cursor;
        }
    }
    if state.page != PageId::Home {
        state.selection = state.page as u16;
    }
    if state.page == PageId::Triangles {
        state.triangles.controls(pressed);
    } else if state.page == PageId::Waves {
        state.waves.controls(pressed);
    } else if state.page == PageId::Geometry {
        state.geometry.controls(pressed);
    }
    state
}

// Nominal full rotation at one radian per second.
const ANIMATION_PERIOD_MS: u16 = 6283;
pub fn animation_phase(state: AnimationState) -> fix32 {
    fix32::from_int((state.elapsed % ANIMATION_PERIOD_MS) as i16) * fix!(0.001)
}
