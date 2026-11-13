#![rcc_resource_library(
    gpu_schema,
    gpu_config,
    gpu_arena,
    gpu_record,
    gpu_storage,
    gpu_packet,
    gpu_memcpy,
    gpu_bind,
    gpu_mesh,
    gpu_group,
    gpu_execute,
    gpu_present,
    gpu_api_fault,
    gpu_api_capture,
    gpu_scene_math,
    gpu_model_cache,
    gpu_scene_patch,
    gpu_scene_objects,
    gpu_scene_transforms,
    gpu_ui_grid,
    gpu_ui_packet
)]
//! Explicit recording, submission, retirement and prepared scene resources.
use crate::dsl_rt::*;
mod static_assets;
use static_assets::*;
mod static_buffer;
use static_buffer::*;
mod gpu_api_values;
use gpu_api_values::*;
mod gpu_value_constructors;
use gpu_value_constructors::*;
mod gpu_transform_values;
use gpu_transform_values::*;
mod texture_layout;
use texture_layout::*;
mod surface;
use surface::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_config;
use gpu_config::*;
mod gpu_arena;
use gpu_arena::*;
mod gpu_record;
use gpu_record::*;
mod gpu_storage;
use gpu_storage::*;
mod gpu_packet;
use gpu_packet::*;
mod gpu_bind;
mod gpu_memcpy;
use gpu_bind::*;
mod gpu_mesh;
use gpu_mesh::*;
mod gpu_group;
use gpu_group::*;
mod gpu_execute;
use gpu_execute::*;
mod gpu_present;
use gpu_present::*;
mod gpu_api_fault;
use gpu_api_fault::*;
mod gpu_api_capture;
use gpu_api_capture::*;
mod gpu_scene_patch;
use gpu_scene_patch::*;

/// Positions belong to a complete shared scene binding, not an individual draw.
#[derive(Copy, Clone)]
pub struct SceneBinding {
    slot: u16,
    generation_low: u16,
    generation_high: u16,
}

/// A sealed single-frame target, independent of model/material preparation.
#[derive(Copy, Clone)]
pub struct TargetBinding {
    slot: u16,
    generation_low: u16,
    generation_high: u16,
}

const GPU_KIND_COLOR: u16 = 8;
const GPU_KIND_DEPTH: u16 = 9;
const GPU_KIND_MESH: u16 = 5;
const GPU_KIND_MODEL_CACHE: u16 = 12;

impl CommandMemory {
    pub const fn allocate(command_bytes: usize, push_bytes: usize) -> Self {
        assert!(command_bytes <= 65536 && push_bytes <= 65536);
        let commands = StaticBuffer::new(command_bytes, 128);
        let push = StaticBuffer::new(push_bytes, 128);
        Self::new(
            GpuMemory::from_buffer(commands),
            GpuMemory::from_buffer(push),
        )
    }
}
impl GpuMemory {
    pub const fn from_buffer(buffer: StaticBuffer) -> Self {
        Self {
            first_sector: buffer.first_sector(),
            sector_count: buffer.sector_count(),
        }
    }
    pub const fn from_surface(surface: Rgb565Surface) -> Self {
        Self {
            first_sector: surface.first_sector(),
            sector_count: surface.sector_count(),
        }
    }
    /// The caller preserves the placed external allocation and its access lifetime.
    pub const unsafe fn from_asset(asset: AssetBytes) -> Self {
        assert!(asset.sector_count != 0);
        assert!(asset.sector_count - 1 <= 65535 - asset.first_sector);
        Self {
            first_sector: asset.first_sector,
            sector_count: asset.sector_count,
        }
    }
    /// Caller guarantees real allocation, lifetime, and no illegal aliasing.
    pub unsafe fn from_sectors(first_sector: u16, sector_count: u16) -> Self {
        if !gpu_memory_extent_valid(first_sector, sector_count) {
            gpu_api_fail(1, 1, first_sector, sector_count);
        }
        GpuMemory {
            first_sector: first_sector,
            sector_count: sector_count,
        }
    }
}
impl TextureDesc {
    pub const fn from_memory(memory: GpuMemory, layout: TextureLayout) -> Self {
        debug_assert!(memory.first_sector & 1 == 0);
        debug_assert!(memory.sector_count >= layout.required_sectors());
        Self {
            memory,
            size_log2: layout.size_log2,
            full_mip: layout.full_mip,
            format: layout.format,
        }
    }
    /// Bind immutable placed bytes. Layout validates the complete native footprint.
    pub const unsafe fn from_asset(asset: AssetBytes, layout: TextureLayout) -> Self {
        let required = layout.required_sectors();
        let high = required >> 9;
        let low = (required & 511) << 7;
        debug_assert!(
            asset.byte_len_hi > high || (asset.byte_len_hi == high && asset.byte_len_lo >= low)
        );
        let memory = unsafe { GpuMemory::from_asset(asset) };
        Self::from_memory(memory, layout)
    }
}
impl MeshDesc {
    /// Word offset within the supplied native package, not an absolute address.
    pub const fn from_memory(
        memory: GpuMemory,
        directory_offset_words: u16,
        meshlets: u16,
    ) -> Self {
        debug_assert!(meshlets <= 5460);
        let relative = directory_offset_words >> 6;
        let word = directory_offset_words & 63;
        debug_assert!(relative < memory.sector_count);
        debug_assert!((word + meshlets * 2 + 63) >> 6 <= memory.sector_count - relative);
        Self {
            directory_sector: memory.first_sector + relative,
            directory_word: word,
            meshlet_count: meshlets,
            storage: memory,
        }
    }
    /// Compile-time byte offset supports directories anywhere in a placed package.
    pub const unsafe fn from_asset(
        asset: AssetBytes,
        directory_offset_bytes: usize,
        meshlets: u16,
    ) -> Self {
        let length = ((asset.byte_len_hi as usize) << 16) | asset.byte_len_lo as usize;
        debug_assert!(meshlets <= 5460 && directory_offset_bytes & 1 == 0);
        debug_assert!(directory_offset_bytes < length);
        debug_assert!(directory_offset_bytes + (meshlets as usize) * 4 <= length);
        let memory = unsafe { GpuMemory::from_asset(asset) };
        Self {
            directory_sector: memory.first_sector + (directory_offset_bytes / 128) as u16,
            directory_word: ((directory_offset_bytes % 128) / 2) as u16,
            meshlet_count: meshlets,
            storage: memory,
        }
    }
}
#[derive(Copy, Clone)]
pub struct Rgb565 {
    pub raw: u16,
}
impl Rgb565 {
    pub const fn from_raw(raw: u16) -> Self {
        Rgb565 { raw: raw }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::from_raw((((r as u16) >> 3) << 11) | (((g as u16) >> 2) << 5) | ((b as u16) >> 3))
    }
    pub const fn gray(value: u8) -> Self {
        Self::rgb(value, value, value)
    }
}
#[repr(u16)]
#[derive(Copy, Clone)]
pub enum LoadOp {
    Load,
    Clear,
}
pub struct ColorLoad {
    pub op: LoadOp,
    pub value: Rgb565,
}
impl ColorLoad {
    pub const fn load() -> Self {
        ColorLoad {
            op: LoadOp::Load,
            value: Rgb565 { raw: 0 },
        }
    }
    pub const fn clear(value: Rgb565) -> Self {
        ColorLoad {
            op: LoadOp::Clear,
            value: value,
        }
    }
}
pub struct DepthLoad {
    pub op: LoadOp,
    pub value: u16,
}
impl DepthLoad {
    pub const fn clear_far() -> Self {
        Self::clear(65535)
    }
    pub const fn load() -> Self {
        DepthLoad {
            op: LoadOp::Load,
            value: 0,
        }
    }
    pub const fn clear(value: u16) -> Self {
        DepthLoad {
            op: LoadOp::Clear,
            value: value,
        }
    }
}

#[rcc_resource]
pub struct Gpu {
    identity: u16,
}
#[rcc_resource]
pub struct CommandBuffer {
    slot: u16,
}
#[rcc_resource(parent=CommandBuffer)]
pub struct Pass {
    slot: u16,
}
#[rcc_resource]
pub struct ColorBuffer {
    slot: u16,
}
#[rcc_resource]
pub struct DepthBuffer {
    slot: u16,
}
#[rcc_resource]
pub struct Mesh {
    slot: u16,
}
#[rcc_resource]
pub struct Display {
    identity: u16,
}

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DisplayFormat {
    Rgb565,
    LinearRgb565,
}

impl Gpu {
    pub fn init(config: GpuConfig) -> Self {
        if !gpu_config_init(config) {
            gpu_api_fail(2, 1, 0, 0);
        }
        gpu_snapshot_reset();
        Gpu { identity: 1 }
    }
    pub fn command_buffer(&mut self) -> CommandBuffer {
        let slot = gpu_config_take_buffer();
        if slot == 65535 {
            gpu_api_fail(2, 2, 0, 0);
        }
        CommandBuffer { slot: slot }
    }
    pub fn submit(&mut self, buffer: CommandBuffer) {
        let slot = buffer.slot;
        resource_disarm(buffer);
        gpu_api_submit(slot, false);
    }
    pub fn submit_capturable(&mut self, buffer: CommandBuffer) {
        let slot = buffer.slot;
        resource_disarm(buffer);
        gpu_api_submit(slot, true);
    }
    pub fn request_snapshot(&mut self) {
        gpu_snapshot_request();
    }
    pub fn poll_snapshot(&mut self) {
        gpu_snapshot_poll();
    }
    pub fn try_wait(&mut self) -> Option<CommandBuffer> {
        gpu_snapshot_poll();
        let completed = gpu_execute_poll();
        if completed == 65535 {
            gpu_api_fail(6, 3, GPU_EXECUTION[5u16], GPU_EXECUTION[1u16]);
        }
        if completed == 0 {
            return None;
        }
        let buffer = CommandBuffer {
            slot: completed - 1,
        };
        Some(buffer)
    }
    pub fn wait(&mut self) -> CommandBuffer {
        if GPU_EXECUTION[0u16] == GPU_EXEC_FAULT {
            gpu_api_fail(6, 3, GPU_EXECUTION[5u16], GPU_EXECUTION[1u16]);
        }
        if GPU_EXECUTION[0u16] != GPU_EXEC_RUNNING {
            gpu_api_fail(6, 4, GPU_EXECUTION[0u16], GPU_EXECUTION[1u16]);
        }
        let mut completed = gpu_execute_poll();
        while completed == 0 {
            gpu_snapshot_poll();
            completed = gpu_execute_poll();
        }
        if completed == 65535 {
            gpu_api_fail(6, 3, GPU_EXECUTION[5u16], GPU_EXECUTION[1u16]);
        }
        CommandBuffer {
            slot: completed - 1,
        }
    }
}
fn gpu_api_submit(slot: u16, capturable: bool) {
    gpu_snapshot_poll();
    if !gpu_execute_prepare(slot) {
        gpu_api_fail(6, 1, GPU_EXECUTION[5u16], slot);
    }
    let capture = capturable && gpu_snapshot_take();
    let mut action: u16 = 0;
    if capture {
        let mut source: Buf<u16, 12> = Buf::new([0; 12]);
        if !gpu_execute_capture_source(slot, source.as_array()) {
            gpu_execute_fail(8);
            gpu_api_fail(6, 5, slot, 0);
        }
        if !gpu_capture_begin(source.as_array(), gpu_record_capture_facts(slot)) {
            gpu_execute_fail(9);
            gpu_api_fail(6, 6, slot, gpu_capture_health());
        }
        action = gpu_capture_gate();
        if action > 1 {
            gpu_execute_fail(10);
            gpu_api_fail(6, 7, action, gpu_capture_health());
        }
    }
    if !gpu_execute_start(slot, action == 1) {
        gpu_api_fail(6, 2, GPU_EXECUTION[5u16], slot);
    }
    gpu_capture_started();
    if capture {
        gpu_capture_submitted();
    }
}
impl Drop for Gpu {
    // No implicit wait, context reset or reclamation of a driver-owned CB.
    fn drop(&mut self) {}
}
fn gpu_api_new_surface(memory: GpuMemory, kind: u16) -> u16 {
    let slot = gpu_arena_alloc(false, kind);
    if slot == 0 {
        gpu_api_fail(3, 1, kind, 0);
    }
    if !gpu_surface_init(gpu_arena_data(slot), memory) {
        gpu_api_fail(3, 2, memory.first_sector(), memory.sector_count());
    }
    slot
}
impl ColorBuffer {
    pub fn from_surface(surface: Rgb565Surface) -> Self {
        Self::from_memory(GpuMemory::from_surface(surface))
    }
    pub fn from_memory(memory: GpuMemory) -> Self {
        ColorBuffer {
            slot: gpu_api_new_surface(memory, GPU_KIND_COLOR),
        }
    }
}
impl DepthBuffer {
    pub fn from_memory(memory: GpuMemory) -> Self {
        DepthBuffer {
            slot: gpu_api_new_surface(memory, GPU_KIND_DEPTH),
        }
    }
}
impl Drop for ColorBuffer {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GPU_KIND_COLOR) {
            gpu_api_fail(3, 3, GPU_KIND_COLOR, self.slot);
        }
    }
}
impl Drop for DepthBuffer {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GPU_KIND_DEPTH) {
            gpu_api_fail(3, 3, GPU_KIND_DEPTH, self.slot);
        }
    }
}
impl Mesh {
    pub fn from_native(desc: MeshDesc) -> Self {
        let slot = gpu_arena_alloc(false, GPU_KIND_MESH);
        if slot == 0 {
            gpu_api_fail(8, 1, 0, 0);
        }
        if !gpu_mesh_init(gpu_arena_data(slot), desc) {
            gpu_api_fail(8, 2, 0, 0);
        }
        Mesh { slot: slot }
    }
    pub fn subset(&self, first: u16, count: u16) -> Mesh {
        let slot = gpu_arena_alloc(false, GPU_KIND_MESH);
        if slot == 0 {
            gpu_api_fail(8, 1, 0, 0);
        }
        if !gpu_mesh_subset(
            gpu_arena_data(slot),
            gpu_arena_data(self.slot),
            first,
            count,
        ) {
            gpu_api_fail(8, 3, first, count);
        }
        Mesh { slot: slot }
    }
    pub fn meshlet_count(&self) -> u16 {
        gpu_arena_data(self.slot)[GPU_MESH_COUNT]
    }
}
impl Drop for Mesh {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, GPU_KIND_MESH) {
            gpu_api_fail(8, 4, self.slot, 0);
        }
    }
}
fn gpu_api_copy_plane(slot: u16, source: u16, destination: u16, kind: u16) {
    let s = gpu_arena_data(source);
    let d = gpu_arena_data(destination);
    if gpu_record_overlap(s[0u16], s[1u16], d[0u16], d[1u16]) {
        gpu_api_fail(4, 2, slot, 0);
    }
    let facts: Buf<u16, 8> = Buf::new([
        s[0u16],
        s[1u16],
        kind,
        GPU_FACT_READ,
        d[0u16],
        d[1u16],
        kind,
        GPU_FACT_WRITE,
    ]);
    if !gpu_record_capture_room(slot, facts.as_array(), 2) {
        gpu_api_fail(4, 6, slot, 0);
    }
    if !gpu_packet_copy_plane(slot, s[0u16], d[0u16]) {
        gpu_api_fail(4, 2, slot, 0);
    }
    gpu_record_capture_commit(slot, facts.as_array(), 2);
}
/// Push partition choices; command, metadata and total push budgets are unchanged.
#[repr(u16)]
#[derive(Copy, Clone)]
pub enum PreparedPartsLayout {
    SingleWide,
    SplitWide,
}
impl CommandBuffer {
    /// Reserve a slot-private 16 KiB template and 16 KiB active command area
    /// inside this CB's existing aligned 64 KiB command allocation.
    pub fn enable_prepared_group(&mut self) {
        gpu_api_checked(gpu_group_enable(self.slot), 15);
    }
    /// SplitWide gives parts0/15 4096 words each; parts1..14 retain1024.
    /// SingleWide preserves the original8192-word part0 and1024-word tail.
    pub fn enable_prepared_parts(&mut self, layout: PreparedPartsLayout) {
        gpu_api_checked(gpu_group_enable_layout(self.slot, layout as u16), 15);
    }
    /// Clear all saved parts and reset the active stream, retaining partitions.
    pub fn clear_prepared_group(&mut self) {
        gpu_api_checked(gpu_group_clear(self.slot), 15);
    }
    /// Invalidate one part between recordings, retaining every other part.
    pub fn invalidate_prepared_part(&mut self, part: u16) {
        gpu_api_checked(gpu_group_invalidate(self.slot, part), 15);
    }
    /// Copy the 400x256 physical RGB565 plane between nonoverlapping surfaces.
    /// Records source-read/destination-write ownership; allocation tails remain
    /// untouched. Call between passes. Submission publication/leases are shared
    /// with drawing, and this fixed-target command disables scene patching.
    pub fn copy_color(&mut self, source: &ColorBuffer, destination: &ColorBuffer) {
        gpu_api_copy_plane(self.slot, source.slot, destination.slot, GPU_FACT_COLOR);
    }
    /// Copy the physical depth plane without changing its stored values.
    /// The caller retains a completed source until this CB retires.
    pub fn copy_depth(&mut self, source: &DepthBuffer, destination: &DepthBuffer) {
        gpu_api_copy_plane(self.slot, source.slot, destination.slot, GPU_FACT_DEPTH);
    }
    /// Seal one target block, with any immutable draw groups and at most one
    /// leading color copy. No matrices, state or source objects are patched.
    pub fn target_binding(&mut self) -> TargetBinding {
        gpu_api_checked(gpu_target_binding(self.slot), 14);
        let c = gpu_record_data(self.slot);
        TargetBinding {
            slot: self.slot,
            generation_low: c[GPU_RECORD_GENERATION_LOW],
            generation_high: c[GPU_RECORD_GENERATION_HIGH],
        }
    }
    /// Change a CPU-owned frame's target and optional copy destination together.
    /// A pending Display swap or overlap with its current Front rejects before
    /// any payload/fact change. Submission still publishes and validates leases.
    pub fn retarget_target(
        &mut self,
        binding: TargetBinding,
        color: &ColorBuffer,
        depth: &DepthBuffer,
    ) {
        gpu_api_checked(
            self.slot == binding.slot
                && gpu_target_matches(self.slot, binding.generation_low, binding.generation_high),
            14,
        );
        gpu_api_checked(
            gpu_target_retarget(
                self.slot,
                gpu_arena_data(color.slot),
                gpu_arena_data(depth.slot),
            ),
            14,
        );
    }
    pub fn scene_binding(&mut self) -> SceneBinding {
        gpu_api_checked(gpu_patch_binding(self.slot), 13);
        let c = gpu_record_data(self.slot);
        SceneBinding {
            slot: self.slot,
            generation_low: c[GPU_RECORD_GENERATION_LOW],
            generation_high: c[GPU_RECORD_GENERATION_HIGH],
        }
    }
    pub fn patch_scene(
        &mut self,
        binding: SceneBinding,
        camera: &Camera,
        model: &Model,
        color: &ColorBuffer,
        depth: &DepthBuffer,
    ) {
        gpu_api_checked(
            self.slot == binding.slot
                && gpu_patch_matches(self.slot, binding.generation_low, binding.generation_high),
            13,
        );
        gpu_api_checked(
            gpu_patch_scene(
                self.slot,
                gpu_arena_data(camera.slot),
                gpu_arena_data(model.slot),
                gpu_arena_data(color.slot),
                gpu_arena_data(depth.slot),
            ),
            13,
        );
    }
    pub fn reset(&mut self) {
        if !gpu_record_reset(self.slot) {
            gpu_api_fail(4, 1, self.slot, 0);
        }
    }
    pub fn pass(
        &mut self,
        color: &ColorBuffer,
        depth: &DepthBuffer,
        color_load: ColorLoad,
        depth_load: DepthLoad,
    ) -> Pass {
        let c = gpu_arena_data(color.slot);
        let d = gpu_arena_data(depth.slot);
        let mut mask: u16 = 0;
        if color_load.op as u16 == 1 {
            mask |= 1;
        }
        if depth_load.op as u16 == 1 {
            mask |= 2;
        }
        let facts: Buf<u16, 8> = Buf::new([
            c[0u16],
            c[1u16],
            GPU_FACT_COLOR,
            GPU_FACT_READ | GPU_FACT_WRITE,
            d[0u16],
            d[1u16],
            GPU_FACT_DEPTH,
            GPU_FACT_READ | GPU_FACT_WRITE,
        ]);
        if !gpu_record_capture_room(self.slot, facts.as_array(), 2) {
            gpu_api_fail(4, 6, self.slot, 0);
        }
        if !gpu_packet_begin(
            self.slot,
            c[0u16],
            d[0u16],
            mask,
            color_load.value.raw,
            depth_load.value,
        ) {
            gpu_api_fail(4, 2, self.slot, mask);
        }
        gpu_record_capture_commit(self.slot, facts.as_array(), 2);
        Pass { slot: self.slot }
    }
    pub fn invalidate_textures(&mut self) {
        if !gpu_packet_invalidate_textures(self.slot) {
            gpu_api_fail(4, 3, self.slot, 0);
        }
    }
    pub fn command_words(&self) -> u16 {
        gpu_record_data(self.slot)[GPU_RECORD_COMMAND_USED]
    }
    pub fn push_words(&self) -> u16 {
        gpu_record_data(self.slot)[GPU_RECORD_PUSH_USED]
    }
}
impl Drop for CommandBuffer {
    fn drop(&mut self) {
        if !gpu_config_drop_buffer(self.slot) {
            gpu_api_fail(4, 4, self.slot, 0);
        }
    }
}
impl Pass {
    /// Test whether this slot has a completed part in the range 0..16.
    pub fn prepared_part_ready(&self, part: u16) -> bool {
        gpu_group_part_ready(self.slot, part)
    }
    /// Record or replace a CPU-owned part not yet referenced by this recording.
    pub fn begin_prepared_part(&mut self, part: u16) {
        gpu_api_checked(gpu_group_part_begin(self.slot, part), 15);
    }
    /// Append a saved part; reinstall camera/state before subsequent draws.
    pub fn append_prepared_part(&mut self, part: u16) {
        gpu_api_checked(gpu_group_part_append(self.slot, part), 15);
    }
    pub fn prepared_group_ready(&self) -> bool {
        gpu_group_ready(self.slot)
    }
    /// Record only mesh draws and their camera/state choices until end.
    /// Native parameter snapshots persist; source objects need not persist.
    pub fn begin_prepared_group(&mut self) {
        gpu_api_checked(gpu_group_begin(self.slot), 15);
    }
    /// Finish the open part. Reinstall camera and state before drawing.
    pub fn end_prepared_group(&mut self) {
        gpu_api_checked(gpu_group_end(self.slot), 15);
    }
    /// Append via LCOPY without scene math, parameter uploads or mesh expansion.
    /// Reinstall camera and state before subsequent immediate draws.
    pub fn append_prepared_group(&mut self) {
        gpu_api_checked(gpu_group_append(self.slot), 15);
    }
    pub fn invalidate_textures(&mut self) {
        gpu_api_checked(gpu_record_data(self.slot)[GPU_GROUP_RECORDING] == 0, 15);
        if !gpu_packet_invalidate_textures(self.slot) {
            gpu_api_fail(4, 3, self.slot, 0);
        }
    }
}
impl Drop for Pass {
    fn drop(&mut self) {
        if !gpu_record_close_pass(self.slot) {
            gpu_api_fail(4, 5, self.slot, 0);
        }
    }
}
impl Display {
    pub fn open() -> Self {
        Self::open_with_format(DisplayFormat::Rgb565)
    }
    pub fn open_with_format(format: DisplayFormat) -> Self {
        if !gpu_present_open_with_format(format as u16) {
            gpu_api_fail(5, 1, 0, 0);
        }
        Display { identity: 1 }
    }
    pub fn frame_index(&self) -> u16 {
        dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX)
    }
    pub fn set_next_swap(&mut self, color: &ColorBuffer) {
        if !gpu_present_set_next(gpu_arena_data(color.slot)[0u16]) {
            gpu_api_fail(5, 2, 0, 0);
        }
    }
    /// Stage the pixel interpretation together with its completed framebuffer.
    pub fn set_next_swap_with_format(&mut self, color: &ColorBuffer, format: DisplayFormat) {
        if !gpu_present_set_next_with_format(gpu_arena_data(color.slot)[0u16], format as u16) {
            gpu_api_fail(5, 2, 0, 0);
        }
    }
    pub fn wait_vblank(&mut self) {
        if !gpu_present_wait_vblank() {
            gpu_api_fail(5, 3, 0, 0);
        }
    }
    // One observation of the queued swap, including old-read retirement.
    // With no owned request this reports stable/idle, not a new frame edge.
    pub fn try_wait_swap(&mut self) -> bool {
        let status = gpu_present_poll();
        if status == 65535 {
            gpu_api_fail(5, 3, 0, 0);
        }
        status != 1
    }
}
impl Drop for Display {
    fn drop(&mut self) {}
}

// Read-only capture consumer bridge; this never owns or submits a buffer.
pub fn gpu_capture_error() -> u16 {
    gpu_capture_health()
}
pub fn gpu_capture_range_word(range: u16, field: u16) -> u16 {
    gpu_capture_range_value(range, field)
}
pub fn gpu_capture_read_word(range: u16, low: u16, high: u16) -> u16 {
    gpu_capture_read_value(range, low, high)
}

mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;
mod gpu_scene_transforms;
use gpu_scene_transforms::*;
mod gpu_model_cache;
use gpu_model_cache::*;
mod gpu_ui_grid;
use gpu_ui_grid::*;
mod gpu_ui_packet;
use gpu_ui_packet::*;

fn gpu_api_object(large: bool, kind: u16) -> u16 {
    let slot = gpu_arena_alloc(large, kind);
    if slot == 0 {
        gpu_api_fail(7, 1, kind, 0);
    }
    slot
}
fn gpu_api_checked(ok: bool, kind: u16) {
    if !ok {
        gpu_api_fail(7, 2, kind, 0);
    }
}
impl Projection {
    pub const fn from_focal_pixels(focal: fix32, far: fix32) -> Self {
        let result: ProjectionResult = gpu_scene_projection_focal_desc(focal, far);
        assert!(result.valid);
        result.value
    }
    pub const fn from_vertical_fov(angle: fix32, far: fix32) -> Self {
        let result: ProjectionResult = gpu_scene_projection_fov_desc(angle, far, false);
        assert!(result.valid);
        result.value
    }
    pub const fn from_horizontal_fov(angle: fix32, far: fix32) -> Self {
        let result: ProjectionResult = gpu_scene_projection_fov_desc(angle, far, true);
        assert!(result.valid);
        result.value
    }
}

#[rcc_resource]
pub struct Camera {
    slot: u16,
}
impl Camera {
    pub fn new(desc: CameraDesc) -> Self {
        let slot = gpu_api_object(true, 1);
        gpu_api_checked(gpu_scene_camera_init(gpu_arena_data(slot), desc), 1);
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(slot)), 1);
        Camera { slot: slot }
    }
}
impl Drop for Camera {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, true, 1) {
            gpu_api_fail(7, 3, 1, self.slot);
        }
        gpu_patch_drop(GPU_RECORD_SCENE_CAMERA, self.slot);
    }
}

#[rcc_resource]
pub struct Model {
    slot: u16,
}
impl Model {
    pub fn new(transform: Transform) -> Self {
        let slot = gpu_api_object(true, 2);
        gpu_api_checked(gpu_scene_model_init(gpu_arena_data(slot), transform), 2);
        gpu_api_finish_model(slot)
    }
    pub fn from_matrices(world: Mat3x4, normal: Mat3x3) -> Self {
        let slot = gpu_api_object(true, 2);
        gpu_api_checked(
            gpu_scene_model_matrices(gpu_arena_data(slot), world, normal),
            2,
        );
        gpu_api_finish_model(slot)
    }
}
fn gpu_api_finish_model(slot: u16) -> Model {
    let derived = gpu_arena_alloc(true, GPU_KIND_MODEL_CACHE);
    if derived == 0 {
        gpu_api_checked(gpu_arena_free(slot, true, 2), 2);
        gpu_api_fail(7, 1, 2, 0);
    }
    gpu_model_cache_init(gpu_arena_data(derived));
    gpu_arena_data(slot)[GPU_MODEL_DERIVED] = derived;
    Model { slot: slot }
}
impl Drop for Model {
    fn drop(&mut self) {
        let derived = gpu_arena_data(self.slot)[GPU_MODEL_DERIVED];
        if !gpu_arena_free(derived, true, GPU_KIND_MODEL_CACHE) {
            gpu_api_fail(7, 3, GPU_KIND_MODEL_CACHE, derived);
        }
        if !gpu_arena_free(self.slot, true, 2) {
            gpu_api_fail(7, 3, 2, self.slot);
        }
        gpu_patch_drop(GPU_RECORD_SCENE_MODEL, self.slot);
    }
}

#[rcc_resource]
pub struct Material {
    slot: u16,
}
impl Material {
    pub fn new(desc: MaterialDesc) -> Self {
        let slot = gpu_api_object(false, 3);
        gpu_api_checked(gpu_scene_material_init(gpu_arena_data(slot), desc), 3);
        Material { slot: slot }
    }
}
impl Drop for Material {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, 3) {
            gpu_api_fail(7, 3, 3, self.slot);
        }
        gpu_patch_drop(GPU_RECORD_SCENE_MATERIAL, self.slot);
    }
}

#[rcc_resource]
pub struct RenderState {
    slot: u16,
}
impl RenderState {
    pub fn new(desc: RenderStateDesc) -> Self {
        let slot = gpu_api_object(false, 4);
        gpu_api_checked(gpu_scene_state_init(gpu_arena_data(slot), desc), 4);
        RenderState { slot: slot }
    }
}
impl Drop for RenderState {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, 4) {
            gpu_api_fail(7, 3, 4, self.slot);
        }
    }
}

#[rcc_resource]
pub struct Texture {
    slot: u16,
}
impl Texture {
    pub fn from_native(desc: TextureDesc) -> Self {
        let slot = gpu_api_object(false, 6);
        gpu_api_checked(gpu_scene_texture_init(gpu_arena_data(slot), desc), 6);
        Texture { slot: slot }
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, 6) {
            gpu_api_fail(7, 3, 6, self.slot);
        }
    }
}

#[derive(Copy, Clone)]
pub struct UiAtlasDesc {
    pub memory: GpuMemory,
    pub glyph_count: u16,
}
impl UiAtlasDesc {
    /// Caller guarantees immutable, exclusive backing storage for the atlas.
    /// Native records occupy 256 bytes; their base is 64-KiB aligned.
    pub const unsafe fn from_asset(asset: AssetBytes) -> Self {
        debug_assert!(asset.first_sector & 511 == 0);
        debug_assert!(asset.byte_len_hi == 0);
        debug_assert!(asset.byte_len_lo != 0 && asset.byte_len_lo & 255 == 0);
        Self {
            memory: unsafe { GpuMemory::from_asset(asset) },
            glyph_count: asset.byte_len_lo / 256,
        }
    }
}

#[rcc_resource]
pub struct UiAtlas {
    slot: u16,
}
impl UiAtlas {
    /// Caller owns the immutable external allocation for the atlas's lifetime.
    pub fn from_desc(desc: UiAtlasDesc) -> Self {
        Self::from_native(desc.memory, desc.glyph_count)
    }
    pub fn from_native(memory: GpuMemory, glyph_count: u16) -> Self {
        let slot = gpu_api_object(false, 10);
        gpu_api_checked(
            gpu_ui_atlas_init(gpu_arena_data(slot), memory, glyph_count),
            10,
        );
        UiAtlas { slot: slot }
    }
}
impl Drop for UiAtlas {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, 10) {
            gpu_api_fail(7, 3, 10, self.slot);
        }
    }
}

#[rcc_resource]
pub struct UiGrid {
    slot: u16,
}
impl UiGrid {
    pub fn clear(&mut self) {
        self.fill(255);
    }
    pub fn from_memory(memory: GpuMemory, atlas: &UiAtlas) -> Self {
        let slot = gpu_api_object(false, 11);
        gpu_api_checked(
            gpu_ui_grid_init(gpu_arena_data(slot), memory, gpu_arena_data(atlas.slot)),
            11,
        );
        UiGrid { slot: slot }
    }
}
impl Drop for UiGrid {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, 11) {
            gpu_api_fail(7, 3, 11, self.slot);
        }
    }
}

#[rcc_resource]
pub struct TextureView {
    slot: u16,
}
impl Drop for TextureView {
    fn drop(&mut self) {
        if !gpu_arena_free(self.slot, false, 7) {
            gpu_api_fail(7, 3, 7, self.slot);
        }
    }
}
impl TextureView {
    pub fn set_address_mode(&mut self, mode: TextureAddressMode) {
        let mut data = gpu_arena_data(self.slot);
        gpu_api_checked(data[GPU_VIEW_FLAGS] & 2 == 0, 7);
        data[GPU_VIEW_FLAGS] = (data[GPU_VIEW_FLAGS] & 7) | ((mode as u16) << 3);
    }
}
impl Texture {
    pub fn view(&self) -> TextureView {
        let slot = gpu_api_object(false, 7);
        gpu_api_checked(
            gpu_scene_texture_view(gpu_arena_data(slot), gpu_arena_data(self.slot)),
            7,
        );
        TextureView { slot: slot }
    }
}
impl ColorBuffer {
    pub fn texture_view(&self) -> TextureView {
        let source = gpu_arena_data(self.slot);
        let memory = unsafe { GpuMemory::from_sectors(source[0u16], source[1u16]) };
        let slot = gpu_api_object(false, 7);
        gpu_api_checked(gpu_scene_color_view(gpu_arena_data(slot), memory), 7);
        TextureView { slot: slot }
    }
}
impl TextureView {
    pub fn width(&self) -> u16 {
        gpu_scene_view_width(gpu_arena_data(self.slot))
    }
    pub fn height(&self) -> u16 {
        gpu_scene_view_height(gpu_arena_data(self.slot))
    }
}
impl Camera {
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3, projection: Projection) -> Self {
        let slot = gpu_api_object(true, 1);
        gpu_api_checked(
            gpu_scene_camera_init_look_at(gpu_arena_data(slot), eye, target, up, projection),
            1,
        );
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(slot)), 1);
        Camera { slot: slot }
    }
    pub fn view(&self) -> Mat3x4 {
        gpu_scene_camera_get_view(gpu_arena_data(self.slot))
    }
}
impl Model {
    pub fn world(&self) -> Mat3x4 {
        gpu_scene_model_world(gpu_arena_data(self.slot))
    }
    pub fn normal(&self) -> Mat3x3 {
        gpu_scene_model_normal(gpu_arena_data(self.slot))
    }
}

impl Camera {
    pub fn set_pose(&mut self, eye: Vec3, angles: Vec3) {
        gpu_api_checked(
            gpu_scene_camera_pose(gpu_arena_data(self.slot), eye, angles),
            1,
        );
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(self.slot)), 1);
    }
}

impl Camera {
    pub fn set_projection(&mut self, projection: Projection) {
        let data = gpu_arena_data(self.slot);
        let focal = projection.focal_pixels();
        let changed = data[GPU_CAMERA_FOCAL] != focal.lo_bits()
            || data[GPU_CAMERA_FOCAL + 1] != focal.hi_bits();
        gpu_api_checked(gpu_scene_camera_projection(data, projection), 1);
        if changed {
            gpu_api_checked(gpu_camera_revision(data), 1);
        }
    }
}

impl Camera {
    pub fn set_look_at(&mut self, eye: Vec3, target: Vec3, up: Vec3) {
        gpu_api_checked(
            gpu_scene_camera_look_at(gpu_arena_data(self.slot), eye, target, up),
            1,
        );
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(self.slot)), 1);
    }
}

impl Camera {
    pub fn set_look_at_y_up(&mut self, eye: Vec3, target: Vec3) {
        gpu_api_checked(
            gpu_scene_camera_look_at_y_up(gpu_arena_data(self.slot), eye, target),
            1,
        );
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(self.slot)), 1);
    }
}

impl Camera {
    pub fn set_look_to(&mut self, eye: Vec3, direction: Vec3, up: Vec3) {
        gpu_api_checked(
            gpu_scene_camera_look_to(gpu_arena_data(self.slot), eye, direction, up),
            1,
        );
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(self.slot)), 1);
    }
}

impl Camera {
    pub fn set_view(&mut self, view: Mat3x4) {
        gpu_api_checked(gpu_scene_camera_view(gpu_arena_data(self.slot), view), 1);
        gpu_api_checked(gpu_camera_revision(gpu_arena_data(self.slot)), 1);
    }
}

impl Model {
    pub fn set_pose(&mut self, transform: Transform) {
        let mut data = gpu_arena_data(self.slot);
        let derived = data[GPU_MODEL_DERIVED];
        gpu_api_checked(gpu_scene_model_init(data, transform), 2);
        data[GPU_MODEL_DERIVED] = derived;
        gpu_arena_data(derived)[GPU_MODEL_DIRTY] = 1;
    }
}

impl Model {
    pub fn set_matrices(&mut self, mut world: Mat3x4, mut normal: Mat3x3) {
        let mut data = gpu_arena_data(self.slot);
        let world_words = unsafe { view_of(&mut world).as_ptr().as_u16_array() };
        let normal_words = unsafe { view_of(&mut normal).as_ptr().as_u16_array() };
        if gpu_bind_equal(data, GPU_MODEL_WORLD, world_words, 0, 24)
            && gpu_bind_equal(data, GPU_MODEL_NORMAL, normal_words, 0, 18)
        {
            return;
        }
        let derived = data[GPU_MODEL_DERIVED];
        gpu_api_checked(gpu_scene_model_matrices(data, world, normal), 2);
        data[GPU_MODEL_DERIVED] = derived;
        gpu_arena_data(derived)[GPU_MODEL_DIRTY] = 1;
    }
}

impl Material {
    pub fn set_uniform_color(&mut self, color: Rgba8) {
        gpu_api_checked(
            gpu_scene_material_color(gpu_arena_data(self.slot), color),
            3,
        );
    }
}

impl Material {
    pub fn set_specular(&mut self, color: Rgb8, shininess: Shininess) {
        gpu_api_checked(
            gpu_scene_material_specular(gpu_arena_data(self.slot), color, shininess),
            3,
        );
    }
}

impl Material {
    pub fn set_response(&mut self, ambient: fix32, directional: fix32) {
        gpu_api_checked(
            gpu_scene_material_response(gpu_arena_data(self.slot), ambient, directional),
            3,
        );
    }
}

impl Material {
    pub fn set_unlit(&mut self, unlit: bool) {
        gpu_api_checked(
            gpu_scene_material_unlit(gpu_arena_data(self.slot), unlit),
            3,
        );
    }
}

impl RenderState {
    pub fn set_depth(&mut self, func: DepthFunc, write: bool) {
        gpu_api_checked(
            gpu_scene_state_depth(gpu_arena_data(self.slot), func, write),
            4,
        );
    }
}

impl RenderState {
    pub fn set_blend(&mut self, mode: BlendMode) {
        gpu_api_checked(gpu_scene_state_blend(gpu_arena_data(self.slot), mode), 4);
    }
}

impl RenderState {
    pub fn set_filter(&mut self, filter: TextureFilter) {
        gpu_api_checked(gpu_scene_state_filter(gpu_arena_data(self.slot), filter), 4);
    }
}

impl RenderState {
    pub fn set_lighting(&mut self, light: Lighting) {
        gpu_api_checked(
            gpu_scene_state_lighting(gpu_arena_data(self.slot), light),
            4,
        );
    }
}

impl UiGrid {
    pub fn fill(&mut self, glyph: u8) {
        gpu_api_checked(gpu_ui_grid_fill(gpu_arena_data(self.slot), glyph), 11);
    }
}

impl UiGrid {
    /// Write one row without repeating its length in a CellRect.
    pub fn write_text(&mut self, x: u16, y: u16, glyphs: &[u8]) {
        self.write(
            CellRect {
                x: x,
                y: y,
                width: glyphs.len() as u16,
                height: 1,
            },
            glyphs,
        );
    }
    /// Replace exactly one cell, preserving the adjacent packed byte.
    pub fn write_glyph(&mut self, x: u16, y: u16, glyph: u8) {
        gpu_api_checked(
            gpu_ui_grid_write_glyph(gpu_arena_data(self.slot), x, y, glyph),
            11,
        );
    }
    pub fn write(&mut self, rect: CellRect, glyphs: &[u8]) {
        gpu_api_checked(
            gpu_ui_grid_write(gpu_arena_data(self.slot), rect, glyphs),
            11,
        );
    }
}

impl Pass {
    pub fn set_camera(&mut self, camera: &Camera) {
        let data = gpu_arena_data(camera.slot);
        gpu_api_checked(gpu_bind_desired(self.slot, true, data), 1);
        gpu_patch_note(self.slot, GPU_RECORD_SCENE_CAMERA, camera.slot);
        gpu_bind_copy(
            gpu_record_data(self.slot),
            GPU_CB_CAMERA_REVISION,
            data,
            GPU_CAMERA_REVISION,
            4,
        );
    }
    pub fn set_state(&mut self, state: &RenderState) {
        gpu_api_checked(
            gpu_bind_desired(self.slot, false, gpu_arena_data(state.slot)),
            4,
        );
    }
    pub fn draw(&mut self, mesh: &Mesh, model: &Model, material: &Material, view: &TextureView) {
        gpu_api_draw(
            self.slot,
            mesh.slot,
            model.slot,
            material.slot,
            view.slot,
            true,
        );
    }
    pub fn draw_colored(&mut self, mesh: &Mesh, model: &Model, material: &Material) {
        gpu_api_draw(self.slot, mesh.slot, model.slot, material.slot, 0, false);
    }
    pub fn draw_ui(&mut self, grid: &UiGrid, clip: CellRect) {
        gpu_api_checked(gpu_record_data(self.slot)[GPU_GROUP_RECORDING] == 0, 15);
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        let data = gpu_arena_data(grid.slot);
        let facts: Buf<u16, 8> = Buf::new([
            data[GPU_UI_GRID_BASE],
            data[GPU_UI_GRID_COUNT],
            GPU_FACT_GRID,
            GPU_FACT_READ,
            data[GPU_UI_GRID_ATLAS_BASE],
            data[GPU_UI_GRID_ATLAS_COUNT],
            GPU_FACT_ATLAS,
            GPU_FACT_READ,
        ]);
        gpu_api_checked(gpu_record_capture_room(self.slot, facts.as_array(), 2), 11);
        let native = unsafe {
            gpu_native_scratch()
                .as_ptr()
                .add(GPU_SCRATCH_UI as i16)
                .as_u16_array()
        };
        gpu_api_checked(gpu_ui_descriptor_prepare(data, native), 11);
        gpu_api_checked(
            gpu_ui_packet_draw(self.slot, native, clip.x, clip.y, clip.width, clip.height),
            11,
        );
        gpu_record_capture_commit(self.slot, facts.as_array(), 2);
    }
}
fn gpu_api_draw(
    slot: u16,
    mesh_slot: u16,
    model_slot: u16,
    material_slot: u16,
    view_slot: u16,
    textured: bool,
) {
    let mesh = gpu_arena_data(mesh_slot);
    if mesh[GPU_MESH_COUNT] == 0 {
        return;
    }
    let c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS || c[GPU_RECORD_VALID] & 24 != 24 {
        gpu_api_fail(9, 1, slot, 0);
    }
    let mut facts: Buf<u16, 8> = Buf::new([
        mesh[GPU_MESH_STORAGE],
        mesh[GPU_MESH_STORAGE_COUNT],
        GPU_FACT_MESH,
        GPU_FACT_READ,
        0,
        0,
        0,
        0,
    ]);
    let mut count: u16 = 1;
    let native = gpu_native_scratch();
    let mut view = native;
    if textured {
        view = gpu_arena_data(view_slot);
        facts[4u16] = view[GPU_VIEW_BASE];
        facts[5u16] = view[GPU_VIEW_COUNT];
        facts[6u16] = GPU_FACT_TEXTURE;
        facts[7u16] = GPU_FACT_READ;
        count = 2;
    }
    gpu_api_checked(gpu_record_capture_room(slot, facts.as_array(), count), 5);
    let desired = unsafe { c.as_ptr().add(GPU_CB_CAMERA as i16).as_u16_array() };
    let material = gpu_arena_data(material_slot);
    gpu_api_checked(
        gpu_scene_draw_header(desired, material, view, textured, native),
        5,
    );
    let model = gpu_arena_data(model_slot);
    let revision = unsafe { c.as_ptr().add(GPU_CB_CAMERA_REVISION as i16).as_u16_array() };
    gpu_model_matrix_prepare(
        desired,
        model,
        native,
        gpu_arena_data(model[GPU_MODEL_DERIVED]),
        revision,
    );
    gpu_scene_draw_finish(material, native);
    gpu_api_checked(gpu_mesh_draw(slot, mesh, native), 5);

    gpu_record_capture_commit(slot, facts.as_array(), count);
    gpu_patch_note(slot, GPU_RECORD_SCENE_MODEL, model_slot);
    gpu_patch_note(slot, GPU_RECORD_SCENE_MATERIAL, material_slot);
}

impl Shininess {
    pub fn nearest(exponent: u16) -> Self {
        gpu_scene_shininess_nearest(exponent)
    }
    pub fn exponent(&self) -> u16 {
        gpu_scene_shininess_exponent(*self)
    }
}
