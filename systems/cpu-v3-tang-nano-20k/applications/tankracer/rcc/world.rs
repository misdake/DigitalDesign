//! A baked world owns ranges, materials, and bounded draw-list scratch.
use crate::dsl_rt::*;
mod boot_resources;
mod gpu_api;
mod gpu_window;
mod map_stage;
mod pvs;
use boot_resources::*;
use gpu_api::*;
use gpu_window::{gpu_memory_faulted, gpu_window_asset_read};

const FAR: fix32 = fix32::from_words(MAP_FAR_LOW, MAP_FAR_HIGH);
const DISPLAY_DISTANCE: fix32 = fix32::from_words(MAP_DISPLAY_LOW, MAP_DISPLAY_HIGH);
const X_PER_DEPTH: fix32 = fix!(0.78515625);
const Y_PER_DEPTH: fix32 = fix!(0.47265625);

// Current render-only s=segment+t; negative means cluster fallback.
static RENDER_ROUTE_POSITION: Buf<u16, 3> = Buf::new([0, 65535, 65535]);

struct View {
    matrix: Mat3x4,
    absolute: Mat3x3,
}

struct RangeVisibility {
    anchor: Mat3x4,
    axes: Ptr,
    valid: bool,
    new_anchor: bool,
    // Per chunk: covered first group/count and the two compact list endpoints.
    // A visible endpoint of 65535 invalidates only the sphere classification.
    chunks: Ptr,
    all: Ptr,
    visible: Ptr,
    fringe: Ptr,
}

struct RingChunk {
    first: u16,
    count: u16,
    classification: u16,
}

const MOTION: fix32 = fix!(2.25);
const MOTION_X: fix32 = fix!(4.01953125);
const MOTION_Y: fix32 = fix!(3.31640625);

pub struct DrawSpan {
    pub first: u16,
    pub count: u16,
    pub material: u16,
    pub item: u16,
    pub page: u16,
}

pub struct World {
    pub(super) transform: Transform,
    pub(super) camera: Camera,
    pub(super) material: Material,
    pub(super) opaque: RenderState,
    pub(super) blended: RenderState,
    pub(super) cutout: RenderState,
    pub(super) groups: Ptr,
    // Prepared: three selected words per ring item, then two ordered words
    // per visible item. Ordinary recording uses first/count/material triples.
    pub(super) opaque_draws: Ptr,
    opaque_mask: Ptr,
    pub(super) blended_draws: Ptr,
    pub(super) blended_order: Ptr,
    previous_blended: u16,
    alpha_ranks: Ptr,
    alpha_ordinals: Ptr,
    alpha_mask: Ptr,
    alpha_rank_key: u16,
    pvs_mask: Ptr,
    pvs_key: u16,
    range_visibility: RangeVisibility,
}

pub struct Selection {
    pub opaque: u16,
    pub cutout: u16,
    pub blended: u16,
}

impl World {
    pub fn new() -> Self {
        map_stage::init();
        let groups = malloc(GROUP_SCRATCH_WORDS);
        let mut opaque_draws = Ptr::from_addr(0);
        let mut blended_draws = Ptr::from_addr(0);
        let mut blended_order = Ptr::from_addr(0);
        let mut alpha_ranks = Ptr::from_addr(0);
        let mut alpha_ordinals = Ptr::from_addr(0);
        let mut alpha_mask = Ptr::from_addr(0);
        if OPAQUE_COUNT != 0 && RANGE_HYBRID == 0 {
            opaque_draws = malloc(OPAQUE_COUNT * OPAQUE_DRAW_WORDS);
        }
        if GROUP_COUNT != OPAQUE_COUNT && RANGE_HYBRID == 0 {
            blended_draws = malloc((GROUP_COUNT - OPAQUE_COUNT) * 6);
            blended_order = malloc(GROUP_COUNT - OPAQUE_COUNT);
            alpha_ranks = malloc(GROUP_COUNT - OPAQUE_COUNT);
            alpha_ordinals = malloc(GROUP_COUNT - OPAQUE_COUNT);
            alpha_mask = malloc((GROUP_COUNT - OPAQUE_COUNT + 15) / 16);
        }
        let state = RenderStateDesc::opaque(Lighting::ambient(fix32::one()))
            .with_filter(TextureFilter::Trilinear);
        let mut world = Self {
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
            cutout: RenderState::new(
                state
                    .with_depth(DepthFunc::LessEqual, true)
                    .with_blend(BlendMode::SrcOver),
            ),
            groups,
            opaque_draws,
            opaque_mask: malloc(OPAQUE_MASK_WORDS),
            blended_draws,
            blended_order,
            previous_blended: 0,
            alpha_ranks,
            alpha_ordinals,
            alpha_mask,
            alpha_rank_key: 65535,
            pvs_mask: pvs::new(),
            pvs_key: 65535,
            range_visibility: RangeVisibility::new(),
        };
        // The derivative estimator has a conservative half-level footprint.
        // This scene uses the one-pixel footprint while retaining full mips.
        world.opaque.set_lod_bias(fix!(-0.5));
        world
    }

    pub fn select_prepared_frame(&mut self, frame: u16) -> Array<u16> {
        let selected = self.select_frame(frame);
        unsafe { self.opaque_mask.as_u16_array() }
    }

    pub fn select_frame(&mut self, frame: u16) -> Selection {
        let saved = map_stage::begin();
        let view = self.load_view(frame);
        let selected = self.select(&view);
        map_stage::end(saved);
        selected
    }

    fn load_view(&mut self, frame: u16) -> View {
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
        view
    }

    pub fn range_key(&mut self, frame: u16) -> u16 {
        let view = self.load_view(frame);
        if view.matrix.row2.y.abs() > fix!(0.3255615234375) {
            return 65535;
        }
        let t = vec3::new(view.matrix.row0.w, view.matrix.row1.w, view.matrix.row2.w);
        let x = -fdot(
            vec3::new(view.matrix.row0.x, view.matrix.row1.x, view.matrix.row2.x),
            t,
        );
        let z = -fdot(
            vec3::new(view.matrix.row0.z, view.matrix.row1.z, view.matrix.row2.z),
            t,
        );
        let origin_x = fix32::from_words(RANGE_ORIGIN_X_LOW, RANGE_ORIGIN_X_HIGH);
        let origin_z = fix32::from_words(RANGE_ORIGIN_Z_LOW, RANGE_ORIGIN_Z_HIGH);
        let inverse = fix32::from_words(RANGE_INVERSE_LOW, RANGE_INVERSE_HIGH);
        if x < origin_x || z < origin_z {
            return 65535;
        }
        let column = ((x - origin_x) * inverse).to_int() as u16;
        let row = ((z - origin_z) * inverse).to_int() as u16;
        if column >= RANGE_COLUMNS || row >= RANGE_ROWS {
            return 65535;
        }
        (row * RANGE_COLUMNS + column) * 32 + pvs_heading(&view)
    }

    pub fn opaque_span(&self, index: u16) -> DrawSpan {
        let words = unsafe { self.opaque_draws.as_u16_array() };
        let mut at = index * OPAQUE_DRAW_WORDS;
        let mut item: u16 = 0;
        let mut page: u16 = 0;
        if TANKRACER_PREPARED != 0 {
            let order = OPAQUE_COUNT * 3 + index;
            item = words[order];
            page = MAP_OPAQUE_RING_PAGE[item];
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

    pub fn record_static_span(&mut self, part: &mut PreparedPart, mesh: MeshPtr, material: u16) {
        self.record_range_span(part, mesh, material, false);
    }

    pub fn record_range_span(
        &mut self,
        part: &mut PreparedPart,
        mesh: MeshPtr,
        material: u16,
        cutout: bool,
    ) {
        self.set_material(material);
        let mut state = self.opaque.ptr();
        if cutout {
            state = self.cutout.ptr();
        }
        part.set_state(state);
        let at = (material & 32767) * 4;
        let texture_id = map_materials()[at];
        if texture_id == 0 {
            part.draw_colored(self.transform.ptr(), mesh, self.material.ptr());
        } else {
            let texture = map_texture(texture_id - 1);
            let mut view = texture.view();
            view.set_min_mip_size((map_materials()[at + 1] >> 1) & 3);
            if map_materials()[at + 1] & 1 != 0 {
                view.set_address_mode(TextureAddressMode::ClampToEdge);
            }
            if material & 32768 != 0 {
                part.draw_billboard(
                    self.transform.ptr(),
                    mesh,
                    self.material.ptr(),
                    &view,
                    BillboardFacing::Vertical,
                );
            } else {
                part.draw(self.transform.ptr(), mesh, self.material.ptr(), &view);
            }
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
        self.draw_range_seed(&mut pass, mesh, seed);
        if RANGE_SHARED_CUTOUT != 0 {
            pass.end_for_list();
        } else {
            drop(pass);
        }
    }

    pub fn record_group_seed(
        &mut self,
        commands: &mut CommandBuffer,
        mesh: MeshPtr,
        seed: &DrawSpan,
    ) {
        let mut pass = commands.continue_list_pass();
        self.draw_range_seed(&mut pass, mesh, seed);
        pass.end_for_list();
    }

    fn draw_range_seed(&mut self, pass: &mut Pass, mesh: MeshPtr, seed: &DrawSpan) {
        let mut state = self.opaque.ptr();
        if seed.item >= OPAQUE_COUNT {
            state = self.cutout.ptr();
        }
        pass.set_state(state);
        if seed.count != 0 {
            self.set_material(seed.material);
            let at = (seed.material & 32767) * 4;
            let texture_id = map_materials()[at];
            if texture_id == 0 {
                pass.draw_colored(self.transform.ptr(), mesh, self.material.ptr());
            } else {
                let texture = map_texture(texture_id - 1);
                let mut view = texture.view();
                view.set_min_mip_size((map_materials()[at + 1] >> 1) & 3);
                if map_materials()[at + 1] & 1 != 0 {
                    view.set_address_mode(TextureAddressMode::ClampToEdge);
                }
                if seed.material & 32768 != 0 {
                    pass.draw_billboard(
                        self.transform.ptr(),
                        mesh,
                        self.material.ptr(),
                        &view,
                        BillboardFacing::Vertical,
                    );
                } else {
                    pass.draw(self.transform.ptr(), mesh, self.material.ptr(), &view);
                }
            }
        }
    }

    pub fn record_range_end(
        &mut self,
        commands: &mut CommandBuffer,
        color: ColorBufferPtr,
        depth: DepthBufferPtr,
    ) {
        let pass = commands.pass(color, depth, ColorLoad::load(), DepthLoad::load());
    }

    pub(super) fn set_material(&mut self, material: u16) {
        let at = (material & 32767) * 4;
        let rg = map_materials()[at + 2];
        let ba = map_materials()[at + 3];
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
            cutout: 0,
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
                if !pvs::load(self.pvs_mask, key) {
                    halt(0x7b43);
                }
                self.pvs_key = key;
            }
        }
        let mut coarse = map_clusters();
        let mut source = MAP_GROUP_DATA.first_sector;
        let mut stride = GROUP_WORDS;
        let mut first: u16 = 0;
        let mut remaining: u16 = 0;
        let mut cluster: u16 = 0;
        let mut incremental = false;
        if route_active {
            let directory = map_ring_segments();
            let segment = route_s.to_int() as u16;
            first = directory[segment * RING_SEGMENT_WORDS];
            remaining = directory[segment * RING_SEGMENT_WORDS + 1];
            coarse = map_ring_chunks();
            source = MAP_RING_GROUP_DATA.first_sector;
            stride = RING_GROUP_WORDS;
            if RANGE_INCREMENTAL != 0 {
                // Bound translation before subtracting Q16.16 rows.
                incremental = bounded_translation(view.matrix.row0.w)
                    && bounded_translation(view.matrix.row1.w)
                    && bounded_translation(view.matrix.row2.w);
                if incremental {
                    self.range_visibility.begin(&view.matrix);
                }
            }
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
            let mut pvs_empty = false;
            if MAP_PVS_COARSE != 0 {
                pvs_empty = pvs[first >> 9] & (1u16 << ((first >> 5) & 15)) == 0;
            } else if use_pvs {
                pvs_empty = (pvs[(first >> 5) * 2] | pvs[(first >> 5) * 2 + 1]) == 0;
            }
            if use_pvs && pvs_empty {
                classification = 0;
            } else if TANKRACER_POLICY != 0 {
                classification = classify_cluster(view, coarse, header);
            }
            if incremental && classification != 0 {
                let chunk = RingChunk {
                    first,
                    count,
                    classification,
                };
                self.range_visibility.select(&chunk, words, mask);
            } else if classification != 0 {
                if !gpu_window_asset_read(words, source, offset, count * stride) {
                    halt(0x7a12);
                }
                let mut index: u16 = 0;
                while index < count {
                    let record = index * stride;
                    if !use_pvs
                        || MAP_PVS_COARSE != 0
                        || pvs[(first + index) >> 4] & (1u16 << ((first + index) & 15)) != 0
                    {
                        let bound = group_bound(words, record);
                        let depth = -project(&view.matrix.row2, &bound);
                        if classification == 2 || visible_group(view, words, record, &bound, depth)
                        {
                            if RANGE_HYBRID != 0 {
                                let unit = words[record + 4];
                                mask[unit >> 4] |= 1u16 << (unit & 15);
                            } else {
                                let draw_first = words[record + 7];
                                let draw_count = words[record + 8];
                                let draw_material = words[record + 9];
                                if words[record + 10] >= OPAQUE_COUNT {
                                    let mut output = blended;
                                    let cutout =
                                        map_materials()[(draw_material & 32767) * 4 + 1] & 8 != 0;
                                    let mut target = result.blended * 6;
                                    if cutout {
                                        target =
                                            (GROUP_COUNT - OPAQUE_COUNT - 1 - result.cutout) * 6;
                                    }
                                    if !cutout && same_blended {
                                        if result.blended >= self.previous_blended {
                                            same_blended = false;
                                        } else if output[target + 3] != words[record + 10] {
                                            same_blended = false;
                                        }
                                    }
                                    output[target + 3] = words[record + 10];
                                    output[target + 4] = depth.lo_bits();
                                    output[target + 5] = depth.hi_bits();
                                    output[target] = draw_first;
                                    output[target + 1] = draw_count;
                                    output[target + 2] = draw_material;
                                    if cutout {
                                        result.cutout += 1;
                                    } else {
                                        result.blended += 1;
                                    }
                                } else {
                                    if TANKRACER_PREPARED != 0 {
                                        let item = MAP_OPAQUE_RING_INDEX[words[record + 10]];
                                        mask[item >> 4] |= 1u16 << (item & 15);
                                        // Retain the exact selected range already loaded by culling.
                                        // The remaining word per group holds its ordered index.
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
        if RANGE_HYBRID != 0 {
            return result;
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
        self.order_cutouts(result.blended, result.cutout);
        result
    }

    fn order_cutouts(&mut self, blended_count: u16, count: u16) {
        if count == 0 {
            return;
        }
        // Rank seeding has finished with these two buffers. Reuse them for a
        // fixed texture/stable-ID order, independent of the candidate window.
        let mut mask = unsafe { self.alpha_mask.as_u16_array() };
        let mut ordinals = unsafe { self.alpha_ordinals.as_u16_array() };
        let records = unsafe { self.blended_draws.as_u16_array() };
        let blocks = (GROUP_COUNT - OPAQUE_COUNT + 15) / 16;
        let mut i: u16 = 0;
        while i < blocks {
            mask[i] = 0;
            i += 1;
        }
        i = 0;
        while i < count {
            let ordinal = GROUP_COUNT - OPAQUE_COUNT - 1 - i;
            let id = records[ordinal * 6 + 3] - OPAQUE_COUNT;
            ordinals[id] = ordinal;
            mask[id >> 4] |= 1u16 << (id & 15);
            i += 1;
        }
        let mut output = unsafe { self.blended_order.as_u16_array() };
        let mut cursor = blended_count;
        let mut texture: u16 = 1;
        while texture <= MAP_TEXTURE_COUNT {
            let mut bucket: u16 = 0;
            while bucket < blocks {
                let mut bits = mask[bucket];
                let mut id = bucket * 16;
                while bits != 0 {
                    if bits & 1 != 0 {
                        let ordinal = ordinals[id];
                        let material = records[ordinal * 6 + 2] & 32767;
                        if map_materials()[material * 4] == texture {
                            output[cursor] = ordinal;
                            cursor += 1;
                        }
                    }
                    bits >>= 1;
                    id += 1;
                }
                bucket += 1;
            }
            texture += 1;
        }
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
        gather_ordinals(
            mask,
            ordinals,
            order,
            (GROUP_COUNT - OPAQUE_COUNT + 15) / 16,
        );
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
        let mut bucket: u16 = 0;
        while bucket < OPAQUE_MASK_WORDS {
            let mut bits = mask[bucket];
            let mut item = bucket * 16;
            while bits != 0 {
                if bits & 1 != 0 {
                    output[OPAQUE_COUNT * 3 + count] = item;
                    count += 1;
                }
                bits >>= 1;
                item += 1;
            }
            bucket += 1;
        }
        count
    }
}

fn gather_ordinals(mask: Array<u16>, ordinals: Array<u16>, mut output: Array<u16>, blocks: u16) {
    let mut bucket: u16 = 0;
    let mut index: u16 = 0;
    while bucket < blocks {
        let mut bits = mask[bucket];
        let mut at = bucket * 16;
        while bits != 0 {
            if bits & 1 != 0 {
                output[index] = ordinals[at];
                index += 1;
            }
            at += 1;
            bits >>= 1;
        }
        bucket += 1;
    }
}

// Quantize the horizontal view direction for the conservative candidate mask.
fn pvs_heading(view: &View) -> u16 {
    let x = view.matrix.row2.x;
    let z = view.matrix.row2.z;
    let axes = map_pvs_quadrant();
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
    let thresholds = map_alpha_pitch_thresholds();
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

impl RangeVisibility {
    fn new() -> Self {
        let mut cache = Self {
            anchor: Mat3x4 {
                row0: Vec4::zero(),
                row1: Vec4::zero(),
                row2: Vec4::zero(),
            },
            axes: Ptr::from_addr(0),
            valid: false,
            new_anchor: false,
            chunks: Ptr::from_addr(0),
            all: Ptr::from_addr(0),
            visible: Ptr::from_addr(0),
            fringe: Ptr::from_addr(0),
        };
        if RANGE_INCREMENTAL != 0 {
            cache.axes = malloc(48);
            cache.chunks = malloc(RANGE_CHUNKS * 4);
            cache.all = map_stage::all();
            cache.visible = map_stage::visible();
            cache.fringe = map_stage::fringe();
            let mut chunks = unsafe { cache.chunks.as_u16_array() };
            let mut at: u16 = 0;
            while at < RANGE_CHUNKS * 4 {
                chunks[at + 1] = 0;
                at += 4;
            }
        }
        cache
    }

    fn begin(&mut self, view: &Mat3x4) {
        store_sphere_axes(view, self.axes);
        if self.valid
            && projection_change(&view.row0, &self.anchor.row0) <= MOTION
            && projection_change(&view.row1, &self.anchor.row1) <= MOTION
            && projection_change(&view.row2, &self.anchor.row2) <= MOTION
        {
            self.new_anchor = false;
            return;
        }
        self.anchor = *view;
        store_sphere_axes(view, self.axes.add(24));
        self.valid = true;
        self.new_anchor = true;
        let mut chunks = unsafe { self.chunks.as_u16_array() };
        let mut at: u16 = 0;
        while at < RANGE_CHUNKS * 4 {
            chunks[at + 2] = 65535;
            at += 4;
        }
    }

    fn select(&mut self, chunk: &RingChunk, mut words: Array<u16>, mask: Array<u16>) {
        let first = chunk.first;
        let count = chunk.count;
        let classification = chunk.classification;
        let base = first & !(RING_CHUNK_GROUPS - 1);
        let slot = base / RING_CHUNK_GROUPS * 4;
        let mut chunks = unsafe { self.chunks.as_u16_array() };
        let mut all = unsafe { self.all.as_u16_array() };
        let mut visible = unsafe { self.visible.as_u16_array() };
        let mut fringe = unsafe { self.fringe.as_u16_array() };
        let shape_changed = chunks[slot + 1] != count || chunks[slot] != first;
        let rebuild = classification == 1 && (shape_changed || chunks[slot + 2] == 65535);
        let mut saved = 0u16;
        let mut mapped = false;
        if shape_changed || rebuild {
            let offset = first * RING_GROUP_WORDS;
            let size = count * RING_GROUP_WORDS;
            if gpu_memory_faulted() || (offset >> 6) > 65535 - MAP_RING_GROUP_DATA.first_sector {
                halt(0x7a12);
            }
            let sector = MAP_RING_GROUP_DATA.first_sector + (offset >> 6);
            let at = ((sector & 255) << 6) + (offset & 63);
            // Immutable group records can be read in place. Keep the existing
            // bounded copy for the occasional chunk spanning two windows.
            if size <= 16384 - at {
                saved = read_dseg_page(3);
                let page = sector >> 8;
                if saved != page {
                    unsafe {
                        write_dseg_page(3, page);
                    }
                    mapped = true;
                }
                words = unsafe { Ptr::from_addr(0xc000 + at).as_u16_array() };
            } else if !gpu_window_asset_read(words, MAP_RING_GROUP_DATA.first_sector, offset, size)
            {
                halt(0x7a12);
            }
            let mut i: u16 = 0;
            let mut accepted = base;
            let mut pending = base * 5;
            while i < count {
                let at = i * RING_GROUP_WORDS;
                let unit = words[at + 4];
                all[base + i] = unit;
                if classification == 2 {
                    select_unit(mask, unit);
                } else {
                    let marked = mask[unit >> 4] & (1u16 << (unit & 15)) != 0;
                    // The frame output is an OR per unit. Deferred groups stay
                    // in fringe so another view can still select them later.
                    let mut status = 1u16;
                    if !marked {
                        status = anchor_sphere(self.axes.add(24), words, at);
                    }
                    if status == 3 {
                        visible[accepted] = unit;
                        accepted += 1;
                        select_unit(mask, unit);
                    } else if status != 0 {
                        fringe[pending] = words[at];
                        fringe[pending + 1] = words[at + 1];
                        fringe[pending + 2] = words[at + 2];
                        fringe[pending + 3] = words[at + GROUP_RADIUS_WORD];
                        fringe[pending + 4] = unit;
                        pending += 5;
                        let mut visible_now = marked || (self.new_anchor && status == 2);
                        if !self.new_anchor && !marked {
                            let bound = group_bound(words, at);
                            visible_now = visible_sphere(self.axes, &bound);
                        }
                        if !marked && visible_now {
                            select_unit(mask, unit);
                        }
                    }
                }
                i += 1;
            }
            chunks[slot] = first;
            chunks[slot + 1] = count;
            chunks[slot + 2] = 65535;
            if classification == 1 {
                chunks[slot + 2] = accepted;
                chunks[slot + 3] = pending;
            }
        } else if classification == 2 {
            let mut i = base;
            while i < base + count {
                select_unit(mask, all[i]);
                i += 1;
            }
        } else {
            let mut i = base;
            while i < chunks[slot + 2] {
                select_unit(mask, visible[i]);
                i += 1;
            }
            i = base * 5;
            while i < chunks[slot + 3] {
                let unit = fringe[i + 4];
                if mask[unit >> 4] & (1u16 << (unit & 15)) == 0 {
                    let bound = Vec4 {
                        x: compact_signed(fringe[i]),
                        y: compact_signed(fringe[i + 1]),
                        z: compact_signed(fringe[i + 2]),
                        w: compact_unsigned(fringe[i + 3]),
                    };
                    if visible_sphere(self.axes, &bound) {
                        select_unit(mask, unit);
                    }
                }
                i += 5;
            }
        }
        if mapped {
            unsafe {
                write_dseg_page(3, saved);
            }
        }
    }
}

#[inline]
fn select_unit(mut mask: Array<u16>, unit: u16) {
    mask[unit >> 4] |= 1u16 << (unit & 15);
}

fn projection_change(current: &Vec4, anchor: &Vec4) -> fix32 {
    let x = current.x - anchor.x;
    let y = current.y - anchor.y;
    let z = current.z - anchor.z;
    let midpoint = fdot(
        vec3::new(x, y, z),
        vec3::new(
            fix32::from_int(RANGE_CENTER_X),
            fix32::from_int(RANGE_CENTER_Y),
            fix32::from_int(RANGE_CENTER_Z),
        ),
    ) + current.w
        - anchor.w;
    // Evaluate the linear difference at the bounds' midpoint. The remaining
    // term bounds any group center about it, with a guard for Q16.16 narrowing.
    midpoint.abs()
        + fdot(
            vec3::new(x.abs(), y.abs(), z.abs()),
            vec3::new(
                fix32::from_int(RANGE_EXTENT_X as i16),
                fix32::from_int(RANGE_EXTENT_Y as i16),
                fix32::from_int(RANGE_EXTENT_Z as i16),
            ),
        )
        + fix!(0.000030517578125)
}

fn bounded_translation(translation: fix32) -> bool {
    let integer = translation.hi_bits() as i16;
    integer >= -1024 && integer < 1024
}

fn store_sphere_axes(view: &Mat3x4, target: Ptr) {
    let x = view.row0;
    let y = view.row1;
    let z = view.row2;
    vec4::export(vec4::new(x.x, x.y, x.z, x.w), target);
    vec4::export(vec4::new(y.x, y.y, y.z, y.w), target.add(8));
    vec4::export(vec4::new(z.x, z.y, z.z, z.w), target.add(16));
}

// 0: stable reject, 1: fringe reject, 2: fringe accept, 3: stable accept.
// The side guards round up MOTION * (1 + slope), including fixed-point error.
fn anchor_sphere(axes: Ptr, words: Array<u16>, at: u16) -> u16 {
    // Consume the packed bound directly; the caller only expands a Vec4 when
    // an older anchor also requires a test against the current view.
    let center = vec4::new(
        compact_signed(words[at]),
        compact_signed(words[at + 1]),
        compact_signed(words[at + 2]),
        fix32::one(),
    );
    let bound_radius = compact_unsigned(words[at + GROUP_RADIUS_WORD]);
    let depth = -fdot(vec4::import(axes.add(16)), center);
    let radius = bound_radius + fix!(0.0078125);
    let near = depth + radius;
    let far = DISPLAY_DISTANCE - (depth - bound_radius);
    if near <= fix32::zero() {
        return if near <= -MOTION { 0 } else { 1 };
    }
    if far < fix32::zero() {
        return if far < -MOTION { 0 } else { 1 };
    }
    let x = depth * X_PER_DEPTH + radius * fix!(1.273) - fdot(vec4::import(axes), center).abs();
    if x < fix32::zero() {
        return if x < -MOTION_X { 0 } else { 1 };
    }
    let y =
        depth * Y_PER_DEPTH + radius * fix!(1.107) - fdot(vec4::import(axes.add(8)), center).abs();
    if y < fix32::zero() {
        return if y < -MOTION_Y { 0 } else { 1 };
    }
    if near > MOTION && far >= MOTION && x >= MOTION_X && y >= MOTION_Y {
        return 3;
    }
    2
}

fn visible_sphere(axes: Ptr, bound: &Vec4) -> bool {
    let center = vec4::new(bound.x, bound.y, bound.z, fix32::one());
    let depth = -fdot(vec4::import(axes.add(16)), center);
    let radius = bound.w + fix!(0.0078125);
    if depth + radius <= fix32::zero() || depth - bound.w > DISPLAY_DISTANCE {
        return false;
    }
    if fdot(vec4::import(axes), center).abs() > depth * X_PER_DEPTH + radius * fix!(1.273) {
        return false;
    }
    fdot(vec4::import(axes.add(8)), center).abs() <= depth * Y_PER_DEPTH + radius * fix!(1.107)
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

// This two-shift decode is on every per-group visibility test.
#[inline]
fn compact_unsigned(word: u16) -> fix32 {
    fix32::from_words(word << 9, word >> 7)
}

fn group_bound(words: Array<u16>, at: u16) -> Vec4 {
    Vec4 {
        x: compact_signed(words[at]),
        y: compact_signed(words[at + 1]),
        z: compact_signed(words[at + 2]),
        w: compact_unsigned(words[at + GROUP_RADIUS_WORD]),
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
    if RANGE_HYBRID != 0 {
        let radius = bound.w + fix!(0.0078125);
        if depth + radius <= fix32::zero() || depth - bound.w > DISPLAY_DISTANCE {
            return false;
        }
        if project(&view.matrix.row0, bound).abs() > depth * X_PER_DEPTH + radius * fix!(1.273) {
            return false;
        }
        return project(&view.matrix.row1, bound).abs()
            <= depth * Y_PER_DEPTH + radius * fix!(1.107);
    }
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
    let directory = map_ring_segments();
    let mut best = -fix32::one();
    let mut best_distance = limit;
    let lists = map_route_location_lists();
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
        let cells = map_route_location_cells();
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
