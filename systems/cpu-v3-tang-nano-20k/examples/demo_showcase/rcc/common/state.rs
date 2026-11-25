//! App-owned page identity, navigation and independent animation clocks.
use crate::dsl_rt::*;
use crate::gamepad::{
    DS2_CROSS, DS2_DOWN, DS2_L1, DS2_LEFT, DS2_R1, DS2_RIGHT, DS2_START, DS2_TRIANGLE, DS2_UP,
};
mod gpu_schema;
use gpu_schema::DisplayFormat;

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
        if pressed & DS2_CROSS != 0 {
            self.paused = !self.paused;
        }
        if pressed & DS2_RIGHT != 0 {
            self.parameter = (self.parameter + 1) % 32;
        }
        if pressed & DS2_LEFT != 0 {
            self.parameter = (self.parameter + 31) % 32;
        }
        if pressed & DS2_TRIANGLE != 0 {
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
    pub color_cursor: DisplayFormat,
    pub color_mode: DisplayFormat,
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
        color_cursor: DisplayFormat::Rgb565,
        color_mode: DisplayFormat::Rgb565,
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
    if pressed & DS2_R1 != 0 {
        state.page = state.page.next();
    }
    if pressed & DS2_L1 != 0 {
        state.page = state.page.previous();
    }
    if pressed & DS2_START != 0 {
        state.page = PageId::Home;
    }
    if state.page == PageId::Home {
        if pressed & DS2_DOWN != 0 {
            state.selection = state.selection % MENU_COUNT + 1;
        }
        if pressed & DS2_UP != 0 {
            state.selection = (state.selection + MENU_COUNT - 2) % MENU_COUNT + 1;
        }
    }
    let vertical = pressed & (DS2_UP | DS2_DOWN);
    if state.page == PageId::Color && (vertical == DS2_UP || vertical == DS2_DOWN) {
        state.color_cursor = if state.color_cursor == DisplayFormat::Rgb565 {
            DisplayFormat::LinearRgb565
        } else {
            DisplayFormat::Rgb565
        };
    }
    if pressed & DS2_CROSS != 0 {
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
