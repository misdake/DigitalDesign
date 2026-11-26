//! A baked world owns ranges, materials, and bounded draw-list scratch.
use crate::dsl_rt::*;
mod boot_resources;
mod gpu_api;
mod gpu_window;
use boot_resources::*;
use gpu_api::*;
use gpu_window::gpu_window_asset_read;

const FAR: fix32 = fix32::from_words(MAP_FAR_LOW, MAP_FAR_HIGH);
const DISPLAY_DISTANCE: fix32 = fix32::from_words(MAP_DISPLAY_LOW, MAP_DISPLAY_HIGH);
const LOD_DISTANCE: fix32 = fix32::from_words(MAP_LOD_LOW, MAP_LOD_HIGH);
const X_PER_DEPTH: fix32 = fix!(0.78515625);
const Y_PER_DEPTH: fix32 = fix!(0.47265625);

// Current render-only s=segment+t; negative means cluster fallback.
static RENDER_ROUTE_POSITION: Buf<u16, 3> = Buf::new([0, 65535, 65535]);

struct View {
    matrix: Mat3x4,
    absolute: Mat3x3,
}

pub struct DrawSpan {
    pub first: u16,
    pub count: u16,
    pub material: u16,
    pub item: u16,
    pub page: u16,
}

pub struct World {
    unlit: UnlitBank,
    bindings: Ptr,
    transform: Transform,
    camera: Camera,
    material: Material,
    opaque: RenderState,
    blended: RenderState,
    groups: Ptr,
    // Prepared: three selected words per ring item, then two ordered words
    // per visible item. UnlitBank: tightly packed first/count/material triples.
    opaque_draws: Ptr,
    opaque_mask: Ptr,
    blended_draws: Ptr,
    blended_order: Ptr,
    cache_a: Ptr,
    cache_b: Ptr,
    count_a: u16,
    count_b: u16,
    previous_blended: u16,
    alpha_ranks: Ptr,
    alpha_ordinals: Ptr,
    alpha_mask: Ptr,
    alpha_rank_key: u16,
    pvs_mask: Ptr,
    pvs_key: u16,
}

pub struct Selection {
    pub opaque: u16,
    pub blended: u16,
}

impl World {
    pub fn new() -> Self {
        let groups = malloc(GROUP_SCRATCH_WORDS);
        let mut opaque_draws = Ptr::from_addr(0);
        let mut blended_draws = Ptr::from_addr(0);
        let mut blended_order = Ptr::from_addr(0);
        let mut alpha_ranks = Ptr::from_addr(0);
        let mut alpha_ordinals = Ptr::from_addr(0);
        let mut alpha_mask = Ptr::from_addr(0);
        if OPAQUE_COUNT != 0 {
            opaque_draws = malloc(OPAQUE_COUNT * OPAQUE_DRAW_WORDS);
        }
        if GROUP_COUNT != OPAQUE_COUNT {
            blended_draws = malloc((GROUP_COUNT - OPAQUE_COUNT) * 6);
            blended_order = malloc(GROUP_COUNT - OPAQUE_COUNT);
            alpha_ranks = malloc(GROUP_COUNT - OPAQUE_COUNT);
            alpha_ordinals = malloc(GROUP_COUNT - OPAQUE_COUNT);
            alpha_mask = malloc((GROUP_COUNT - OPAQUE_COUNT + 15) / 16);
        }
        let state = RenderStateDesc::opaque(Lighting::ambient(fix32::one()))
            .with_filter(TextureFilter::Bilinear);
        let mut cache_a = Ptr::from_addr(0);
        let mut cache_b = Ptr::from_addr(0);
        if TANKRACER_REUSE != 0 {
            cache_a = malloc(GROUP_COUNT * 3);
            cache_b = malloc(GROUP_COUNT * 3);
        }
        let mesh = Mesh::from_native(TRACK_MESH);
        let unlit = UnlitBank::new(mesh.ptr());
        drop(mesh);
        let mut world = Self {
            unlit,
            bindings: Ptr::from_addr(0),
            transform: Transform::new(Pose::identity()),
            camera: Camera::new(CameraDesc::new(
                Vec3::zero(),
                Vec3::zero(),
                Projection::from_focal_pixels(fix!(256), FAR),
            )),
            material: Material::new(MaterialDesc::diffuse(Rgba8::white()).with_unlit(true)),
            opaque: RenderState::new(state),
            blended: RenderState::new(
                state
                    .with_depth(DepthFunc::LessEqual, false)
                    .with_blend(BlendMode::SrcOver),
            ),
            groups,
            opaque_draws,
            opaque_mask: malloc(OPAQUE_MASK_WORDS),
            blended_draws,
            blended_order,
            cache_a,
            cache_b,
            count_a: 0,
            count_b: 0,
            previous_blended: 0,
            alpha_ranks,
            alpha_ordinals,
            alpha_mask,
            alpha_rank_key: 65535,
            pvs_mask: malloc(MAP_PVS_MASK_WORDS),
            pvs_key: 65535,
        };
        world.prepare_bindings();
        world
    }

    fn prepare_bindings(&mut self) {
        let words = unsafe { self.groups.as_u16_array() };
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
                let material = words[i * GROUP_WORDS + 11] & 32767;
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
                let key = words[at + 11];
                let material = key & 32767;
                let index = material * 2 + (key >> 15);
                if map[index] == 0 {
                    self.set_material(key);
                    let mut state = self.opaque.ptr();
                    if words[at + 12] >= OPAQUE_COUNT {
                        state = self.blended.ptr();
                    }
                    let texture_id = MAP_MATERIALS[material * 4];
                    if texture_id == 0 {
                        map[index] = self.unlit.add_colored(state, self.material.ptr());
                    } else {
                        let texture = map_texture(texture_id - 1);
                        let mut view = texture.view();
                        view.set_min_mip_size(MAP_MATERIALS[material * 4 + 1] >> 1);
                        if MAP_MATERIALS[material * 4 + 1] != 0 {
                            view.set_address_mode(TextureAddressMode::ClampToEdge);
                        }
                        map[index] =
                            self.unlit
                                .add(state, self.material.ptr(), &view, key & 32768 != 0);
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
        let selected = self.select_frame(frame);
        self.record_unlit(commands, bank, color, depth, &selected);
    }

    pub fn select_frame(&mut self, frame: u16) -> Selection {
        let at = frame * 24;
        let data: Buf<u16, 24> = Buf::new([0; 24]);
        let rows = data.as_array();
        if !gpu_window_asset_read(rows, MAP_CAMERA_DATA.first_sector, at, 24) {
            halt(0x7a12);
        }
        let row0 = row(rows, 0);
        let row1 = row(rows, 8);
        let row2 = row(rows, 16);
        let matrix = Mat3x4 { row0, row1, row2 };
        let x_axis = absolute_basis(row0);
        let y_axis = absolute_basis(row1);
        let z_axis = absolute_basis(row2);
        let view = View {
            matrix,
            absolute: Mat3x3 {
                row0: x_axis,
                row1: y_axis,
                row2: z_axis,
            },
        };
        self.camera.set_view(matrix);
        self.select(&view)
    }

    fn record_unlit(
        &mut self,
        commands: &mut CommandBuffer,
        bank: u16,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
        selected: &Selection,
    ) {
        let count = selected.opaque + selected.blended;
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
                commands.patch_unlit(&self.unlit, self.camera.ptr(), color, depth);
                return;
            }
        }
        commands.reset();
        commands.prepare_unlit(&mut self.unlit, self.camera.ptr(), self.transform.ptr());
        {
            let mut pass = commands.pass(
                color,
                depth,
                // Static-world sky [0.48, 0.62, 0.72], rounded to RGB565.
                ColorLoad::clear(Rgb565::from_raw(0x7cf6)),
                DepthLoad::clear_far(),
            );
            let opaque = unsafe { self.opaque_draws.as_u16_array() };
            self.draw_list(&mut pass, opaque, selected.opaque, OPAQUE_DRAW_WORDS);
            if GROUP_COUNT != OPAQUE_COUNT {
                let blended = unsafe { self.blended_draws.as_u16_array() };
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

    pub fn opaque_span(&self, index: u16) -> DrawSpan {
        let words = unsafe { self.opaque_draws.as_u16_array() };
        let mut at = index * OPAQUE_DRAW_WORDS;
        let mut item: u16 = 0;
        let mut page: u16 = 0;
        if TANKRACER_PREPARED != 0 {
            let order = OPAQUE_COUNT * 3 + index * 2;
            item = words[order];
            page = words[order + 1];
            at = item * 3;
        }
        DrawSpan {
            first: words[at],
            count: words[at + 1],
            material: words[at + 2],
            item,
            page,
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

    pub fn record_static_span(&mut self, part: &mut PreparedPart, mesh: MeshPtr, material: u16) {
        self.set_material(material);
        part.set_state(self.opaque.ptr());
        let at = (material & 32767) * 4;
        let texture_id = MAP_MATERIALS[at];
        if texture_id == 0 {
            part.draw_colored(self.transform.ptr(), mesh, self.material.ptr());
        } else {
            let texture = map_texture(texture_id - 1);
            let mut view = texture.view();
            view.set_min_mip_size(MAP_MATERIALS[at + 1] >> 1);
            if MAP_MATERIALS[at + 1] != 0 {
                view.set_address_mode(TextureAddressMode::ClampToEdge);
            }
            part.draw(self.transform.ptr(), mesh, self.material.ptr(), &view);
        }
    }

    // One actual opaque group establishes the checked Target/View prefix.
    // Subsequent opaque work comes from immutable, marked Prepared storage.
    pub fn record_prefix(
        &mut self,
        commands: &mut CommandBuffer,
        mesh: MeshPtr,
        seed: &DrawSpan,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
    ) {
        // The list owner supplies a freshly acquired, empty command buffer.
        let mut pass = commands.pass(
            color,
            depth,
            ColorLoad::clear(Rgb565::from_raw(0x7cf6)),
            DepthLoad::clear_far(),
        );
        pass.set_camera(self.camera.ptr());
        pass.set_state(self.opaque.ptr());
        if seed.count != 0 {
            self.set_material(seed.material);
            let at = (seed.material & 32767) * 4;
            let texture_id = MAP_MATERIALS[at];
            if texture_id == 0 {
                pass.draw_colored(self.transform.ptr(), mesh, self.material.ptr());
            } else {
                let texture = map_texture(texture_id - 1);
                let mut view = texture.view();
                view.set_min_mip_size(MAP_MATERIALS[at + 1] >> 1);
                if MAP_MATERIALS[at + 1] != 0 {
                    view.set_address_mode(TextureAddressMode::ClampToEdge);
                }
                pass.draw(self.transform.ptr(), mesh, self.material.ptr(), &view);
            }
        }
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
        commands.prepare_unlit(&mut self.unlit, self.camera.ptr(), self.transform.ptr());
        let mut pass = commands.pass(color, depth, ColorLoad::load(), DepthLoad::load());
        let map = unsafe { self.bindings.as_u16_array() };
        let mut index = first;
        while index < selected.opaque {
            let span = self.opaque_span(index);
            let binding = map[(span.material & 32767) * 2 + (span.material >> 15)];
            pass.draw_unlit(&self.unlit, binding, span.first, span.count);
            index += 1;
        }
        if selected.blended != 0 {
            let blended = unsafe { self.blended_draws.as_u16_array() };
            self.draw_list(&mut pass, blended, selected.blended, 6);
        }
    }

    fn cache_draws(&mut self, cache: Ptr, selected: &Selection, compare: bool) -> bool {
        let mut saved = unsafe { cache.as_u16_array() };
        let opaque = unsafe { self.opaque_draws.as_u16_array() };
        let blended = unsafe { self.blended_draws.as_u16_array() };
        let order = unsafe { self.blended_order.as_u16_array() };
        let mut same = compare;
        let mut i: u16 = 0;
        while i < selected.opaque + selected.blended {
            let mut input = opaque;
            let mut at = i * 3;
            if i >= selected.opaque {
                input = blended;
                at = order[i - selected.opaque] * 6;
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

    fn set_material(&mut self, material: u16) {
        let at = (material & 32767) * 4;
        let rg = MAP_MATERIALS[at + 2];
        let ba = MAP_MATERIALS[at + 3];
        self.material.set_uniform_color(Rgba8 {
            r: rg as u8,
            g: (rg >> 8) as u8,
            b: ba as u8,
            a: (ba >> 8) as u8,
        });
    }

    fn select(&mut self, view: &View) -> Selection {
        let words = unsafe { self.groups.as_u16_array() };
        let mut mask = unsafe { self.opaque_mask.as_u16_array() };
        let mut bucket: u16 = 0;
        while bucket < OPAQUE_MASK_WORDS {
            mask[bucket] = 0;
            bucket += 1;
        }
        let mut blended = unsafe { self.blended_draws.as_u16_array() };
        let mut result = Selection {
            opaque: 0,
            blended: 0,
        };
        let mut same_blended = self.previous_blended != 0;
        let mut position = RENDER_ROUTE_POSITION.as_array();
        position[2u16] = 65535;
        let mut route_s = -fix32::one();
        if TANKRACER_ROUTE_CANDIDATES != 0 && TANKRACER_POLICY != 0 && RING_SEGMENT_COUNT != 0 {
            route_s = route_position(view);
        }
        position[0u16] = route_s.lo_bits();
        position[1u16] = route_s.hi_bits();
        let route_active = route_s >= fix32::zero();
        let use_pvs = MAP_PVS_ENABLED != 0
            && route_active
            && position[2u16] != 65535
            && view.matrix.row2.y.abs() <= fix!(0.3255615234375);
        let pvs = unsafe { self.pvs_mask.as_u16_array() };
        if use_pvs {
            let mut yaw = alpha_direction_key(view) / ALPHA_PITCH_COUNT;
            if MAP_PVS_YAW_SECTORS != 16 {
                yaw = pvs_heading(view);
            }
            let key = position[2u16] * MAP_PVS_YAW_SECTORS + yaw;
            if key != self.pvs_key {
                let entry: Buf<u16, 1> = Buf::new([0]);
                if !gpu_window_asset_read(entry.as_array(), MAP_VIEW_PVS.first_sector, key, 1)
                    || !gpu_window_asset_read(
                        pvs,
                        MAP_VIEW_PVS.first_sector
                            + MAP_PVS_MASK_FIRST_SECTOR
                            + entry[0u16] * MAP_PVS_STRIDE_SECTORS,
                        0,
                        MAP_PVS_MASK_WORDS,
                    )
                {
                    halt(0x7b43);
                }
                self.pvs_key = key;
            }
        }
        let mut coarse = MAP_CLUSTERS.as_array();
        let mut source = MAP_GROUP_DATA.first_sector;
        let mut stride = GROUP_WORDS;
        let mut first: u16 = 0;
        let mut remaining: u16 = 0;
        let mut cluster: u16 = 0;
        if route_active {
            let directory = MAP_RING_SEGMENTS.as_array();
            let segment = route_s.to_int() as u16;
            first = directory[segment * RING_SEGMENT_WORDS];
            remaining = directory[segment * RING_SEGMENT_WORDS + 1];
            coarse = MAP_RING_CHUNKS.as_array();
            source = MAP_RING_GROUP_DATA.first_sector;
            stride = RING_GROUP_WORDS;
        }
        while (route_active && remaining != 0) || (!route_active && cluster < CLUSTER_COUNT) {
            let mut header = cluster * CLUSTER_WORDS;
            let mut offset: u16 = 0;
            let mut count: u16 = 0;
            if route_active {
                header = (first / RING_CHUNK_GROUPS) * CLUSTER_WORDS;
                count = coarse[header + 14] + coarse[header + 15] - first;
                if count > remaining {
                    count = remaining;
                }
                offset = first * RING_GROUP_WORDS;
            } else {
                offset = coarse[header + 14];
                count = coarse[header + 15];
            }
            let mut classification: u16 = 2;
            if use_pvs && (pvs[(first >> 5) * 2] | pvs[(first >> 5) * 2 + 1]) == 0 {
                classification = 0;
            } else if TANKRACER_POLICY != 0 {
                classification = classify_cluster(view, coarse, header);
            }
            if classification != 0 {
                if !gpu_window_asset_read(words, source, offset, count * stride) {
                    halt(0x7a12);
                }
                let mut index: u16 = 0;
                while index < count {
                    let record = index * stride;
                    if !use_pvs || pvs[(first + index) >> 4] & (1u16 << ((first + index) & 15)) != 0
                    {
                        let bound = group_bound(words, record);
                        let depth = -project(&view.matrix.row2, &bound);
                        if classification == 2 || visible_group(view, words, record, &bound, depth)
                        {
                            let mut span = record + 7;
                            if TANKRACER_POLICY == 2 && depth - bound.w > LOD_DISTANCE {
                                span = record + 9;
                            }
                            let draw_first = words[span];
                            let draw_count = words[span + 1];
                            let draw_material = words[record + 11];
                            if words[record + 12] >= OPAQUE_COUNT {
                                let mut output = blended;
                                let target = result.blended * 6;
                                if same_blended {
                                    if result.blended >= self.previous_blended {
                                        same_blended = false;
                                    } else if output[target + 3] != words[record + 12] {
                                        same_blended = false;
                                    }
                                }
                                output[target + 3] = words[record + 12];
                                output[target + 4] = depth.lo_bits();
                                output[target + 5] = depth.hi_bits();
                                output[target] = draw_first;
                                output[target + 1] = draw_count;
                                output[target + 2] = draw_material;
                                result.blended += 1;
                            } else {
                                if TANKRACER_PREPARED != 0 {
                                    let item = MAP_OPAQUE_RING_INDEX[words[record + 12]];
                                    let mut bits: u16 = 1;
                                    if draw_first != words[record + 7] {
                                        bits = 3;
                                    }
                                    mask[item >> 3] |= bits << ((item & 7) * 2);
                                    // Retain the exact selected range already loaded by culling.
                                    // The remaining two words per group hold the ordered indices.
                                    let mut spans = unsafe { self.opaque_draws.as_u16_array() };
                                    spans[item * 3] = draw_first;
                                    spans[item * 3 + 1] = draw_count;
                                    spans[item * 3 + 2] = draw_material;
                                } else {
                                    mask[draw_first >> 4] |= 1u16 << (draw_first & 15);
                                }
                            }
                        }
                    }
                    index += 1;
                }
            }
            if route_active {
                remaining -= count;
                first += count;
                if first == RING_GROUP_COUNT {
                    first = 0;
                }
            } else {
                cluster += 1;
            }
        }
        result.opaque = self.ordered_opaque(words);
        let order = unsafe { self.blended_order.as_u16_array() };
        same_blended = same_blended && result.blended == self.previous_blended;
        self.previous_blended = result.blended;
        if !same_blended {
            self.seed_blended(view, blended, order, result.blended);
            repair_blended_order(blended, order, result.blended);
        } else if !blended_ordered(blended, order, result.blended) {
            repair_blended_order(blended, order, result.blended);
        }
        result
    }

    fn seed_blended(
        &mut self,
        view: &View,
        records: Array<u16>,
        mut order: Array<u16>,
        count: u16,
    ) {
        if count < 2 {
            if count == 1 {
                order[0u16] = 0;
            }
            return;
        }
        let key = alpha_direction_key(view);
        let ranks = unsafe { self.alpha_ranks.as_u16_array() };
        if key != self.alpha_rank_key {
            if !gpu_window_asset_read(
                ranks,
                MAP_ALPHA_RANKS.first_sector + key * ALPHA_RANK_SECTORS,
                0,
                GROUP_COUNT - OPAQUE_COUNT,
            ) {
                halt(0x7b42);
            }
            self.alpha_rank_key = key;
        }
        let mut mask = unsafe { self.alpha_mask.as_u16_array() };
        let mut ordinals = unsafe { self.alpha_ordinals.as_u16_array() };
        let mut bucket: u16 = 0;
        while bucket < (GROUP_COUNT - OPAQUE_COUNT + 15) / 16 {
            mask[bucket] = 0;
            bucket += 1;
        }
        let mut index: u16 = 0;
        while index < count {
            let rank = ranks[records[index * 6 + 3] - OPAQUE_COUNT];
            ordinals[rank] = index;
            mask[rank >> 4] |= 1u16 << (rank & 15);
            index += 1;
        }
        bucket = 0;
        index = 0;
        while bucket < (GROUP_COUNT - OPAQUE_COUNT + 15) / 16 {
            let mut bits = mask[bucket];
            let mut at = bucket * 16;
            while bits != 0 {
                if bits & 1 != 0 {
                    order[index] = ordinals[at];
                    index += 1;
                }
                at += 1;
                bits >>= 1;
            }
            bucket += 1;
        }
    }

    fn ordered_opaque(&mut self, words: Array<u16>) -> u16 {
        if TANKRACER_PREPARED != 0 {
            return self.ordered_ring();
        }
        let mask = unsafe { self.opaque_mask.as_u16_array() };
        let mut output = unsafe { self.opaque_draws.as_u16_array() };
        let mut count: u16 = 0;
        let mut bucket: u16 = 0;
        while bucket < OPAQUE_MASK_WORDS {
            let mut bits = mask[bucket];
            if bits != 0 {
                if !gpu_window_asset_read(words, MAP_OPAQUE_SPANS.first_sector, bucket * 48, 48) {
                    halt(0x7a13);
                }
                let mut at: u16 = 0;
                while bits != 0 {
                    if bits & 1 != 0 {
                        let first = words[at];
                        let length = words[at + 1];
                        let material = words[at + 2];
                        if count != 0
                            && output[(count - 1) * 3 + 2] == material
                            && output[(count - 1) * 3] + output[(count - 1) * 3 + 1] == first
                        {
                            output[(count - 1) * 3 + 1] += length;
                        } else {
                            output[count * 3] = first;
                            output[count * 3 + 1] = length;
                            output[count * 3 + 2] = material;
                            count += 1;
                        }
                    }
                    bits >>= 1;
                    at += 3;
                }
            }
            bucket += 1;
        }
        count
    }

    fn ordered_ring(&mut self) -> u16 {
        let mask = unsafe { self.opaque_mask.as_u16_array() };
        let mut output = unsafe { self.opaque_draws.as_u16_array() };
        let mut count: u16 = 0;
        let mut variant: u16 = 0;
        while variant < 2 {
            let mut bucket: u16 = 0;
            while bucket < OPAQUE_MASK_WORDS {
                let candidates = mask[bucket];
                let mut bits = candidates & (65535 ^ (candidates >> 1)) & 21845;
                if variant != 0 {
                    bits = (candidates >> 1) & 21845;
                }
                if bits != 0 {
                    let mut index: u16 = 0;
                    while bits != 0 {
                        if bits & 1 != 0 {
                            let item = bucket * 8 + index;
                            let at = OPAQUE_COUNT * 3 + count * 2;
                            output[at] = item;
                            output[at + 1] =
                                MAP_OPAQUE_RING_PAGE[item] + variant * MAP_PREPARED_RING_PAGES;
                            count += 1;
                        }
                        bits >>= 2;
                        index += 1;
                    }
                }
                bucket += 1;
            }
            variant += 1;
        }
        count
    }

    fn draw_list(&mut self, pass: &mut Pass, words: Array<u16>, count: u16, stride: u16) {
        let order = unsafe { self.blended_order.as_u16_array() };
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
                let span = self.opaque_span(i);
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

// Quantize the horizontal view direction for the conservative candidate mask.
fn pvs_heading(view: &View) -> u16 {
    let x = view.matrix.row2.x;
    let z = view.matrix.row2.z;
    let axes = MAP_PVS_QUADRANT.as_array();
    let mut sector: u16 = 0;
    let mut best = z.abs();
    let mut i: u16 = 1;
    while i <= MAP_PVS_YAW_SECTORS / 4 {
        let value = x.abs() * fixed(axes, i * 4) + z.abs() * fixed(axes, i * 4 + 2);
        if value > best {
            best = value;
            sector = i;
        }
        i += 1;
    }
    if z < fix32::zero() {
        if x < fix32::zero() {
            sector = MAP_PVS_YAW_SECTORS / 2 + sector;
        } else {
            sector = MAP_PVS_YAW_SECTORS / 2 - sector;
        }
    } else if x < fix32::zero() {
        sector = (MAP_PVS_YAW_SECTORS - sector) % MAP_PVS_YAW_SECTORS;
    }
    sector
}

// Camera translation is common to all depths. Direction-only ranks seed an
// exact sort; they never decide visibility or replace the stable-ID tie break.
fn alpha_direction_key(view: &View) -> u16 {
    let x = view.matrix.row2.x;
    let z = view.matrix.row2.z;
    let ax = x.abs();
    let az = z.abs();
    let mut yaw: u16 = 0;
    let mut best = az;
    let a = ax * fix!(0.3826904296875) + az * fix!(0.9238739013671875);
    if a > best {
        yaw = 1;
        best = a;
    }
    let diagonal = (ax + az) * fix!(0.7071075439453125);
    if diagonal > best {
        yaw = 2;
        best = diagonal;
    }
    let b = ax * fix!(0.9238739013671875) + az * fix!(0.3826904296875);
    if b > best {
        yaw = 3;
        best = b;
    }
    if ax > best {
        yaw = 4;
    }
    if z < fix32::zero() {
        if x < fix32::zero() {
            yaw = 8 + yaw;
        } else {
            yaw = 8 - yaw;
        }
    } else if x < fix32::zero() {
        yaw = (16 - yaw) & 15;
    }
    let thresholds = MAP_ALPHA_PITCH_THRESHOLDS.as_array();
    let mut pitch: u16 = 0;
    while pitch + 1 < ALPHA_PITCH_COUNT && view.matrix.row2.y > fixed(thresholds, pitch * 2) {
        pitch += 1;
    }
    yaw * ALPHA_PITCH_COUNT + pitch
}

fn repair_blended_order(records: Array<u16>, mut order: Array<u16>, count: u16) {
    let mut budget = count * 4;
    let mut i: u16 = 1;
    while i < count {
        let record = order[i];
        let group = records[record * 6 + 3];
        let depth = fixed(records, record * 6 + 4);
        let mut at = i;
        while at != 0 {
            let previous = order[at - 1] * 6;
            let previous_depth = fixed(records, previous + 4);
            if previous_depth > depth || (previous_depth == depth && records[previous + 3] < group)
            {
                break;
            }
            order[at] = order[at - 1];
            at -= 1;
            budget -= 1;
            if budget == 0 {
                order[at] = record;
                sort_back_to_front(records, order, count);
                return;
            }
        }
        order[at] = record;
        i += 1;
    }
}

fn blended_ordered(records: Array<u16>, order: Array<u16>, count: u16) -> bool {
    if count < 2 {
        return true;
    }
    let first = order[0u16] * 6;
    let mut previous_depth = fixed(records, first + 4);
    let mut previous_group = records[first + 3];
    let mut index: u16 = 1;
    while index < count {
        let at = order[index] * 6;
        let depth = fixed(records, at + 4);
        let group = records[at + 3];
        if previous_depth < depth || (previous_depth == depth && previous_group > group) {
            return false;
        }
        previous_depth = depth;
        previous_group = group;
        index += 1;
    }
    true
}

fn sort_back_to_front(records: Array<u16>, mut order: Array<u16>, count: u16) {
    let mut index: u16 = 0;
    while index < count {
        order[index] = index;
        index += 1;
    }
    let mut gap: u16 = 1;
    while gap * 3 + 1 <= count / 3 {
        gap = gap * 3 + 1;
    }
    while gap > 0 {
        let mut i = gap;
        while i < count {
            let record = order[i];
            let group = records[record * 6 + 3];
            let depth = fixed(records, record * 6 + 4);
            let mut at = i;
            while at >= gap {
                let previous = order[at - gap] * 6;
                let previous_depth = fixed(records, previous + 4);
                if previous_depth > depth
                    || (previous_depth == depth && records[previous + 3] < group)
                {
                    break;
                }
                order[at] = order[at - gap];
                at -= gap;
            }
            order[at] = record;
            i += 1;
        }
        gap /= 3;
    }
}

fn fixed(words: Array<u16>, at: u16) -> fix32 {
    fix32::from_words(words[at], words[at + 1])
}

fn row(words: Array<u16>, at: u16) -> Vec4 {
    Vec4 {
        x: fixed(words, at),
        y: fixed(words, at + 2),
        z: fixed(words, at + 4),
        w: fixed(words, at + 6),
    }
}

fn project(axis: &Vec4, bound: &Vec4) -> fix32 {
    fdot(
        vec3::new(axis.x, axis.y, axis.z),
        vec3::new(bound.x, bound.y, bound.z),
    ) + axis.w
}

fn absolute_basis(axis: Vec4) -> Vec3 {
    Vec3::new(axis.x.abs(), axis.y.abs(), axis.z.abs())
}

fn extent_at(basis: &Vec3, extent: vec3) -> fix32 {
    fdot(vec3::new(basis.x, basis.y, basis.z), extent)
}

fn compact_signed(word: u16) -> fix32 {
    fix32::from_words(word << 9, ((word as i16) >> 7) as u16)
}

fn compact_unsigned(word: u16) -> fix32 {
    fix32::from_words(word << 9, word >> 7)
}

fn group_bound(words: Array<u16>, at: u16) -> Vec4 {
    Vec4 {
        x: compact_signed(words[at]),
        y: compact_signed(words[at + 1]),
        z: compact_signed(words[at + 2]),
        w: compact_unsigned(words[at + 6]),
    }
}

fn center_visible(view: &View, bound: &Vec4, depth: fix32) -> bool {
    if depth <= fix32::zero() || depth > DISPLAY_DISTANCE {
        return false;
    }
    if project(&view.matrix.row0, bound).abs() > depth * X_PER_DEPTH {
        return false;
    }
    project(&view.matrix.row1, bound).abs() <= depth * Y_PER_DEPTH
}

fn intersects(view: &View, bound: &Vec4, extent: vec3, depth: fix32) -> bool {
    let z_extent = extent_at(&view.absolute.row2, extent);
    let farthest = depth + z_extent;
    if farthest <= fix32::zero() || depth - bound.w > DISPLAY_DISTANCE {
        return false;
    }
    if project(&view.matrix.row0, bound).abs()
        > farthest * X_PER_DEPTH + extent_at(&view.absolute.row0, extent)
    {
        return false;
    }
    project(&view.matrix.row1, bound).abs()
        <= farthest * Y_PER_DEPTH + extent_at(&view.absolute.row1, extent)
}

fn visible_group(view: &View, words: Array<u16>, at: u16, bound: &Vec4, depth: fix32) -> bool {
    if center_visible(view, bound, depth) {
        return true;
    }
    intersects(
        view,
        bound,
        vec3::new(
            compact_unsigned(words[at + 3]),
            compact_unsigned(words[at + 4]),
            compact_unsigned(words[at + 5]),
        ),
        depth,
    )
}

fn classify_cluster(view: &View, words: Array<u16>, at: u16) -> u16 {
    let bound = row(words, at);
    let depth = -project(&view.matrix.row2, &bound);
    let extent = vec3::new(
        fixed(words, at + 8),
        fixed(words, at + 10),
        fixed(words, at + 12),
    );
    // Keep projected bounds in scalar locals for both reject and full-accept
    // tests. Boundary groups still use the original per-group visibility path.
    let z_extent = extent_at(&view.absolute.row2, extent);
    let farthest = depth + z_extent;
    if farthest <= fix32::zero() || depth - bound.w > DISPLAY_DISTANCE {
        return 0;
    }
    let x = project(&view.matrix.row0, &bound).abs();
    let x_extent = extent_at(&view.absolute.row0, extent);
    if x > farthest * X_PER_DEPTH + x_extent {
        return 0;
    }
    let y = project(&view.matrix.row1, &bound).abs();
    let y_extent = extent_at(&view.absolute.row1, extent);
    if y > farthest * Y_PER_DEPTH + y_extent {
        return 0;
    }
    let nearest = depth - z_extent;
    if nearest > fix32::zero()
        && depth + bound.w <= DISPLAY_DISTANCE
        && x + x_extent <= nearest * X_PER_DEPTH
        && y + y_extent <= nearest * Y_PER_DEPTH
    {
        return 2;
    }
    1
}

// Render-only s=segment+t. Any accepted point is on a real segment; nearest
// accuracy is unnecessary because the corridor check is the enclosure proof.
fn route_position(view: &View) -> fix32 {
    let translation = vec3::new(view.matrix.row0.w, view.matrix.row1.w, view.matrix.row2.w);
    let camera_x = -fdot(
        vec3::new(view.matrix.row0.x, view.matrix.row1.x, view.matrix.row2.x),
        translation,
    );
    let camera_z = -fdot(
        vec3::new(view.matrix.row0.z, view.matrix.row1.z, view.matrix.row2.z),
        translation,
    );
    let corridor = fix32::from_words(MAP_RING_ACCEPT_LOW, MAP_RING_ACCEPT_HIGH);
    let limit = fix32::from_words(MAP_RING_ACCEPT_SQUARED_LOW, MAP_RING_ACCEPT_SQUARED_HIGH);
    let directory = MAP_RING_SEGMENTS.as_array();
    let mut best = -fix32::one();
    let mut best_distance = limit;
    let lists = MAP_ROUTE_LOCATION_LISTS.as_array();
    let mut first: u16 = 1;
    let mut count = RING_SEGMENT_COUNT;
    if ROUTE_LOCATION_GRID != 0 {
        let origin_x = fix32::from_words(ROUTE_LOCATION_ORIGIN_X_LOW, ROUTE_LOCATION_ORIGIN_X_HIGH);
        let origin_z = fix32::from_words(ROUTE_LOCATION_ORIGIN_Z_LOW, ROUTE_LOCATION_ORIGIN_Z_HIGH);
        let end_x = fix32::from_words(ROUTE_LOCATION_END_X_LOW, ROUTE_LOCATION_END_X_HIGH);
        let end_z = fix32::from_words(ROUTE_LOCATION_END_Z_LOW, ROUTE_LOCATION_END_Z_HIGH);
        if camera_x < origin_x || camera_z < origin_z || camera_x >= end_x || camera_z >= end_z {
            return best;
        }
        let inverse = fix32::from_words(ROUTE_LOCATION_INVERSE_LOW, ROUTE_LOCATION_INVERSE_HIGH);
        let x = ((camera_x - origin_x) * inverse).to_int() as u16;
        let z = ((camera_z - origin_z) * inverse).to_int() as u16;
        let cells = MAP_ROUTE_LOCATION_CELLS.as_array();
        let cell = z * ROUTE_LOCATION_COLUMNS + x;
        RENDER_ROUTE_POSITION[2u16] = cell;
        first = cells[cell];
        count = lists[first];
        first += 1;
    }
    let mut index: u16 = 0;
    while index < count {
        let segment = lists[first + index];
        let at = segment * RING_SEGMENT_WORDS;
        let ax = fixed(directory, at + 2);
        let az = fixed(directory, at + 4);
        let dx = fixed(directory, at + 6);
        let dz = fixed(directory, at + 8);
        let rx = camera_x - ax;
        let rz = camera_z - az;
        let mut min_x = fix32::zero();
        let mut max_x = dx;
        let mut min_z = fix32::zero();
        let mut max_z = dz;
        if dx < fix32::zero() {
            min_x = dx;
            max_x = fix32::zero();
        }
        if dz < fix32::zero() {
            min_z = dz;
            max_z = fix32::zero();
        }
        if rx >= min_x - corridor
            && rx <= max_x + corridor
            && rz >= min_z - corridor
            && rz <= max_z + corridor
        {
            let mut t = fdot(
                vec3::new(rx, fix32::zero(), rz),
                vec3::new(dx, fix32::zero(), dz),
            ) * fixed(directory, at + 10);
            if t < fix32::zero() {
                t = fix32::zero();
            }
            if t > fix32::one() {
                t = fix32::one();
            }
            let ex = camera_x - (ax + dx * t);
            let ez = camera_z - (az + dz * t);
            if ex.abs() <= corridor && ez.abs() <= corridor {
                let delta = vec3::new(ex, fix32::zero(), ez);
                let distance = fdot(delta, delta);
                if distance <= limit && (best < fix32::zero() || distance < best_distance) {
                    best = fix32::from_int(segment as i16) + t;
                    if best >= fix32::from_int(RING_SEGMENT_COUNT as i16) {
                        best = fix32::zero();
                    }
                    best_distance = distance;
                }
            }
        }
        index += 1;
    }
    best
}
