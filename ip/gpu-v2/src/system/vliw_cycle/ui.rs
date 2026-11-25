//! The production two-task UI engine. Grid/atlas bytes stay in the source RAM;
//! this owner retains only task metadata and the one synchronous-Q calendar.
use super::{require, ContractError};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub start: bool,
    pub stop: bool,
    pub grid_base: u16,
    pub atlas_base: u8,
    pub first_cell: u16,
    pub last_cell: u16,
    pub cp_ready: bool,
    pub cp_q: u64,
    pub work_ready: bool,
    pub dma_idle: bool,
    pub command_ready: bool,
    pub command_done: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub active: bool,
    pub done: bool,
    pub cp_read: bool,
    pub cp_address: u16,
    pub work_valid: bool,
    pub work_source: u32,
    pub work_destination: u8,
    pub command_valid: bool,
    pub writing: bool,
    pub lower_ready: bool,
    pub tile: u16,
    pub pixel_valid: bool,
    pub pixel_half: bool,
    pub pixel_word: u8,
    pub pixels: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    pub active: bool,
    pub done: bool,
    pub atlas: u8,
    pub init_sector: u8,
    pub initialized: bool,
    pub scan_done: bool,
    pub scan: u16,
    pub tile_column: u8,
    pub tile_row: u8,
    pub window: u16,
    pub pair_valid: bool,
    pub valid: [bool; 2],
    pub loaded: [bool; 2],
    pub put: bool,
    pub get: bool,
    pub loader: bool,
    pub task_tile: [u16; 2],
    pub ids: [u16; 2],
    pub load_part: u8,
    pub phase: u8,
    pub overlay: bool,
    pub overlay_half: bool,
    pub pixel: u8,
    pub q_valid: bool,
    pub q_pixel: bool,
    pub q_half: bool,
    pub q_word: u8,
}
#[derive(Clone, Debug, Default)]
pub struct Engine {
    pub state: State,
}
const WAIT_TASK: u8 = 0;
const READ0: u8 = 1;
const READ0_WAIT: u8 = 2;
const READ1_WAIT: u8 = 4;
const OVER0: u8 = 5;
const WRITE0: u8 = 6;
const WRITE0_WAIT: u8 = 7;
struct Evaluation {
    output: Output,
    can_scan: bool,
    last_pair: bool,
    selected_ids: u16,
    push: bool,
    scan_step: bool,
    pixel_read: bool,
    read_take: bool,
    covered: bool,
    init_work: bool,
    atlas_work: bool,
    skip_part: bool,
    work_take: bool,
    overlay_done: bool,
}
impl Engine {
    fn evaluate(&self, i: Input) -> Evaluation {
        let s = &self.state;
        let can_scan = s.initialized && !s.scan_done;
        let first_pair = s.scan == (i.first_cell & 1023) >> 1;
        let last_pair = s.scan == (i.last_cell & 1023) >> 1;
        let selected_ids = (if last_pair && i.last_cell & 1 == 0 {
            0xff00
        } else {
            s.window & 0xff00
        }) | (if first_pair && i.first_cell & 1 != 0 {
            0xff
        } else {
            s.window & 255
        });
        let scan_step =
            i.ce && s.active && !i.stop && can_scan && s.pair_valid && !s.valid[usize::from(s.put)];
        let push = scan_step && selected_ids != 0xffff;
        let scan_read = can_scan && !s.pair_valid && !s.q_valid && !s.overlay;
        let covered = if s.pixel & 2 != 0 {
            s.ids[usize::from(s.get)] >> 8 != 255
        } else {
            s.ids[usize::from(s.get)] & 255 != 255
        };
        let pixel_read = s.overlay && s.pixel < 32 && covered;
        let cp_read = s.active && !i.stop && (pixel_read || scan_read);
        let read_take = i.ce && cp_read && i.cp_ready;
        let init_work = !s.initialized && s.init_sector != 7;
        let atlas_work = s.initialized
            && s.valid[usize::from(s.loader)]
            && !s.loaded[usize::from(s.loader)]
            && s.load_part < 4;
        let char_id = if s.load_part & 2 != 0 {
            s.ids[usize::from(s.loader)] >> 8
        } else {
            s.ids[usize::from(s.loader)] & 255
        };
        let skip_part = atlas_work && char_id == 255;
        let work_valid = s.active && !i.stop && (init_work || atlas_work && !skip_part);
        let overlay_done = !s.overlay && !(s.q_valid && s.q_pixel);
        let output = Output {
            active: s.active,
            done: s.done,
            cp_read,
            cp_address: if s.overlay {
                128 | u16::from(s.get) << 6
                    | u16::from(s.pixel >> 1 & 1) << 5
                    | u16::from(s.overlay_half) << 4
                    | u16::from(s.pixel >> 2 & 7) << 1
                    | u16::from(s.pixel & 1)
            } else {
                s.scan >> 2
            },
            work_valid,
            work_source: if init_work {
                u32::from(s.window.wrapping_add(s.init_sector.into())) << 6
            } else {
                u32::from(s.atlas) << 15 | u32::from(char_id) << 7 | u32::from(s.load_part & 1) << 6
            },
            work_destination: if init_work {
                s.init_sector << 2
            } else {
                32 | u8::from(s.loader) << 4 | (s.load_part & 3) << 2
            },
            command_valid: s.active && !i.stop && (s.phase == READ0 || s.phase == WRITE0),
            writing: s.phase == WRITE0,
            lower_ready: i.stop || s.overlay_half && overlay_done,
            tile: s.task_tile[usize::from(s.get)],
            pixel_valid: s.active && i.ce && s.q_valid && s.q_pixel && !i.stop,
            pixel_half: s.q_half,
            pixel_word: s.q_word,
            pixels: i.cp_q,
        };
        Evaluation {
            output,
            can_scan,
            last_pair,
            selected_ids,
            push,
            scan_step,
            pixel_read,
            read_take,
            covered,
            init_work,
            atlas_work,
            skip_part,
            work_take: i.ce && work_valid && i.work_ready,
            overlay_done,
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.evaluate(i).output
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        if i.reset {
            self.state = State::default();
            return Ok(());
        }
        let s = self.state;
        let e = self.evaluate(i);
        let o = e.output;
        require(!(i.start && s.active), "ui", "UI start while active")?;
        require(
            !o.pixel_valid || matches!(s.phase, READ1_WAIT | OVER0 | WRITE0 | WRITE0_WAIT),
            "ui",
            "UI pixel lifetime",
        )?;
        let mut n = s;
        if i.ce {
            n.q_valid = e.read_take;
            n.q_pixel = e.pixel_read;
            if e.read_take {
                n.q_half = s.overlay_half;
                n.q_word = s.pixel & 31;
            }
            if s.q_valid && !s.q_pixel {
                n.window = (i.cp_q >> ((s.scan & 3) * 16)) as u16;
                n.pair_valid = true;
            }
            if s.active && !s.initialized && !s.pair_valid && !i.stop {
                if s.scan >= 25 {
                    n.scan = s.scan - 25;
                    n.tile_row = (s.tile_row + 1) & 15;
                } else {
                    n.tile_column = s.scan as u8;
                    n.scan = (i.first_cell & 1023) >> 1;
                    n.pair_valid = true;
                }
            }
            if e.scan_step {
                n.scan = (s.scan + 1) & 511;
                if s.tile_column == 24 {
                    n.tile_column = 0;
                    n.tile_row = (s.tile_row + 1) & 15;
                } else {
                    n.tile_column = (s.tile_column + 1) & 31;
                }
                n.pair_valid = false;
                if e.last_pair {
                    n.scan_done = true;
                }
                if e.push {
                    let p = usize::from(s.put);
                    n.valid[p] = true;
                    n.loaded[p] = false;
                    n.task_tile[p] = u16::from(s.tile_column) << 4 | u16::from(s.tile_row);
                    n.ids[p] = e.selected_ids;
                    n.put = !s.put;
                }
            }
            if s.active {
                if e.work_take && e.init_work {
                    n.init_sector = if s.init_sector == ((i.last_cell & 1023) >> 7) as u8 {
                        7
                    } else {
                        (s.init_sector + 1) & 7
                    };
                }
                if !s.initialized && !e.init_work && i.dma_idle && s.pair_valid {
                    n.initialized = true;
                    n.pair_valid = false;
                }
                if e.atlas_work && (e.skip_part || e.work_take) {
                    n.load_part = (s.load_part + 1) & 7;
                }
                if s.load_part == 4 && i.dma_idle {
                    n.loaded[usize::from(s.loader)] = true;
                    n.loader = !s.loader;
                    n.load_part = 0;
                }
                if s.overlay && s.pixel < 32 && (!e.covered || e.read_take) {
                    n.pixel = (s.pixel + 1) & 63;
                }
                if s.overlay && s.pixel == 32 {
                    n.overlay = false;
                }
                if s.phase == WAIT_TASK
                    && s.valid[usize::from(s.get)]
                    && s.loaded[usize::from(s.get)]
                    && (!(s.valid[usize::from(!s.get)] || e.push) || s.loaded[usize::from(!s.get)])
                {
                    n.phase = READ0;
                }
                if s.phase == OVER0 && e.overlay_done {
                    n.phase = WRITE0;
                    n.overlay = true;
                    n.overlay_half = true;
                    n.pixel = 0;
                }
                if s.initialized
                    && !e.can_scan
                    && s.valid == [false; 2]
                    && i.dma_idle
                    && s.phase == WAIT_TASK
                    && !s.q_valid
                {
                    n.active = false;
                    n.done = true;
                }
            }
        }
        if o.command_valid && i.command_ready {
            match s.phase {
                READ0 => n.phase = READ0_WAIT,
                WRITE0 => n.phase = WRITE0_WAIT,
                _ => {}
            }
        }
        if i.command_done {
            match s.phase {
                READ0_WAIT => {
                    n.phase = READ1_WAIT;
                    n.overlay = true;
                    n.overlay_half = false;
                    n.pixel = 0;
                }
                READ1_WAIT => n.phase = OVER0,
                WRITE0_WAIT => {
                    n.valid[usize::from(s.get)] = false;
                    n.loaded[usize::from(s.get)] = false;
                    n.get = !s.get;
                    n.phase = WAIT_TASK;
                }
                _ => {}
            }
        }
        if s.active && i.stop && i.dma_idle && i.command_ready {
            n.active = false;
            n.done = true;
            n.overlay = false;
            n.q_valid = false;
        }
        if i.start {
            n.active = true;
            n.done = false;
            n.window = i.grid_base;
            n.atlas = i.atlas_base & 127;
            n.init_sector = ((i.first_cell & 1023) >> 7) as u8;
            n.initialized = false;
            n.scan = (i.first_cell & 1023) >> 1;
            n.tile_row = 0;
            n.scan_done = false;
            n.pair_valid = false;
            n.valid = [false; 2];
            n.loaded = [false; 2];
            n.put = false;
            n.get = false;
            n.loader = false;
            n.load_part = 0;
            n.phase = WAIT_TASK;
            n.overlay = false;
            n.q_valid = false;
        }
        self.state = n;
        Ok(())
    }
}
