//! Route-ordered fine/coarse opaque command rings and an ordinary frame tail.
use crate::dsl_rt::*;
mod boot_resources;
mod gpu_api;
mod gpu_board;
mod gpu_window;
mod static_buffer;
mod world;
use boot_resources::*;
use gpu_api::*;
use gpu_board::*;
use gpu_window::gpu_window_asset_read;
use static_buffer::StaticBuffer;
use world::{DrawSpan, Selection, World};

const MAX_RANGES: u16 = 30;

#[static_buffer]
const OPAQUE_STORAGE: StaticBuffer = StaticBuffer::new(MAP_PREPARED_PAGES * 40960, 128);

struct PreparedMap {
    pool: PreparedPool,
    // The native mesh limit bounds both rings to at most 86 command pages.
    parts: Buf<PreparedDrawsPtr, 86>,
    offsets: Ptr,
}

struct Plan {
    ranges: u16,
    tail_span: u16,
    seed: DrawSpan,
}

impl PreparedMap {
    fn new(world: &mut World) -> Self {
        let mut pool = PreparedPool::new(GpuMemory::from_buffer(OPAQUE_STORAGE));
        let first = pool.allocate(8192, 32768);
        let mut result = Self {
            pool,
            parts: Buf::new([first; 86]),
            offsets: malloc((OPAQUE_COUNT + 1) * 4),
        };
        let mut page: u16 = 1;
        while page < MAP_PREPARED_PAGES as u16 {
            result.parts[page] = result.pool.allocate(8192, 32768);
            page += 1;
        }
        result.record(world);
        result
    }

    fn record(&mut self, world: &mut World) {
        let mut offsets = unsafe { self.offsets.as_u16_array() };
        let entries: Buf<u16, 56> = Buf::new([0; 56]);
        let words = entries.as_array();
        let mut page: u16 = 0;
        while page < MAP_PREPARED_PAGES as u16 {
            let variant = page / MAP_PREPARED_RING_PAGES;
            let local_page = page % MAP_PREPARED_RING_PAGES;
            let owner: PreparedDrawsPtr = self.parts[page];
            let mut part: PreparedPart = owner.record();
            if MAP_SHARED_OPAQUE_STATE != 0 {
                part.share_state();
            }
            let mut item = MAP_PREPARED_PAGE_FIRST[local_page];
            let end = MAP_PREPARED_PAGE_FIRST[local_page + 1];
            let mut loaded: u16 = 65535;
            while item < end {
                let bucket = item >> 3;
                if loaded != bucket {
                    let mut count = OPAQUE_COUNT - bucket * 8;
                    if count > 8 {
                        count = 8;
                    }
                    if !gpu_window_asset_read(
                        words,
                        MAP_OPAQUE_RING.first_sector,
                        bucket * 56,
                        count * 7,
                    ) {
                        halt(0x7b40);
                    }
                    loaded = bucket;
                }
                let at = (item & 7) * 7;
                let first = words[at + variant * 2];
                let count = words[at + variant * 2 + 1];
                let mark = item * 4 + variant * 2;
                offsets[mark] = part.mark_slice();
                // Groups without a reduced mesh reuse their fine-ring entry.
                if variant == 0 || first != words[at] || count != words[at + 1] {
                    let group = Mesh::from_native(TRACK_MESH.subset(first, count));
                    world.record_static_span(&mut part, group.ptr(), words[at + 4]);
                    drop(group);
                }
                offsets[mark + 1] = part.mark_slice();
                item += 1;
            }
            part.finish();
            page += 1;
        }
    }

    // Consecutive route groups share a slice. Holes and LOD switches remain
    // explicit; only the caller's exact selected geometry enters the list.
    fn plan(&self, world: &World, selected: &Selection, mut ranges: Array<u16>) -> Plan {
        let offsets = unsafe { self.offsets.as_u16_array() };
        let mut result = Plan {
            ranges: 0,
            tail_span: 0,
            seed: DrawSpan {
                first: 0,
                count: 0,
                material: 0,
                item: 0,
                page: 0,
            },
        };
        let mut index: u16 = 0;
        if selected.opaque != 0 {
            result.seed = world.opaque_span(0);
            index = 1;
        }
        while index < selected.opaque {
            let span = world.opaque_span(index);
            let variant = span.page / MAP_PREPARED_RING_PAGES;
            let mark = span.item * 4 + variant * 2;
            let first = offsets[mark];
            let end = offsets[mark + 1];
            let mut merged = false;
            if result.ranges != 0 {
                let prior = (result.ranges - 1) * 3;
                if ranges[prior] == span.page && ranges[prior + 2] == first {
                    ranges[prior + 2] = end;
                    merged = true;
                }
            }
            if !merged {
                if result.ranges == MAX_RANGES {
                    result.tail_span = index;
                    return result;
                }
                let at = result.ranges * 3;
                ranges[at] = span.page;
                ranges[at + 1] = first;
                ranges[at + 2] = end;
                result.ranges += 1;
            }
            index += 1;
        }
        result.tail_span = selected.opaque;
        result
    }

    fn append(&self, commands: &mut CommandBuffer, ranges: Array<u16>, count: u16) {
        let mut index: u16 = 0;
        while index < count {
            let at = index * 3;
            commands.link_prepared_range(self.parts[ranges[at]], ranges[at + 1], ranges[at + 2]);
            index += 1;
        }
    }
}

fn present_frame(display: &mut Display, previous: u16, color: ColorBufferPtr) -> u16 {
    if display.frame_index() == previous {
        display.wait_vblank();
    }
    display.set_next_swap(color);
    display.wait_vblank();
    display.frame_index()
}

pub fn run() {
    let mut gpu = Gpu::init(board_gpu_config());
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let mut last_present = display.frame_index();
    let mut world = World::new();
    let atlas = PreparedMap::new(&mut world);
    let ranges: Buf<u16, 90> = Buf::new([0; 90]);
    let mut frame: u16 = 0;
    let mut odd = false;
    let mut active = false;
    let mut completed_color = color_b.ptr();
    loop {
        let selected = world.select_frame(frame);
        let plan = atlas.plan(&world, &selected, ranges.as_array());
        let mut back = color_b.ptr();
        if odd {
            back = color_a.ptr();
        }
        let mut commands = gpu.command_buffer();
        let mut segments = plan.ranges + 2;
        if plan.ranges == 0 {
            segments = 1;
        }
        commands.begin_list(segments);
        let seed = Mesh::from_native(TRACK_MESH.subset(plan.seed.first, plan.seed.count));
        world.record_prefix(&mut commands, seed.ptr(), &plan.seed, back, depth.ptr());
        drop(seed);
        atlas.append(&mut commands, ranges.as_array(), plan.ranges);
        let mut tail_first = commands.command_words();
        if plan.ranges == 0 {
            tail_first = 0;
        }
        world.record_tail(&mut commands, &selected, plan.tail_span, back, depth.ptr());
        // A segment covers at most 64 sectors, including its starting offset.
        // The closed tail pass supplies WAIT/FLUSH; sealing still adds END.
        if commands.command_words() - tail_first > 4094 - (tail_first & 63) {
            // Direct submission retains the full 64-KiB CB budget. A large
            // transparent view must not truncate work to fit one list entry.
            commands.reset();
            world.record_fallback(&mut commands, &selected, back, depth.ptr());
            if active {
                gpu.wait_list();
                last_present = present_frame(&mut display, last_present, completed_color);
            }
            gpu.submit(commands);
            let mut recording = gpu.command_buffer();
            completed_color = back;
            // Keep both owners and use the ordinary double-CB loop from here.
            // Dropping a configured CB would permanently retire its slot.
            loop {
                odd = !odd;
                frame += 1;
                if frame == CAMERA_COUNT {
                    frame = 0;
                }
                let mut next_back = color_b.ptr();
                if odd {
                    next_back = color_a.ptr();
                }
                let next = world.select_frame(frame);
                world.record_fallback(&mut recording, &next, next_back, depth.ptr());
                let completed = gpu.wait();
                last_present = present_frame(&mut display, last_present, completed_color);
                gpu.submit(recording);
                recording = completed;
                completed_color = next_back;
            }
        } else {
            let list = commands.finish_list();
            if active {
                gpu.wait_list();
                last_present = present_frame(&mut display, last_present, completed_color);
            }
            gpu.submit_list(list);
            active = true;
        }
        completed_color = back;
        odd = !odd;
        frame += 1;
        if frame == CAMERA_COUNT {
            frame = 0;
        }
    }
}
