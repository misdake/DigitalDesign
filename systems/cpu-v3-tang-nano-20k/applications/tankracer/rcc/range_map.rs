//! Bounded offline visibility plans over immutable opaque and cutout commands.
use crate::dsl_rt::*;
mod boot_resources;
mod gpu_api;
mod gpu_board;
mod gpu_window;
mod playback;
mod prepared_map;
mod world;
use boot_resources::*;
use gpu_api::*;
use gpu_board::*;
use gpu_window::gpu_window_asset_read;
use playback::Playback;
use world::{DrawSpan, World};

struct RangeMap {
    pool: PreparedPool,
    parts: Buf<PreparedDrawsPtr, 8>,
    offsets: Ptr,
    headings: Buf<u16, 32>,
    plan: Buf<u16, 64>,
    scratch: Ptr,
    seed: Buf<u16, 4>,
    cell: u16,
    root: u16,
}

impl RangeMap {
    fn new(world: &mut World) -> Self {
        let mut pool = PreparedPool::new(GpuMemory::from_buffer(prepared_map::OPAQUE_STORAGE));
        let first = pool.allocate(8192, 65408);
        let mut result = Self {
            pool,
            parts: Buf::new([first; 8]),
            offsets: malloc(RANGE_UNITS * 5),
            scratch: Ptr::from_addr(0),
            headings: Buf::new([0; 32]),
            plan: Buf::new([0; 64]),
            seed: Buf::new([0; 4]),
            cell: 65535,
            root: 65535,
        };
        if RANGE_HYBRID != 0 {
            result.scratch = malloc(RANGE_UNITS * 2);
        }
        let mut page: u16 = 1;
        while page < RANGE_PAGES as u16 {
            result.parts[page] = result.pool.allocate(8192, 65408);
            page += 1;
        }
        let mut offsets = unsafe { result.offsets.as_u16_array() };
        let entry: Buf<u16, 4> = Buf::new([0; 4]);
        let descriptor: Buf<u16, 4> = Buf::new([0; 4]);
        page = 0;
        while page < RANGE_PAGES as u16 {
            let owner: PreparedDrawsPtr = result.parts[page];
            let mut part = owner.record();
            if RANGE_SHARED_OPAQUE != 0 && MAP_RANGE_PAGE_KIND[page] == 0 {
                part.share_state();
            } else if RANGE_SHARED_CUTOUT != 0 && MAP_RANGE_PAGE_KIND[page] != 0 {
                part.share_material();
            }
            let mut unit = MAP_RANGE_PAGE_FIRST[page];
            let end = MAP_RANGE_PAGE_FIRST[page + 1];
            while unit < end {
                let at = unit * 4;
                let mut draw: u16 = 0;
                let mut count: u16 = 0;
                let mut kind: u16 = 0;
                if RANGE_HYBRID != 0 {
                    if !gpu_window_asset_read(
                        descriptor.as_array(),
                        MAP_RANGE_UNIT_DATA.first_sector,
                        at,
                        4,
                    ) {
                        halt(0x7b58);
                    }
                    draw = descriptor[0u16];
                    count = descriptor[1u16];
                    kind = descriptor[3u16];
                } else {
                    draw = MAP_RANGE_UNITS[at];
                    count = MAP_RANGE_UNITS[at + 1];
                    kind = MAP_RANGE_UNITS[at + 3];
                }
                offsets[unit * 5 + 3] = page;
                offsets[unit * 5 + 4] = draw;
                offsets[unit * 5] = part.mark_slice();
                let mut i: u16 = 0;
                while i < count {
                    if !gpu_window_asset_read(
                        entry.as_array(),
                        MAP_RANGE_DRAWS.first_sector,
                        draw * 4,
                        4,
                    ) {
                        halt(0x7b50);
                    }
                    let mesh = Mesh::from_native(TRACK_MESH.subset(entry[0u16], entry[1u16]));
                    world.record_range_span(&mut part, mesh.ptr(), entry[2u16], kind != 0);
                    drop(mesh);
                    if i == 0 {
                        offsets[unit * 5 + 2] = part.mark_slice();
                    }
                    draw += 1;
                    i += 1;
                }
                offsets[unit * 5 + 1] = part.mark_slice();
                unit += 1;
            }
            part.finish();
            page += 1;
        }
        result
    }

    fn load(&mut self, key: u16) {
        if key == 65535 {
            halt(0x7b51);
        }
        let cell = key >> 5;
        if cell != self.cell {
            let entry: Buf<u16, 1> = Buf::new([0]);
            if !gpu_window_asset_read(entry.as_array(), MAP_RANGE_PLANS.first_sector, cell, 1) {
                halt(0x7b52);
            }
            let row = entry[0u16];
            if !gpu_window_asset_read(
                self.headings.as_array(),
                MAP_RANGE_PLANS.first_sector + RANGE_HEADING_FIRST + (row >> 1),
                (row & 1) << 5,
                32,
            ) {
                halt(0x7b53);
            }
            self.cell = cell;
        }
        let root = self.headings[key & 31];
        if root == 65535 {
            halt(0x7b54);
        }
        if root == self.root {
            return;
        }
        let mut sector = root >> 2;
        let mut offset = (root & 3) << 4;
        if RANGE_PLAN_WORDS == 8 {
            sector = root >> 3;
            offset = (root & 7) << 3;
        }
        if RANGE_PLAN_WORDS == 32 {
            sector = root >> 1;
            offset = (root & 1) << 5;
        }
        if !gpu_window_asset_read(
            self.plan.as_array(),
            MAP_RANGE_PLANS.first_sector + RANGE_PLAN_FIRST + sector,
            offset,
            RANGE_PLAN_WORDS,
        ) {
            halt(0x7b55);
        }
        if self.plan[0u16] > RANGE_LIMIT {
            halt(0x7b56);
        }
        let unit = self.plan[1u16];
        if unit != 65535 {
            if !gpu_window_asset_read(
                self.seed.as_array(),
                MAP_RANGE_DRAWS.first_sector,
                unsafe { self.offsets.as_u16_array() }[unit * 5 + 4] * 4,
                4,
            ) {
                halt(0x7b57);
            }
        } else {
            self.seed[0u16] = 0;
            self.seed[1u16] = 0;
            self.seed[2u16] = 0;
            self.seed[3u16] = 0;
        }
        self.root = root;
    }

    // Selection rounds up to marked units. Only overflow merges holes; page
    // boundaries are mandatory cuts, and the CPU never records a cutout tail.
    fn select(&mut self, mut mask: Array<u16>) {
        let offsets = unsafe { self.offsets.as_u16_array() };
        let mut raw = unsafe { self.scratch.as_u16_array() };
        let mut seed: u16 = 65535;
        let mut unit: u16 = 0;
        let mut split: u16 = 0;
        while split < RANGE_SPLIT_WORDS {
            let first = MAP_RANGE_SPLIT_BLOCKS[split];
            let end = MAP_RANGE_SPLIT_BLOCKS[split + 1];
            let mut any = false;
            unit = first;
            while unit < end {
                any = any || mask[unit >> 4] & (1u16 << (unit & 15)) != 0;
                unit += 1;
            }
            if any {
                unit = first;
                while unit < end {
                    mask[unit >> 4] |= 1u16 << (unit & 15);
                    unit += 1;
                }
            }
            split += 2;
        }
        unit = 0;
        while unit < RANGE_UNITS {
            if mask[unit >> 4] == 0 {
                unit += 16 - (unit & 15);
            } else {
                if mask[unit >> 4] & (1u16 << (unit & 15)) != 0 {
                    seed = unit;
                    break;
                }
                unit += 1;
            }
        }
        if seed != self.root {
            if seed == 65535 {
                self.seed[0u16] = 0;
                self.seed[1u16] = 0;
                self.seed[2u16] = 0;
                self.seed[3u16] = 0;
            } else if !gpu_window_asset_read(
                self.seed.as_array(),
                MAP_RANGE_DRAWS.first_sector,
                offsets[seed * 5 + 4] * 4,
                4,
            ) {
                halt(0x7b57);
            }
            self.root = seed;
        }
        self.plan[1u16] = seed;
        if seed != 65535 && offsets[seed * 5 + 2] == offsets[seed * 5 + 1] {
            mask[seed >> 4] &= !(1u16 << (seed & 15));
        }
        let mut count: u16 = 0;
        unit = seed;
        while unit < RANGE_UNITS {
            if mask[unit >> 4] == 0 {
                unit += 16 - (unit & 15);
            } else if mask[unit >> 4] & (1u16 << (unit & 15)) == 0 {
                unit += 1;
            } else {
                let first = unit;
                let page = offsets[unit * 5 + 3];
                unit += 1;
                while unit < RANGE_UNITS
                    && offsets[unit * 5 + 3] == page
                    && mask[unit >> 4] & (1u16 << (unit & 15)) != 0
                {
                    unit += 1;
                }
                raw[count * 2] = first;
                raw[count * 2 + 1] = unit;
                count += 1;
            }
        }
        if count <= RANGE_LIMIT {
            let mut i: u16 = 0;
            while i < count {
                self.store_range(i, raw[i * 2], raw[i * 2 + 1]);
                i += 1;
            }
            self.plan[0u16] = count;
            return;
        }
        // Keep the most expensive gaps, treating page crossings as infinite.
        // At most eight pages exist, so every physical boundary is retained.
        let mut costs: Buf<u16, 32> = Buf::new([0; 32]);
        let mut cuts: Buf<u16, 32> = Buf::new([65535; 32]);
        let mut i: u16 = 1;
        while i < count {
            let before = raw[i * 2 - 1] - 1;
            let after = raw[i * 2];
            let mut cost: u16 = 65535;
            if offsets[before * 5 + 3] == offsets[after * 5 + 3] {
                cost = offsets[after * 5] - offsets[before * 5 + 1];
            }
            let mut at: u16 = 0;
            while at < RANGE_LIMIT - 1 && cuts[at] != 65535 && costs[at] >= cost {
                at += 1;
            }
            if at < RANGE_LIMIT - 1 {
                let mut move_to = RANGE_LIMIT - 2;
                while move_to > at {
                    costs[move_to] = costs[move_to - 1];
                    cuts[move_to] = cuts[move_to - 1];
                    move_to -= 1;
                }
                costs[at] = cost;
                cuts[at] = i;
            }
            i += 1;
        }
        // Order the retained cuts along the existing command stream.
        i = 1;
        while i < RANGE_LIMIT - 1 {
            let value = cuts[i];
            let mut at = i;
            while at != 0 && cuts[at - 1] > value {
                cuts[at] = cuts[at - 1];
                at -= 1;
            }
            cuts[at] = value;
            i += 1;
        }
        let mut start = raw[0u16];
        i = 0;
        while i < RANGE_LIMIT - 1 {
            let cut = cuts[i];
            self.store_range(i, start, raw[cut * 2 - 1]);
            start = raw[cut * 2];
            i += 1;
        }
        self.store_range(i, start, raw[count * 2 - 1]);
        self.plan[0u16] = RANGE_LIMIT;
    }

    fn store_range(&mut self, index: u16, first: u16, end: u16) {
        if RANGE_PACKED != 0 {
            self.plan[2 + index] = first | (end << 8);
        } else {
            self.plan[2 + index * 2] = first;
            self.plan[3 + index * 2] = end;
        }
    }

    fn record(
        &self,
        world: &mut World,
        commands: &mut CommandBuffer,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
    ) {
        let count = self.plan[0u16];
        let mut segments = count + 2;
        if count == 0 {
            segments = 1;
        }
        if RANGE_SHARED_CUTOUT != 0 {
            segments = self.shared_segment_count(count);
        }
        commands.begin_list(segments);
        let seed = DrawSpan {
            first: self.seed[0u16],
            count: self.seed[1u16],
            material: self.seed[2u16],
            item: self.seed[3u16],
            page: 0,
        };
        let mesh = Mesh::from_native(TRACK_MESH.subset(seed.first, seed.count));
        world.record_prefix(commands, mesh.ptr(), &seed, color, depth);
        drop(mesh);
        let offsets = unsafe { self.offsets.as_u16_array() };
        let mut group = self.seed_group(offsets);
        let mut i: u16 = 0;
        while i < count {
            let first = self.range_first(i);
            let end = self.range_end(i);
            let page = offsets[first * 5 + 3];
            let mut start = offsets[first * 5];
            if first == self.plan[1u16] {
                start = offsets[first * 5 + 2];
            }
            if RANGE_SHARED_CUTOUT != 0 && MAP_RANGE_PAGE_GROUP[page] != group {
                self.record_group_seed(world, commands, offsets[first * 5 + 4]);
                group = MAP_RANGE_PAGE_GROUP[page];
                start = offsets[first * 5 + 2];
            }
            let stop = offsets[(end - 1) * 5 + 1];
            if start < stop {
                commands.link_prepared_range(self.parts[page], start, stop);
            }
            i += 1;
        }
        world.record_range_end(commands, color, depth);
    }

    fn range_first(&self, i: u16) -> u16 {
        if RANGE_PACKED != 0 {
            return self.plan[2 + i] & 255;
        }
        self.plan[2 + i * 2]
    }

    fn range_end(&self, i: u16) -> u16 {
        if RANGE_PACKED != 0 {
            return self.plan[2 + i] >> 8;
        }
        self.plan[3 + i * 2]
    }

    fn seed_group(&self, offsets: Array<u16>) -> u16 {
        let seed = self.plan[1u16];
        if seed == 65535 || RANGE_SHARED_CUTOUT == 0 {
            return 65535;
        }
        MAP_RANGE_PAGE_GROUP[offsets[seed * 5 + 3]]
    }

    fn shared_segment_count(&self, count: u16) -> u16 {
        let offsets = unsafe { self.offsets.as_u16_array() };
        let mut group = self.seed_group(offsets);
        let mut pending_draw = true;
        let mut segments: u16 = 1; // Final dynamic segment.
        let mut i: u16 = 0;
        while i < count {
            let first = self.range_first(i);
            let end = self.range_end(i);
            let next_group = MAP_RANGE_PAGE_GROUP[offsets[first * 5 + 3]];
            let mut start = offsets[first * 5];
            if first == self.plan[1u16] || group != next_group {
                start = offsets[first * 5 + 2];
            }
            if group != next_group {
                pending_draw = true;
                group = next_group;
            }
            if start < offsets[(end - 1) * 5 + 1] {
                segments += 1;
                if pending_draw {
                    segments += 1;
                    pending_draw = false;
                }
            }
            i += 1;
        }
        segments
    }

    fn record_group_seed(&self, world: &mut World, commands: &mut CommandBuffer, draw: u16) {
        let entry: Buf<u16, 4> = Buf::new([0; 4]);
        if !gpu_window_asset_read(entry.as_array(), MAP_RANGE_DRAWS.first_sector, draw * 4, 4) {
            halt(0x7b57);
        }
        let seed = DrawSpan {
            first: entry[0u16],
            count: entry[1u16],
            material: entry[2u16],
            item: entry[3u16],
            page: 0,
        };
        let mesh = Mesh::from_native(TRACK_MESH.subset(seed.first, seed.count));
        world.record_group_seed(commands, mesh.ptr(), &seed);
    }
}

pub fn run() {
    let mut gpu = Gpu::init(board_gpu_config());
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let mut previous = display.frame_index();
    let mut world = World::new();
    let mut atlas = RangeMap::new(&mut world);
    let mut playback = Playback::new();
    let mut active = false;
    let mut odd = false;
    let mut completed = color_b.ptr();
    loop {
        let frame = playback.next();
        if RANGE_HYBRID != 0 {
            atlas.select(world.select_prepared_frame(frame));
        } else {
            atlas.load(world.range_key(frame));
        }
        let mut back = color_b.ptr();
        if odd {
            back = color_a.ptr();
        }
        let mut commands = gpu.command_buffer();
        atlas.record(&mut world, &mut commands, back, depth.ptr());
        let list = commands.finish_list();
        if active {
            gpu.wait_list();
            previous = prepared_map::present_frame(&mut display, previous, completed);
        }
        gpu.submit_list(list);
        active = true;
        odd = !odd;
        completed = back;
    }
}
