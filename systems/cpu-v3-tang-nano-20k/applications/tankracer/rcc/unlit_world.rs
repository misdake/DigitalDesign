//! Ordinary unlit recording and its command-reuse storage.
use crate::dsl_rt::*;
mod boot_resources;
mod gpu_api;
mod gpu_window;
mod world;
use boot_resources::*;
use gpu_api::*;
use gpu_window::gpu_window_asset_read;
use world::{Selection, World};

pub struct UnlitWorld {
    pub world: World,
    unlit: UnlitBank,
    bindings: Ptr,
    cache_a: Ptr,
    cache_b: Ptr,
    count_a: u16,
    count_b: u16,
}

impl UnlitWorld {
    pub fn new() -> Self {
        let world = World::new();
        let mesh = Mesh::from_native(TRACK_MESH);
        let unlit = UnlitBank::new(mesh.ptr());
        drop(mesh);
        let mut cache_a = Ptr::from_addr(0);
        let mut cache_b = Ptr::from_addr(0);
        if TANKRACER_REUSE != 0 {
            cache_a = malloc(GROUP_COUNT * 3);
            cache_b = malloc(GROUP_COUNT * 3);
        }
        let mut result = Self {
            world,
            unlit,
            bindings: Ptr::from_addr(0),
            cache_a,
            cache_b,
            count_a: 0,
            count_b: 0,
        };
        result.prepare_bindings();
        result
    }
    fn prepare_bindings(&mut self) {
        let words = unsafe { self.world.groups.as_u16_array() };
        let mut maximum: u16 = 0;
        let mut first: u16 = 0;
        while first < GROUP_COUNT {
            let mut count = GROUP_COUNT - first;
            if count > (GROUP_SCRATCH_WORDS / GROUP_WORDS) {
                count = (GROUP_SCRATCH_WORDS / GROUP_WORDS);
            }
            if !gpu_window_asset_read(
                words,
                MAP_GROUP_DATA.first_sector,
                first * GROUP_WORDS,
                count * GROUP_WORDS,
            ) {
                halt(0x7b30);
            }
            let mut i: u16 = 0;
            while i < count {
                let material = words[i * GROUP_WORDS + 9] & 32767;
                if material > maximum {
                    maximum = material;
                }
                i += 1;
            }
            first += count;
        }
        self.bindings = malloc((maximum + 1) * 2);
        let mut map = unsafe { self.bindings.as_u16_array() };
        let mut i: u16 = 0;
        while i < (maximum + 1) * 2 {
            map[i] = 0;
            i += 1;
        }
        first = 0;
        while first < GROUP_COUNT {
            let mut count = GROUP_COUNT - first;
            if count > (GROUP_SCRATCH_WORDS / GROUP_WORDS) {
                count = (GROUP_SCRATCH_WORDS / GROUP_WORDS);
            }
            if !gpu_window_asset_read(
                words,
                MAP_GROUP_DATA.first_sector,
                first * GROUP_WORDS,
                count * GROUP_WORDS,
            ) {
                halt(0x7b31);
            }
            i = 0;
            while i < count {
                let at = i * GROUP_WORDS;
                let key = words[at + 9];
                let material = key & 32767;
                let index = material * 2 + (key >> 15);
                if map[index] == 0 {
                    self.world.set_material(key);
                    let mut state = self.world.opaque.ptr();
                    if words[at + 10] >= OPAQUE_COUNT {
                        state = self.world.blended.ptr();
                        if map_materials()[material * 4 + 1] & 8 != 0 {
                            state = self.world.cutout.ptr();
                        }
                    }
                    let texture_id = map_materials()[material * 4];
                    if texture_id == 0 {
                        map[index] = self.unlit.add_colored(state, self.world.material.ptr());
                    } else {
                        let texture = map_texture(texture_id - 1);
                        let mut view = texture.view();
                        view.set_min_mip_size((map_materials()[material * 4 + 1] >> 1) & 3);
                        if map_materials()[material * 4 + 1] & 1 != 0 {
                            view.set_address_mode(TextureAddressMode::ClampToEdge);
                        }
                        map[index] = self.unlit.add(
                            state,
                            self.world.material.ptr(),
                            &view,
                            key & 32768 != 0,
                        );
                    }
                }
                i += 1;
            }
            first += count;
        }
    }

    pub fn record(
        &mut self,
        commands: &mut CommandBuffer,
        frame: u16,
        bank: u16,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
    ) {
        let selected = self.world.select_frame(frame);
        self.record_unlit(commands, bank, color, depth, &selected);
    }

    fn record_unlit(
        &mut self,
        commands: &mut CommandBuffer,
        bank: u16,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
        selected: &Selection,
    ) {
        let count = selected.opaque + selected.cutout + selected.blended;
        let mut previous_count = self.count_a;
        let mut cache = self.cache_a;
        if bank != 0 {
            previous_count = self.count_b;
            cache = self.cache_b;
        }
        if TANKRACER_REUSE != 0 {
            let compare = previous_count != 0 && previous_count == count;
            let same = self.cache_draws(cache, selected, compare);
            if same {
                commands.patch_unlit(&self.unlit, self.world.camera.ptr(), color, depth);
                return;
            }
        }
        commands.reset();
        commands.prepare_unlit(
            &mut self.unlit,
            self.world.camera.ptr(),
            self.world.transform.ptr(),
        );
        {
            let mut pass = commands.pass(
                color,
                depth,
                // Static-world sky [0.48, 0.62, 0.72], rounded to RGB565.
                ColorLoad::clear(Rgb565::from_raw(0x7cf6)),
                DepthLoad::clear_far(),
            );
            let opaque = unsafe { self.world.opaque_draws.as_u16_array() };
            self.draw_list(&mut pass, opaque, selected.opaque, OPAQUE_DRAW_WORDS);
            self.draw_cutouts(&mut pass, selected.blended, selected.cutout);
            if GROUP_COUNT != OPAQUE_COUNT {
                let blended = unsafe { self.world.blended_draws.as_u16_array() };
                self.draw_list(&mut pass, blended, selected.blended, 6);
            }
        }
        if TANKRACER_REUSE != 0 {
            if bank == 0 {
                self.count_a = count;
            } else {
                self.count_b = count;
            }
        }
    }

    pub fn record_fallback(
        &mut self,
        commands: &mut CommandBuffer,
        selected: &Selection,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
    ) {
        self.record_unlit(commands, 0, color, depth, selected);
    }

    // Keep transparent order and overflow work on the original fast writer.
    pub fn record_tail(
        &mut self,
        commands: &mut CommandBuffer,
        selected: &Selection,
        first: u16,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
    ) {
        // Append after the linked opaque ranges in the same writable CB.
        commands.prepare_unlit(
            &mut self.unlit,
            self.world.camera.ptr(),
            self.world.transform.ptr(),
        );
        let mut pass = commands.pass(color, depth, ColorLoad::load(), DepthLoad::load());
        let map = unsafe { self.bindings.as_u16_array() };
        let mut index = first;
        while index < selected.opaque {
            let span = self.world.opaque_span(index);
            let binding = map[(span.material & 32767) * 2 + (span.material >> 15)];
            pass.draw_unlit(&self.unlit, binding, span.first, span.count);
            index += 1;
        }
        self.draw_cutouts(&mut pass, selected.blended, selected.cutout);
        if selected.blended != 0 {
            let blended = unsafe { self.world.blended_draws.as_u16_array() };
            self.draw_list(&mut pass, blended, selected.blended, 6);
        }
    }

    fn cache_draws(&mut self, cache: Ptr, selected: &Selection, compare: bool) -> bool {
        let mut saved = unsafe { cache.as_u16_array() };
        let opaque = unsafe { self.world.opaque_draws.as_u16_array() };
        let blended = unsafe { self.world.blended_draws.as_u16_array() };
        let order = unsafe { self.world.blended_order.as_u16_array() };
        let mut same = compare;
        let mut i: u16 = 0;
        while i < selected.opaque + selected.cutout + selected.blended {
            let mut input = opaque;
            let mut at = i * 3;
            if i >= selected.opaque {
                input = blended;
                if i < selected.opaque + selected.cutout {
                    at = order[selected.blended + i - selected.opaque] * 6;
                } else {
                    at = order[i - selected.opaque - selected.cutout] * 6;
                }
            }
            let mut word: u16 = 0;
            while word < 3 {
                let value = input[at + word];
                if !compare || saved[i * 3 + word] != value {
                    same = false;
                    saved[i * 3 + word] = value;
                }
                word += 1;
            }
            i += 1;
        }
        same
    }

    fn draw_cutouts(&mut self, pass: &mut Pass, start: u16, count: u16) {
        let words = unsafe { self.world.blended_draws.as_u16_array() };
        let map = unsafe { self.bindings.as_u16_array() };
        let order = unsafe { self.world.blended_order.as_u16_array() };
        let mut i: u16 = 0;
        // Fixed texture/stable-ID order, after terrain and before SrcOver.
        // The two ends of the existing scratch hold disjoint classes.
        while i < count {
            let at = order[start + i] * 6;
            let key = words[at + 2];
            pass.draw_unlit(
                &self.unlit,
                map[(key & 32767) * 2 + (key >> 15)],
                words[at],
                words[at + 1],
            );
            i += 1;
        }
    }

    fn draw_list(&mut self, pass: &mut Pass, words: Array<u16>, count: u16, stride: u16) {
        let order = unsafe { self.world.blended_order.as_u16_array() };
        let map = unsafe { self.bindings.as_u16_array() };
        let mut i: u16 = 0;
        while i < count {
            let mut at = i * stride;
            if stride == 6 {
                at = order[i] * 6;
            }
            let mut key: u16 = 0;
            let mut first: u16 = 0;
            let mut length: u16 = 0;
            if TANKRACER_PREPARED != 0 && stride == OPAQUE_DRAW_WORDS {
                let span = self.world.opaque_span(i);
                key = span.material;
                first = span.first;
                length = span.count;
            } else {
                key = words[at + 2];
                first = words[at];
                length = words[at + 1];
            }
            let binding = map[(key & 32767) * 2 + (key >> 15)];
            pass.draw_unlit(&self.unlit, binding, first, length);
            i += 1;
        }
    }
}
